import { syntaxTree, syntaxTreeAvailable } from '@codemirror/language';
import type { EditorState } from '@codemirror/state';
import type { SyntaxNode } from '@lezer/common';

/** Lezer retains strings and comments even while the program is too incomplete for Pest. */
export const isNonCodePosition = (state: EditorState, pos: number): boolean => {
  for (let node: SyntaxNode | null = syntaxTree(state).resolveInner(pos, -1); node; node = node.parent) {
    if (node.name === 'LineComment' && pos > node.from) return true;
    if (node.name === 'StringLiteral' && pos > node.from) {
      // The cursor immediately after a closed string is back in code.
      return pos < node.to || !!node.firstChild?.lastChild?.type.isError;
    }
  }
  return false;
};

/** An identifier at either edge is still being edited, even if completion has no matches. */
export const isEditingArgument = (state: EditorState): boolean => {
  const pos = state.selection.main.head;
  if (!syntaxTreeAvailable(state, pos)) {
    // A bounded fallback before the background parse reaches the cursor; no call syntax is
    // inferred here. Pipeline ownership and parameter binding always come from Rust's AST.
    return /[\w@]/.test(state.sliceDoc(Math.max(0, pos - 1), Math.min(state.doc.length, pos + 1)));
  }
  if (isNonCodePosition(state, pos)) return true;
  const tree = syntaxTree(state);
  for (const side of [-1, 1] as const) {
    const node = tree.resolveInner(pos, side);
    if (
      [
        'Identifier',
        'GlobalIdentifier',
        'Integer',
        'HexInteger',
        'Float',
        'BoolLiteral',
        'NilLiteral',
      ].includes(node.name)
    )
      return true;
    // A lone global sigil is an error token until its identifier is entered.
    if (node.type.isError && state.sliceDoc(node.from, node.to) === '@') return true;
    if (
      ['BinaryExpr', 'UnaryExpr', 'StaticFieldAccessExpr', 'FieldAccessExpr'].includes(node.name) &&
      node.lastChild?.type.isError
    )
      return true;
  }
  return false;
};
