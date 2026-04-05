import * as THREE from 'three';
import { mount, unmount } from 'svelte';

import { runGeoscript } from 'src/geoscript/runner/geoscriptRunner';
import { WorkerManager } from 'src/geoscript/workerManager';
import type { CsgAssetDef, CsgTreeNode, AssetDef } from './types';
import type { LevelObject } from './loadLevelDef';
import { LEVEL_PLACEHOLDER_MAT, instantiateLevelObject } from './levelObjectUtils';
import { generateCsgCode, generateComplementCode, generateSubtreeCode } from './csgCodeGen';
import { isOpNode, getNodeAtPath, cloneTree, computeNodePolarities } from './csgTreeUtils';
import CsgTreeEditor from './CsgTreeEditor.svelte';
import type { LevelEditor } from './LevelEditor.svelte';
import { UndoSystem } from './undoSystem';

type TransformTuple = [number, number, number];

interface CsgTreeUndoEntry {
  type: 'tree';
  before: CsgTreeNode;
  after: CsgTreeNode;
}

interface CsgRootTransformUndoEntry {
  type: 'rootTransform';
  before: { position: TransformTuple; rotation: TransformTuple; scale: TransformTuple };
  after: { position: TransformTuple; rotation: TransformTuple; scale: TransformTuple };
}

type CsgUndoEntry = CsgTreeUndoEntry | CsgRootTransformUndoEntry;

const CSG_NEGATIVE_MAT = new THREE.MeshBasicMaterial({ color: 0xff3333, transparent: true, opacity: 0.4 });
const CSG_SELECTED_MAT = new THREE.MeshBasicMaterial({ color: 0x00ffff, wireframe: true });
const CSG_NESTED_NEGATIVE_MAT = new THREE.MeshBasicMaterial({
  color: 0x9966ff,
  transparent: true,
  opacity: 0.35,
});
const CSG_PICK_MAT = new THREE.MeshBasicMaterial({ transparent: true, opacity: 0, depthWrite: false });

export class CsgEditController {
  private editor: LevelEditor;

  private csgPanelState = $state({
    editorOpen: false,
    assetName: null as string | null,
    tree: null as CsgTreeNode | null,
    selectedNodePath: null as string | null,
    nodePolarities: new Map<string, 'positive' | 'negative'>(),
  });

  private csgEditorComponent: Record<string, any> | null = null;
  private csgEditorTarget: HTMLDivElement | null = null;

  private _isActive = false;
  private editLevelObj: LevelObject | null = null;
  private editGroup: THREE.Group | null = null;
  private nodePreviews = new Map<string, THREE.Object3D>();
  private resolvedPreviews = new Map<string, { wrapper: THREE.Group; preview: THREE.Object3D }>();
  private nodePolarities = new Map<string, 'positive' | 'negative'>();
  private selectableMeshes: THREE.Mesh[] = [];
  private meshToNodePath = new Map<THREE.Mesh, string>();
  private selectedNodePath: string | null = null;
  private complementPreview: THREE.Object3D | null = null;
  private configGeneration = 0;
  private resolveInFlight = false;
  private resolvePending = false;
  private previewWorkerManager: WorkerManager | null = null;
  private previewRepl: ReturnType<WorkerManager['getWorker']> | null = null;
  private previewCtxPtrPromise: Promise<number> | null = null;
  private previewResolveQueue: Promise<void> = Promise.resolve();
  private assetWorkerManager: WorkerManager | null = null;
  private assetRepl: ReturnType<WorkerManager['getWorker']> | null = null;
  private assetCtxPtrPromise: Promise<number> | null = null;
  private assetResolveRequestId = 0;
  private assetResolveLatestQueuedRequestId = 0;
  private assetResolveQueuedAssetId: string | null = null;
  private assetResolveDrainPromise: Promise<void> | null = null;

  private undoSystem = new UndoSystem<CsgUndoEntry>();
  private treeBeforeDrag: CsgTreeNode | null = null;
  private rootTransformBeforeDrag: {
    position: TransformTuple;
    rotation: TransformTuple;
    scale: TransformTuple;
  } | null = null;

  constructor(editor: LevelEditor) {
    this.editor = editor;
  }

  get isActive(): boolean {
    return this._isActive;
  }

  get editingLevelObj(): LevelObject | null {
    return this.editLevelObj;
  }

  get isEditorOpen(): boolean {
    return this.csgPanelState.editorOpen;
  }

  undo(): boolean {
    return this.undoSystem.undo(entry => {
      if (entry.type === 'tree') {
        this.onTreeChange(cloneTree(entry.before), false);
      } else {
        this.applyRootTransform(entry.before);
      }
    });
  }

  redo(): boolean {
    return this.undoSystem.redo(entry => {
      if (entry.type === 'tree') {
        this.onTreeChange(cloneTree(entry.after), false);
      } else {
        this.applyRootTransform(entry.after);
      }
    });
  }

