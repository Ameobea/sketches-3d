// Pure functions for mutating a `TreeDef`. Svelte-free so the logic is
// unit-testable under plain node.

import type { GizmoValue, NodeDef, Transform3, TreeDef } from 'src/geoscript/geotoyAPIClient';
import {
  ROOT_NODE_NAME,
  buildEmptyTree,
  buildIdentityTransform,
  buildInstance,
  cloneTransform3,
} from 'src/geoscript/geotoyAPIClient';

const NAME_RE = /^[a-zA-Z_][a-zA-Z0-9_]*$/;
const RESERVED_NAMES: ReadonlySet<string> = new Set([ROOT_NODE_NAME, '_globals']);

export { buildIdentityTransform };

/** Alias of `buildEmptyTree` for callers that want a tree-shaped initializer. */
export const emptyTree = buildEmptyTree;

const isValidName = (name: string): boolean => NAME_RE.test(name) && !RESERVED_NAMES.has(name);

const nameTaken = (tree: TreeDef, name: string, excludingId?: string): boolean => {
  for (const node of Object.values(tree.nodes)) {
    if (node.id !== excludingId && node.name === name) {
      return true;
    }
  }
  return false;
};

/** Returns a unique-in-tree name based on `base`, appending `_2`, `_3`, ... if needed. */
export const uniqueName = (tree: TreeDef, base: string, excludingId?: string): string => {
  const sanitized = isValidName(base) ? base : 'node';
  if (!nameTaken(tree, sanitized, excludingId)) {
    return sanitized;
  }
  for (let i = 2; i < 10000; i++) {
    const candidate = `${sanitized}_${i}`;
    if (!nameTaken(tree, candidate, excludingId)) {
      return candidate;
    }
  }
  throw new Error('uniqueName: could not find a unique name');
};

/**
 * Returns a `childId → parentId` map for the tree in one pass. Useful when many
 * ancestor lookups are needed; `findParentId` is O(N) per call.
 */
export const buildParentMap = (tree: TreeDef): Map<string, string> => {
  const out = new Map<string, string>();
  for (const node of Object.values(tree.nodes)) {
    for (const cid of node.children) {
      out.set(cid, node.id);
    }
  }
  return out;
};

/** Returns the id of the parent of `id`, or null if `id` is the root or absent. */
export const findParentId = (tree: TreeDef, id: string): string | null => {
  for (const node of Object.values(tree.nodes)) {
    if (node.children.includes(id)) {
      return node.id;
    }
  }
  return null;
};

/** True if `candidate` is a strict ancestor of `of` (irreflexive). */
export const isAncestorOf = (tree: TreeDef, candidate: string, of: string): boolean => {
  let cur = findParentId(tree, of);
  const visited = new Set<string>();
  while (cur && !visited.has(cur)) {
    if (cur === candidate) {
      return true;
    }
    visited.add(cur);
    cur = findParentId(tree, cur);
  }
  return false;
};

export interface CreateNodeOpts {
  /** Parent to attach to. `null`/`undefined` defaults to `_root`. */
  parentId?: string | null;
  name?: string;
  source?: string;
  transform?: Transform3;
  /** Insert at this index in the parent's children. Defaults to append. */
  index?: number;
  /** Caller-supplied id (e.g. for deterministic tests). Defaults to a fresh UUID. */
  id?: string;
}

/** Inserts a new node into the tree. Returns the new node's id. Mutates `tree`. */
export const createNode = (tree: TreeDef, opts: CreateNodeOpts = {}): string => {
  const id = opts.id ?? crypto.randomUUID();
  if (tree.nodes[id]) {
    throw new Error(`createNode: id "${id}" already exists`);
  }
  const requestedName = opts.name ?? 'node';
  if (RESERVED_NAMES.has(requestedName)) {
    throw new Error(`createNode: "${requestedName}" is a reserved name`);
  }
  const name = uniqueName(tree, requestedName);
  const node: NodeDef = {
    id,
    name,
    source: opts.source ?? '',
    instances: [buildInstance(opts.transform)],
    children: [],
  };
  tree.nodes[id] = node;

  const parentId = opts.parentId ?? tree.rootId;
  const parent = tree.nodes[parentId];
  if (!parent) {
    throw new Error(`createNode: parent id "${parentId}" not found`);
  }
  const at = opts.index ?? parent.children.length;
  parent.children.splice(at, 0, id);
  return id;
};

