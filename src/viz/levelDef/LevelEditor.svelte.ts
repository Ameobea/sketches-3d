import * as THREE from 'three';
import { mount, unmount } from 'svelte';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';

import type { Viz } from 'src/viz';
import type { BulletPhysics } from 'src/viz/collision';
import type { CollisionMeshOverride } from 'src/viz/collisionShapes';
import type { AmbientLightDef, EditorBookmark, HemisphereLightDef, LevelDef, LightDef } from './types';
import type { LevelLight, LevelObject, LevelSceneNode } from './levelSceneTypes';
import { isLevelGroup } from './levelSceneTypes';
import { SelectionManager } from './SelectionManager.svelte';
import {
  TransformHandler,
  snapshotTransform,
  applySnapshot,
  snapshotsEqual,
  snapshotWorldTransform,
} from './TransformHandler';
import type { TransformSnapshot, TransformMode } from './TransformHandler';
import { LEVEL_PLACEHOLDER_MAT, SELECTION_HIGHLIGHT_MAT, assignMaterial } from './levelObjectUtils';
import { isDescendantOf } from './levelDefTreeUtils';
import { resolveGeoscriptAsset } from './loadLevelDef';
import LevelEditorPanel from './LevelEditorPanel.svelte';
import { LevelEditorApi } from './levelEditorApi';
import { UndoSystem } from '../util/undoSystem';
import { MaterialEditorController } from './materialEditorController';
import { CsgEditController } from './csgEditController.svelte';
import { focusCamera } from '../util/focusCamera';
import { clearPhysicsBinding } from '../util/physics';
import { withWorldSpaceTransform } from '../util/three';
import { installClickRaycaster } from '../util/clickRaycaster';
import { round } from './mathUtils';
import { collectSubtreeLeaves } from './editorStructuralOps';
import { EditorMutationController } from './editorMutationController';
import type { StructuralUndoEntry, ClipboardEntry } from './editorMutationController';
import type { LevelEditorPanelActions, LevelEditorPanelViewState } from './levelEditorPanelTypes';
import {
  addLevelLightToScene,
  applyLightDefToLevelLight,
  createLevelLight,
  removeLevelLightFromScene,
} from './levelLightUtils';

export type { TransformSnapshot } from './TransformHandler';

/** Lights with no meaningful world position — no transform gizmo or proxy mesh. */
const isNonPositionalLight = (def: LightDef): def is AmbientLightDef | HemisphereLightDef =>
  def.type === 'ambient' || def.type === 'hemisphere';

type UndoEntry =
  | {
      type: 'transform';
      entries: Array<{ node: LevelSceneNode; before: TransformSnapshot; after: TransformSnapshot }>;
    }
  | StructuralUndoEntry;

export class LevelEditor {
  viz: Viz;
  levelDef: LevelDef;
  prototypes: Map<string, THREE.Mesh>;
  builtMaterials: Map<string, THREE.Material>;
  /** Shared with `loadLevelDef`: precomputed collision-hull data for `convexHull` assets. */
  assetCollisionMeshes: Map<string, CollisionMeshOverride>;
  /**
   * Adopt a new visual prototype for an asset and (if its `colliderShape` requires it)
   * recompute and cache its collision hull.  Returns a promise that resolves once the
   * asset is fully ready — callers should `await` before triggering syncPhysics so that
   * registration uses the new hull rather than the stale one.
   *
   * Provided by `loadLevelDef`; null when running outside that scope.
   */
  resolveAssetPrototype: ((assetId: string, prototype: THREE.Mesh) => Promise<void>) | null;

  api: LevelEditorApi;
  private undoSystem = new UndoSystem<UndoEntry>();
  private materialEditor: MaterialEditorController;
  readonly selection = new SelectionManager();

