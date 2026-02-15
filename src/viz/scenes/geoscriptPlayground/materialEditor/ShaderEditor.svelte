<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import type { EditorView } from 'codemirror';
  import type { KeyBinding } from '@codemirror/view';
  import { buildEditor, buildGLSLLanguage } from 'src/geoscript/editor';
  import {
    buildDefaultShaders,
    type BasicMaterialDef,
    type PhysicalMaterialDef,
  } from 'src/geoscript/materials';

  type State =
    | { type: 'physical'; shaders: PhysicalMaterialDef['shaders'] }
    | { type: 'basic'; shaders: BasicMaterialDef['shaders'] };

  let {
    state: shaderState,
    onchange,
    onclose,
  }: { state: State; onchange: (newState: State) => void; onclose: () => void } = $props();

  let codemirrorContainer = $state<HTMLDivElement | null>(null);
  let editorView = $state<EditorView | null>(null);

  const shaderTypes = {
    physical: ['color', 'roughness', 'metalness', 'iridescence'] as const,
    basic: ['color'] as const,
  };

  let activeShader = $state<(typeof shaderTypes.physical)[number]>('color');

  let localShaders = $state({ ...shaderState.shaders });
  const initialShaders = { ...shaderState.shaders };

  const getCode = () => (localShaders as any)?.[activeShader] ?? buildDefaultShaders()[activeShader];

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
  ];

  onMount(() => {
    if (!codemirrorContainer || !!editorView) {
      return;
    }

    buildGLSLLanguage()
      .then(language => language)
      .then(language => {
        if (!codemirrorContainer || !!editorView) {
          return;
        }

        const editor = buildEditor({
          container: codemirrorContainer,
          initialCode: getCode(),
          buildLanguage: () => language,
          onDocChange: () => {
            if (editorView) {
              (localShaders as any)[activeShader] = editorView.state.doc.toString();
            }
          },
          customKeymap,
        });
        editorView = editor.editorView;
      });
  });

  onDestroy(() => {
    editorView?.destroy();
    editorView = null;
  });

  const save = () => {
    onchange({ ...shaderState, shaders: { ...localShaders } });
    onclose();
  };

  const run = () => onchange({ ...shaderState, shaders: { ...localShaders } });

  const cancel = () => {
    localShaders = { ...initialShaders };
    onchange({ ...shaderState, shaders: { ...initialShaders } });
    onclose();
  };
</script>

<div style="width: 100%" class="shader-editor">
  <div class="root">
    <div class="content">
      <div class="sidebar">
        <div class="shader-list">
          {#each shaderTypes[shaderState.type] as shaderName (shaderName)}
            <div class="shader-item" class:selected={activeShader === shaderName}>
              <button
                class="select-button"
                onclick={() => {
                  activeShader = shaderName;
                  if (editorView) {
                    editorView.dispatch({
                      changes: { from: 0, to: editorView.state.doc.length, insert: getCode() },
                    });
                  }
                }}
              >
                {shaderName}
              </button>
            </div>
          {/each}
        </div>
      </div>
      <div bind:this={codemirrorContainer} class="codemirror-wrapper"></div>
    </div>
  </div>
  <div class="actions">
    <button onclick={cancel}>cancel</button>
    <button onclick={save}>save</button>
    <button onclick={run}>run</button>
  </div>
</div>

<style lang="css">
  .root {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    height: 100%;
  }

  .content {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  .sidebar {
    width: 180px;
    min-width: 180px;
    max-width: 180px;
    padding: 4px;
    display: flex;
    flex-direction: column;
    border-right: 1px solid #555;
  }

  .shader-list {
    flex-grow: 1;
    overflow-y: auto;
    min-height: 0;
  }

  .shader-item {
    display: flex;
    flex: 1;
    justify-content: space-between;
    align-items: center;
    border-bottom: 1px solid #333;
  }

  .shader-list .shader-item:hover {
    background: #333;
  }

  .shader-item.selected,
  .shader-item.selected:hover {
    background: #444;
  }

  .shader-list button {
    display: block;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    color: #f0f0f0;
    font-size: 12px;
    cursor: pointer;
    padding: 8px;
  }

  .actions {
    display: flex;
    flex-direction: row-reverse;
    gap: 6px;
    padding-top: 6px;
    padding-left: 8px;
    padding-right: 8px;
    border-top: 1px solid #555;

    button {
      background: #333;
      border: 1px solid #555;
      color: #f0f0f0;
      padding: 4px 6px 3px 6px;
      cursor: pointer;
    }
  }

  .actions button:hover {
    background: #3d3d3d;
  }

  .codemirror-wrapper {
    flex: 1;
    background: #222;
    min-width: 0;
    font-size: 15px;
  }

  :global(.shader-editor .codemirror-wrapper > div) {
    display: flex;
    flex: 1;
    width: 100%;
    min-width: 0;
    box-sizing: border-box;
  }

  :global(.shader-editor .cm-content) {
    padding-top: 0 !important;
  }

  :global(.shader-editor .cm-editor) {
    height: 100%;
  }
</style>
