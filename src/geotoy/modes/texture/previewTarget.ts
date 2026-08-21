import type { TexturePreviewTarget, TreeDef, TreeKind } from 'src/geoscript/geotoyAPIClient';
import { ROOT_NODE_NAME } from 'src/geoscript/geotoyAPIClient';
import { qualifyModuleName, referencedTabIds } from 'src/geoscript/treeCodegen';
import { buildParentMap } from 'src/geotoy/modules/treeOps';

/** Hidden module synthesized into the active texture tab's namespace to pull the preview
 *  target into the run; its renders are attributed to it. */
export const PREVIEW_MODULE_NAME = '_preview';

export type PreviewTargetProblem = 'missing-tab' | 'missing-node' | 'disabled' | 'cycle';

export const PROBLEM_TEXT: Record<PreviewTargetProblem, string> = {
  'missing-tab': 'preview target tab no longer exists',
  'missing-node': 'preview target node no longer exists',
  disabled: 'preview target node is disabled',
  cycle: 'preview target tab imports this tab',
};

export interface PreviewTargetResolution {
  target: TexturePreviewTarget;
  /** The target tab's tree as of this run; ancestor transforms compose from it. */
  tree: TreeDef;
  nodeName: string;
}

interface ResolveOpts {
  activeTabId: string;
  tabKinds: ReadonlyMap<string, TreeKind>;
  treeFor: (tabId: string) => TreeDef;
}

export const resolvePreviewTarget = (
  target: TexturePreviewTarget,
  { activeTabId, tabKinds, treeFor }: ResolveOpts
): { ok: PreviewTargetResolution } | { problem: PreviewTargetProblem } => {
  if (tabKinds.get(target.tabId) !== 'mesh') return { problem: 'missing-tab' };
  const tree = treeFor(target.tabId);
  const node = tree.nodes[target.nodeId];
  if (!node) return { problem: 'missing-node' };
  const parentMap = buildParentMap(tree);
  for (let cur: string | undefined = target.nodeId; cur; cur = parentMap.get(cur)) {
    if (tree.nodes[cur]?.disabled) return { problem: 'disabled' };
  }
  // The active root is inlined as the entry program, never registered as a module, so a
  // qualified import of it anywhere in the target's closure can't resolve.
  const seen = new Set<string>();
  const stack = [target.tabId];
  while (stack.length) {
    const id = stack.pop()!;
    if (seen.has(id) || !tabKinds.has(id)) continue;
    seen.add(id);
    for (const ref of referencedTabIds(id === target.tabId ? tree : treeFor(id))) {
      if (ref === activeTabId) return { problem: 'cycle' };
      stack.push(ref);
    }
  }
  return { ok: { target, tree, nodeName: node.name } };
};

/** The target tab's whole root is imported (its scene lights plus the subtree's renders);
 *  an export is additionally rendered here. The mesh prelude is prepended worker-side. */
export const buildPreviewModuleSource = ({ target, nodeName }: PreviewTargetResolution): string => {
  const root = `import { } from "${qualifyModuleName(ROOT_NODE_NAME, target.tabId)}"\n`;
  if (!target.exportName) return root;
  const { exportName } = target;
  return `${root}import { ${exportName} } from "${qualifyModuleName(nodeName, target.tabId)}"\n${exportName} | render\n`;
};
