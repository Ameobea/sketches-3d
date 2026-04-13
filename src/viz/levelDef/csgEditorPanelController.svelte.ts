import { mount, unmount } from 'svelte';

import type { CsgTreeNode, AssetDef, LevelDef } from './types';
import { computeNodePolarities } from './csgTreeUtils';
import CsgTreeEditor from './CsgTreeEditor.svelte';

/**
 * Manages the lifecycle of the CSG tree editor panel (mount/unmount) and the
 * reactive Svelte state that drives it.
 *
 * Intentionally separate from preview scene logic and asset resolution — the
 * panel can be open/closed independently of whether previews are rendering.
 */
export class CsgEditorPanelController {
  private panelState = $state({
    editorOpen: false,
    assetName: null as string | null,
    tree: null as CsgTreeNode | null,
    selectedNodePath: null as string | null,
    nodePolarities: new Map<string, 'positive' | 'negative'>(),
  });

  private component: Record<string, any> | null = null;
  private target: HTMLDivElement | null = null;

  constructor(
    private readonly onTreeChange: (tree: CsgTreeNode) => void,
    private readonly onNodeSelect: (path: string | null) => void,
    private readonly onExit: () => void
  ) {}

  get isOpen(): boolean {
    return this.panelState.editorOpen;
  }

  get assetName(): string | null {
    return this.panelState.assetName;
  }

  get tree(): CsgTreeNode | null {
    return this.panelState.tree;
  }

  /** Mount the editor panel for the given CSG asset. */
  open(assetName: string, tree: CsgTreeNode, levelDef: LevelDef): void {
    this.close();

    this.panelState.editorOpen = true;
    this.panelState.assetName = assetName;
    this.panelState.tree = tree;
    this.panelState.nodePolarities = computeNodePolarities(tree);

    const target = document.createElement('div');
    document.body.appendChild(target);
    this.target = target;

    const state = this.panelState;
    const geoscriptAssetIds = Object.entries(levelDef.assets)
      .filter(([, def]: [string, AssetDef]) => def.type === 'geoscript')
      .map(([id]) => id);

    this.component = mount(CsgTreeEditor, {
      target,
      props: {
        get tree(): CsgTreeNode | null {
          return state.tree;
        },
        get selectedNodePath(): string | null {
          return state.selectedNodePath;
        },
        get nodePolarities(): Map<string, 'positive' | 'negative'> {
          return state.nodePolarities;
        },
        assetIds: geoscriptAssetIds,
        ontreechange: (t: CsgTreeNode) => this.onTreeChange(t),
        onnodeselect: (path: string | null) => this.onNodeSelect(path),
        onexitcsg: () => this.onExit(),
      },
    });
  }

  /** Unmount the editor panel and reset all panel state. */
  close(): void {
    if (this.component) {
      unmount(this.component);
      this.component = null;
    }
    if (this.target) {
      this.target.remove();
      this.target = null;
    }
    this.panelState.editorOpen = false;
    this.panelState.assetName = null;
    this.panelState.tree = null;
    this.panelState.selectedNodePath = null;
    this.panelState.nodePolarities = new Map();
  }

  setSelectedNodePath(path: string | null): void {
    this.panelState.selectedNodePath = path;
  }

  updateTree(tree: CsgTreeNode): void {
    this.panelState.tree = tree;
  }

  updateNodePolarities(polarities: Map<string, 'positive' | 'negative'>): void {
    this.panelState.nodePolarities = polarities;
  }
}
