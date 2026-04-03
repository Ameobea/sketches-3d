import * as THREE from 'three';
import { mount, unmount } from 'svelte';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import { TransformControls } from 'three/examples/jsm/controls/TransformControls.js';

import type { Viz } from 'src/viz';
import type { BulletPhysics } from 'src/viz/collision';
import type { LevelDef, ObjectDef, ObjectGroupDef } from './types';
import type { LevelGroup, LevelObject, LevelSceneNode } from './levelSceneTypes';
import { isLevelGroup } from './levelSceneTypes';
import { LEVEL_PLACEHOLDER_MAT, assignMaterial, instantiateLevelObject } from './levelObjectUtils';
import LevelEditorPanel from './LevelEditorPanel.svelte';
import { LevelEditorApi } from './levelEditorApi';
import { UndoSystem } from './undoSystem';
import { MaterialEditorController } from './materialEditorController';
import { CsgEditController } from './csgEditController.svelte';
import { focusCamera } from '../util/focusCamera';
import { clearPhysicsBinding } from '../util/physics';
import { withWorldSpaceTransform } from '../util/three';

type TransformMode = 'translate' | 'rotate' | 'scale';

export interface TransformSnapshot {
  position: [number, number, number];
  rotation: [number, number, number];
  scale: [number, number, number];
}

type UndoEntry =
  | { type: 'transform'; levelObj: LevelSceneNode; before: TransformSnapshot; after: TransformSnapshot }
  | { type: 'add'; levelObj: LevelObject; snapshot: TransformSnapshot }
  | { type: 'delete'; levelObj: LevelObject; snapshot: TransformSnapshot };

/**
 * Captures the relative change of a transform operation so it can be replayed
 * on a different object via Shift+R (similar to Blender's "repeat last").
 */
interface ReplayableTransformDelta {
  positionDelta: [number, number, number];
  rotationDelta: [number, number, number];
  scaleFactor: [number, number, number];
}

interface ClipboardEntry {
  assetId: string;
  def: ObjectDef;
}

export class LevelEditor {
  viz: Viz;
  levelDef: LevelDef;
  prototypes: Map<string, THREE.Object3D>;
  builtMaterials: Map<string, THREE.Material>;

  api: LevelEditorApi;
  private undoSystem = new UndoSystem<UndoEntry>();
  private materialEditor: MaterialEditorController;

  private isEditMode = false;
  private orbitControls: OrbitControls | null = null;
  transformControls: TransformControls | null = null;
  selectedNode: LevelSceneNode | null = null;
  /** Convenience accessor — null when the selected node is a group. */
  get selectedObject(): LevelObject | null {
    return this.selectedNode && !isLevelGroup(this.selectedNode) ? this.selectedNode : null;
  }
  private transformMode: TransformMode = 'translate';
  private transformSpace: 'world' | 'local' = 'world';

  private raycaster = new THREE.Raycaster();
  private selectableMeshes: THREE.Mesh[] = [];
  private meshToLevelObject = new Map<THREE.Mesh, LevelObject>();
  allLevelObjects: LevelObject[];
  rootNodes: LevelSceneNode[];
  nodeById: Map<string, LevelSceneNode>;

  // Distinguish clicks from drags — skip raycast if pointer moved significantly
  private pointerDownPos = new THREE.Vector2();
  private pointerMoved = false;
  private originalSetPointerCapture: ((pointerId: number) => void) | null = null;
  private originalReleasePointerCapture: ((pointerId: number) => void) | null = null;

  private dragStartSnapshot: TransformSnapshot | null = null;

  private clipboard: ClipboardEntry | null = null;

  private lastReplayableAction: ReplayableTransformDelta | null = null;

  private selectionState = $state({
    nodeId: null as string | null,
    materialId: null as string | null,
    isGroup: false,
    isGenerated: false,
    isCsgAsset: false,
    position: [0, 0, 0] as [number, number, number],
    rotation: [0, 0, 0] as [number, number, number],
    scale: [1, 1, 1] as [number, number, number],
    /** Incremented whenever rootNodes changes — triggers hierarchy panel re-render. */
    treeVersion: 0,
  });
  private panelComponent: Record<string, any> | null = null;
  private panelTarget: HTMLDivElement | null = null;

  private csgController = new CsgEditController(this);

