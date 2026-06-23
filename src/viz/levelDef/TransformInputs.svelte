<script lang="ts">
  import Vec3Input from './Vec3Input.svelte';

  type TransformValue = [number, number, number];

  export type TransformPatch = Partial<{
    position: TransformValue;
    rotation: TransformValue;
    scale: TransformValue;
  }>;

  interface Props {
    position: TransformValue;
    rotation: TransformValue;
    scale: TransformValue;
    onapply: (patch: TransformPatch) => void;
  }

  let { position, rotation, scale, onapply }: Props = $props();

  const fields = [
    ['pos', 'position'],
    ['rot', 'rotation'],
    ['scl', 'scale'],
  ] as const;

  const valuesFor = (field: 'position' | 'rotation' | 'scale'): TransformValue =>
    field === 'position' ? position : field === 'rotation' ? rotation : scale;
</script>

{#each fields as [label, field] (field)}
  <Vec3Input {label} values={valuesFor(field)} onchange={next => onapply({ [field]: next })} />
{/each}
