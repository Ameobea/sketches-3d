import * as THREE from 'three';
import { untrack } from 'svelte';

import { scanControlHandleIds, scanGizmoHandleIds } from 'src/geoscript/gizmoScan';
import type { EnvironmentConfig, MeshTabView, TabView, TreeDef } from 'src/geoscript/geotoyAPIClient';
import { FallbackMat, HiddenMat, NormalMat, WireframeMat, type MaterialDef } from 'src/geoscript/materials';
import { populateScene } from 'src/geoscript/runner/geoscriptRunner';
import type { RunResult } from 'src/geoscript/runner/runner';
import type { RenderedControl, RenderedObject } from 'src/geoscript/runner/types';
import { MaterialRuntime } from 'src/geotoy/modules/materialRuntime.svelte';
import type { GeotoyPersistence } from 'src/geotoy/modules/persistence.svelte';
import type { Viz } from 'src/viz';
import type { PostprocessingPipelineController } from 'src/viz/postprocessing/defaultPostprocessing';
import {
  centerView,
  focusOnSubtree,
  setProjection,
  toggleProjection as toggleProjectionCamera,
  untilOrbitControls,
} from 'src/geotoy/modes/mesh/cameraControls';
import {
  buildLightHelpers,
  toggleLightHelpers as toggleLightHelpersImpl,
} from 'src/geotoy/modes/mesh/gizmos';
import { Textures } from 'src/geotoy/panels/materialEditor/state.svelte';
import { fetchAndSetTextures, getReferencedTextureIDs } from 'src/geotoy/modules/materialLoading.svelte';
import { applyGeoscriptSceneEnvironment } from 'src/geotoy/modes/mesh/sceneEnvironment';
import { buildParentMap, collectDescendants, computeMeshCounts } from 'src/geotoy/modules/treeOps';
import { GLOBALS_SELECTION_ID, type TreeState } from 'src/geotoy/modules/treeState.svelte';
import type { GizmoEditorHooks } from 'src/geoscript/gizmoExtensions';
import {
  runtimeMetric,
  type MenuSection,
  type Mode,
  type SceneMenuActions,
  type StatusMetric,
  type ViewMenuActions,
} from 'src/geotoy/modes/mode';
import { getView } from 'src/geotoy/modules/compositionStorage';
import { DefaultView, IntFormatter } from 'src/geotoy/types';
import type { RunStats } from 'src/geoscript/runner/runner';

interface MeshSceneDeps {
  viz: Viz;
  /** Read through a getter: the active tab's `TreeState` changes when tabs switch. */
  getTreeState: () => TreeState;
  persistence: GeotoyPersistence;
  pipelineController: PostprocessingPipelineController | null;
  bootSignal: AbortSignal;
  /** Last consumed run's tree — solo/disabled membership uses it (live tree only for flags). */
  getLastRunTree: () => TreeDef | null;
  /** Spline-editor refresh hook, invoked once per consumed run. */
  onRunConsumed: (controls: RenderedControl[]) => void;
  /** Gizmo affordances surfaced through the `Mode` contract; owned by GizmoController. */
  getEditorHooks: () => GizmoEditorHooks;
  /** Scene environment of the *active* tab; per-tab and mesh-only. */
  getEnvironment: () => EnvironmentConfig | undefined;
}

const OverrideMats = { wireframe: WireframeMat, 'wireframe-xray': WireframeMat, normal: NormalMat };

/**
 * Owns the 3D side of a run's output: scene population + object reuse/disposal, mesh
 * counts, per-run handle/control GC, solo/disabled visibility, material builds + mesh
 * material assignment (incl. debug overrides), scene environment, light helpers, and
 * camera/view state.
 */
export class MeshScene implements Mode {
  readonly kind = 'mesh' as const;

  private readonly deps: MeshSceneDeps;
  private readonly loader = new THREE.ImageBitmapLoader();
  readonly materialRuntime: MaterialRuntime;

  renderedObjects: RenderedObject[] = $state([]);
  meshCounts: ReadonlyMap<string, number> = $state(new Map());
  materialOverride = $state<'wireframe' | 'wireframe-xray' | 'normal' | null>(null);
  cameraProjection = $state<'perspective' | 'orthographic'>('perspective');

