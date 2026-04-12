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

/**
 * Removes a node by id from the tree. Returns the updated array and whether the node was found.
 * Operates immutably: returns new arrays rather than mutating in place.
 */
export const removeNodeById = (
  nodes: (ObjectDef | ObjectGroupDef)[],
  id: string
): { removed: boolean; nodes: (ObjectDef | ObjectGroupDef)[] } => {
  const idx = nodes.findIndex(n => n.id === id);
  if (idx !== -1) {
    const next = [...nodes];
    next.splice(idx, 1);
    return { removed: true, nodes: next };
  }
  for (let i = 0; i < nodes.length; i++) {
    const node = nodes[i];
    if (isObjectGroup(node)) {
      const res = removeNodeById(node.children, id);
      if (res.removed) {
        const next = [...nodes];
        next[i] = { ...node, children: res.nodes };
        return { removed: true, nodes: next };
      }
    }
  }
  return { removed: false, nodes };
};

/**
 * Find the parent array containing a node with the given id, along with its
 * index within that array. Returns null if not found.
 */
export const findNodeWithParent = (
  nodes: (ObjectDef | ObjectGroupDef)[],
  id: string
): {
  node: ObjectDef | ObjectGroupDef;
  parentArray: (ObjectDef | ObjectGroupDef)[];
  index: number;
} | null => {
  const idx = nodes.findIndex(n => n.id === id);
  if (idx !== -1) return { node: nodes[idx], parentArray: nodes, index: idx };
  for (const node of nodes) {
    if (isObjectGroup(node)) {
      const result = findNodeWithParent(node.children, id);
      if (result) return result;
    }
  }
  return null;
};
