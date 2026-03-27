import * as THREE from 'three';

import type { AssetDef, CsgAssetDef, CsgTreeNode } from './types';
import { getNodeAtPath, deleteAtPath } from './csgTreeUtils';

export interface CsgCodeGenResult {
  /** Module bindings: asset key → geoscript source with `export mesh` */
  modules: Record<string, string>;
  /** The main CSG program to execute (exports mesh, does NOT render) */
  code: string;
}

const isDefaultVec3 = (v: [number, number, number] | undefined, def: [number, number, number]): boolean => {
  if (!v) return true;
  return v[0] === def[0] && v[1] === def[1] && v[2] === def[2];
};

const isIdentityTransform = (node: {
  position?: [number, number, number];
  rotation?: [number, number, number];
  scale?: [number, number, number];
}): boolean =>
  isDefaultVec3(node.position, [0, 0, 0]) &&
  isDefaultVec3(node.rotation, [0, 0, 0]) &&
  isDefaultVec3(node.scale, [1, 1, 1]);

/**
 * Build a geoscript `apply_mat4(...)` call that applies the correct TRS matrix.
 * We compute the matrix on the JS side using Three.js (which composes T * R * S
 * with YXZ Euler order) and emit it as 16 row-major floats. This avoids all
 * convention mismatches between Three.js and geoscript's individual transform ops.
 */
const buildTransformExpr = (
  varName: string,
  node: {
    position?: [number, number, number];
    rotation?: [number, number, number];
    scale?: [number, number, number];
  }
): string => {
  if (isIdentityTransform(node)) return varName;

  const [px = 0, py = 0, pz = 0] = node.position ?? [];
  const [rx = 0, ry = 0, rz = 0] = node.rotation ?? [];
  const [sx = 1, sy = 1, sz = 1] = node.scale ?? [];

  const mat = new THREE.Matrix4();
  mat.compose(
    new THREE.Vector3(px, py, pz),
    new THREE.Quaternion().setFromEuler(new THREE.Euler(rx, ry, rz, 'YXZ')),
    new THREE.Vector3(sx, sy, sz)
  );

  // Three.js Matrix4.elements is column-major. nalgebra Matrix4::new() takes
  // row-major args, so we read elements in row-major order.
  const e = mat.elements;
  const r = (n: number) => Math.round(n * 1e7) / 1e7;
  // elements[col*4+row]: e[0]=m00 e[1]=m10 e[2]=m20 e[3]=m30
  //                      e[4]=m01 e[5]=m11 e[6]=m21 e[7]=m31 ...
  const row0 = `${r(e[0])}, ${r(e[4])}, ${r(e[8])}, ${r(e[12])}`;
  const row1 = `${r(e[1])}, ${r(e[5])}, ${r(e[9])}, ${r(e[13])}`;
  const row2 = `${r(e[2])}, ${r(e[6])}, ${r(e[10])}, ${r(e[14])}`;
  const row3 = `${r(e[3])}, ${r(e[7])}, ${r(e[11])}, ${r(e[15])}`;

  return `${varName} | apply_mat4(${row0}, ${row1}, ${row2}, ${row3})`;
};

interface TreeGenState {
  imports: Map<string, string>; // asset key → import var name
  importCounter: number;
  lets: string[];
  letCounter: number;
}

const generateNode = (node: CsgTreeNode, state: TreeGenState): string => {
  if ('asset' in node) {
    // Leaf node — get or create import binding
    let importVar = state.imports.get(node.asset);
    if (!importVar) {
      importVar = `__csg_${state.importCounter++}`;
      state.imports.set(node.asset, importVar);
    }

    // Apply leaf transform
    const varName = `__t_${state.letCounter++}`;
    const expr = buildTransformExpr(importVar, node);
    state.lets.push(`${varName} = ${expr}`);
    return varName;
  }

  // Op node
  const leftVar = generateNode(node.a, state);
  const rightVar = generateNode(node.b, state);

  const opFn = node.op === 'union' ? 'union' : node.op === 'difference' ? 'difference' : 'intersect';

  const opVarName = `__t_${state.letCounter++}`;
  const opExpr = `${opFn}(${leftVar}, ${rightVar})`;

  // Apply op node transform if present
  state.lets.push(`${opVarName} = ${buildTransformExpr(opExpr, node)}`);

  return opVarName;
};

