// Parser parity test cases. The cases below mirror the Rust list in
// `src/viz/wasm/geoscript/src/lib.rs::PARSER_PARITY_CASES` — keep them in sync by hand.
//
// Outcome encoding:
//   - { ok: n }: parse succeeds with n top-level statements and no error nodes.
//   - { err: true }: parse contains at least one error node.
//
// Note on Lezer vs Pest divergence:
//   For inputs like `arr [0]` (whitespace before `[`), Lezer parses two valid statements
//   (`arr` and `[0]`) because `TightLBrack` is only emitted when the `[` is adjacent to
//   the preceding token. Pest's runtime parser rejects the same input with a tightness
//   error. The runtime is the authoritative arbiter for whether the program is valid;
//   the Lezer tree provides best-effort highlighting. Cases marked `lezerOnlyOk` document
//   this gap explicitly. Run with:
//     yarn tsx --test src/geoscript/parser/parser.test.ts

import assert from 'node:assert/strict';
import { test } from 'node:test';

import { parser } from './geoscript';

type Outcome = { ok: number } | { err: true };

interface Case {
  src: string;
  // Expected outcome under the canonical "tight `[`" rule.
  expected: Outcome;
  // Set when the Lezer parser is expected to accept the input as N statements but the
  // Pest runtime rejects it. Documents intentional Lezer-side leniency.
  lezerOnlyOk?: number;
}

const CASES: Case[] = [
  // Basic
  { src: '1', expected: { ok: 1 } },
  { src: '1 + 2', expected: { ok: 1 } },
  { src: 'a = 1', expected: { ok: 1 } },
  { src: 'f(1, 2)', expected: { ok: 1 } },
  // Field access — tight
  { src: 'arr[0]', expected: { ok: 1 } },
  { src: 'arr[0][1]', expected: { ok: 1 } },
  { src: 'arr[0].field', expected: { ok: 1 } },
  { src: '[1,2,3][0]', expected: { ok: 1 } },
  { src: '{ 1 }[0]', expected: { ok: 1 } },
  { src: 'f(1)[0]', expected: { ok: 1 } },
  // Static field access — permissive
  { src: 'arr.field', expected: { ok: 1 } },
  { src: 'arr .field', expected: { ok: 1 } },
  { src: 'arr\n  .field', expected: { ok: 1 } },
  { src: 'arr.a.b.c', expected: { ok: 1 } },
  // Field access — single-line space before `[`. Pest rejects via AST tightness check;
  // Lezer tolerates and treats it as field access (1 statement). Documented divergence
  // — the runtime is authoritative.
  { src: 'arr [0]', expected: { err: true }, lezerOnlyOk: 1 },
  { src: '{ 1 } [0]', expected: { err: true }, lezerOnlyOk: 1 },
  // `arr[1,2,3]` — Pest's targeted AST check rejects; Lezer errors on the comma inside
  // FieldAccessExpr.
  { src: 'arr[1,2,3]', expected: { err: true } },
  // Newline before `[` / `(` — preprocessor on the Pest side inserts `;`; Lezer's
  // tokenizer skips emitting Tight* because the gap contains `\n`. Both: 2 statements.
  { src: 'arr\n[0]', expected: { ok: 2 } },
  { src: 'arr\n[1,2,3]', expected: { ok: 2 } },
  { src: 'foo\n(x)', expected: { ok: 2 } },
  { src: '{ 1 }\n[0]', expected: { ok: 2 } },
  // Multi-line array literal cases (the original bug). Both parsers: two statements.
  { src: '{ 1 }\n[1, 2, 3]', expected: { ok: 2 } },
  { src: 'x = { 1 }\n[1, 2, 3]', expected: { ok: 2 } },
  { src: '[1,2,3]\n[4,5,6]', expected: { ok: 2 } },
  // Function calls — tight rule already in place
  { src: 'foo()', expected: { ok: 1 } },
  { src: 'foo ()', expected: { err: true }, lezerOnlyOk: 1 },
  // Closures — Pest side runs the source preprocessor; Lezer accepts the original
  // source via the `ClosureExpr` rule that allows shorthand bodies.
  { src: '|| 1', expected: { ok: 1 } },
  { src: '|x| x', expected: { ok: 1 } },
  { src: '|x| x + 1', expected: { ok: 1 } },
  { src: '|x| x | 1', expected: { ok: 1 } },
  { src: '|x| x || y', expected: { ok: 1 } },
  { src: '|x| |y| x + y', expected: { ok: 1 } },
  { src: '|x = 1| x', expected: { ok: 1 } },
  { src: '|x: int| x', expected: { ok: 1 } },
  { src: 'foo(x=|a| a + 1, b=2)', expected: { ok: 1 } },
  { src: 'foo(|a| a, |b| b)', expected: { ok: 1 } },
  { src: '[|x| x + 1, |y| y * 2]', expected: { ok: 1 } },
  { src: '{key: |x| x + 1}', expected: { ok: 1 } },
  { src: 'x = |a| a + 1\ny = 2', expected: { ok: 2 } },
  { src: 'a || b', expected: { ok: 1 } },
  { src: 'a | b', expected: { ok: 1 } },
  { src: 'a ?? b', expected: { ok: 1 } },
  { src: 'a ?? b ?? c', expected: { ok: 1 } },
  // Empty body before newline — Pest preprocessor errors. Lezer accepts with the
  // body being whatever follows on the next line.
  { src: 'x = ||\n 1', expected: { err: true }, lezerOnlyOk: 1 },
  // `from` is contextual: a valid identifier/kwarg name everywhere except an import.
  { src: 'align(from=1, to=2)', expected: { ok: 1 } },
  { src: 'from = 5', expected: { ok: 1 } },
  { src: 'import { a, b } from "mod"', expected: { ok: 1 } },
];

interface LezerResult {
  hasError: boolean;
  topLevelStmts: number;
}

function parseWithLezer(src: string): LezerResult {
  const tree = parser.parse(src);
  let hasError = false;
  let topLevelStmts = 0;
  const cursor = tree.cursor();
  // First child is the Program root; iterate its direct children for statement count.
  if (cursor.firstChild()) {
    do {
      if (cursor.type.isError) {
        hasError = true;
      } else if (cursor.name !== '⚠') {
        topLevelStmts++;
      }
    } while (cursor.nextSibling());
  }
  // Also scan the whole tree for nested error nodes (e.g. inside an expression).
  if (!hasError) {
    tree.cursor().iterate(({ type }) => {
      if (type.isError) hasError = true;
    });
  }
  return { hasError, topLevelStmts };
}

for (const { src, expected, lezerOnlyOk } of CASES) {
  test(`lezer: ${JSON.stringify(src)}`, () => {
    const result = parseWithLezer(src);
    // Lezer outcome: prefer `lezerOnlyOk` when set, otherwise use `expected`.
    const target: Outcome = lezerOnlyOk != null ? { ok: lezerOnlyOk } : expected;
    if ('ok' in target) {
      assert.equal(result.hasError, false, `expected no parse errors, got tree:\n${parser.parse(src).toString()}`);
      assert.equal(
        result.topLevelStmts,
        target.ok,
        `expected ${target.ok} top-level statements, got ${result.topLevelStmts}`
      );
    } else {
      assert.equal(result.hasError, true, 'expected at least one error node in the tree');
    }
  });
}
