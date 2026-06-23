<script lang="ts">
  import { evalMathExpr } from '../util/mathExpr';
  import { fmt } from './mathUtils';

  type Vec3 = [number, number, number];

  interface Props {
    label: string;
    values: Vec3;
    onchange: (next: Vec3) => void;
    /** Width of the label column; widen to align with sibling rows outside this component. */
    labelWidth?: string;
  }

  let { label, values, onchange, labelWidth = '24px' }: Props = $props();

  type Axis = 0 | 1 | 2;
  let focused: Axis | null = $state(null);
  let draft = $state('');

  const displayVal = (axis: Axis): string => (focused === axis ? draft : fmt(values[axis]));

  const onFocus = (axis: Axis) => {
    draft = fmt(values[axis]);
    focused = axis;
  };

  const commit = () => {
    if (focused === null) return;
    const n = evalMathExpr(draft);
    if (n === null) return;
    const next: Vec3 = [...values];
    next[focused] = n;
    onchange(next);
  };

  const onBlur = () => {
    commit();
    focused = null;
  };

  const onKeydown = (e: KeyboardEvent) => {
    if (e.key === 'Enter') {
      commit();
      (e.target as HTMLInputElement).blur();
    }
    if (e.key === 'Escape') {
      focused = null;
      (e.target as HTMLInputElement).blur();
    }
  };

  const axisLabels = ['x', 'y', 'z'] as const;
</script>

<div class="tf-row" style:--tf-label-width={labelWidth}>
  <span class="tf-label">{label}</span>
  {#each [0, 1, 2] as const as axis}
    <span class="axis-label">{axisLabels[axis]}</span>
    <input
      class="tf-input"
      type="text"
      value={displayVal(axis)}
      oninput={e => {
        draft = (e.target as HTMLInputElement).value;
      }}
      onfocus={() => onFocus(axis)}
      onblur={onBlur}
      onkeydown={onKeydown}
    />
  {/each}
</div>

<style>
  .tf-row {
    display: flex;
    align-items: center;
    gap: 3px;
  }

  .tf-label {
    width: var(--tf-label-width, 24px);
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
