<script lang="ts">
  import { onDestroy } from 'svelte';
  import type * as Comlink from 'comlink';
  import type { GeoscriptWorkerMethods } from 'src/geoscript/geoscriptWorker.worker';

  import MaterialPropertiesEditor from './MaterialPropertiesEditor.svelte';
  import {
    type BasicMaterialDef,
    buildDefaultMaterial,
    type MaterialDefinitions,
    type MaterialID,
    type PhysicalMaterialDef,
  } from 'src/geoscript/materials';
  import TexturePicker from './TexturePicker.svelte';
  import TextureUploader from './TextureUploader.svelte';
  import ShaderEditor from './ShaderEditor.svelte';
  import { makeDraggable, uuidv4 } from './util';
  import UvViewer from './UVViewer.svelte';

  let {
    isOpen = $bindable(),
    materials = $bindable(),
    rerun,
    repl,
    ctxPtr,
  }: {
    isOpen: boolean;
    materials: MaterialDefinitions;
    rerun: (onlyIfUVUnwrapperNotLoaded: boolean) => Promise<void>;
    repl: Comlink.Remote<GeoscriptWorkerMethods>;
    ctxPtr: number | null;
  } = $props();

  let dialogElement = $state<HTMLDivElement | null>(null);
  let dragHandleElement = $state<HTMLDivElement | null>(null);

  let view = $state<
    | { type: 'properties' }
    | {
        type: 'texture_picker';
        field: 'map' | 'normalMap' | 'roughnessMap' | 'metalnessMap';
      }
    | {
        type: 'texture_uploader';
        field: 'map' | 'normalMap' | 'roughnessMap' | 'metalnessMap';
      }
    | { type: 'shader_editor' }
    | { type: 'uv_viewer' }
  >({ type: 'properties' });

  let selectedMaterialID: MaterialID | null = $state(null);

  $effect(() => {
    if (!selectedMaterialID && materials.materials) {
      selectedMaterialID = Object.keys(materials.materials)[0] || null;
    }
  });

  const addMaterial = () => {
    let i = 1;
    let newName = 'new_material';
    while (materials.materials[newName]) {
      newName = `new_material_${i}`;
      i += 1;
    }
    const id = uuidv4();
    materials.materials[id] = buildDefaultMaterial(newName);
    selectedMaterialID = id;
  };

  let dragCbs: {
    destroy: () => void;
    checkBounds: () => void;
  } | null = null;

  $effect(() => {
    if (dialogElement && dragHandleElement) {
      dragCbs?.destroy();
      if (window.innerWidth > 600) {
        dragCbs = makeDraggable(dialogElement, dragHandleElement);
      }
    }
  });

  $effect(() => {
    if (view.type === 'shader_editor') {
      dragCbs?.checkBounds();
    }
  });

  onDestroy(() => {
    if (dragCbs) {
      dragCbs.destroy();
      dragCbs = null;
    }
  });
</script>

