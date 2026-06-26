<script lang="ts">
  import { untrack } from 'svelte';
  import type { MaterialDef } from './types';
  import MaterialForm from 'src/viz/materials/ui/MaterialForm.svelte';
  import ShaderEditor from 'src/viz/materials/ui/ShaderEditor.svelte';
  import { sharedToSlots, slotsToShared } from 'src/viz/materials/ui/shaderSlots';
  import type { MaterialEditorHost, PhysicalMaterialTextureField } from 'src/viz/materials/ui/host';

  interface Props {
    materials: Record<string, MaterialDef>;
    textureKeys: string[];
    initialSelectedId?: string | null;
    onchange: (id: string, def: MaterialDef) => void;
    onadd: (id: string, def: MaterialDef) => void;
    ondelete: (id: string) => void;
  }

  let { materials, textureKeys, initialSelectedId = null, onchange, onadd, ondelete }: Props = $props();

  let selectedId = $state<string | null>(untrack(() => initialSelectedId ?? null));

  export const setSelectedId = (id: string | null) => {
    selectedId = id;
  };

  let view = $state<'properties' | 'shader_editor'>('properties');
  let showAdvanced = $state(false);
  let nameInputMode = $state<'none' | 'new' | 'clone'>('none');
  let nameInputValue = $state('');

  let localDef = $state<MaterialDef | null>(null);
  let suppressEmit = false;

  $effect(() => {
    if (selectedId && materials[selectedId]) {
      suppressEmit = true;
      // Ensure the shape MaterialForm binds against exists before it mounts (avoids a render-time
      // crash on materials lacking props/options) — done here under suppressEmit so it's not a save.
      const snap = $state.snapshot(materials[selectedId]) as MaterialDef;
      if (snap.type === 'customShader') {
        snap.props ??= {};
        snap.options ??= {};
        snap.props.uvScale ??= [1, 1];
      }
      localDef = snap;
    } else {
      localDef = null;
    }
  });

  $effect(() => {
    const snap = $state.snapshot(localDef);
    if (!localDef || !selectedId) return;
    if (suppressEmit) {
      suppressEmit = false;
      return;
    }
    onchange(selectedId, snap as MaterialDef);
  });

  const convertType = (to: 'customShader' | 'customBasicShader') => {
    const cur = localDef;
    if (!cur || cur.type === 'generated' || to === cur.type) return;
    if (to === 'customBasicShader') {
      localDef = { ...cur, type: 'customBasicShader' } as MaterialDef;
    } else {
      const props: Record<string, unknown> = { ...(cur.props ?? {}) };
      props.uvScale ??= [1, 1];
      localDef = { ...cur, type: 'customShader', props, options: cur.options ?? {} } as MaterialDef;
    }
  };

  let host = $derived<MaterialEditorHost>({
    showName: false,
    showSaveToLibrary: false,
    showUvUnwrap: false,
    showLevelProps: true,
    onpicktexture: () => {},
    onconverttype: convertType,
    oneditshaders: () => (view = 'shader_editor'),
    onviewuvmappings: () => {},
    onsavetolibrary: () => {},
    rerun: () => {},
  });

  const confirmName = () => {
    const name = nameInputValue.trim();
    if (!name || materials[name]) return;
    const newDef: MaterialDef =
      nameInputMode === 'clone' && selectedId && materials[selectedId]
        ? ($state.snapshot(materials[selectedId]) as MaterialDef)
        : { type: 'customShader' };
    nameInputMode = 'none';
    nameInputValue = '';
    onadd(name, newDef);
  };

  const cancelName = () => {
    nameInputMode = 'none';
    nameInputValue = '';
  };

  const startNew = () => {
    nameInputValue = '';
    nameInputMode = 'new';
  };

  const startClone = () => {
    if (!selectedId) return;
    nameInputValue = `${selectedId}_copy`;
    nameInputMode = 'clone';
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Enter') confirmName();
    else if (e.key === 'Escape') cancelName();
  };
</script>

