<script lang="ts">
  import * as THREE from 'three';
  import { onMount, untrack } from 'svelte';
  import { resolve } from '$app/paths';

  import type { Viz } from 'src/viz';
  import type { WorkerManager } from 'src/geoscript/workerManager';
  import type { GeoscriptPlaygroundUserData } from 'src/viz/scenes/geoscriptPlayground/geoscriptPlayground.svelte';
  import SaveControls from 'src/geotoy/panels/SaveControls.svelte';
  import { goto } from '$app/navigation';
  import { startRenderHarness } from 'src/geotoy/renderHarness';
  import ReplOutput from 'src/geotoy/panels/ReplOutput.svelte';
  import ReplControls from 'src/geotoy/panels/ReplControls.svelte';
  import EditorPane from 'src/geotoy/panels/EditorPane.svelte';
  import ExportModal from 'src/geotoy/panels/ExportModal.svelte';
  import { GeoscriptExecution, type RunInput } from 'src/geotoy/modules/execution.svelte';
  import { HiddenMat, type MaterialDef } from 'src/geoscript/materials';
  import MaterialEditor from 'src/viz/scenes/geoscriptPlayground/materialEditor/MaterialEditor.svelte';
  import EnvironmentSettings from 'src/viz/scenes/geoscriptPlayground/EnvironmentSettings.svelte';
  import {
    cloneTransform3,
    type Composition,
    type CompositionVersion,
    type Transform3,
    type TreeDef,
  } from 'src/geoscript/geotoyAPIClient';
  import { GeotoyPersistence } from 'src/geotoy/modules/persistence.svelte';
  import { GeotoyKeymap } from 'src/geotoy/modules/keymap';
  import { buildGeotoyKeymap, type GeotoyKeymapActions } from 'src/viz/scenes/geoscriptPlayground/keymap';
  import { compileTree, buildInjectedValues, buildModuleNameToNodeId } from 'src/geoscript/treeCodegen';
  import ControlsPanel from 'src/geotoy/panels/ControlsPanel.svelte';
  import { TreeState, GLOBALS_SELECTION_ID } from 'src/viz/scenes/geoscriptPlayground/treeState.svelte';
  import { buildParentMap, findParentId } from 'src/viz/scenes/geoscriptPlayground/treeOps';
  import HierarchyPanel from 'src/geotoy/panels/HierarchyPanel.svelte';
  import NodeInspector from 'src/geotoy/panels/NodeInspector.svelte';
  import { getIsUVUnwrapLoaded } from 'src/viz/wasm/uv_unwrap/uvUnwrap';
  import ReadOnlyCompositionDetails from 'src/geotoy/panels/ReadOnlyCompositionDetails.svelte';
  import { buildWorldMatrixCache, instancePathKey } from 'src/geoscript/runner/geoscriptRunner';
  import type { RenderedGizmo, RenderedControl } from 'src/geoscript/runner/types';
  import { GizmoController } from 'src/geotoy/modes/mesh/gizmoController.svelte';
  import { SplineController } from 'src/geotoy/modes/mesh/splineController.svelte';
  import { MeshScene } from 'src/geotoy/modes/mesh/meshScene.svelte';
  import { snapView, orbit, untilOrbitControls } from 'src/viz/scenes/geoscriptPlayground/cameraControls';
  import { toggleAxisHelpers } from 'src/viz/scenes/geoscriptPlayground/gizmos';
  import { useRecording } from 'src/viz/scenes/geoscriptPlayground/recording';
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
    updateCanvasSize();
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
      updateCanvasSize();
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
  // Canvas fills the viewport minus the editor inset. Registered after the pipeline's
  // own resize cb (pipeline is constructed before the app mounts), so its setSize wins.
  const updateCanvasSize = () => {
    if (userData?.renderMode) {
      return;
    }

    let canvasWidth: number;
    let canvasHeight: number;
    if (layoutOrientation === 'horizontal') {
      const newWidth = isEditorCollapsed ? 36 : size;
      canvasWidth = Math.max(window.innerWidth - newWidth, 0);
      canvasHeight = window.innerHeight;
    } else {
      const newHeight = isEditorCollapsed ? 36 : size;
      canvasWidth = window.innerWidth;
      canvasHeight = Math.max(window.innerHeight - newHeight, 0);
    }

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
    treeState,
    getGizmo: () => gizmoController.gizmo,
    getModuleNameToNodeId: () => lastRun?.moduleNameToNodeId,
    nodeWorldMatrix: id => gizmoController.nodeWorldMatrix(id),
    setGizmoTranslateMode: () => gizmoController.setMode('translate'),
    armNone: () => gizmoController.armNone(),
    runOrFast: () => runOrFast(),
  });
  const gizmoController = new GizmoController({
    viz: untrack(() => viz),
    treeState,
    renderMode: () => userData?.renderMode ?? false,
    bootSignal: bootAbort.signal,
    getLastGizmos: () => lastRun?.gizmos,
    getRenderedObjects: () => meshScene.renderedObjects,
    runOrFast: () => runOrFast(),
    blurEditor: () => editorPane?.blur(),
    isSplineActive: () => splineController.activeKey !== null,
    interceptSplineClick: rc => splineController.interceptClick(rc),
  });
  const meshScene = new MeshScene({
    viz: untrack(() => viz),
    treeState,
    persistence,
    pipelineController: untrack(() => pipelineController),
    bootSignal: bootAbort.signal,
    getLastRunTree: () => lastRun?.tree ?? null,
    onRunConsumed: splineController.onRunConsumed,
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
    togglePreludeEjected: () => Promise<void>;
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
        const newHeight = Math.min(window.innerHeight * 0.9, Math.max(100, window.innerHeight - e.clientY));
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
  let environmentSettingsOpen = $state(false);

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
      const compiled = compileTree(tree);
      return {
        code: compiled.rootSource,
        modules: compiled.modules,
        extraAmbientSources: tree.globalsSource.trim().length > 0 ? [tree.globalsSource] : [],
        includePrelude: !persistence.preludeEjected,
        materials: matsByName,
        materialOverride: meshScene.materialOverride,
        renderMode: userData?.renderMode ?? false,
        gizmoValues: buildInjectedValues(tree),
        moduleNameToNodeId: buildModuleNameToNodeId(tree),
        tree,
        inputKey: computeEvalInputsHash(),
      };
    },
    consume: (result, { tree, moduleNameToNodeId }) => {
      lastRun = { tree, gizmos: result.gizmos, controls: result.controls, moduleNameToNodeId };
      meshScene.consume(result, tree, moduleNameToNodeId);
    },
    onCancelCleanup: () => {
      meshScene.clearScene();
      clearTimeout(controlRunTimer);
    },
  });

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
    });
  }

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

  const rerun = async (onlyIfUVUnwrapperNotLoaded: boolean) => {
    if (onlyIfUVUnwrapperNotLoaded && getIsUVUnwrapLoaded()) {
      return;
    }
    await execution.run();
  };

  const toggleEditorCollapsed = () => {
    persistence.saveDraft();
    isEditorCollapsed = !isEditorCollapsed;
    updateCanvasSize();
  };

  const togglePreludeEjected = () => void editorPane?.togglePreludeEjected();

  let exportDialog = $state<HTMLDialogElement | null>(null);
  const onExport = () => {
    exportDialog?.showModal();
  };

  const handleToggleProjection = () => {
    meshScene.toggleProjection();
    logGeotoyEvent('view', 'projection_toggle', { projection: meshScene.cameraProjection });
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

    if (serverState.view) {
      meshScene.setView(serverState.view);
    }

    execution.run();
  };

  const wrappedToggleAxesHelpers = () => toggleAxisHelpers(viz);

  onMount(() => {
    setTimeout(() => meshScene.setView(persistence.initial.view));

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
        meshScene.focus(resolveSelectedNode()?.sel ?? null);
      },
      toggleWireframe: meshScene.toggleWireframe,
      toggleWireframeXray: meshScene.toggleWireframeXray,
      toggleNormalMat: meshScene.toggleNormalMat,
      toggleLightHelpers: meshScene.toggleLightHelpers,
      toggleAxesHelper: wrappedToggleAxesHelpers,
      run: runManual,
      snapView: axis => snapView(viz, axis),
      orbit: (axis, angle) => orbit(viz, axis, angle),
      toggleProjection: handleToggleProjection,
      toggleRecording,
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
    keymap.setTable(buildGeotoyKeymap(() => actions));
    keymap.install();

    window.addEventListener('beforeunload', persistence.saveDraft);

    return () => {
      bootAbort.abort();
      clearTimeout(controlRunTimer);
      workerManager.terminate();
      execution.dispose();
      keymap.dispose();

      meshScene.clearScene();

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
    spline={splineController.panelCtx}
  />
{/if}

<ExportModal bind:dialog={exportDialog} renderedObjects={meshScene.renderedObjects} />
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
    toggleLightHelpers={meshScene.toggleLightHelpers}
    toggleGizmoGhosts={gizmoController.toggleGhosts}
    showGizmoGhosts={gizmoController.showGhosts}
    gizmosExist={hasAnyGizmos}
    cameraProjection={meshScene.cameraProjection}
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
          {execution}
          gizmoEditorHooks={gizmoController.editorHooks}
          onRun={runManual}
          onCenterView={() => meshScene.focus(null)}
          armedHandleId={gizmoController.armedRef?.kind === 'handle' ? gizmoController.armedRef.name : null}
          readouts={gizmoController.readouts}
        />
      </div>
      <div class="controls">
        <div class="output">
          {@render replControls()}
          <ReplOutput err={execution.err ?? meshScene.materialRuntime.err} runStats={execution.runStats} />
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
