import * as THREE from 'three';

import type { ControlValue, GizmoValue, NodeDef, Transform3, TreeDef } from './geotoyAPIClient';
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

export const compileTree = (tree: TreeDef): CompiledTree => {
  const modules: Record<string, string> = {};
  for (const node of Object.values(tree.nodes)) {
    if (node.disabled) continue;
    modules[node.name] = buildModuleSource(node, tree);
  }

  const rootSource = modules[ROOT_NODE_NAME] ?? '';
  delete modules[ROOT_NODE_NAME];

  return { modules, rootSource };
};

/** Map from compiled module name → node id, for resolving a rendered mesh's owning node. */
export const buildModuleNameToNodeId = (tree: TreeDef): Record<string, string> => {
  const out: Record<string, string> = {};
  for (const node of Object.values(tree.nodes)) {
    if (!node.disabled) out[node.name] = node.id;
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

/** Tree handle values → per-module injection map keyed by node name (matches `compileTree`). */
export const buildGizmoValues = (tree: TreeDef): GizmoValuesByModule => {
  const out: GizmoValuesByModule = {};
  for (const node of Object.values(tree.nodes)) {
    if (!node.handles) continue;
    const handles: Record<string, GizmoValueWire> = {};
    for (const [id, v] of Object.entries(node.handles)) handles[id] = gizmoValueToWire(v);
    out[node.name] = handles;
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
  }
};

/** All host-injected handle values (gizmos + control inputs) merged per module name. */
export const buildInjectedValues = (tree: TreeDef): GizmoValuesByModule => {
  const out = buildGizmoValues(tree);
  for (const node of Object.values(tree.nodes)) {
    if (!node.controls) continue;
    const bucket = (out[node.name] ??= {});
    for (const [id, v] of Object.entries(node.controls)) bucket[id] = controlValueToWire(v);
  }
  return out;
};

const buildModuleSource = (node: NodeDef, tree: TreeDef): string => {
  const sideEffectImports: string[] = [];
  for (const cid of node.children) {
    const child = tree.nodes[cid];
    if (child && !child.disabled) {
      sideEffectImports.push(`import { } from "${child.name}"`);
    }
  }
  if (sideEffectImports.length === 0) {
    return node.source;
  }
  const sep = node.source.length > 0 ? '\n' : '';
  return sideEffectImports.join('\n') + sep + node.source;
};