  private applyRootTransform(snap: {
    position: TransformTuple;
    rotation: TransformTuple;
    scale: TransformTuple;
  }) {
    if (!this.editLevelObj) return;
    const obj = this.editLevelObj.object;
    obj.position.set(...snap.position);
    obj.rotation.set(...snap.rotation, 'YXZ');
    obj.scale.set(...snap.scale);
    this.editLevelObj.def.position = [...snap.position];
    this.editLevelObj.def.rotation = [...snap.rotation];
    this.editLevelObj.def.scale = [...snap.scale];
    this.editor.api.saveTransform(this.editLevelObj);
    this.editor.syncPhysics(this.editLevelObj);
    if (this.editGroup) {
      this.editGroup.position.copy(obj.position);
      this.editGroup.rotation.copy(obj.rotation);
      this.editGroup.scale.copy(obj.scale);
    }
    if (this.selectedNodePath === '') {
      this.editor.transformControls?.attach(obj);
    }
  }

  private snapshotRootTransform(): {
    position: TransformTuple;
    rotation: TransformTuple;
    scale: TransformTuple;
  } | null {
    if (!this.editLevelObj) return null;
    const d = this.editLevelObj.def;
    return {
      position: [...(d.position ?? [0, 0, 0])] as TransformTuple,
      rotation: [...(d.rotation ?? [0, 0, 0])] as TransformTuple,
      scale: [...(d.scale ?? [1, 1, 1])] as TransformTuple,
    };
  }

  /** Handle start of a transform drag in CSG edit mode — snapshot for undo. */
  onDragStart() {
    if (this.selectedNodePath === '') {
      this.rootTransformBeforeDrag = this.snapshotRootTransform();
    } else if (this.csgPanelState.tree) {
      this.treeBeforeDrag = cloneTree(this.csgPanelState.tree);
    }
  }

  /** Handle end of a transform drag in CSG edit mode. */
  onDragEnd() {
    if (this.selectedNodePath === null || !this.csgPanelState.assetName || !this.csgPanelState.tree) {
      return;
    }

    if (this.selectedNodePath === '') {
      // Root selected — transform applies to the level object
      if (this.editLevelObj) {
        const obj = this.editLevelObj.object;
        const round = (n: number) => Math.round(n * 10000) / 10000;
        this.editLevelObj.def.position = obj.position.toArray().map(round) as [number, number, number];
        this.editLevelObj.def.rotation = [obj.rotation.x, obj.rotation.y, obj.rotation.z].map(round) as [
          number,
          number,
          number,
        ];
        this.editLevelObj.def.scale = obj.scale.toArray().map(round) as [number, number, number];
        this.editor.api.saveTransform(this.editLevelObj);
        this.editor.syncPhysics(this.editLevelObj);

        if (this.rootTransformBeforeDrag) {
          const after = this.snapshotRootTransform();
          if (after) {
            this.undoSystem.push({ type: 'rootTransform', before: this.rootTransformBeforeDrag, after });
          }
          this.rootTransformBeforeDrag = null;
        }
      }
    } else {
      this.onNodeTransformUpdate(); // final update
      this.editor.api.saveCsgTree(this.csgPanelState.assetName, this.csgPanelState.tree);

      // Push undo entry for the transform
      if (this.treeBeforeDrag) {
        this.undoSystem.push({
          type: 'tree',
          before: this.treeBeforeDrag,
          after: cloneTree(this.csgPanelState.tree),
        });
        this.treeBeforeDrag = null;
      }
    }
  }

  /** Handle live transform updates in CSG edit mode (called on every objectChange). */
  onObjectChange() {
    if (this.selectedNodePath === '') {
      // Root selected — keep editGroup in sync with the level object
      if (this.editGroup && this.editLevelObj) {
        const obj = this.editLevelObj.object;
        this.editGroup.position.copy(obj.position);
        this.editGroup.rotation.copy(obj.rotation);
        this.editGroup.scale.copy(obj.scale);
      }
    } else if (this.selectedNodePath) {
      this.onNodeTransformUpdate();
    }
  }

  handleEscape(): boolean {
    if (!this._isActive) return false;

    if (this.selectedNodePath !== null) {
      this.deselectNode();
    } else {
      this.exit();
    }
    return true;
  }

  /** Raycast against CSG node previews. Returns true if handled. */
  doRaycast(raycaster: THREE.Raycaster): boolean {
    if (!this._isActive) return false;

    const hits = raycaster.intersectObjects(this.selectableMeshes, false);
    if (hits.length > 0) {
      const path = this.meshToNodePath.get(hits[0].object as THREE.Mesh);
      if (path !== undefined) {
        this.selectNode(path);
        return true;
      }
    }

    if (this.editLevelObj?.object.visible) {
      const rootHits = raycaster.intersectObject(this.editLevelObj.object, true);
      if (rootHits.length > 0) {
        this.selectNode('');
        return true;
      }
    }

    this.deselectNode();
    return true;
  }

