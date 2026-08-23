<script lang="ts">
  import type { CustomShaderMatDef, UvAlignAxis } from 'src/viz/materials/schema';
  import { DEFAULT_UV_ALIGN } from 'src/viz/wasm/uv_unwrap/uvAlign';
  import FormField from './FormField.svelte';

  let {
    material,
    rerun,
  }: {
    material: CustomShaderMatDef;
    rerun: (onlyIfUVUnwrapperNotLoaded: boolean) => void;
  } = $props();

  let unwrap = $derived(material.meshUvUnwrap ?? null);

  const AXES: UvAlignAxis[] = ['+x', '-x', '+y', '-y', '+z', '-z'];
  const isParallel = (a: UvAlignAxis, b: UvAlignAxis) => a[1] === b[1];
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
    label="align islands"
    help="Rotates each UV island so a texture axis follows a fixed local-space direction on every face, instead of rotating islands freely for tighter packing.  Use for anisotropic textures (planks, slats, stripes)."
  >
    <input
      type="checkbox"
      checked={!!unwrap.align}
      onchange={() => {
        unwrap.align = unwrap.align ? undefined : { ...DEFAULT_UV_ALIGN };
        rerun(false);
      }}
    />
  </FormField>
  {#if unwrap.align}
    <div class="align-controls">
      <FormField
        label={`texture ${unwrap.align.axis} →`}
        help="Local-space direction the pinned texture axis follows on each face."
      >
        <div class="toggle-group">
          {#each AXES as axis (axis)}
            <button
              class:selected={unwrap.align.up === axis}
              onclick={() => {
                unwrap.align!.up = axis;
                if (isParallel(axis, unwrap.align!.fallback)) {
                  unwrap.align!.fallback = axis[1] === 'y' ? '-z' : '+y';
                }
                rerun(false);
              }}
            >
              {axis.toUpperCase()}
            </button>
          {/each}
        </div>
      </FormField>
      <FormField
        label="fallback →"
        help="Used instead for faces whose normal is within 15° of the primary direction (floors and ceilings when the primary is ±Y)."
      >
        <div class="toggle-group">
          {#each AXES as axis (axis)}
            <button
              class:selected={unwrap.align.fallback === axis}
              disabled={isParallel(axis, unwrap.align.up)}
              onclick={() => {
                unwrap.align!.fallback = axis;
                rerun(false);
              }}
            >
              {axis.toUpperCase()}
            </button>
          {/each}
        </div>
      </FormField>
      <FormField
        label="pinned uv axis"
        help="Which texture axis follows the direction above.  +v is texture up; +u is texture right — pick +u to run a texture's grain the other way."
      >
        <select bind:value={unwrap.align.axis} onchange={() => rerun(false)}>
          <option value="+v">+v (texture up)</option>
          <option value="+u">+u (texture right)</option>
          <option value="-v">−v</option>
          <option value="-u">−u</option>
        </select>
      </FormField>
    </div>
  {/if}
{/if}

<style>
  .align-controls {
    padding-left: 16px;
  }

  .toggle-group {
    display: flex;
  }

  .toggle-group button {
    background: #333;
    border: 1px solid #555;
    color: #f0f0f0;
    padding: 2px 6px;
    cursor: pointer;
    font-size: 12px;
  }

  .toggle-group button.selected {
    background: #555;
    border-color: #777;
  }

  .toggle-group button:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .toggle-group button:not(:last-child) {
    border-right: none;
  }
</style>
