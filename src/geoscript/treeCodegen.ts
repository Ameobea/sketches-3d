import * as THREE from 'three';

import type {
  ControlValue,
  GizmoValue,
  ImageLevelsJson,
  NodeDef,
  Transform3,
  TreeDef,
} from './geotoyAPIClient';
import { ROOT_NODE_NAME } from './geotoyAPIClient';
import type { GizmoValuesByModule, GizmoValueWire } from './runner/types';
import { composeTransform3 } from './runner/worldMatrixCache';

/**
 * Compile a `TreeDef` into a set of geoscript module sources plus a root program
 * source that the worker evaluates.
 *
 * One module is emitted per non-disabled node, keyed by the node's name. The
 * emitted source for a node is:
 *
 *   {side-effect imports of each enabled child}
 *   {user's source verbatim}
 *
 * Side-effect imports drive eval ordering: every non-disabled module gets evaluated
 * so its `render()` calls fire. Each rendered mesh carries the owning module's name
 * back to JS, where ancestor tree-transforms are composed at scene populate time.
 *
 * `_root` is the entry point: its emitted source is returned as `rootSource` and
 * stripped from the `modules` map (which goes to `setModuleSources`).
 */
export interface CompiledTree {
  modules: Record<string, string>;
  rootSource: string;
}

/** Namespaces a tree's modules so several can compile into one program without colliding;
 *  omitting `tabId` keeps the bare keys single-tree hosts (level defs) rely on. `:` is safe
 *  as the separator — `NAME_RE` can never match it. */
export const qualifyModuleName = (name: string, tabId?: string): string =>
  tabId ? `${tabId}:${name}` : name;

/** All of a tree's modules including `_root` — the shape a *dependency* tab contributes
 *  to a multi-tab run (its root is imported, not inlined). */
export const compileTreeModules = (tree: TreeDef, tabId?: string): Record<string, string> => {
  const modules: Record<string, string> = {};
  for (const node of Object.values(tree.nodes)) {
    if (node.disabled) continue;
    modules[qualifyModuleName(node.name, tabId)] = buildModuleSource(node, tree, tabId);
  }
  return modules;
};

export const compileTree = (tree: TreeDef, tabId?: string): CompiledTree => {
  const modules = compileTreeModules(tree, tabId);
  const rootKey = qualifyModuleName(ROOT_NODE_NAME, tabId);
  const rootSource = modules[rootKey] ?? '';
  delete modules[rootKey];

  return { modules, rootSource };
};

/** Tab ids referenced by qualified imports (`from "<tabId>:…"`) in the tree's enabled node
 *  sources + `_globals`. Regex-level scan: a false positive only widens the run set. */
export const referencedTabIds = (tree: TreeDef): Set<string> => {
  const out = new Set<string>();
  const scan = (src: string) => {
    for (const m of src.matchAll(/from\s+"([^":]+):/g)) out.add(m[1]);
  };
  for (const node of Object.values(tree.nodes)) {
    if (!node.disabled) scan(node.source);
  }
  scan(tree.globalsSource);
  return out;
};

/**
 * Map from compiled module name → node id, for resolving a rendered mesh's owning node.
 * Covers one tree, so a failure inside a dependency tab's module resolves to nothing — widen
 * it across the run set when cross-tab dependencies land (D20).
 */
export const buildModuleNameToNodeId = (tree: TreeDef, tabId?: string): Record<string, string> => {
  const out: Record<string, string> = {};
  for (const node of Object.values(tree.nodes)) {
    if (!node.disabled) out[qualifyModuleName(node.name, tabId)] = node.id;
  }
  return out;
};

const gizmoValueToWire = (v: GizmoValue): GizmoValueWire => {
  if (v.kind === 'vec3') {
    const a = v.value as [number, number, number];
    return { kind: 'vec3', value: [a[0], a[1], a[2]] };
  }
  const m = composeTransform3(new THREE.Matrix4(), v.value as Transform3);
  return { kind: 'transform', value: Array.from(m.elements) };
};

/** Tree handle values → per-module injection map keyed by module name (matches `compileTree`). */
export const buildGizmoValues = (tree: TreeDef, tabId?: string): GizmoValuesByModule => {
  const out: GizmoValuesByModule = {};
  for (const node of Object.values(tree.nodes)) {
    if (!node.handles) continue;
    const handles: Record<string, GizmoValueWire> = {};
    for (const [id, v] of Object.entries(node.handles)) handles[id] = gizmoValueToWire(v);
    out[qualifyModuleName(node.name, tabId)] = handles;
  }
  return out;
};

export const controlValueToWire = (v: ControlValue): GizmoValueWire => {
  switch (v.kind) {
    case 'float':
      return { kind: 'float', value: [v.value as number] };
    case 'int':
      return { kind: 'int', value: [v.value as number] };
    case 'bool':
      return { kind: 'bool', value: [(v.value as boolean) ? 1 : 0] };
    case 'color': {
      const c = v.value as [number, number, number];
      return { kind: 'color', value: [c[0], c[1], c[2]] };
    }
    case 'select':
      return { kind: 'select', str_value: v.value as string };
    case 'spline':
      return { kind: 'spline', value: (v.value as [number, number, number][]).flat() };
    case 'ramp':
      return { kind: 'ramp', str_value: JSON.stringify(v.value) };
    case 'image_levels': {
      const l = v.value as ImageLevelsJson;
      return { kind: 'image_levels', value: [l.in_lo, l.in_hi, l.out_lo, l.out_hi, l.gamma] };
    }
  }
};

/** All host-injected handle values (gizmos + control inputs) merged per module name. */
export const buildInjectedValues = (tree: TreeDef, tabId?: string): GizmoValuesByModule => {
  const out = buildGizmoValues(tree, tabId);
  for (const node of Object.values(tree.nodes)) {
    if (!node.controls) continue;
    const bucket = (out[qualifyModuleName(node.name, tabId)] ??= {});
    for (const [id, v] of Object.entries(node.controls)) bucket[id] = controlValueToWire(v);
  }
  return out;
};

const buildModuleSource = (node: NodeDef, tree: TreeDef, tabId?: string): string => {
  const sideEffectImports: string[] = [];
  for (const cid of node.children) {
    const child = tree.nodes[cid];
    if (child && !child.disabled) {
      // Generated imports are emitted already-qualified; user source stays untouched and
      // resolves bare names within its own tab.
      sideEffectImports.push(`import { } from "${qualifyModuleName(child.name, tabId)}"`);
    }
  }
  if (sideEffectImports.length === 0) {
    return node.source;
  }
  const sep = node.source.length > 0 ? '\n' : '';
  return sideEffectImports.join('\n') + sep + node.source;
};
