import * as THREE from 'three';
import { mount, unmount } from 'svelte';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';

import type { Viz } from 'src/viz';
import type { BulletPhysics } from 'src/viz/collision';
import type { LevelDef, LightDef, ObjectDef, ObjectGroupDef } from './types';
import type { LevelGroup, LevelLight, LevelObject, LevelSceneNode } from './levelSceneTypes';
import { isLevelGroup } from './levelSceneTypes';
import { SelectionManager } from './SelectionManager.svelte';
import { TransformHandler, snapshotTransform, applySnapshot, snapshotsEqual } from './TransformHandler';
import type { TransformSnapshot, TransformMode } from './TransformHandler';
import {
  LEVEL_PLACEHOLDER_MAT,
  applyTransform,
  assignMaterial,
  instantiateLevelObject,
} from './levelObjectUtils';
import { flattenLeaves, isObjectGroup } from './levelDefTreeUtils';
import { resolveGeoscriptAsset } from './loadLevelDef';
import LevelEditorPanel from './LevelEditorPanel.svelte';
import { LevelEditorApi } from './levelEditorApi';
import { UndoSystem } from './undoSystem';
import { MaterialEditorController } from './materialEditorController';
import { CsgEditController } from './csgEditController.svelte';
import { focusCamera } from '../util/focusCamera';
import { clearPhysicsBinding } from '../util/physics';
import { withWorldSpaceTransform } from '../util/three';

export type { TransformSnapshot } from './TransformHandler';

type UndoEntry =
  | {
      type: 'transform';
      entries: Array<{ node: LevelSceneNode; before: TransformSnapshot; after: TransformSnapshot }>;
    }
  | { type: 'add'; levelObj: LevelObject; snapshot: TransformSnapshot }
  | { type: 'delete'; entries: Array<{ levelObj: LevelObject; snapshot: TransformSnapshot }> };

type ClipboardEntry =
  | { type: 'object'; assetId: string; def: ObjectDef }
  | { type: 'group'; def: ObjectGroupDef };

export class LevelEditor {
  viz: Viz;
  levelDef: LevelDef;
  prototypes: Map<string, THREE.Object3D>;
  builtMaterials: Map<string, THREE.Material>;

  api: LevelEditorApi;
  private undoSystem = new UndoSystem<UndoEntry>();
  private materialEditor: MaterialEditorController;
  readonly selection = new SelectionManager();

  private isEditMode = false;
  private orbitControls: OrbitControls | null = null;
  private transformHandler: TransformHandler | null = null;
  /** Public accessor for transform controls (used by CsgEditController). */
  get transformControls() {
    return this.transformHandler?.controls ?? null;
  }
  /** Delegating accessor for the primary selected node (used by CsgEditController). */
  get selectedNode(): LevelSceneNode | null {
    return this.selection.primaryNode;
  }
  set selectedNode(node: LevelSceneNode | null) {
    if (node) this.selection.select(node);
    else this.selection.deselect();
  }
  /** Convenience accessor — null when the primary selected node is a group. */
  get selectedObject(): LevelObject | null {
    return this.selection.primaryObject;
  }

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

  private clipboard: ClipboardEntry | null = null;

  allLevelLights: LevelLight[];
  /** Delegating accessor for the selected light (used by CsgEditController / external code). */
  get selectedLight(): LevelLight | null {
    return this.selection.selectedLight;
  }
  private lightProxyMeshes: THREE.Mesh[] = [];
  private meshToLevelLight = new Map<THREE.Mesh, LevelLight>();
  private lightToProxy = new Map<string, THREE.Mesh>();
  private lightProxyGeometry = new THREE.OctahedronGeometry(0.4);
  /** Transform mode that was active before a light was selected (restored on deselect). */
  private preLightTransformMode: TransformMode | null = null;

