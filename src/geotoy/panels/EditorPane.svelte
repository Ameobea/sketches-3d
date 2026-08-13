<script lang="ts">
  import { untrack } from 'svelte';
  import type { EditorView, KeyBinding } from '@codemirror/view';

  import { buildEditor } from 'src/geoscript/editor';
  import type { GizmoEditorHooks, GizmoReadout } from 'src/geoscript/gizmoExtensions';
  import { logGeotoyEvent } from 'src/analytics';
  import { GLOBALS_SELECTION_ID, type TreeState } from 'src/geotoy/modules/treeState.svelte';
  import type { GeotoyPersistence } from 'src/geotoy/modules/persistence.svelte';
  import type { GeoscriptExecution } from 'src/geotoy/modules/execution.svelte';

  let {
    treeState,
    persistence,
    execution,
    gizmoEditorHooks,
    onRun,
    onCenterView,
    armedHandleId,
    readouts,
  }: {
    treeState: TreeState;
    persistence: GeotoyPersistence;
    execution: GeoscriptExecution<any>;
    gizmoEditorHooks: GizmoEditorHooks;
    onRun: () => void;
    onCenterView: () => void;
    /** Armed gizmo handle to highlight in the editor (null = none). */
    armedHandleId: string | null;
    /** Inline gizmo readout values, keyed by handle id. */
    readouts: Map<string, GizmoReadout>;
  } = $props();

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

  let codemirrorContainer = $state<HTMLDivElement | null>(null);
  let editorView = $state<EditorView | null>(null);
  let resetEditorHistory: (() => void) | null = null;
  let loggedCodeEdit = false;

  // Editor dispatch channels, wired once the gizmo extensions install. `$state` is
  // load-bearing: the mirror effects below track them, so the install itself re-fires
  // the effects and seeds the editor with the current armed/readout state.
  let dispatchArmed = $state<((handleId: string | null) => void) | null>(null);
  let dispatchValues = $state<((values: Map<string, GizmoReadout>) => void) | null>(null);

  $effect(() => {
    dispatchArmed?.(armedHandleId);
  });
  $effect(() => {
    dispatchValues?.(readouts);
  });

  const createEditor = (container: HTMLDivElement) => {
    const customKeymap: readonly KeyBinding[] = [
      {
        key: 'Ctrl-Enter',
        run: () => {
          if (!editorView) {
            return true;
          }
          onRun();
          return true;
        },
      },
      {
        key: 'Ctrl-.',
        run: () => {
          onCenterView();
          return true;
        },
      },
      {
        key: 'Ctrl-s',
        run: () => {
          persistence.saveDraft();
          return true;
        },
      },
    ];

    const editor = buildEditor({
      container,
      customKeymap,
      initialCode: getActiveSource(),
      onDocChange: () => {
        if (editorView) {
          writeActiveSource(editorView.state.doc.toString());
          if (!loggedCodeEdit && editorView.hasFocus) {
            loggedCodeEdit = true;
            logGeotoyEvent('editor', 'code_edited');
          }
        }
      },
    });
    editorView = editor.editorView;
    resetEditorHistory = editor.resetHistory;

    import('src/geoscript/analysisExtensions').then(({ buildAnalysisExtensions }) => {
      // Expose the `_globals` node as ambient scope so its helpers/constants resolve in other
      // nodes; '' while editing `_globals` itself so it's analyzed directly.
      const getAmbientSource = () =>
        treeState.state.selectedId === GLOBALS_SELECTION_ID ? '' : treeState.state.tree.globalsSource;
      editor.setAnalysisExtensions(
        buildAnalysisExtensions(() => !persistence.preludeEjected, getAmbientSource)
      );
    });

    import('src/geoscript/gizmoExtensions').then(
      ({ buildGizmoExtensions, pushGizmoArmed, pushGizmoValues }) => {
        editor.setGizmoExtensions(buildGizmoExtensions(gizmoEditorHooks));
        dispatchArmed = h => editorView && pushGizmoArmed(editorView, h);
        dispatchValues = m => editorView && pushGizmoValues(editorView, m);
      }
    );

    return editor;
  };

  // Create/destroy the editor with its container; teardown (never tracked) saves the
  // draft, so the deep-proxy tree serialize inside saveDraft can't subscribe this effect.
  $effect(() => {
    if (!codemirrorContainer) return;
    const container = codemirrorContainer;
    const editor = untrack(() => createEditor(container));
    return () => {
      persistence.saveDraft();
      editor.editorView.destroy();
      editorView = null;
      resetEditorHistory = null;
    };
  });

  // Swap the editor doc when the selection changes or tree content is replaced under
  // it (contentEpoch); clear CM undo so Ctrl-Z can't rewind past the swap. First run
  // (pre-editor) no-ops; the create effect seeds initialCode itself.
  $effect(() => {
    void treeState.state.selectedId;
    void treeState.contentEpoch;
    untrack(() => {
      if (!editorView) return;
      editorView.dispatch({
        changes: { from: 0, to: editorView.state.doc.length, insert: getActiveSource() },
        selection: { anchor: 0 },
      });
      resetEditorHistory?.();
    });
  });

  const ejectPrelude = async (editorView: EditorView) => {
    const prelude = await execution.repl.getPrelude();
    editorView.dispatch({
      changes: { from: 0, insert: prelude + '\n//-- end prelude\n\n' },
    });
  };

  // Tree mode: the prelude is render-only (default lights), and renders fired in `_globals`
  // (ambient scope) are dropped — so it has to land in a real scene-eval'd node. The root is
  // that node; dump it there and focus it so it's clear where the code went.
  const ejectPreludeIntoRoot = async () => {
    const prelude = await execution.repl.getPrelude();
    const rootId = treeState.state.tree.rootId;
    const cur = treeState.state.tree.nodes[rootId]?.source ?? '';
    treeState.rewriteSource(rootId, prelude + '\n//-- end prelude\n\n' + cur);
    treeState.setSelected(rootId);
  };

  export const togglePreludeEjected = async () => {
    if (!editorView) {
      return;
    }

    if (!persistence.preludeEjected) {
      if (Object.keys(treeState.state.tree.nodes).length > 1) {
        await ejectPreludeIntoRoot();
      } else {
        await ejectPrelude(editorView);
      }
    }
    persistence.preludeEjected = !persistence.preludeEjected;
    logGeotoyEvent('editor', 'prelude_toggle', { ejected: persistence.preludeEjected });

    execution.run();
  };

  /** Viewport mode → Ctrl-Z routes to the tree undo stack. */
  export const blur = () => editorView?.contentDOM.blur();
</script>

<div bind:this={codemirrorContainer} class="codemirror-wrapper" style="flex: 1; background: #222;"></div>

<style lang="css">
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
</style>