  // ---------------------------------------------------------------------------
  // Enter / exit
  // ---------------------------------------------------------------------------

  enter(levelObj: LevelObject) {
    if (this._isActive) this.exit();

    this._isActive = true;
    this.undoSystem.clear();
    this.editLevelObj = levelObj;
    this.editor.selectedNode = levelObj;
    this.editor.transformControls?.detach();

    const assetDef = this.editor.levelDef.assets[levelObj.assetId] as CsgAssetDef;

    // Create container group at the CSG object's world transform
    this.editGroup = new THREE.Group();
    const obj = levelObj.object;
    this.editGroup.position.copy(obj.position);
    this.editGroup.rotation.copy(obj.rotation);
    this.editGroup.scale.copy(obj.scale);
    this.editor.viz.scene.add(this.editGroup);

    this.nodePolarities = computeNodePolarities(assetDef.tree);
    this.openEditor(levelObj.assetId, assetDef.tree);
    this.selectNode('');
    this.editor.updateSelectionState();
  }

  exit() {
    if (!this._isActive) return;

    // Save current tree state before exiting
    if (this.csgPanelState.assetName && this.csgPanelState.tree) {
      this.editor.api.saveCsgTree(this.csgPanelState.assetName, this.csgPanelState.tree);
    }

    this.editor.transformControls?.detach();
    this.selectedNodePath = null;
    this.csgPanelState.selectedNodePath = null;
    this.configGeneration++;
    this.teardownPreviews();

    // Restore visibility and ensure physics is in sync with the final mesh
    if (this.editLevelObj) {
      this.editLevelObj.object.visible = true;
      this.editor.syncPhysics(this.editLevelObj);
    }

    if (this.editGroup) {
      this.editor.viz.scene.remove(this.editGroup);
      this.editGroup = null;
    }

    this.terminatePreviewWorker();
    this.terminateAssetWorker();

    this._isActive = false;
    const levelObj = this.editLevelObj;
    this.editLevelObj = null;

    this.closeEditor();

    // Re-attach transform controls to the level object if still selected
    if (levelObj) {
      this.editor.transformControls?.attach(levelObj.object);
    }
  }

  // ---------------------------------------------------------------------------
  // CSG editor UI
  // ---------------------------------------------------------------------------

  private openEditor(assetName: string, tree: CsgTreeNode) {
    this.closeEditor();

    this.csgPanelState.editorOpen = true;
    this.csgPanelState.assetName = assetName;
    this.csgPanelState.tree = tree;
    this.csgPanelState.nodePolarities = computeNodePolarities(tree);

    const target = document.createElement('div');
    document.body.appendChild(target);
    this.csgEditorTarget = target;

    const csgState = this.csgPanelState;
    const geoscriptAssetIds = Object.entries(this.editor.levelDef.assets)
      .filter(([, def]: [string, AssetDef]) => def.type === 'geoscript')
      .map(([id]) => id);

    this.csgEditorComponent = mount(CsgTreeEditor, {
      target,
      props: {
        get tree(): CsgTreeNode | null {
          return csgState.tree;
        },
        get selectedNodePath(): string | null {
          return csgState.selectedNodePath;
        },
        get nodePolarities(): Map<string, 'positive' | 'negative'> {
          return csgState.nodePolarities;
        },
        assetIds: geoscriptAssetIds,
        ontreechange: (tree: CsgTreeNode) => this.onTreeChange(tree),
        onnodeselect: (path: string | null) => {
          if (path !== null) this.selectNode(path);
          else this.deselectNode();
        },
        onexitcsg: () => this.exit(),
      },
    });
  }

  closeEditor() {
    if (this.csgEditorComponent) {
      unmount(this.csgEditorComponent);
      this.csgEditorComponent = null;
    }
    if (this.csgEditorTarget) {
      this.csgEditorTarget.remove();
      this.csgEditorTarget = null;
    }
    this.csgPanelState.editorOpen = false;
    this.csgPanelState.assetName = null;
    this.csgPanelState.tree = null;
    this.csgPanelState.selectedNodePath = null;
    this.csgPanelState.nodePolarities = new Map();
  }

  // ---------------------------------------------------------------------------
  // Node selection
  // ---------------------------------------------------------------------------

  private selectNode(path: string) {
    this.selectedNodePath = path;
    this.csgPanelState.selectedNodePath = path;
    this.applyRenderConfig();
  }

  private deselectNode() {
    this.selectedNodePath = null;
    this.csgPanelState.selectedNodePath = null;
    this.applyRenderConfig();
  }

  // ---------------------------------------------------------------------------
  // Previews
  // ---------------------------------------------------------------------------