  /** Shorthand for the selection manager's reactive state. */
  private get selectionState() {
    return this.selection.state;
  }
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
    nodeById: Map<string, LevelSceneNode>,
    levelLights: LevelLight[]
  ) {
    this.viz = viz;
    this.levelDef = levelDef;
    this.prototypes = prototypes;
    this.builtMaterials = builtMaterials;
    this.allLevelObjects = objects;
    this.rootNodes = rootNodes;
    this.nodeById = nodeById;
    this.allLevelLights = levelLights;

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

  private snapshotTransform = snapshotTransform;
  private applySnapshot = applySnapshot;

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
      const node = this.selection.primaryNode;
      if (node) {
        if (isLevelGroup(node)) {
          this.clipboard = { type: 'group', def: JSON.parse(JSON.stringify(node.def)) };
        } else {
          this.clipboard = { type: 'object', assetId: node.assetId, def: node.def };
        }
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
      // Shift+R: repeat last transform action (single selection only)
      if (this.selection.isSingle) this.replayLastAction();
    } else if (e.key === 'r') {
      // Block rotation in multi-select
      if (!this.selection.isMulti) this.setTransformMode('rotate');
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
      if (this.selectedLight) {
        e.preventDefault();
        this.deleteLight(this.selectedLight);
      } else if (this.selection.count > 0) {
        e.preventDefault();
        this.deleteSelected();
      }
    }
  };

  private setTransformMode(mode: TransformMode) {
    this.transformHandler?.setMode(mode);
  }

  private toggleTransformSpace() {
    this.transformHandler?.toggleSpace();
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

    this.transformHandler = new TransformHandler(
      this.viz.camera,
      this.viz.renderer.domElement,
      this.viz.overlayScene,
      {
        onDraggingChanged: isDragging => {
          if (this.orbitControls) this.orbitControls.enabled = !isDragging;
        },
        onDragComplete: result => {
          this.undoSystem.push({ type: 'transform', entries: result.entries });
          for (const { node } of result.entries) {
            this.syncSceneNodePhysics(node);
            this.api.saveTransform(node);
          }
          this.syncTransformFromNode();
        },
        onObjectChange: () => this.syncTransformFromNode(),
        onCsgDragStart: () => this.csgController.onDragStart(),
        onCsgDragEnd: () => this.csgController.onDragEnd(),
        onCsgObjectChange: () => this.csgController.onObjectChange(),
        onLightObjectChange: () => this.syncLightPositionFromObject(),
        onLightDragComplete: () => {
          if (this.selectedLight) this.saveLightPosition(this.selectedLight);
        },
        isCsgActive: () => this.csgController.isActive,
        isLightSelected: () => this.selectedLight !== null,
      }
    );

    this.viz.registerBeforeRenderCb(this.tickOrbitControls);

    const canvas = this.viz.renderer.domElement;
    this.installSafePointerCapture(canvas);
    canvas.addEventListener('pointerdown', this.onPointerDown);
    canvas.addEventListener('pointermove', this.onPointerMove);
    canvas.addEventListener('pointerup', this.onPointerUp);

    this.createLightProxies();
    this.createPanel();

    // Fetch the asset library tree in the background; update the panel once it arrives.
    this.api.fetchAssetLibrary().then(folders => {
      this.selectionState.libFolders = folders;
    });
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

    if (this.transformHandler) {
      this.transformHandler.dispose(this.viz.overlayScene);
      this.transformHandler = null;
    }

    const canvas = this.viz.renderer.domElement;
    this.restorePointerCapture(canvas);
    canvas.removeEventListener('pointerdown', this.onPointerDown);
    canvas.removeEventListener('pointermove', this.onPointerMove);
    canvas.removeEventListener('pointerup', this.onPointerUp);

    this.destroyLightProxies();
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
        get assetIds() {
          void state.assetsVersion;
          return Object.keys(self.levelDef.assets);
        },
        materialIds: Object.keys(this.levelDef.materials ?? {}),
        get libFolders() {
          return state.libFolders;
        },
        get rootNodes() {
          void state.treeVersion;
          return self.rootNodes;
        },
        get lights() {
          void state.treeVersion;
          return self.allLevelLights;
        },
        get selectedLightId(): string | null {
          return state.selectedLightDef?.id ?? null;
        },
        get selectedLightDef() {
          return state.selectedLightDef;
        },
        get lightPosition(): [number, number, number] {
          return state.lightPosition;
        },
        get selectedNodeIds(): string[] {
          return state.selectedNodeIds;
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
        onselectnode: (node: import('./loadLevelDef').LevelSceneNode, ctrlKey: boolean) => {
          if (ctrlKey) this.toggleSelect(node);
          else this.select(node);
        },
        onselectlight: (light: import('./levelSceneTypes').LevelLight) => this.selectLight(light),
        onaddlight: (lightType: import('./types').LightDef['type']) => void this.onAddLightClick(lightType),
        onlightpositionchange: (pos: [number, number, number]) => this.applyLightPositionInput(pos),
        onlightpropertychange: (update: Partial<import('./types').LightDef>) =>
          this.applyLightPropertyChange(update),
        ondeletelight: () => {
          if (this.selectedLight) this.deleteLight(this.selectedLight);
        },
        onadd: (assetId: string, materialId: string | undefined) => this.onAddClick(assetId, materialId),
        onaddlibrary: (libPath: string, materialId: string | undefined) =>
          void this.onAddLibraryClick(libPath, materialId),
        onaddgroup: () => void this.onAddGroupClick(),
        onrename: (newId: string) => {
          if (this.selectedNode && !this.selectedNode.generated) {
            void this.renameNode(this.selectedNode, newId);
          }
        },
        onmaterialchange: (matId: string | null) => this.onObjectMaterialChange(matId),
        onapplytransform: (snap: Partial<TransformSnapshot>) => this.applyTransformInput(snap),
        ondelete: () => this.deleteSelected(),
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
        ongroupselected: () => void this.groupSelected(),
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
    if (this.selectedLight) {
      this.selection.syncState({ isCsgAsset: false });
      this.syncLightPositionFromObject();
      return;
    }
    this.selection.syncState({ isCsgAsset: this.csgController.isEditorOpen });
  }

  /** Reads the current Three.js object transform into selectionState. Called at all points
   *  where the transform may have changed: selection, drag events, undo/redo, replay. */
  private syncTransformFromNode() {
    this.selection.syncTransformDisplay();
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
    if (!snapshotsEqual(before, after)) {
      this.undoSystem.push({ type: 'transform', entries: [{ node, before, after }] });
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

    const isToggle = e.ctrlKey || e.metaKey;
    console.log({ isToggle });

    // Check light proxies first (they're small so prioritize proximity)
    const lightHits = this.raycaster.intersectObjects(this.lightProxyMeshes, false);
    if (lightHits.length > 0) {
      const levelLight = this.meshToLevelLight.get(lightHits[0].object as THREE.Mesh);
      if (levelLight) {
        // Lights don't participate in multi-select
        this.selectLight(levelLight);
        return;
      }
    }

    const hits = this.raycaster.intersectObjects(this.selectableMeshes, false);
    if (hits.length > 0) {
      const levelObj = this.meshToLevelObject.get(hits[0].object as THREE.Mesh);
      if (levelObj) {
        if (isToggle) {
          this.toggleSelect(levelObj);
        } else {
          this.select(levelObj);
        }
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
    if (this.csgController.isActive) {
      const levelObj = isLevelGroup(node) ? null : node;
      if (this.csgController.editingLevelObj !== levelObj) {
        this.csgController.exit();
      }
    }

    this.selection.select(node);
    this.syncAfterSelectionChange();
  }

  toggleSelect(node: LevelSceneNode) {
    if (this.csgController.isActive) this.csgController.exit();

    this.selection.toggleSelect(node);
    this.syncAfterSelectionChange();
  }

  /**
   * After any selection change, sync the transform gizmo attachment,
   * CSG/material editor state, and reactive UI state.
   */
  private syncAfterSelectionChange() {
    const nodes = this.selection.selectedNodes;
    const primary = this.selection.primaryNode;
    const primaryObj = this.selection.primaryObject;

    if (nodes.length === 0) {
      this.transformHandler?.detach();
      this.csgController.closeEditor();
      this.updateSelectionState();
      return;
    }

    // Multi-select: no CSG editing, use pivot-based transform
    if (this.selection.isMulti) {
      this.csgController.closeEditor();
      // Block rotation in multi-select
      if (this.transformHandler?.getMode() === 'rotate') {
        this.transformHandler.setMode('translate');
      }
      // Filter out generated nodes for transform attachment
      const editableNodes = nodes.filter(n => !n.generated);
      if (editableNodes.length > 0) {
        this.transformHandler?.attachToSelection([...editableNodes]);
      } else {
        this.transformHandler?.detach();
      }
      this.updateSelectionState();
      return;
    }

    // Single selection
    const node = primary!;

    if (isLevelGroup(node)) {
      if (node.generated) this.transformHandler?.detach();
      else this.transformHandler?.attachToSelection([node]);
      this.csgController.closeEditor();
      this.updateSelectionState();
      return;
    }

    const levelObj = primaryObj!;
    if (this.materialEditor.isOpen && levelObj.def.material) {
      this.materialEditor.setSelectedId(levelObj.def.material);
    }

    const assetDef = this.levelDef.assets[levelObj.assetId];
    if (levelObj.generated) {
      this.transformHandler?.detach();
      this.csgController.closeEditor();
    } else if (assetDef?.type === 'csg') {
      if (!this.csgController.isActive || this.csgController.editingLevelObj !== levelObj) {
        this.csgController.enter(levelObj);
      }
    } else {
      this.transformHandler?.attachToSelection([levelObj]);
      this.csgController.closeEditor();
    }

    this.updateSelectionState();
  }

  private deselect() {
    if (this.csgController.isActive) {
      this.csgController.exit();
    }
    if (this.selectedLight) {
      this.deselectLight();
      return;
    }
    this.selection.deselect();
    this.transformHandler?.detach();
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

  private createLightProxies() {
    for (const levelLight of this.allLevelLights) {
      if (levelLight.def.type === 'ambient') continue; // no position to show
      const mat = new THREE.MeshBasicMaterial({
        color: levelLight.def.color ?? 0xffffff,
        transparent: true,
        opacity: 0.75,
        depthTest: false,
      });
      const proxy = new THREE.Mesh(this.lightProxyGeometry, mat);
      if (levelLight.def.position) proxy.position.fromArray(levelLight.def.position);
      proxy.renderOrder = 999;
      this.viz.scene.add(proxy);
      this.lightProxyMeshes.push(proxy);
      this.meshToLevelLight.set(proxy, levelLight);
      this.lightToProxy.set(levelLight.id, proxy);
    }
  }

  private destroyLightProxies() {
    for (const proxy of this.lightProxyMeshes) {
      this.viz.scene.remove(proxy);
      (proxy.material as THREE.Material).dispose();
    }
    this.lightProxyMeshes = [];
    this.meshToLevelLight.clear();
    this.lightToProxy.clear();
  }

  selectLight(levelLight: LevelLight) {
    // Deselect any active scene node / CSG first
    if (this.csgController.isActive) this.csgController.exit();
    this.csgController.closeEditor();

    this.selection.selectLight(levelLight);

    // Force translate mode for lights (only position is meaningful)
    if (levelLight.def.type !== 'ambient') {
      this.preLightTransformMode = this.transformHandler?.getMode() ?? 'translate';
      this.setTransformMode('translate');
      this.transformHandler?.attach(levelLight.light);
    } else {
      this.transformHandler?.detach();
    }

    this.updateSelectionState();
  }

  private deselectLight() {
    this.selection.deselectLight();
    this.transformHandler?.detach();
    // Restore previous transform mode
    if (this.preLightTransformMode) {
      this.setTransformMode(this.preLightTransformMode);
      this.preLightTransformMode = null;
    }
    this.selectionState.selectedLightDef = null;
    this.syncTransformFromNode();
  }

  private syncLightPositionFromObject() {
    if (!this.selectedLight) return;
    this.selection.syncLightPosition();
    // Keep proxy in sync
    const proxy = this.lightToProxy.get(this.selectedLight.id);
    if (proxy) proxy.position.copy(this.selectedLight.light.position);
  }

  private saveLightPosition(levelLight: LevelLight) {
    if (levelLight.def.type === 'ambient') return;
    const round = (n: number) => Math.round(n * 10000) / 10000;
    const pos = levelLight.light.position;
    const position: [number, number, number] = [round(pos.x), round(pos.y), round(pos.z)];
    levelLight.def = { ...levelLight.def, position } as LightDef;
    this.updateLevelDefLight(levelLight);
    void this.api.saveLight(levelLight.def);
    this.selectionState.selectedLightDef = levelLight.def;
  }

  /** Apply a position change from the info panel inputs. */
  applyLightPositionInput(pos: [number, number, number]) {
    const light = this.selectedLight;
    if (!light || light.def.type === 'ambient') return;
    light.light.position.fromArray(pos);
    const proxy = this.lightToProxy.get(light.id);
    if (proxy) proxy.position.fromArray(pos);
    this.saveLightPosition(light);
    this.selectionState.lightPosition = pos;
  }

  /** Apply property changes (color, intensity, type-specific params) from the info panel. */
  applyLightPropertyChange(update: Partial<LightDef>) {
    const light = this.selectedLight;
    if (!light) return;
    light.def = { ...light.def, ...update } as LightDef;
    this.updateLevelDefLight(light);
    void this.api.saveLight(light.def);
    this.selectionState.selectedLightDef = light.def;

    // Apply live to the Three.js light
    const l = light.light;
    if ('color' in update && update.color !== undefined) {
      l.color.setHex(update.color);
      const proxy = this.lightToProxy.get(light.id);
      if (proxy) (proxy.material as THREE.MeshBasicMaterial).color.setHex(update.color);
    }
    if ('intensity' in update && update.intensity !== undefined) {
      l.intensity = update.intensity;
    }
    if ('castShadow' in update && update.castShadow !== undefined) {
      l.castShadow = update.castShadow;
    }
    if ('distance' in update && update.distance !== undefined && 'distance' in l) {
      (l as THREE.PointLight | THREE.SpotLight).distance = update.distance;
    }
    if ('decay' in update && update.decay !== undefined && 'decay' in l) {
      (l as THREE.PointLight | THREE.SpotLight).decay = update.decay as number;
    }
    if ('angle' in update && update.angle !== undefined && l instanceof THREE.SpotLight) {
      l.angle = update.angle;
    }
    if ('penumbra' in update && update.penumbra !== undefined && l instanceof THREE.SpotLight) {
      l.penumbra = update.penumbra;
    }
  }

  private updateLevelDefLight(levelLight: LevelLight) {
    if (!this.levelDef.lights) return;
    const idx = this.levelDef.lights.findIndex(l => l.id === levelLight.id);
    if (idx !== -1) this.levelDef.lights[idx] = levelLight.def;
  }

  private deleteLight(levelLight: LevelLight) {
    if (this.selection.selectedLight === levelLight) this.deselectLight();

    // Remove from scene
    this.viz.scene.remove(levelLight.light);

    // Remove proxy
    const proxy = this.lightToProxy.get(levelLight.id);
    if (proxy) {
      this.viz.scene.remove(proxy);
      (proxy.material as THREE.Material).dispose();
      const proxyIdx = this.lightProxyMeshes.indexOf(proxy);
      if (proxyIdx !== -1) this.lightProxyMeshes.splice(proxyIdx, 1);
      this.meshToLevelLight.delete(proxy);
      this.lightToProxy.delete(levelLight.id);
    }

    // Remove from allLevelLights and levelDef
    const idx = this.allLevelLights.indexOf(levelLight);
    if (idx !== -1) this.allLevelLights.splice(idx, 1);
    if (this.levelDef.lights) {
      const defIdx = this.levelDef.lights.findIndex(l => l.id === levelLight.id);
      if (defIdx !== -1) this.levelDef.lights.splice(defIdx, 1);
    }

    this.selectionState.treeVersion++;
    void this.api.deleteLight(levelLight.id);
  }

  async onAddLightClick(lightType: LightDef['type']) {
    const round = (n: number) => Math.round(n * 10000) / 10000;
    const orbitTarget = this.orbitControls?.target ?? new THREE.Vector3();
    const position: [number, number, number] = [
      round(orbitTarget.x),
      round(orbitTarget.y),
      round(orbitTarget.z),
    ];

    const candidate: Partial<LightDef> & { type: LightDef['type'] } = { type: lightType };
    if (lightType !== 'ambient') {
      (candidate as any).position = position;
    }

    const newDef = await this.api.addLight(candidate as Omit<LightDef, 'id'>);
    if (!newDef) return;

    // Create the Three.js light (inline since the helper is not exported)
    let newLight: THREE.Light;
    switch (newDef.type) {
      case 'ambient':
        newLight = new THREE.AmbientLight(newDef.color ?? 0xffffff, newDef.intensity ?? 1);
        break;
      case 'directional': {
        const l = new THREE.DirectionalLight(newDef.color ?? 0xffffff, newDef.intensity ?? 1);
        if (newDef.position) l.position.fromArray(newDef.position);
        newLight = l;
        break;
      }
      case 'point': {
        const l = new THREE.PointLight(
          newDef.color ?? 0xffffff,
          newDef.intensity ?? 1,
          newDef.distance ?? 0,
          newDef.decay ?? 2
        );
        if (newDef.position) l.position.fromArray(newDef.position);
        newLight = l;
        break;
      }
      case 'spot': {
        const l = new THREE.SpotLight(
          newDef.color ?? 0xffffff,
          newDef.intensity ?? 1,
          newDef.distance ?? 0,
          newDef.angle ?? Math.PI / 4,
          newDef.penumbra ?? 0,
          newDef.decay ?? 2
        );
        if (newDef.position) l.position.fromArray(newDef.position);
        newLight = l;
        break;
      }
    }
    newLight!.name = newDef.id;
    this.viz.scene.add(newLight!);

    const levelLight: LevelLight = { id: newDef.id, light: newLight!, def: newDef };
    this.allLevelLights.push(levelLight);
    if (!this.levelDef.lights) this.levelDef.lights = [];
    this.levelDef.lights.push(newDef);

    // Create proxy if positional
    if (newDef.type !== 'ambient') {
      const mat = new THREE.MeshBasicMaterial({
        color: newDef.color ?? 0xffffff,
        transparent: true,
        opacity: 0.75,
        depthTest: false,
      });
      const proxy = new THREE.Mesh(this.lightProxyGeometry, mat);
      if (newDef.position) proxy.position.fromArray(newDef.position);
      proxy.renderOrder = 999;
      this.viz.scene.add(proxy);
      this.lightProxyMeshes.push(proxy);
      this.meshToLevelLight.set(proxy, levelLight);
      this.lightToProxy.set(newDef.id, proxy);
    }

    this.selectionState.treeVersion++;
    this.selectLight(levelLight);
  }

  /**
   * Replay the last transform action on the currently selected object (Shift+R).
   * The stored delta is applied additively for position/rotation and
   * multiplicatively for scale, mirroring Blender's "repeat last" behaviour.
   */
  private replayLastAction() {
    const node = this.selectedNode;
    if (!node || !this.transformHandler) return;

    const result = this.transformHandler.replayLastAction(node);
    if (!result) return;

    this.undoSystem.push({
      type: 'transform',
      entries: [{ node, before: result.before, after: result.after }],
    });
    this.api.saveTransform(node);
    this.syncSceneNodePhysics(node);
    this.syncTransformFromNode();
  }

  private applyUndoEntry = (entry: UndoEntry, direction: 'undo' | 'redo') => {
    if (entry.type === 'transform') {
      for (const te of entry.entries) {
        const snap = direction === 'undo' ? te.before : te.after;
        this.applySnapshot(te.node.object, snap);
        this.api.saveTransform(te.node);
        this.syncSceneNodePhysics(te.node);
      }
      // Re-select the first node for focus
      if (entry.entries.length > 0) this.select(entry.entries[0].node);
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
      for (const de of entry.entries) {
        if (direction === 'undo') {
          this.addToScene(de.levelObj, de.snapshot);
          this.api.sendRestore(de.levelObj, de.snapshot);
        } else {
          this.removeFromScene(de.levelObj);
          this.api.sendDelete(de.levelObj.id);
        }
      }
    } else {
      entry satisfies never;
    }
  };

  private removeFromScene(levelObj: LevelObject) {
    if (this.selection.isSelected(levelObj)) this.deselect();
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
    if (this.selection.isSelected(group)) this.deselect();
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

  /**
   * Group the currently selected nodes into a new parent group.
   * All selected nodes must be siblings (same level in the hierarchy).
   */
  private async groupSelected() {
    const nodes = [...this.selection.selectedNodes];
    if (nodes.length < 2) return;

    // Filter out generated nodes
    const editableNodes = nodes.filter(n => !n.generated);
    if (editableNodes.length < 2) return;

    const round = (n: number) => Math.round(n * 10000) / 10000;

    // Compute centroid of selected nodes' positions
    const centroid = new THREE.Vector3();
    for (const node of editableNodes) {
      centroid.add(node.object.getWorldPosition(new THREE.Vector3()));
    }
    centroid.divideScalar(editableNodes.length);
    const position: [number, number, number] = [round(centroid.x), round(centroid.y), round(centroid.z)];

    const nodeIds = editableNodes.map(n => n.id);
    const newDef = await this.api.groupNodes(nodeIds, position);
    if (!newDef) return;

    // Create the Three.js group
    const groupObj = new THREE.Group();
    groupObj.position.fromArray(newDef.position ?? [0, 0, 0]);

    // Reparent each selected node's Three.js object under the new group.
    // Adjust local position to preserve world placement.
    for (const node of editableNodes) {
      const worldPos = node.object.getWorldPosition(new THREE.Vector3());
      const localPos = worldPos.sub(centroid);
      node.object.position.set(round(localPos.x), round(localPos.y), round(localPos.z));

      // Remove from current Three.js parent
      node.object.parent?.remove(node.object);
      groupObj.add(node.object);
    }

    this.viz.scene.add(groupObj);

    // Build the LevelGroup for our scene graph
    const childNodes: LevelSceneNode[] = [];
    for (const node of editableNodes) {
      childNodes.push(node);

      // Remove from rootNodes if it was top-level
      const rootIdx = this.rootNodes.indexOf(node);
      if (rootIdx !== -1) this.rootNodes.splice(rootIdx, 1);
    }

    const levelGroup: LevelGroup = {
      id: newDef.id,
      object: groupObj,
      def: newDef as ObjectGroupDef,
      children: childNodes,
      generated: false,
    };
    this.rootNodes.push(levelGroup);
    this.nodeById.set(levelGroup.id, levelGroup);
    this.selectionState.treeVersion++;

    // Clear selection and select the new group
    this.select(levelGroup);
  }

  private async renameNode(node: LevelSceneNode, newId: string) {
    const oldId = node.id;
    if (newId === oldId) return;

    const result = await this.api.renameNode(oldId, newId);
    if (!result) return;

    const { resolvedId } = result;

    // Update the in-memory node and its def.
    node.id = resolvedId;
    node.def.id = resolvedId;

    // Re-key the nodeById map.
    this.nodeById.delete(oldId);
    this.nodeById.set(resolvedId, node);

    // Sync the selection display.
    if (this.selectionState.nodeId === oldId) {
      this.selectionState.nodeId = resolvedId;
    }

    // Trigger hierarchy panel re-render.
    this.selectionState.treeVersion++;
  }

  /**
   * Delete all currently selected nodes. Creates a single compound undo entry.
   */
  private deleteSelected() {
    const nodes = [...this.selection.selectedNodes];
    if (nodes.length === 0) return;

    // Check if any are generated (read-only)
    if (nodes.every(n => n.generated)) {
      console.info('[LevelEditor] Generated nodes are read-only in the editor.');
      return;
    }

    const deleteEntries: Array<{ levelObj: LevelObject; snapshot: TransformSnapshot }> = [];

    for (const node of nodes) {
      if (node.generated) continue;
      if (isLevelGroup(node)) {
        this.deleteGroup(node);
      } else {
        const snapshot = this.snapshotTransform(node.object);
        this.removeFromScene(node);
        deleteEntries.push({ levelObj: node, snapshot });
        this.api.sendDelete(node.id);
      }
    }

    if (deleteEntries.length > 0) {
      // Purge stale transform entries for deleted objects
      const deletedSet = new Set<LevelSceneNode>(deleteEntries.map(e => e.levelObj));
      this.undoSystem.purge(e => e.type === 'transform' && e.entries.some(te => deletedSet.has(te.node)));
      this.undoSystem.push({ type: 'delete', entries: deleteEntries });
    }
  }

  private async onAddLibraryClick(libPath: string, materialId: string | undefined) {
    // Register the file as an asset in the level def (idempotent if already registered).
    const result = await this.api.registerLibraryAsset(libPath);
    if (!result) return;

    const { id, code } = result;

    // Build a prototype if we don't already have one for this id.
    if (!this.prototypes.has(id)) {
      const prototype = await resolveGeoscriptAsset(code);
      if (!prototype) {
        console.error(`[LevelEditor] Failed to resolve library asset "${libPath}"`);
        return;
      }
      prototype.name = id;
      this.prototypes.set(id, prototype);
      this.levelDef.assets[id] = { type: 'geoscript', code };
      this.selectionState.assetsVersion++;
    }

    await this.onAddClick(id, materialId);
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
    const clip = this.clipboard;
    if (!clip) return;

    if (clip.type === 'group') {
      const srcPos = clip.def.position ?? [0, 0, 0];
      const patchedDef: ObjectGroupDef = {
        ...JSON.parse(JSON.stringify(clip.def)),
        position: [srcPos[0], srcPos[1] + 0.5, srcPos[2]] as [number, number, number],
      };

      // Guard: verify all leaf prototypes are loaded (they always should be for same-level paste).
      for (const leaf of flattenLeaves([patchedDef])) {
        if (!this.prototypes.has(leaf.asset)) {
          console.warn(`[LevelEditor] No prototype for asset "${leaf.asset}" in pasted group`);
          return;
        }
      }

      const newDef = await this.api.sendPasteGroup(patchedDef);
      if (!newDef) return;

      const levelGroup = this.instantiateGroupSubtree(newDef, this.viz.scene);
      this.rootNodes.push(levelGroup);
      this.selectionState.treeVersion++;
      this.select(levelGroup);
      return;
    }

    // Leaf object paste
    const { assetId, def } = clip;
    if (!this.prototypes.has(assetId)) {
      console.warn(`[LevelEditor] No prototype for asset "${assetId}" — asset may not be loaded yet`);
      return;
    }
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

  /**
   * Create a LevelObject from a prototype and register it with the editor's tracking state.
   * Adds the clone to `parent` (scene root or a group THREE.Object3D).
   * Does NOT add to rootNodes, push undo, or update treeVersion — caller handles those.
   */
  private instantiateLeaf(assetId: string, def: ObjectDef, parent: THREE.Object3D): LevelObject {
    const clone = instantiateLevelObject(this.prototypes.get(assetId)!, def, {
      builtMaterials: this.builtMaterials,
      fallbackMaterial: LEVEL_PLACEHOLDER_MAT,
    });
    const levelObj: LevelObject = { id: def.id, assetId, object: clone, def, generated: false };
    parent.add(clone);
    this.allLevelObjects.push(levelObj);
    this.nodeById.set(levelObj.id, levelObj);
    this.registerMeshes(levelObj);
    if (this.viz.fpCtx) this.syncPhysics(levelObj);
    return levelObj;
  }

  /** Recursively build a LevelGroup subtree from a def, parented under `parentObj`. */
  private instantiateGroupSubtree(def: ObjectGroupDef, parentObj: THREE.Object3D): LevelGroup {
    const groupObj = new THREE.Group();
    applyTransform(groupObj, def);
    parentObj.add(groupObj);

    const levelGroup: LevelGroup = { id: def.id, object: groupObj, def, children: [], generated: false };
    this.nodeById.set(def.id, levelGroup);

    for (const childDef of def.children) {
      if (isObjectGroup(childDef)) {
        levelGroup.children.push(this.instantiateGroupSubtree(childDef, groupObj));
      } else {
        levelGroup.children.push(this.instantiateLeaf(childDef.asset, childDef, groupObj));
      }
    }
    return levelGroup;
  }

  private finalizeSpawn(assetId: string, newDef: ObjectDef) {
    const levelObj = this.instantiateLeaf(assetId, newDef, this.viz.scene);
    const snapshot = this.snapshotTransform(levelObj.object);
    this.rootNodes.push(levelObj);
    this.selectionState.treeVersion++;
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
  nodeById: Map<string, LevelSceneNode>,
  levelLights: LevelLight[]
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
    nodeById,
    levelLights
  );
