// Action-based undo for the geotoy tree. Source edits live outside this
// system — CodeMirror owns per-node text history. Selection isn't tracked
// either; entries return a `selectAfter` hint where it matters (e.g. undo of
// delete re-selects the restored root).

import type { GizmoValue, Instance, NodeDef, Transform3, TreeDef } from 'src/geoscript/geotoyAPIClient';
import { cloneTransform3 } from 'src/geoscript/geotoyAPIClient';

import { UndoSystem } from 'src/viz/util/undoSystem';
import { createNode as opsCreateNode, deleteNode as opsDeleteNode, reparent as opsReparent } from './treeOps';

export type GeotoyUndoEntry =
  | { type: 'transform'; id: string; instanceId: string; before: Transform3; after: Transform3 }
  | {
      type: 'setHandle';
      nodeId: string;
      handleId: string;
      before: GizmoValue | null;
      after: GizmoValue | null;
    }
  | { type: 'addInstance'; nodeId: string; instance: Instance }
  | { type: 'removeInstance'; nodeId: string; instance: Instance; index: number }
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
      const inst = tree.nodes[entry.id]?.instances.find(i => i.id === entry.instanceId);
      if (!inst) return {};
      Object.assign(inst, cloneTransform3(direction === 'undo' ? entry.before : entry.after));
      return {};
    }
    case 'setHandle': {
      const node = tree.nodes[entry.nodeId];
      if (!node) return {};
      const target = direction === 'undo' ? entry.before : entry.after;
      if (target === null) {
        if (node.handles) {
          delete node.handles[entry.handleId];
          if (Object.keys(node.handles).length === 0) delete node.handles;
        }
      } else {
        (node.handles ??= {})[entry.handleId] = structuredClone(target);
      }
      return {};
    }
    case 'addInstance': {
      const node = tree.nodes[entry.nodeId];
      if (!node) return {};
      if (direction === 'undo') {
        const ix = node.instances.findIndex(i => i.id === entry.instance.id);
        if (ix >= 0 && node.instances.length > 1) node.instances.splice(ix, 1);
      } else if (!node.instances.some(i => i.id === entry.instance.id)) {
        node.instances.push(structuredClone(entry.instance));
      }
      return {};
    }
    case 'removeInstance': {
      const node = tree.nodes[entry.nodeId];
      if (!node) return {};
      if (direction === 'undo') {
        if (!node.instances.some(i => i.id === entry.instance.id)) {
          const at = Math.min(Math.max(entry.index, 0), node.instances.length);
          node.instances.splice(at, 0, structuredClone(entry.instance));
        }
      } else {
        const ix = node.instances.findIndex(i => i.id === entry.instance.id);
        if (ix >= 0 && node.instances.length > 1) node.instances.splice(ix, 1);
      }
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
        parentId: entry.parentId,
        index: entry.index,
      });
      // createNode seeds a single identity instance and no handles/disabled;
      // carry the captured node's full placement state over.
      const created = tree.nodes[def.id];
      created.instances = structuredClone(def.instances);
      if (def.handles) created.handles = structuredClone(def.handles);
      if (def.disabled) created.disabled = true;
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