  private teardownPreviews() {
    if (this.editGroup) {
      while (this.editGroup.children.length > 0) {
        this.editGroup.remove(this.editGroup.children[0]);
      }
    }
    this.nodePreviews.clear();
    this.resolvedPreviews.clear();
    this.selectableMeshes.length = 0;
    this.meshToNodePath.clear();
    this.complementPreview = null;
  }

  /**
   * Collect paths of negative children of difference ops within a subtree.
   * For difference ops, children at index > 0 are negative.
   * If `rootPath` is empty, walks the whole tree.
   */
  private collectNegativeSubtreePaths(tree: CsgTreeNode, rootPath: string): string[] {
    const node = rootPath ? getNodeAtPath(tree, rootPath) : tree;
    const paths: string[] = [];
    const walk = (n: CsgTreeNode, path: string) => {
      if (!isOpNode(n)) return;
      for (let i = 0; i < n.children.length; i++) {
        const childPath = path ? `${path}.${i}` : `${i}`;
        if (n.op === 'difference' && i > 0) paths.push(childPath);
        walk(n.children[i], childPath);
      }
    };
    walk(node, rootPath);
    return paths;
  }

  /** Collect all descendant paths within a subtree, excluding the subtree root by default. */
  private collectSubtreePaths(tree: CsgTreeNode, rootPath: string, includeRoot = false): string[] {
    const node = rootPath ? getNodeAtPath(tree, rootPath) : tree;
    const paths: string[] = [];
    const walk = (n: CsgTreeNode, path: string) => {
      if (includeRoot || path !== rootPath) {
        paths.push(path);
      }
      if (!isOpNode(n)) return;
      for (let i = 0; i < n.children.length; i++) {
        const childPath = path ? `${path}.${i}` : `${i}`;
        walk(n.children[i], childPath);
      }
    };
    walk(node, rootPath);
    return paths;
  }

  /**
   * Central method that sets up the correct CSG edit rendering based on the
   * current selection state. Always tears down and rebuilds from scratch.
   *
   * Config 1 (no selection): full CSG result visible + negative subtree overlays.
   * Config 2 (positive node selected): full result hidden, complement shown as
   *   solid, selected subtree highlighted, negatives within selection shown.
   * Config 3 (negative node selected): full result visible, selected subtree
   *   shown as red transparent, nested negatives in third color.
   */
  private applyRenderConfig() {
    this.configGeneration++;
    this.editor.transformControls?.detach();
    this.teardownPreviews();

    if (!this.editGroup || !this.editLevelObj) return;
    const assetName = this.csgPanelState.assetName;
    if (!assetName) return;
    const csgDef = this.editor.levelDef.assets[assetName] as CsgAssetDef;

    if (this.selectedNodePath === null) {
      // Config 1: no selection
      this.editLevelObj.object.visible = true;
      const negPaths = new Set(this.collectNegativeSubtreePaths(csgDef.tree, ''));
      for (const path of this.collectSubtreePaths(csgDef.tree, '')) {
        if (negPaths.has(path)) continue;
        void this.resolveSubtreePreview(path, CSG_PICK_MAT, { pickable: true, trackNodePreview: false });
      }
      for (const path of negPaths) {
        void this.resolveSubtreePreview(path, CSG_NEGATIVE_MAT, { pickable: true, trackNodePreview: false });
      }
      return;
    }

    if (this.selectedNodePath === '') {
      // Config ROOT: root selected — treat as level object selection.
      // Show full result, attach gizmo to level object, and keep descendants
      // pickable so scene selection still works.
      this.editLevelObj.object.visible = true;
      this.editor.transformControls?.attach(this.editLevelObj.object);
      for (const path of this.collectSubtreePaths(csgDef.tree, '')) {
        void this.resolveSubtreePreview(path, CSG_PICK_MAT, { pickable: true, trackNodePreview: false });
      }
      return;
    }

    const polarity = this.nodePolarities.get(this.selectedNodePath) ?? 'positive';

    if (polarity === 'positive') {
      // Config 2: positive selection — hide full result, show complement + selection
      this.editLevelObj.object.visible = false;
      void this.resolveComplementPreview(this.selectedNodePath);
      void this.resolveSubtreePreview(this.selectedNodePath, CSG_SELECTED_MAT, {
        attachGizmo: true,
        pickable: true,
        trackNodePreview: true,
      });
      // Show negatives within the selected subtree
      const negPaths = new Set(this.collectNegativeSubtreePaths(csgDef.tree, this.selectedNodePath));
      for (const path of this.collectSubtreePaths(csgDef.tree, this.selectedNodePath)) {
        if (negPaths.has(path)) continue;
        void this.resolveSubtreePreview(path, CSG_PICK_MAT, { pickable: true, trackNodePreview: false });
      }
      for (const path of negPaths) {
        void this.resolveSubtreePreview(path, CSG_NEGATIVE_MAT, { pickable: true, trackNodePreview: false });
      }
    } else {
      // Config 3: negative selection — full result visible, overlay selection
      this.editLevelObj.object.visible = true;
      void this.resolveSubtreePreview(this.selectedNodePath, CSG_NEGATIVE_MAT, {
        attachGizmo: true,
        pickable: true,
        trackNodePreview: true,
      });
      // Show nested negatives within the selected subtree in a third color
      const nestedNegPaths = new Set(this.collectNegativeSubtreePaths(csgDef.tree, this.selectedNodePath));
      for (const path of this.collectSubtreePaths(csgDef.tree, this.selectedNodePath)) {
        if (nestedNegPaths.has(path)) continue;
        void this.resolveSubtreePreview(path, CSG_PICK_MAT, { pickable: true, trackNodePreview: false });
      }
      for (const path of nestedNegPaths) {
        void this.resolveSubtreePreview(path, CSG_NESTED_NEGATIVE_MAT, {
          pickable: true,
          trackNodePreview: false,
        });
      }
    }
  }

