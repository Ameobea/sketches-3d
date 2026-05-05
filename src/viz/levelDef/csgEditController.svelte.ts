import * as THREE from 'three';

import type { CsgAssetDef, CsgTreeNode, CsgOpNode } from './types';
import type { LevelObject } from './loadLevelDef';
import { cloneTree, getNodeAtPath, insertAfterPath, isOpNode, splitPath } from './csgTreeUtils';
import type { LevelEditor } from './LevelEditor.svelte';
import { UndoSystem } from './undoSystem';
import { round } from './mathUtils';
import { CsgResolveRuntime } from './csgResolveRuntime';
import { CsgEditorPanelController } from './csgEditorPanelController.svelte';
import { CsgPreviewScene } from './csgPreviewScene';
import { CsgAssetResolver } from './csgAssetResolver';
import { detachSubtree } from './editorStructuralOps';
import { isLevelGroup } from './levelSceneTypes';

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

export class CsgEditController {
  private _isActive = false;
  private editLevelObj: LevelObject | null = null;
  private editGroup: THREE.Group | null = null;
  private editGroupParent: THREE.Object3D | null = null;
  private selectedNodePath: string | null = null;

  private readonly undoSystem = new UndoSystem<CsgUndoEntry>();
  private treeBeforeDrag: CsgTreeNode | null = null;
  private rootTransformBeforeDrag: {
    position: TransformTuple;
    rotation: TransformTuple;
    scale: TransformTuple;
  } | null = null;

  private readonly runtime: CsgResolveRuntime;
  private readonly panelController: CsgEditorPanelController;
  private readonly previewScene: CsgPreviewScene;
  private readonly assetResolver: CsgAssetResolver;

  /** Single-node clipboard for CSG copy/paste. Persists across edit sessions. */
  private csgClipboard: CsgTreeNode | null = null;

  constructor(private readonly editor: LevelEditor) {
    this.runtime = new CsgResolveRuntime();
    this.panelController = new CsgEditorPanelController(
      tree => this.onTreeChange(tree),
      path => {
        if (path !== null) this.selectNode(path);
        else this.deselectNode();
      },
      () => this.exit()
    );
    this.previewScene = new CsgPreviewScene(editor, this.runtime);
    this.assetResolver = new CsgAssetResolver(editor, this.runtime);
  }

  get isActive(): boolean {
    return this._isActive;
  }

  get editingLevelObj(): LevelObject | null {
    return this.editLevelObj;
  }

