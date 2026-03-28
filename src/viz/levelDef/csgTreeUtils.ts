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
    node = node.children[Number(p)];
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
    parent = parent.children[Number(parts[i])];
  }
  if (!isOpNode(parent)) throw new Error('Invalid path');
  parent.children[Number(parts[parts.length - 1])] = value;
  return newRoot;
};

export const deleteAtPath = (root: CsgTreeNode, path: string): CsgTreeNode | null => {
  if (!path) return null;
  const parts = path.split('.');
  const parentPath = parts.slice(0, -1).join('.');
  const index = Number(parts[parts.length - 1]);

  const newRoot = cloneTree(root);
  const parentNode = parentPath ? getNodeAtPath(newRoot, parentPath) : newRoot;
  if (!isOpNode(parentNode)) return root;

  if (parentNode.children.length > 2) {
    // More than 2 children — just splice out the deleted child
    parentNode.children.splice(index, 1);
    return newRoot;
  }

  // Exactly 2 children — collapse: promote sibling to replace the parent
  const siblingIndex = index === 0 ? 1 : 0;
  const sibling = parentNode.children[siblingIndex];

  if (!parentPath) return sibling;
  return setNodeAtPath(root, parentPath, sibling);
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