  private lightHelpers: THREE.Object3D[] = [];
  private pomRescanQueued = false;
  /** nodeId → last-scanned {source, ids}; skips re-parsing unchanged sources on GC. */
  private readonly handleScanCache = new Map<string, { source: string; ids: Set<string> }>();
  private readonly controlScanCache = new Map<string, { source: string; ids: Set<string> }>();

  /** Referenced-texture id set (defs + env equirect) as a joined string so unrelated def
   *  edits don't re-fire the fetch effect. */
  private readonly referencedTextureIDsKey = $derived.by(() => {
    const ids = new Set(getReferencedTextureIDs(this.deps.persistence.materialDefinitions.materials));
    const env = this.deps.getEnvironment();
    if (env?.kind === 'equirect' && env.textureId >= 0) ids.add(env.textureId);
    return [...ids].sort((a, b) => a - b).join(',');
  });

  constructor(deps: MeshSceneDeps) {
    this.deps = deps;
    this.materialRuntime = new MaterialRuntime(deps.viz, this.loader);

    // Solo + disabled visibility. Membership uses the last-run tree; disabled flags
    // come from the live tree so toggles are instant.
    $effect(() => {
      const soloId = deps.getTreeState().state.soloId;
      const renderTree = deps.getLastRunTree();
      const liveTree = deps.getTreeState().state.tree;
      if (!renderTree) {
        for (const obj of this.renderedObjects) {
          if (obj instanceof THREE.Mesh) obj.visible = true;
        }
        return;
      }

      const parentMap = buildParentMap(renderTree);
      const soloAllowed = soloId ? collectDescendants(renderTree, soloId) : null;
      const ancestorHidden = (id: string): boolean => {
        let cur: string | undefined = id;
        while (cur) {
          if (liveTree.nodes[cur]?.disabled) return true;
          cur = parentMap.get(cur);
        }
        return false;
      };

      for (const obj of this.renderedObjects) {
        if (!(obj instanceof THREE.Mesh)) continue;
        const sourceNodeId = obj.userData.sourceNodeId as string | undefined;
        if (!sourceNodeId) {
          obj.visible = !soloId;
          continue;
        }
        const inSolo = !soloAllowed || soloAllowed.has(sourceNodeId);
        obj.visible = inSolo && !ancestorHidden(sourceNodeId);
      }
    });

    // Texture metadata is a build dependency: fetch when the referenced id set changes
    // (boot, def edits, library picks, revert, env equirect). Registered BEFORE the sync
    // effect so in-flight placeholders are seeded before the first build captures its
    // textures (an unfetchable texture then lands the visible fallback instead of a
    // silent map drop).
    $effect(() => {
      void this.referencedTextureIDsKey;
      this.ensureReferencedTextures();
    });

    // Rebuild on def edits and texture-metadata arrival (per-id hashing inside sync).
    $effect(() => {
      void Textures.textures;
      const defs = $state.snapshot(deps.persistence.materialDefinitions.materials) as Record<
        string,
        MaterialDef
      >;
      untrack(() => this.materialRuntime.sync(defs));
    });

    // Re-apply on env config change and texture-metadata arrival (the equirect URL may
    // resolve late); the post-run re-apply is an explicit call in consume. PMREM is
    // cached, so double-applies are cheap and idempotent.
    $effect(() => {
      void Textures.textures;
      void $state.snapshot(deps.getEnvironment());
      untrack(this.applyEnv);
    });

    // Single owner of mesh material assignment: run completions (renderedObjects), build
    // landings / def edits (byName), and override toggles all converge here. Pending
    // build → HiddenMat; unknown material name → FallbackMat (matches the runner).
    $effect(() => {
      const overrideMat = this.materialOverride ? OverrideMats[this.materialOverride] : null;
      const byName = this.materialRuntime.byName;
      for (const obj of this.renderedObjects) {
        if (!(obj instanceof THREE.Mesh)) continue;
        if (overrideMat) {
          obj.material = overrideMat;
          continue;
        }
        const entry = byName[obj.userData.materialName as string];
        obj.material = entry ? (entry.material ?? HiddenMat) : FallbackMat;
      }
      this.schedulePomRescan();
    });
  }