const collectModules = (state: TreeGenState, allAssets: Record<string, AssetDef>): Record<string, string> => {
  const modules: Record<string, string> = {};
  for (const [assetKey] of state.imports) {
    const assetDef = allAssets[assetKey];
    if (!assetDef) {
      throw new Error(`CSG references unknown asset "${assetKey}"`);
    }
    if (assetDef.type === 'geoscript') {
      modules[assetKey] = assetDef.code;
    } else if (assetDef.type === 'csg') {
      const inner = generateCsgCode(assetDef as CsgAssetDef, allAssets);
      modules[assetKey] = inner.code;
      Object.assign(modules, inner.modules);
    } else {
      throw new Error(`CSG references non-script asset "${assetKey}" (type: ${assetDef.type})`);
    }
  }
  return modules;
};

const assembleProgram = (state: TreeGenState, resultVar: string): string => {
  const importLines: string[] = [];
  for (const [assetKey, varName] of state.imports) {
    importLines.push(`import { mesh: ${varName} } from "${assetKey}"`);
  }
  return [...importLines, '', ...state.lets, `export mesh = ${resultVar}`].join('\n');
};

/**
 * Generate a CSG program from a CSG asset definition.
 *
 * All referenced assets (geoscript or other CSG) are expected to `export mesh`.
 * The generated program imports each referenced asset's mesh, applies transforms
 * and boolean ops, and exports the result as `mesh`. It does NOT call `render` —
 * that is the caller's responsibility (via the loader's render wrapper).
 */
export const generateCsgCode = (
  csgAssetDef: CsgAssetDef,
  allAssets: Record<string, AssetDef>
): CsgCodeGenResult => {
  const state: TreeGenState = {
    imports: new Map(),
    importCounter: 0,
    lets: [],
    letCounter: 0,
  };

  const resultVar = generateNode(csgAssetDef.tree, state);
  const modules = collectModules(state, allAssets);
  const code = assembleProgram(state, resultVar);

  return { modules, code };
};

/**
 * Generate code for the CSG tree with a subtree excluded (complement).
 * Used by the editor to render "everything except the selected subtree" when a
 * positive-polarity node is selected, avoiding z-fighting with the selection preview.
 * Returns null if the excluded path is root (nothing left to render).
 */
export const generateComplementCode = (
  csgAssetDef: CsgAssetDef,
  excludePath: string,
  allAssets: Record<string, AssetDef>
): CsgCodeGenResult | null => {
  const trimmedTree = deleteAtPath(csgAssetDef.tree, excludePath);
  if (!trimmedTree) return null;
  return generateCsgCode({ type: 'csg', tree: trimmedTree } as CsgAssetDef, allAssets);
};

/**
 * Generate code for a subtree of a CSG asset (used for op node previews in the editor).
 * Returns the same format as generateCsgCode but only processes the subtree at the given path.
 * The root node's own transform is always stripped — the editor applies it externally
 * so that TransformControls can manipulate it independently of ancestor transforms.
 */
export const generateSubtreeCode = (
  csgAssetDef: CsgAssetDef,
  subtreePath: string,
  allAssets: Record<string, AssetDef>
): CsgCodeGenResult => {
  const subtree = getNodeAtPath(csgAssetDef.tree, subtreePath);
  const subtreeWithoutTransform = { ...subtree, position: undefined, rotation: undefined, scale: undefined };
  return generateCsgCode({ type: 'csg', tree: subtreeWithoutTransform } as CsgAssetDef, allAssets);
};
