import type { GizmoValuesByModule, GizmoValueWire, RenderedControl } from 'src/geoscript/runner/types';
import type { InputValueJson } from './types';

const reifyColor = (value: [number, number, number] | number | string): [number, number, number] => {
  if (Array.isArray(value)) return value;
  const int = typeof value === 'number' ? value : parseInt(value.slice(1), 16);
  return [((int >> 16) & 255) / 255, ((int >> 8) & 255) / 255, (int & 255) / 255];
};

export const reifyInput = (v: InputValueJson): GizmoValueWire => {
  switch (v.type) {
    case 'float':
      return { kind: 'float', value: [v.value] };
    case 'int':
      return { kind: 'int', value: [v.value] };
    case 'bool':
      return { kind: 'bool', value: [v.value ? 1 : 0] };
    case 'color':
      return { kind: 'color', value: reifyColor(v.value) };
    case 'select':
      return { kind: 'select', str_value: v.value };
  }
};

/**
 * Level-def inputs are addressed by bare name; spread them across every named module so whichever
 * module declares the matching `input_*` picks its value up. Merges onto (and returns) `base`.
 */
export const injectInputs = (
  base: GizmoValuesByModule,
  inputs: Record<string, InputValueJson> | undefined,
  moduleNames: string[]
): GizmoValuesByModule => {
  if (!inputs || Object.keys(inputs).length === 0) return base;
  const reified: Record<string, GizmoValueWire> = {};
  for (const [id, v] of Object.entries(inputs)) reified[id] = reifyInput(v);
  for (const mod of moduleNames) base[mod] = { ...(base[mod] ?? {}), ...reified };
  return base;
};

/** Non-fatal sanity check: warn on supplied inputs that don't match a declared control by name/type. */
export const warnUnmatchedInputs = (
  assetId: string,
  inputs: Record<string, InputValueJson> | undefined,
  controls: RenderedControl[]
): void => {
  if (!inputs) return;
  const byId = new Map(controls.map(c => [c.handleId, c]));
  for (const [name, v] of Object.entries(inputs)) {
    const c = byId.get(name);
    if (!c) {
      console.warn(
        `[levelDef] asset "${assetId}": input "${name}" matches no input_* control in the program`
      );
    } else if (c.kind !== v.type) {
      console.warn(
        `[levelDef] asset "${assetId}": input "${name}" is type "${v.type}" but the control is "${c.kind}"`
      );
    }
  }
};