/**
 * Removes `id` and its entire subtree from the tree. Mutates `tree`. Refuses to
 * delete `_root` (the tree's invariant root). Deleting a non-existent id is a no-op.
 */
export const deleteNode = (tree: TreeDef, id: string): void => {
  if (id === tree.rootId) {
    throw new Error('deleteNode: the root node cannot be deleted');
  }
  if (!tree.nodes[id]) {
    return;
  }
  const stack: string[] = [id];
  while (stack.length > 0) {
    const cur = stack.pop()!;
    const node = tree.nodes[cur];
    if (!node) continue;
    for (const cid of node.children) {
      stack.push(cid);
    }
    delete tree.nodes[cur];
  }
  for (const parent of Object.values(tree.nodes)) {
    const ix = parent.children.indexOf(id);
    if (ix >= 0) {
      parent.children.splice(ix, 1);
    }
  }
};

/**
 * Moves `id` under `newParentId`. Passing `null` is rejected — every node except
 * `_root` must live somewhere under `_root`. Throws on cycles and on attempts to
 * move `_root` or to use `_root` as the moved subtree.
 */
export const reparent = (tree: TreeDef, id: string, newParentId: string | null, index?: number): void => {
  if (!tree.nodes[id]) {
    throw new Error(`reparent: node "${id}" not found`);
  }
  if (id === tree.rootId) {
    throw new Error('reparent: the root node cannot be reparented');
  }
  const effectiveParentId = newParentId ?? tree.rootId;
  if (!tree.nodes[effectiveParentId]) {
    throw new Error(`reparent: parent "${effectiveParentId}" not found`);
  }
  if (effectiveParentId === id || isAncestorOf(tree, id, effectiveParentId)) {
    throw new Error('reparent: would create a cycle');
  }

  for (const parent of Object.values(tree.nodes)) {
    const ix = parent.children.indexOf(id);
    if (ix >= 0) {
      parent.children.splice(ix, 1);
    }
  }

  const parent = tree.nodes[effectiveParentId];
  const at = index ?? parent.children.length;
  parent.children.splice(at, 0, id);
};

/** Renames a node. Throws if the new name is invalid or collides with another node. */
export const renameNode = (tree: TreeDef, id: string, newName: string): void => {
  const node = tree.nodes[id];
  if (!node) {
    throw new Error(`renameNode: node "${id}" not found`);
  }
  if (id === tree.rootId) {
    throw new Error('renameNode: the root node cannot be renamed');
  }
  if (newName === node.name) {
    return;
  }
  if (!isValidName(newName)) {
    throw new Error(`renameNode: "${newName}" is not a valid module name`);
  }
  if (nameTaken(tree, newName, id)) {
    throw new Error(`renameNode: name "${newName}" is already in use`);
  }
  node.name = newName;
};

/** Writes a placement's transform in place, preserving its id. */
export const setInstanceTransform = (
  tree: TreeDef,
  nodeId: string,
  instanceId: string,
  transform: Transform3
): void => {
  const inst = tree.nodes[nodeId]?.instances.find(i => i.id === instanceId);
  if (!inst) return;
  Object.assign(inst, cloneTransform3(transform));
};

