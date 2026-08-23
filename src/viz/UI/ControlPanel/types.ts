export type Rgb = [number, number, number];

/** A row-level action surfaced behind the row's `⋯` menu. */
export interface SettingAction {
  label: string;
  action: () => void;
  disabled?: boolean;
  title?: string;
}

interface BaseSetting {
  /** Display text. Doubles as the state key unless `key` is set, so must be unique then. */
  label: string;
  /** Explicit state key; defaults to `label`. Use when the display label isn't unique/stable. */
  key?: string;
  /** Marks the label with a dot (e.g. "holds a stored override"). */
  modified?: boolean;
  actions?: SettingAction[];
}

export interface RangeSetting extends BaseSetting {
  type: 'range';
  min: number;
  max: number;
  step?: number;
  /** Integer subdivision count; mutually exclusive with `step`. */
  steps?: number;
  scale?: 'log';
  initial?: number;
}

export interface NumberSetting extends BaseSetting {
  type: 'number';
  min?: number;
  max?: number;
  step?: number;
  initial?: number;
}

export interface CheckboxSetting extends BaseSetting {
  type: 'checkbox';
  initial?: boolean;
}

export interface SelectSetting extends BaseSetting {
  type: 'select';
  options: string[] | Record<string, unknown>;
  initial?: unknown;
}

export interface ColorSetting extends BaseSetting {
  type: 'color';
  initial?: Rgb;
}

export interface TextSetting extends BaseSetting {
  type: 'text';
  initial?: string;
}

export interface ButtonSetting extends BaseSetting {
  type: 'button';
  action: () => void;
}

export type ControlPanelSetting =
  | RangeSetting
  | NumberSetting
  | CheckboxSetting
  | SelectSetting
  | ColorSetting
  | TextSetting
  | ButtonSetting;

export type ControlPanelState = Record<string, any>;
