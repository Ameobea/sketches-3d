/**
 * Inverse of the flattening `sync-geo-compositions.ts` writes to `geo_compositions/*.geo`.
 * `renderGeo(parseGeoFile(text)) === text` for anything the sync script emits, and
 * `applyToDoc(doc, parseGeoFile(renderGeo(parsedFromDoc(id, title, doc))))` deep-equals `doc`.
 */
import type { CompositionDoc, NodeDef, TreeDef, TreeKind } from '../src/geoscript/geotoyAPIClient';

export interface ParsedNode {
  name: string;
  source: string;
}

export interface ParsedTree {
  name: string;
  kind: TreeKind;
  /** `null` when the file has no globals section (the sync omits whitespace-only globals). */
  globalsSource: string | null;
  nodes: ParsedNode[];
}

export interface ParsedGeo {
  id: number;
  title: string;
  trees: ParsedTree[];
}

const HEADER_RE = /^\/\/ Composition (\d+): (.*)$/;
const TREE_RE = /^\/\/ ==== tree: (.+) \((mesh|texture)\) ====$/;
const NODE_RE = /^\/\/ === node: (.+) ===$/;
const GLOBALS_LINE = '// === globals ===';

const newTree = (name: string, kind: TreeKind): ParsedTree => ({
  name,
  kind,
  globalsSource: null,
  nodes: [],
});

export const parseGeoFile = (text: string): ParsedGeo => {
  const lines = text.split('\n');
  if (lines.at(-1) === '') {
    lines.pop();
  }
  const header = HEADER_RE.exec(lines[0] ?? '');
  if (!header) {
    throw new Error('missing `// Composition <id>: <title>` header');
  }

  const trees: ParsedTree[] = [];
  let tree: ParsedTree | undefined;
  let section: { name: string | null; lines: string[] } | undefined;
  const close = () => {
    if (section && tree) {
      const source = section.lines.join('\n');
      if (section.name === null) {
        tree.globalsSource = source;
      } else {
        tree.nodes.push({ name: section.name, source });
      }
    }
    section = undefined;
  };

  for (const line of lines.slice(1)) {
    const treeM = TREE_RE.exec(line);
    const nodeM = treeM ? null : NODE_RE.exec(line);
    if (treeM || nodeM || line === GLOBALS_LINE) {
      close();
      if (treeM) {
        tree = newTree(treeM[1], treeM[2] as TreeKind);
        trees.push(tree);
        continue;
      }
      if (!tree) {
        tree = newTree('main', 'mesh');
        trees.push(tree);
      }
      section = { name: nodeM ? nodeM[1] : null, lines: [] };
    } else if (section) {
      section.lines.push(line);
    } else {
      throw new Error(`line outside any section: ${JSON.stringify(line)}`);
    }
  }
  close();
  return { id: Number(header[1]), title: header[2], trees };
};

/** Root first, then depth-first through children; unreachable nodes trail in id order. */
const orderNodes = (tree: TreeDef): NodeDef[] => {
  const ordered: NodeDef[] = [];
  const seen = new Set<string>();
  const visit = (id: string) => {
    const node = tree.nodes[id];
    if (!node || seen.has(id)) {
      return;
    }
    seen.add(id);
    ordered.push(node);
    node.children.forEach(visit);
  };
  visit(tree.rootId);
  Object.keys(tree.nodes).sort().forEach(visit);
  return ordered;
};

export const parsedFromDoc = (id: number, title: string, doc: CompositionDoc): ParsedGeo => ({
  id,
  title,
  trees: doc.trees.map(({ name, kind, tree }) => ({
    name,
    kind,
    globalsSource: tree.globalsSource.trim() ? tree.globalsSource : null,
    nodes: orderNodes(tree).map(({ name, source }) => ({ name, source })),
  })),
});

const renderTreeSections = (tree: ParsedTree): string[] => {
  const sections = tree.nodes.map(node => `// === node: ${node.name} ===\n${node.source}`);
  if (tree.globalsSource?.trim()) {
    sections.unshift(`// === globals ===\n${tree.globalsSource}`);
  }
  return sections;
};

export const renderGeo = ({ id, title, trees }: ParsedGeo): string => {
  const sections =
    trees.length === 1
      ? renderTreeSections(trees[0])
      : trees.flatMap(tree => [
          `// ==== tree: ${tree.name} (${tree.kind}) ====`,
          ...renderTreeSections(tree),
        ]);
  return `// Composition ${id}: ${title}\n${sections.join('\n')}\n`;
};

/**
 * Deep-copies `doc` with node sources + globals replaced from `parsed`. Trees match by name
 * (a single-tree file always targets a single-tree doc's only tree); nodes match by name in
 * sync order, so duplicate names pair up positionally. Nodes/trees absent from the file are
 * left untouched; ones absent from the doc throw.
 */
export const applyToDoc = (doc: CompositionDoc, parsed: ParsedGeo): CompositionDoc => {
  const out = structuredClone(doc);
  for (const pt of parsed.trees) {
    const entry =
      out.trees.length === 1 && parsed.trees.length === 1
        ? out.trees[0]
        : out.trees.find(t => t.name === pt.name);
    if (!entry) {
      throw new Error(`tree \`${pt.name}\` not in composition ${parsed.id}`);
    }
    const queues = new Map<string, NodeDef[]>();
    for (const node of orderNodes(entry.tree)) {
      queues.set(node.name, [...(queues.get(node.name) ?? []), node]);
    }
    for (const { name, source } of pt.nodes) {
      const node = queues.get(name)?.shift();
      if (!node) {
        throw new Error(`node \`${name}\` not in tree \`${entry.name}\` of composition ${parsed.id}`);
      }
      node.source = source;
    }
    if (pt.globalsSource !== null) {
      entry.tree.globalsSource = pt.globalsSource;
    } else if (entry.tree.globalsSource.trim()) {
      entry.tree.globalsSource = '';
    }
  }
  return out;
};
