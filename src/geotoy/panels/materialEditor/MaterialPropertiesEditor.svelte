<script lang="ts">
  import { type MaterialDef, type PhysicalMaterialTextureField } from 'src/geoscript/materials';
  import MaterialForm from 'src/viz/materials/ui/MaterialForm.svelte';
  import type { MaterialEditorHost } from 'src/viz/materials/ui/host';
  import TexturePreview from './TexturePreview.svelte';
  import { Textures } from './state.svelte';
  import type { User } from 'src/geoscript/geotoyAPIClient';

  let {
    material = $bindable(),
    onpicktexture,
    oneditshaders,
    onviewuvmappings,
    rerun,
    showAdvanced = $bindable(),
    onsavetolibrary,
    me,
  }: {
    material: MaterialDef;
    onpicktexture: (name: PhysicalMaterialTextureField) => void;
    oneditshaders: () => void;
    onviewuvmappings: () => void;
    rerun: (onlyIfUVUnwrapperNotLoaded: boolean) => void;
    showAdvanced: boolean;
    onsavetolibrary: () => void;
    me: User | undefined | null;
  } = $props();

  const convertType = (to: 'customShader' | 'customBasicShader') => {
    if (to === material.type) return;
    if (to === 'customBasicShader') {
      material = { ...material, type: 'customBasicShader' } as MaterialDef;
    } else {
      const props: Record<string, unknown> = { ...(material.props ?? {}) };
      props.uvScale ??= [1, 1];
      material = { ...material, type: 'customShader', props, options: material.options ?? {} } as MaterialDef;
    }
  };

  let host = $derived<MaterialEditorHost>({
    showName: true,
    showSaveToLibrary: !!me,
    showUvUnwrap: true,
    showLevelProps: false,
    onpicktexture,
    onconverttype: convertType,
    oneditshaders,
    onviewuvmappings,
    onsavetolibrary,
    rerun,
  });
</script>

{#snippet textureSlot({ field, handle }: { field: PhysicalMaterialTextureField; handle: string | undefined })}
  <TexturePreview
    texture={handle ? Textures.textures[handle] : undefined}
    onclick={() => onpicktexture(field)}
  />
{/snippet}

<MaterialForm bind:material {host} bind:showAdvanced {textureSlot} />
