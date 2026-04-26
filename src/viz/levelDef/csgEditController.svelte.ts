import * as THREE from 'three';

import type { CsgAssetDef, CsgTreeNode } from './types';
import type { LevelObject } from './loadLevelDef';
import { cloneTree, getNodeAtPath } from './csgTreeUtils';
import type { LevelEditor } from './LevelEditor.svelte';
import { UndoSystem } from './undoSystem';
import { round } from './mathUtils';
import { CsgResolveRuntime } from './csgResolveRuntime';
import { CsgEditorPanelController } from './csgEditorPanelController.svelte';
import { CsgPreviewScene } from './csgPreviewScene';
import { CsgAssetResolver } from './csgAssetResolver';

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

  async convertToCsg(objectId: string) {
    const result = await this.editor.api.convertToCsg(objectId);
    if (!result) return;

    const { csgAssetName, tree } = result;
    this.editor.levelDef.assets[csgAssetName] = { type: 'csg', tree } as any;

    const levelObj = this.editor.allLevelObjects.find((o: LevelObject) => o.id === objectId);
    if (levelObj) {
      levelObj.assetId = csgAssetName;
      levelObj.def.asset = csgAssetName;

      // Strip rotation + scale from the level object — they've been moved into
      // the root leaf node of the CSG tree by the server.
      levelObj.object.rotation.set(0, 0, 0);
      levelObj.object.scale.set(1, 1, 1);
      delete levelObj.def.rotation;
      delete levelObj.def.scale;

      const originalAssetId = (tree as any).asset;
      const originalPrototype = this.editor.prototypes.get(originalAssetId);
      if (originalPrototype) {
        this.editor.prototypes.set(csgAssetName, originalPrototype);
      }

      this.editor.select(levelObj);
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
