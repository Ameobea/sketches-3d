import type { ControlPanelSetting } from 'src/viz/UI/ControlPanel';
import type { RenderedControl } from './runner/types';

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
  }
};

/** Map an `input_*` control site to a ControlPanel setting, keyed by `key`. */
export const controlToSetting = (c: RenderedControl, key: string): ControlPanelSetting => {
  const label = c.label ?? c.handleId;
  switch (c.kind) {
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