  private rebuildPreviews() {
    const assetName = this.csgPanelState.assetName;
    if (!assetName) return;
    const csgDef = this.editor.levelDef.assets[assetName] as CsgAssetDef;
    this.nodePolarities = computeNodePolarities(csgDef.tree);
    this.csgPanelState.nodePolarities = this.nodePolarities;
    this.applyRenderConfig();
  }

  /** Build the ancestor transform matrix for a subtree path. */
  private buildAncestorMatrix(csgDef: CsgAssetDef, path: string): THREE.Matrix4 {
    const ancestorMatrix = new THREE.Matrix4();
    if (!path) return ancestorMatrix;

    const parts = path.split('.');
    for (let i = 0; i < parts.length; i++) {
      const ancestorPath = parts.slice(0, i).join('.');
      const ancestor = ancestorPath ? getNodeAtPath(csgDef.tree, ancestorPath) : csgDef.tree;
      if (isOpNode(ancestor)) {
        const [px = 0, py = 0, pz = 0] = ancestor.position ?? [];
        const [rx = 0, ry = 0, rz = 0] = ancestor.rotation ?? [];
        const [sx = 1, sy = 1, sz = 1] = ancestor.scale ?? [];
        if (
          px !== 0 ||
          py !== 0 ||
          pz !== 0 ||
          rx !== 0 ||
          ry !== 0 ||
          rz !== 0 ||
          sx !== 1 ||
          sy !== 1 ||
          sz !== 1
        ) {
          const m = new THREE.Matrix4();
          const euler = new THREE.Euler(rx, ry, rz, 'YXZ');
          m.compose(
            new THREE.Vector3(px, py, pz),
            new THREE.Quaternion().setFromEuler(euler),
            new THREE.Vector3(sx, sy, sz)
          );
          ancestorMatrix.multiply(m);
        }
      }
    }
    return ancestorMatrix;
  }

  private applyNodeTransform(
    object: THREE.Object3D,
    node: {
      position?: [number, number, number];
      rotation?: [number, number, number];
      scale?: [number, number, number];
    }
  ) {
    const [px = 0, py = 0, pz = 0] = node.position ?? [];
    const [rx = 0, ry = 0, rz = 0] = node.rotation ?? [];
    const [sx = 1, sy = 1, sz = 1] = node.scale ?? [];
    object.position.set(px, py, pz);
    object.rotation.set(rx, ry, rz, 'YXZ');
    object.scale.set(sx, sy, sz);
  }

  private applyMatrixTransform(object: THREE.Object3D, matrix: THREE.Matrix4) {
    const position = new THREE.Vector3();
    const quaternion = new THREE.Quaternion();
    const scale = new THREE.Vector3();
    matrix.decompose(position, quaternion, scale);
    object.position.copy(position);
    object.quaternion.copy(quaternion);
    object.scale.copy(scale);
  }

  private getPreviewRuntime() {
    if (!this.previewWorkerManager || !this.previewRepl || !this.previewCtxPtrPromise) {
      this.previewWorkerManager = new WorkerManager();
      this.previewRepl = this.previewWorkerManager.getWorker();
      this.previewCtxPtrPromise = this.previewRepl.init();
    }

    return {
      repl: this.previewRepl,
      ctxPtrPromise: this.previewCtxPtrPromise,
    };
  }

  private getAssetRuntime() {
    if (!this.assetWorkerManager || !this.assetRepl || !this.assetCtxPtrPromise) {
      this.assetWorkerManager = new WorkerManager();
      this.assetRepl = this.assetWorkerManager.getWorker();
      this.assetCtxPtrPromise = this.assetRepl.init();
    }

    return {
      repl: this.assetRepl,
      ctxPtrPromise: this.assetCtxPtrPromise,
    };
  }

  private terminatePreviewWorker() {
    if (this.previewWorkerManager) {
      this.previewWorkerManager.terminate();
    }
    this.previewWorkerManager = null;
    this.previewRepl = null;
    this.previewCtxPtrPromise = null;
    this.previewResolveQueue = Promise.resolve();
  }

