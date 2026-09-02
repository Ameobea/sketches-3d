<script lang="ts">
  import * as THREE from 'three';
  import { onMount, untrack, tick } from 'svelte';
  import { resolve } from '$app/paths';

  import type { Viz } from 'src/viz';
  import type { WorkerManager } from 'src/geoscript/workerManager';
  import type { GeoscriptPlaygroundUserData } from 'src/viz/scenes/geoscriptPlayground/geoscriptPlayground.svelte';
  import SaveControls from 'src/geotoy/panels/SaveControls.svelte';
  import SavePopover from 'src/geotoy/panels/SavePopover.svelte';
  import { goto } from '$app/navigation';
  import { startRenderHarness } from 'src/geotoy/renderHarness';
  import RunBar from 'src/geotoy/panels/RunBar.svelte';
  import TabStrip from 'src/geotoy/panels/TabStrip.svelte';
  import RunOutput from 'src/geotoy/panels/RunOutput.svelte';
  import type { VectorizeRow, VectorizeSummary } from 'src/geotoy/panels/RunOutput.svelte';
  import type { VectorizeMarker } from 'src/geoscript/vectorizeMarkers';
  import type { VectorizeReport } from 'src/geoscript/runner/runner';
  import Menubar, { type Menu } from 'src/geotoy/panels/Menubar.svelte';
  import EditorPane from 'src/geotoy/panels/EditorPane.svelte';
  import ExportModal from 'src/geotoy/panels/ExportModal.svelte';
  import UvMapsOverlay from 'src/geotoy/panels/UvMapsOverlay.svelte';
  import { GeoscriptExecution, type RunInput } from 'src/geotoy/modules/execution.svelte';
  import { HiddenMat, type MaterialDef } from 'src/geoscript/materials';
  import MaterialEditor from 'src/geotoy/panels/materialEditor/MaterialEditor.svelte';
  import EnvironmentSettings from 'src/geotoy/modes/mesh/EnvironmentSettings.svelte';
  import {
    cloneTransform3,
    ROOT_NODE_NAME,
    type Composition,
    type CompositionVersion,
    type Transform3,
    type TreeDef,
    type TreeKind,
  } from 'src/geoscript/geotoyAPIClient';
  import { GeotoyPersistence } from 'src/geotoy/modules/persistence.svelte';
  import { GeotoyKeymap } from 'src/geotoy/modules/keymap';
  import {
    buildCoreKeymap,
    buildMeshKeymap,
    buildTextureKeymap,
    type GeotoyKeymapActions,
  } from 'src/geotoy/modules/keymapTable';
  import {
    compileTree,
    compileTreeModules,
    buildInjectedValues,
    buildModuleNameToNodeId,
    qualifyModuleName,
    referencedTabIds,
    moduleSourceLineOffset,
    controlValueToWire,
  } from 'src/geoscript/treeCodegen';
  import { applySourceEdits, getAnalysisClient, type SourceEdit } from 'src/geoscript/analysisClient';
  import { showToast } from 'src/viz/util/GlobalToastState.svelte';
  import {
    proceduralOutputOptions,
    proceduralRefTabIds,
    pruneProceduralTextures,
    textureOutputsByTab,
    uploadProceduralTextures,
  } from 'src/geotoy/modules/proceduralTextures';
  import ControlsPanel from 'src/geotoy/panels/ControlsPanel.svelte';
  import { setTopLeftSlot, TopLeftSlot } from 'src/geotoy/modules/topLeftOverlay.svelte';
  import { GLOBALS_SELECTION_ID } from 'src/geotoy/modules/treeState.svelte';
  import { GeotoyTabs } from 'src/geotoy/modules/tabs.svelte';
  import { togglePreludeEjected as togglePrelude } from 'src/geotoy/modules/preludeEject';
  import { buildParentMap, findParentId } from 'src/geotoy/modules/treeOps';
  import HierarchyPanel from 'src/geotoy/panels/HierarchyPanel.svelte';
  import NodeInspector from 'src/geotoy/panels/NodeInspector.svelte';
  import {
    buildWorldMatrixCache,
    disposeRunObjects,
    instancePathKey,
  } from 'src/geoscript/runner/geoscriptRunner';
  import type {
    GeneratedTexture,
    RenderedGizmo,
    RenderedControl,
    TextureParamsEntry,
  } from 'src/geoscript/runner/types';
  import { GizmoController } from 'src/geotoy/modes/mesh/gizmoController.svelte';
  import { SplineController } from 'src/geotoy/modes/mesh/splineController.svelte';
  import { MeshScene } from 'src/geotoy/modes/mesh/meshScene.svelte';
  import { TextureMode } from 'src/geotoy/modes/texture/textureMode.svelte';
  import Preview3dHud from 'src/geotoy/modes/texture/Preview3dHud.svelte';
  import PreviewObjectPicker from 'src/geotoy/modes/texture/PreviewObjectPicker.svelte';
  import {
    buildPreviewModuleSource,
    PREVIEW_MODULE_NAME,
    resolvePreviewTarget,
    type PreviewTargetResolution,
  } from 'src/geotoy/modes/texture/previewTarget';
  import TexturePlaceholder from 'src/geotoy/modes/texture/TexturePlaceholder.svelte';
  import TexturePreview from 'src/geotoy/modes/texture/TexturePreview.svelte';
  import type { Mode, StatusMetric } from 'src/geotoy/modes/mode';
  import { snapView, orbit, untilOrbitControls } from 'src/geotoy/modes/mesh/cameraControls';
  import { toggleAxisHelpers } from 'src/geotoy/modes/mesh/gizmos';
  import { useRecording } from 'src/geotoy/modes/mesh/recording';
  import type { PostprocessingPipelineController } from 'src/viz/postprocessing/defaultPostprocessing';
  import { logGeotoyEvent } from 'src/analytics';

  let {
    viz,
    workerManager,
    userData: providedUserData,
    pipelineController,
  }: {
    viz: Viz;
    workerManager: WorkerManager;
    userData?: GeoscriptPlaygroundUserData;
    pipelineController: PostprocessingPipelineController | null;
  } = $props();

  let userData = $state<GeoscriptPlaygroundUserData | undefined>(untrack(() => providedUserData));

  /** Full-width bottom bar: tab strip + run bar. */
  const BOTTOM_BAR_HEIGHT = 32;
  /** Only used with the editor collapsed: there is no panel to expand into, so the run
   *  output falls back to overlaying at a readable fixed width. */
  const COLLAPSED_RUN_OUTPUT_WIDTH = 520;

  const storedPanelSize = (orientation: 'vertical' | 'horizontal'): number =>
    orientation === 'horizontal'
      ? Number(localStorage.getItem('geoscript-repl-width')) || Math.max(400, 0.35 * window.innerWidth)
      : Number(localStorage.getItem('geoscript-repl-height')) || Math.max(250, 0.25 * window.innerHeight);

  let savePopoverOpen = $state(false);
  let runOutputExpanded = $state(localStorage.getItem('geoscript-run-output-expanded') === 'true');
  /** Transient force-open from an error; collapsing clears it without touching the pref. */
  let runOutputForced = $state(false);
  const toggleRunOutput = () => {
    if (runOutputForced && !runOutputExpanded) {
      runOutputForced = false;
      return;
    }
    runOutputExpanded = !runOutputExpanded;
    runOutputForced = false;
    localStorage.setItem('geoscript-run-output-expanded', runOutputExpanded ? 'true' : 'false');
  };

  const { toggleRecording, recordingState } = useRecording(
    untrack(() => viz),
    untrack(() => providedUserData)
  );

  let layoutOrientation = $state<'vertical' | 'horizontal'>(
    (localStorage.getItem('geoscriptLayoutOrientation') as 'vertical' | 'horizontal') || 'horizontal'
  );
  $effect(() => {
    localStorage.setItem('geoscriptLayoutOrientation', layoutOrientation);
  });

  const toggleLayoutOrientation = () => {
    layoutOrientation = layoutOrientation === 'vertical' ? 'horizontal' : 'vertical';
    size = storedPanelSize(layoutOrientation);
    updateCanvasSize();
  };

  /** Texel-vectorizer controls (view › vectorization); persisted, all off by default. The
   *  kill switch and verify mode only exist for dev A/B work, so they're localhost-only. */
  const vectorizeDevTools = window.location.hostname.includes('localhost');
  let vectorizePrefs = $state<{ report: boolean; disabled: boolean; verify: boolean }>(
    JSON.parse(localStorage.getItem('geotoyVectorize') ?? 'null') ?? {
      report: false,
      disabled: false,
      verify: false,
    }
  );
  $effect(() => {
    localStorage.setItem('geotoyVectorize', JSON.stringify(vectorizePrefs));
  });
  const vectorizeFlags = $derived({
    disabled: vectorizeDevTools && vectorizePrefs.disabled,
    verify: vectorizeDevTools && vectorizePrefs.verify,
    profile: vectorizePrefs.report,
  });

  /**
   * The single live-capture point for camera state: refresh the active tab from its mode, then
   * hand back the whole record. Resolved by id, not through `tabs.active` — its `tabs[0]`
   * fallback would file the outgoing camera onto an unrelated tab in the window where the
   * active id names a tab that was just removed.
   */
  const collectTabMeta = () => {
    const active = tabs.tabs.find(t => t.id === persistence.activeTreeId);
    const view = active && mode.buildViewState();
    if (active && view) active.view = view;
    return tabs.metaRecord();
  };

  const persistence = new GeotoyPersistence({
    getUserData: () => userData,
    serializeTabs: () => tabs.serialize(),
    isTreeDirty: () => tabs.anyDirty,
    tabShapeKey: () => tabs.shapeKey,
    collectTabMeta,
    peekTabMeta: () => tabs.metaRecord(),
  });
  const keymap = new GeotoyKeymap();
  /** Aborts boot-time orbit-controls waiters if the component unmounts mid-boot. */
  const bootAbort = new AbortController();

  const tabs = new GeotoyTabs({
    doc: persistence.initial.doc,
    tabMeta: persistence.initial.tabMeta,
    serverDoc: untrack(() => userData?.initialComposition?.version.tree) ?? null,
    getActiveId: () => persistence.activeTreeId,
  });
  const treeState = $derived(tabs.active.treeState);

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
  let innerHeight = $state(window.innerHeight);
  let isEditorCollapsed = $state(
    (() => {
      const raw = localStorage.getItem('geoscriptEditorCollapsed');
      return typeof raw === 'string' ? raw === 'true' : innerWidth < 768;
    })()
  );
  $effect(() => {
    localStorage.setItem('geoscriptEditorCollapsed', isEditorCollapsed ? 'true' : 'false');
  });
  // Only on an actual narrow→wide crossing; as a standing rule it would re-expand on every
  // tick, making the panel impossible to collapse at desktop widths.
  let wasNarrow = untrack(() => innerWidth) < 768;
  $effect(() => {
    const narrow = innerWidth < 768;
    if (wasNarrow && !narrow && isEditorCollapsed) {
      isEditorCollapsed = false;
      updateCanvasSize();
    }
    wasNarrow = narrow;
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
    tabs.markAllSaved();
    persistence.markClean();
  };

  let size = $state(storedPanelSize(untrack(() => layoutOrientation)));
  /**
   * The one place panel/bar geometry is derived. The run bar's width *is* `panelSize`, not a
   * second copy of the clamp, so its left border can't drift off the panel's left edge.
   * Collapsing goes to zero — the always-visible bar carries run, so there is no rail.
   */
  const layout = $derived.by(() => {
    // Clamped to 90% of the viewport: the stored/default size predates the current window,
    // and a 400px default exceeds a narrow phone entirely.
    const horizontal = layoutOrientation === 'horizontal';
    const panelSize = isEditorCollapsed ? 0 : Math.min(size, 0.9 * (horizontal ? innerWidth : innerHeight));
    return {
      orientation: layoutOrientation,
      panelSize,
      barHeight: BOTTOM_BAR_HEIGHT,
      /** In vertical the run bar sizes to content instead. */
      runBarWidth: horizontal && !isEditorCollapsed ? panelSize : null,
    };
  });

  // Canvas fills the viewport minus the panel inset and the bottom bar. Registered after
  // the pipeline's own resize cb (constructed before the app mounts), so its setSize wins.
  const updateCanvasSize = () => {
    if (userData?.renderMode) {
      return;
    }

    const { orientation, panelSize, barHeight } = layout;
    const canvasWidth = Math.max(window.innerWidth - (orientation === 'horizontal' ? panelSize : 0), 0);
    const canvasHeight = Math.max(
      window.innerHeight - barHeight - (orientation === 'vertical' ? panelSize : 0),
      0
    );

    if (pipelineController) {
      pipelineController.effectComposer.setSize(canvasWidth, canvasHeight, true);
    } else {
      viz.renderer.setSize(canvasWidth, canvasHeight, true);
    }

    if (viz.camera instanceof THREE.PerspectiveCamera) {
      viz.camera.aspect = canvasWidth / canvasHeight;
    } else if (viz.camera instanceof THREE.OrthographicCamera) {
      const halfH = (viz.camera.top - viz.camera.bottom) / 2;
      const aspect = canvasHeight > 0 ? canvasWidth / canvasHeight : 1;
      viz.camera.left = -halfH * aspect;
      viz.camera.right = halfH * aspect;
    }
    viz.camera.updateProjectionMatrix();
  };
  untrack(() => viz).registerResizeCb(updateCanvasSize);
  updateCanvasSize();

  onMount(() => {
    execution.init().then(() => {
      // If the tab was closed while the last run was in progress, don't eagerly re-run
      // it — it may have been an infinite loop.
      if (persistence.initial.lastRunWasSuccessful) {
        execution.run();
      }
    });
  });

  const splineController = new SplineController({
    viz: untrack(() => viz),
    getTreeState: () => treeState,
    getGizmo: () => gizmoController.gizmo,
    getModuleNameToNodeId: () => lastRun?.moduleNameToNodeId,
    nodeWorldMatrix: id => gizmoController.nodeWorldMatrix(id),
    setGizmoTranslateMode: () => gizmoController.setMode('translate'),
    armNone: () => gizmoController.armNone(),
    runOrFast: () => runOrFast(),
  });
  const gizmoController = new GizmoController({
    viz: untrack(() => viz),
    getTreeState: () => treeState,
    renderMode: () => userData?.renderMode ?? false,
    bootSignal: bootAbort.signal,
    getLastGizmos: () => lastRun?.gizmos,
    getModuleNameToNodeId: () => lastRun?.moduleNameToNodeId,
    getRenderedObjects: () => meshScene.renderedObjects,
    runOrFast: () => runOrFast(),
    blurEditor: () => editorPane?.blur(),
    isSplineActive: () => splineController.activeKey !== null,
    interceptSplineClick: rc => splineController.interceptClick(rc),
  });
  const meshScene = new MeshScene({
    viz: untrack(() => viz),
    getTreeState: () => treeState,
    persistence,
    pipelineController: untrack(() => pipelineController),
    bootSignal: bootAbort.signal,
    getLastRunTree: () => lastRun?.tree ?? null,
    onRunConsumed: splineController.onRunConsumed,
    getEditorHooks: () => gizmoController.editorHooks,
    getEnvironment: () =>
      tabs.active.kind === 'mesh' ? tabs.active.environment : textureMode.previewEnvironment,
    getEmissiveBloom: () =>
      tabs.active.kind === 'mesh' ? tabs.active.emissiveBloom : textureMode.previewEmissiveBloom,
  });
  const textureMode = new TextureMode({
    getTreeState: () => treeState,
    getActiveTabId: () => tabs.active.id,
    getTabs: () => tabs.tabs,
    getMaterialDefs: () => persistence.materialDefinitions.materials,
    viz: untrack(() => viz),
    materialRuntime: meshScene.materialRuntime,
    bootSignal: bootAbort.signal,
    reapplyEnv: () => meshScene.applyEnv(),
    onPreviewChanged: () => {
      persistence.saveDraft();
      void execution.run();
    },
  });
  const modesByKind: Record<TreeKind, Mode> = { mesh: meshScene, texture: textureMode };
  const mode: Mode = $derived(modesByKind[tabs.active.kind]);

  // The 2D texture preview (and its placeholder) is an opaque fixed overlay, so the 3D
  // canvas underneath it is drawing nothing anyone can see.
  $effect(() => {
    viz.frameGovernor?.setSuspended(mode.kind === 'texture' && !textureMode.preview3d);
    // `viz` outlives this component, so a teardown while suspended would leave its loop stopped.
    return () => viz.frameGovernor?.setSuspended(false);
  });

  // Per-mode key table: the action surface is built in onMount, so the table reads it lazily.
  let keymapActions: GeotoyKeymapActions | null = null;
  $effect(() => {
    const get = () => keymapActions;
    keymap.setTable([
      ...buildCoreKeymap(get),
      ...(mode.kind === 'mesh' ? buildMeshKeymap(get) : buildTextureKeymap(get)),
    ]);
  });
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
    vectorizeReports: VectorizeReport[];
  } | null>(null);
  /** Gates the ghost-toggle menu item / controls panel. */
  const hasAnyGizmos = $derived((lastRun?.gizmos.length ?? 0) > 0);
  const hasAnyControls = $derived((lastRun?.controls.length ?? 0) > 0);

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

  let editorPane = $state<{
    blur: () => void;
    revealLoc: (line: number, col: number) => void;
    applyEdits: (edits: SourceEdit[]) => boolean;
  } | null>(null);

  onMount(() => {
    const cleanupGizmos = gizmoController.mount();
    return () => {
      splineController.exit();
      cleanupGizmos();
    };
  });

  let hierarchyPanel = $state<{ startRename: (id: string) => void } | null>(null);

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
        const newHeight = Math.min(
          window.innerHeight * 0.9,
          Math.max(100, window.innerHeight - BOTTOM_BAR_HEIGHT - e.clientY)
        );
        size = newHeight;
        localStorage.setItem('geoscript-repl-height', `${newHeight}`);
      }
      updateCanvasSize();
    };

    const handleMouseup = () => {
      window.removeEventListener('mousemove', handleMousemove);
      window.removeEventListener('mouseup', handleMouseup);
    };

    window.addEventListener('mousemove', handleMousemove);
    window.addEventListener('mouseup', handleMouseup);
  };

  let materialEditorOpen = $state(false);
  let uvMapsOverlayOpen = $state(false);
  let environmentSettingsOpen = $state(false);
  let previewPickerOpen = $state(false);

  /** `P` / view menu: with no target yet, picking one is the only sensible first step. */
  const togglePreview3d = () => {
    if (!textureMode.previewTarget) {
      previewPickerOpen = true;
      return;
    }
    textureMode.setPreview3d(!textureMode.preview3d);
    logGeotoyEvent('view', 'texture_preview_3d', { on: textureMode.preview3d });
  };

  const toggleMaterialEditorOpen = () => {
    materialEditorOpen = !materialEditorOpen;
    if (materialEditorOpen) logGeotoyEvent('materials', 'editor_open');
  };
  const toggleEnvironmentSettingsOpen = () => {
    environmentSettingsOpen = !environmentSettingsOpen;
    if (environmentSettingsOpen) logGeotoyEvent('environment', 'settings_open');
  };

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

  /**
   * Hash of every wasm input except per-node transforms; the fast path uses it
   * to decide whether a re-eval is needed. Material defs are serialized whole
   * because UV-mapping fields drive JS-side UV unwrap during the per-mesh build.
   */
  const computeEvalInputsHash = (): string => {
    // Covers *every* tab, not just the active one: once a material can reference another
    // tab's texture output, a change over there must move this hash too, or the fast path
    // would skip the re-eval and leave the dependency stale on screen. Hashing all tabs is
    // the conservative superset of the run set — at worst it costs an extra re-eval.
    const parts: string[] = [
      `t:${tabs.active.id}`,
      `vec:${vectorizeFlags.disabled ? 1 : 0}${vectorizeFlags.verify ? 1 : 0}${vectorizeFlags.profile ? 1 : 0}`,
    ];
    for (const tab of tabs.tabs) {
      const tree = tab.treeState.state.tree;
      parts.push(`g:${tab.id}:${tree.globalsSource}`);
      for (const k of Object.keys(tree.nodes).sort()) {
        const n = tree.nodes[k];
        // `children` matters: reparenting changes `compileTree`'s emitted imports.
        // `instances.length` (not the transforms) matters: add/remove changes the
        // rendered-object set, so it must force a full re-run while drags stay fast.
        // `handles`/`controls` matter: a gizmo or input-control value can change geometry, so
        // either must force a full re-eval rather than the transform-only fast path.
        parts.push(
          `n:${tab.id}:${k}:${n.name}:${n.disabled ? 1 : 0}:${n.instances.length}:${n.source}:${n.children.join(',')}:${JSON.stringify(n.handles ?? null)}:${JSON.stringify(n.controls ?? null)}`
        );
      }
    }
    for (const tab of tabs.tabs) parts.push(`pe:${tab.id}:${tab.preludeEjected ? 1 : 0}`);
    for (const tab of tabs.tabs) {
      for (const [name, p] of Object.entries(tab.textureParams).sort(([a], [b]) => (a < b ? -1 : 1))) {
        parts.push(`txp:${tab.id}:${name}:${p.minFilter ?? ''}|${p.magFilter ?? ''}|${p.format ?? ''}`);
      }
    }
    const matIds = Object.keys(persistence.materialDefinitions.materials).sort();
    for (const id of matIds) {
      parts.push(`m:${id}:${JSON.stringify(persistence.materialDefinitions.materials[id])}`);
    }
    parts.push(`dm:${persistence.materialDefinitions.defaultMaterialID ?? ''}`);
    // The 3D preview changes the run set (target tab pulled in) only while it shows.
    if (tabs.active.kind === 'texture' && textureMode.preview3d) {
      parts.push(`pv:${JSON.stringify(textureMode.previewTarget)}`);
    }
    return parts.join('\x00');
  };

  const _fastScratch = new THREE.Matrix4();

  /** Recompose each mesh's `ancestor × localInScript` if only transforms changed. */
  const tryTransformOnlyFastPath = (): boolean => {
    if (execution.isRunning) return false;
    if (execution.lastOkInputKey === null) return false;
    const drag = gizmoController.dragSession;
    if (!drag && computeEvalInputsHash() !== execution.lastOkInputKey) return false;

    const tree = treeState.state.tree;
    const worldMatrices = buildWorldMatrixCache(tree, drag?.parentMap ?? buildParentMap(tree));
    const worldByKey = new Map<string, THREE.Matrix4>();
    for (const [nodeId, list] of worldMatrices) {
      for (const e of list) worldByKey.set(`${nodeId}\x00${instancePathKey(e.path)}`, e.world);
    }
    for (const obj of meshScene.renderedObjects) {
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

  const controlNodeId = (c: RenderedControl): string | null => {
    const nodeId = c.sourceModule ? lastRun?.moduleNameToNodeId[c.sourceModule] : undefined;
    return nodeId && treeState.state.tree.nodes[nodeId] ? nodeId : null;
  };

  const clearControlOverride = (nodeId: string, handleId: string) => {
    const before = treeState.captureControl(nodeId, handleId);
    if (!before) return;
    treeState.deleteControl(nodeId, handleId);
    treeState.recordControlChange(nodeId, handleId, before, null);
  };

  const resetControl = (c: RenderedControl) => {
    const nodeId = controlNodeId(c);
    if (!nodeId) return;
    clearControlOverride(nodeId, c.handleId);
    runOrFast();
  };

  /** Bakes the stored value into the node source's `default=` (planned by the Rust parser),
   *  then drops the override so the source is the single truth again. */
  const bakeControlDefault = async (c: RenderedControl) => {
    const nodeId = controlNodeId(c);
    const stored = nodeId ? treeState.captureControl(nodeId, c.handleId) : null;
    if (!nodeId || !stored) return;
    const source = treeState.state.tree.nodes[nodeId].source;
    const wire = controlValueToWire(stored);
    const res = await getAnalysisClient().rewriteInputDefaults(source, [
      { handle_id: c.handleId, kind: c.kind, value: wire.value ?? [], str_value: wire.str_value ?? null },
    ]);
    if (res.errors.length > 0) {
      showToast({ status: 'error', message: `set as default: ${res.errors[0].message}`, durationMs: 6000 });
      return;
    }
    if (treeState.state.tree.nodes[nodeId]?.source !== source) {
      showToast({ status: 'warning', message: 'set as default: the source changed meanwhile; try again' });
      return;
    }
    // The open node goes through CodeMirror (keeps its undo history); others edit the tree
    // directly and the editor picks the text up when they're selected.
    const viaEditor = treeState.state.selectedId === nodeId && editorPane?.applyEdits(res.edits);
    if (!viaEditor) treeState.setSource(nodeId, applySourceEdits(source, res.edits));
    clearControlOverride(nodeId, c.handleId);
    logGeotoyEvent('editor', 'control_baked_default');
    runOrFast();
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
    /** Which tab the run was built for; a switch mid-flight invalidates the result. */
    tabId: string;
    /** Bumped whenever the tree set is replaced wholesale; a switch alone can't catch that,
     *  since a revert rebuilds the tabs under their original ids. */
    docEpoch: number;
    /** Every tab the run evaluated; their texture-output indexes sync from the result. */
    runTabIds: string[];
    /** Texture 3D preview target pulled into this run, if any. */
    preview: PreviewTargetResolution | null;
  }

  const execution = new GeoscriptExecution<ReplRunInput>({
    workerManager: untrack(() => workerManager),
    onRunStart: persistence.saveDraft,
    setLastRunWasSuccessful: persistence.setLastRunWasSuccessful,
    buildRunInput: () => {
      const defs = $state.snapshot(persistence.materialDefinitions.materials) as Record<string, MaterialDef>;
      pruneProceduralTextures(defs);
      // Seed texture fetches before the sync below: a run triggered in the same task as a
      // def swap (clear-local-changes) builds before the fetch effect flushes, and a build
      // that misses the placeholders silently drops its maps.
      meshScene.ensureReferencedTextures();
      // Hash-guarded no-op unless a def changed this tick — the run never sees entries
      // staler than the defs it compiles against.
      meshScene.materialRuntime.sync(defs);
      const matsByName: Record<string, { def: MaterialDef; mat: THREE.Material }> = {};
      for (const [id, def] of Object.entries(defs)) {
        matsByName[def.name] = { def, mat: meshScene.materialRuntime.entries[id]?.material ?? HiddenMat };
      }

      const tree = treeState.serialize();
      const tabId = tabs.active.id;
      const runDocEpoch = docEpoch;

      const tabById = new Map(tabs.tabs.map(t => [t.id, t]));
      const treeCache = new Map<string, TreeDef>([[tabId, tree]]);
      const treeFor = (id: string): TreeDef => {
        let t = treeCache.get(id);
        if (!t) {
          t = tabById.get(id)!.treeState.serialize();
          treeCache.set(id, t);
        }
        return t;
      };

      // Texture tab with the 3D preview up: the target mesh tab joins the run through a
      // synthesized `<tab>:_preview` module, and palette procedural refs join as render
      // deps (as in mesh mode) so the previewed object's materials see live maps.
      let preview: PreviewTargetResolution | null = null;
      if (tabs.active.kind === 'texture' && textureMode.preview3d && textureMode.previewTarget) {
        const resolved = resolvePreviewTarget(textureMode.previewTarget, {
          activeTabId: tabId,
          tabKinds: new Map(tabs.tabs.map(t => [t.id, t.kind])),
          treeFor,
        });
        textureMode.previewProblem = 'problem' in resolved ? resolved.problem : null;
        if ('ok' in resolved) preview = resolved.ok;
      }

      // Run set: active tab, plus texture tabs referenced by procedural material textures
      // (their rendered outputs feed the mesh scene → prepended root imports), plus the
      // preview target's tab, plus tabs referenced by qualified imports anywhere in the set
      // (transitively; the user's own import drives their evaluation, so no prepend).
      const runSet: string[] = [tabId];
      const renderDeps: string[] = [];
      if (tabs.active.kind === 'mesh' || preview) {
        for (const dep of proceduralRefTabIds(defs)) {
          if (tabById.has(dep) && dep !== tabId) {
            runSet.push(dep);
            renderDeps.push(dep);
          }
        }
      }
      if (preview && !runSet.includes(preview.target.tabId)) runSet.push(preview.target.tabId);
      for (let i = 0; i < runSet.length; i += 1) {
        for (const ref of referencedTabIds(treeFor(runSet[i]))) {
          if (tabById.has(ref) && !runSet.includes(ref)) runSet.push(ref);
        }
      }

      const compiled = compileTree(tree, tabId);
      const modules = compiled.modules;
      const moduleNameToNodeId = buildModuleNameToNodeId(tree, tabId);
      const gizmoValues = buildInjectedValues(tree, tabId);
      for (const depId of runSet.slice(1)) {
        const depTree = treeFor(depId);
        Object.assign(modules, compileTreeModules(depTree, depId));
        Object.assign(moduleNameToNodeId, buildModuleNameToNodeId(depTree, depId));
        Object.assign(gizmoValues, buildInjectedValues(depTree, depId));
      }
      let modulePreludes: Record<string, TreeKind> | undefined;
      if (preview) {
        const previewModule = qualifyModuleName(PREVIEW_MODULE_NAME, tabId);
        modules[previewModule] = buildPreviewModuleSource(preview);
        // An exported value rendered by the preview module takes its node's ancestor
        // transforms, as the node's own renders would.
        moduleNameToNodeId[previewModule] = preview.target.nodeId;
        // Dependency roots never get the entry prelude; the source tab's default rig is
        // re-evaluated here unless that tab ejected it (then it renders its own lights).
        if (!tabById.get(preview.target.tabId)!.preludeEjected) modulePreludes = { [previewModule]: 'mesh' };
      }
      const sideEffectImports = [
        ...renderDeps.map(id => qualifyModuleName(ROOT_NODE_NAME, id)),
        ...(preview ? [qualifyModuleName(PREVIEW_MODULE_NAME, tabId)] : []),
      ];
      const code = sideEffectImports.map(m => `import { } from "${m}"\n`).join('') + compiled.rootSource;

      const textureParams: TextureParamsEntry[] = [];
      for (const id of runSet) {
        const t = tabById.get(id)!;
        for (const [name, p] of Object.entries(t.textureParams)) {
          textureParams.push({ tabId: id, name, ...p });
        }
      }

      // Active tab last: its ambient construction ends the RNG stream the entry continues.
      const tabAmbients = [...runSet.slice(1), tabId].map(id => {
        const t = tabById.get(id)!;
        return {
          tabId: id,
          preludeKind: t.preludeEjected ? ('' as const) : t.kind,
          globalsSource: treeFor(id).globalsSource,
        };
      });

      return {
        code,
        modules,
        modulePreludes,
        tabAmbients,
        preludeKind: tabs.active.preludeEjected ? undefined : tabs.active.kind,
        materials: matsByName,
        materialOverride: meshScene.materialOverride,
        renderMode: userData?.renderMode ?? false,
        gizmoValues,
        textureParams,
        moduleNameToNodeId,
        rootModuleName: qualifyModuleName(ROOT_NODE_NAME, tabId),
        vectorize: vectorizeFlags,
        tree,
        tabId,
        docEpoch: runDocEpoch,
        runTabIds: runSet,
        preview,
        inputKey: computeEvalInputsHash(),
      };
    },
    consume: (result, { tree, moduleNameToNodeId, tabId, docEpoch: runDocEpoch, runTabIds, preview }) => {
      // A run started before a tab switch or a revert settles against state it wasn't built
      // for; dropping it keeps its geometry out of the new scene, and — the reason this
      // matters beyond a repaint — keeps its stale node list out of the handle/control GC
      // below, which would prune live values off the tree that replaced it.
      if (tabId !== tabs.active.id || runDocEpoch !== docEpoch) {
        disposeRunObjects(result);
        return false;
      }
      // Every run's texture outputs land in any material-referenced placeholder textures —
      // including texture-mode runs, which is what makes editing a texture tab live-update
      // meshes that consume it.
      const texOutputs = result.objects.filter((o): o is GeneratedTexture => o.type === 'texture');
      uploadProceduralTextures(texOutputs);
      // Sync each evaluated texture tab's output index — including to empty, so a removed
      // `render_texture` call drops out of the material editor's picker.
      const outputsByTab = textureOutputsByTab(texOutputs);
      for (const runTabId of runTabIds) {
        if (tabs.tabs.find(t => t.id === runTabId)?.kind === 'texture') {
          tabs.setTextureOutputs(runTabId, outputsByTab.get(runTabId) ?? []);
        }
      }
      lastRun = {
        tree,
        gizmos: result.gizmos,
        controls: result.controls,
        moduleNameToNodeId,
        vectorizeReports: result.vectorizeReports,
      };
      mode.consume(result, tree, moduleNameToNodeId, preview);
    },
    onCancelCleanup: () => {
      mode.clearScene();
      clearTimeout(controlRunTimer);
    },
  });

  let statsHeight = $state(0);
  // The FPS widget's top-left slot belongs to the texture HUD in texture mode (design §9);
  // elsewhere it heads the overlay column, so the controls below it stack off its measured height.
  $effect(() => {
    // `setStatsEnabled` appends a brand-new `#viz-stats` (dropping the old one) every time the
    // graphics menu's FPS toggle flips, so the element must be re-resolved on mutation rather
    // than captured once — a stale miss leaves the controls stacked on top of a live FPS meter.
    const container = viz.renderer.domElement.parentElement;
    if (!container) {
      return;
    }
    const hidden = mode.kind === 'texture';
    let tracked: HTMLElement | null = null;
    const ro = new ResizeObserver(() => {
      statsHeight = tracked?.offsetHeight ?? 0;
    });
    const release = () => {
      if (tracked) {
        ro.unobserve(tracked);
        tracked.style.display = '';
        tracked = null;
      }
      statsHeight = 0;
    };
    const sync = () => {
      const stats = document.getElementById('viz-stats');
      if (stats === tracked) {
        return;
      }
      release();
      tracked = stats;
      if (!stats) {
        return;
      }
      if (hidden) {
        stats.style.display = 'none';
        return;
      }
      ro.observe(stats);
      statsHeight = stats.offsetHeight;
    };
    sync();
    const mo = new MutationObserver(sync);
    mo.observe(container, { childList: true });
    return () => {
      mo.disconnect();
      ro.disconnect();
      release();
    };
  });
  $effect(() => setTopLeftSlot(TopLeftSlot.stats, statsHeight));

  // Installed synchronously during init so its render override lands before the first
  // frame, after the pipeline's own (constructed pre-mount; the resize-cb order and
  // override-replacement order both depend on pipeline-first).
  if (untrack(() => userData)?.renderMode) {
    startRenderHarness({
      viz: untrack(() => viz),
      pipelineController: untrack(() => pipelineController)!,
      userData: untrack(() => userData)!,
      execution,
      meshScene,
      getTree: () => treeState.state.tree,
      getTabs: () => tabs.tabs.map(t => ({ id: t.id, kind: t.kind, name: t.name })),
    });
  }

  let docEpoch = 0;

  /**
   * Switching re-runs the newly-active tab's dependency closure — tabs are module groups in
   * one program, not isolated scenes, so there is no persistent per-tab scene to swap to.
   */
  const switchTab = (id: string) => {
    if (id === persistence.activeTreeId || !tabs.tabs.some(t => t.id === id)) return;
    // Before the id flips, so `collectTabMeta` files the live camera under the tab it
    // actually belongs to.
    persistence.saveDraft();

    mode.clearScene();
    lastRun = null;
    // Bound to the active tab, so it can't follow a switch — and a texture tab would drop
    // any edit made through it.
    environmentSettingsOpen = false;
    persistence.activeTreeId = id;

    mode.restoreViewState(tabs.active.view);
    execution.run();
    logGeotoyEvent('editor', 'tab_switch');
  };

  const createTab = (kind: TreeKind) => {
    const id = tabs.create(kind);
    switchTab(id);
    logGeotoyEvent('editor', 'tab_create', { kind });
  };

  const deleteTab = (id: string) => {
    const tab = tabs.tabs.find(t => t.id === id);
    if (!tab || !tabs.canDelete(id)) return;
    // Tab lifecycle is outside the undo system, so deletion is irreversible.
    const nodeCount = Object.keys(tab.treeState.state.tree.nodes).length;
    const isEmpty =
      nodeCount <= 1 && !tab.treeState.state.tree.nodes[tab.treeState.state.tree.rootId]?.source;
    if (!isEmpty && !confirm(`Delete tab "${tab.name}"? This can't be undone.`)) return;

    // Only the *active* tab's removal forces a switch; `remove` returns the deleted tab's
    // neighbour either way, so comparing against it would yank the user off an unrelated tab.
    const wasActive = id === persistence.activeTreeId;
    // Tear down while the tab still exists: once it's removed, `activeTreeId` names nothing
    // and `tabs.active` falls back to `tabs[0]`, so `mode` can resolve to a different kind
    // whose `clearScene()` leaves this tab's output on screen.
    if (wasActive) {
      mode.clearScene();
      lastRun = null;
    }
    const next = tabs.remove(id);
    if (wasActive) switchTab(next);
    persistence.saveDraft();
    logGeotoyEvent('editor', 'tab_delete');
  };

  const compTitle = $derived(userData?.initialComposition?.comp.title || 'untitled');
  /** Relocates into the run bar as a single `☰` when the panel is collapsed or too narrow —
   *  otherwise collapsing strands the only affordance that can un-collapse it. */
  const menubarInBar = $derived(
    isEditorCollapsed || (layout.orientation === 'horizontal' && layout.panelSize < 360)
  );
  /** Mobile views one composition; the strip has no room and no job. */
  const showTabStrip = $derived(innerWidth >= 768);

  const runErr = $derived(execution.err ?? meshScene.materialRuntime.err);
  const vectorizeRows = $derived.by((): VectorizeRow[] => {
    if (!vectorizePrefs.report || !lastRun) return [];
    const { tree, moduleNameToNodeId } = lastRun;
    return lastRun.vectorizeReports.map(r => {
      const nodeId = r.module ? (moduleNameToNodeId[r.module] ?? null) : null;
      const node = nodeId ? tree.nodes[nodeId] : undefined;
      return {
        vectorized: r.vectorized,
        aborted: !r.vectorized && (r.reason?.startsWith('aborted') ?? false),
        where: node?.name ?? r.module ?? '?',
        nodeId: node ? nodeId : null,
        line: node ? r.line - moduleSourceLineOffset(node, tree) : r.line,
        col: r.col,
        reason: r.reason,
        plan: r.plan,
      };
    });
  });
  const vectorizeSummary = $derived.by((): VectorizeSummary | null =>
    vectorizePrefs.report
      ? {
          rows: vectorizeRows,
          disabled: vectorizeFlags.disabled,
          rerunUncached: () => void execution.runUncached(),
          reveal: async row => {
            if (!row.nodeId) return;
            treeState.setSelected(row.nodeId);
            // The editor swaps its doc in an effect; place the cursor once that has run.
            await tick();
            editorPane?.revealLoc(row.line, row.col);
          },
        }
      : null
  );
  const editorVectorizeMarkers = $derived.by((): VectorizeMarker[] => {
    const sel = treeState.state.selectedId;
    return vectorizeRows
      .filter(r => !r.vectorized && r.nodeId === sel && r.reason !== null)
      .map(r => ({ line: r.line, col: r.col, reason: r.reason! }));
  });
  const runMetrics = $derived.by((): StatusMetric[] | null => {
    if (!execution.runStats) return null;
    const metrics = mode.statusMetrics(execution.runStats);
    const scalar = vectorizeRows.filter(r => !r.vectorized).length;
    if (scalar === 0) return metrics;
    return [
      ...metrics,
      {
        label: 'Scalar texel bodies',
        value: String(scalar),
        short: `${scalar} scalar ${scalar === 1 ? 'body' : 'bodies'}`,
      },
    ];
  });
  /** An error force-opens the output without overwriting the user's preference; the
   *  disclosure can still collapse it, so the control is never dead. */
  const runOutputVisible = $derived(runOutputExpanded || runOutputForced);
  let lastErrSeen: string | null = null;
  $effect(() => {
    const err = runErr;
    // Only on a *change*, so collapsing a forced-open panel isn't undone while the same
    // error stands; clearing the error releases the force back to the stored preference.
    if (err !== lastErrSeen) runOutputForced = !!err;
    lastErrSeen = err;
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
    if (newId) gizmoController.armInstance(nodeId, newId);
  };

  const handleRemoveInstance = (nodeId: string, instanceId: string) => {
    treeState.removeInstance(nodeId, instanceId);
    runOrFast();
  };

  const handleInspectorDisableToggle = (id: string, disabled: boolean) => {
    treeState.setDisabled(id, disabled);
  };

  const rerun = async () => {
    await execution.run();
  };

  const toggleEditorCollapsed = () => {
    persistence.saveDraft();
    isEditorCollapsed = !isEditorCollapsed;
    updateCanvasSize();
  };

  const togglePreludeEjected = async () => {
    const tab = tabs.active;
    await togglePrelude({
      treeState,
      getPrelude: execution.getPrelude,
      kind: tab.kind,
      ejected: tab.preludeEjected,
      setEjected: ejected => tabs.setPreludeEjected(tab.id, ejected),
    });
    logGeotoyEvent('editor', 'prelude_toggle', { ejected: !tab.preludeEjected });
    execution.run();
  };

  let exportDialog = $state<HTMLDialogElement | null>(null);
  const onExport = () => {
    exportDialog?.showModal();
  };

  const handleToggleProjection = () => {
    let projection: string;
    if (mode.kind === 'texture') {
      if (!textureMode.preview3d) return;
      textureMode.previewScene.toggleProjection();
      projection = textureMode.previewScene.cameraProjection;
    } else {
      meshScene.toggleProjection();
      projection = meshScene.cameraProjection;
    }
    logGeotoyEvent('view', 'projection_toggle', { projection });
    persistence.viewDirty = true;
    persistence.saveDraft();
  };

  const clearLocalChanges = () => {
    if (persistence.isDirty && !confirm('Really clear local changes?')) {
      return;
    }

    logGeotoyEvent('editor', 'clear_local_changes');
    // Discards the whole local changeset, not just the active tab's: the tab set itself is
    // rebuilt from the saved snapshot, so locally added/removed tabs are undone too.
    // Teardown precedes the revert — `revertToServer` reassigns `activeTreeId`, after which
    // `mode` can resolve to a different kind whose `clearScene()` is a no-op.
    mode.clearScene();
    lastRun = null;
    docEpoch += 1;
    const serverState = persistence.revertToServer();
    tabs.resetFromDoc(serverState.doc, serverState.tabMeta);
    // `revertToServer` re-baselines before the tab set is rebuilt, so the shape baseline it
    // captured is the pre-revert one; re-baseline now that the tabs match the snapshot.
    persistence.markClean();

    mode.restoreViewState(tabs.active.view);

    execution.run();
  };

  const wrappedToggleAxesHelpers = () => toggleAxisHelpers(viz);

  onMount(() => {
    setTimeout(() => mode.restoreViewState(tabs.active.view));

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

    const actions: GeotoyKeymapActions = {
      centerView: () => {
        mode.focus(resolveSelectedNode()?.sel ?? null);
      },
      toggleWireframe: meshScene.toggleWireframe,
      toggleWireframeXray: meshScene.toggleWireframeXray,
      toggleNormalMat: meshScene.toggleNormalMat,
      toggleLightHelpers: meshScene.toggleLightHelpers,
      toggleAxesHelper: wrappedToggleAxesHelpers,
      run: runManual,
      toggleEditorCollapsed,
      snapView: axis => snapView(viz, axis),
      orbit: (axis, angle) => orbit(viz, axis, angle),
      toggleProjection: handleToggleProjection,
      toggleRecording,
      togglePreview3d,
      toggleTextureGrid: textureMode.toggleLayout,
      preview3dActive: () => textureMode.preview3d,
      setGizmoMode: mode => {
        if (!resolveSelectedNode()) return;
        gizmoController.setMode(mode);
      },
      toggleGizmoSpace: () => {
        if (!resolveSelectedNode()) return;
        gizmoController.toggleSpace();
      },
      toggleSelectionSolo: () => {
        const ns = resolveSelectedNode();
        if (!ns || ns.sel === ns.rootId) return;
        treeState.setSolo(treeState.state.soloId === ns.sel ? null : ns.sel);
      },
      escapeSelection: e => {
        if (gizmoController.isDragging()) return;
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
        if (gizmoController.isDragging()) return; // never delete a node mid gizmo-drag
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
        if (gizmoController.isDragging()) return;
        runUndo();
        e?.preventDefault();
      },
      treeRedo: e => {
        if (gizmoController.isDragging()) return;
        runRedo();
        e?.preventDefault();
      },
    };
    keymapActions = actions;
    keymap.install();

    window.addEventListener('beforeunload', persistence.saveDraft);

    return () => {
      bootAbort.abort();
      clearTimeout(controlRunTimer);
      workerManager.release();
      execution.dispose();
      keymap.dispose();

      mode.clearScene();

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

  const viewMenuActions = $derived({
    toggleAxisHelpers: wrappedToggleAxesHelpers,
    toggleProjection: handleToggleProjection,
    toggleGizmoGhosts: gizmoController.toggleGhosts,
    showGizmoGhosts: gizmoController.showGhosts,
    gizmosExist: hasAnyGizmos,
    togglePreview3d,
  });

  const sceneMenuActions = $derived({
    openMaterialEditor: toggleMaterialEditorOpen,
    openEnvironment: toggleEnvironmentSettingsOpen,
    exportScene: onExport,
    toggleRecording,
    recordingState: $recordingState,
    openPreviewPicker: () => (previewPickerOpen = true),
  });

  const menus: Menu[] = $derived([
    {
      title: 'view',
      sections: [
        ...mode.viewSections(viewMenuActions),
        {
          header: 'panels',
          items: [
            {
              label: isEditorCollapsed ? 'show editor' : 'hide editor',
              shortcut: '^E',
              action: toggleEditorCollapsed,
            },
            { label: 'ui layout', state: layoutOrientation, action: toggleLayoutOrientation },
          ],
        },
        {
          header: 'vectorization',
          items: [
            {
              label: 'report',
              state: vectorizePrefs.report ? 'on' : 'off',
              action: () => {
                vectorizePrefs.report = !vectorizePrefs.report;
                if (vectorizePrefs.report) {
                  runOutputExpanded = true;
                  // Plans + timings only exist for bodies that actually execute under
                  // profiling — a cached replay would show nothing.
                  void execution.runUncached();
                }
              },
            },
            ...(vectorizeDevTools
              ? [
                  {
                    label: 'vectorize',
                    state: vectorizePrefs.disabled ? 'off' : 'on',
                    action: () => (vectorizePrefs.disabled = !vectorizePrefs.disabled),
                  },
                  {
                    label: 'verify',
                    state: vectorizePrefs.verify ? 'on' : 'off',
                    action: () => (vectorizePrefs.verify = !vectorizePrefs.verify),
                  },
                ]
              : []),
          ],
        },
      ],
    },
    { title: 'scene', sections: mode.sceneMenu(sceneMenuActions) },
    {
      title: 'comp',
      sections: [
        {
          items: [
            { label: 'clear local changes', action: clearLocalChanges },
            {
              label: 'eject prelude',
              state: tabs.active.preludeEjected ? 'ejected' : '',
              // Only mesh trees have a prelude to eject.
              disabled: tabs.active.kind !== 'mesh',
              action: togglePreludeEjected,
            },
          ],
        },
        { items: [{ label: 'back to geotoy', action: goHome }] },
      ],
    },
    {
      title: 'help',
      sections: [
        {
          items: [
            { label: 'docs', action: () => void window.open(resolve('/geotoy/docs'), '_blank') },
            {
              label: 'report bug',
              action: () => void window.open('https://github.com/Ameobea/sketches-3d/issues/new', '_blank'),
            },
            { label: 'credits', action: () => void window.open(resolve('/geotoy/credits'), '_blank') },
          ],
        },
      ],
    },
  ]);
</script>

<svelte:window bind:innerWidth bind:innerHeight />

{#snippet saveForm()}
  <SaveControls
    comp={userData?.initialComposition?.comp}
    getCurrentDoc={persistence.currentDoc}
    activeTreeId={persistence.activeTreeId}
    materials={persistence.materialDefinitions}
    {collectTabMeta}
    onSave={() => {
      persistence.markClean();
      tabs.markAllSaved();
    }}
    {userData}
  />
{/snippet}

{#if mode.kind === 'texture' && !userData?.renderMode}
  {#if textureMode.preview3d}
    <Preview3dHud
      mode={textureMode}
      onPick={() => (previewPickerOpen = true)}
      onShow2d={() => textureMode.setPreview3d(false)}
    />
  {:else if textureMode.textures.length > 0}
    <TexturePreview
      mode={textureMode}
      onSetTextureParams={(sourceModule, output, patch) => {
        const sep = sourceModule.indexOf(':');
        if (sep <= 0) return;
        tabs.setTextureParams(sourceModule.slice(0, sep), output, patch);
        void execution.run();
      }}
      width={Math.max(innerWidth - (layout.orientation === 'horizontal' ? layout.panelSize : 0), 0)}
      height={Math.max(
        innerHeight - layout.barHeight - (layout.orientation === 'vertical' ? layout.panelSize : 0),
        0
      )}
    />
  {:else}
    <TexturePlaceholder hasRun={textureMode.hasRun} />
  {/if}
{/if}

{#if hasAnyControls && !userData?.renderMode}
  <ControlsPanel
    controls={(lastRun?.controls ?? []).filter(
      // Dependency-tab controls can't route edits to their owning tree yet — showing them
      // would silently no-op (and falsely dirty the doc). Cross-tab editing is phase-4 work.
      c => !c.sourceModule || c.sourceModule.split(':')[0] === tabs.active.id
    )}
    {treeState}
    moduleNameToNodeId={lastRun?.moduleNameToNodeId ?? {}}
    onEdit={scheduleControlRun}
    onBakeDefault={bakeControlDefault}
    onResetControl={resetControl}
    spline={splineController.panelCtx}
    onViewUvMaps={() => (uvMapsOverlayOpen = true)}
  />
{/if}

{#if uvMapsOverlayOpen && execution.ctxPtr !== null}
  <UvMapsOverlay
    repl={execution.repl}
    ctxPtr={execution.ctxPtr}
    onclose={() => (uvMapsOverlayOpen = false)}
  />
{/if}

<ExportModal bind:dialog={exportDialog} renderedObjects={meshScene.renderedObjects} />
<MaterialEditor
  bind:isOpen={materialEditorOpen}
  bind:materials={persistence.materialDefinitions}
  {rerun}
  me={userData?.me}
  proceduralTextureOptions={proceduralOutputOptions(tabs.tabs)}
/>

<EnvironmentSettings
  bind:isOpen={environmentSettingsOpen}
  bind:environment={() => tabs.active.environment, env => tabs.setEnvironment(tabs.active.id, env)}
  bind:emissiveBloom={() => tabs.active.emissiveBloom, s => tabs.setEmissiveBloom(tabs.active.id, s)}
  me={userData?.me}
/>

<PreviewObjectPicker
  bind:isOpen={previewPickerOpen}
  tabs={tabs.tabs}
  activeTabId={tabs.active.id}
  current={textureMode.previewTarget}
  onPick={target => {
    textureMode.setPreviewTarget(target);
    logGeotoyEvent('view', 'texture_preview_pick', { kind: target.exportName ? 'export' : 'node' });
  }}
/>

{#snippet runOutputPane()}
  <RunOutput err={runErr} metrics={runMetrics} vectorize={vectorizeSummary} />
{/snippet}

{#if !isEditorCollapsed}
  <div
    class={['root', layoutOrientation === 'horizontal' ? 'horizontal' : '']}
    style={`${userData?.renderMode ? 'visibility: hidden; height: 0; width: 0;' : ''} bottom: ${BOTTOM_BAR_HEIGHT}px; ${layoutOrientation === 'horizontal' ? `width: ${layout.panelSize}px;` : `height: ${layout.panelSize}px;`}`}
  >
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class={['dragger', layoutOrientation === 'horizontal' ? 'horizontal' : '']}
      role="separator"
      aria-orientation={layoutOrientation === 'horizontal' ? 'vertical' : 'horizontal'}
      onmousedown={handleMousedown}
    ></div>
    {#if !menubarInBar}
      <Menubar {menus} title={compTitle} barHeight={BOTTOM_BAR_HEIGHT} />
    {/if}
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
                {gizmoController.mode[0]}·{gizmoController.space === 'world' ? 'W' : 'L'}
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
            meshCounts={meshScene.meshCounts}
            armedRef={gizmoController.armedRef}
            onselect={id => treeState.setSelected(id)}
            onInstanceTransformChange={handleInstanceTransformChange}
            onArmInstance={gizmoController.armInstance}
            onAddInstance={handleAddInstance}
            onRemoveInstance={handleRemoveInstance}
            onDisableToggle={handleInspectorDisableToggle}
          />
        {/if}
        <EditorPane
          bind:this={editorPane}
          {treeState}
          {persistence}
          analysisPrelude={tabs.active.kind === 'mesh' && !tabs.active.preludeEjected}
          gizmoEditorHooks={mode.editorHooks}
          onRun={runManual}
          onCenterView={() => mode.focus(null)}
          armedHandleId={gizmoController.armedRef?.kind === 'handle' ? gizmoController.armedRef.name : null}
          readouts={gizmoController.readouts}
          vectorizeMarkers={editorVectorizeMarkers}
        />
      </div>
    </div>
    {#if runOutputVisible && !userData?.renderMode}
      <div class="run-output-inline">{@render runOutputPane()}</div>
    {/if}
  </div>
{/if}

{#if !userData?.renderMode}
  {#if savePopoverOpen}
    <div class="save-popover-anchor" style={`bottom: ${BOTTOM_BAR_HEIGHT}px;`}>
      <SavePopover
        comp={userData?.initialComposition?.comp ?? null}
        isOwner={!!userData?.me &&
          (!userData.initialComposition || userData.me.id === userData.initialComposition.comp.author_id)}
        loggedIn={!!userData?.me}
        onClose={() => (savePopoverOpen = false)}
        onForked={handleForkedComposition}
        form={saveForm}
      />
    </div>
  {/if}
  {#if runOutputVisible && isEditorCollapsed}
    <div
      class="run-output-anchor"
      style={`bottom: ${BOTTOM_BAR_HEIGHT}px; width: ${COLLAPSED_RUN_OUTPUT_WIDTH}px;`}
    >
      {@render runOutputPane()}
    </div>
  {/if}
  <div class="bottom-bar" style={`height: ${BOTTOM_BAR_HEIGHT}px;`}>
    {#if showTabStrip}
      <TabStrip
        tabs={tabs.tabs}
        activeId={persistence.activeTreeId}
        barHeight={BOTTOM_BAR_HEIGHT}
        canDelete={id => tabs.canDelete(id)}
        onSelect={switchTab}
        onCreate={createTab}
        onRename={(id, name) => {
          tabs.rename(id, name);
          persistence.saveDraft();
        }}
        onDelete={deleteTab}
      />
    {:else}
      <div class="strip-filler"></div>
    {/if}
    <div
      class={['run-bar', layout.runBarWidth === null ? 'flexible' : '']}
      style={layout.runBarWidth === null ? '' : `width: ${layout.runBarWidth}px;`}
    >
      {#if menubarInBar}
        <Menubar {menus} title={compTitle} barHeight={BOTTOM_BAR_HEIGHT} compact />
      {/if}
      <RunBar
        isRunning={execution.isRunning}
        err={runErr}
        metrics={runMetrics}
        isDirty={persistence.isDirty}
        expanded={runOutputVisible}
        recordingState={$recordingState}
        run={runManual}
        cancel={execution.cancel}
        onToggleExpanded={toggleRunOutput}
        onToggleSave={() => (savePopoverOpen = !savePopoverOpen)}
        saveOpen={savePopoverOpen}
        compactMetrics={!showTabStrip}
      />
    </div>
  </div>
{/if}

<style lang="css">
  .root {
    width: 100%;
    position: absolute;
    max-width: 100vw;
    overflow-x: hidden;
    display: flex;
    flex-direction: column;
    color: #efefef;
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
    font-size: 15px;
  }

  .root.horizontal {
    width: auto;
    height: auto;
    max-width: none;
    overflow-x: auto;
    overflow-y: hidden;
    right: 0;
    left: auto;
    top: 0;
    flex-direction: column;
  }

  /* Fixed-height bar: its content box is `height - border-top`, so clip rather than let a
   * child spill and add a page scrollbar. */
  .bottom-bar {
    box-sizing: border-box;
    overflow: hidden;
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    display: flex;
    align-items: stretch;
    background: #0d0d0d;
    border-top: 1px solid #444;
    color: #efefef;
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
    z-index: 3;
  }

  .run-bar {
    /* Its width is set to the panel's, so padding + border must stay inside it or the
       bar's left edge drifts off the panel's. */
    box-sizing: border-box;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 8px;
    flex-shrink: 0;
    min-width: 0;
    border-left: 1px solid #444;
  }

  /* Without an explicit width the bar is content-sized, so nothing can give when the
   * viewport is narrow and the tail (dirty + save) gets pushed off the edge. Let it take
   * the remaining space instead; the metrics inside are what shrink. */
  .run-bar.flexible {
    flex: 1 1 auto;
  }

  .save-popover-anchor {
    position: absolute;
    right: 0;
    z-index: 5;
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
  }

  .strip-filler {
    flex: 1;
    min-width: 0;
  }

  .run-output-anchor {
    position: absolute;
    right: 0;
    max-width: 100vw;
    z-index: 4;
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
  }

  /* In the panel's flow rather than over it, so expanding shortens the editor instead of
   * hiding it. Capped so an open plan listing can't squeeze the editor out entirely. */
  .run-output-inline {
    display: flex;
    flex-direction: column;
    min-height: 0;
    max-height: 60%;
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

  @media (max-width: 768px) {
    .editor-container {
      flex-direction: column;
    }
  }
</style>
