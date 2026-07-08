import * as THREE from 'three';

import type {
  GizmoValuesByModule,
  GizmoValueWire,
  RenderedControl,
  RenderedGizmo,
} from 'src/geoscript/runner/types';
import { composeTransform3 } from 'src/geoscript/runner/worldMatrixCache';
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
    case 'vec3':
      return { kind: 'vec3', value: [...v.value] };
    case 'transform':
      return {
        kind: 'transform',
        value: Array.from(composeTransform3(new THREE.Matrix4(), v.value).elements),
      };
    case 'spline':
      return { kind: 'spline', value: v.value.flat() };
  }
};

/** `module/handle`-qualified key → parts, when the prefix names a known module. */
export const splitQualifiedInputKey = (
  key: string,
  moduleNames: readonly string[]
): { module: string; handleId: string } | null => {
  const slash = key.indexOf('/');
  if (slash <= 0) return null;
  const module = key.slice(0, slash);
  return moduleNames.includes(module) ? { module, handleId: key.slice(slash + 1) } : null;
};

/**
 * Level-def inputs are addressed by bare name — spread across every named module so whichever
 * module declares the matching `input_*`/`gizmo(...)` picks its value up — or `module/handle`-
 * qualified to target exactly one module. Merges onto (and returns) `base`.
 */
export const injectInputs = (
  base: GizmoValuesByModule,
  inputs: Record<string, InputValueJson> | undefined,
  moduleNames: string[]
): GizmoValuesByModule => {
  if (!inputs || Object.keys(inputs).length === 0) return base;
  for (const [id, v] of Object.entries(inputs)) {
    const wire = reifyInput(v);
    const qualified = splitQualifiedInputKey(id, moduleNames);
    if (qualified) {
      (base[qualified.module] ??= {})[qualified.handleId] = wire;
    } else {
      for (const mod of moduleNames) (base[mod] ??= {})[id] = wire;
    }
  }
  return base;
};

const GIZMO_INPUT_TYPES = new Set(['vec3', 'transform']);

/** Non-fatal sanity check: warn on supplied inputs that don't match a declared control/gizmo. */
export const warnUnmatchedInputs = (
  assetId: string,
  inputs: Record<string, InputValueJson> | undefined,
  controls: RenderedControl[],
  gizmos: RenderedGizmo[] = []
): void => {
  if (!inputs) return;
  const moduleNames = [...new Set(gizmos.map(g => g.sourceModule ?? '_root'))];
  for (const [name, v] of Object.entries(inputs)) {
    const q = splitQualifiedInputKey(name, moduleNames);
    const handleId = q?.handleId ?? name;
    const gizmo = gizmos.find(
      g => g.handleId === handleId && (!q || (g.sourceModule ?? '_root') === q.module)
    );
    if (GIZMO_INPUT_TYPES.has(v.type)) {
      if (!gizmo) {
        console.warn(`[levelDef] asset "${assetId}": input "${name}" matches no gizmo handle`);
      } else if (gizmo.kind !== v.type) {
        console.warn(
          `[levelDef] asset "${assetId}": input "${name}" is type "${v.type}" but the gizmo is "${gizmo.kind}"`
        );
      }
      continue;
    }
    const c = controls.find(ctl => ctl.handleId === handleId);
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
