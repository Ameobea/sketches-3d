<script lang="ts">
  import type { MaterialDef } from 'src/geoscript/materials';
  import FormField from './FormField.svelte';
  import ColorPicker from './ColorPicker.svelte';
  import TexturePreview from './TexturePreview.svelte';
  import { Textures } from './state.svelte';

  let {
    material = $bindable(),
    onpicktexture,
  }: {
    material: MaterialDef;
    onpicktexture: (name: 'map' | 'normalMap' | 'roughnessMap' | 'metalnessMap') => void;
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
      <input type="range" min="0" max="2" step="0.01" bind:value={material.normalScale} />
      <span>{material.normalScale?.toFixed(2)}</span>
    </FormField>
    <FormField label="uv scale" help="The scale of the texture coordinates.">
      <input type="number" step="0.1" bind:value={material.uvScale.x} style="width: 80px" />
      <input type="number" step="0.1" bind:value={material.uvScale.y} style="width: 80px" />
    </FormField>

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
</style>