  constructor(
    viz: Viz,
    objects: LevelObject[],
    levelName: string,
    prototypes: Map<string, THREE.Object3D>,
    builtMaterials: Map<string, THREE.Material>,
    loadedTextures: Map<string, THREE.Texture>,
    levelDef: LevelDef,
    rootNodes: LevelSceneNode[],
    nodeById: Map<string, LevelSceneNode>
  ) {
    this.viz = viz;
    this.levelDef = levelDef;
    this.prototypes = prototypes;
    this.builtMaterials = builtMaterials;
    this.allLevelObjects = objects;
    this.rootNodes = rootNodes;
    this.nodeById = nodeById;

    this.api = new LevelEditorApi(levelName);
    this.materialEditor = new MaterialEditorController(
      levelDef,
      builtMaterials,
      loadedTextures,
      objects,
      this.api
    );

    for (const levelObj of objects) {
      this.registerMeshes(levelObj);
    }

    window.addEventListener('keydown', this.onKeyDown);
    viz.registerDestroyedCb(() => this.destroy());
  }

  registerMeshes(levelObj: LevelObject) {
    levelObj.object.traverse(child => {
      if (child instanceof THREE.Mesh) {
        this.selectableMeshes.push(child);
        this.meshToLevelObject.set(child, levelObj);
      }
    });
  }

  unregisterMeshes(levelObj: LevelObject) {
    levelObj.object.traverse(child => {
      if (child instanceof THREE.Mesh) {
        const idx = this.selectableMeshes.indexOf(child);
        if (idx !== -1) this.selectableMeshes.splice(idx, 1);
        this.meshToLevelObject.delete(child);
      }
    });
  }

  private snapshotTransform(obj: THREE.Object3D): TransformSnapshot {
    const r = obj.rotation;
    return {
      position: obj.position.toArray() as [number, number, number],
      rotation: [r.x, r.y, r.z],
      scale: obj.scale.toArray() as [number, number, number],
    };
  }

  private applySnapshot(obj: THREE.Object3D, snap: TransformSnapshot) {
    obj.position.fromArray(snap.position);
    obj.rotation.set(snap.rotation[0], snap.rotation[1], snap.rotation[2]);
    obj.scale.fromArray(snap.scale);
  }

  private static readonly SNAP_EPS = 1e-6;

  /** Returns true when two snapshots represent the same transform (within floating-point tolerance). */
  private snapshotsEqual(a: TransformSnapshot, b: TransformSnapshot): boolean {
    const eps = LevelEditor.SNAP_EPS;
    for (let i = 0; i < 3; i++) {
      if (Math.abs(a.position[i] - b.position[i]) > eps) return false;
      if (Math.abs(a.rotation[i] - b.rotation[i]) > eps) return false;
      if (Math.abs(a.scale[i] - b.scale[i]) > eps) return false;
    }
    return true;
  }