  /** Kick metadata fetches for referenced-but-unfetched textures. Placeholder seeding is
   *  synchronous, so calling this before a build lets the build capture in-flight fetches —
   *  required on synchronous run paths (buildRunInput after a def swap) where the fetch
   *  effect hasn't flushed yet. Dedupes via LoadedTextures, so extra calls are free. */
  ensureReferencedTextures = () => {
    const key = untrack(() => this.referencedTextureIDsKey);
    if (!key) return;
    void fetchAndSetTextures(this.loader, key.split(',').map(Number));
  };

  private applyEnv = () =>
    void applyGeoscriptSceneEnvironment(
      this.deps.viz,
      this.loader,
      $state.snapshot(this.deps.getEnvironment()) as EnvironmentConfig | undefined,
      id => Textures.textures[id]?.url
    );

  // Material swaps invalidate the bounded-silhouette manager's per-mesh registry.
  private schedulePomRescan = () => {
    if (this.pomRescanQueued) return;
    this.pomRescanQueued = true;
    queueMicrotask(() => {
      this.pomRescanQueued = false;
      this.deps.viz.postprocessingController?.rescanPomMeshes();
    });
  };

  private removeRenderedObject(obj: RenderedObject) {
    const { viz } = this.deps;
    viz.scene.remove(obj);
    if (
      (obj instanceof THREE.DirectionalLight || obj instanceof THREE.SpotLight) &&
      obj.userData.geotoyTarget instanceof THREE.Object3D
    ) {
      if (obj.userData.geotoyTarget) {
        viz.scene.remove(obj.userData.geotoyTarget);
      }
    }
    if (obj instanceof THREE.Mesh || obj instanceof THREE.Line) {
      obj.geometry.dispose();
    }
  }

  clearScene = () => {
    for (const obj of this.renderedObjects) {
      this.removeRenderedObject(obj);
    }
    this.renderedObjects = [];
    for (const helper of this.lightHelpers) {
      this.deps.viz.scene.remove(helper);
    }
    this.lightHelpers = [];
    this.meshCounts = new Map();
    // Keyed by node id, which is per-tree — without this they accumulate an entry for every
    // node of every tab visited (and every `clear local changes`, which re-ids the trees).
    this.handleScanCache.clear();
    this.controlScanCache.clear();
  };

  consume(result: RunResult, tree: TreeDef, moduleNameToNodeId: Record<string, string>) {
    const { viz } = this.deps;
    const treeState = this.deps.getTreeState();
    // Defer disposal until after populate so unchanged objects can be reused.
    const prevObjects = this.renderedObjects;
    const prevByReuseKey = new Map<string, RenderedObject>();
    for (const obj of prevObjects) {
      const key = obj.userData.reuseKey as string | undefined;
      if (typeof key === 'string') prevByReuseKey.set(key, obj);
    }

    const populated = populateScene(viz.scene, result, {
      tree,
      moduleNameToNodeId,
      prev: prevByReuseKey,
    });
    this.renderedObjects = populated.objects;
    for (const obj of prevObjects) {
      const key = obj.userData.reuseKey as string | undefined;
      if (typeof key === 'string' && populated.reusedKeys.has(key)) continue;
      this.removeRenderedObject(obj);
    }

    const directCounts = new Map<string, number>();
    for (const obj of this.renderedObjects) {
      if (!(obj instanceof THREE.Mesh)) continue;
      const id = obj.userData.sourceNodeId as string | undefined;
      if (!id) continue;
      directCounts.set(id, (directCounts.get(id) ?? 0) + 1);
    }
    this.meshCounts = computeMeshCounts(tree, directCounts);

    this.deps.onRunConsumed(result.controls);

    // GC orphaned handles: keep ids the channel reported this run (covers dynamic names
    // the static scan can't see), plus the static handle ids in each node's source
    // (covers gizmos in branches that didn't execute this run).
    const liveByNode = new Map<string, Set<string>>();
    for (const gz of result.gizmos) {
      const nid = gz.sourceModule ? moduleNameToNodeId[gz.sourceModule] : undefined;
      if (!nid) continue;
      let set = liveByNode.get(nid);
      if (!set) {
        set = new Set();
        liveByNode.set(nid, set);
      }
      set.add(gz.handleId);
    }
    for (const node of Object.values(tree.nodes)) {
      if (!node.handles) continue;
      const live = liveByNode.get(node.id) ?? new Set<string>();
      let scan = this.handleScanCache.get(node.id);
      if (!scan || scan.source !== node.source) {
        scan = { source: node.source, ids: scanGizmoHandleIds(node.source) };
        this.handleScanCache.set(node.id, scan);
      }
      for (const id of scan.ids) live.add(id);
      treeState.pruneHandles(node.id, live);
    }

    // Same orphan-GC for input controls (runtime-reported ids + static scan for un-run branches).
    const liveControlsByNode = new Map<string, Set<string>>();
    for (const c of result.controls) {
      const nid = c.sourceModule ? moduleNameToNodeId[c.sourceModule] : undefined;
      if (!nid) continue;
      let set = liveControlsByNode.get(nid);
      if (!set) {
        set = new Set();
        liveControlsByNode.set(nid, set);
      }
      set.add(c.handleId);
    }
    for (const node of Object.values(tree.nodes)) {
      if (!node.controls) continue;
      const live = liveControlsByNode.get(node.id) ?? new Set<string>();
      let scan = this.controlScanCache.get(node.id);
      if (!scan || scan.source !== node.source) {
        scan = { source: node.source, ids: scanControlHandleIds(node.source) };
        this.controlScanCache.set(node.id, scan);
      }
      for (const id of scan.ids) live.add(id);
      treeState.pruneControls(node.id, live);
    }

    for (const helper of this.lightHelpers) {
      viz.scene.remove(helper);
    }
    if (localStorage['geoscript-light-helpers'] === 'true') {
      this.lightHelpers = buildLightHelpers(viz, this.renderedObjects);
    } else {
      this.lightHelpers = [];
    }

    // Fresh CustomShaderMaterials need scene.environment re-pushed after populate.
    this.applyEnv();
  }