/** Appends a placement (id unique within the node); returns its id, or null for `_root`/missing. */
export const addInstance = (tree: TreeDef, nodeId: string, transform?: Transform3): string | null => {
  const node = tree.nodes[nodeId];
  if (!node || nodeId === tree.rootId) return null;
  const inst = buildInstance(
    transform,
    node.instances.map(i => i.id)
  );
  node.instances.push(inst);
  return inst.id;
};

/** Removes a placement by id. Refuses to remove the last instance or any of `_root`. */
export const removeInstance = (tree: TreeDef, nodeId: string, instanceId: string): void => {
  const node = tree.nodes[nodeId];
  if (!node || nodeId === tree.rootId) return;
  if (node.instances.length <= 1) return;
  const ix = node.instances.findIndex(i => i.id === instanceId);
  if (ix >= 0) node.instances.splice(ix, 1);
};

/** Stores a gizmo handle value on a node (creates the `handles` map if needed). */
export const setHandle = (tree: TreeDef, nodeId: string, handleId: string, value: GizmoValue): void => {
  const node = tree.nodes[nodeId];
  if (!node) return;
  (node.handles ??= {})[handleId] = structuredClone(value);
};

/** Removes a stored handle value; drops the `handles` map when it empties. */
export const deleteHandle = (tree: TreeDef, nodeId: string, handleId: string): void => {
  const handles = tree.nodes[nodeId]?.handles;
  if (!handles) return;
  delete handles[handleId];
  if (Object.keys(handles).length === 0) delete tree.nodes[nodeId].handles;
};

/** Drops stored handle values whose handleId is no longer live (GC of orphans). */
export const pruneHandles = (tree: TreeDef, nodeId: string, liveHandleIds: ReadonlySet<string>): void => {
  const handles = tree.nodes[nodeId]?.handles;
  if (!handles) return;
  for (const id of Object.keys(handles)) {
    if (!liveHandleIds.has(id)) delete handles[id];
  }
  if (Object.keys(handles).length === 0) delete tree.nodes[nodeId].handles;
};

export const setSource = (tree: TreeDef, id: string, source: string): void => {
  const node = tree.nodes[id];
  if (node) {
    node.source = source;
  }
};

export const setDisabled = (tree: TreeDef, id: string, disabled: boolean): void => {
  const node = tree.nodes[id];
  if (!node) return;
  if (id === tree.rootId) return; // _root is never disabled
  if (disabled) {
    node.disabled = true;
  } else {
    delete node.disabled;
  }
};

export const setGlobalsSource = (tree: TreeDef, source: string): void => {
  tree.globalsSource = source;
};

/** Per-node mesh counts summed recursively over each subtree. */
export const computeMeshCounts = (
  tree: TreeDef,
  directCounts: ReadonlyMap<string, number>
): Map<string, number> => {
  const out = new Map<string, number>();
  const visit = (id: string): number => {
    if (out.has(id)) return out.get(id)!;
    const node = tree.nodes[id];
    if (!node) return 0;
    let total = directCounts.get(id) ?? 0;
    for (const cid of node.children) total += visit(cid);
    out.set(id, total);
    return total;
  };
  for (const id of Object.keys(tree.nodes)) visit(id);
  return out;
};

/**
 * Returns `[node, parent, ..., _root]` — the chain of nodes from `nodeId` up to and
 * including the root. Returns `null` if `nodeId` is not in the tree or the chain
 * doesn't terminate at `_root`.
 */
export const getNodeAncestorChain = (tree: TreeDef, nodeId: string): NodeDef[] | null => {
  const parent = buildParentMap(tree); // once, vs findParentId's O(n) per ancestor (hot: per frame)
  const chain: NodeDef[] = [];
  let cur: string | null = nodeId;
  const visited = new Set<string>();
  while (cur && !visited.has(cur)) {
    visited.add(cur);
    const node = tree.nodes[cur];
    if (!node) return null;
    chain.push(node);
    if (cur === tree.rootId) return chain;
    cur = parent.get(cur) ?? null;
  }
  return null;
};