  private isEditMode = false;
  private orbitControls: OrbitControls | null = null;
  private transformHandler: TransformHandler | null = null;
  /** Three.js scene fog stashed on enter, restored on exit. */
  private savedSceneFog: THREE.FogBase | null = null;
  /** Public accessor for transform controls (used by CsgEditController). */
  get transformControls() {
    return this.transformHandler;
  }
  /**
   * Purge undo/redo entries that reference any of the given node ids.
   * Used by structural ops (e.g. multi-CSG conversion) that delete nodes
   * outside the normal structural-undo path.
   */
  purgeUndoForNodeIds(ids: ReadonlySet<string>) {
    this.undoSystem.purge(entry => {
      if (entry.type === 'transform') {
        return entry.entries.some(e => ids.has(e.node.id));
      }
      const refsDeleted = (op: { subtree: { root: { id: string }; leaves: { id: string }[] } }) =>
        ids.has(op.subtree.root.id) || op.subtree.leaves.some(l => ids.has(l.id));
      return entry.undoOps.some(refsDeleted) || entry.redoOps.some(refsDeleted);
    });
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

  private selectableMeshes: THREE.Mesh[] = [];
  private meshToLevelObject = new Map<THREE.Mesh, LevelObject>();
  allLevelObjects: LevelObject[];
  rootNodes: LevelSceneNode[];
  nodeById: Map<string, LevelSceneNode>;

  private clickRaycasterDisposer: (() => void) | null = null;
  private originalSetPointerCapture: ((pointerId: number) => void) | null = null;
  private originalReleasePointerCapture: ((pointerId: number) => void) | null = null;

  private clipboard: ClipboardEntry[] = [];

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
  private get lightQuality() {
    return this.viz.vizConfig.current.graphics.quality;
  }
  private panelComponent: Record<string, any> | null = null;
  private panelTarget: HTMLDivElement | null = null;

  private csgController = new CsgEditController(this);

  private bookmarks = new Map<number, EditorBookmark>();

  constructor(
    viz: Viz,
    objects: LevelObject[],
    levelName: string,
    prototypes: Map<string, THREE.Mesh>,
    builtMaterials: Map<string, THREE.Material>,
    loadedTextures: Map<string, THREE.Texture>,
    levelDef: LevelDef,
    rootNodes: LevelSceneNode[],
    nodeById: Map<string, LevelSceneNode>,
    levelLights: LevelLight[],
    assetCollisionMeshes: Map<string, CollisionMeshOverride>,
    resolveAssetPrototype: ((assetId: string, prototype: THREE.Mesh) => Promise<void>) | null
  ) {
    this.viz = viz;
    this.levelDef = levelDef;
    this.prototypes = prototypes;
    this.builtMaterials = builtMaterials;
    this.assetCollisionMeshes = assetCollisionMeshes;
    this.resolveAssetPrototype = resolveAssetPrototype;
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

    this.mutationController = new EditorMutationController(
      this,
      this.api,
      e => this.undoSystem.push(e),
      pred => this.undoSystem.purge(pred)
    );

    for (const levelObj of objects) {
      this.registerMeshes(levelObj);
    }

    window.addEventListener('keydown', this.onKeyDown);
    viz.registerDestroyedCb(() => this.destroy());

    void this.api.fetchLocations().then(file => {
      if (!file) return;
      for (const b of file.editor_bookmarks ?? []) {
        this.bookmarks.set(b.slot, b);
      }
    });
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

  /** Stores per-mesh original materials while selection highlight is active. */
  private originalMeshMaterials = new Map<THREE.Mesh, THREE.Material | THREE.Material[]>();
  private mutationController: EditorMutationController;

  private clearSelectionHighlights() {
    for (const [mesh, mat] of this.originalMeshMaterials) {
      (mesh as THREE.Mesh).material = mat as THREE.Material;
    }
    this.originalMeshMaterials.clear();
  }

  private applySelectionHighlights() {
    for (const node of this.selection.selectedNodes) {
      const leaves = isLevelGroup(node)
        ? collectSubtreeLeaves(node)
        : [node as import('./levelSceneTypes').LevelObject];
      for (const leaf of leaves) {
        leaf.object.traverse(child => {
          if (child instanceof THREE.Mesh && !this.originalMeshMaterials.has(child)) {
            this.originalMeshMaterials.set(child, child.material);
            child.material = SELECTION_HIGHLIGHT_MAT;
          }
        });
      }
    }
  }

  private snapshotTransform = snapshotTransform;
  private applySnapshot = applySnapshot;

  private onKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Tab') {
      e.preventDefault();
      this.toggleEditMode();
      return;
    }

    // Don't fire editor binds when the user is typing in an input or textarea.
    const activeTag = (document.activeElement as HTMLElement | null)?.tagName?.toLowerCase();
    const isTypingInput = activeTag === 'input' || activeTag === 'textarea';

    // Bookmark binds — work in BOTH edit and play mode.
    // Ctrl+0..9: teleport to slot. Ctrl+Shift+0..9: save current state to slot.
    if ((e.ctrlKey || e.metaKey) && !isTypingInput && /^Digit[0-9]$/.test(e.code)) {
      e.preventDefault();
      const slot = parseInt(e.code.slice(5), 10);
      if (e.shiftKey) this.captureBookmark(slot);
      else this.teleportToBookmark(slot);
      return;
    }

    if (!this.isEditMode) return;

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
      if (this.csgController.isActive) {
        this.csgController.copySelectedNode();
      } else {
        const nodes = this.selection.selectedNodes.filter(n => !n.generated);
        if (nodes.length > 0) {
          this.clipboard = nodes.map(node =>
            this.mutationController.captureClipboardEntry(node, snapshotWorldTransform(node.object))
          );
        }
      }
      return;
    }
    if (e.key === 'v' && (e.ctrlKey || e.metaKey) && !isTypingInput) {
      if (this.csgController.isActive) {
        if (this.csgController.hasClipboard) {
          e.preventDefault();
          void this.csgController.pasteNode();
        }
      } else if (this.clipboard.length > 0) {
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
    } else if (e.key === 'Delete') {
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
    this.viz.behaviorsPaused = true;

    // Disable any active fog while editing — it obscures distant geometry the
    // user needs to see and interact with. Custom postprocessing fog disables
    // via a shader define; the scene's THREE.js fog is just stashed.
    this.savedSceneFog = this.viz.scene.fog;
    this.viz.scene.fog = null;
    this.viz.postprocessingController?.setFogEnabled(false);

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
    this.viz.registerBeforeRenderCb(this.tickTransformHandler);

    const canvas = this.viz.renderer.domElement;
    this.installSafePointerCapture(canvas);
    this.clickRaycasterDisposer = installClickRaycaster({
      canvas,
      camera: this.viz.camera,
      onClick: ({ raycaster, event }) => this.handleSceneClick(raycaster, event),
    });

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
    this.viz.behaviorsPaused = false;

    this.viz.scene.fog = this.savedSceneFog;
    this.savedSceneFog = null;
    this.viz.postprocessingController?.setFogEnabled(true);

    this.viz.unregisterBeforeRenderCb(this.tickOrbitControls);
    this.orbitControls?.dispose();
    this.orbitControls = null;

    if (this.transformHandler) {
      this.viz.unregisterBeforeRenderCb(this.tickTransformHandler);
      this.transformHandler.dispose();
      this.transformHandler = null;
    }

    const canvas = this.viz.renderer.domElement;
    this.restorePointerCapture(canvas);
    this.clickRaycasterDisposer?.();
    this.clickRaycasterDisposer = null;

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
    const view: LevelEditorPanelViewState = {
      get assetIds() {
        void state.assetsVersion;
        return Object.keys(self.levelDef.assets);
      },
      get materialIds() {
        return Object.keys(self.levelDef.materials ?? {});
      },
      get libFolders() {
        return state.libFolders;
      },
      get rootNodes() {
        void state.treeVersion;
        return [...self.rootNodes];
      },
      get lights() {
        void state.treeVersion;
        return self.allLevelLights;
      },
      get selectedNodeIds() {
        return state.selectedNodeIds;
      },
      get selectedNodeId() {
        return state.nodeId;
      },
      get treeVersion() {
        return state.treeVersion;
      },
      get selectedMaterialId() {
        return state.materialId;
      },
      get selectedLightId() {
        return state.selectedLightDef?.id ?? null;
      },
      get selectedLightDef() {
        return state.selectedLightDef;
      },
      get lightPosition() {
        return state.lightPosition;
      },
      get isGroupSelected() {
        return state.isGroup;
      },
      get isGeneratedSelected() {
        return state.isGenerated;
      },
      get materialEditorOpen() {
        return materialEditorOpen();
      },
      get isCsgAsset() {
        return state.isCsgAsset;
      },
      get position() {
        return state.position;
      },
      get rotation() {
        return state.rotation;
      },
      get scale() {
        return state.scale;
      },
      get canGroupSelected() {
        void state.treeVersion;
        return self.selection.canGroupWith(self.nodeById, self.rootNodes);
      },
      get canConvertSelectedToCsg() {
        void state.treeVersion;
        void state.selectedNodeIds;
        return self.canConvertSelectedToCsg;
      },
    };
    const actions: LevelEditorPanelActions = {
      selectNode: (node, ctrlKey) => {
        if (ctrlKey) this.toggleSelect(node);
        else this.select(node);
      },
      selectLight: light => this.selectLight(light),
      addLight: lightType => void this.onAddLightClick(lightType),
      applyLightPosition: pos => this.applyLightPositionInput(pos),
      applyLightProperty: update => this.applyLightPropertyChange(update),
      deleteLight: () => {
        if (this.selectedLight) this.deleteLight(this.selectedLight);
      },
      addObject: (assetId, materialId) => this.onAddClick(assetId, materialId),
      addLibraryObject: (libPath, materialId) => void this.onAddLibraryClick(libPath, materialId),
      addGroup: () => void this.onAddGroupClick(),
      rename: newId => {
        if (this.selectedNode && !this.selectedNode.generated) {
          void this.renameNode(this.selectedNode, newId);
        }
      },
      changeMaterial: matId => this.onObjectMaterialChange(matId),
      applyTransform: snap => this.applyTransformInput(snap),
      deleteSelection: () => this.deleteSelected(),
      toggleMaterialEditor: () => {
        if (this.materialEditor.isOpen) {
          this.materialEditor.close();
        } else {
          this.materialEditor.open(this.selectedObject?.def?.material ?? null);
        }
      },
      convertToCsg: () => void this.convertSelectedToCsg(),
      groupSelected: () => void this.groupSelected(),
      reparent: parentId => void this.reparentSelected(parentId),
    };

    this.panelComponent = mount(LevelEditorPanel, {
      target,
      props: {
        view,
        actions,
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
      this.clearSelectionHighlights();
      return;
    }
    this.selection.syncState({ isCsgAsset: this.csgController.isEditorOpen });
    this.clearSelectionHighlights();
    if (!this.csgController.isActive) {
      this.applySelectionHighlights();
    }
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

  private tickTransformHandler = () => {
    this.transformHandler?.update();
  };

  private handleSceneClick = (raycaster: THREE.Raycaster, event: PointerEvent) => {
    if (this.csgController.doRaycast(raycaster)) return;

    const isToggle = event.ctrlKey || event.metaKey;

    // Lights are small targets; resolve them first.
    const lightHits = raycaster.intersectObjects(this.lightProxyMeshes, false);
    if (lightHits.length > 0) {
      const levelLight = this.meshToLevelLight.get(lightHits[0].object as THREE.Mesh);
      if (levelLight) {
        this.selectLight(levelLight);
        return;
      }
    }

    const hits = raycaster.intersectObjects(this.selectableMeshes, false);
    if (hits.length > 0) {
      const levelObj = this.meshToLevelObject.get(hits[0].object as THREE.Mesh);
      if (levelObj) {
        if (isToggle) this.toggleSelect(levelObj);
        else this.select(levelObj);
        return;
      }
    }

    this.deselect();
  };

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
   * When CSG editing is active, focuses on the selected CSG sub-node's preview
   * instead of the level object itself.
   */
  private focusSelected() {
    if (!this.orbitControls) return;

    const obj = this.csgController.isActive
      ? this.csgController.getFocusTarget()
      : (this.selectedNode?.object ?? null);
    if (!obj) return;

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
      if (isNonPositionalLight(levelLight.def)) continue; // no position to show
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
    if (!isNonPositionalLight(levelLight.def)) {
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
    if (isNonPositionalLight(levelLight.def)) return;
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
    if (!light || isNonPositionalLight(light.def)) return;
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
    applyLightDefToLevelLight(light, this.lightQuality);
    void this.api.saveLight(light.def);
    this.selectionState.selectedLightDef = light.def;
    if ('color' in update && update.color !== undefined) {
      const proxy = this.lightToProxy.get(light.id);
      if (proxy) (proxy.material as THREE.MeshBasicMaterial).color.setHex(update.color);
    }
  }

  private updateLevelDefLight(levelLight: LevelLight) {
    if (!this.levelDef.lights) return;
    const idx = this.levelDef.lights.findIndex(l => l.id === levelLight.id);
    if (idx !== -1) this.levelDef.lights[idx] = levelLight.def;
  }

  private deleteLight(levelLight: LevelLight) {
    if (this.selection.selectedLight === levelLight) this.deselectLight();

    removeLevelLightFromScene(this.viz.scene, levelLight);

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
    const orbitTarget = this.orbitControls?.target ?? new THREE.Vector3();
    const position: [number, number, number] = [
      round(orbitTarget.x),
      round(orbitTarget.y),
      round(orbitTarget.z),
    ];

    const candidate: Partial<LightDef> & { type: LightDef['type'] } = { type: lightType };
    if (lightType !== 'ambient' && lightType !== 'hemisphere') {
      (candidate as any).position = position;
    }

    const newDef = await this.api.addLight(candidate as Omit<LightDef, 'id'>);
    if (!newDef) return;
    const levelLight = createLevelLight(newDef, this.lightQuality);
    addLevelLightToScene(this.viz.scene, levelLight);
    this.allLevelLights.push(levelLight);
    if (!this.levelDef.lights) this.levelDef.lights = [];
    this.levelDef.lights.push(newDef);

    // Create proxy if positional
    if (!isNonPositionalLight(newDef)) {
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
    } else if (entry.type === 'structural') {
      const nodeToSelect = this.mutationController.applyStructuralUndoEntry(entry, direction);
      this.selectionState.treeVersion++;
      if (nodeToSelect) this.select(nodeToSelect);
      else this.deselect();
    } else {
      entry satisfies never;
    }
  };

  private async onAddGroupClick() {
    const group = await this.mutationController.spawnGroup(this.getOrbitPosition());
    if (group) {
      this.selectionState.treeVersion++;
      this.select(group);
    }
  }

  private getOrbitPosition(): [number, number, number] {
    const t = this.orbitControls?.target ?? new THREE.Vector3();
    return [round(t.x), round(t.y), round(t.z)];
  }

  /**
   * Group the currently selected nodes into a new parent group.
   * All selected nodes must be siblings (same level in the hierarchy).
   */
  private async groupSelected() {
    if (!this.selection.canGroupWith(this.nodeById, this.rootNodes)) return;
    const editableNodes = [...this.selection.selectedNodes].filter(n => !n.generated);
    // Deselect before mutations so highlight state is clean.
    this.deselect();
    const group = await this.mutationController.groupNodes(editableNodes);
    if (group) {
      this.selectionState.treeVersion++;
      this.select(group);
    }
  }

  /**
   * True when the current selection is eligible for "convert to CSG":
   * - At least one node selected
   * - All selected nodes are leaf objects (no groups), non-generated
   * - All referenced assets are geoscript or csg
   * - For multi-select: all selected nodes share the same parent
   */
  get canConvertSelectedToCsg(): boolean {
    const nodes = this.selection.selectedNodes;
    if (nodes.length === 0) return false;
    for (const n of nodes) {
      if (n.generated) return false;
      if (isLevelGroup(n)) return false;
      const assetDef = this.levelDef.assets[n.assetId];
      if (!assetDef) return false;
      if (assetDef.type !== 'geoscript' && assetDef.type !== 'csg') return false;
    }
    if (nodes.length > 1 && !this.selection.haveSharedParent(this.nodeById, this.rootNodes)) {
      return false;
    }
    return true;
  }

  private async convertSelectedToCsg() {
    if (!this.canConvertSelectedToCsg) return;
    const ids = this.selection.selectedNodes.map(n => n.id);
    await this.csgController.convertToCsg(ids);
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

    if (nodes.every(n => n.generated)) {
      console.info('[LevelEditor] Generated nodes are read-only in the editor.');
      return;
    }

    this.deselect();
    this.mutationController.deleteNodes(nodes);
    this.selectionState.treeVersion++;
  }

  /**
   * Reparent the currently selected nodes to a new parent group (or root).
   * Maintains world-space transforms.
   */
  private async reparentSelected(targetParentId: string | null) {
    const nodes = [...this.selection.selectedNodes].filter(n => !n.generated);
    if (nodes.length === 0) return;

    // Filter out invalid reparents (e.g. into self or descendant).
    const validNodes = nodes.filter(node => {
      if (targetParentId === null) return true;
      if (node.id === targetParentId) return false;
      if (
        isLevelGroup(node) &&
        isDescendantOf(
          this.rootNodes.map(n => n.def),
          node.id,
          targetParentId
        )
      )
        return false;
      return true;
    });
    if (validNodes.length === 0) return;

    const targetParent = targetParentId ? this.nodeById.get(targetParentId) : null;
    if (targetParentId && (!targetParent || !isLevelGroup(targetParent) || targetParent.generated)) return;

    await this.mutationController.reparentNodes(validNodes, targetParentId);
    this.selectionState.treeVersion++;
    this.syncAfterSelectionChange();
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
    const leaf = await this.mutationController.spawnLeaf(assetId, materialId, this.getOrbitPosition());
    if (leaf) {
      this.selectionState.treeVersion++;
      this.select(leaf);
    }
  }

  private async pasteObject() {
    if (this.clipboard.length === 0) return;
    const newNodes = await this.mutationController.pasteEntries(this.clipboard);
    if (newNodes.length === 0) return;

    this.selectionState.treeVersion++;

    // CSG editing is incompatible with multi-select; exit before re-selecting.
    if (this.csgController.isActive) this.csgController.exit();

    if (newNodes.length === 1) {
      this.select(newNodes[0]);
    } else {
      this.selection.selectMany(newNodes);
      this.syncAfterSelectionChange();
    }
  }

  private onObjectMaterialChange(matId: string | null) {
    const levelObj = this.selectedObject;
    if (!levelObj) return;
    if (levelObj.generated) {
      console.info('[LevelEditor] Generated objects are read-only in the editor.');
      return;
    }

    // Clear highlight before modifying materials, then re-apply with new base.
    this.clearSelectionHighlights();

    if (matId) {
      levelObj.def.material = matId;
      assignMaterial(levelObj.object, this.builtMaterials.get(matId) ?? LEVEL_PLACEHOLDER_MAT);
    } else {
      delete levelObj.def.material;
      assignMaterial(levelObj.object, LEVEL_PLACEHOLDER_MAT);
    }

    this.selectionState.materialId = matId;
    void this.api.saveMaterialAssignment(levelObj.id, matId);

    this.applySelectionHighlights();
  }

  syncPhysics(levelObj: LevelObject) {
    const fpCtx: BulletPhysics | undefined = this.viz.fpCtx;
    if (!fpCtx) return;

    // The entity's `object` may have been swapped by `replaceLeafInstance`; point
    // it at the current object so future behavior ticks and transforms use the
    // live scene-graph node.  Existing bodies were detached in `removePhysics`.
    levelObj.entity.object = levelObj.object;
    // Resync def-derived physics flags in case the def was edited since placement.
    levelObj.entity.nonPermeable = levelObj.def.nonPermeable;
    // Authored placement may have changed (transform commit, group, reparent);
    // re-base behaviors off the new local transform so they don't fight it.
    levelObj.entity.refreshBaseTransform();

    clearPhysicsBinding(levelObj.object, fpCtx);
    const collisionMeshOverride = this.assetCollisionMeshes.get(levelObj.assetId);
    withWorldSpaceTransform(levelObj.object, mesh =>
      fpCtx.addTriMesh(mesh, 'static', levelObj.entity, collisionMeshOverride)
    );
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

    clearPhysicsBinding(levelObj.object, fpCtx);
  }

  private captureBookmark(slot: number) {
    let bookmark: EditorBookmark;
    if (this.isEditMode) {
      const cam = this.viz.camera.position;
      const target = this.orbitControls?.target ?? new THREE.Vector3();
      bookmark = {
        slot,
        mode: 'edit',
        cameraPos: [round(cam.x), round(cam.y), round(cam.z)],
        orbitTarget: [round(target.x), round(target.y), round(target.z)],
      };
    } else {
      const playerPos = this.viz.fpCtx?.playerStateGetters.getPlayerPos();
      if (!playerPos) {
        console.warn('[LevelEditor] cannot bookmark — no player context yet');
        return;
      }
      const angles = this.viz.cameraController?.getAngles() ?? { phi: Math.PI / 2, theta: 0 };
      bookmark = {
        slot,
        mode: 'play',
        playerPos: [round(playerPos[0]), round(playerPos[1]), round(playerPos[2])],
        cameraAngles: { phi: angles.phi, theta: angles.theta },
      };
    }
    this.bookmarks.set(slot, bookmark);
    void this.api.saveBookmark(bookmark);
  }

  private teleportToBookmark(slot: number) {
    const bookmark = this.bookmarks.get(slot);
    if (!bookmark) return;

    if (bookmark.mode === 'play') {
      if (this.isEditMode) this.exitEditMode();
      this.viz.fpCtx?.teleportPlayer(bookmark.playerPos);
      this.viz.cameraController?.setAngles(bookmark.cameraAngles.phi, bookmark.cameraAngles.theta);
    } else {
      if (!this.isEditMode) this.enterEditMode();
      this.viz.camera.position.fromArray(bookmark.cameraPos);
      this.orbitControls?.target.fromArray(bookmark.orbitTarget);
      this.orbitControls?.update();
    }
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
  prototypes: Map<string, THREE.Mesh>,
  builtMaterials: Map<string, THREE.Material>,
  loadedTextures: Map<string, THREE.Texture>,
  levelDef: LevelDef,
  rootNodes: LevelSceneNode[],
  nodeById: Map<string, LevelSceneNode>,
  levelLights: LevelLight[],
  assetCollisionMeshes: Map<string, CollisionMeshOverride>,
  resolveAssetPrototype: ((assetId: string, prototype: THREE.Mesh) => Promise<void>) | null
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
    levelLights,
    assetCollisionMeshes,
    resolveAssetPrototype
  );
