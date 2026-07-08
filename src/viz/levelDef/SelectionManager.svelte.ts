import type { LightDef } from './types';
import type { LevelGroup, LevelLight, LevelObject, LevelSceneNode } from './levelSceneTypes';
import { isLevelGroup, isEditable } from './levelSceneTypes';
import type { AssetLibFolder } from './assetLibTypes';

/**
 * Manages selection state for the level editor, including multi-selection.
 * The selection is an ordered array of scene nodes; the last element is the
 * "primary" node — used for the info panel, transform anchor, etc.
 * Light selection (`selectedLight`) is tracked separately and is single-select.
 */
export class SelectionManager {
  private _selectedNodes: LevelSceneNode[] = [];
  selectedLight: LevelLight | null = null;

  /** Reactive state driving the Svelte UI panels. */
  readonly state = $state({
    selectedNodeIds: [] as string[],
    /** ID of the primary (last-selected) node, or null. */
    nodeId: null as string | null,
    materialId: null as string | null,
    isGroup: false,
    isGenerated: false,
    isCsgAsset: false,
    position: [0, 0, 0] as [number, number, number],
    rotation: [0, 0, 0] as [number, number, number],
    scale: [1, 1, 1] as [number, number, number],
    /** When non-null, a light is selected instead of a scene node. */
    selectedLightDef: null as LightDef | null,
    lightPosition: [0, 0, 0] as [number, number, number],
    /** Incremented whenever rootNodes changes — triggers hierarchy panel re-render. */
    treeVersion: 0,
    /** Incremented on gizmo-handle arm/value/ghost changes — triggers handle panel re-render. */
    gizmosVersion: 0,
    /** Incremented when a new asset is added — triggers asset list re-render. */
    assetsVersion: 0,
    libFolders: [] as AssetLibFolder[],
    materialLibFolders: [] as AssetLibFolder[],
  });

  get selectedNodes(): readonly LevelSceneNode[] {
    return this._selectedNodes;
  }

  get count(): number {
    return this._selectedNodes.length;
  }

  get primaryNode(): LevelSceneNode | null {
    return this._selectedNodes.length > 0 ? this._selectedNodes[this._selectedNodes.length - 1] : null;
  }

  get primaryObject(): LevelObject | null {
    const node = this.primaryNode;
    return node && !isLevelGroup(node) ? node : null;
  }

  get isSingle(): boolean {
    return this._selectedNodes.length === 1;
  }

  get isMulti(): boolean {
    return this._selectedNodes.length > 1;
  }

  /**
   * True when all selected nodes share the same parent (enabling grouping).
   * Requires at least 2 editable (non-generated) nodes.
   * Uses the nodeById map to identify each node's parent group.
   */
  canGroupWith(
    nodeById: Map<string, import('./levelSceneTypes').LevelSceneNode>,
    rootNodes: import('./levelSceneTypes').LevelSceneNode[]
  ): boolean {
    const editable = this._selectedNodes.filter(isEditable);
    if (editable.length < 2) return false;
    return this.haveSharedParent(nodeById, rootNodes);
  }

  /**
   * True when every selected node shares the same parent. Vacuously true
   * for selections of 0 or 1. Unlike `canGroupWith`, this does not require
   * a minimum count or filter by `generated`.
   */
  haveSharedParent(
    nodeById: Map<string, import('./levelSceneTypes').LevelSceneNode>,
    rootNodes: import('./levelSceneTypes').LevelSceneNode[]
  ): boolean {
    if (this._selectedNodes.length < 2) return true;

    const getParentId = (node: import('./levelSceneTypes').LevelSceneNode): string | null => {
      if (rootNodes.includes(node)) return null;
      for (const [id, candidate] of nodeById) {
        if (isLevelGroup(candidate) && candidate.children.includes(node)) return id;
      }
      return null;
    };

    const firstParentId = getParentId(this._selectedNodes[0]);
    return this._selectedNodes.every(n => getParentId(n) === firstParentId);
  }

  select(node: LevelSceneNode) {
    this.selectedLight = null;
    this._selectedNodes = [node];
  }

  /**
   * Ctrl+click behavior. Adding a group removes any descendants already in the
   * selection; adding a child removes any ancestor groups.
   */
  toggleSelect(node: LevelSceneNode) {
    this.selectedLight = null;

    const idx = this._selectedNodes.indexOf(node);
    if (idx !== -1) {
      this._selectedNodes = this._selectedNodes.filter((_, i) => i !== idx);
      return;
    }

    this._selectedNodes = this._selectedNodes.filter(existing => {
      if (isLevelGroup(existing) && this.isDescendantOf(node, existing)) return false;
      if (isLevelGroup(node) && this.isDescendantOf(existing, node)) return false;
      return true;
    });

    this._selectedNodes.push(node);
  }

  deselect() {
    this._selectedNodes = [];
  }

  selectMany(nodes: LevelSceneNode[]) {
    this.selectedLight = null;
    this._selectedNodes = [...nodes];
  }

  deselectLight() {
    this.selectedLight = null;
  }

  selectLight(light: LevelLight) {
    this._selectedNodes = [];
    this.selectedLight = light;
  }

  clear() {
    this._selectedNodes = [];
    this.selectedLight = null;
  }

  /** `isCsgAsset` is passed in because it depends on the CSG controller's state. */
  syncState(opts: { isCsgAsset: boolean }) {
    if (this.selectedLight) {
      this.state.selectedNodeIds = [];
      this.state.nodeId = null;
      this.state.materialId = null;
      this.state.isGroup = false;
      this.state.isGenerated = false;
      this.state.isCsgAsset = false;
      this.state.selectedLightDef = this.selectedLight.def;
      this.syncLightPosition();
      return;
    }

    const node = this.primaryNode;
    this.state.selectedNodeIds = this._selectedNodes.map(n => n.id);
    this.state.nodeId = node?.id ?? null;
    this.state.materialId = this.primaryObject?.def?.material ?? null;
    this.state.isGroup = node ? isLevelGroup(node) : false;
    this.state.isGenerated = node ? node.generated : false;
    this.state.isCsgAsset = opts.isCsgAsset;
    this.state.selectedLightDef = null;
    this.syncTransformDisplay();
  }

  syncTransformDisplay() {
    const node = this.primaryNode;
    if (!node) {
      this.state.position = [0, 0, 0];
      this.state.rotation = [0, 0, 0];
      this.state.scale = [1, 1, 1];
      return;
    }
    const obj = node.object;
    const r = obj.rotation;
    this.state.position = obj.position.toArray() as [number, number, number];
    this.state.rotation = [r.x, r.y, r.z];
    this.state.scale = obj.scale.toArray() as [number, number, number];
  }

  syncLightPosition() {
    if (!this.selectedLight) return;
    const pos = this.selectedLight.light.position;
    this.state.lightPosition = [pos.x, pos.y, pos.z];
  }

  private isDescendantOf(node: LevelSceneNode, group: LevelGroup): boolean {
    for (const child of group.children) {
      if (child === node) return true;
      if (isLevelGroup(child) && this.isDescendantOf(node, child)) return true;
    }
    return false;
  }
}
