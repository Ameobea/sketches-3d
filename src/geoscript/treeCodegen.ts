import type { NodeDef, TreeDef } from './geotoyAPIClient';
import { ROOT_NODE_NAME } from './geotoyAPIClient';

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
