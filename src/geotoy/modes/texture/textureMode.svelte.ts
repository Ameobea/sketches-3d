// Owns the 2D side of a run's output: `render_texture` outputs, output/channel/display
// selection, pan-zoom view state, and solo/disabled visibility (same semantics as mesh
// visibility). `TexturePreview.svelte` renders from and mutates this state directly.
// Also owns the 3D preview (a mesh-tab object shown through `PreviewScene`) and its
// per-tab target/camera, which travel with the tab view.

import type {
  MeshTabView,
  TabView,
  TextureChannel,
  TexturePreviewTarget,
  TextureTabView,
  TreeDef,
} from 'src/geoscript/geotoyAPIClient';
import type { MaterialDef } from 'src/geoscript/materials';
import { disposeRunObjects } from 'src/geoscript/runner/geoscriptRunner';
import type { GeneratedTexture, RunResult, RunStats } from 'src/geoscript/runner/runner';
import type { GeneratedObject } from 'src/geoscript/runner/types';
import { qualifyModuleName } from 'src/geoscript/treeCodegen';
import {
  runtimeMetric,
  type MenuSection,
  type Mode,
  type SceneMenuActions,
  type StatusMetric,
  type ViewMenuActions,
} from 'src/geotoy/modes/mode';
import { PreviewScene } from 'src/geotoy/modes/texture/previewScene.svelte';
import {
  PREVIEW_MODULE_NAME,
  type PreviewTargetProblem,
  type PreviewTargetResolution,
} from 'src/geotoy/modes/texture/previewTarget';
import type { MaterialRuntime } from 'src/geotoy/modules/materialRuntime.svelte';
import type { GeotoyTab } from 'src/geotoy/modules/tabs.svelte';
import { buildParentMap, collectDescendants } from 'src/geotoy/modules/treeOps';
import type { TreeState } from 'src/geotoy/modules/treeState.svelte';
import type { Viz } from 'src/viz';

interface TextureModeDeps {
  /** Read through getters: the active tab changes when tabs switch. */
  getTreeState: () => TreeState;
  getActiveTabId: () => string;
  getTabs: () => readonly GeotoyTab[];
  getMaterialDefs: () => Record<string, MaterialDef>;
  viz: Viz;
  materialRuntime: MaterialRuntime;
  bootSignal: AbortSignal;
  /** Scene env is owned by the mesh scene; re-pushed after the preview populates. */
  reapplyEnv: () => void;
  /** The preview's run-set contribution changed (target or 3D toggle): persist + re-run. */
  onPreviewChanged: () => void;
}

export class TextureMode implements Mode {
  readonly kind = 'texture' as const;
  private readonly deps: TextureModeDeps;
  readonly previewScene: PreviewScene;

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

  previewTarget = $state<TexturePreviewTarget | null>(null);
  /** 3D view showing instead of the 2D canvas; only meaningful with a target. */
  preview3d = $state(false);
  /** Camera pose while the 3D view is hidden; the live camera is authoritative while shown. */
  private previewCamera: MeshTabView | null = null;
  /** Set at run-build time when the target can't be pulled into the run. */
  previewProblem = $state<PreviewTargetProblem | null>(null);