  private onKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Tab') {
      e.preventDefault();
      this.toggleEditMode();
      return;
    }

    if (!this.isEditMode) return;

    // Don't fire editor binds when the user is typing in an input or textarea.
    const activeTag = (document.activeElement as HTMLElement | null)?.tagName?.toLowerCase();
    const isTypingInput = activeTag === 'input' || activeTag === 'textarea';

    // Undo / Redo
    if (!isTypingInput) {
      if (e.key === 'z' && (e.ctrlKey || e.metaKey) && !e.shiftKey) {
        e.preventDefault();
        if (this.csgController.isActive) this.csgController.undo();
        else this.undoSystem.undo(this.applyUndoEntry);
        return;
      }
      if (
        ((e.key === 'z' || e.key === 'Z') && (e.ctrlKey || e.metaKey) && e.shiftKey) ||
        (e.key === 'y' && (e.ctrlKey || e.metaKey))
      ) {
        e.preventDefault();
        if (this.csgController.isActive) this.csgController.redo();
        else this.undoSystem.redo(this.applyUndoEntry);
        return;
      }
    }

    // Copy / Paste
    if (e.key === 'c' && (e.ctrlKey || e.metaKey) && !isTypingInput) {
      const selObj = this.selectedObject;
      if (selObj) {
        this.clipboard = { assetId: selObj.assetId, def: selObj.def };
      }
      return;
    }
    if (e.key === 'v' && (e.ctrlKey || e.metaKey) && !isTypingInput) {
      if (this.clipboard) {
        e.preventDefault();
        void this.pasteObject();
      }
      return;
    }

    if (isTypingInput) {
      return;
    }

    if (e.key === 'g' || e.key === 'G') {
      this.setTransformMode('translate');
    } else if (e.key === 'R' && e.shiftKey) {
      // Shift+R: repeat last transform action
      this.replayLastAction();
    } else if (e.key === 'r') {
      this.setTransformMode('rotate');
    } else if (e.key === 's' || e.key === 'S') {
      this.setTransformMode('scale');
    } else if (e.key === 'l') {
      this.toggleTransformSpace();
    } else if (e.key === '.') {
      this.focusSelected();
    } else if (e.key === 'Escape') {
      if (!this.csgController.handleEscape()) {
        this.deselect();
      }
    } else if (e.key === 'Delete' || e.key === 'Backspace') {
      if (this.selectedNode) {
        if (this.selectedNode.generated) {
          console.info('[LevelEditor] Generated nodes are read-only in the editor.');
          return;
        }
        e.preventDefault();
        if (isLevelGroup(this.selectedNode)) {
          this.deleteGroup(this.selectedNode);
        } else {
          this.deleteObject(this.selectedNode);
        }
      }
    }
  };

  private setTransformMode(mode: TransformMode) {
    this.transformMode = mode;
    this.transformControls?.setMode(mode);
  }

  private toggleTransformSpace() {
    this.transformSpace = this.transformSpace === 'world' ? 'local' : 'world';
    this.transformControls?.setSpace(this.transformSpace);
  }

  private toggleEditMode() {
    if (this.isEditMode) {
      this.exitEditMode();
    } else {
      this.enterEditMode();
    }
  }

  private enterEditMode() {
    this.isEditMode = true;

    this.viz.controlState.movementEnabled = false;
    this.viz.controlState.cameraControlEnabled = false;

    if (document.pointerLockElement) {
      document.exitPointerLock();
    }

    const playerPos = this.viz.fpCtx?.playerStateGetters.getPlayerPos();
    const target = playerPos
      ? new THREE.Vector3(playerPos[0], playerPos[1], playerPos[2])
      : new THREE.Vector3();

    const ORBIT_DISTANCE = 20;
    const lookDir = this.viz.camera.getWorldDirection(new THREE.Vector3());
    this.viz.camera.position.copy(target).addScaledVector(lookDir, -ORBIT_DISTANCE);

    this.orbitControls = new OrbitControls(this.viz.camera, this.viz.renderer.domElement);
    this.orbitControls.enableDamping = true;
    this.orbitControls.dampingFactor = 0.1;
    this.orbitControls.target.copy(target);
    this.orbitControls.update();

    this.transformControls = new TransformControls(this.viz.camera, this.viz.renderer.domElement);
    this.transformControls.setMode(this.transformMode);
    this.transformControls.setSpace(this.transformSpace);
    this.transformControls.addEventListener('dragging-changed', (e: any) => {
      if (this.orbitControls) {
        this.orbitControls.enabled = !e.value;
      }

      if (this.csgController.isActive) {
        if (e.value) this.csgController.onDragStart();
        else this.csgController.onDragEnd();
        return;
      }

      if (e.value) {
        if (this.selectedNode) {
          this.dragStartSnapshot = this.snapshotTransform(this.selectedNode.object);
        }
      } else {
        if (this.selectedNode && this.dragStartSnapshot) {
          const after = this.snapshotTransform(this.selectedNode.object);
          const before = this.dragStartSnapshot;
          this.dragStartSnapshot = null;

          if (this.snapshotsEqual(before, after)) return;

          this.undoSystem.push({
            type: 'transform',
            levelObj: this.selectedNode,
            before,
            after,
          });

          // Replay captures direct object edits only, but physics must be synced for both
          // leaf objects and groups (which affect descendant world transforms).
          if (!isLevelGroup(this.selectedNode)) {
            this.lastReplayableAction = {
              positionDelta: [
                after.position[0] - before.position[0],
                after.position[1] - before.position[1],
                after.position[2] - before.position[2],
              ],
              rotationDelta: [
                after.rotation[0] - before.rotation[0],
                after.rotation[1] - before.rotation[1],
                after.rotation[2] - before.rotation[2],
              ],
              scaleFactor: [
                before.scale[0] !== 0 ? after.scale[0] / before.scale[0] : 1,
                before.scale[1] !== 0 ? after.scale[1] / before.scale[1] : 1,
                before.scale[2] !== 0 ? after.scale[2] / before.scale[2] : 1,
              ],
            };
          }

          this.syncSceneNodePhysics(this.selectedNode);
          this.api.saveTransform(this.selectedNode);
          this.syncTransformFromNode();
        }
      }
    });
    // Live preview: sync transform display and CSG during drag.
    this.transformControls.addEventListener('objectChange', () => {
      if (this.csgController.isActive) {
        this.csgController.onObjectChange();
      } else {
        this.syncTransformFromNode();
      }
    });
    this.viz.overlayScene.add(this.transformControls);

    this.viz.registerBeforeRenderCb(this.tickOrbitControls);

    const canvas = this.viz.renderer.domElement;
    this.installSafePointerCapture(canvas);
    canvas.addEventListener('pointerdown', this.onPointerDown);
    canvas.addEventListener('pointermove', this.onPointerMove);
    canvas.addEventListener('pointerup', this.onPointerUp);

    this.createPanel();
  }

  private exitEditMode() {
    this.isEditMode = false;

    if (this.csgController.isActive) this.csgController.exit();
    this.deselect();
    this.materialEditor.close();
    this.csgController.closeEditor();

    this.viz.controlState.movementEnabled = true;
    this.viz.controlState.cameraControlEnabled = true;

    this.viz.unregisterBeforeRenderCb(this.tickOrbitControls);
    this.orbitControls?.dispose();
    this.orbitControls = null;

    if (this.transformControls) {
      this.viz.overlayScene.remove(this.transformControls);
      this.transformControls.dispose();
      this.transformControls = null;
    }

    const canvas = this.viz.renderer.domElement;
    this.restorePointerCapture(canvas);
    canvas.removeEventListener('pointerdown', this.onPointerDown);
    canvas.removeEventListener('pointermove', this.onPointerMove);
    canvas.removeEventListener('pointerup', this.onPointerUp);

    this.destroyPanel();
  }

  private createPanel() {
    const target = document.createElement('div');
    document.body.appendChild(target);
    this.panelTarget = target;

    const state = this.selectionState;
    const materialEditorOpen = () => this.materialEditor.isOpen;
    // eslint-disable-next-line @typescript-eslint/no-this-alias
    const self = this;

    this.panelComponent = mount(LevelEditorPanel, {
      target,
      props: {
        assetIds: Object.keys(this.levelDef.assets),
        materialIds: Object.keys(this.levelDef.materials ?? {}),
        get rootNodes() {
          void state.treeVersion;
          return self.rootNodes;
        },
        get selectedNodeId(): string | null {
          return state.nodeId;
        },
        get selectedMaterialId(): string | null {
          return state.materialId;
        },
        get isGroupSelected(): boolean {
          return state.isGroup;
        },
        get isGeneratedSelected(): boolean {
          return state.isGenerated;
        },
        get isCsgAsset(): boolean {
          return state.isCsgAsset;
        },
        get materialEditorOpen(): boolean {
          return materialEditorOpen();
        },
        get position(): [number, number, number] {
          return state.position;
        },
        get rotation(): [number, number, number] {
          return state.rotation;
        },
        get scale(): [number, number, number] {
          return state.scale;
        },
        onselectnode: (node: import('./loadLevelDef').LevelSceneNode) => this.select(node),
        onadd: (assetId: string, materialId: string | undefined) => this.onAddClick(assetId, materialId),
        onaddgroup: () => void this.onAddGroupClick(),
        onmaterialchange: (matId: string | null) => this.onObjectMaterialChange(matId),
        onapplytransform: (snap: Partial<TransformSnapshot>) => this.applyTransformInput(snap),
        ondelete: () => {
          if (!this.selectedNode || this.selectedNode.generated) return;
          if (isLevelGroup(this.selectedNode)) this.deleteGroup(this.selectedNode);
          else this.deleteObject(this.selectedNode);
        },
        ontoggleMaterialEditor: () => {
          if (this.materialEditor.isOpen) {
            this.materialEditor.close();
          } else {
            this.materialEditor.open(this.selectedObject?.def?.material ?? null);
          }
        },
        onconvertToCsg: () => {
          if (this.selectedObject && !this.selectedObject.generated) {
            void this.csgController.convertToCsg(this.selectedObject.id);
          }
        },
      },
    });
  }

  private destroyPanel() {
    if (this.panelComponent) {
      unmount(this.panelComponent);
      this.panelComponent = null;
    }
    if (this.panelTarget) {
      this.panelTarget.remove();
      this.panelTarget = null;
    }
  }

  updateSelectionState() {
    const node = this.selectedNode;
    this.selectionState.nodeId = node?.id ?? null;
    this.selectionState.materialId = this.selectedObject?.def?.material ?? null;
    this.selectionState.isGroup = node ? isLevelGroup(node) : false;
    this.selectionState.isGenerated = node ? node.generated : false;
    this.selectionState.isCsgAsset = this.csgController.isEditorOpen;
    this.syncTransformFromNode();
  }

  /** Reads the current Three.js object transform into selectionState. Called at all points
   *  where the transform may have changed: selection, drag events, undo/redo, replay. */
  private syncTransformFromNode() {
    const node = this.selectedNode;
    if (!node) {
      this.selectionState.position = [0, 0, 0];
      this.selectionState.rotation = [0, 0, 0];
      this.selectionState.scale = [1, 1, 1];
      return;
    }
    const obj = node.object;
    const r = obj.rotation;
    this.selectionState.position = obj.position.toArray() as [number, number, number];
    this.selectionState.rotation = [r.x, r.y, r.z];
    this.selectionState.scale = obj.scale.toArray() as [number, number, number];
  }

  /** Called from the info panel when a transform field is committed (blur/Enter).
   *  Applies the new value to the Three.js object, pushes undo, and saves to disk. */
  applyTransformInput(snap: Partial<TransformSnapshot>) {
    const node = this.selectedNode;
    if (!node || node.generated) return;
    const obj = node.object;
    const before = this.snapshotTransform(obj);

    if (snap.position) {
      obj.position.fromArray(snap.position);
      this.selectionState.position = snap.position;
    }
    if (snap.rotation) {
      obj.rotation.set(snap.rotation[0], snap.rotation[1], snap.rotation[2]);
      this.selectionState.rotation = snap.rotation;
    }
    if (snap.scale) {
      obj.scale.fromArray(snap.scale);
      this.selectionState.scale = snap.scale;
    }

    const after = this.snapshotTransform(obj);
    if (!this.snapshotsEqual(before, after)) {
      this.undoSystem.push({ type: 'transform', levelObj: node, before, after });
      this.api.saveTransform(node);
      this.syncSceneNodePhysics(node);
    }
  }

  private tickOrbitControls = () => {
    this.orbitControls?.update();
  };

  private onPointerDown = (e: PointerEvent) => {
    this.pointerDownPos.set(e.clientX, e.clientY);
    this.pointerMoved = false;
  };

  private onPointerMove = (e: PointerEvent) => {
    const dx = e.clientX - this.pointerDownPos.x;
    const dy = e.clientY - this.pointerDownPos.y;
    if (dx * dx + dy * dy > 16) {
      this.pointerMoved = true;
    }
  };

  private onPointerUp = (e: PointerEvent) => {
    if (!this.pointerMoved && e.button === 0) {
      this.doRaycast(e);
    }
  };

  private doRaycast(e: PointerEvent) {
    const rect = this.viz.renderer.domElement.getBoundingClientRect();
    const mouse = new THREE.Vector2(
      ((e.clientX - rect.left) / rect.width) * 2 - 1,
      -((e.clientY - rect.top) / rect.height) * 2 + 1
    );

    this.raycaster.setFromCamera(mouse, this.viz.camera);

    if (this.csgController.doRaycast(this.raycaster)) {
      return;
    }

    const hits = this.raycaster.intersectObjects(this.selectableMeshes, false);
    if (hits.length > 0) {
      const levelObj = this.meshToLevelObject.get(hits[0].object as THREE.Mesh);
      if (levelObj) {
        this.select(levelObj);
        return;
      }
    }

    this.deselect();
  }

  private installSafePointerCapture(canvas: HTMLCanvasElement) {
    if (this.originalSetPointerCapture || this.originalReleasePointerCapture) {
      return;
    }

    this.originalSetPointerCapture = canvas.setPointerCapture.bind(canvas);
    this.originalReleasePointerCapture = canvas.releasePointerCapture.bind(canvas);

    canvas.setPointerCapture = ((pointerId: number) => {
      try {
        this.originalSetPointerCapture?.(pointerId);
      } catch (err) {
        if (err instanceof DOMException && err.name === 'InvalidStateError') {
          return;
        }
        throw err;
      }
    }) as typeof canvas.setPointerCapture;

    canvas.releasePointerCapture = ((pointerId: number) => {
      try {
        this.originalReleasePointerCapture?.(pointerId);
      } catch (err) {
        if (
          err instanceof DOMException &&
          (err.name === 'InvalidStateError' || err.name === 'NotFoundError')
        ) {
          return;
        }
        throw err;
      }
    }) as typeof canvas.releasePointerCapture;
  }

  private restorePointerCapture(canvas: HTMLCanvasElement) {
    if (this.originalSetPointerCapture) {
      canvas.setPointerCapture = this.originalSetPointerCapture as typeof canvas.setPointerCapture;
      this.originalSetPointerCapture = null;
    }

    if (this.originalReleasePointerCapture) {
      canvas.releasePointerCapture = this
        .originalReleasePointerCapture as typeof canvas.releasePointerCapture;
      this.originalReleasePointerCapture = null;
    }
  }

  select(node: LevelSceneNode) {
    const levelObj = isLevelGroup(node) ? null : node;

    if (this.csgController.isActive && this.csgController.editingLevelObj !== levelObj) {
      this.csgController.exit();
    }

    this.selectedNode = node;

    if (isLevelGroup(node)) {
      // Groups: attach TransformControls only for editable groups; no CSG/material picker.
      if (node.generated) this.transformControls?.detach();
      else this.transformControls?.attach(node.object);
      this.csgController.closeEditor();
      this.updateSelectionState();
      return;
    }

    if (this.materialEditor.isOpen && levelObj!.def.material) {
      this.materialEditor.setSelectedId(levelObj!.def.material);
    }

    const assetDef = this.levelDef.assets[levelObj!.assetId];
    if (levelObj!.generated) {
      this.transformControls?.detach();
      this.csgController.closeEditor();
    } else if (assetDef?.type === 'csg') {
      if (!this.csgController.isActive || this.csgController.editingLevelObj !== levelObj) {
        this.csgController.enter(levelObj!);
      }
    } else {
      this.transformControls?.attach(levelObj!.object);
      this.csgController.closeEditor();
    }

    this.updateSelectionState();
  }

  private deselect() {
    if (this.csgController.isActive) {
      this.csgController.exit();
    }
    this.selectedNode = null;
    this.transformControls?.detach();
    this.updateSelectionState();
    this.csgController.closeEditor();
  }

  /**
   * Centers the orbit camera on the selected object (like Blender's numpad '.').
   */
  private focusSelected() {
    if (!this.selectedNode || !this.orbitControls) return;

    const obj = this.selectedNode.object;
    const box = new THREE.Box3().setFromObject(obj);
    const center = box.getCenter(new THREE.Vector3());
    const sphere = new THREE.Sphere();
    box.getBoundingSphere(sphere);
    const radius = sphere.radius > 0 ? sphere.radius : 1;

    focusCamera({
      camera: this.viz.camera as THREE.PerspectiveCamera,
      orbitControls: this.orbitControls,
      center,
      radius,
    });
  }

  /**
   * Replay the last transform action on the currently selected object (Shift+R).
   * The stored delta is applied additively for position/rotation and
   * multiplicatively for scale, mirroring Blender's "repeat last" behaviour.
   */
  private replayLastAction() {
    if (!this.lastReplayableAction || !this.selectedNode) {
      return;
    }
    if (this.selectedNode.generated) {
      return;
    }

    const delta = this.lastReplayableAction;
    const obj = this.selectedNode.object;
    const before = this.snapshotTransform(obj);

    const after: TransformSnapshot = {
      position: [
        before.position[0] + delta.positionDelta[0],
        before.position[1] + delta.positionDelta[1],
        before.position[2] + delta.positionDelta[2],
      ],
      rotation: [
        before.rotation[0] + delta.rotationDelta[0],
        before.rotation[1] + delta.rotationDelta[1],
        before.rotation[2] + delta.rotationDelta[2],
      ],
      scale: [
        before.scale[0] * delta.scaleFactor[0],
        before.scale[1] * delta.scaleFactor[1],
        before.scale[2] * delta.scaleFactor[2],
      ],
    };

    this.applySnapshot(obj, after);
    this.undoSystem.push({
      type: 'transform',
      levelObj: this.selectedNode,
      before,
      after,
    });
    this.api.saveTransform(this.selectedNode);
    this.syncSceneNodePhysics(this.selectedNode);
    this.syncTransformFromNode();
  }

  private applyUndoEntry = (entry: UndoEntry, direction: 'undo' | 'redo') => {
    if (entry.type === 'transform') {
      const snap = direction === 'undo' ? entry.before : entry.after;
      this.applySnapshot(entry.levelObj.object, snap);
      this.select(entry.levelObj);
      this.api.saveTransform(entry.levelObj);
      this.syncSceneNodePhysics(entry.levelObj);
      this.syncTransformFromNode();
    } else if (entry.type === 'add') {
      if (direction === 'undo') {
        this.removeFromScene(entry.levelObj);
        this.api.sendDelete(entry.levelObj.id);
      } else {
        this.addToScene(entry.levelObj, entry.snapshot);
        this.api.sendRestore(entry.levelObj, entry.snapshot);
      }
    } else if (entry.type === 'delete') {
      if (direction === 'undo') {
        this.addToScene(entry.levelObj, entry.snapshot);
        this.api.sendRestore(entry.levelObj, entry.snapshot);
      } else {
        this.removeFromScene(entry.levelObj);
        this.api.sendDelete(entry.levelObj.id);
      }
    } else {
      entry satisfies never;
    }
  };

  private removeFromScene(levelObj: LevelObject) {
    if (this.selectedObject === levelObj) this.deselect();
    this.unregisterMeshes(levelObj);
    this.removePhysics(levelObj);
    const parent = levelObj.object.parent ?? this.viz.scene;
    parent.remove(levelObj.object);
    const idx = this.allLevelObjects.indexOf(levelObj);
    if (idx !== -1) this.allLevelObjects.splice(idx, 1);
    // Remove from rootNodes if it was a top-level object
    const rootIdx = this.rootNodes.indexOf(levelObj);
    if (rootIdx !== -1) {
      this.rootNodes.splice(rootIdx, 1);
      this.selectionState.treeVersion++;
    }
  }

  private addToScene(levelObj: LevelObject, snapshot: TransformSnapshot) {
    this.applySnapshot(levelObj.object, snapshot);
    this.viz.scene.add(levelObj.object);
    this.allLevelObjects.push(levelObj);
    this.rootNodes.push(levelObj);
    this.selectionState.treeVersion++;
    this.registerMeshes(levelObj);
    if (this.viz.fpCtx) this.syncPhysics(levelObj);
    this.select(levelObj);
  }

  /** Recursively collect all LevelObjects within a group subtree. */
  private collectGroupLeaves(group: LevelGroup): LevelObject[] {
    const leaves: LevelObject[] = [];
    for (const child of group.children) {
      if (isLevelGroup(child)) {
        leaves.push(...this.collectGroupLeaves(child));
      } else {
        leaves.push(child);
      }
    }
    return leaves;
  }

  private deleteGroup(group: LevelGroup) {
    if (this.selectedNode === group) this.deselect();
    // Remove physics and mesh registration for all leaf descendants.
    for (const leaf of this.collectGroupLeaves(group)) {
      this.unregisterMeshes(leaf);
      this.removePhysics(leaf);
      const idx = this.allLevelObjects.indexOf(leaf);
      if (idx !== -1) this.allLevelObjects.splice(idx, 1);
    }
    // Remove the group's Three.js object (children are part of the group hierarchy).
    (group.object.parent ?? this.viz.scene).remove(group.object);
    // Remove from rootNodes.
    const rootIdx = this.rootNodes.indexOf(group);
    if (rootIdx !== -1) this.rootNodes.splice(rootIdx, 1);
    this.selectionState.treeVersion++;
    void this.api.sendDelete(group.id);
  }

  private async onAddGroupClick() {
    const round = (n: number) => Math.round(n * 10000) / 10000;
    const orbitTarget = this.orbitControls?.target ?? new THREE.Vector3();
    const position: [number, number, number] = [
      round(orbitTarget.x),
      round(orbitTarget.y),
      round(orbitTarget.z),
    ];

    const newDef = await this.api.sendAddGroup({ position });
    if (!newDef) return;

    const groupObj = new THREE.Group();
    groupObj.position.fromArray(newDef.position ?? [0, 0, 0]);
    this.viz.scene.add(groupObj);

    const levelGroup: LevelGroup = {
      id: newDef.id,
      object: groupObj,
      def: newDef as ObjectGroupDef,
      children: [],
      generated: false,
    };
    this.rootNodes.push(levelGroup);
    this.nodeById.set(levelGroup.id, levelGroup);
    this.selectionState.treeVersion++;
    this.select(levelGroup);
  }

  private deleteObject(levelObj: LevelObject) {
    const snapshot = this.snapshotTransform(levelObj.object);
    this.removeFromScene(levelObj);
    // Only purge stale transform entries; the delete entry itself is the new action
    this.undoSystem.purge(e => e.type === 'transform' && e.levelObj === levelObj);
    this.undoSystem.push({ type: 'delete', levelObj, snapshot });
    this.api.sendDelete(levelObj.id);
  }

  private async onAddClick(assetId: string, materialId: string | undefined) {
    if (!this.prototypes.has(assetId)) {
      console.warn(`[LevelEditor] No prototype for asset "${assetId}" — asset may not be loaded yet`);
      return;
    }

    const round = (n: number) => Math.round(n * 10000) / 10000;
    const orbitTarget = this.orbitControls?.target ?? new THREE.Vector3();
    const position: [number, number, number] = [
      round(orbitTarget.x),
      round(orbitTarget.y),
      round(orbitTarget.z),
    ];

    const newDef = await this.api.sendAdd({ asset: assetId, material: materialId, position });
    if (newDef) this.finalizeSpawn(assetId, newDef);
  }

  private async pasteObject() {
    if (!this.clipboard) return;
    const { assetId, def } = this.clipboard;

    if (!this.prototypes.has(assetId)) {
      console.warn(`[LevelEditor] No prototype for asset "${assetId}" — asset may not be loaded yet`);
      return;
    }

    // Offset slightly so the copy doesn't land exactly on the original.
    const srcPos = def.position ?? [0, 0, 0];
    const position: [number, number, number] = [srcPos[0], srcPos[1] + 0.5, srcPos[2]];

    const newDef = await this.api.sendAdd({
      asset: assetId,
      material: def.material,
      position,
      rotation: def.rotation,
      scale: def.scale,
    });
    if (newDef) this.finalizeSpawn(assetId, newDef);
  }

  private finalizeSpawn(assetId: string, newDef: ObjectDef) {
    const prototype = this.prototypes.get(assetId)!;
    const clone = instantiateLevelObject(prototype, newDef, {
      builtMaterials: this.builtMaterials,
      fallbackMaterial: LEVEL_PLACEHOLDER_MAT,
    });

    const levelObj: LevelObject = { id: newDef.id, assetId, object: clone, def: newDef, generated: false };
    const snapshot = this.snapshotTransform(clone);

    this.viz.scene.add(clone);
    this.allLevelObjects.push(levelObj);
    this.rootNodes.push(levelObj);
    this.selectionState.treeVersion++;
    this.nodeById.set(levelObj.id, levelObj);
    this.registerMeshes(levelObj);
    if (this.viz.fpCtx) this.syncPhysics(levelObj);

    this.undoSystem.push({ type: 'add', levelObj, snapshot });
    this.select(levelObj);
  }

  private onObjectMaterialChange(matId: string | null) {
    const levelObj = this.selectedObject;
    if (!levelObj) return;
    if (levelObj.generated) {
      console.info('[LevelEditor] Generated objects are read-only in the editor.');
      return;
    }

    if (matId) {
      levelObj.def.material = matId;
      assignMaterial(levelObj.object, this.builtMaterials.get(matId) ?? LEVEL_PLACEHOLDER_MAT);
    } else {
      delete levelObj.def.material;
      assignMaterial(levelObj.object, LEVEL_PLACEHOLDER_MAT);
    }

    this.selectionState.materialId = matId;
    void this.api.saveMaterialAssignment(levelObj.id, matId);
  }

  syncPhysics(levelObj: LevelObject) {
    const fpCtx: BulletPhysics | undefined = this.viz.fpCtx;
    if (!fpCtx) return;

    levelObj.object.traverse(child => {
      if (!(child instanceof THREE.Mesh)) {
        return;
      }

      clearPhysicsBinding(child, fpCtx);
      withWorldSpaceTransform(child, mesh => fpCtx.addTriMesh(mesh));
    });
  }

  private syncSceneNodePhysics(node: LevelSceneNode) {
    if (isLevelGroup(node)) {
      for (const child of node.children) {
        this.syncSceneNodePhysics(child);
      }
      return;
    }

    this.syncPhysics(node);
  }

  removePhysics(levelObj: LevelObject) {
    const fpCtx: BulletPhysics | undefined = this.viz.fpCtx;
    if (!fpCtx) {
      return;
    }

    levelObj.object.traverse(child => {
      if (!(child instanceof THREE.Mesh)) {
        return;
      }

      clearPhysicsBinding(child, fpCtx);
    });
  }

  private destroy() {
    window.removeEventListener('keydown', this.onKeyDown);
    if (this.isEditMode) {
      this.exitEditMode();
    }
  }
}

export const initLevelEditor = (
  viz: Viz,
  objects: LevelObject[],
  levelName: string,
  prototypes: Map<string, THREE.Object3D>,
  builtMaterials: Map<string, THREE.Material>,
  loadedTextures: Map<string, THREE.Texture>,
  levelDef: LevelDef,
  rootNodes: LevelSceneNode[],
  nodeById: Map<string, LevelSceneNode>
): LevelEditor =>
  new LevelEditor(
    viz,
    objects,
    levelName,
    prototypes,
    builtMaterials,
    loadedTextures,
    levelDef,
    rootNodes,
    nodeById
  );