  private terminateAssetWorker() {
    if (this.assetWorkerManager) {
      this.assetWorkerManager.terminate();
    }
    this.assetWorkerManager = null;
    this.assetRepl = null;
    this.assetCtxPtrPromise = null;
  }

  private queuePreviewResolve<T>(task: () => Promise<T>): Promise<T> {
    const next = this.previewResolveQueue.then(task, task);
    this.previewResolveQueue = next.then(
      () => undefined,
      () => undefined
    );
    return next;
  }

  private syncResolvedPreviewTransforms(tree: CsgTreeNode) {
    const assetName = this.csgPanelState.assetName;
    if (!assetName) return;
    const csgDef = this.editor.levelDef.assets[assetName] as CsgAssetDef;

    for (const [path, entry] of this.resolvedPreviews) {
      if (path === this.selectedNodePath) continue;
      const node = getNodeAtPath(tree, path);
      this.applyMatrixTransform(entry.wrapper, this.buildAncestorMatrix(csgDef, path));
      this.applyNodeTransform(entry.preview, node);
    }
  }

  /**
   * Resolve a subtree at `path` and add it as a preview mesh to the CSG edit group.
   * If `attachGizmo` is true, also attaches the transform controls to it.
   */
  private async resolveSubtreePreview(
    path: string,
    material: THREE.Material,
    options: { attachGizmo?: boolean; pickable?: boolean; trackNodePreview?: boolean } = {}
  ) {
    const attachGizmo = options.attachGizmo ?? false;
    const pickable = options.pickable ?? true;
    const trackNodePreview = options.trackNodePreview ?? attachGizmo;
    const generation = this.configGeneration;
    const assetName = this.csgPanelState.assetName;
    if (!assetName || !this.editGroup) return;

    const csgDef = this.editor.levelDef.assets[assetName] as CsgAssetDef;
    const { modules: subModules, code: subCode } = generateSubtreeCode(
      csgDef,
      path,
      this.editor.levelDef.assets
    );

    const modules = { ...subModules, code: subCode };
    const renderWrapper = 'import { mesh } from "code"\nmesh | render';

    let result;
    try {
      result = await this.queuePreviewResolve(async () => {
        if (generation !== this.configGeneration || !this._isActive || !this.editGroup) {
          return null;
        }

        const { repl, ctxPtrPromise } = this.getPreviewRuntime();
        const ctxPtr = await ctxPtrPromise;
        return runGeoscript({
          code: renderWrapper,
          ctxPtr,
          repl,
          includePrelude: false,
          modules,
        });
      });
    } catch (error) {
      console.error(`[CsgEditController] Subtree resolve failed for "${path}":`, error);
      this.terminatePreviewWorker();
      return;
    }

    if (!result) return;

    if (result.error) {
      console.error(`[CsgEditController] Subtree resolve failed for "${path}":`, result.error);
      return;
    }

    // Bail if the config changed while we were resolving
    if (generation !== this.configGeneration || !this._isActive || !this.editGroup) return;

    const meshes: THREE.Mesh[] = [];
    for (const obj of result.objects) {
      if (obj.type !== 'mesh') continue;
      const mesh = new THREE.Mesh(obj.geometry, material);
      mesh.applyMatrix4(obj.transform);
      meshes.push(mesh);
    }
    if (meshes.length === 0) return;

    const preview =
      meshes.length === 1
        ? (meshes[0] as THREE.Object3D)
        : (() => {
            const g = new THREE.Group();
            meshes.forEach(m => g.add(m));
            return g;
          })();

    // Use a wrapper group for the ancestor transform so the preview's own
    // local transform represents only this node's transform.  This lets
    // TransformControls read/write the node transform without conflating
    // ancestor contributions.
    const wrapper = new THREE.Group();
    wrapper.applyMatrix4(this.buildAncestorMatrix(csgDef, path));
    this.editGroup.add(wrapper);

    // Set the node's own transform as the preview's local transform
    const node = getNodeAtPath(csgDef.tree, path);
    this.applyNodeTransform(preview, node);

    wrapper.add(preview);
    this.resolvedPreviews.set(path, { wrapper, preview });
    if (trackNodePreview) {
      this.nodePreviews.set(path, preview);
    }
    preview.traverse(child => {
      if (child instanceof THREE.Mesh) {
        if (pickable) {
          this.selectableMeshes.push(child);
          this.meshToNodePath.set(child, path);
        }
      }
    });

    if (attachGizmo) {
      this.editor.transformControls?.attach(preview);
    }
  }