  get isEditorOpen(): boolean {
    return this.panelController.isOpen;
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

  /** Handle start of a transform drag in CSG edit mode — snapshot for undo. */
  onDragStart() {
    if (this.selectedNodePath === '') {
      this.rootTransformBeforeDrag = this.snapshotRootTransform();
    } else if (this.panelController.tree) {
      this.treeBeforeDrag = cloneTree(this.panelController.tree);
    }
  }

  /** Handle end of a transform drag in CSG edit mode. */
  onDragEnd() {
    if (this.selectedNodePath === null || !this.panelController.assetName || !this.panelController.tree) {
      return;
    }

    if (this.selectedNodePath === '') {
      // Root selected — transform applies to the level object
      if (this.editLevelObj) {
        const obj = this.editLevelObj.object;
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
      this.onNodeTransformUpdate(); // final writeback
      this.editor.api.saveCsgTree(this.panelController.assetName, this.panelController.tree);

      if (this.treeBeforeDrag) {
        this.undoSystem.push({
          type: 'tree',
          before: this.treeBeforeDrag,
          after: cloneTree(this.panelController.tree),
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

  /**
   * Resolve the Three.js object the camera should focus on for the current
   * selection (used by the "." keybind). Falls back to the level object when
   * nothing is selected or the selected sub-node has no preview yet.
   */
  getFocusTarget(): THREE.Object3D | null {
    if (!this.editLevelObj) return null;
    if (this.selectedNodePath === '' || this.selectedNodePath === null) {
      return this.editLevelObj.object;
    }
    return this.previewScene.getNodePreview(this.selectedNodePath) ?? this.editLevelObj.object;
  }

  /**
   * Copy the currently selected sub-tree into the CSG clipboard. No-op when
   * nothing is selected. Selecting the root copies the entire tree.
   */
  copySelectedNode(): void {
    if (!this._isActive || this.selectedNodePath === null) return;
    const tree = this.panelController.tree;
    if (!tree) return;
    const node = getNodeAtPath(tree, this.selectedNodePath);
    this.csgClipboard = cloneTree(node);
  }

  get hasClipboard(): boolean {
    return this.csgClipboard !== null;
  }

  /**
   * Insert the clipboard sub-tree near the current selection:
   * - leaf selected: insert as a sibling immediately after it
   * - op selected: append as a child
   * - root or nothing selected: append to root op, or wrap a leaf root in a union
   *
   * The newly inserted node becomes the selection.
   */
  async pasteNode(): Promise<void> {
    if (!this._isActive || !this.csgClipboard) return;
    const tree = this.panelController.tree;
    if (!tree) return;

    const newNode = cloneTree(this.csgClipboard);
    let newTree: CsgTreeNode;
    let newSelectedPath: string;

    if (this.selectedNodePath === null || this.selectedNodePath === '') {
      if (isOpNode(tree)) {
        const cloned = cloneTree(tree) as CsgOpNode;
        cloned.children.push(newNode);
        newTree = cloned;
        newSelectedPath = `${cloned.children.length - 1}`;
      } else {
        newTree = { op: 'union', children: [cloneTree(tree), newNode] };
        newSelectedPath = '1';
      }
    } else {
      const selected = getNodeAtPath(tree, this.selectedNodePath);
      if (isOpNode(selected)) {
        const cloned = cloneTree(tree);
        const target = getNodeAtPath(cloned, this.selectedNodePath) as CsgOpNode;
        target.children.push(newNode);
        newTree = cloned;
        newSelectedPath = `${this.selectedNodePath}.${target.children.length - 1}`;
      } else {
        const info = splitPath(this.selectedNodePath)!;
        newTree = insertAfterPath(tree, this.selectedNodePath, newNode);
        newSelectedPath = info.parentPath
          ? `${info.parentPath}.${info.childIndex + 1}`
          : `${info.childIndex + 1}`;
      }
    }

    // Update selection ahead of onTreeChange so its applyRenderConfig pass
    // already targets the freshly-pasted node.
    this.selectedNodePath = newSelectedPath;
    this.panelController.setSelectedNodePath(newSelectedPath);
    await this.onTreeChange(newTree);
  }

  /** Raycast against CSG node previews. Returns true if handled. */
  doRaycast(raycaster: THREE.Raycaster): boolean {
    if (!this._isActive) return false;
    const hit = this.previewScene.pickNode(raycaster);
    if (hit !== undefined) {
      this.selectNode(hit);
    } else {
      this.deselectNode();
    }
    return true;
  }

  enter(levelObj: LevelObject) {
    if (this._isActive) this.exit();

    this._isActive = true;
    this.undoSystem.clear();
    this.editLevelObj = levelObj;
    this.editor.selectedNode = levelObj;
    this.editor.transformControls?.detach();

    const assetDef = this.editor.levelDef.assets[levelObj.assetId] as CsgAssetDef;

    this.editGroup = new THREE.Group();
    const obj = levelObj.object;
    this.editGroup.position.copy(obj.position);
    this.editGroup.rotation.copy(obj.rotation);
    this.editGroup.scale.copy(obj.scale);
    // Parent to the same parent as the level object so editGroup inherits any
    // ancestor group transforms — copying only the local TRS would otherwise
    // place the previews at the wrong world position when the level object is
    // nested inside an object group.
    this.editGroupParent = obj.parent ?? this.editor.viz.scene;
    this.editGroupParent.add(this.editGroup);

    this.previewScene.activate(this.editGroup, levelObj);

    this.panelController.open(levelObj.assetId, assetDef.tree, this.editor.levelDef);
    this.selectNode('');
    this.editor.updateSelectionState();
  }

  exit() {
    if (!this._isActive) return;

    if (this.panelController.assetName && this.panelController.tree) {
      this.editor.api.saveCsgTree(this.panelController.assetName, this.panelController.tree);
    }

    this.editor.transformControls?.detach();
    this.selectedNodePath = null;
    this.previewScene.deactivate();

    if (this.editLevelObj) {
      this.editLevelObj.object.visible = true;
      this.editor.syncPhysics(this.editLevelObj);
    }

    if (this.editGroup) {
      (this.editGroupParent ?? this.editor.viz.scene).remove(this.editGroup);
      this.editGroup = null;
      this.editGroupParent = null;
    }

    this.runtime.terminatePreviewWorker();
    this.runtime.terminateAssetWorker();

    this._isActive = false;
    const levelObj = this.editLevelObj;
    this.editLevelObj = null;

    this.panelController.close();

    if (levelObj) {
      this.editor.transformControls?.attach(levelObj.object);
    }
  }

  closeEditor() {
    this.panelController.close();
  }

  /**
   * Convert one or more selected leaf objects to a single CSG-asset object.
   *
   * Single-object: equivalent to the original conversion — the new tree is one
   * leaf carrying the source's rotation + scale.
   *
   * Multi-object: the server unifies the inputs into a `union` op tree
   * (rooted at the first object's position) and deletes the other objects.
   * The remaining (primary) level object is updated to reference the new
   * CSG asset, and a re-resolve is triggered so the visible mesh reflects
   * the union geometry.
   *
   * Not undoable (matches the single-object behavior). Undo entries that
   * reference the deleted nodes are purged.
   */
  async convertToCsg(objectIds: string[]) {
    if (objectIds.length === 0) return;

    const result = await this.editor.api.convertToCsg(objectIds);
    if (!result) return;

    const { csgAssetName, tree, primaryId, deletedIds } = result;
    this.editor.levelDef.assets[csgAssetName] = { type: 'csg', tree } as any;

    const primary = this.editor.allLevelObjects.find((o: LevelObject) => o.id === primaryId);
    if (!primary) return;

    // Detach the deleted (non-primary) objects from the editor's scene/physics/tracking.
    if (deletedIds.length > 0) {
      const deletedNodeSet = new Set<string>(deletedIds);
      for (const id of deletedIds) {
        const node = this.editor.nodeById.get(id);
        if (node && !isLevelGroup(node)) {
          detachSubtree(this.editor, node);
        }
      }
      // Stale undo entries referencing deleted nodes would mis-target after this op.
      this.editor.purgeUndoForNodeIds(deletedNodeSet);
      this.editor.selection.state.treeVersion++;
    }

    primary.assetId = csgAssetName;
    primary.def.asset = csgAssetName;

    // Strip rotation + scale from the level object — they've been baked into
    // the root tree node(s) by the server.
    primary.object.rotation.set(0, 0, 0);
    primary.object.scale.set(1, 1, 1);
    delete primary.def.rotation;
    delete primary.def.scale;

    // Register a placeholder prototype under the new asset name so the level
    // object continues to render until the asynchronous re-resolve completes.
    // For single-object: the source asset's prototype is correct as-is.
    // For multi-object: any of the input prototypes works as a stand-in.
    const findFirstLeafAsset = (node: CsgTreeNode): string | undefined => {
      if ('asset' in node) return node.asset;
      for (const child of node.children) {
        const a = findFirstLeafAsset(child);
        if (a) return a;
      }
      return undefined;
    };
    const placeholderSourceAsset = findFirstLeafAsset(tree);
    if (placeholderSourceAsset) {
      const proto = this.editor.prototypes.get(placeholderSourceAsset);
      if (proto && !this.editor.prototypes.has(csgAssetName)) {
        this.editor.prototypes.set(csgAssetName, proto);
      }
    }

    this.editor.select(primary);

    // For multi-object, the unioned geometry differs from any single input
    // prototype, so kick off a re-resolve to replace the placeholder.
    if (deletedIds.length > 0) {
      void this.assetResolver.reResolveCsgAsset(csgAssetName);
    }
  }

  private selectNode(path: string) {
    this.selectedNodePath = path;
    this.panelController.setSelectedNodePath(path);
    this.previewScene.applyRenderConfig(path, this.panelController.assetName);
    this.panelController.updateNodePolarities(this.previewScene.nodePolarities);
  }

  private deselectNode() {
    this.selectedNodePath = null;
    this.panelController.setSelectedNodePath(null);
    this.previewScene.applyRenderConfig(null, this.panelController.assetName);
    this.panelController.updateNodePolarities(this.previewScene.nodePolarities);
  }

  private async onTreeChange(tree: CsgTreeNode, pushUndo = true) {
    const assetName = this.panelController.assetName;
    if (!assetName) return;

    const csgDef = this.editor.levelDef.assets[assetName] as CsgAssetDef;
    if (pushUndo && csgDef.tree) {
      this.undoSystem.push({ type: 'tree', before: cloneTree(csgDef.tree), after: cloneTree(tree) });
    }

    csgDef.tree = tree;
    this.panelController.updateTree(tree);
    this.previewScene.applyRenderConfig(this.selectedNodePath, assetName);
    this.panelController.updateNodePolarities(this.previewScene.nodePolarities);

    this.editor.api.saveCsgTree(assetName, tree);

    await this.assetResolver.reResolveCsgAsset(assetName, this.editLevelObj, this.selectedNodePath);
  }

  /** Called during drag (TransformControls objectChange) for live preview. */
  private onNodeTransformUpdate() {
    if (!this.selectedNodePath || !this.panelController.assetName || !this.panelController.tree) return;

    const preview = this.previewScene.getNodePreview(this.selectedNodePath);
    if (!preview) return;

    const tree = cloneTree(this.panelController.tree);
    const node = getNodeAtPath(tree, this.selectedNodePath);
    node.position = preview.position.toArray().map(round) as [number, number, number];
    node.rotation = [preview.rotation.x, preview.rotation.y, preview.rotation.z].map(round) as [
      number,
      number,
      number,
    ];
    node.scale = preview.scale.toArray().map(round) as [number, number, number];

    const csgDef = this.editor.levelDef.assets[this.panelController.assetName] as CsgAssetDef;
    csgDef.tree = tree;
    this.panelController.updateTree(tree);
    this.previewScene.syncResolvedPreviewTransforms(
      tree,
      this.selectedNodePath,
      this.panelController.assetName
    );

    this.triggerLiveResolve();
  }

  private triggerLiveResolve() {
    const assetName = this.panelController.assetName;
    if (!assetName) return;
    void this.assetResolver.reResolveCsgAsset(assetName, this.editLevelObj, this.selectedNodePath);
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
}