  constructor(deps: TextureModeDeps) {
    this.deps = deps;
    this.previewScene = new PreviewScene({
      viz: deps.viz,
      materialRuntime: deps.materialRuntime,
      bootSignal: deps.bootSignal,
    });
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

  /** The target tab's environment while the 3D view shows — lit as in its own scene. */
  readonly previewEnvironment = $derived.by(() => {
    const t = this.previewTarget;
    return this.preview3d && t ? this.deps.getTabs().find(x => x.id === t.tabId)?.environment : undefined;
  });

  /** `tab › node[ › export]` from the live trees; missing parts read as such. */
  readonly previewTargetLabel: string | null = $derived.by(() => {
    const t = this.previewTarget;
    if (!t) return null;
    const tab = this.deps.getTabs().find(x => x.id === t.tabId);
    const node = tab?.treeState.state.tree.nodes[t.nodeId];
    return [
      tab?.name ?? '(missing tab)',
      node?.name ?? '(missing node)',
      ...(t.exportName ? [t.exportName] : []),
    ].join(' › ');
  });

  /** Non-null when nothing on the previewed object can show this tab's outputs. */
  readonly previewMaterialWarning: string | null = $derived.by(() => {
    const names = this.previewScene.materialNames;
    if (!names.size) return null;
    const re = new RegExp(`"procedural(-stack)?:${this.deps.getActiveTabId()}:`);
    const defs = Object.values(this.deps.getMaterialDefs());
    const referenced = [...names].some(n => {
      const def = defs.find(d => d.name === n);
      return def && re.test(JSON.stringify(def));
    });
    return referenced ? null : "no material on the previewed object uses this tab's outputs";
  });

  consume(
    result: RunResult,
    tree: TreeDef,
    moduleNameToNodeId: Record<string, string>,
    preview?: PreviewTargetResolution | null
  ) {
    this.textures = result.objects.filter((o): o is GeneratedTexture => o.type === 'texture');
    this.lastRunTree = tree;
    this.moduleNameToNodeId = moduleNameToNodeId;
    this.hasRun = true;

    const rest = result.objects.filter(o => o.type !== 'texture');
    if (!preview) {
      this.previewScene.clear();
      disposeRunObjects({ ...result, objects: rest });
      return;
    }
    // Meshes: the picked subtree's (or, for an export, the preview module's own render).
    // Lights: the whole source tab's plus the prelude rig re-evaluated in the preview
    // module — the object is lit as in its own scene. Paths and everything else: dropped.
    const { tabId, nodeId, exportName } = preview.target;
    const previewModule = qualifyModuleName(PREVIEW_MODULE_NAME, this.deps.getActiveTabId());
    const subtree = collectDescendants(preview.tree, nodeId);
    const kept: GeneratedObject[] = [];
    const dropped: GeneratedObject[] = [];
    for (const o of rest) {
      const keep =
        o.type === 'mesh'
          ? exportName
            ? o.sourceModule === previewModule
            : subtree.has(moduleNameToNodeId[o.sourceModule] ?? '')
          : o.type === 'light' &&
            (o.sourceModule === previewModule || o.sourceModule.startsWith(`${tabId}:`));
      (keep ? kept : dropped).push(o);
    }
    disposeRunObjects({ ...result, objects: dropped });
    this.previewScene.consume({ ...result, objects: kept }, preview.tree, moduleNameToNodeId);
    this.deps.reapplyEnv();
  }

  /** View/selection/preview state survives — this runs on cancel and tab switch, and the
   *  pan-zoom and preview target belong to the tab, not the run. */
  clearScene = () => {
    this.hasRun = false;
    this.textures = [];
    this.lastRunTree = null;
    this.moduleNameToNodeId = {};
    this.previewScene.clear();
  };

  /** `null` refits the view (run bar's center-view); a node selects its first output. In
   *  the 3D view the selection is the texture tree's, so both just frame the object. */
  focus = (nodeId: string | null) => {
    if (this.preview3d) {
      this.previewScene.focus();
      return;
    }
    if (!nodeId) {
      this.center = null;
      this.zoom = null;
      return;
    }
    const sub = this.lastRunTree ? collectDescendants(this.lastRunTree, nodeId) : null;
    const hit = sub && this.textures.find(t => sub.has(this.moduleNameToNodeId[t.sourceModule] ?? ''));
    if (hit) this.selectedName = hit.name;
  };

  setPreviewTarget = (target: TexturePreviewTarget | null) => {
    this.previewTarget = target;
    this.previewProblem = null;
    if (target) {
      this.previewScene.autoFrame = true;
      this.showPreview3d(true);
    } else {
      this.showPreview3d(false);
      this.previewScene.clear();
    }
    this.deps.onPreviewChanged();
  };

  setPreview3d = (on: boolean) => {
    if (on === this.preview3d || (on && !this.previewTarget)) return;
    this.showPreview3d(on);
    if (!on) this.previewScene.clear();
    this.deps.onPreviewChanged();
  };

  private showPreview3d(on: boolean) {
    if (on === this.preview3d) return;
    if (on) {
      this.preview3d = true;
      void this.previewScene.setView(this.previewCamera);
    } else {
      this.previewCamera = this.previewScene.buildViewState() ?? this.previewCamera;
      this.preview3d = false;
    }
  }

  buildViewState = (): TabView | null => {
    const camera = (this.preview3d ? this.previewScene.buildViewState() : null) ?? this.previewCamera;
    if (!this.center && this.zoom === null && !this.previewTarget && !camera) return null;
    return {
      center: this.center ?? undefined,
      zoom: this.zoom ?? undefined,
      output: this.selected?.name,
      channel: this.channel,
      tiled: this.tiled,
      srgb: this.srgbOverride ?? undefined,
      preview: this.previewTarget ?? undefined,
      preview3d: this.preview3d || undefined,
      previewCamera: camera ?? undefined,
    };
  };

  restoreViewState = (view: TabView | null) => {
    const v = view as TextureTabView | null;
    this.center = v?.center ?? null;
    this.zoom = v?.zoom ?? null;
    this.selectedName = v?.output ?? null;
    this.channel = v?.channel ?? 'rgb';
    this.tiled = v?.tiled ?? false;
    this.srgbOverride = v?.srgb ?? null;
    this.previewTarget = v?.preview ?? null;
    this.previewProblem = null;
    this.previewCamera = v?.previewCamera ?? null;
    this.preview3d = !!(v?.preview3d && v.preview);
    if (this.preview3d) void this.previewScene.setView(this.previewCamera);
  };

  sceneMenu(a: SceneMenuActions): MenuSection[] {
    return [
      {
        items: [
          { label: 'preview object…', action: a.openPreviewPicker },
          {
            label: 'clear preview object',
            disabled: !this.previewTarget,
            action: () => this.setPreviewTarget(null),
          },
        ],
      },
    ];
  }

  viewSections(a: ViewMenuActions): MenuSection[] {
    const display: MenuSection = {
      header: 'display',
      items: [
        {
          label: '3d preview',
          shortcut: 'P',
          state: this.preview3d ? 'on' : 'off',
          action: a.togglePreview3d,
        },
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
    };
    const camera: MenuSection = this.preview3d
      ? {
          header: 'camera',
          items: [
            {
              label: 'projection',
              state: this.previewScene.cameraProjection === 'orthographic' ? 'ortho' : 'persp',
              shortcut: 'O',
              action: a.toggleProjection,
            },
            { label: 'center view', shortcut: '.', action: () => this.focus(null) },
          ],
        }
      : { header: 'camera', items: [{ label: 'reset view', shortcut: '.', action: () => this.focus(null) }] };
    return [display, camera];
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
    if (this.preview3d) {
      const m = this.previewScene.meshCount;
      out.push({
        label: 'Preview Meshes',
        value: String(m),
        short: `${m} preview ${m === 1 ? 'mesh' : 'meshes'}`,
      });
    }
    return out;
  }
}
