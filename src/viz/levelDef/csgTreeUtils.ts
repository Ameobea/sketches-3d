import type { CsgTreeNode, CsgLeafNode, CsgOpNode } from './types';

export const isOpNode = (node: CsgTreeNode): node is CsgOpNode => 'op' in node;
export const isLeafNode = (node: CsgTreeNode): node is CsgLeafNode => 'asset' in node;

export const cloneTree = (node: CsgTreeNode): CsgTreeNode => JSON.parse(JSON.stringify(node));

export const getNodeAtPath = (root: CsgTreeNode, path: string): CsgTreeNode => {
  if (!path) return root;
  const parts = path.split('.');
  let node = root;
  for (const p of parts) {
    if (!isOpNode(node)) throw new Error('Invalid path');
    node = p === 'a' ? node.a : node.b;
  }
  return node;
};

export const setNodeAtPath = (root: CsgTreeNode, path: string, value: CsgTreeNode): CsgTreeNode => {
  if (!path) return value;
  const newRoot = cloneTree(root);
  const parts = path.split('.');
  let parent = newRoot;
  for (let i = 0; i < parts.length - 1; i++) {
    if (!isOpNode(parent)) throw new Error('Invalid path');
    parent = parts[i] === 'a' ? parent.a : parent.b;
  }
  if (!isOpNode(parent)) throw new Error('Invalid path');
  parent[parts[parts.length - 1] as 'a' | 'b'] = value;
  return newRoot;
};

export const deleteAtPath = (root: CsgTreeNode, path: string): CsgTreeNode | null => {
  if (!path) return null;
  const parts = path.split('.');
  const parentPath = parts.slice(0, -1).join('.');
  const side = parts[parts.length - 1] as 'a' | 'b';
  const siblingSide = side === 'a' ? 'b' : 'a';

  const parentNode = getNodeAtPath(cloneTree(root), parentPath);
  if (!isOpNode(parentNode)) return root;
  const sibling = parentNode[siblingSide];

  if (!parentPath) return sibling;
  return setNodeAtPath(root, parentPath, sibling);
};

/**
 * Compute polarity for every node in the tree.
 * Root is positive. Union/intersection: children inherit. Difference: a inherits, b flips.
 */
export const computeNodePolarities = (tree: CsgTreeNode): Map<string, 'positive' | 'negative'> => {
  const out = new Map<string, 'positive' | 'negative'>();

  const walk = (node: CsgTreeNode, polarity: 'positive' | 'negative', path: string) => {
    out.set(path, polarity);

    if (isOpNode(node)) {
      const aPath = path ? `${path}.a` : 'a';
      const bPath = path ? `${path}.b` : 'b';
      const bPolarity =
        node.op === 'difference' ? (polarity === 'positive' ? 'negative' : 'positive') : polarity;
      walk(node.a, polarity, aPath);
      walk(node.b, bPolarity, bPath);
    }
  };

  walk(tree, 'positive', '');
  return out;
};

/** Collect paths of all leaf nodes in the tree. */
export const collectLeafPaths = (tree: CsgTreeNode): string[] => {
  const paths: string[] = [];
  const walk = (node: CsgTreeNode, path: string) => {
    if (isLeafNode(node)) {
      paths.push(path);
    } else {
      walk(node.a, path ? `${path}.a` : 'a');
      walk(node.b, path ? `${path}.b` : 'b');
    }
  };
  walk(tree, '');
  return paths;
};