{#snippet textureSlot({
  handle,
  set,
}: {
  field: PhysicalMaterialTextureField;
  handle: string | undefined;
  set: (h: string | undefined) => void;
})}
  <select value={handle ?? ''} onchange={e => set((e.target as HTMLSelectElement).value || undefined)}>
    <option value="">(none)</option>
    {#each textureKeys as k}
      <option value={k}>{k}</option>
    {/each}
  </select>
{/snippet}

<div class="mat-editor">
  <div class="header">■ MATERIAL EDITOR</div>

  {#if view === 'shader_editor' && localDef?.type === 'customShader'}
    {@const def = localDef}
    <div class="shader-view">
      <ShaderEditor
        state={{ type: 'physical', shaders: sharedToSlots(def.shaders) }}
        pomEnabled={!!def.options?.pom}
        onchange={ns => {
          if (localDef?.type === 'customShader')
            localDef.shaders = slotsToShared(localDef.shaders, ns.shaders);
        }}
        onclose={() => (view = 'properties')}
      />
    </div>
  {:else}
    <div class="body">
      <div class="mat-list">
        {#each Object.keys(materials) as id (id)}
          <button class="mat-item" class:active={id === selectedId} onclick={() => (selectedId = id)}>
            {id}
          </button>
        {/each}

        {#if nameInputMode !== 'none'}
          <div class="name-input-row">
            <!-- svelte-ignore a11y_autofocus -->
            <input
              class="name-input"
              type="text"
              bind:value={nameInputValue}
              onkeydown={handleKeyDown}
              placeholder={nameInputMode === 'clone' ? 'clone name...' : 'new name...'}
              autofocus
            />
            <button class="confirm-btn" onclick={confirmName} title="Confirm">✓</button>
            <button class="cancel-btn" onclick={cancelName} title="Cancel">✕</button>
          </div>
        {/if}
      </div>

      <div class="mat-form">
        {#if localDef}
          <MaterialForm bind:material={localDef} {host} bind:showAdvanced {textureSlot} />
        {:else}
          <div class="placeholder">select a material</div>
        {/if}
      </div>
    </div>

    <div class="footer">
      <div class="footer-left">
        <button onclick={startNew}>+ New</button>
        <button onclick={startClone} disabled={!selectedId}>⧉ Clone</button>
      </div>
      <div class="footer-right">
        <button class="delete-btn" onclick={() => selectedId && ondelete(selectedId)} disabled={!selectedId}>
          ✕ Delete
        </button>
      </div>
    </div>
  {/if}
</div>

<style>
  .mat-editor {
    position: fixed;
    top: 12px;
    right: 12px;
    width: 640px;
    background: #1a1a1a;
    color: #e8e8e8;
    font:
      12px 'IBM Plex Mono',
      monospace;
    border: 1px solid #444;
    z-index: 9999;
    display: flex;
    flex-direction: column;
    max-height: calc(100vh - 24px);
    pointer-events: auto;
    user-select: none;
  }

  .header {
    padding: 8px 12px;
    font-weight: bold;
    color: #7ec8e3;
    border-bottom: 1px solid #444;
    flex-shrink: 0;
  }

  .body {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .shader-view {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 420px;
    overflow: hidden;
  }

  .mat-list {
    width: 180px;
    flex-shrink: 0;
    border-right: 1px solid #444;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }

  .mat-item {
    display: block;
    width: 100%;
    background: none;
    border: none;
    border-bottom: 1px solid #333;
    color: #ccc;
    font:
      11px 'IBM Plex Mono',
      monospace;
    padding: 6px 8px;
    text-align: left;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mat-item:hover {
    background: #252525;
    color: #e8e8e8;
  }

  .mat-item.active {
    background: #2a3a4a;
    color: #7ec8e3;
  }

  .name-input-row {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px;
    border-top: 1px solid #444;
    flex-shrink: 0;
  }

  .name-input {
    flex: 1;
    min-width: 0;
    background: #111;
    color: #e8e8e8;
    border: 1px solid #555;
    padding: 3px 4px;
    font:
      11px 'IBM Plex Mono',
      monospace;
  }

  .confirm-btn,
  .cancel-btn {
    background: #1a1a1a;
    border: 1px solid #555;
    color: #e8e8e8;
    cursor: pointer;
    padding: 2px 5px;
    font: 11px monospace;
    flex-shrink: 0;
  }

  .mat-form {
    flex: 1;
    overflow-y: auto;
    min-width: 0;
  }

  .placeholder {
    color: #666;
    padding: 12px;
    font-style: italic;
  }

  .footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 6px 10px;
    border-top: 1px solid #444;
    flex-shrink: 0;
    gap: 6px;
  }

  .footer-left,
  .footer-right {
    display: flex;
    gap: 4px;
  }

  .footer button {
    background: #1a1a1a;
    border: 1px solid #555;
    color: #e8e8e8;
    cursor: pointer;
    padding: 3px 8px;
    font:
      11px 'IBM Plex Mono',
      monospace;
  }

  .footer button:disabled {
    color: #555;
    border-color: #333;
    cursor: default;
  }

  .footer button:not(:disabled):hover {
    background: #252525;
  }

  .delete-btn {
    color: #e87070 !important;
  }

  .delete-btn:not(:disabled):hover {
    background: #2a1a1a !important;
  }

  :global(.mat-form .control input[type='range']) {
    flex-grow: 1;
    accent-color: #7ec8e3;
  }

  :global(.mat-form .control input[type='number']) {
    background-color: #1a1a1a;
    color: #eee;
    border: 1px solid #444;
    padding: 3px 5px;
    font-size: 12px;
    font-family: inherit;
    box-sizing: border-box;
  }

  :global(.mat-form .control input[type='color']) {
    width: 40px;
    height: 22px;
    border: 1px solid #444;
    padding: 1px;
    background: #111;
    cursor: pointer;
  }

  :global(.mat-form .control input[type='checkbox']) {
    width: 14px;
    height: 14px;
    cursor: pointer;
    accent-color: #7ec8e3;
  }
</style>
