import type { ObjectDef, ObjectGroupDef } from './types';

export const GENERATED_NODE_USERDATA_KEY = '__generated';

export const isObjectGroup = (n: ObjectDef | ObjectGroupDef): n is ObjectGroupDef => 'children' in n;

export const isGeneratedDef = (n: ObjectDef | ObjectGroupDef) =>
  n.userData?.[GENERATED_NODE_USERDATA_KEY] === true;

/** Recursive DFS search by id. Returns null if not found. */
export const findNodeById = (
  nodes: (ObjectDef | ObjectGroupDef)[],
  id: string
): ObjectDef | ObjectGroupDef | null => {
  for (const node of nodes) {
    if (node.id === id) return node;
    if (isObjectGroup(node)) {
      const found = findNodeById(node.children, id);
      if (found) return found;
    }
  }
  return null;
};

/** Immutable update of a node's transform fields. Returns a new array. */
export const updateNodeTransform = (
  nodes: (ObjectDef | ObjectGroupDef)[],
  id: string,
  snap: {
    position?: [number, number, number];
    rotation?: [number, number, number];
    scale?: [number, number, number];
  }
): (ObjectDef | ObjectGroupDef)[] =>
  nodes.map(node => {
    if (node.id === id) {
      return { ...node, ...snap };
    }
    if (isObjectGroup(node)) {
      return { ...node, children: updateNodeTransform(node.children, id, snap) };
    }
    return node;
  });

/** Collect all leaf ObjectDefs from the tree (flattened). */
export const flattenLeaves = (nodes: (ObjectDef | ObjectGroupDef)[]): ObjectDef[] => {
  const result: ObjectDef[] = [];
  for (const node of nodes) {
    if (isObjectGroup(node)) {
      result.push(...flattenLeaves(node.children));
    } else {
      result.push(node);
    }
  }
  return result;
};

/** Collect every node (groups and leaves) from the tree (flattened). */
export const flattenAllNodes = (nodes: (ObjectDef | ObjectGroupDef)[]): (ObjectDef | ObjectGroupDef)[] => {
  const result: (ObjectDef | ObjectGroupDef)[] = [];
  for (const node of nodes) {
    result.push(node);
    if (isObjectGroup(node)) {
      result.push(...flattenAllNodes(node.children));
    }
  }
  return result;
};