  private resetDepthPrepass() {
    const pc = this.deps.pipelineController;
    if (pc?.depthPrePassMaterial) {
      pc.depthPrePassMaterial.polygonOffset = false;
    }
    pc?.setDepthPrePassEnabled(true);
  }

  toggleWireframe = () => {
    const wasWireframe = this.materialOverride === 'wireframe';
    if (this.materialOverride) {
      this.resetDepthPrepass();
      this.materialOverride = null;
    }
    if (wasWireframe) {
      return;
    }

    this.materialOverride = 'wireframe';
    const pc = this.deps.pipelineController;
    if (pc?.depthPrePassMaterial) {
      pc.depthPrePassMaterial.polygonOffset = true;
      pc.depthPrePassMaterial.polygonOffsetFactor = 1;
      pc.depthPrePassMaterial.polygonOffsetUnits = 1;
    }
    pc?.setDepthPrePassEnabled(true);
  };

  toggleWireframeXray = () => {
    const wasXray = this.materialOverride === 'wireframe-xray';
    if (this.materialOverride) {
      this.resetDepthPrepass();
      this.materialOverride = null;
    }
    if (wasXray) {
      return;
    }

    this.materialOverride = 'wireframe-xray';
    this.deps.pipelineController?.setDepthPrePassEnabled(false);
  };

  toggleNormalMat = () => {
    const wasNormal = this.materialOverride === 'normal';
    if (this.materialOverride) {
      this.resetDepthPrepass();
      this.materialOverride = null;
    }
    if (wasNormal) {
      return;
    }

    this.materialOverride = 'normal';
  };

  toggleLightHelpers = () => {
    this.lightHelpers = toggleLightHelpersImpl(this.deps.viz, this.renderedObjects, this.lightHelpers);
  };

  /** Frame the given subtree, or fit-all when null. */
  focus = (sel: string | null) => {
    const { viz } = this.deps;
    const treeState = this.deps.getTreeState();
    if (sel) {
      focusOnSubtree(viz, this.renderedObjects, treeState.state.tree, sel);
    } else {
      centerView(viz, this.renderedObjects);
    }
  };

  get editorHooks(): GizmoEditorHooks {
    return this.deps.getEditorHooks();
  }

  sceneMenu(a: SceneMenuActions): MenuSection[] {
    return [
      {
        items: [
          { label: 'edit materials…', action: a.openMaterialEditor },
          { label: 'scene environment…', action: a.openEnvironment },
          { label: 'export scene…', action: a.exportScene },
          {
            label: {
              recording: 'stop recording',
              initializing: 'initializing…',
              'not-recording': 'record video',
            }[a.recordingState],
            disabled: a.recordingState === 'initializing',
            action: a.toggleRecording,
          },
        ],
      },
    ];
  }

