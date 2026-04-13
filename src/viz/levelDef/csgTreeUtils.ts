import type { CsgTreeNode, CsgLeafNode, CsgOpNode } from './types';

export const isOpNode = (node: CsgTreeNode): node is CsgOpNode => 'op' in node;
export const isLeafNode = (node: CsgTreeNode): node is CsgLeafNode => 'asset' in node;

export const cloneTree = (node: CsgTreeNode): CsgTreeNode => JSON.parse(JSON.stringify(node));

/** Get the parent path and child index from a full path. */
export const splitPath = (path: string): { parentPath: string; childIndex: number } | null => {
  if (!path) return null;
  const parts = path.split('.');
  return {
    parentPath: parts.slice(0, -1).join('.'),
    childIndex: Number(parts[parts.length - 1]),
  };
};

export const getNodeAtPath = (root: CsgTreeNode, path: string): CsgTreeNode => {
  if (!path) return root;
  const parts = path.split('.');
  let node = root;
  for (const p of parts) {
    if (!isOpNode(node)) throw new Error('Invalid path');
    node = node.children[Number(p)];
  }
  return node;
};

export const setNodeAtPath = (root: CsgTreeNode, path: string, value: CsgTreeNode): CsgTreeNode => {
  if (!path) return value;
  const info = splitPath(path);
  if (!info) return value;
  const newRoot = cloneTree(root);
  const parent = info.parentPath ? getNodeAtPath(newRoot, info.parentPath) : newRoot;
  if (!isOpNode(parent)) throw new Error('Invalid path');
  parent.children[info.childIndex] = value;
  return newRoot;
};

export const insertAfterPath = (root: CsgTreeNode, path: string, newNode: CsgTreeNode): CsgTreeNode => {
  const info = splitPath(path);
  if (!info) {
    // If root is selected, wrap it in a new union with the new node
    return {
      op: 'union',
      children: [cloneTree(root), newNode],
    };
  }

  const newRoot = cloneTree(root);
  const parentNode = info.parentPath ? getNodeAtPath(newRoot, info.parentPath) : newRoot;
  if (!isOpNode(parentNode)) return root;

  parentNode.children.splice(info.childIndex + 1, 0, newNode);
  return newRoot;
};

export const deleteAtPath = (root: CsgTreeNode, path: string): CsgTreeNode | null => {
  const info = splitPath(path);
  if (!info) return null;

  const newRoot = cloneTree(root);
  const parentNode = info.parentPath ? getNodeAtPath(newRoot, info.parentPath) : newRoot;
  if (!isOpNode(parentNode)) return root;

  if (parentNode.children.length > 2) {
    // More than 2 children — just splice out the deleted child
    parentNode.children.splice(info.childIndex, 1);
    return newRoot;
  }

  // Exactly 2 children — collapse: promote sibling to replace the parent
  const siblingIndex = info.childIndex === 0 ? 1 : 0;
  const sibling = parentNode.children[siblingIndex];

  if (!info.parentPath) return sibling;
  return setNodeAtPath(root, info.parentPath, sibling);
};

/**
 * Compute polarity for every node in the tree.
 * Root is positive. Union/intersection: children inherit. Difference: first child inherits, rest flip.
 */
export const computeNodePolarities = (tree: CsgTreeNode): Map<string, 'positive' | 'negative'> => {
  const out = new Map<string, 'positive' | 'negative'>();

  const walk = (node: CsgTreeNode, polarity: 'positive' | 'negative', path: string) => {
    out.set(path, polarity);

    if (isOpNode(node)) {
      for (let i = 0; i < node.children.length; i++) {
        const childPath = path ? `${path}.${i}` : `${i}`;
        let childPolarity = polarity;
        if (node.op === 'difference' && i > 0) {
          childPolarity = polarity === 'positive' ? 'negative' : 'positive';
        }
        walk(node.children[i], childPolarity, childPath);
      }
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
      for (let i = 0; i < node.children.length; i++) {
        walk(node.children[i], path ? `${path}.${i}` : `${i}`);
      }
    }
  };
  walk(tree, '');
  return paths;
};
