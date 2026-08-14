import type { TreeKind } from 'src/geoscript/geotoyAPIClient';
import type { TreeState } from 'src/geotoy/modules/treeState.svelte';

const preludeBlock = (prelude: string) => `${prelude}\n//-- end prelude\n\n`;

interface TogglePreludeOpts {
  treeState: TreeState;
  getPrelude: (kind: TreeKind) => Promise<string>;
  kind: TreeKind;
  ejected: boolean;
  setEjected: (ejected: boolean) => void;
}

/**
 * Move the prelude between "implicit" and "literal text in the tree's root".
 *
 * The root is where the implicit copy was prepended (it's the entry program), so ejecting
 * there is what makes un-ejecting unambiguous. `rewriteSource` bumps `contentEpoch`, so an
 * open editor re-syncs itself. Un-eject strips only an exact prefix match — any edit of the
 * pasted text, whitespace included, is left for the user to remove by hand.
 */
export const togglePreludeEjected = async ({
  treeState,
  getPrelude,
  kind,
  ejected,
  setEjected,
}: TogglePreludeOpts): Promise<void> => {
  const prelude = await getPrelude(kind);
  const rootId = treeState.state.tree.rootId;
  const cur = treeState.state.tree.nodes[rootId]?.source ?? '';

  if (!ejected) {
    treeState.rewriteSource(rootId, preludeBlock(prelude) + cur);
    treeState.setSelected(rootId);
  } else if (cur.startsWith(preludeBlock(prelude))) {
    treeState.rewriteSource(rootId, cur.slice(preludeBlock(prelude).length));
  }
  setEjected(!ejected);
};