  /**
   * Resolve the complement (full tree minus selected subtree) and show as solid geometry.
   * Used in Config 2 (positive selection) to provide visual context.
   */
  private async resolveComplementPreview(excludePath: string) {
    const generation = this.configGeneration;
    const assetName = this.csgPanelState.assetName;
    if (!assetName || !this.editGroup) return;

    const csgDef = this.editor.levelDef.assets[assetName] as CsgAssetDef;
    const complementResult = generateComplementCode(csgDef, excludePath, this.editor.levelDef.assets);
    if (!complementResult) return; // selected root — no complement

    const modules = { ...complementResult.modules, code: complementResult.code };
    const renderWrapper = 'import { mesh } from "code"\nmesh | render';

    let result;
    try {
      result = await this.queuePreviewResolve(async () => {
        if (generation !== this.configGeneration || !this._isActive || !this.editGroup) {
          return null;
        }

        const { repl, ctxPtrPromise } = this.getPreviewRuntime();
        const ctxPtr = await ctxPtrPromise;
        return runGeoscript({
          code: renderWrapper,
          ctxPtr,
          repl,
          includePrelude: false,
          modules,
        });
      });
    } catch (error) {
      console.error(`[CsgEditController] Complement resolve failed:`, error);
      this.terminatePreviewWorker();
      return;
    }

    if (!result) return;

    if (result.error) {
      console.error(`[CsgEditController] Complement resolve failed:`, result.error);
      return;
    }

    if (generation !== this.configGeneration || !this._isActive || !this.editGroup) return;

    const meshes: THREE.Mesh[] = [];
    // Apply the level object's material if available
    const levelMat = this.editLevelObj?.def.material
      ? (this.editor.builtMaterials.get(this.editLevelObj.def.material) ?? LEVEL_PLACEHOLDER_MAT)
      : LEVEL_PLACEHOLDER_MAT;
    for (const obj of result.objects) {
      if (obj.type !== 'mesh') continue;
      const mesh = new THREE.Mesh(obj.geometry, levelMat);
      mesh.applyMatrix4(obj.transform);
      meshes.push(mesh);
    }
    if (meshes.length === 0) return;

    // Remove old complement if any
    if (this.complementPreview) {
      this.editGroup.remove(this.complementPreview);
    }

    const complement =
      meshes.length === 1
        ? (meshes[0] as THREE.Object3D)
        : (() => {
            const g = new THREE.Group();
            meshes.forEach(m => g.add(m));
            return g;
          })();

    this.complementPreview = complement;
    this.editGroup.add(complement);
    // Complement is not selectable — purely visual context
  }

  // ---------------------------------------------------------------------------
  // Tree changes and live resolve
  // ---------------------------------------------------------------------------

  private async onTreeChange(tree: CsgTreeNode, pushUndo = true) {
    const assetName = this.csgPanelState.assetName;
    if (!assetName) return;

    // Push undo entry for structural changes
    const csgDef = this.editor.levelDef.assets[assetName] as CsgAssetDef;
    if (pushUndo && csgDef.tree) {
      this.undoSystem.push({ type: 'tree', before: cloneTree(csgDef.tree), after: cloneTree(tree) });
    }

    csgDef.tree = tree;
    this.csgPanelState.tree = tree;

    // Structural change — rebuild previews
    this.rebuildPreviews();

    // Save to server
    this.editor.api.saveCsgTree(assetName, tree);

    // Re-resolve the result mesh
    await this.reResolveCsgAsset(assetName);
  }

  /** Called during drag (TransformControls objectChange) for live preview */
  private onNodeTransformUpdate() {
    if (!this.selectedNodePath || !this.csgPanelState.assetName || !this.csgPanelState.tree) return;

    const preview = this.nodePreviews.get(this.selectedNodePath);
    if (!preview) return;

    // Read local transform from the preview mesh and write back to the tree node
    const round = (n: number) => Math.round(n * 10000) / 10000;
    const tree = cloneTree(this.csgPanelState.tree);
    const node = getNodeAtPath(tree, this.selectedNodePath);
    node.position = preview.position.toArray().map(round) as [number, number, number];
    node.rotation = [preview.rotation.x, preview.rotation.y, preview.rotation.z].map(round) as [
      number,
      number,
      number,
    ];
    node.scale = preview.scale.toArray().map(round) as [number, number, number];

    const csgDef = this.editor.levelDef.assets[this.csgPanelState.assetName] as CsgAssetDef;
    csgDef.tree = tree;
    this.csgPanelState.tree = tree;
    this.syncResolvedPreviewTransforms(tree);

    this.triggerLiveResolve();
  }

  private async triggerLiveResolve() {
    if (this.resolveInFlight) {
      this.resolvePending = true;
      return;
    }

    const assetName = this.csgPanelState.assetName;
    if (!assetName) return;

    this.resolveInFlight = true;

    await this.reResolveCsgAsset(assetName);

    this.resolveInFlight = false;

    if (this.resolvePending) {
      this.resolvePending = false;
      this.triggerLiveResolve();
    }
  }

