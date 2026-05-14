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
  import {
    type Composition,
    type CompositionVersion,
    type CompositionVersionMetadata,
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
  import { compileTree } from 'src/geoscript/treeCodegen';
  import { TreeState, GLOBALS_SELECTION_ID } from './treeState.svelte';
  import { buildParentMap, computeMeshCounts, findParentId } from './treeOps';
  import HierarchyPanel from './HierarchyPanel.svelte';
  import NodeInspector from './NodeInspector.svelte';
  import { TransformGizmo, type GizmoMode, type GizmoSpace } from './transformGizmo';
  import { installRaycastSelect } from './raycastSelect';
  import { getIsUVUnwrapLoaded } from 'src/viz/wasm/uv_unwrap/uvUnwrap';
  import ReadOnlyCompositionDetails from './ReadOnlyCompositionDetails.svelte';
  import { populateScene, buildWorldMatrixCache } from 'src/geoscript/runner/geoscriptRunner';
  import type { MatEntry, RenderedObject } from 'src/geoscript/runner/types';
  import {
    buildCustomMaterials,
    fetchAndSetTextures,
    getReferencedTextureIDs,
  } from './materialLoading.svelte';
  import { centerView, focusOnSubtree, snapView, orbit } from './cameraControls';
  import { buildLightHelpers, toggleAxisHelpers, toggleLightHelpers } from './gizmos';
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

  const { toggleRecording, recordingState } = useRecording(untrack(() => viz), untrack(() => providedUserData));

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
  } = $derived(loadState(userData));

  const treeState = new TreeState({
    initial: untrack(() => initialTree),
    savedBaseline:
      untrack(() => userData?.initialComposition?.version.tree) ?? untrack(() => initialTree),
  });
  treeState.setSelected(untrack(() => initialTree).rootId);

  let failedNodeIds = $state<Set<string>>(new Set());

  const getActiveSource = (): string => {
    const sel = treeState.state.selectedId;
    if (sel === GLOBALS_SELECTION_ID) return treeState.state.tree.globalsSource;
    if (sel && treeState.state.tree.nodes[sel]) return treeState.state.tree.nodes[sel].source;
    return '';
  };
  // Source edits skip `applyEdit`: CM owns per-node text undo; tree undo handles
  // only structural ops, transforms, disable, etc.
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
          onTransformChange: (id, transform) => {
            treeState.applyEdit(`transform:${id}`, () => treeState.setTransform(id, transform));
            isDirty = true;
            runOrFast();
          },
          onDragEnd: () => {
            // Catches the final state if the last `onTransformChange` was dropped by `isRunning`.
            runOrFast();
          },
        }
      );
      const tickGizmo = () => g.update();
      viz.registerBeforeRenderCb(tickGizmo);
      const disposer = installRaycastSelect({
        canvas: viz.renderer.domElement,
        camera: viz.camera,
        getCandidates: () =>
          renderedObjects.filter(o => o instanceof THREE.Mesh && !!o.userData.sourceNodeId),
        onSelect: id => {
          // Empty-space click deselects to root — same neutral state Escape produces.
          treeState.setSelected(id ?? treeState.state.tree.rootId);
        },
        isDraggingGizmo: () => g.dragging(),
      });
      if (cancelled) {
        viz.unregisterBeforeRenderCb(tickGizmo);
        disposer();
        g.dispose();
        return;
      }
      gizmo = g;
      raycastDisposer = disposer;
      gizmoTick = tickGizmo;
    })();
    return () => {
      cancelled = true;
      raycastDisposer?.();
      raycastDisposer = null;
      if (gizmoTick) viz.unregisterBeforeRenderCb(gizmoTick);
      gizmoTick = null;
      gizmo?.dispose();
      gizmo = null;
    };
  });

  // Re-sync gizmo on selection change and after each run (ancestor world transforms
  // refresh). Reading `lastRunTree` subscribes the effect to reruns.
  $effect(() => {
    const sel = treeState.state.selectedId;
    void lastRunTree;
    if (!gizmo) return;
    const id = sel === null || sel === GLOBALS_SELECTION_ID ? null : sel;
    gizmo.syncTo(id, treeState.state.tree);
  });

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

  const isEditorFocused = (): boolean => {
    const active = document.activeElement;
    if (!active || !codemirrorContainer) return false;
    return codemirrorContainer.contains(active);
  };

  // Blender-style hotkeys: G/R/S/L/./Esc/Delete plus `/` solo and F2 rename.
  onMount(() => {
    const isTypingTarget = (el: Element | null): boolean => {
      if (!el) return false;
      const tag = el.tagName.toLowerCase();
      if (tag === 'input' || tag === 'textarea') return true;
      return (el as HTMLElement).isContentEditable;
    };

    const handler = (e: KeyboardEvent) => {
      // When the editor is focused, let CM's historyKeymap handle undo/redo
      // (per-node text history); otherwise route to the tree undo system.
      const isMod = e.ctrlKey || e.metaKey;
      const isUndoKey = isMod && !e.altKey && (e.key === 'z' || e.key === 'Z');
      const isRedoKey =
        isMod &&
        !e.altKey &&
        ((e.shiftKey && (e.key === 'z' || e.key === 'Z')) || (!e.shiftKey && e.key === 'y'));
      if (isUndoKey || isRedoKey) {
        if (e.defaultPrevented) return;
        if (isEditorFocused()) return;
        if (isRedoKey) runRedo();
        else runUndo();
        e.preventDefault();
        return;
      }

      if (isTypingTarget(document.activeElement)) return;
      if (isMod || e.altKey) return;

      const sel = treeState.state.selectedId;
      const tree = treeState.state.tree;
      const isNodeSelected = sel !== null && sel !== GLOBALS_SELECTION_ID && !!tree.nodes[sel];

      // Delete is destructive enough that we want it only when the user is
      // demonstrably in a tree-editing context — focused inside the panel or
      // with no UI focused at all. Other shortcuts are non-destructive and
      // applied broadly, but still no-op when nothing's selected.
      const active = document.activeElement;
      const inHierarchyPanel = !!(active && (active as HTMLElement).closest?.('[data-hierarchy-panel]'));
      const treeContextFocused = inHierarchyPanel || !active || active === document.body;

      switch (e.key) {
        case 'g':
        case 'G':
          if (!isNodeSelected) return;
          setGizmoMode('translate');
          e.preventDefault();
          return;
        case 'r':
        case 'R':
          if (!isNodeSelected) return;
          setGizmoMode('rotate');
          e.preventDefault();
          return;
        case 's':
        case 'S':
          if (!isNodeSelected) return;
          setGizmoMode('scale');
          e.preventDefault();
          return;
        case 'l':
        case 'L':
          if (!isNodeSelected) return;
          toggleGizmoSpace();
          e.preventDefault();
          return;
        case '.':
          if (isNodeSelected) {
            focusOnSubtree(viz, renderedObjects, tree, sel);
            e.preventDefault();
          }
          return;
        case '/':
          if (isNodeSelected && sel !== tree.rootId) {
            treeState.setSolo(treeState.state.soloId === sel ? null : sel);
            e.preventDefault();
          }
          return;
        case 'Escape':
          if (treeState.state.soloId !== null) {
            treeState.setSolo(null);
            e.preventDefault();
          } else if (isNodeSelected && sel !== tree.rootId) {
            treeState.setSelected(tree.rootId);
            e.preventDefault();
          }
          return;
        case 'Delete':
          if (!treeContextFocused) return;
          if (isNodeSelected && treeState.canDelete(sel)) {
            e.preventDefault();
            treeState.applyEdit(null, () => treeState.deleteNode(sel));
            isDirty = true;
          }
          return;
        case 'F2':
          if (isNodeSelected && sel !== tree.rootId) {
            e.preventDefault();
            hierarchyPanel?.startRename(sel);
          }
          return;
      }
    };

    // Capture phase so our Escape claims the event before Viz's pause toggle.
    window.addEventListener('keydown', handler, true);
    return () => window.removeEventListener('keydown', handler, true);
  });

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
      editor.setAnalysisExtensions(buildAnalysisExtensions(() => !preludeEjected));
    });
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
  let materialDefinitions = $state<MaterialDefinitions>(untrack(() => initialMatDefs));
  let preludeEjected = $state(untrack(() => initialPreludeEjected));

  onMount(() => {
    const referencedTextureIDs = getReferencedTextureIDs(materialDefinitions.materials);
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
  let customMaterials: Record<string, MatEntry> = $derived.by(() =>
    // `$state.snapshot` seems required here in order to trigger this derived to actually run when things change
    buildCustomMaterials(
      loader,
      $state.snapshot(materialDefinitions.materials) as Record<string, MaterialDef>,
      viz
    )
  );

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
    for (const obj of renderedObjects) {
      if (!(obj instanceof THREE.Mesh)) {
        continue;
      }

      for (const [id, matEntry] of Object.entries(customMaterials)) {
        if (obj.material.name === id) {
          if (matEntry.resolved) {
            obj.material = matEntry.resolved;
          } else {
            matEntry.promise.then(mat => {
              obj.material = mat;
            });
          }
          break;
        }
      }
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

  const buildModuleNameToNodeId = (tree: TreeDef): Record<string, string> => {
    const out: Record<string, string> = {};
    for (const node of Object.values(tree.nodes)) {
      if (!node.disabled) out[node.name] = node.id;
    }
    return out;
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
      parts.push(`n:${k}:${n.name}:${n.disabled ? 1 : 0}:${n.source}:${n.children.join(',')}`);
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

  /** Recompose each mesh's `ancestor × localInScript` if only transforms changed. */
  const tryTransformOnlyFastPath = (): boolean => {
    if (isRunning) return false;
    if (lastEvalInputsHash === null) return false;
    if (computeEvalInputsHash() !== lastEvalInputsHash) return false;

    const tree = treeState.serialize();
    const worldMatrices = buildWorldMatrixCache(tree, buildParentMap(tree));
    for (const obj of renderedObjects) {
      if (!(obj instanceof THREE.Mesh)) continue;
      const sourceNodeId = obj.userData.sourceNodeId as string | undefined;
      const localInScript = obj.userData.localInScript as THREE.Matrix4 | undefined;
      if (!sourceNodeId || !localInScript) continue;
      const ancestor = worldMatrices.get(sourceNodeId)?.clone() ?? new THREE.Matrix4();
      const final = ancestor.multiply(localInScript);
      final.decompose(obj.position, obj.quaternion, obj.scale);
    }
    lastRunTree = tree;
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
    const prevByReuseKey = new Map<number, RenderedObject>();
    for (const obj of prevObjects) {
      const key = obj.userData.reuseKey as number | undefined;
      if (typeof key === 'number') prevByReuseKey.set(key, obj);
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
        const key = obj.userData.reuseKey as number | undefined;
        if (typeof key === 'number' && populated.reusedKeys.has(key)) continue;
        removeRenderedObject(obj);
      }
      lastRunTree = tree;

      const directCounts = new Map<string, number>();
      for (const obj of renderedObjects) {
        if (!(obj instanceof THREE.Mesh)) continue;
        const id = obj.userData.sourceNodeId as string | undefined;
        if (!id) continue;
        directCounts.set(id, (directCounts.get(id) ?? 0) + 1);
      }
      meshCounts = computeMeshCounts(tree, directCounts);

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
      // cancel() bumped runGen and is taking over teardown — drop this run's
      // rejection silently. Anything else is a real bug worth surfacing.
      if (myGen !== runGen) return;
      console.error('geoscript run failed', e);
      err = `Run failed: ${e instanceof Error ? e.message : String(e)}`;
      isRunning = false;
    }
  };

  const handleInspectorTransformChange = (id: string, transform: Transform3) => {
    treeState.applyEdit(`transform:${id}`, () => treeState.setTransform(id, transform));
    isDirty = true;
    runOrFast();
  };

  const handleInspectorDisableToggle = (id: string, disabled: boolean) => {
    treeState.applyEdit(null, () => treeState.setDisabled(id, disabled));
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

  const togglePreludeEjected = async () => {
    if (!editorView) {
      return;
    }

    if (!preludeEjected) {
      await ejectPrelude(editorView);
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
    if ('fov' in viz.camera && view.fov !== undefined) {
      viz.camera.fov = view.fov;
      viz.camera.updateProjectionMatrix();
    }
    if ('zoom' in viz.camera && view.zoom !== undefined) {
      viz.camera.zoom = view.zoom;
      viz.camera.updateProjectionMatrix();
    }
    viz.camera.lookAt(viz.orbitControls.target);
    viz.orbitControls.update();
  };

  const clearLocalChanges = () => {
    if (isDirty && !confirm('Really clear local changes?')) {
      return;
    }

    clearSavedState(userData);

    const serverState = getServerState(userData);

    // Reset the tree to the server-side version. `replaceTree` clears selection/solo
    // and updates the dirty baseline; the editor-swap effect picks up the new
    // selection and refreshes the doc, but we set selection explicitly here so we
    // land on a real node rather than null.
    treeState.replaceTree(serverState.tree);
    treeState.setSelected(serverState.tree.rootId);

    didInitMats = false;

    materialDefinitions = serverState.materials;
    const referencedTextureIDs = getReferencedTextureIDs(materialDefinitions.materials);
    fetchAndSetTextures(loader, referencedTextureIDs).then(() => {
      didInitMats = false;
      materialDefinitions = { ...serverState.materials };
    });

    if (serverState.view) {
      setView(serverState.view);
    }
    preludeEjected = serverState.preludeEjected;

    run();

    saveState(
      {
        tree: serverState.tree,
        materials: serverState.materials,
        view: serverState.view,
        preludeEjected: serverState.preludeEjected,
      },
      userData
    );

    isDirty = false;
  };

  const wrappedToggleAxesHelpers = () => toggleAxisHelpers(viz);
  const wrappedToggleLightHelpers = () => {
    lightHelpers = toggleLightHelpers(viz, renderedObjects, lightHelpers);
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
      centerView: () => centerView(viz, renderedObjects),
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
      toggleRecording,
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
      {isDirty}
      {preludeEjected}
      {togglePreludeEjected}
      toggleMaterialEditorOpen={() => (materialEditorOpen = true)}
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
            onselect={(id) => treeState.setSelected(id)}
            onsoloToggle={(id) => treeState.setSolo(treeState.state.soloId === id ? null : id)}
            onDisableToggle={(id) => {
              const node = treeState.state.tree.nodes[id];
              if (node) treeState.applyEdit(null, () => treeState.setDisabled(id, !node.disabled));
              isDirty = true;
            }}
            oncreate={(parentId) => {
              treeState.applyEdit(null, () => {
                const newId = treeState.createNode({ parentId: parentId ?? undefined });
                treeState.setSelected(newId);
              });
              isDirty = true;
            }}
            ondelete={(id) => {
              treeState.applyEdit(null, () => treeState.deleteNode(id));
              isDirty = true;
            }}
            onrename={(id, newName) => {
              try {
                treeState.applyEdit(null, () => treeState.rename(id, newName));
                isDirty = true;
                return true;
              } catch (err) {
                console.warn('rename failed:', err);
                return false;
              }
            }}
            onreparent={(id, newParentId) => {
              try {
                treeState.applyEdit(null, () => treeState.reparent(id, newParentId));
                isDirty = true;
              } catch (err) {
                console.warn('reparent failed:', err);
              }
            }}
            canDelete={(id) => treeState.canDelete(id)}
          />
        </div>
      {/if}
      <div class="editor-pane">
        {#if treePanelVisible || breadcrumb}
          <div class="editor-header">
            <span class="breadcrumb">{breadcrumb || '(no selection)'}</span>
            {#if treeState.state.selectedId
              && treeState.state.selectedId !== GLOBALS_SELECTION_ID
              && treeState.state.selectedId !== treeState.state.tree.rootId}
              <span
                class="gizmo-indicator"
                title="gizmo mode (G/R/S) · space (L)"
              >{gizmoMode[0]}·{gizmoSpace === 'world' ? 'W' : 'L'}</span>
            {/if}
            {#if !treePanelVisible}
              <button
                class="add-node-btn"
                title="add a sibling node"
                onclick={() => {
                  treeState.applyEdit(null, () => {
                    const newId = treeState.createNode({ name: 'node_2' });
                    treeState.setSelected(newId);
                  });
                  isDirty = true;
                }}
              >
                + node
              </button>
            {/if}
          </div>
        {/if}
        {#if treeState.state.selectedId
          && treeState.state.selectedId !== GLOBALS_SELECTION_ID
          && (treeState.state.tree.nodes[treeState.state.selectedId]?.children.length ?? 0) > 0}
          <NodeInspector
            tree={treeState.state.tree}
            parentId={treeState.state.selectedId}
            {meshCounts}
            onselect={(id) => treeState.setSelected(id)}
            onTransformChange={handleInspectorTransformChange}
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
            {isDirty}
            {preludeEjected}
            {togglePreludeEjected}
            toggleMaterialEditorOpen={() => {
              materialEditorOpen = !materialEditorOpen;
            }}
            {toggleLayoutOrientation}
          />
          <ReplOutput {err} {runStats} />
        </div>
        {#if userData?.me}
          {#if !userData.initialComposition || userData.me.id === userData.initialComposition.comp.author_id}
            <SaveControls
              comp={userData.initialComposition?.comp}
              getCurrentTree={() => treeState.serialize()}
              materials={materialDefinitions}
              {viz}
              {preludeEjected}
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
            <ReadOnlyCompositionDetails
              comp={userData.initialComposition.comp}
              showFork={false}
            />
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
    flex: 0 0 180px;
    width: auto;
    height: 180px;
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
    border-radius: 2px;
    padding: 0 4px;
    line-height: 14px;
    flex-shrink: 0;
    user-select: none;
  }

  .add-node-btn {
    background: #1c1c1c;
    color: #ddd;
    border: 1px solid #444;
    border-radius: 2px;
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
