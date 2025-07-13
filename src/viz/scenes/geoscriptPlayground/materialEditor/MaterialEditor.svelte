<script lang="ts">
  import { onDestroy } from 'svelte';
  import { makeDraggable, uuidv4 } from './util';

  import MaterialPropertiesEditor from './MaterialPropertiesEditor.svelte';
  import { buildDefaultMaterial, type MaterialDefinitions, type MaterialID } from 'src/geoscript/materials';
  import TexturePicker from './TexturePicker.svelte';

  let {
    isOpen = $bindable(),
    materials = $bindable(),
  }: {
    isOpen: boolean;
    materials: MaterialDefinitions;
  } = $props();

  let dialogElement = $state<HTMLDivElement | null>(null);
  let dragHandleElement = $state<HTMLDivElement | null>(null);

  let view = $state<
    | { type: 'properties' }
    | { type: 'texture_picker'; field: 'map' | 'normalMap' | 'roughnessMap' | 'metalnessMap' }
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
  } | null = null;

  $effect(() => {
    if (dialogElement && dragHandleElement) {
      dragCbs?.destroy();
      dragCbs = makeDraggable(dialogElement, dragHandleElement, 340, 340);
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
  <div class="material-editor-dialog" bind:this={dialogElement}>
    <div class="drag-handle" bind:this={dragHandleElement}>
      <span>materials</span>
      <button class="close-button" onclick={() => (isOpen = false)}>×</button>
    </div>
    <div class="content">
      <div class="sidebar">
        <div class="material-list">
          {#each Object.entries(materials.materials) as [id, material] (id)}
            <div class="material-item" class:selected={selectedMaterialID === id}>
              <button
                class="select-button"
                onclick={() => {
                  console.log({ selectedMaterialID, id });
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
      {#if selectedMaterialID !== null}
        {#if view.type === 'properties'}
          <MaterialPropertiesEditor
            bind:material={materials.materials[selectedMaterialID]}
            onpicktexture={fieldName => {
              view = { type: 'texture_picker', field: fieldName };
            }}
          />
        {:else if view.type === 'texture_picker'}
          {#if materials.materials[selectedMaterialID].type === 'physical'}
            <TexturePicker
              bind:selectedTextureId={materials.materials[selectedMaterialID][view.field]}
              onclose={() => {
                view = { type: 'properties' };
              }}
            />
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
    width: 80%;
    max-width: 600px;
    min-height: 400px;
    max-height: calc(min(50vh, 600px));
    z-index: 100;
    display: flex;
    flex-direction: column;
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
    width: 200px;
    min-width: 200px;
    max-width: 200px;
    padding: 6px;
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
  }

  .material-list .delete:hover {
    color: #f88;
  }

  .add-material {
    width: 100%;
    margin-top: 8px;
    background: #333;
    border: 1px solid #555;
    color: #f0f0f0;
    padding: 8px;
    cursor: pointer;
  }

  .add-material:hover {
    background: #3d3d3d;
  }
</style>
