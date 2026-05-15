// Action-based undo for the geotoy tree. Source edits live outside this
// system — CodeMirror owns per-node text history. Selection isn't tracked
// either; entries return a `selectAfter` hint where it matters (e.g. undo of
// delete re-selects the restored root).

import type { NodeDef, Transform3, TreeDef } from 'src/geoscript/geotoyAPIClient';

import { UndoSystem } from 'src/viz/util/undoSystem';
import { createNode as opsCreateNode, deleteNode as opsDeleteNode, reparent as opsReparent } from './treeOps';

export type GeotoyUndoEntry =
  | { type: 'transform'; id: string; before: Transform3; after: Transform3 }
  | { type: 'createNode'; nodeDef: NodeDef; parentId: string; index: number }
  | {
      type: 'deleteSubtree';
      rootId: string;
      nodes: Record<string, NodeDef>;
      parentId: string;
      index: number;
    }
  | {
      type: 'reparent';
      id: string;
      oldParentId: string;
      oldIndex: number;
      newParentId: string;
      newIndex: number;
    };

export type GeotoyUndoSystem = UndoSystem<GeotoyUndoEntry>;
export const buildGeotoyUndoSystem = (): GeotoyUndoSystem => new UndoSystem<GeotoyUndoEntry>();

export const captureSubtreeNodes = (tree: TreeDef, id: string): Record<string, NodeDef> => {
  const out: Record<string, NodeDef> = {};
  const stack = [id];
  while (stack.length > 0) {
    const cur = stack.pop()!;
    const node = tree.nodes[cur];
    if (!node) continue;
    out[cur] = structuredClone(node);
    for (const cid of node.children) stack.push(cid);
  }
  return out;
};

const restoreSubtree = (
  tree: TreeDef,
  rootId: string,
  nodes: Record<string, NodeDef>,
  parentId: string,
  index: number
): void => {
  for (const [id, def] of Object.entries(nodes)) {
    tree.nodes[id] = structuredClone(def);
  }
  const parent = tree.nodes[parentId];
  if (!parent) return;
  // Clamp index in case the parent's children array shrank since capture.
  const at = Math.min(Math.max(index, 0), parent.children.length);
  parent.children.splice(at, 0, rootId);
};

// Stale entries (referencing missing nodes) no-op rather than throw.
// `selectAfter` is a selection hint the caller may honor.
export const applyGeotoyUndoEntry = (
  tree: TreeDef,
  entry: GeotoyUndoEntry,
  direction: 'undo' | 'redo'
): { selectAfter?: string } => {
  switch (entry.type) {
    case 'transform': {
      const node = tree.nodes[entry.id];
      if (!node) return {};
      const t = direction === 'undo' ? entry.before : entry.after;
      node.transform = {
        pos: [t.pos[0], t.pos[1], t.pos[2]],
        rot: [t.rot[0], t.rot[1], t.rot[2]],
        scale: [t.scale[0], t.scale[1], t.scale[2]],
      };
      return {};
    }
    case 'createNode': {
      if (direction === 'undo') {
        if (!tree.nodes[entry.nodeDef.id]) return {};
        opsDeleteNode(tree, entry.nodeDef.id);
        return { selectAfter: tree.nodes[entry.parentId] ? entry.parentId : tree.rootId };
      }
      if (tree.nodes[entry.nodeDef.id]) return {};
      const def = entry.nodeDef;
      opsCreateNode(tree, {
        id: def.id,
        name: def.name,
        source: def.source,
        transform: def.transform,
        parentId: entry.parentId,
        index: entry.index,
      });
      // createNode's opts don't include `disabled`; carry it over here.
      if (def.disabled) tree.nodes[def.id].disabled = true;
      return { selectAfter: def.id };
    }
    case 'deleteSubtree': {
      if (direction === 'undo') {
        if (tree.nodes[entry.rootId]) return {};
        restoreSubtree(tree, entry.rootId, entry.nodes, entry.parentId, entry.index);
        return { selectAfter: entry.rootId };
      }
      if (!tree.nodes[entry.rootId]) return {};
      opsDeleteNode(tree, entry.rootId);
      return { selectAfter: tree.rootId };
    }
    case 'reparent': {
      if (!tree.nodes[entry.id]) return {};
      if (direction === 'undo') {
        opsReparent(tree, entry.id, entry.oldParentId, entry.oldIndex);
      } else {
        opsReparent(tree, entry.id, entry.newParentId, entry.newIndex);
      }
      return {};
    }
  }
};
