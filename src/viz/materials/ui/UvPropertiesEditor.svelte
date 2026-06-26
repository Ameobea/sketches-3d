<script lang="ts">
  import type { CustomShaderMatDef } from 'src/viz/materials/schema';
  import FormField from './FormField.svelte';

  let {
    material,
    rerun,
  }: {
    material: CustomShaderMatDef;
    rerun: (onlyIfUVUnwrapperNotLoaded: boolean) => void;
  } = $props();

  let unwrap = $derived(material.meshUvUnwrap ?? null);
</script>

{#if !!unwrap}
  <FormField
    label="num cones"
    help="The number of singularities in the boundary-first-flattening algorithm. Larger values give the unwrapping algorithm a greater degree of freedom to reduce distortion at the cost of increasing discontinuities."
  >
    <input
      type="number"
      min="0"
      step="1"
      bind:value={unwrap.numCones}
      oninput={() => rerun(false)}
      style="width: 80px"
    />
  </FormField>
  <FormField label="flatten to disk" help="If true, the UVs will be flattened to a disk shape.">
    <input type="checkbox" bind:checked={unwrap.flattenToDisk} oninput={() => rerun(false)} />
  </FormField>
  <FormField
    label="map to sphere"
    help="If true, the UVs will be mapped to a sphere.  This can useful for meshes with spherical topology and will likely have no effect if meshes are not closed and topologically watertight.  It also has no effect if the number of cones is >= 3."
  >
    <input type="checkbox" bind:checked={unwrap.mapToSphere} oninput={() => rerun(false)} />
  </FormField>
  <FormField
    label="enable UV island rotation"
    help="If true, the UV islands will be rotated to minimize their bounding box before packing them into the UV unit square. This can help to reduce wasted space in the UV layout but may result in textures appearing rotated on the mesh."
  >
    <input type="checkbox" bind:checked={unwrap.enableUVIslandRotation} oninput={() => rerun(false)} />
  </FormField>
{/if}