  async convertToCsg(objectId: string) {
    const result = await this.editor.api.convertToCsg(objectId);
    if (!result) return;

    const { csgAssetName, tree } = result;
    this.editor.levelDef.assets[csgAssetName] = { type: 'csg', tree } as any;

    const levelObj = this.editor.allLevelObjects.find((o: LevelObject) => o.id === objectId);
    if (levelObj) {
      levelObj.assetId = csgAssetName;
      levelObj.def.asset = csgAssetName;

      const originalAssetId = (tree as any).asset;
      const originalPrototype = this.editor.prototypes.get(originalAssetId);
      if (originalPrototype) {
        this.editor.prototypes.set(csgAssetName, originalPrototype);
      }

      this.editor.select(levelObj);
    }
  }

  async reResolveCsgAsset(assetId: string) {
    this.assetResolveQueuedAssetId = assetId;
    const requestId = ++this.assetResolveRequestId;
    this.assetResolveLatestQueuedRequestId = requestId;

    if (!this.assetResolveDrainPromise) {
      this.assetResolveDrainPromise = this.drainAssetResolveQueue();
    }

    await this.assetResolveDrainPromise;
  }

  private async drainAssetResolveQueue() {
    while (this.assetResolveQueuedAssetId) {
      const assetId = this.assetResolveQueuedAssetId;
      const requestId = this.assetResolveLatestQueuedRequestId;
      this.assetResolveQueuedAssetId = null;
      await this.performCsgAssetResolve(assetId, requestId);
    }
    this.assetResolveDrainPromise = null;
  }

  private async performCsgAssetResolve(assetId: string, requestId: number) {
    const csgDef = this.editor.levelDef.assets[assetId] as CsgAssetDef;
    const { modules: csgModules, code: csgCode } = generateCsgCode(csgDef, this.editor.levelDef.assets);

    const modules = { ...csgModules, code: csgCode };
    const renderWrapper = 'import { mesh } from "code"\nmesh | render';

    let result;
    try {
      const { repl, ctxPtrPromise } = this.getAssetRuntime();
      const ctxPtr = await ctxPtrPromise;
      result = await runGeoscript({
        code: renderWrapper,
        ctxPtr,
        repl,
        includePrelude: false,
        modules,
      });
    } catch (error) {
      console.error(`[CsgEditController] CSG re-resolve failed:`, error);
      this.terminateAssetWorker();
      return;
    }

    if (result.error) {
      console.error(`[CsgEditController] CSG re-resolve failed:`, result.error);
      return;
    }

    if (requestId !== this.assetResolveLatestQueuedRequestId) {
      return;
    }

    const meshes: THREE.Mesh[] = [];
    for (const obj of result.objects) {
      if (obj.type !== 'mesh') continue;
      const mesh = new THREE.Mesh(obj.geometry, LEVEL_PLACEHOLDER_MAT);
      mesh.applyMatrix4(obj.transform);
      meshes.push(mesh);
    }
    if (meshes.length === 0) {
      console.warn(`[CsgEditController] CSG asset "${assetId}" produced no meshes`);
      return;
    }

    const newPrototype =
      meshes.length === 1
        ? meshes[0]
        : (() => {
            const g = new THREE.Group();
            meshes.forEach(m => g.add(m));
            return g;
          })();

    this.editor.prototypes.set(assetId, newPrototype);

    for (const levelObj of this.editor.allLevelObjects) {
      if (levelObj.assetId !== assetId) continue;

      // In CSG edit mode, don't re-register for normal raycast (previews handle selection)
      if (this._isActive && levelObj === this.editLevelObj) {
        const wasVisible = levelObj.object.visible;
        this.editor.removePhysics(levelObj);
        this.editor.viz.scene.remove(levelObj.object);
        const clone = instantiateLevelObject(newPrototype, levelObj.def, {
          builtMaterials: this.editor.builtMaterials,
          fallbackMaterial: LEVEL_PLACEHOLDER_MAT,
          // Preserve visibility state from current config (hidden in Config 2)
          visible: wasVisible,
        });
        levelObj.object = clone;
        this.editor.viz.scene.add(clone);
        this.editor.syncPhysics(levelObj);
        if (this.selectedNodePath === '') {
          this.editor.transformControls?.attach(clone);
        }
        continue;
      }

      this.editor.unregisterMeshes(levelObj);
      this.editor.removePhysics(levelObj);
      this.editor.viz.scene.remove(levelObj.object);

      const clone = instantiateLevelObject(newPrototype, levelObj.def, {
        builtMaterials: this.editor.builtMaterials,
        fallbackMaterial: LEVEL_PLACEHOLDER_MAT,
      });

      levelObj.object = clone;
      this.editor.viz.scene.add(clone);
      this.editor.registerMeshes(levelObj);
      this.editor.syncPhysics(levelObj);

      if (this.editor.selectedObject === levelObj) {
        this.editor.transformControls?.attach(clone);
      }
    }
  }
}
