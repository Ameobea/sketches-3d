// Owns the 2D side of a run's output: `render_texture` outputs, output/channel/display
// selection, pan-zoom view state, and solo/disabled visibility (same semantics as mesh
// visibility). `TexturePreview.svelte` renders from and mutates this state directly.

import type { TabView, TextureChannel, TextureTabView, TreeDef } from 'src/geoscript/geotoyAPIClient';
import { disposeRunObjects } from 'src/geoscript/runner/geoscriptRunner';
import type { GeneratedTexture, RunResult, RunStats } from 'src/geoscript/runner/runner';
import { runtimeMetric, type MenuSection, type Mode, type StatusMetric } from 'src/geotoy/modes/mode';
import { buildParentMap, collectDescendants } from 'src/geotoy/modules/treeOps';
import type { TreeState } from 'src/geotoy/modules/treeState.svelte';

interface TextureModeDeps {
  /** Read through a getter: the active tab's `TreeState` changes when tabs switch. */
  getTreeState: () => TreeState;
}

export class TextureMode implements Mode {
  readonly kind = 'texture' as const;
  private readonly deps: TextureModeDeps;

  hasRun = $state(false);
  // $state.raw: deep-proxying multi-MB Float32Array-holding objects makes hot per-pixel
  // reads in TexturePreview go through the proxy get trap (~100x slowdown); only ever
  // reassigned wholesale, never mutated in place.
  textures = $state.raw<GeneratedTexture[]>([]);
  private lastRunTree = $state.raw<TreeDef | null>(null);
  private moduleNameToNodeId = $state.raw<Record<string, string>>({});

  selectedName = $state<string | null>(null);
  channel = $state<TextureChannel>('rgb');
  /** Stack-preview interpolation index t ∈ [0,1]; only meaningful when the selected
   *  output has layers > 1. Session-local (not persisted in the tab view). */
  stackT = $state(0);
  tiled = $state(false);
  /** Explicit user override; `null` defers to the selected output's usage (albedo → on). */
  srgbOverride = $state<boolean | null>(null);
  /** UV point at the viewport center + screen px per texel; `null` = fit on next draw. */
  center = $state<[number, number] | null>(null);
  zoom = $state<number | null>(null);

  constructor(deps: TextureModeDeps) {
    this.deps = deps;
  }

  /** Solo = preview isolation; disabled flags come from the live tree so toggles are instant. */
  readonly visibleTextures: GeneratedTexture[] = $derived.by(() => {
    const tree = this.lastRunTree;
    if (!tree) return this.textures;
    const { soloId, tree: liveTree } = this.deps.getTreeState().state;
    const parentMap = buildParentMap(tree);
    const soloAllowed = soloId ? collectDescendants(tree, soloId) : null;
    const ancestorHidden = (id: string): boolean => {
      let cur: string | undefined = id;
      while (cur) {
        if (liveTree.nodes[cur]?.disabled) return true;
        cur = parentMap.get(cur);
      }
      return false;
    };
    return this.textures.filter(t => {
      const nodeId = this.moduleNameToNodeId[t.sourceModule];
      if (!nodeId) return !soloId;
      return (!soloAllowed || soloAllowed.has(nodeId)) && !ancestorHidden(nodeId);
    });
  });

  readonly selected: GeneratedTexture | null = $derived(
    this.visibleTextures.find(t => t.name === this.selectedName) ?? this.visibleTextures[0] ?? null
  );

  readonly srgb: boolean = $derived(this.srgbOverride ?? this.selected?.usage === 'albedo');

  consume(result: RunResult, tree: TreeDef, moduleNameToNodeId: Record<string, string>) {
    this.textures = result.objects.filter((o): o is GeneratedTexture => o.type === 'texture');
    this.lastRunTree = tree;
    this.moduleNameToNodeId = moduleNameToNodeId;
    // Meshes/paths/lights rendered from a texture tab have no consumer here yet.
    disposeRunObjects(result);
    this.hasRun = true;
  }

  /** View/selection state survives — this runs on cancel and tab switch, and the pan-zoom
   *  belongs to the tab, not the run. */
  clearScene = () => {
    this.hasRun = false;
    this.textures = [];
    this.lastRunTree = null;
    this.moduleNameToNodeId = {};
  };

  /** `null` refits the view (run bar's center-view); a node selects its first output. */
  focus = (nodeId: string | null) => {
    if (!nodeId) {
      this.center = null;
      this.zoom = null;
      return;
    }
    const sub = this.lastRunTree ? collectDescendants(this.lastRunTree, nodeId) : null;
    const hit = sub && this.textures.find(t => sub.has(this.moduleNameToNodeId[t.sourceModule] ?? ''));
    if (hit) this.selectedName = hit.name;
  };

  buildViewState = (): TabView | null =>
    this.center && this.zoom !== null
      ? {
          center: this.center,
          zoom: this.zoom,
          output: this.selected?.name,
          channel: this.channel,
          tiled: this.tiled,
          srgb: this.srgbOverride ?? undefined,
        }
      : null;

  restoreViewState = (view: TabView | null) => {
    const v = view as TextureTabView | null;
    this.center = v?.center ?? null;
    this.zoom = v?.zoom ?? null;
    this.selectedName = v?.output ?? null;
    this.channel = v?.channel ?? 'rgb';
    this.tiled = v?.tiled ?? false;
    this.srgbOverride = v?.srgb ?? null;
  };

  /** The rest of the texture menu (resolution, export png…) is later texture-engine work. */
  sceneMenu(): MenuSection[] {
    return [{ items: [{ label: 'no texture settings yet', disabled: true, action: () => {} }] }];
  }

  viewSections(): MenuSection[] {
    return [
      {
        header: 'display',
        items: [
          {
            label: 'tiled preview',
            state: this.tiled ? 'on' : 'off',
            action: () => (this.tiled = !this.tiled),
          },
          {
            label: 'srgb display',
            state: this.srgb ? 'on' : 'off',
            action: () => (this.srgbOverride = !this.srgb),
          },
        ],
      },
      { header: 'camera', items: [{ label: 'reset view', action: () => this.focus(null) }] },
    ];
  }

  statusMetrics(stats: RunStats): StatusMetric[] {
    const n = stats.renderedTextureCount;
    const out: StatusMetric[] = [
      runtimeMetric(stats),
      {
        label: 'Rendered Textures',
        value: String(n),
        short: `${n} ${n === 1 ? 'texture' : 'textures'}`,
      },
    ];
    const dims = new Set(this.textures.map(t => `${t.width}×${t.height}`));
    if (dims.size === 1) {
      const d = [...dims][0];
      out.push({ label: 'Dims', value: d, short: d });
    }
    return out;
  }
}
