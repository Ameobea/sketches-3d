<script lang="ts">
  import type { MaterialDef } from 'src/geoscript/materials';
  import FormField from './FormField.svelte';

  let {
    material: rawMat,
    rerun,
  }: {
    material: Extract<MaterialDef, { type: 'physical' }>;
    rerun: (onlyIfUVUnwrapperNotLoaded: boolean) => void;
  } = $props();

  let material = $derived.by(() => {
    if (rawMat.textureMapping?.type !== 'uv') {
      return null;
    }
    return rawMat as Omit<MaterialDef, 'textureMapping'> & {
      textureMapping: Extract<MaterialDef['textureMapping'], { type: 'uv' }>;
    };
  });
</script>

{#if !!material}
  <FormField
    label="num cones"
    help="The number of singularities in the boundary-first-flattening algorithm. Larger values give the unwrapping algorithm a greater degree of freedom to reduce distortion at the cost of increasing discontinuities."
  >
    <input
      type="number"
      min="0"
      step="1"
      bind:value={material.textureMapping.numCones}
      oninput={() => rerun(false)}
      style="width: 80px"
    />
  </FormField>
  <FormField label="flatten to disk" help="If true, the UVs will be flattened to a disk shape.">
    <input
      type="checkbox"
      bind:checked={material.textureMapping.flattenToDisk}
      oninput={() => rerun(false)}
    />
  </FormField>
  <FormField
    label="map to sphere"
    help="If true, the UVs will be mapped to a sphere.  This can useful for meshes with spherical topology and will likely have no effect if meshes are not closed and topologically watertight.  It also has no effect if the number of cones is >= 3."
  >
    <input type="checkbox" bind:checked={material.textureMapping.mapToSphere} oninput={() => rerun(false)} />
  </FormField>
  <FormField
    label="enable UV island rotation"
    help="If true, the UV islands will be rotated to minimize their bounding box before packing them into the UV unit square. This can help to reduce wasted space in the UV layout but may result in textures appearing rotated on the mesh."
  >
    <input
      type="checkbox"
      bind:checked={material.textureMapping.enableUVIslandRotation}
      oninput={() => rerun(false)}
    />
  </FormField>
{/if}