  viewSections(a: ViewMenuActions): MenuSection[] {
    return [
      {
        header: 'display',
        items: [
          { label: 'axis helpers', shortcut: 'A', action: a.toggleAxisHelpers },
          { label: 'light helpers', shortcut: '⇧L', action: this.toggleLightHelpers },
          {
            label: 'gizmo ghosts',
            state: a.showGizmoGhosts ? 'on' : 'off',
            disabled: !a.gizmosExist,
            action: a.toggleGizmoGhosts,
          },
          { label: 'wireframe', shortcut: 'W', action: this.toggleWireframe },
        ],
      },
      {
        header: 'camera',
        items: [
          {
            label: 'projection',
            state: this.cameraProjection === 'orthographic' ? 'ortho' : 'persp',
            shortcut: 'O',
            action: a.toggleProjection,
          },
          {
            label: 'center on selection',
            shortcut: '.',
            action: () => this.focus(this.selectedNodeId()),
          },
        ],
      },
    ];
  }

  statusMetrics(stats: RunStats): StatusMetric[] {
    const metric = (label: string, n: number, one: string, many = `${one}s`): StatusMetric => ({
      label,
      value: IntFormatter.format(n),
      short: `${IntFormatter.format(n)} ${n === 1 ? one : many}`,
    });
    const out: StatusMetric[] = [runtimeMetric(stats)];
    if (stats.renderedMeshCount > 0 || stats.renderedPathCount === 0) {
      out.push(metric('Rendered Meshes', stats.renderedMeshCount, 'mesh', 'meshes'));
    }
    if (stats.renderedPathCount > 0) {
      out.push(metric('Rendered Paths', stats.renderedPathCount, 'path'));
    }
    out.push(metric('Total Vertices', stats.totalVtxCount, 'vert'));
    out.push(metric('Total Faces', stats.totalFaceCount, 'face'));
    return out;
  }

  /** `focus(null)` is the fit-all branch, so the selection has to be resolved explicitly. */
  private selectedNodeId(): string | null {
    const { selectedId, tree } = this.deps.getTreeState().state;
    return selectedId && selectedId !== GLOBALS_SELECTION_ID && tree.nodes[selectedId] ? selectedId : null;
  }

  /** Null when the live camera doesn't describe this tab yet — mid-restore (`setView` is
   *  async, so the pose still belongs to the previous tab) or pre-orbit-controls (`getView`
   *  would pair a live position with a default target). The caller keeps what it had. */
  buildViewState = (): MeshTabView | null =>
    this.restoresInFlight > 0 || !this.deps.viz.orbitControls ? null : getView(this.deps.viz);

  restoreViewState = (view: TabView | null) => void this.setView((view as MeshTabView | null) ?? DefaultView);

  /** Counted, not a flag: overlapping restores (rapid switching before orbit controls
   *  resolve) would otherwise let the first one to finish reopen the window. */
  private restoresInFlight = 0;

  setView = async (view: MeshTabView) => {
    const { viz } = this.deps;
    this.restoresInFlight += 1;
    try {
      const orbitControls = await untilOrbitControls(viz, this.deps.bootSignal).catch(() => null);
      if (!orbitControls) return;

      if (view.cameraPosition) {
        viz.camera.position.set(...view.cameraPosition);
      }
      if (view.target) {
        orbitControls.target.set(...view.target);
      }
      // Position/target are set first so the ortho frustum is sized from the correct distance.
      this.cameraProjection = view.projection;
      setProjection(viz, this.cameraProjection);
      if (viz.camera instanceof THREE.PerspectiveCamera && view.fov !== undefined) {
        viz.camera.fov = view.fov;
        viz.camera.updateProjectionMatrix();
      }
      if (viz.camera instanceof THREE.OrthographicCamera && view.zoom !== undefined) {
        viz.camera.zoom = view.zoom;
        viz.camera.updateProjectionMatrix();
      }
      viz.camera.lookAt(orbitControls.target);
      orbitControls.update();
    } finally {
      this.restoresInFlight -= 1;
    }
  };

  toggleProjection = () => {
    this.cameraProjection = toggleProjectionCamera(this.deps.viz);
  };
}
