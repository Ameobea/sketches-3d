import { syntaxTree, syntaxTreeAvailable } from '@codemirror/language';
import type { EditorState } from '@codemirror/state';
import type { SyntaxNode, SyntaxNodeRef, Tree } from '@lezer/common';

import { parser } from './parser/geoscript';

export interface GizmoSite {
  /** Stable key, scoped to the node. Empty for `dynamic` sites. */
  handleId: string;
  kind: 'vec3' | 'transform';
  /** Component count of the value: 3 (`gizmo`), 2 (`gizmo2d`), 1 (`gizmo1d`); 3 for transforms. */
  arity: 1 | 2 | 3;
  /** Runtime-computed name (non-literal first arg) — no static arm/readout widget. */
  dynamic: boolean;
  callFrom: number;
  callTo: number;
  /** Insertion point for a name literal (just after `(`). */
  nameInsertPos: number;
  /** Span of the existing string-literal name, if any. */
  nameRange: [number, number] | null;
}

const GIZMO_CALLEES: Record<string, { kind: 'vec3' | 'transform'; arity: 1 | 2 | 3 }> = {
  gizmo: { kind: 'vec3', arity: 3 },
  giz: { kind: 'vec3', arity: 3 },
  gizmo2d: { kind: 'vec3', arity: 2 },
  giz2d: { kind: 'vec3', arity: 2 },
  gizmo1d: { kind: 'vec3', arity: 1 },
  giz1d: { kind: 'vec3', arity: 1 },
  gizmo_transform: { kind: 'transform', arity: 3 },
  transform_gizmo: { kind: 'transform', arity: 3 },
  giz_tfn: { kind: 'transform', arity: 3 },
};

const unquote = (raw: string): string =>
  raw.length < 2 ? raw : raw.slice(1, -1).replace(/\\(["'\\])/g, '$1');

type Read = (from: number, to: number) => string;

/** The name arg the runtime resolves as `arg_refs[0]`: first positional, else `name=` kwarg. */
const resolveNameValue = (call: SyntaxNode, read: Read): SyntaxNode | null => {
  let firstPositional: SyntaxNode | null = null;
  let nameKwarg: SyntaxNode | null = null;
  for (const fa of call.getChildren('FunctionArg')) {
    const inner = fa.firstChild;
    if (!inner) continue;
    if (inner.name === 'Kwarg') {
      const id = inner.firstChild;
      if (id?.name === 'Identifier' && read(id.from, id.to) === 'name') {
        nameKwarg = inner.getChild('Expr')?.firstChild ?? null;
      }
    } else if (!firstPositional) {
      firstPositional = inner.name === 'Expr' ? inner.firstChild : inner;
    }
  }
  return firstPositional ?? nameKwarg;
};

const scanTree = (tree: Tree, read: Read): GizmoSite[] => {
  const sites: GizmoSite[] = [];
  let unnamed = 0;
  tree.iterate({
    enter: (ref: SyntaxNodeRef) => {
      if (ref.name !== 'CallExpr') return;
      const node = ref.node;
      const callee = node.firstChild;
      if (callee?.name !== 'Identifier') return;
      const name = read(callee.from, callee.to);
      const def = GIZMO_CALLEES[name];
      if (!def) return;

      const { kind, arity } = def;
      const nameVal = resolveNameValue(node, read);

      let handleId = '';
      let dynamic = false;
      let nameRange: [number, number] | null = null;
      if (nameVal?.name === 'StringLiteral') {
        handleId = unquote(read(nameVal.from, nameVal.to));
        nameRange = [nameVal.from, nameVal.to];
      } else if (nameVal) {
        dynamic = true;
      } else {
        handleId = `@${unnamed++}`;
      }

      const nameInsertPos = node.getChild('TightLParen')?.to ?? node.from;
      sites.push({
        handleId,
        kind,
        arity,
        dynamic,
        callFrom: node.from,
        callTo: node.to,
        nameInsertPos,
        nameRange,
      });
    },
  });
  return sites;
};

export const scanGizmoSites = (state: EditorState): GizmoSite[] =>
  syntaxTreeAvailable(state)
    ? scanTree(syntaxTree(state), (from, to) => state.doc.sliceString(from, to))
    : scanSource(state.doc.toString());

export const scanSource = (source: string): GizmoSite[] =>
  scanTree(parser.parse(source), (from, to) => source.slice(from, to));

/** Static (non-dynamic) handle ids in a node's source; used for orphaned-handle GC. */
export const scanGizmoHandleIds = (source: string): Set<string> => {
  const out = new Set<string>();
  for (const s of scanSource(source)) if (!s.dynamic) out.add(s.handleId);
  return out;
};

/** Ordered static handle ids (document order); the index drives a handle's categorical color. */
export const scanGizmoHandleOrder = (source: string): string[] =>
  scanSource(source)
    .filter(s => !s.dynamic)
    .map(s => s.handleId);
