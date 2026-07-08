import type { ControlPanelSetting } from 'src/viz/UI/ControlPanel';
import type { RenderedControl } from './runner/types';

/** Panel key for a control site: handleIds are unique only per module, so join both.
 *  NUL separator — collision-proof, and explicit so it can't be mistaken for a space. */
export const controlKey = (c: RenderedControl): string => `${c.sourceModule ?? ''}\0${c.handleId}`;

/** Spline-editing surface bridged from a viewport `SplineOverlay` into a controls panel. */
export interface SplinePanelCtx {
  activeKey: string | null;
  points: [number, number, number][];
  selectedIx: number | null;
  toggle(c: RenderedControl): void;
  select(ix: number): void;
  setPoint(ix: number, p: [number, number, number]): void;
  add(): void;
  remove(ix: number): void;
}

/** A spline control's reported flat 3·N floats → point triples. */
export const splineControlPoints = (c: RenderedControl): [number, number, number][] => {
  const out: [number, number, number][] = [];
  for (let i = 0; i + 2 < c.value.length; i += 3) {
    out.push([c.value[i], c.value[i + 1], c.value[i + 2]]);
  }
  return out;
};

/** The control's current value as reported by the last eval, in ControlPanel state form. */
export const controlCurrentValue = (c: RenderedControl): any => {
  switch (c.kind) {
    case 'float':
    case 'int':
      return c.value[0] ?? 0;
    case 'bool':
      return (c.value[0] ?? 0) !== 0;
    case 'color':
      return [c.value[0] ?? 0, c.value[1] ?? 0, c.value[2] ?? 0];
    case 'select':
      return c.str_value ?? c.options[0] ?? '';
    case 'spline':
      return splineControlPoints(c);
  }
};

/** Map an `input_*` control site to a ControlPanel setting, keyed by `key`.
 *  Splines have no panel widget (they're edited in the viewport) — null. */
export const controlToSetting = (c: RenderedControl, key: string): ControlPanelSetting | null => {
  const label = c.label ?? c.handleId;
  switch (c.kind) {
    case 'spline':
      return null;
    case 'bool':
      return { type: 'checkbox', key, label };
    case 'color':
      return { type: 'color', key, label };
    case 'select':
      return { type: 'select', key, label, options: c.options };
    case 'float':
    case 'int': {
      const step = c.step ?? (c.kind === 'int' ? 1 : undefined);
      if (c.min != null && c.max != null && c.style !== 'entry') {
        return { type: 'range', key, label, min: c.min, max: c.max, step };
      }
      return {
        type: 'number',
        key,
        label,
        min: c.min ?? undefined,
        max: c.max ?? undefined,
        step,
      };
    }
  }
};
