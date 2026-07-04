<script lang="ts">
  import * as THREE from 'three';
  import { onDestroy, onMount, untrack } from 'svelte';
  import type { EditorView, KeyBinding } from '@codemirror/view';
  import { resolve } from '$app/paths';

  import type { Viz } from 'src/viz';
  import type { WorkerManager } from 'src/geoscript/workerManager';
  import { getGeoscriptWorkerWasmURLs } from 'src/viz/wasmComp/wasmAssetURLs';
  import { buildEditor } from '../../../geoscript/editor';
  import type { GeoscriptPlaygroundUserData } from './geoscriptPlayground.svelte';
  import SaveControls from './SaveControls.svelte';
  import { goto } from '$app/navigation';
  import { type ReplCtx, type RunStats } from './types';
  import ReplOutput from './ReplOutput.svelte';
  import ReplControls from './ReplControls.svelte';
  import ExportModal from './ExportModal.svelte';
  import { runGeoscript } from 'src/geoscript/runner/runner';
  import {
    HiddenMat,
    NormalMat,
    WireframeMat,
    type MaterialDef,
    type MaterialDefinitions,
  } from 'src/geoscript/materials';
  import MaterialEditor from './materialEditor/MaterialEditor.svelte';
  import EnvironmentSettings from './EnvironmentSettings.svelte';
  import { Textures } from './materialEditor/state.svelte';
  import {
    cloneTransform3,
    type Composition,
    type CompositionVersion,
    type CompositionVersionMetadata,
    type EnvironmentConfig,
    type GizmoValue,
    type Transform3,
    type TreeDef,
  } from 'src/geoscript/geotoyAPIClient';
  import {
    clearSavedState,
    getIsDirty,
    getServerState,
    getView,
    loadState,
    saveNewVersion,
    saveState,
    setLastRunWasSuccessful,
  } from './persistence';
  import { compileTree, buildGizmoValues, buildModuleNameToNodeId } from 'src/geoscript/treeCodegen';
  import { buildEvalResultJson } from './evalResult';
  import { TreeState, GLOBALS_SELECTION_ID } from './treeState.svelte';
  import { buildParentMap, computeMeshCounts, findParentId, getNodeAncestorChain } from './treeOps';
  import HierarchyPanel from './HierarchyPanel.svelte';
  import NodeInspector from './NodeInspector.svelte';
  import { TransformGizmo, type GizmoMode, type GizmoSpace } from './transformGizmo';
  import type { GizmoTargetRef } from 'src/viz/gizmos/gizmoTypes';
  import { scanGizmoHandleIds, scanGizmoHandleOrder } from 'src/geoscript/gizmoScan';
  import { GizmoGhosts, type GhostSpec } from 'src/viz/gizmos/gizmoGhosts';
  import { gizmoColorForIndex } from 'src/viz/gizmos/gizmoPalette';
  import type { GizmoEditorHooks, GizmoReadout } from 'src/geoscript/gizmoExtensions';
  import { installRaycastSelect } from './raycastSelect';
  import { getIsUVUnwrapLoaded } from 'src/viz/wasm/uv_unwrap/uvUnwrap';
  import ReadOnlyCompositionDetails from './ReadOnlyCompositionDetails.svelte';
  import {
    populateScene,
    buildWorldMatrixCache,
    instancePathKey,
  } from 'src/geoscript/runner/geoscriptRunner';
  import { decomposeTransform3, composeTransform3 } from 'src/geoscript/runner/worldMatrixCache';
  import type { MatEntry, RenderedObject, RenderedGizmo } from 'src/geoscript/runner/types';
  import {
    buildCustomMaterials,
    fetchAndSetTextures,
    getReferencedTextureIDs,
  } from './materialLoading.svelte';
  import {
    centerView,
    focusOnSubtree,
    snapView,
    orbit,
    setProjection,
    toggleProjection,
  } from './cameraControls';
  import { buildLightHelpers, toggleAxisHelpers, toggleLightHelpers } from './gizmos';
  import { applyGeoscriptSceneEnvironment } from './sceneEnvironment';
  import { useRecording } from './recording';
  import type { PostprocessingPipelineController } from 'src/viz/postprocessing/defaultPostprocessing';

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

  let repl = $derived(workerManager.getWorker());

  const {
    tree: initialTree,
    materials: initialMatDefs,
    lastRunWasSuccessful,
    view: initialView,
    preludeEjected: initialPreludeEjected,
    environment: initialEnvironment,
  } = $derived(loadState(userData));

  const treeState = new TreeState({
    initial: untrack(() => initialTree),
    savedBaseline: untrack(() => userData?.initialComposition?.version.tree) ?? untrack(() => initialTree),
  });
  treeState.setSelected(untrack(() => initialTree).rootId);

  let failedNodeIds = $state<Set<string>>(new Set());

  const getActiveSource = (): string => {
    const sel = treeState.state.selectedId;
    if (sel === GLOBALS_SELECTION_ID) return treeState.state.tree.globalsSource;
    if (sel && treeState.state.tree.nodes[sel]) return treeState.state.tree.nodes[sel].source;
    return '';
  };
  // Source edits stay out of tree undo: CodeMirror owns per-node text history.
  const writeActiveSource = (source: string): void => {
    const sel = treeState.state.selectedId;
    if (sel === GLOBALS_SELECTION_ID) {
      treeState.setGlobalsSource(source);
    } else if (sel && treeState.state.tree.nodes[sel]) {
      treeState.setSource(sel, source);
    }
  };

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

  let ctxPtr = $state<number | null>(null);

  let isDirty = $state(getIsDirty(untrack(() => providedUserData)));

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
    await saveNewVersion(
      newComp,
      treeState.serialize(),
      viz,
      materialDefinitions,
      preludeEjected,
      environment,
      userData?.initialComposition?.comp?.title ?? 'untitled (fork)',
      userData?.initialComposition?.comp?.description ?? '',
      userData?.initialComposition?.comp?.is_shared ?? false,
      newUserData
    );
    userData = newUserData;
    treeState.markSaved();
    isDirty = false;
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

    repl.init(getGeoscriptWorkerWasmURLs()).then(ptr => {
      ctxPtr = ptr;
    });
  });

  let gizmo = $state<TransformGizmo | null>(null);
  let raycastDisposer: (() => void) | null = null;
  let gizmoTick: (() => void) | null = null;
  let gizmoMode = $state<GizmoMode>('translate');
  let gizmoSpace = $state<GizmoSpace>('local');

  // What the viewport gizmo edits. Defaults to the selected node's first instance, but
  // the inspector / a viewport click can arm any specific instance without changing
  // selection. `armedForSel` (plain) records which selection the default was applied for,
  // so an explicit arm in the same tick isn't clobbered by the selection-tracking effect.
  let armedRef = $state<GizmoTargetRef | null>(null);
  let armedForSel: string | null = null;

  let dragStartTransform: Transform3 | null = null;
  let dragStartHandle: GizmoValue | null = null;
  /** Gizmos reported by the last successful run; feeds handle arming + GC. */
  let lastGizmos: RenderedGizmo[] = [];
  /** Whether the last run produced any gizmos anywhere — gates the ghost-toggle menu item. */
  let hasAnyGizmos = $state(false);
  let showGizmoGhosts = $state(localStorage.getItem('geoscript-gizmo-ghosts') !== 'false');
  let ghosts: GizmoGhosts | null = null;
  let ghostTick: (() => void) | null = null;
  /** Inline-readout subscribers + armed-state pusher, wired once the editor's gizmo extensions install. */
  // Editor push channels, wired once the gizmo extensions install (null until then).
  let dispatchArmed: ((handleId: string | null) => void) | null = null;
  let dispatchValues: ((values: Map<string, GizmoReadout>) => void) | null = null;
  let dispatchValuePatch: ((id: string, readout: GizmoReadout) => void) | null = null;
  /** nodeId → last-scanned {source, handleIds}; skips re-parsing unchanged sources on GC. */
  const handleScanCache = new Map<string, { source: string; ids: Set<string> }>();

  onMount(() => {
    let cancelled = false;
    (async () => {
      while (!viz.orbitControls && !cancelled) {
        await new Promise(r => setTimeout(r, 16));
      }
      if (cancelled) return;
      const orbit = viz.orbitControls!;
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
            isDirty = true;
            runOrFast();
          },
          onHandleChange: (nodeId, handleId, value) => {
            // Store + live readout per drag-tick, but defer the (geometry-changing) re-eval
            // to drag end — per-tick re-runs aren't smooth enough to be worth it.
            treeState.setHandle(nodeId, handleId, value);
            isDirty = true;
            // single-handle, no full rebuild
            dispatchValuePatch?.(handleId, storedReadout(value, axesForHandle(nodeId, handleId)));
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
            // Catches the final state if the last `onTransformChange` was dropped by `isRunning`.
            runOrFast();
          },
        }
      );
      // Resolve a handle's origin/kind/mode from the last run's channel + stored value.
      g.setHandleContextResolver((nodeId, handleId) => {
        const node = treeState.state.tree.nodes[nodeId];
        if (!node) return null;
        const reported = lastGizmos.find(gz => gz.sourceModule === node.name && gz.handleId === handleId);
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
          const hit = gh.pickGhost(raycaster);
          if (!hit) return false;
          gizmoEditorHooks.arm(hit.handleId, hit.kind);
          return true;
        },
        onSelect: (id, instancePath) => {
          if (id === null) {
            // Background click: deselect to root and unarm the gizmo entirely.
            treeState.setSelected(treeState.state.tree.rootId);
            armedForSel = treeState.state.tree.rootId;
            armedRef = null;
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
    armedForSel = treeState.state.selectedId;
    armedRef = { kind: 'instance', nodeId, instanceId };
  };

  // Default-arm the selected node's first instance whenever selection changes. Guarded by
  // `armedForSel` so an explicit arm (raycast/inspector) in the same tick survives.
  $effect(() => {
    const sel = treeState.state.selectedId;
    if (sel === armedForSel) return;
    armedForSel = sel;
    armedRef = defaultArmFor(sel);
  });

  // Keep the gizmo bound to whatever is armed; re-sync after each run (ancestor world
  // transforms refresh). Reading `armedRef`/`lastRunTree` subscribes the effect to both.
  $effect(() => {
    void armedRef;
    void lastRunTree;
    gizmo?.syncTo(armedRef, treeState.state.tree);
  });

  // Mirror the armed handle into the editor so the armed chip highlights (and clears on
  // node switch / instance arm, which reset `armedRef` to a non-handle). Read `armedRef`
  // into a local first: `dispatchArmed?.(…)` would short-circuit arg eval while
  // `dispatchArmed` is still null (pre-import), leaving the effect with no tracked dep.
  $effect(() => {
    const armedHandle = armedRef?.kind === 'handle' ? armedRef.name : null;
    dispatchArmed?.(armedHandle);
  });

  // Rebuild ghosts on discrete changes only (selection / arm / setting / each run); the
  // deep tree reads inside happen untracked so a drag's transform churn doesn't re-fire this.
  $effect(() => {
    void treeState.state.selectedId;
    void armedRef;
    void showGizmoGhosts;
    void lastRunTree;
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
    for (const gz of lastGizmos) {
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

  const publishGizmoReadouts = () => {
    dispatchValues?.(buildGizmoReadouts(treeState.state.selectedId));
  };

  const axesForHandle = (nodeId: string, handleId: string): [boolean, boolean, boolean] => {
    const node = treeState.state.tree.nodes[nodeId];
    const gz = node ? lastGizmos.find(g => g.sourceModule === node.name && g.handleId === handleId) : null;
    return gz?.axes ?? [true, true, true];
  };

  // World matrix of a node's representative (instance-0) copy, root → node inclusive — same
  // anchor `HandleTarget` uses, so a ghost sits exactly where its armed gizmo would.
  const _ghostWorld = new THREE.Matrix4();
  const _ghostScratch = new THREE.Matrix4();
  const nodeWorldMatrix = (nodeId: string): THREE.Matrix4 => {
    _ghostWorld.identity();
    const chain = getNodeAncestorChain(treeState.state.tree, nodeId);
    if (!chain) return _ghostWorld;
    for (let i = chain.length - 1; i >= 0; i -= 1) {
      _ghostWorld.multiply(composeTransform3(_ghostScratch, chain[i].instances[0]));
    }
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
    for (const gz of lastGizmos) {
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
      armedForSel = sel;
      armedRef = { kind: 'handle', nodeId: sel, name: handleId };
      if (kind === 'vec3') setGizmoMode('translate');
      editorView?.contentDOM.blur(); // viewport mode → Ctrl-Z routes to the tree undo stack
    },
    disarm: () => {
      if (armedRef?.kind === 'handle') armedRef = defaultArmFor(treeState.state.selectedId);
    },
    resetHandle: handleId => {
      const sel = treeState.state.selectedId;
      const before = sel ? treeState.captureHandle(sel, handleId) : null;
      if (!sel || before === null) return; // already at default
      treeState.deleteHandle(sel, handleId);
      treeState.recordHandleChange(sel, handleId, before, null);
      isDirty = true;
      publishGizmoReadouts();
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
      isDirty = true;
      publishGizmoReadouts();
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

  let resetEditorHistory: (() => void) | null = null;

  const runUndo = (): boolean => {
    if (!treeState.undo()) return true;
    isDirty = treeState.isDirty();
    runOrFast();
    return true;
  };

  const runRedo = (): boolean => {
    if (!treeState.redo()) return true;
    isDirty = treeState.isDirty();
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

  let err: string | null = $state(null);
  let materialErr: string | null = $state(null);
  let isRunning: boolean = $state(false);
  let runStats: RunStats | null = $state(null);
  let renderedObjects: RenderedObject[] = $state([]);
  // Bumped on cancel(). A run captures its gen up front and bails on any post-
  // await continuation whose gen no longer matches — distinguishes "worker
  // terminated mid-call" from a real eval failure.
  let runGen = 0;
  let lightHelpers: THREE.Object3D[] = $state([]);
  let lastRunTree: TreeDef | null = $state(null);
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
  // come from the live tree so toggles are instant. Precomputed sets keep the
  // per-mesh check O(1).
  $effect(() => {
    const soloId = treeState.state.soloId;
    const renderTree = lastRunTree;
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

  let codemirrorContainer = $state<HTMLDivElement | null>(null);
  let editorView = $state<EditorView | null>(null);

  let didFirstRun = $state(false);
  $effect(() => {
    if (ctxPtr === null) {
      return;
    }

    if (didFirstRun) {
      return;
    }
    didFirstRun = true;

    // if the user closed the tab while the last run was in progress, avoid eagerly running it again in
    // case there was an infinite loop or something
    if (lastRunWasSuccessful) {
      run();
    }
  });

  const beforeUnloadHandler = () =>
    saveState(
      {
        tree: treeState.serialize(),
        materials: materialDefinitions,
        view: getView(viz),
        preludeEjected,
        environment,
      },
      userData
    );

  let lastSwappedSelection: string | null = null;

  const setupEditor = () => {
    if (!codemirrorContainer) {
      if (editorView) {
        beforeUnloadHandler();
        editorView.destroy();
        editorView = null;
        resetEditorHistory = null;
      }
      return;
    }

    if (editorView) {
      return;
    }

    const customKeymap: readonly KeyBinding[] = [
      {
        key: 'Ctrl-Enter',
        run: () => {
          if (!editorView) {
            return true;
          }
          run();
          return true;
        },
      },
      {
        key: 'Ctrl-.',
        run: () => {
          centerView(viz, renderedObjects);
          return true;
        },
      },
      {
        key: 'Ctrl-s',
        run: () => {
          saveState(
            {
              tree: treeState.serialize(),
              materials: materialDefinitions,
              view: getView(viz),
              preludeEjected,
            },
            userData
          );
          return true;
        },
      },
    ];

    const editor = buildEditor({
      container: codemirrorContainer,
      customKeymap,
      initialCode: untrack(() => getActiveSource()),
      onDocChange: () => {
        if (editorView) {
          writeActiveSource(editorView.state.doc.toString());
        }
        // Recompute (not just `= true`) so CM undo back to the saved baseline clears dirty.
        isDirty = treeState.isDirty();
      },
    });
    editorView = editor.editorView;
    resetEditorHistory = editor.resetHistory;
    lastSwappedSelection = untrack(() => treeState.state.selectedId);

    import('../../../geoscript/analysisExtensions').then(({ buildAnalysisExtensions }) => {
      // Expose the `_globals` node as ambient scope so its helpers/constants resolve in other
      // nodes; '' while editing `_globals` itself so it's analyzed directly.
      const getAmbientSource = () =>
        treeState.state.selectedId === GLOBALS_SELECTION_ID ? '' : treeState.state.tree.globalsSource;
      editor.setAnalysisExtensions(buildAnalysisExtensions(() => !preludeEjected, getAmbientSource));
    });

    import('../../../geoscript/gizmoExtensions').then(
      ({ buildGizmoExtensions, pushGizmoArmed, pushGizmoValues, pushGizmoValue }) => {
        editor.setGizmoExtensions(buildGizmoExtensions(gizmoEditorHooks));
        dispatchArmed = h => editorView && pushGizmoArmed(editorView, h);
        dispatchValues = m => editorView && pushGizmoValues(editorView, m);
        dispatchValuePatch = (id, r) => editorView && pushGizmoValue(editorView, id, r);
        dispatchArmed(gizmoEditorHooks.getArmedHandleId());
        publishGizmoReadouts(); // seed inline readouts
      }
    );
  };

  // Swap the editor doc on selection change; clear CM undo so Ctrl-Z can't
  // rewind past the swap.
  $effect(() => {
    const sel = treeState.state.selectedId;
    if (sel === lastSwappedSelection) return;
    if (!editorView) {
      lastSwappedSelection = sel;
      return;
    }
    const newSource = untrack(() => getActiveSource());
    editorView.dispatch({
      changes: { from: 0, to: editorView.state.doc.length, insert: newSource },
      selection: { anchor: 0 },
    });
    resetEditorHistory?.();
    lastSwappedSelection = sel;
    publishGizmoReadouts();
  });

  onDestroy(() => {
    if (editorView) {
      beforeUnloadHandler();
      editorView.destroy();
    }
  });

  $effect(setupEditor);

  let materialOverride = $state<'wireframe' | 'wireframe-xray' | 'normal' | null>(null);

  const restoreMaterials = () => {
    for (const obj of renderedObjects) {
      if (obj instanceof THREE.Mesh) {
        obj.material = customMaterialsByName[obj.userData.materialName]?.resolved ?? HiddenMat;
      }
    }
    if (pipelineController?.depthPrePassMaterial) {
      pipelineController.depthPrePassMaterial.polygonOffset = false;
    }
    pipelineController?.setDepthPrePassEnabled(true);
  };

  const applyWireframeMaterial = () => {
    for (const obj of renderedObjects) {
      if (obj instanceof THREE.Mesh) {
        obj.material = WireframeMat;
      }
    }
  };

  const toggleWireframe = () => {
    const wasWireframe = materialOverride === 'wireframe';
    if (materialOverride) {
      restoreMaterials();
      materialOverride = null;
    }
    if (wasWireframe) {
      return;
    }

    materialOverride = 'wireframe';
    applyWireframeMaterial();
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
      restoreMaterials();
      materialOverride = null;
    }
    if (wasXray) {
      return;
    }

    materialOverride = 'wireframe-xray';
    applyWireframeMaterial();
    pipelineController?.setDepthPrePassEnabled(false);
  };

  const toggleNormalMat = () => {
    const wasNormal = materialOverride === 'normal';
    if (materialOverride) {
      restoreMaterials();
      materialOverride = null;
    }
    if (wasNormal) {
      return;
    }

    materialOverride = 'normal';
    for (const obj of renderedObjects) {
      if (obj instanceof THREE.Mesh) {
        obj.material = NormalMat;
      }
    }
  };

  let materialEditorOpen = $state(false);
  let environmentSettingsOpen = $state(false);
  let cameraProjection = $state<'perspective' | 'orthographic'>('perspective');
  let materialDefinitions = $state<MaterialDefinitions>(untrack(() => initialMatDefs));
  let preludeEjected = $state(untrack(() => initialPreludeEjected));
  let environment = $state<EnvironmentConfig | undefined>(untrack(() => initialEnvironment));

  onMount(() => {
    const referencedTextureIDs = getReferencedTextureIDs(materialDefinitions.materials);
    if (environment?.kind === 'equirect' && environment.textureId >= 0) {
      referencedTextureIDs.push(environment.textureId);
    }
    if (referencedTextureIDs.length > 0) {
      fetchAndSetTextures(loader, referencedTextureIDs);
    }
  });

  let lastMaterialsKey: string | null = null;
  let lastMaterialsCtxPtr: number | null = null;
  $effect(() => {
    if (ctxPtr === null) {
      return;
    }

    const materialNames = Object.values(materialDefinitions.materials).map(mat => mat.name);
    const key = `${materialDefinitions.defaultMaterialID ?? ''}|${materialNames.join('\u0000')}`;
    if (ctxPtr === lastMaterialsCtxPtr && key === lastMaterialsKey) {
      return;
    }

    repl.setMaterials(ctxPtr, materialDefinitions.defaultMaterialID, materialNames);
    lastMaterialsKey = key;
    lastMaterialsCtxPtr = ctxPtr;
  });

  const loader = new THREE.ImageBitmapLoader();
  let customMaterials: Record<string, MatEntry> = $derived.by(() => {
    // Re-run when async-fetched texture metadata arrives.
    void Textures.textures;
    // `$state.snapshot` seems required here in order to trigger this derived to actually run when things change
    return buildCustomMaterials(
      loader,
      $state.snapshot(materialDefinitions.materials) as Record<string, MaterialDef>,
      viz,
      // queueMicrotask: synchronous `$state` writes inside `$derived.by` are disallowed.
      msg => queueMicrotask(() => (materialErr = msg))
    );
  });

  // avoid a ton of before render callbacks from being stuck around, which also prevents
  // old materials from being garbage collected
  $effect(() => {
    const customMatVals = Object.values(customMaterials);
    return () => {
      for (const matEntry of customMatVals) {
        if (matEntry.beforeRenderCb) {
          viz.unregisterBeforeRenderCb(matEntry.beforeRenderCb);
        }
      }
    };
  });

  let didInitMats = false;
  $effect(() => {
    // force dependency
    if ($state.snapshot(materialDefinitions)) {
      if (!didInitMats) {
        didInitMats = true;
      } else {
        isDirty = true;
      }
    } else {
      throw new Error('unreachable');
    }
  });

  // Re-apply on env change, texture-metadata arrival, and after each run rebuilds
  // materials (the `void` refs are the deps). PMREM is cached, so this is cheap.
  $effect(() => {
    void Textures.textures;
    void renderedObjects;
    const env = $state.snapshot(environment) as EnvironmentConfig | undefined;
    void applyGeoscriptSceneEnvironment(viz, loader, env, id => Textures.textures[id]?.url);
  });

  // Editing the environment marks the composition dirty (mirrors materials).
  let didInitEnv = false;
  $effect(() => {
    void $state.snapshot(environment);
    if (!didInitEnv) {
      didInitEnv = true;
    } else {
      isDirty = true;
    }
  });

  let customMaterialsByName: Record<
    string,
    { promise: Promise<THREE.Material>; resolved: THREE.Material | null }
  > = $derived.by(() => {
    const matsByName: Record<string, { promise: Promise<THREE.Material>; resolved: THREE.Material | null }> =
      {};
    for (const [id, def] of Object.entries($state.snapshot(materialDefinitions.materials))) {
      matsByName[def.name] = customMaterials[id];
    }
    return matsByName;
  });

  $effect(() => {
    const pendingSwaps: Promise<unknown>[] = [];
    for (const obj of renderedObjects) {
      if (!(obj instanceof THREE.Mesh)) {
        continue;
      }

      for (const [id, matEntry] of Object.entries(customMaterials)) {
        if (obj.material.name === id) {
          if (matEntry.resolved) {
            obj.material = matEntry.resolved;
          } else {
            pendingSwaps.push(
              matEntry.promise.then(mat => {
                obj.material = mat;
              })
            );
          }
          break;
        }
      }
    }
    // Material swaps invalidate the bounded-silhouette manager's per-mesh
    // registry, so reconcile after swaps settle.
    const reconcile = () => viz.postprocessingController?.rescanPomMeshes();
    if (pendingSwaps.length === 0) {
      reconcile();
    } else {
      Promise.allSettled(pendingSwaps).then(reconcile);
    }
  });

  let lastRunOutcome = $derived(
    (() => {
      if (err) {
        return { type: 'err' as const, err };
      }
      if (runStats) {
        return { type: 'ok' as const, stats: runStats };
      }
      return null;
    })()
  );

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

  const extractFailedModuleName = (msg: string): string | null => {
    const m = msg.match(/module\s+["']([^"']+)["']/i);
    return m ? m[1] : null;
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
      // `handles` matters: a gizmo value can change geometry, so it must force re-eval.
      parts.push(
        `n:${k}:${n.name}:${n.disabled ? 1 : 0}:${n.instances.length}:${n.source}:${n.children.join(',')}:${JSON.stringify(n.handles ?? null)}`
      );
    }
    parts.push(`pe:${preludeEjected ? 1 : 0}`);
    const matIds = Object.keys(materialDefinitions.materials).sort();
    for (const id of matIds) {
      parts.push(`m:${id}:${JSON.stringify(materialDefinitions.materials[id])}`);
    }
    parts.push(`dm:${materialDefinitions.defaultMaterialID ?? ''}`);
    return parts.join('\x00');
  };

  let lastEvalInputsHash: string | null = null;
  // Set for the duration of a gizmo drag, where the tree structure is frozen, so the
  // fast path can skip the eval-hash recompute + parent-map rebuild every frame.
  let dragSession: { parentMap: Map<string, string> } | null = null;
  const _fastScratch = new THREE.Matrix4();

  /** Recompose each mesh's `ancestor × localInScript` if only transforms changed. */
  const tryTransformOnlyFastPath = (): boolean => {
    if (isRunning) return false;
    if (lastEvalInputsHash === null) return false;
    const drag = dragSession;
    if (!drag && computeEvalInputsHash() !== lastEvalInputsHash) return false;

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
    // Skip the snapshot mid-drag: structure is frozen, so `lastRunTree`-derived effects
    // (solo/disabled visibility) needn't re-run; `onDragEnd`'s final run refreshes it.
    if (!drag) lastRunTree = treeState.serialize();
    return true;
  };

  const runOrFast = () => {
    if (tryTransformOnlyFastPath()) return;
    run();
  };

  const run = async () => {
    if (isRunning || ctxPtr === null) {
      return;
    }

    beforeUnloadHandler();

    const myGen = runGen;
    isRunning = true;
    err = null;
    failedNodeIds = new Set();

    // Defer disposal until after populate so unchanged objects can be reused.
    const prevObjects = renderedObjects;
    const prevByReuseKey = new Map<string, RenderedObject>();
    for (const obj of prevObjects) {
      const key = obj.userData.reuseKey as string | undefined;
      if (typeof key === 'string') prevByReuseKey.set(key, obj);
    }
    runStats = null;

    const matsByName: Record<string, { def: MaterialDef; mat: MatEntry }> = {};
    for (const [id, def] of Object.entries(materialDefinitions.materials)) {
      matsByName[def.name] = { def, mat: customMaterials[id] };
    }

    const tree = treeState.serialize();
    const compiled = compileTree(tree);
    const moduleNameToNodeId = buildModuleNameToNodeId(tree);

    try {
      const ambientSources: string[] = [];
      if (!preludeEjected) {
        ambientSources.push(await repl.getPrelude());
      }
      if (tree.globalsSource.trim().length > 0) {
        ambientSources.push(tree.globalsSource);
      }

      setLastRunWasSuccessful(false, userData);
      const result = await runGeoscript({
        code: compiled.rootSource,
        modules: compiled.modules,
        ambientSources,
        ctxPtr,
        repl,
        materials: matsByName,
        includePrelude: !preludeEjected,
        materialOverride,
        renderMode: userData?.renderMode ?? false,
        gizmoValues: buildGizmoValues(tree),
      });

      if (myGen !== runGen) return;

      if (result.error) {
        // Keep the previous scene visible on failure.
        err = result.error;
        const failedModule = extractFailedModuleName(result.error);
        if (failedModule && moduleNameToNodeId[failedModule]) {
          failedNodeIds = new Set([moduleNameToNodeId[failedModule]]);
        }
        isRunning = false;
        return;
      }

      setLastRunWasSuccessful(true, userData);
      runStats = result.stats;
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
      lastRunTree = tree;

      // populateScene's own `materialPromise.then` swaps `HiddenMat` for the
      // real material later, but that mutation isn't reactive — rescan manually.
      const pendingMatPromises = Object.values(customMaterials)
        .filter(m => !m.resolved)
        .map(m => m.promise);
      if (pendingMatPromises.length > 0) {
        Promise.allSettled(pendingMatPromises).then(() => {
          if (myGen !== runGen) {
            return;
          }
          viz.postprocessingController?.rescanPomMeshes();
        });
      }

      const directCounts = new Map<string, number>();
      for (const obj of renderedObjects) {
        if (!(obj instanceof THREE.Mesh)) continue;
        const id = obj.userData.sourceNodeId as string | undefined;
        if (!id) continue;
        directCounts.set(id, (directCounts.get(id) ?? 0) + 1);
      }
      meshCounts = computeMeshCounts(tree, directCounts);

      lastGizmos = result.gizmos;
      hasAnyGizmos = result.gizmos.length > 0;
      publishGizmoReadouts();
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

      for (const helper of lightHelpers) {
        viz.scene.remove(helper);
      }
      if (localStorage['geoscript-light-helpers'] === 'true') {
        lightHelpers = buildLightHelpers(viz, renderedObjects);
      } else {
        lightHelpers = [];
      }

      lastEvalInputsHash = computeEvalInputsHash();
      isRunning = false;
    } catch (e) {
      if (myGen !== runGen) {
        return;
      }
      console.error('geoscript run failed', e);
      err = `Run failed: ${e instanceof Error ? e.message : String(e)}`;
      isRunning = false;
    }
  };

  const handleInstanceTransformChange = (nodeId: string, instanceId: string, transform: Transform3) => {
    const before = treeState.captureInstanceTransform(nodeId, instanceId);
    if (!before) return;
    treeState.setInstanceTransform(nodeId, instanceId, transform);
    treeState.recordInstanceTransformChange(nodeId, instanceId, before, transform);
    isDirty = true;
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
    isDirty = true;
    runOrFast();
    if (newId) armInstance(nodeId, newId);
  };

  const handleRemoveInstance = (nodeId: string, instanceId: string) => {
    treeState.removeInstance(nodeId, instanceId);
    if (armedRef?.kind === 'instance' && armedRef.nodeId === nodeId && armedRef.instanceId === instanceId) {
      armedRef = defaultArmFor(treeState.state.selectedId);
    }
    isDirty = true;
    runOrFast();
  };

  const handleInspectorDisableToggle = (id: string, disabled: boolean) => {
    treeState.setDisabled(id, disabled);
    isDirty = true;
  };

  const cancel = async () => {
    if (!isRunning) {
      return;
    }

    runGen++;
    workerManager.terminate();

    for (const obj of renderedObjects) {
      removeRenderedObject(obj);
    }
    renderedObjects = [];
    runStats = null;

    repl = await workerManager.recreate();

    ctxPtr = await repl.init(getGeoscriptWorkerWasmURLs());

    err = 'Execution interrupted';
    isRunning = false;
  };

  const rerun = async (onlyIfUVUnwrapperNotLoaded: boolean) => {
    if (onlyIfUVUnwrapperNotLoaded && getIsUVUnwrapLoaded()) {
      return;
    }
    return run();
  };

  const toggleEditorCollapsed = () => {
    saveState(
      {
        tree: treeState.serialize(),
        materials: materialDefinitions,
        view: getView(viz),
        preludeEjected,
        environment,
      },
      userData
    );
    isEditorCollapsed = !isEditorCollapsed;
    onSizeChange(size, isEditorCollapsed, layoutOrientation);
  };

  const ejectPrelude = async (editorView: EditorView) => {
    const prelude = await repl.getPrelude();
    editorView.dispatch({
      changes: { from: 0, insert: prelude + '\n//-- end prelude\n\n' },
    });
  };

  // Tree mode: the prelude is render-only (default lights), and renders fired in `_globals`
  // (ambient scope) are dropped — so it has to land in a real scene-eval'd node. The root is
  // that node; dump it there and focus it so it's clear where the code went.
  const ejectPreludeIntoRoot = async () => {
    const prelude = await repl.getPrelude();
    const rootId = treeState.state.tree.rootId;
    const cur = treeState.state.tree.nodes[rootId]?.source ?? '';
    const newSource = prelude + '\n//-- end prelude\n\n' + cur;
    treeState.setSource(rootId, newSource);
    treeState.setSelected(rootId);
    if (editorView) {
      // Force the swap even if the root was already selected (the doc-swap effect no-ops then).
      editorView.dispatch({
        changes: { from: 0, to: editorView.state.doc.length, insert: newSource },
        selection: { anchor: 0 },
      });
      resetEditorHistory?.();
      lastSwappedSelection = rootId;
    }
  };

  const togglePreludeEjected = async () => {
    if (!editorView) {
      return;
    }

    if (!preludeEjected) {
      if (Object.keys(treeState.state.tree.nodes).length > 1) {
        await ejectPreludeIntoRoot();
      } else {
        await ejectPrelude(editorView);
      }
    }
    preludeEjected = !preludeEjected;

    run();
  };

  let exportDialog = $state<HTMLDialogElement | null>(null);
  const onExport = () => {
    exportDialog?.showModal();
  };

  const setView = async (view: CompositionVersionMetadata['view']) => {
    while (!viz.orbitControls) {
      await new Promise(resolve => setTimeout(resolve, 10));
    }

    if (view.cameraPosition) {
      viz.camera.position.set(...view.cameraPosition);
    }
    if (view.target) {
      viz.orbitControls.target.set(...view.target);
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
    viz.camera.lookAt(viz.orbitControls.target);
    viz.orbitControls.update();
  };

  const handleToggleProjection = () => {
    cameraProjection = toggleProjection(viz);
    isDirty = true;
    saveState(
      {
        tree: treeState.serialize(),
        materials: materialDefinitions,
        view: getView(viz),
        preludeEjected,
        environment,
      },
      userData
    );
  };

  const clearLocalChanges = () => {
    if (isDirty && !confirm('Really clear local changes?')) {
      return;
    }

    clearSavedState(userData);

    const serverState = getServerState(userData);

    treeState.replaceTree(serverState.tree);
    treeState.setSelected(serverState.tree.rootId);

    didInitMats = false;

    materialDefinitions = serverState.materials;
    const referencedTextureIDs = getReferencedTextureIDs(materialDefinitions.materials);
    if (serverState.environment?.kind === 'equirect' && serverState.environment.textureId >= 0) {
      referencedTextureIDs.push(serverState.environment.textureId);
    }
    fetchAndSetTextures(loader, referencedTextureIDs).then(() => {
      didInitMats = false;
      materialDefinitions = { ...serverState.materials };
    });

    if (serverState.view) {
      setView(serverState.view);
    }
    preludeEjected = serverState.preludeEjected;
    environment = serverState.environment;

    run();

    saveState(
      {
        tree: serverState.tree,
        materials: serverState.materials,
        view: serverState.view,
        preludeEjected: serverState.preludeEjected,
        environment: serverState.environment,
      },
      userData
    );

    isDirty = false;
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

    setTimeout(() => setView(initialView));

    setReplCtx({
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
      getLastRunOutcome: () => lastRunOutcome,
      getAreAllMaterialsLoaded: () => Object.values(customMaterials).every(mat => mat.resolved),
      run,
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
        isDirty = true;
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
        if (ctxPtr === null) throw new Error('no geoscript context');
        return buildEvalResultJson({
          repl,
          ctxPtr,
          renderedObjects,
          tree: treeState.state.tree,
          stats: runStats,
          req,
        });
      },
    });

    window.addEventListener('beforeunload', beforeUnloadHandler);

    return () => {
      workerManager.terminate();

      for (const mesh of renderedObjects) {
        removeRenderedObject(mesh);
      }

      window.removeEventListener('beforeunload', beforeUnloadHandler);
    };
  });

  const goHome = () => {
    beforeUnloadHandler();

    if (isDirty) {
      if (!confirm('You have unsaved changes. Really leave page?')) {
        return;
      }
    }

    workerManager.terminate();

    goto(resolve('/geotoy'));
  };
</script>

<svelte:window bind:innerWidth />

<ExportModal bind:dialog={exportDialog} {renderedObjects} />
<MaterialEditor
  bind:isOpen={materialEditorOpen}
  bind:materials={materialDefinitions}
  {rerun}
  {repl}
  {ctxPtr}
  me={userData?.me}
/>

<EnvironmentSettings bind:isOpen={environmentSettingsOpen} bind:environment me={userData?.me} />

{#if isEditorCollapsed}
  <div
    class={['root', 'collapsed', layoutOrientation === 'horizontal' ? 'horizontal' : '']}
    style={`${userData?.renderMode ? 'visibility: hidden; height: 0;' : ''} ${layoutOrientation === 'horizontal' ? 'width: 36px;' : 'height: 36px;'}`}
  >
    <ReplControls
      {isRunning}
      {isEditorCollapsed}
      {run}
      {cancel}
      {toggleEditorCollapsed}
      {goHome}
      {err}
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
      {isDirty}
      {preludeEjected}
      {togglePreludeEjected}
      toggleMaterialEditorOpen={() => (materialEditorOpen = true)}
      toggleEnvironmentSettingsOpen={() => (environmentSettingsOpen = true)}
      {toggleLayoutOrientation}
    />
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
            {failedNodeIds}
            onselect={id => treeState.setSelected(id)}
            onsoloToggle={id => treeState.setSolo(treeState.state.soloId === id ? null : id)}
            onDisableToggle={id => {
              const node = treeState.state.tree.nodes[id];
              if (node) treeState.setDisabled(id, !node.disabled);
              isDirty = true;
            }}
            oncreate={parentId => {
              const newId = treeState.createNode({ parentId: parentId ?? undefined });
              treeState.setSelected(newId);
              isDirty = true;
            }}
            ondelete={id => {
              treeState.deleteNode(id);
              isDirty = true;
            }}
            onrename={(id, newName) => {
              try {
                treeState.rename(id, newName);
                isDirty = true;
                return true;
              } catch (err) {
                console.warn('rename failed:', err);
                return false;
              }
            }}
            onreparent={(id, newParentId) => {
              try {
                treeState.reparent(id, newParentId);
                isDirty = true;
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
                  isDirty = true;
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
        <div
          bind:this={codemirrorContainer}
          class="codemirror-wrapper"
          style="flex: 1; background: #222;"
        ></div>
      </div>
      <div class="controls">
        <div class="output">
          <ReplControls
            {isRunning}
            {isEditorCollapsed}
            {run}
            {cancel}
            {toggleEditorCollapsed}
            {goHome}
            {err}
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
            {isDirty}
            {preludeEjected}
            {togglePreludeEjected}
            toggleMaterialEditorOpen={() => {
              materialEditorOpen = !materialEditorOpen;
            }}
            toggleEnvironmentSettingsOpen={() => {
              environmentSettingsOpen = !environmentSettingsOpen;
            }}
            {toggleLayoutOrientation}
          />
          <ReplOutput err={err ?? materialErr} {runStats} />
        </div>
        {#if userData?.me}
          {#if !userData.initialComposition || userData.me.id === userData.initialComposition.comp.author_id}
            <SaveControls
              comp={userData.initialComposition?.comp}
              getCurrentTree={() => treeState.serialize()}
              materials={materialDefinitions}
              {viz}
              {preludeEjected}
              {environment}
              onSave={() => {
                isDirty = false;
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

  .codemirror-wrapper {
    display: flex;
    flex: 1;
    width: 100%;
    min-width: 0;
    overflow-x: auto;
    background: #222;
  }

  :global(.codemirror-wrapper > div) {
    display: flex;
    flex: 1;
    width: 100%;
    min-width: 0;
    box-sizing: border-box;
  }

  :global(.cm-content) {
    padding-top: 0 !important;
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

    .codemirror-wrapper {
      flex: 1;
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