{#if isOpen}
  <div
    class="material-editor-dialog"
    class:shader-editor-open={view.type === 'shader_editor'}
    bind:this={dialogElement}
  >
    <div class="drag-handle" bind:this={dragHandleElement}>
      <span>materials</span>
      <button class="close-button" onclick={() => (isOpen = false)}>×</button>
    </div>
    <div class="content">
      {#if view.type === 'properties'}
        <div class="sidebar">
          <div class="material-list">
            {#each Object.entries(materials.materials) as [id, material] (id)}
              <div class="material-item" class:selected={selectedMaterialID === id}>
                <button
                  class="select-button"
                  onclick={() => {
                    selectedMaterialID = id;
                  }}
                >
                  {material.name}
                </button>
                <button
                  class="delete"
                  onclick={() => {
                    const newMaterials = { ...materials.materials };
                    delete newMaterials[id];
                    materials.materials = newMaterials;

                    if (selectedMaterialID === id) {
                      selectedMaterialID = null;
                    }
                  }}
                >
                  ×
                </button>
              </div>
            {/each}
          </div>
          <button class="add-material" onclick={addMaterial}>add material</button>
        </div>
      {/if}
      {#if selectedMaterialID !== null && !!materials.materials[selectedMaterialID]}
        {#if view.type === 'properties'}
          <MaterialPropertiesEditor
            bind:material={materials.materials[selectedMaterialID]}
            onpicktexture={fieldName => {
              view = { type: 'texture_picker', field: fieldName };
            }}
            oneditshaders={() => {
              view = { type: 'shader_editor' };
            }}
            onviewuvmappings={() => {
              view = { type: 'uv_viewer' };
            }}
            {rerun}
          />
        {:else if view.type === 'texture_picker'}
          {#if materials.materials[selectedMaterialID].type === 'physical'}
            {@const field = view.field}
            <TexturePicker
              bind:selectedTextureId={materials.materials[selectedMaterialID][view.field]}
              onclose={() => {
                view = { type: 'properties' };
              }}
              onupload={() => {
                view = { type: 'texture_uploader', field };
              }}
            />
          {/if}
        {:else if view.type === 'texture_uploader'}
          {#if materials.materials[selectedMaterialID].type === 'physical'}
            {@const field = view.field}
            <TextureUploader
              onclose={() => {
                view = { type: 'texture_picker', field };
              }}
              onupload={texture => {
                if (selectedMaterialID === null) {
                  return;
                }

                if (materials.materials[selectedMaterialID].type === 'physical') {
                  materials.materials[selectedMaterialID][field] = texture.id;
                }
                view = { type: 'properties' };
              }}
            />
          {/if}
        {:else if view.type === 'shader_editor'}
          {@const state =
            materials.materials[selectedMaterialID].type === 'physical'
              ? {
                  type: 'physical' as const,
                  shaders: (materials.materials[selectedMaterialID] as PhysicalMaterialDef).shaders,
                }
              : {
                  type: 'basic' as const,
                  shaders: (materials.materials[selectedMaterialID] as BasicMaterialDef).shaders,
                }}
          {@const onchange = (newState: typeof state) => {
            if (selectedMaterialID === null) {
              return;
            }

            materials.materials[selectedMaterialID].shaders = newState.shaders;
          }}
          <ShaderEditor
            {state}
            {onchange}
            onclose={() => {
              view = { type: 'properties' };
            }}
          />
        {:else if ctxPtr !== null && view.type === 'uv_viewer'}
          {#if materials.materials[selectedMaterialID]?.type === 'physical' && materials.materials[selectedMaterialID].textureMapping?.type === 'uv'}
            <UvViewer
              onclose={() => {
                view = { type: 'properties' };
              }}
              {repl}
              {ctxPtr}
              matDef={materials.materials[selectedMaterialID] as PhysicalMaterialDef}
              {rerun}
            />
          {:else}
            <div style="display: flex; flex-direction: column; flex: 1; max-width: 400px; margin: auto;">
              <p>active material missing or not UV mapped</p>
              <button
                onclick={() => {
                  view = { type: 'properties' };
                }}
              >
                close
              </button>
            </div>
          {/if}
        {/if}
      {/if}
    </div>
  </div>
{/if}

<style>
  .material-editor-dialog {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    background: #222;
    color: #f0f0f0;
    border: 1px solid #888;
    width: 600px;
    max-width: calc(min(600px, 100vw));
    min-height: calc(max(20vh, 400px) + 30px);
    max-height: calc(min(50vh, 600px));
    z-index: 100;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .shader-editor-open {
    width: 80vw;
    height: 80vh;
    max-width: 1200px;
    max-height: 900px;

    .content {
      max-height: calc(100% - 70px) !important;
    }
  }

  .drag-handle {
    display: flex;
    padding: 6px 8px;
    background: #333;
    cursor: grab;
    user-select: none;
    font-size: 13px;

    .close-button {
      margin-left: auto;
      background: none;
      border: none;
      color: #f0f0f0;
      font-size: 22px;
      cursor: pointer;
      padding: 0;
      height: 20px;
      line-height: 0;
      margin-top: -2px;
    }

    .close-button:hover {
      color: #f22;
    }
  }

  .content {
    display: flex;
    flex-grow: 1;
    max-height: calc(max(20vh, 400px));
  }

  .sidebar {
    width: 180px;
    min-width: 180px;
    max-width: 180px;
    padding: 4px;
    display: flex;
    flex-direction: column;
  }

  .material-list {
    flex-grow: 1;
    overflow-y: auto;
    min-height: 0;
  }

  .material-item {
    display: flex;
    flex: 1;
    justify-content: space-between;
    align-items: center;
    border-bottom: 1px solid #333;
  }

  .material-list .material-item:hover {
    background: #333;
  }

  .material-item.selected,
  .material-item.selected:hover {
    background: #444;
  }

  .material-list button {
    display: block;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    color: #f0f0f0;
    font-size: 12px;
    cursor: pointer;
  }

  .material-list .delete {
    background: none;
    border: none;
    color: #f00;
    font-size: 24px;
    cursor: pointer;
    padding: 0 4px;
    flex: 0;
    line-height: 24px;
  }

  .material-list .delete:hover {
    color: #f88;
  }

  .add-material {
    width: calc(100%+12px);
    margin-top: 6px;
    margin-bottom: -4px;
    margin-left: -4px;
    margin-right: -4px;
    background: #333;
    border: 1px solid #555;
    color: #f0f0f0;
    padding: 10px 9px 9px 9px;
    cursor: pointer;
  }

  .add-material:hover {
    background: #3d3d3d;
  }
</style>
