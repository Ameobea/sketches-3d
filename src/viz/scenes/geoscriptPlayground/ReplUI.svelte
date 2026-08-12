<script lang="ts">
  import * as THREE from 'three';
  import { onMount, untrack } from 'svelte';
  import { resolve } from '$app/paths';

  import type { Viz } from 'src/viz';
  import type { WorkerManager } from 'src/geoscript/workerManager';
  import type { GeoscriptPlaygroundUserData } from './geoscriptPlayground.svelte';
  import SaveControls from 'src/geotoy/panels/SaveControls.svelte';
  import { goto } from '$app/navigation';
  import { type ReplCtx } from './types';
  import ReplOutput from 'src/geotoy/panels/ReplOutput.svelte';
  import ReplControls from 'src/geotoy/panels/ReplControls.svelte';
  import EditorPane from 'src/geotoy/panels/EditorPane.svelte';
  import ExportModal from 'src/geotoy/panels/ExportModal.svelte';
  import { GeoscriptExecution, type RunInput } from 'src/geotoy/modules/execution.svelte';
  import { FallbackMat, HiddenMat, NormalMat, WireframeMat, type MaterialDef } from 'src/geoscript/materials';
  import MaterialEditor from './materialEditor/MaterialEditor.svelte';
  import EnvironmentSettings from './EnvironmentSettings.svelte';
  import { Textures } from './materialEditor/state.svelte';
  import {
    cloneTransform3,
    type Composition,
    type CompositionVersion,
    type EnvironmentConfig,
    type GizmoValue,
    type Transform3,
    type TreeDef,
    type ViewState,
  } from 'src/geoscript/geotoyAPIClient';
  import { GeotoyPersistence } from 'src/geotoy/modules/persistence.svelte';
  import { GeotoyKeymap } from 'src/geotoy/modules/keymap';
  import { buildGeotoyKeymap } from './keymap';
  import { compileTree, buildInjectedValues, buildModuleNameToNodeId } from 'src/geoscript/treeCodegen';
  import ControlsPanel from 'src/geotoy/panels/ControlsPanel.svelte';
  import { buildEvalResultJson } from './evalResult';
  import { TreeState, GLOBALS_SELECTION_ID } from './treeState.svelte';
  import { buildParentMap, composeInstance0World, computeMeshCounts, findParentId } from './treeOps';
  import HierarchyPanel from 'src/geotoy/panels/HierarchyPanel.svelte';
  import NodeInspector from 'src/geotoy/panels/NodeInspector.svelte';
  import { TransformGizmo, type GizmoMode, type GizmoSpace } from './transformGizmo';
  import type { GizmoTargetRef } from 'src/viz/gizmos/gizmoTypes';
  import { scanGizmoHandleIds, scanGizmoHandleOrder, scanControlHandleIds } from 'src/geoscript/gizmoScan';
  import { GizmoGhosts, type GhostSpec } from 'src/viz/gizmos/gizmoGhosts';
  import { gizmoColorForIndex } from 'src/viz/gizmos/gizmoPalette';
  import { SplineOverlay, type SplinePoint } from 'src/viz/gizmos/splineOverlay';
  import { controlKey, splineControlPoints } from 'src/geoscript/controlsUi';
  import type { GizmoEditorHooks, GizmoReadout } from 'src/geoscript/gizmoExtensions';
  import { installRaycastSelect } from './raycastSelect';
  import { getIsUVUnwrapLoaded } from 'src/viz/wasm/uv_unwrap/uvUnwrap';
  import ReadOnlyCompositionDetails from 'src/geotoy/panels/ReadOnlyCompositionDetails.svelte';
  import {
    populateScene,
    buildWorldMatrixCache,
    instancePathKey,
  } from 'src/geoscript/runner/geoscriptRunner';
  import { decomposeTransform3 } from 'src/geoscript/runner/worldMatrixCache';
  import type { RenderedObject, RenderedGizmo, RenderedControl } from 'src/geoscript/runner/types';
  import { fetchAndSetTextures, getReferencedTextureIDs } from './materialLoading.svelte';
  import { MaterialRuntime } from 'src/geotoy/modules/materialRuntime.svelte';
  import {
    centerView,
    focusOnSubtree,
    snapView,
    orbit,
    setProjection,
    toggleProjection,
    untilOrbitControls,
  } from './cameraControls';
  import { buildLightHelpers, toggleAxisHelpers, toggleLightHelpers } from './gizmos';
  import { applyGeoscriptSceneEnvironment } from './sceneEnvironment';
  import { useRecording } from './recording';
  import type { PostprocessingPipelineController } from 'src/viz/postprocessing/defaultPostprocessing';
  import { logGeotoyEvent } from 'src/analytics';

  let {
    viz,
    workerManager,
    setReplCtx,
    userData: providedUserData,
    onSizeChange,
    pipelineController = null,
  }: {
    viz: Viz;
    workerManager: WorkerManager;
    setReplCtx: (ctx: ReplCtx) => void;
    userData?: GeoscriptPlaygroundUserData;
    onSizeChange: (size: number, isCollapsed: boolean, orientation: 'vertical' | 'horizontal') => void;
    pipelineController?: PostprocessingPipelineController | null;
  } = $props();

  let userData = $state<GeoscriptPlaygroundUserData | undefined>(untrack(() => providedUserData));

  const { toggleRecording, recordingState } = useRecording(
    untrack(() => viz),
    untrack(() => providedUserData)
  );

  let layoutOrientation = $state<'vertical' | 'horizontal'>(
    (localStorage.getItem('geoscriptLayoutOrientation') as 'vertical' | 'horizontal') || 'vertical'
  );
  $effect(() => {
    localStorage.setItem('geoscriptLayoutOrientation', layoutOrientation);
  });

  const toggleLayoutOrientation = () => {
    const newOrientation = layoutOrientation === 'vertical' ? 'horizontal' : 'vertical';
    layoutOrientation = newOrientation;
    if (newOrientation === 'horizontal') {
      size = Number(localStorage.getItem('geoscript-repl-width')) || Math.max(400, 0.35 * window.innerWidth);
    } else {
      size =
        Number(localStorage.getItem('geoscript-repl-height')) || Math.max(250, 0.25 * window.innerHeight);
    }
    onSizeChange(size, isEditorCollapsed, layoutOrientation);
  };

  const persistence = new GeotoyPersistence({
    viz: untrack(() => viz),
    getUserData: () => userData,
    serializeActiveTree: () => treeState.serialize(),
    isTreeDirty: () => treeState.treeDirty,
  });
  const keymap = new GeotoyKeymap();
  /** Aborts boot-time orbit-controls waiters if the component unmounts mid-boot. */
  const bootAbort = new AbortController();
  const initialTree = persistence.initial.tree;

  const serverDoc = untrack(() => userData?.initialComposition?.version.tree);
  const treeState = new TreeState({
    initial: initialTree,
    savedBaseline: serverDoc
      ? (serverDoc.trees.find(t => t.id === persistence.initial.activeTreeId)?.tree ?? initialTree)
      : initialTree,
  });
  treeState.setSelected(initialTree.rootId);

  const treePanelVisible = $derived(
    Object.keys(treeState.state.tree.nodes).length > 1 ||
      treeState.state.tree.globalsSource.length > 0 ||
      treeState.state.selectedId === GLOBALS_SELECTION_ID
  );

  const breadcrumb = $derived.by(() => {
    const sel = treeState.state.selectedId;
    if (sel === GLOBALS_SELECTION_ID) return '_globals';
    if (!sel) return '';
    const tree = treeState.state.tree;
    const names: string[] = [];
    let cur: string | null = sel;
    while (cur) {
      const node = tree.nodes[cur];
      if (!node) break;
      names.unshift(cur === tree.rootId ? 'Root' : node.name);
      cur = findParentId(tree, cur);
    }
    return names.join(' / ');
  });

  let innerWidth = $state(window.innerWidth);
  let isEditorCollapsed = $state(
    (() => {
      const raw = localStorage.getItem('geoscriptEditorCollapsed');
      return typeof raw === 'string' ? raw === 'true' : innerWidth < 768;
    })()
  );
  $effect(() => {
    localStorage.setItem('geoscriptEditorCollapsed', isEditorCollapsed ? 'true' : 'false');
  });
  $effect(() => {
    if (innerWidth >= 768 && isEditorCollapsed) {
      isEditorCollapsed = false;
      onSizeChange(size, isEditorCollapsed, layoutOrientation);
    }
  });

  const handleForkedComposition = async (newComp: Composition, newVersion: CompositionVersion) => {
    if (!userData?.me) {
      return;
    }
    const newUserData: GeoscriptPlaygroundUserData = {
      initialComposition: { comp: newComp, version: newVersion },
      workerManager: userData.workerManager,
      me: userData.me,
      renderMode: userData.renderMode,
    };
    const forkedFrom = userData?.initialComposition?.comp;
    await persistence.saveVersion(
      newComp,
      {
        title: forkedFrom?.title ?? 'untitled (fork)',
        description: forkedFrom?.description ?? '',
        isShared: forkedFrom?.is_shared ?? false,
        tags: forkedFrom?.tags ?? [],
      },
      newUserData
    );
    userData = newUserData;
    treeState.markSaved();
    persistence.markClean();
  };

  const initialLayoutOrientation =
    (localStorage.getItem('geoscriptLayoutOrientation') as 'vertical' | 'horizontal' | null) || 'vertical';
  let size = $state(
    initialLayoutOrientation === 'horizontal'
      ? Number(localStorage.getItem('geoscript-repl-width')) || Math.max(400, 0.35 * window.innerWidth)
      : Number(localStorage.getItem('geoscript-repl-height')) || Math.max(250, 0.25 * window.innerHeight)
  );
  onMount(() => {
    onSizeChange(size, isEditorCollapsed, layoutOrientation);
    execution.init().then(() => {
      // If the tab was closed while the last run was in progress, don't eagerly re-run
      // it — it may have been an infinite loop.
      if (persistence.initial.lastRunWasSuccessful) {
        execution.run();
      }
    });
  });

  let gizmo = $state<TransformGizmo | null>(null);
  let raycastDisposer: (() => void) | null = null;
  let gizmoTick: (() => void) | null = null;
  let gizmoMode = $state<GizmoMode>('translate');
  let gizmoSpace = $state<GizmoSpace>('local');

  // What the viewport gizmo edits: an explicit arm (inspector / viewport click / chip)
  // recorded against the selection it was made under, falling back to the selected
  // node's first instance. A stale override (selection moved on, armed node/instance
  // deleted) falls back automatically — no latch, no same-tick clobber window.
  let armedOverride = $state<{ sel: string | null; ref: GizmoTargetRef | null } | null>(null);
  const armedRef = $derived.by((): GizmoTargetRef | null => {
    const sel = treeState.state.selectedId;
    if (armedOverride?.sel !== sel) return defaultArmFor(sel);
    const ref = armedOverride.ref;
    if (ref === null) return null;
    const node = treeState.state.tree.nodes[ref.nodeId];
    if (!node) return defaultArmFor(sel);
    if (ref.kind === 'instance' && !node.instances.some(i => i.id === ref.instanceId)) {
      return defaultArmFor(sel);
    }
    return ref;
  });

  let dragStartTransform: Transform3 | null = null;
  let dragStartHandle: GizmoValue | null = null;
  /**
   * Snapshot of the last successful run: serialized tree, reported gizmos/controls, and
   * the module-name → node-id mapping. Written once per run; effects key on it as their
   * run-completed signal (the transform-only fast path reassigns it as a change token).
   */
  let lastRun = $state.raw<{
    tree: TreeDef;
    gizmos: RenderedGizmo[];
    controls: RenderedControl[];
    moduleNameToNodeId: Record<string, string>;
  } | null>(null);
  /** Gates the ghost-toggle menu item / controls panel. */
  const hasAnyGizmos = $derived((lastRun?.gizmos.length ?? 0) > 0);
  const hasAnyControls = $derived((lastRun?.controls.length ?? 0) > 0);
  const controlScanCache = new Map<string, { source: string; ids: Set<string> }>();

  let loggedControlUse = false;
  let controlRunTimer = 0;
  // Continuous inputs (sliders) fire rapidly; coalesce into a trailing run once edits
  // settle. Discarded on cancel (onCancelCleanup) and unmount.
  const scheduleControlRun = () => {
    if (!loggedControlUse) {
      loggedControlUse = true;
      logGeotoyEvent('editor', 'controls_used');
    }
    clearTimeout(controlRunTimer);
    controlRunTimer = window.setTimeout(runOrFast, 120);
  };
  // Spline-control viewport editing (input_spline): one control editable at a time; the
  // shared SplineOverlay owns markers/polyline/point-gizmo, we own persistence + the panel
  // bridge. Reactive bits live in a `$state` object so the panel's getter reads track
  // through the deep proxy (a reassigned `$state` local wouldn't cross the prop boundary).
  const splineState = $state({
    activeKey: null as string | null,
    points: [] as SplinePoint[],
    selectedIx: null as number | null,
  });
  let splineEdit: { key: string; nodeId: string; handleId: string } | null = null;
  let splineOverlay: SplineOverlay | null = null;
  let splineTick: (() => void) | null = null;

  const splineKeyOf = controlKey;

  const exitSplineEdit = () => {
    if (!splineEdit) return;
    splineEdit = null;
    splineState.activeKey = null;
    splineState.selectedIx = null;
    if (splineTick) {
      viz.unregisterBeforeRenderCb(splineTick);
      splineTick = null;
    }
    splineOverlay?.dispose();
    splineOverlay = null;
  };

  const enterSplineEdit = (c: RenderedControl) => {
    exitSplineEdit();
    const nodeId = c.sourceModule ? lastRun?.moduleNameToNodeId[c.sourceModule] : undefined;
    const g = gizmo;
    if (!nodeId || !g) return;
    treeState.setSelected(nodeId);
    splineEdit = { key: splineKeyOf(c), nodeId, handleId: c.handleId };
    splineState.activeKey = splineEdit.key;
    armedOverride = { sel: treeState.state.selectedId, ref: null };
    const overlay = new SplineOverlay({
      overlayScene: viz.overlayScene,
      camera: viz.camera,
      canvas: viz.renderer.domElement,
      getBaseMatrix: out => out.copy(nodeWorldMatrix(nodeId)),
      attachGizmo: target => {
        setGizmoMode('translate');
        g.setCustomTarget(target);
      },
      detachGizmo: () => g.setCustomTarget(null),
      isDraggingGizmo: () => g.dragging(),
      onChange: (points, phase) => {
        splineState.points = points;
        if (phase === 'commit') commitSplineValue(nodeId, c.handleId, points);
      },
      onSelectionChange: ix => {
        splineState.selectedIx = ix;
      },
    });
    splineOverlay = overlay;
    const pts = splineControlPoints(c);
    overlay.setPoints(pts);
    splineState.points = pts;
    splineTick = () => overlay.tick();
    viz.registerBeforeRenderCb(splineTick);
  };

  const commitSplineValue = (nodeId: string, handleId: string, points: SplinePoint[]) => {
    const before = treeState.captureControl(nodeId, handleId);
    treeState.setControl(nodeId, handleId, { kind: 'spline', value: points });
    treeState.recordControlChange(nodeId, handleId, before, treeState.captureControl(nodeId, handleId));
    runOrFast();
  };

  const splinePanelCtx = {
    get activeKey() {
      return splineState.activeKey;
    },
    get points() {
      return splineState.points;
    },
    get selectedIx() {
      return splineState.selectedIx;
    },
    toggle: (c: RenderedControl) => {
      if (splineEdit?.key === splineKeyOf(c)) exitSplineEdit();
      else enterSplineEdit(c);
    },
    select: (ix: number) => splineOverlay?.selectPoint(ix),
    setPoint: (ix: number, p: [number, number, number]) => splineOverlay?.setPoint(ix, p),
    add: () => splineOverlay?.addPointAfter(),
    remove: (ix: number) => splineOverlay?.deletePoint(ix),
  };

  // Exit spline editing when the selection moves off the owning node.
  $effect(() => {
    const sel = treeState.state.selectedId;
    if (splineEdit && sel !== splineEdit.nodeId) untrack(exitSplineEdit);
  });

  let showGizmoGhosts = $state(localStorage.getItem('geoscript-gizmo-ghosts') !== 'false');
  let ghosts: GizmoGhosts | null = null;
  let ghostTick: (() => void) | null = null;
  let editorPane = $state<{
    blur: () => void;
    togglePreludeEjected: () => Promise<void>;
  } | null>(null);
  /** nodeId → last-scanned {source, handleIds}; skips re-parsing unchanged sources on GC. */
  const handleScanCache = new Map<string, { source: string; ids: Set<string> }>();

  onMount(() => {
    let cancelled = false;
    (async () => {
      const orbit = await untilOrbitControls(viz, bootAbort.signal).catch(() => null);
      if (!orbit || cancelled) return;
      const g = new TransformGizmo(
        viz.camera,
        viz.renderer.domElement,
        viz.overlayScene,
        () => treeState.state.tree,
        {
          onDraggingChanged: dragging => {
            orbit.enabled = !dragging;
          },
          onDragStart: ref => {
            if (ref.kind === 'handle') {
              dragStartHandle = treeState.captureHandle(ref.nodeId, ref.name);
              return;
            }
            if (ref.kind !== 'instance') return;
            dragStartTransform = treeState.captureInstanceTransform(ref.nodeId, ref.instanceId);
            dragSession = { parentMap: buildParentMap(treeState.state.tree) };
          },
          onTransformChange: (ref, transform) => {
            if (ref.kind !== 'instance') return;
            treeState.setInstanceTransform(ref.nodeId, ref.instanceId, transform);
            runOrFast();
          },
          onHandleChange: (nodeId, handleId, value) => {
            // Store + live readout per drag-tick, but defer the (geometry-changing) re-eval
            // to drag end — per-tick re-runs aren't smooth enough to be worth it.
            treeState.setHandle(nodeId, handleId, value);
          },
          onDragEnd: ref => {
            if (ref.kind === 'handle') {
              const after = treeState.captureHandle(ref.nodeId, ref.name);
              treeState.recordHandleChange(ref.nodeId, ref.name, dragStartHandle, after);
              dragStartHandle = null;
              runOrFast();
              return;
            }
            if (ref.kind !== 'instance') return;
            dragSession = null;
            const after = treeState.captureInstanceTransform(ref.nodeId, ref.instanceId);
            if (dragStartTransform && after) {
              treeState.recordInstanceTransformChange(ref.nodeId, ref.instanceId, dragStartTransform, after);
            }
            dragStartTransform = null;
            runOrFast();
          },
        }
      );
      // Resolve a handle's origin/kind/mode from the last run's channel + stored value.
      g.setHandleContextResolver((nodeId, handleId) => {
        const node = treeState.state.tree.nodes[nodeId];
        if (!node) return null;
        const reported = lastRun?.gizmos.find(
          gz => gz.sourceModule === node.name && gz.handleId === handleId
        );
        const stored = node.handles?.[handleId];
        const kind = reported?.kind ?? stored?.kind ?? 'vec3';
        return {
          kind,
          mode: reported ? (reported.absolute ? 'absolute' : 'delta') : (stored?.mode ?? 'delta'),
          origin: reported?.origin ?? [0, 0, 0],
          transform:
            kind === 'transform' && reported?.value.length === 16
              ? decomposeTransform3(new THREE.Matrix4().fromArray(reported.value))
              : undefined,
          axes: reported?.axes ?? [true, true, true],
        };
      });
      const tickGizmo = () => g.update();
      viz.registerBeforeRenderCb(tickGizmo);

      const gh = new GizmoGhosts(viz.overlayScene, {
        camera: viz.camera,
        canvas: viz.renderer.domElement,
        isDraggingGizmo: () => g.dragging(),
      });
      const tickGhosts = () => gh.update();
      viz.registerBeforeRenderCb(tickGhosts);

      const disposer = installRaycastSelect({
        canvas: viz.renderer.domElement,
        camera: viz.camera,
        getCandidates: () =>
          renderedObjects.filter(o => o instanceof THREE.Mesh && !!o.userData.sourceNodeId),
        interceptClick: raycaster => {
          if (splineOverlay?.interceptClick(raycaster)) return true;
          const hit = gh.pickGhost(raycaster);
          if (!hit) return false;
          gizmoEditorHooks.arm(hit.handleId, hit.kind);
          return true;
        },
        onSelect: (id, instancePath) => {
          if (id === null) {
            // Background click: deselect to root (whose default arm is none).
            treeState.setSelected(treeState.state.tree.rootId);
            armedOverride = null;
            return;
          }
          const tree = treeState.state.tree;
          const node = tree.nodes[id];
          if (!node || id === tree.rootId) {
            treeState.setSelected(id);
            return;
          }
          // The clicked copy's own instance is the last element of its instance path.
          const clickedIdx = instancePath?.at(-1) ?? 0;
          const armId = (node.instances[clickedIdx] ?? node.instances[0]).id;
          // Multi-instance: select the parent so the inspector surfaces this node's
          // instance list (with the clicked instance armed); single-instance: select
          // the node itself, as before.
          treeState.setSelected(node.instances.length > 1 ? (findParentId(tree, id) ?? tree.rootId) : id);
          armInstance(id, armId);
        },
        isDraggingGizmo: () => g.dragging(),
      });
      if (cancelled) {
        viz.unregisterBeforeRenderCb(tickGizmo);
        viz.unregisterBeforeRenderCb(tickGhosts);
        disposer();
        gh.dispose();
        g.dispose();
        return;
      }
      gizmo = g;
      ghosts = gh;
      ghostTick = tickGhosts;
      raycastDisposer = disposer;
      gizmoTick = tickGizmo;
      rebuildGhosts();
    })();
    return () => {
      cancelled = true;
      exitSplineEdit();
      raycastDisposer?.();
      raycastDisposer = null;
      if (gizmoTick) viz.unregisterBeforeRenderCb(gizmoTick);
      gizmoTick = null;
      if (ghostTick) viz.unregisterBeforeRenderCb(ghostTick);
      ghostTick = null;
      ghosts?.dispose();
      ghosts = null;
      gizmo?.dispose();
      gizmo = null;
    };
  });

  const defaultArmFor = (sel: string | null): GizmoTargetRef | null => {
    if (sel === null || sel === GLOBALS_SELECTION_ID || sel === treeState.state.tree.rootId) {
      return null;
    }
    const node = treeState.state.tree.nodes[sel];
    if (!node || node.instances.length === 0) return null;
    return { kind: 'instance', nodeId: sel, instanceId: node.instances[0].id };
  };

  /** Arm a specific instance without disturbing selection (inspector / viewport click). */
  const armInstance = (nodeId: string, instanceId: string) => {
    armedOverride = {
      sel: treeState.state.selectedId,
      ref: { kind: 'instance', nodeId, instanceId },
    };
  };

  // Keep the gizmo bound to whatever is armed; re-sync after each run (ancestor world
  // transforms refresh). Reading `armedRef`/`lastRun` subscribes the effect to both.
  // Suspended while spline editing owns the gizmo via a custom target (re-fires on exit).
  $effect(() => {
    void armedRef;
    void lastRun;
    if (splineState.activeKey !== null) return;
    gizmo?.syncTo(armedRef, treeState.state.tree);
  });

  // Rebuild ghosts on discrete changes only (selection / arm / setting / each run); the
  // deep tree reads inside happen untracked so a drag's transform churn doesn't re-fire this.
  $effect(() => {
    void treeState.state.selectedId;
    void armedRef;
    void showGizmoGhosts;
    void lastRun;
    untrack(rebuildGhosts);
  });

  // gizmo2d/gizmo1d store a full vec3 but expose only their active axes; project so the
  // inline readout shows the right component count.
  const projectAxes = (value: number[], axes: [boolean, boolean, boolean]): number[] => {
    const out: number[] = [];
    for (let i = 0; i < 3; i += 1) if (axes[i]) out.push(value[i] ?? 0);
    return out;
  };

  const channelReadout = (gz: RenderedGizmo): GizmoReadout =>
    gz.kind === 'transform'
      ? { kind: 'transform', transform: { pos: gz.origin, rot: [0, 0, 0], scale: [1, 1, 1] } }
      : { kind: 'vec3', values: projectAxes(gz.value, gz.axes) };

  const storedReadout = (v: GizmoValue, axes: [boolean, boolean, boolean]): GizmoReadout =>
    v.kind === 'transform'
      ? { kind: 'transform', transform: v.value as GizmoReadout['transform'] }
      : { kind: 'vec3', values: projectAxes(v.value as number[], axes) };

  // Per-node readout map: last run's reported values, overridden by the locally-stored
  // (live-edited) handle value so a drag updates the inline readout before re-eval.
  const buildGizmoReadouts = (nodeId: string | null): Map<string, GizmoReadout> => {
    const map = new Map<string, GizmoReadout>();
    const node = nodeId ? treeState.state.tree.nodes[nodeId] : null;
    if (!node) return map;
    const axesByHandle = new Map<string, [boolean, boolean, boolean]>();
    for (const gz of lastRun?.gizmos ?? []) {
      if (gz.sourceModule !== node.name) continue;
      axesByHandle.set(gz.handleId, gz.axes);
      map.set(gz.handleId, channelReadout(gz));
    }
    if (node.handles) {
      for (const [id, v] of Object.entries(node.handles)) {
        map.set(id, storedReadout(v, axesByHandle.get(id) ?? [true, true, true]));
      }
    }
    return map;
  };

  const gizmoReadouts = $derived.by(() => buildGizmoReadouts(treeState.state.selectedId));

  // World matrix of a node's representative (instance-0) copy, root → node inclusive — same
  // anchor `HandleTarget` uses, so a ghost sits exactly where its armed gizmo would.
  const _ghostWorld = new THREE.Matrix4();
  const _ghostScratch = new THREE.Matrix4();
  const nodeWorldMatrix = (nodeId: string): THREE.Matrix4 => {
    _ghostWorld.identity();
    composeInstance0World(treeState.state.tree, nodeId, _ghostWorld, _ghostScratch);
    return _ghostWorld;
  };

  const _ghostPos = new THREE.Vector3();
  // Ghosts only for the selected node's gizmos, at their live-gizmo positions. The armed
  // handle's own ghost is hidden (the real gizmo draws there instead).
  const rebuildGhosts = () => {
    if (!ghosts) return;
    const sel = treeState.state.selectedId;
    const node = sel && sel !== GLOBALS_SELECTION_ID ? treeState.state.tree.nodes[sel] : null;
    if (userData?.renderMode || !node) {
      ghosts.setGhosts([]);
      return;
    }
    const order = scanGizmoHandleOrder(node.source);
    const armedHandle = armedRef?.kind === 'handle' && armedRef.nodeId === sel ? armedRef.name : null;
    const world = nodeWorldMatrix(sel!);
    const specs: GhostSpec[] = [];
    for (const gz of lastRun?.gizmos ?? []) {
      if (gz.sourceModule !== node.name || gz.handleId === armedHandle) continue;
      if (!(gz.ghost ?? showGizmoGhosts)) continue;
      // transform handles report a 16-float matrix; its translation is `origin`.
      const lp =
        gz.kind === 'transform'
          ? gz.origin
          : gz.absolute
            ? gz.value
            : [gz.origin[0] + gz.value[0], gz.origin[1] + gz.value[1], gz.origin[2] + gz.value[2]];
      _ghostPos.set(lp[0], lp[1], lp[2]).applyMatrix4(world);
      const ix = order.indexOf(gz.handleId);
      specs.push({
        handleId: gz.handleId,
        kind: gz.kind,
        color: gizmoColorForIndex(ix >= 0 ? ix : specs.length),
        position: [_ghostPos.x, _ghostPos.y, _ghostPos.z],
      });
    }
    ghosts.setGhosts(specs);
  };

  const gizmoEditorHooks: GizmoEditorHooks = {
    arm: (handleId, kind) => {
      const sel = treeState.state.selectedId;
      // Handles are valid on any real node, including `_root` (unlike instance arming).
      if (!sel || sel === GLOBALS_SELECTION_ID || !treeState.state.tree.nodes[sel]) return;
      armedOverride = { sel, ref: { kind: 'handle', nodeId: sel, name: handleId } };
      if (kind === 'vec3') setGizmoMode('translate');
      editorPane?.blur();
    },
    disarm: () => {
      if (armedRef?.kind === 'handle') armedOverride = null;
    },
    resetHandle: handleId => {
      const sel = treeState.state.selectedId;
      const before = sel ? treeState.captureHandle(sel, handleId) : null;
      if (!sel || before === null) return; // already at default
      treeState.deleteHandle(sel, handleId);
      treeState.recordHandleChange(sel, handleId, before, null);
      runOrFast();
    },
    setHandleVec3: (handleId, value) => {
      const sel = treeState.state.selectedId;
      if (!sel || !treeState.state.tree.nodes[sel]) return;
      const before = treeState.captureHandle(sel, handleId);
      const after: GizmoValue = {
        kind: 'vec3',
        mode: treeState.state.tree.nodes[sel].handles?.[handleId]?.mode ?? 'delta',
        value,
      };
      treeState.setHandle(sel, handleId, after);
      treeState.recordHandleChange(sel, handleId, before, after);
      runOrFast();
    },
    getArmedHandleId: () => (armedRef?.kind === 'handle' ? armedRef.name : null),
  };

  let hierarchyPanel = $state<{ startRename: (id: string) => void } | null>(null);

  const setGizmoMode = (mode: GizmoMode) => {
    gizmoMode = mode;
    gizmo?.setMode(mode);
  };

  const toggleGizmoSpace = () => {
    gizmoSpace = gizmoSpace === 'world' ? 'local' : 'world';
    gizmo?.setSpace(gizmoSpace);
  };

  const runUndo = (): boolean => {
    if (!treeState.undo()) return true;
    runOrFast();
    return true;
  };

  const runRedo = (): boolean => {
    if (!treeState.redo()) return true;
    runOrFast();
    return true;
  };

  const resolveSelectedNode = (): { sel: string; rootId: string } | null => {
    const sel = treeState.state.selectedId;
    const tree = treeState.state.tree;
    if (sel === null || sel === GLOBALS_SELECTION_ID || !tree.nodes[sel]) return null;
    return { sel, rootId: tree.rootId };
  };

  const handleMousedown = (e: MouseEvent) => {
    e.preventDefault();

    const handleMousemove = (e: MouseEvent) => {
      if (layoutOrientation === 'horizontal') {
        const newWidth = Math.min(window.innerWidth * 0.9, Math.max(200, window.innerWidth - e.clientX));
        size = newWidth;
        localStorage.setItem('geoscript-repl-width', `${newWidth}`);
      } else {
        const newHeight = Math.min(window.innerHeight * 0.9, Math.max(100, window.innerHeight - e.clientY));
        size = newHeight;
        localStorage.setItem('geoscript-repl-height', `${newHeight}`);
      }
      onSizeChange(size, isEditorCollapsed, layoutOrientation);
    };

    const handleMouseup = () => {
      window.removeEventListener('mousemove', handleMousemove);
      window.removeEventListener('mouseup', handleMouseup);
    };

    window.addEventListener('mousemove', handleMousemove);
    window.addEventListener('mouseup', handleMouseup);
  };

  let renderedObjects: RenderedObject[] = $state([]);
  let lightHelpers: THREE.Object3D[] = $state([]);
  let meshCounts: ReadonlyMap<string, number> = $state(new Map());

  const collectDescendants = (tree: TreeDef, rootId: string): Set<string> => {
    const out = new Set<string>([rootId]);
    const queue = [rootId];
    while (queue.length > 0) {
      const id = queue.pop()!;
      const node = tree.nodes[id];
      if (!node) continue;
      for (const cid of node.children) {
        if (!out.has(cid)) {
          out.add(cid);
          queue.push(cid);
        }
      }
    }
    return out;
  };

  // Solo + disabled visibility. Membership uses the last-run tree; disabled flags
  // come from the live tree so toggles are instant.
  $effect(() => {
    const soloId = treeState.state.soloId;
    const renderTree = lastRun?.tree ?? null;
    const liveTree = treeState.state.tree;
    if (!renderTree) {
      for (const obj of renderedObjects) {
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

    for (const obj of renderedObjects) {
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

  let materialOverride = $state<'wireframe' | 'wireframe-xray' | 'normal' | null>(null);

  const resetDepthPrepass = () => {
    if (pipelineController?.depthPrePassMaterial) {
      pipelineController.depthPrePassMaterial.polygonOffset = false;
    }
    pipelineController?.setDepthPrePassEnabled(true);
  };

  const toggleWireframe = () => {
    const wasWireframe = materialOverride === 'wireframe';
    if (materialOverride) {
      resetDepthPrepass();
      materialOverride = null;
    }
    if (wasWireframe) {
      return;
    }

    materialOverride = 'wireframe';
    if (pipelineController?.depthPrePassMaterial) {
      pipelineController.depthPrePassMaterial.polygonOffset = true;
      pipelineController.depthPrePassMaterial.polygonOffsetFactor = 1;
      pipelineController.depthPrePassMaterial.polygonOffsetUnits = 1;
    }
    pipelineController?.setDepthPrePassEnabled(true);
  };

  const toggleWireframeXray = () => {
    const wasXray = materialOverride === 'wireframe-xray';
    if (materialOverride) {
      resetDepthPrepass();
      materialOverride = null;
    }
    if (wasXray) {
      return;
    }

    materialOverride = 'wireframe-xray';
    pipelineController?.setDepthPrePassEnabled(false);
  };

  const toggleNormalMat = () => {
    const wasNormal = materialOverride === 'normal';
    if (materialOverride) {
      resetDepthPrepass();
      materialOverride = null;
    }
    if (wasNormal) {
      return;
    }

    materialOverride = 'normal';
  };

  let materialEditorOpen = $state(false);
  let environmentSettingsOpen = $state(false);

  const toggleMaterialEditorOpen = () => {
    materialEditorOpen = !materialEditorOpen;
    if (materialEditorOpen) logGeotoyEvent('materials', 'editor_open');
  };
  const toggleEnvironmentSettingsOpen = () => {
    environmentSettingsOpen = !environmentSettingsOpen;
    if (environmentSettingsOpen) logGeotoyEvent('environment', 'settings_open');
  };
  let cameraProjection = $state<'perspective' | 'orthographic'>('perspective');

  onMount(() => {
    const referencedTextureIDs = getReferencedTextureIDs(persistence.materialDefinitions.materials);
    const environment = persistence.environment;
    if (environment?.kind === 'equirect' && environment.textureId >= 0) {
      referencedTextureIDs.push(environment.textureId);
    }
    if (referencedTextureIDs.length > 0) {
      fetchAndSetTextures(loader, referencedTextureIDs);
    }
  });

  const materialNames = $derived(
    Object.values(persistence.materialDefinitions.materials).map(mat => mat.name)
  );
  let pushedMaterialsKey: string | null = null;
  // Push ctx-scoped material state on change and on ctx recreation. Keyed on
  // `execution.ctxEpoch`, not `ctxPtr` — a recreated wasm instance usually allocates
  // the ctx at the same address.
  $effect(() => {
    if (execution.ctxPtr === null) {
      return;
    }
    const defaultMaterialID = persistence.materialDefinitions.defaultMaterialID;
    const key = `${execution.ctxEpoch}|${defaultMaterialID ?? ''}|${materialNames.join('\u0000')}`;
    if (key === pushedMaterialsKey) {
      return;
    }
    pushedMaterialsKey = key;
    untrack(() => void execution.repl.setMaterials(execution.ctxPtr!, defaultMaterialID, materialNames));
  });

  const loader = new THREE.ImageBitmapLoader();
  const materialRuntime = new MaterialRuntime(
    untrack(() => viz),
    loader
  );
  // Rebuild on def edits and texture-metadata arrival (per-id hashing inside sync).
  $effect(() => {
    void Textures.textures;
    const defs = $state.snapshot(persistence.materialDefinitions.materials) as Record<string, MaterialDef>;
    untrack(() => materialRuntime.sync(defs));
  });

  const applyEnv = () =>
    void applyGeoscriptSceneEnvironment(
      viz,
      loader,
      $state.snapshot(persistence.environment) as EnvironmentConfig | undefined,
      id => Textures.textures[id]?.url
    );
  // Re-apply on env config change and texture-metadata arrival (the equirect URL may
  // resolve late); the post-run re-apply is an explicit call in consume. PMREM is
  // cached, so double-applies are cheap and idempotent.
  $effect(() => {
    void Textures.textures;
    void $state.snapshot(persistence.environment);
    untrack(applyEnv);
  });

  let pomRescanQueued = false;
  // Material swaps invalidate the bounded-silhouette manager's per-mesh registry.
  const schedulePomRescan = () => {
    if (pomRescanQueued) return;
    pomRescanQueued = true;
    queueMicrotask(() => {
      pomRescanQueued = false;
      viz.postprocessingController?.rescanPomMeshes();
    });
  };

  const OverrideMats = { wireframe: WireframeMat, 'wireframe-xray': WireframeMat, normal: NormalMat };
  // Single owner of mesh material assignment: run completions (renderedObjects), build
  // landings / def edits (byName), and override toggles all converge here. Pending
  // build → HiddenMat; unknown material name → FallbackMat (matches the runner).
  $effect(() => {
    const overrideMat = materialOverride ? OverrideMats[materialOverride] : null;
    const byName = materialRuntime.byName;
    for (const obj of renderedObjects) {
      if (!(obj instanceof THREE.Mesh)) continue;
      if (overrideMat) {
        obj.material = overrideMat;
        continue;
      }
      const entry = byName[obj.userData.materialName as string];
      obj.material = entry ? (entry.material ?? HiddenMat) : FallbackMat;
    }
    schedulePomRescan();
  });

  const removeRenderedObject = (obj: RenderedObject) => {
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
  };

  /**
   * Hash of every wasm input except per-node transforms; the fast path uses it
   * to decide whether a re-eval is needed. Material defs are serialized whole
   * because UV-mapping fields drive JS-side UV unwrap during the per-mesh build.
   */
  const computeEvalInputsHash = (): string => {
    const tree = treeState.state.tree;
    const nodeKeys = Object.keys(tree.nodes).sort();
    const parts: string[] = [`g:${tree.globalsSource}`];
    for (const k of nodeKeys) {
      const n = tree.nodes[k];
      // `children` matters: reparenting changes `compileTree`'s emitted imports.
      // `instances.length` (not the transforms) matters: add/remove changes the
      // rendered-object set, so it must force a full re-run while drags stay fast.
      // `handles`/`controls` matter: a gizmo or input-control value can change geometry, so
      // either must force a full re-eval rather than the transform-only fast path.
      parts.push(
        `n:${k}:${n.name}:${n.disabled ? 1 : 0}:${n.instances.length}:${n.source}:${n.children.join(',')}:${JSON.stringify(n.handles ?? null)}:${JSON.stringify(n.controls ?? null)}`
      );
    }
    parts.push(`pe:${persistence.preludeEjected ? 1 : 0}`);
    const matIds = Object.keys(persistence.materialDefinitions.materials).sort();
    for (const id of matIds) {
      parts.push(`m:${id}:${JSON.stringify(persistence.materialDefinitions.materials[id])}`);
    }
    parts.push(`dm:${persistence.materialDefinitions.defaultMaterialID ?? ''}`);
    return parts.join('\x00');
  };

  // Set for the duration of a gizmo drag, where the tree structure is frozen, so the
  // fast path can skip the eval-hash recompute + parent-map rebuild every frame.
  let dragSession: { parentMap: Map<string, string> } | null = null;
  const _fastScratch = new THREE.Matrix4();

  /** Recompose each mesh's `ancestor × localInScript` if only transforms changed. */
  const tryTransformOnlyFastPath = (): boolean => {
    if (execution.isRunning) return false;
    if (execution.lastOkInputKey === null) return false;
    const drag = dragSession;
    if (!drag && computeEvalInputsHash() !== execution.lastOkInputKey) return false;

    const tree = treeState.state.tree;
    const worldMatrices = buildWorldMatrixCache(tree, drag?.parentMap ?? buildParentMap(tree));
    const worldByKey = new Map<string, THREE.Matrix4>();
    for (const [nodeId, list] of worldMatrices) {
      for (const e of list) worldByKey.set(`${nodeId}\x00${instancePathKey(e.path)}`, e.world);
    }
    for (const obj of renderedObjects) {
      if (!(obj instanceof THREE.Mesh)) continue;
      const sourceNodeId = obj.userData.sourceNodeId as string | undefined;
      const localInScript = obj.userData.localInScript as THREE.Matrix4 | undefined;
      const instancePath = obj.userData.instancePath as number[] | undefined;
      if (!sourceNodeId || !localInScript || !instancePath) continue;
      const world = worldByKey.get(`${sourceNodeId}\x00${instancePathKey(instancePath)}`);
      if (world) _fastScratch.copy(world);
      else _fastScratch.identity();
      _fastScratch.multiply(localInScript);
      _fastScratch.decompose(obj.position, obj.quaternion, obj.scale);
    }
    // Reassign as a change token so `lastRun`-keyed effects (gizmo re-sync, ghosts)
    // re-fire; structure is frozen on this path so the contents are still accurate.
    // Skipped mid-drag; `onDragEnd`'s full run refreshes everything.
    if (!drag && lastRun) lastRun = { ...lastRun };
    return true;
  };

  const runOrFast = () => {
    if (tryTransformOnlyFastPath()) return;
    execution.run();
  };

  const runManual = async () => {
    const outcome = await execution.run();
    if (!userData?.renderMode && outcome) {
      logGeotoyEvent('editor', 'run', {
        success: outcome.type === 'ok',
        num_nodes: Object.keys(treeState.state.tree.nodes).length,
        comp_id: userData?.initialComposition?.comp.id ?? null,
      });
    }
  };

  interface ReplRunInput extends RunInput {
    tree: TreeDef;
  }

  const execution = new GeoscriptExecution<ReplRunInput>({
    workerManager: untrack(() => workerManager),
    onRunStart: persistence.saveDraft,
    setLastRunWasSuccessful: persistence.setLastRunWasSuccessful,
    buildRunInput: () => {
      const defs = $state.snapshot(persistence.materialDefinitions.materials) as Record<string, MaterialDef>;
      // Hash-guarded no-op unless a def changed this tick — the run never sees entries
      // staler than the defs it compiles against.
      materialRuntime.sync(defs);
      const matsByName: Record<string, { def: MaterialDef; mat: THREE.Material }> = {};
      for (const [id, def] of Object.entries(defs)) {
        matsByName[def.name] = { def, mat: materialRuntime.entries[id]?.material ?? HiddenMat };
      }

      const tree = treeState.serialize();
      const compiled = compileTree(tree);
      return {
        code: compiled.rootSource,
        modules: compiled.modules,
        extraAmbientSources: tree.globalsSource.trim().length > 0 ? [tree.globalsSource] : [],
        includePrelude: !persistence.preludeEjected,
        materials: matsByName,
        materialOverride,
        renderMode: userData?.renderMode ?? false,
        gizmoValues: buildInjectedValues(tree),
        moduleNameToNodeId: buildModuleNameToNodeId(tree),
        tree,
        inputKey: computeEvalInputsHash(),
      };
    },
    consume: (result, { tree, moduleNameToNodeId }, isCurrent) => {
      // Defer disposal until after populate so unchanged objects can be reused.
      const prevObjects = renderedObjects;
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
      renderedObjects = populated.objects;
      for (const obj of prevObjects) {
        const key = obj.userData.reuseKey as string | undefined;
        if (typeof key === 'string' && populated.reusedKeys.has(key)) continue;
        removeRenderedObject(obj);
      }

      const directCounts = new Map<string, number>();
      for (const obj of renderedObjects) {
        if (!(obj instanceof THREE.Mesh)) continue;
        const id = obj.userData.sourceNodeId as string | undefined;
        if (!id) continue;
        directCounts.set(id, (directCounts.get(id) ?? 0) + 1);
      }
      meshCounts = computeMeshCounts(tree, directCounts);

      lastRun = { tree, gizmos: result.gizmos, controls: result.controls, moduleNameToNodeId };
      // Refresh the active spline editor from the run channel (or exit if its control vanished).
      if (splineEdit) {
        const sc = result.controls.find(c => splineKeyOf(c) === splineEdit!.key);
        if (!sc || sc.kind !== 'spline') {
          exitSplineEdit();
        } else if (!(gizmo?.dragging() ?? false)) {
          const pts = splineControlPoints(sc);
          splineOverlay?.setPoints(pts);
          splineState.points = pts;
        }
      }
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
        let scan = handleScanCache.get(node.id);
        if (!scan || scan.source !== node.source) {
          scan = { source: node.source, ids: scanGizmoHandleIds(node.source) };
          handleScanCache.set(node.id, scan);
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
        let scan = controlScanCache.get(node.id);
        if (!scan || scan.source !== node.source) {
          scan = { source: node.source, ids: scanControlHandleIds(node.source) };
          controlScanCache.set(node.id, scan);
        }
        for (const id of scan.ids) live.add(id);
        treeState.pruneControls(node.id, live);
      }

      for (const helper of lightHelpers) {
        viz.scene.remove(helper);
      }
      if (localStorage['geoscript-light-helpers'] === 'true') {
        lightHelpers = buildLightHelpers(viz, renderedObjects);
      } else {
        lightHelpers = [];
      }

      // Fresh CustomShaderMaterials need scene.environment re-pushed after populate.
      applyEnv();
    },
    onCancelCleanup: () => {
      for (const obj of renderedObjects) {
        removeRenderedObject(obj);
      }
      renderedObjects = [];
      clearTimeout(controlRunTimer);
    },
  });

  const handleInstanceTransformChange = (nodeId: string, instanceId: string, transform: Transform3) => {
    const before = treeState.captureInstanceTransform(nodeId, instanceId);
    if (!before) return;
    treeState.setInstanceTransform(nodeId, instanceId, transform);
    treeState.recordInstanceTransformChange(nodeId, instanceId, before, transform);
    runOrFast();
  };

  const handleAddInstance = (nodeId: string) => {
    const node = treeState.state.tree.nodes[nodeId];
    if (!node) return;
    const last = node.instances[node.instances.length - 1];
    const seed = cloneTransform3(last);
    seed.pos[0] += 0.5;
    seed.pos[2] += 0.5;
    const newId = treeState.addInstance(nodeId, seed);
    runOrFast();
    if (newId) armInstance(nodeId, newId);
  };

  const handleRemoveInstance = (nodeId: string, instanceId: string) => {
    treeState.removeInstance(nodeId, instanceId);
    runOrFast();
  };

  const handleInspectorDisableToggle = (id: string, disabled: boolean) => {
    treeState.setDisabled(id, disabled);
  };

  const rerun = async (onlyIfUVUnwrapperNotLoaded: boolean) => {
    if (onlyIfUVUnwrapperNotLoaded && getIsUVUnwrapLoaded()) {
      return;
    }
    await execution.run();
  };

  const toggleEditorCollapsed = () => {
    persistence.saveDraft();
    isEditorCollapsed = !isEditorCollapsed;
    onSizeChange(size, isEditorCollapsed, layoutOrientation);
  };

  const togglePreludeEjected = () => void editorPane?.togglePreludeEjected();

  let exportDialog = $state<HTMLDialogElement | null>(null);
  const onExport = () => {
    exportDialog?.showModal();
  };

  const setView = async (view: ViewState) => {
    const orbitControls = await untilOrbitControls(viz, bootAbort.signal).catch(() => null);
    if (!orbitControls) return;

    if (view.cameraPosition) {
      viz.camera.position.set(...view.cameraPosition);
    }
    if (view.target) {
      orbitControls.target.set(...view.target);
    }
    // Position/target are set first so the ortho frustum is sized from the correct distance.
    cameraProjection = view.projection ?? 'perspective';
    setProjection(viz, cameraProjection);
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
  };

  const handleToggleProjection = () => {
    cameraProjection = toggleProjection(viz);
    logGeotoyEvent('view', 'projection_toggle', { projection: cameraProjection });
    persistence.viewDirty = true;
    persistence.saveDraft();
  };

  const clearLocalChanges = () => {
    if (persistence.isDirty && !confirm('Really clear local changes?')) {
      return;
    }

    logGeotoyEvent('editor', 'clear_local_changes');
    const serverState = persistence.revertToServer();

    treeState.replaceTree(serverState.tree);
    treeState.setSelected(serverState.tree.rootId);

    const referencedTextureIDs = getReferencedTextureIDs(serverState.materials.materials);
    if (serverState.environment?.kind === 'equirect' && serverState.environment.textureId >= 0) {
      referencedTextureIDs.push(serverState.environment.textureId);
    }
    fetchAndSetTextures(loader, referencedTextureIDs).then(() => {
      persistence.materialDefinitions = { ...serverState.materials };
    });

    if (serverState.view) {
      setView(serverState.view);
    }

    execution.run();
  };

  const wrappedToggleAxesHelpers = () => toggleAxisHelpers(viz);
  const wrappedToggleLightHelpers = () => {
    lightHelpers = toggleLightHelpers(viz, renderedObjects, lightHelpers);
  };
  const toggleGizmoGhosts = () => {
    showGizmoGhosts = !showGizmoGhosts;
    localStorage['geoscript-gizmo-ghosts'] = showGizmoGhosts ? 'true' : 'false';
  };

  onMount(() => {
    if (userData?.renderMode) {
      const stats = document.getElementById('viz-stats');
      if (stats) {
        stats.style.display = 'none';
      }
    }

    setTimeout(() => setView(persistence.initial.view));

    if (!userData?.renderMode) {
      let loggedVizEngaged = false;
      untilOrbitControls(viz, bootAbort.signal).then(
        orbitControls =>
          orbitControls.addEventListener('start', () => {
            if (loggedVizEngaged) return;
            loggedVizEngaged = true;
            logGeotoyEvent('view', 'viz_engaged');
          }),
        () => {}
      );
    }

    const replCtx: ReplCtx = {
      centerView: () => {
        const ns = resolveSelectedNode();
        if (ns) {
          focusOnSubtree(viz, renderedObjects, treeState.state.tree, ns.sel);
        } else {
          centerView(viz, renderedObjects);
        }
      },
      toggleWireframe,
      toggleWireframeXray,
      toggleNormalMat,
      toggleLightHelpers: wrappedToggleLightHelpers,
      toggleAxesHelper: wrappedToggleAxesHelpers,
      getLastRunOutcome: () => execution.lastOutcome,
      getAreAllMaterialsLoaded: () => materialRuntime.allLoaded,
      run: runManual,
      snapView: axis => snapView(viz, axis),
      orbit: (axis, angle) => orbit(viz, axis, angle),
      toggleProjection: handleToggleProjection,
      toggleRecording,
      setGizmoMode: mode => {
        if (!resolveSelectedNode()) return;
        setGizmoMode(mode);
      },
      toggleGizmoSpace: () => {
        if (!resolveSelectedNode()) return;
        toggleGizmoSpace();
      },
      toggleSelectionSolo: () => {
        const ns = resolveSelectedNode();
        if (!ns || ns.sel === ns.rootId) return;
        treeState.setSolo(treeState.state.soloId === ns.sel ? null : ns.sel);
      },
      escapeSelection: e => {
        if (gizmo?.dragging()) return;
        if (treeState.state.soloId !== null) {
          treeState.setSolo(null);
          e?.preventDefault();
          return;
        }
        const ns = resolveSelectedNode();
        if (ns && ns.sel !== ns.rootId) {
          treeState.setSelected(ns.rootId);
          e?.preventDefault();
        }
      },
      deleteSelected: () => {
        if (gizmo?.dragging()) return; // never delete a node mid gizmo-drag
        // Destructive, so require a tree-editing context: hierarchy panel focus
        // or no UI focus at all.
        const active = document.activeElement;
        const inHierarchyPanel = !!(active && (active as HTMLElement).closest?.('[data-hierarchy-panel]'));
        const treeContextFocused = inHierarchyPanel || !active || active === document.body;
        if (!treeContextFocused) return;
        const ns = resolveSelectedNode();
        if (!ns || !treeState.canDelete(ns.sel)) return;
        treeState.deleteNode(ns.sel);
        logGeotoyEvent('editor', 'node_delete');
      },
      startRenameSelected: () => {
        const ns = resolveSelectedNode();
        if (!ns || ns.sel === ns.rootId) return;
        hierarchyPanel?.startRename(ns.sel);
      },
      treeUndo: e => {
        if (gizmo?.dragging()) return;
        runUndo();
        e?.preventDefault();
      },
      treeRedo: e => {
        if (gizmo?.dragging()) return;
        runRedo();
        e?.preventDefault();
      },
      autoFrameForRender: () => {
        void centerView(viz, renderedObjects);
      },
      buildEvalResultJson: req => {
        if (execution.ctxPtr === null) throw new Error('no geoscript context');
        return buildEvalResultJson({
          repl: execution.repl,
          ctxPtr: execution.ctxPtr,
          renderedObjects,
          tree: treeState.state.tree,
          stats: execution.runStats,
          req,
        });
      },
    };
    setReplCtx(replCtx);
    keymap.setTable(buildGeotoyKeymap(() => replCtx));
    keymap.install();

    window.addEventListener('beforeunload', persistence.saveDraft);

    return () => {
      bootAbort.abort();
      clearTimeout(controlRunTimer);
      workerManager.terminate();
      execution.dispose();
      keymap.dispose();

      for (const mesh of renderedObjects) {
        removeRenderedObject(mesh);
      }

      window.removeEventListener('beforeunload', persistence.saveDraft);
    };
  });

  const goHome = () => {
    persistence.saveDraft();

    if (persistence.isDirty) {
      if (!confirm('You have unsaved changes. Really leave page?')) {
        return;
      }
    }

    workerManager.terminate();

    goto(resolve('/geotoy'));
  };
</script>

<svelte:window bind:innerWidth />

{#if hasAnyControls && !userData?.renderMode}
  <ControlsPanel
    controls={lastRun?.controls ?? []}
    {treeState}
    moduleNameToNodeId={lastRun?.moduleNameToNodeId ?? {}}
    onEdit={scheduleControlRun}
    spline={splinePanelCtx}
  />
{/if}

<ExportModal bind:dialog={exportDialog} {renderedObjects} />
<MaterialEditor
  bind:isOpen={materialEditorOpen}
  bind:materials={persistence.materialDefinitions}
  {rerun}
  repl={execution.repl}
  ctxPtr={execution.ctxPtr}
  me={userData?.me}
/>

<EnvironmentSettings
  bind:isOpen={environmentSettingsOpen}
  bind:environment={persistence.environment}
  me={userData?.me}
/>

{#snippet replControls()}
  <ReplControls
    isRunning={execution.isRunning}
    {isEditorCollapsed}
    run={runManual}
    cancel={execution.cancel}
    {toggleEditorCollapsed}
    {goHome}
    err={execution.err}
    {onExport}
    {clearLocalChanges}
    onRecord={toggleRecording}
    recordingState={$recordingState}
    toggleAxisHelpers={wrappedToggleAxesHelpers}
    toggleLightHelpers={wrappedToggleLightHelpers}
    {toggleGizmoGhosts}
    {showGizmoGhosts}
    gizmosExist={hasAnyGizmos}
    {cameraProjection}
    toggleProjection={handleToggleProjection}
    isDirty={persistence.isDirty}
    preludeEjected={persistence.preludeEjected}
    {togglePreludeEjected}
    {toggleMaterialEditorOpen}
    {toggleEnvironmentSettingsOpen}
    {toggleLayoutOrientation}
  />
{/snippet}

{#if isEditorCollapsed}
  <div
    class={['root', 'collapsed', layoutOrientation === 'horizontal' ? 'horizontal' : '']}
    style={`${userData?.renderMode ? 'visibility: hidden; height: 0;' : ''} ${layoutOrientation === 'horizontal' ? 'width: 36px;' : 'height: 36px;'}`}
  >
    {@render replControls()}
  </div>
{:else}
  <div
    class={['root', layoutOrientation === 'horizontal' ? 'horizontal' : '']}
    style={`${userData?.renderMode ? 'visibility: hidden; height: 0; width: 0;' : ''} ${layoutOrientation === 'horizontal' ? `width: ${size}px;` : `height: ${size}px;`}`}
  >
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class={['dragger', layoutOrientation === 'horizontal' ? 'horizontal' : '']}
      role="separator"
      aria-orientation={layoutOrientation === 'horizontal' ? 'vertical' : 'horizontal'}
      onmousedown={handleMousedown}
    ></div>
    <div class={['editor-container', layoutOrientation === 'horizontal' ? 'horizontal' : '']}>
      {#if treePanelVisible}
        <div class={['tree-pane', layoutOrientation === 'horizontal' ? 'horizontal' : '']}>
          <HierarchyPanel
            bind:this={hierarchyPanel}
            tree={treeState.state.tree}
            selectedId={treeState.state.selectedId}
            soloId={treeState.state.soloId}
            failedNodeIds={execution.failedNodeIds}
            onselect={id => treeState.setSelected(id)}
            onsoloToggle={id => treeState.setSolo(treeState.state.soloId === id ? null : id)}
            onDisableToggle={id => {
              const node = treeState.state.tree.nodes[id];
              if (node) treeState.setDisabled(id, !node.disabled);
            }}
            oncreate={parentId => {
              const newId = treeState.createNode({ parentId: parentId ?? undefined });
              treeState.setSelected(newId);
              logGeotoyEvent('editor', 'node_add');
            }}
            ondelete={id => {
              treeState.deleteNode(id);
              logGeotoyEvent('editor', 'node_delete');
            }}
            onrename={(id, newName) => {
              try {
                treeState.rename(id, newName);
                return true;
              } catch (err) {
                console.warn('rename failed:', err);
                return false;
              }
            }}
            onreparent={(id, newParentId) => {
              try {
                treeState.reparent(id, newParentId);
              } catch (err) {
                console.warn('reparent failed:', err);
              }
            }}
            canDelete={id => treeState.canDelete(id)}
          />
        </div>
      {/if}
      <div class="editor-pane">
        {#if treePanelVisible || breadcrumb}
          <div class="editor-header">
            <span class="breadcrumb">{breadcrumb || '(no selection)'}</span>
            {#if treeState.state.selectedId && treeState.state.selectedId !== GLOBALS_SELECTION_ID && treeState.state.selectedId !== treeState.state.tree.rootId}
              <span class="gizmo-indicator" title="gizmo mode (G/R/S) · space (L)">
                {gizmoMode[0]}·{gizmoSpace === 'world' ? 'W' : 'L'}
              </span>
            {/if}
            {#if !treePanelVisible}
              <button
                class="add-node-btn"
                title="add a sibling node"
                onclick={() => {
                  const newId = treeState.createNode({ name: 'node_2' });
                  treeState.setSelected(newId);
                  logGeotoyEvent('editor', 'node_add');
                }}
              >
                + node
              </button>
            {/if}
          </div>
        {/if}
        {#if treeState.state.selectedId && treeState.state.selectedId !== GLOBALS_SELECTION_ID && (treeState.state.tree.nodes[treeState.state.selectedId]?.children.length ?? 0) > 0}
          <NodeInspector
            tree={treeState.state.tree}
            parentId={treeState.state.selectedId}
            {meshCounts}
            {armedRef}
            onselect={id => treeState.setSelected(id)}
            onInstanceTransformChange={handleInstanceTransformChange}
            onArmInstance={armInstance}
            onAddInstance={handleAddInstance}
            onRemoveInstance={handleRemoveInstance}
            onDisableToggle={handleInspectorDisableToggle}
          />
        {/if}
        <EditorPane
          bind:this={editorPane}
          {treeState}
          {persistence}
          {execution}
          {gizmoEditorHooks}
          onRun={runManual}
          onCenterView={() => centerView(viz, renderedObjects)}
          armedHandleId={armedRef?.kind === 'handle' ? armedRef.name : null}
          readouts={gizmoReadouts}
        />
      </div>
      <div class="controls">
        <div class="output">
          {@render replControls()}
          <ReplOutput err={execution.err ?? materialRuntime.err} runStats={execution.runStats} />
        </div>
        {#if userData?.me}
          {#if !userData.initialComposition || userData.me.id === userData.initialComposition.comp.author_id}
            <SaveControls
              comp={userData.initialComposition?.comp}
              getCurrentDoc={persistence.currentDoc}
              activeTreeId={persistence.activeTreeId}
              materials={persistence.materialDefinitions}
              {viz}
              preludeEjected={persistence.preludeEjected}
              environment={persistence.environment}
              onSave={() => {
                persistence.markClean();
                treeState.markSaved();
              }}
              onForked={handleForkedComposition}
              {userData}
            />
          {:else}
            <ReadOnlyCompositionDetails
              comp={userData.initialComposition.comp}
              onForked={handleForkedComposition}
            />
          {/if}
        {:else}
          {#if userData?.initialComposition}
            <ReadOnlyCompositionDetails comp={userData.initialComposition.comp} showFork={false} />
          {/if}
          <div class="not-logged-in" style="border-top: 1px solid #333">
            <span style="color: #ddd">you must be logged in to save/share compositions</span>
            <div>
              <a href={resolve('/geotoy/login')}>log in</a>
              /
              <a href={resolve('/geotoy/register')}>register</a>
            </div>
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style lang="css">
  .root {
    width: 100%;
    position: absolute;
    max-width: 100vw;
    overflow-x: hidden;
    bottom: 0;
    display: flex;
    flex-direction: column;
    color: #efefef;
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
    font-size: 15px;
  }

  .root.horizontal {
    width: auto;
    height: 100%;
    max-width: none;
    max-height: 100vh;
    overflow-x: auto;
    overflow-y: hidden;
    bottom: 0;
    right: 0;
    left: auto;
    top: 0;
    flex-direction: row;
  }

  .root.horizontal.collapsed {
    flex-direction: column;
    width: 36px;
    overflow: hidden;
  }

  .dragger {
    width: 100%;
    height: 5px;
    position: absolute;
    top: -2px;
    left: 0;
    cursor: ns-resize;
    z-index: 2;
  }

  .dragger.horizontal {
    width: 5px;
    height: 100%;
    top: 0;
    left: -2px;
    cursor: ew-resize;
  }

  .editor-container {
    display: flex;
    flex-direction: row;
    flex: 1;
    min-height: 0;
  }

  .editor-container.horizontal {
    flex-direction: column;
    min-height: 0;
    min-width: 0;
  }

  .output {
    display: flex;
    flex-direction: column;
    flex: 1;
    padding: 8px;
    overflow-y: auto;
    min-height: 80px;
  }

  .tree-pane {
    display: flex;
    flex-direction: column;
    flex: 0 0 200px;
    width: 200px;
    min-width: 0;
    border-right: 1px solid #444;
    overflow-y: auto;
    overflow-x: hidden;
    background: #1a1a1a;
  }

  .tree-pane.horizontal {
    /* Height follows content (basis: auto, no grow); can still shrink + scroll when
     * the tree is taller than the pane. `flex: 0` would collapse it (basis 0%). */
    flex: 0 1 auto;
    width: auto;
    min-height: 0;
    border-right: none;
    border-bottom: 1px solid #444;
  }

  .editor-pane {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }

  .editor-header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 2px 8px;
    border-bottom: 1px solid #333;
    background: #1a1a1a;
    font-size: 11px;
    color: #aaa;
    flex-shrink: 0;
    min-height: 22px;
  }

  .breadcrumb {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: inherit;
  }

  .gizmo-indicator {
    color: #888;
    font-size: 10px;
    font-family: monospace;
    border: 1px solid #333;
    padding: 0 4px;
    line-height: 14px;
    flex-shrink: 0;
    user-select: none;
  }

  .add-node-btn {
    background: #1c1c1c;
    color: #ddd;
    border: 1px solid #444;
    padding: 0 6px;
    font-size: 11px;
    cursor: pointer;
    font-family: inherit;
    line-height: 16px;
  }

  .add-node-btn:hover {
    background: #2a2a2a;
    border-color: #666;
  }

  .controls {
    display: flex;
    flex-direction: column;
    min-width: 200px;
    flex: 0.4;
    border-top: 1px solid #444;
    overflow-y: auto;
  }

  .horizontal .controls {
    border-top: none;
    border-left: 1px solid #444;
    flex: 0.5;
    min-width: 180px;
  }

  .not-logged-in {
    font-size: 13px;
    padding: 8px;
  }

  @media (max-width: 768px) {
    .editor-container {
      flex-direction: column;
    }

    .output {
      padding: 4px;
    }

    .controls {
      flex: 1;
      border-top: none;
      border-left: 1px solid #444;
    }

    .not-logged-in {
      font-size: 12px;
      padding: 4px;
    }

    .output {
      overflow-x: hidden;
    }
  }
</style>
