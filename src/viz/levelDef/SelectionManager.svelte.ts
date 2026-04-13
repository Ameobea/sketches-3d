import type { LightDef } from './types';
import type { LevelGroup, LevelLight, LevelObject, LevelSceneNode } from './levelSceneTypes';
import { isLevelGroup } from './levelSceneTypes';
import type { AssetLibFolder } from './assetLibTypes';

/**
 * Manages selection state for the level editor, including multi-selection.
 *
 * The selection is an ordered array of scene nodes. The last element is the
 * "primary" node — used for the info panel, transform anchor, etc.
 *
 * Light selection remains separate (single-select only, managed via
 * `selectedLight`).
 */
export class SelectionManager {
  /** Ordered selection — last element is the primary node. */
  private _selectedNodes: LevelSceneNode[] = [];
  /** Currently selected light (separate track from scene nodes). */
  selectedLight: LevelLight | null = null;

  /**
   * Reactive state object that drives the Svelte UI panels.
   * Mutations to its properties are picked up by Svelte's reactivity system.
   */
  readonly state = $state({
    /** IDs of all selected nodes (multi-select). */
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
    /** Position of the selected light, synced from TransformControls. */
    lightPosition: [0, 0, 0] as [number, number, number],
    /** Incremented whenever rootNodes changes — triggers hierarchy panel re-render. */
    treeVersion: 0,
    /** Incremented when a new asset is added — triggers asset list re-render in panel. */
    assetsVersion: 0,
    /** Asset library folder tree, populated after fetch on editor open. */
    libFolders: [] as AssetLibFolder[],
  });

  get selectedNodes(): readonly LevelSceneNode[] {
    return this._selectedNodes;
  }

  get count(): number {
    return this._selectedNodes.length;
  }

  /** The primary (last-selected) node, or null if nothing is selected. */
  get primaryNode(): LevelSceneNode | null {
    return this._selectedNodes.length > 0 ? this._selectedNodes[this._selectedNodes.length - 1] : null;
  }

  /** Convenience: the primary node if it's a leaf object, otherwise null. */
  get primaryObject(): LevelObject | null {
    const node = this.primaryNode;
    return node && !isLevelGroup(node) ? node : null;
  }

  /** True when exactly one node is selected. */
  get isSingle(): boolean {
    return this._selectedNodes.length === 1;
  }

  /** True when more than one node is selected. */
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
    const editable = this._selectedNodes.filter(n => !n.generated);
    if (editable.length < 2) return false;

    const getParentId = (node: import('./levelSceneTypes').LevelSceneNode): string | null => {
      // Check if it's at root level
      if (rootNodes.includes(node)) return null;
      // Otherwise find parent group
      for (const [id, candidate] of nodeById) {
        if (isLevelGroup(candidate) && candidate.children.includes(node)) return id;
      }
      return null;
    };

    const firstParentId = getParentId(editable[0]);
    return editable.every(n => getParentId(n) === firstParentId);
  }

  isSelected(node: LevelSceneNode): boolean {
    return this._selectedNodes.includes(node);
  }

  /**
   * Replace the entire selection with a single node.
   * Clears any active light selection.
   */
  select(node: LevelSceneNode) {
    this.selectedLight = null;
    this._selectedNodes = [node];
  }

  /**
   * Toggle a node in/out of the selection (ctrl+click behavior).
   * Handles ancestor/descendant pruning: adding a group removes its
   * descendants from the selection, and adding a child removes ancestor
   * groups.
   */
  toggleSelect(node: LevelSceneNode) {
    this.selectedLight = null;

    const idx = this._selectedNodes.indexOf(node);
    if (idx !== -1) {
      // Remove from selection
      this._selectedNodes = this._selectedNodes.filter((_, i) => i !== idx);
      return;
    }

    // Prune ancestors and descendants
    this._selectedNodes = this._selectedNodes.filter(existing => {
      // Remove if `existing` is an ancestor of `node`
      if (isLevelGroup(existing) && this.isDescendantOf(node, existing)) return false;
      // Remove if `existing` is a descendant of `node`
      if (isLevelGroup(node) && this.isDescendantOf(existing, node)) return false;
      return true;
    });

    this._selectedNodes.push(node);
  }

  /** Clear all node selection (does not clear light selection). */
  deselect() {
    this._selectedNodes = [];
  }

  /** Clear light selection. */
  deselectLight() {
    this.selectedLight = null;
  }

  /** Select a light, clearing any node selection. */
  selectLight(light: LevelLight) {
    this._selectedNodes = [];
    this.selectedLight = light;
  }

  /** Clear everything — nodes and lights. */
  clear() {
    this._selectedNodes = [];
    this.selectedLight = null;
  }

  /**
   * Sync the reactive `state` object from the current selection.
   * Called after any selection change. The `isCsgAsset` flag is set
   * by the caller since it depends on the CSG controller state.
   */
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

  /**
   * Read the current Three.js object transform of the primary node into
   * the reactive state. Called after selection changes, drag events,
   * undo/redo, and replay.
   */
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

  /** Sync the light position into the reactive state. */
  syncLightPosition() {
    if (!this.selectedLight) return;
    const pos = this.selectedLight.light.position;
    this.state.lightPosition = [pos.x, pos.y, pos.z];
  }

  /**
   * Check whether `node` is a descendant of `group` by walking the
   * group's children recursively.
   */
  private isDescendantOf(node: LevelSceneNode, group: LevelGroup): boolean {
    for (const child of group.children) {
      if (child === node) return true;
      if (isLevelGroup(child) && this.isDescendantOf(node, child)) return true;
    }
    return false;
  }
}
