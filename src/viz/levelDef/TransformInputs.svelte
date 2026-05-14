<script lang="ts">
  import { evalMathExpr } from '../util/mathExpr';

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

  const fmt = (n: number) => {
    const s = n.toFixed(4);
    return s.replace(/(\.\d*?)0+$/, '$1').replace(/\.$/, '.0');
  };

  type Axis = 0 | 1 | 2;
  type Field = 'position' | 'rotation' | 'scale';

  let focused: { field: Field; axis: Axis } | null = $state(null);
  let draft = $state('');

  const arrFor = (field: Field): TransformValue =>
    field === 'position' ? position : field === 'rotation' ? rotation : scale;

  const displayVal = (field: Field, axis: Axis): string => {
    if (focused?.field === field && focused.axis === axis) return draft;
    return fmt(arrFor(field)[axis]);
  };

  const onFocus = (field: Field, axis: Axis) => {
    draft = fmt(arrFor(field)[axis]);
    focused = { field, axis };
  };

  const onBlur = () => {
    commit();
    focused = null;
  };

  const onKeydown = (e: KeyboardEvent) => {
    if (e.key === 'Enter') { commit(); (e.target as HTMLInputElement).blur(); }
    if (e.key === 'Escape') { focused = null; (e.target as HTMLInputElement).blur(); }
  };

  const commit = () => {
    if (!focused) return;
    const n = evalMathExpr(draft);
    if (n === null) return;
    const { field, axis } = focused;
    const src = arrFor(field);
    const next: TransformValue = [src[0], src[1], src[2]];
    next[axis] = n;
    onapply({ [field]: next });
  };

  const axisLabels: [string, string, string] = ['x', 'y', 'z'];
  const fields = ['position', 'rotation', 'scale'] as const;
</script>

{#each fields as field}
  <div class="tf-row">
    <span class="tf-label">{field.slice(0, 3)}</span>
    {#each ([0, 1, 2] as const) as axis}
      <span class="axis-label">{axisLabels[axis]}</span>
      <input
        class="tf-input"
        type="text"
        value={displayVal(field, axis)}
        oninput={(e) => { draft = (e.target as HTMLInputElement).value; }}
        onfocus={() => onFocus(field, axis)}
        onblur={onBlur}
        onkeydown={onKeydown}
      />
    {/each}
  </div>
{/each}

<style>
  .tf-row {
    display: flex;
    align-items: center;
    gap: 3px;
  }

  .tf-label {
    width: 24px;
    font-size: 11px;
    color: #888;
    flex-shrink: 0;
  }

  .axis-label {
    font-size: 10px;
    color: #666;
    width: 8px;
    flex-shrink: 0;
  }

  .tf-input {
    flex: 1;
    min-width: 0;
    background: #111;
    color: #ddd;
    border: 1px solid #444;
    padding: 1px 3px;
    font: 11px monospace;
  }

  .tf-input:focus {
    outline: none;
    border-color: #7a7;
  }
</style>
