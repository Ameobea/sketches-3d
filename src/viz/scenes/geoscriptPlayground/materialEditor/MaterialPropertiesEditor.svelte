<script lang="ts">
  import type { MaterialDef } from 'src/geoscript/materials';
  import FormField from './FormField.svelte';
  import ColorPicker from './ColorPicker.svelte';
  import TexturePreview from './TexturePreview.svelte';
  import { Textures } from './state.svelte';
  import UvPropertiesEditor from './UVPropertiesEditor.svelte';

  let {
    material = $bindable(),
    onpicktexture,
    oneditshaders,
    onviewuvmappings,
    rerun,
  }: {
    material: MaterialDef;
    onpicktexture: (name: 'map' | 'normalMap' | 'roughnessMap' | 'metalnessMap') => void;
    oneditshaders: () => void;
    onviewuvmappings: () => void;
    rerun: (onlyIfUVUnwrapperNotLoaded: boolean) => void;
  } = $props();
  let showAdvanced = $state(false);
</script>

<div class="properties-editor">
  <FormField label="name">
    <input type="text" bind:value={material.name} />
  </FormField>

  <FormField label="type">
    <select bind:value={material.type}>
      <option value="basic">basic</option>
      <option value="physical">physical</option>
    </select>
  </FormField>

  <FormField label="color">
    <ColorPicker bind:color={material.color} />
  </FormField>

  {#if material.type === 'physical'}
    <FormField label="roughness">
      <input type="range" min="0" max="1" step="0.01" bind:value={material.roughness} />
      <span>{material.roughness.toFixed(2)}</span>
    </FormField>
    <FormField label="metalness">
      <input type="range" min="0" max="1" step="0.01" bind:value={material.metalness} />
      <span>{material.metalness.toFixed(2)}</span>
    </FormField>

    <FormField label="map">
      <TexturePreview
        texture={material.map ? Textures.textures[material.map] : undefined}
        onclick={() => onpicktexture('map')}
      />
    </FormField>
    <FormField label="normal map">
      <TexturePreview
        texture={material.normalMap ? Textures.textures[material.normalMap] : undefined}
        onclick={() => onpicktexture('normalMap')}
      />
    </FormField>
    <FormField label="roughness map">
      <TexturePreview
        texture={material.roughnessMap ? Textures.textures[material.roughnessMap] : undefined}
        onclick={() => onpicktexture('roughnessMap')}
      />
    </FormField>
    <FormField label="metalness map">
      <TexturePreview
        texture={material.metalnessMap ? Textures.textures[material.metalnessMap] : undefined}
        onclick={() => onpicktexture('metalnessMap')}
      />
    </FormField>
    <FormField label="normal scale">
      <input type="range" min="0" max="5" step="0.01" bind:value={material.normalScale} />
      <span>{material.normalScale?.toFixed(2)}</span>
    </FormField>
    <FormField label="uv scale" help="The scale of the texture coordinates.">
      <input type="number" step="0.1" bind:value={material.uvScale.x} style="width: 80px" />
      <input type="number" step="0.1" bind:value={material.uvScale.y} style="width: 80px" />
    </FormField>
    <FormField
      label="texture mapping"
      help="Controls how textures are mapped to the mesh's surface.  Triplanar mapping works great for many uses, but generating UVs can be useful for exporting to other tools where native triplanar mapping is not available."
    >
      <div class="toggle-group">
        <button
          class:selected={!material.textureMapping || material.textureMapping?.type === 'triplanar'}
          onclick={() => (material.textureMapping = { type: 'triplanar' })}
        >
          triplanar
        </button>
        <button
          class:selected={material.textureMapping?.type === 'uv'}
          onclick={() => {
            if (material.textureMapping?.type !== 'uv') {
              material.textureMapping = { type: 'uv', numCones: 0, flattenToDisk: false, mapToSphere: false };
              rerun(true);
            }
          }}
        >
          uv
        </button>
      </div>
    </FormField>
    {#if material.textureMapping?.type === 'uv'}
      <UvPropertiesEditor {material} {rerun} />
      <div style="display: flex; padding-left: 8px">
        <button class="edit-shaders" onclick={onviewuvmappings} style="width:240px">
          view generated uv mappings
        </button>
      </div>
    {/if}
    <div style="display: flex; padding-left: 8px">
      <button class="edit-shaders" onclick={oneditshaders}>edit shaders</button>
    </div>

    <div class="advanced-options">
      <button
        class="advanced-toggle"
        onclick={() => {
          showAdvanced = !showAdvanced;
        }}
      >
        {showAdvanced ? 'hide' : 'show'} advanced options
      </button>
      {#if showAdvanced}
        <div class="advanced-content">
          <FormField label="clearcoat">
            <input type="range" min="0" max="1" step="0.01" bind:value={material.clearcoat} />
            <span>{material.clearcoat?.toFixed(2)}</span>
          </FormField>
          <FormField label="clearcoat roughness">
            <input type="range" min="0" max="1" step="0.01" bind:value={material.clearcoatRoughness} />
            <span>{material.clearcoatRoughness?.toFixed(2)}</span>
          </FormField>
          <FormField label="iridescence">
            <input type="range" min="0" max="1" step="0.01" bind:value={material.iridescence} />
            <span>{material.iridescence?.toFixed(2)}</span>
          </FormField>
          <FormField label="sheen">
            <input type="range" min="0" max="1" step="0.01" bind:value={material.sheen} />
            <span>{material.sheen?.toFixed(2)}</span>
          </FormField>
          <FormField label="sheen color">
            <ColorPicker bind:color={material.sheenColor} />
          </FormField>
          <FormField label="sheen roughness">
            <input type="range" min="0" max="1" step="0.01" bind:value={material.sheenRoughness} />
            <span>{material.sheenRoughness?.toFixed(2)}</span>
          </FormField>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .properties-editor {
    padding: 16px;
    border-left: 1px solid #444;
    flex-grow: 1;
    overflow-y: auto;
    font-size: 12px;
  }

  .advanced-options {
    margin-top: 16px;
    border-top: 1px solid #444;
    padding-top: 8px;
  }

  .advanced-toggle {
    background: none;
    border: none;
    color: #aaa;
    cursor: pointer;
    padding: 0;
    margin-bottom: 16px;
    font-size: 12px;
  }

  .advanced-content {
    padding-left: 16px;
    font-size: 12px;
  }

  .edit-shaders {
    background: #333;
    border: 1px solid #555;
    color: #f0f0f0;
    padding: 2px 2px 3px 4px;
    cursor: pointer;
    margin-bottom: 16px;
    width: 180px;
    font-size: 12px;
  }

  .edit-shaders:hover {
    background: #3d3d3d;
  }

  .toggle-group {
    display: flex;
  }

  .toggle-group button {
    background: #333;
    border: 1px solid #555;
    color: #f0f0f0;
    padding: 4px 8px;
    cursor: pointer;
    font-size: 12px;
  }

  .toggle-group button.selected {
    background: #555;
    border-color: #777;
  }

  .toggle-group button:not(:last-child) {
    border-right: none;
  }
</style>
