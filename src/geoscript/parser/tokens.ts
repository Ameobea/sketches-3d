/**
 * Adapted from the Rust Lezer grammar to work around the range/float ambiguity:
 * https://github.com/lezer-parser/rust/blob/41493cbd5190c72212908cb66c372f4ae3a13cd1/src/tokens.js#L1
 */

import { ExternalTokenizer } from '@lezer/lr';
import { Float, TightLBrack, TightLParen } from './geoscript.terms';

const Dot = 46;
const LBrack = 91;
const LParen = 40;
const Space = 32;
const Tab = 9;
const LF = 10;
const CR = 13;

const isNum = (ch: number) => ch >= 48 && ch <= 57;

const isNum_ = (ch: number) => isNum(ch) || ch == 95;

export const literalTokens = new ExternalTokenizer((input, _stack) => {
  if (!isNum(input.next)) {
    return;
  }

  let isFloat = false;
  do {
    input.advance();
  } while (isNum_(input.next));
  if (input.next == Dot) {
    isFloat = true;
    input.advance();
    if (isNum(input.next)) {
      do {
        input.advance();
      } while (isNum_(input.next));
    } else if (input.next == Dot || input.next > 0x7f || /\w/.test(String.fromCharCode(input.next))) {
      return;
    }
  }

  if (isFloat) {
    input.acceptToken(Float);
  }
});

// Diverges from the Pest runtime: Pest rejects `arr [0]` outright, but here we
// accept single-line whitespace before `[`/`(` to keep editor highlighting forgiving.
// Newlines are still hard separators (matching the runtime's `\n[`/`\n(` preprocessor).
// `stack.canShift` prevents emitting where these tokens aren't valid (e.g. `(` at the
// start of a function arg, which should begin a `ParenthesizedExpr`).
export const tightPostfixTokens = new ExternalTokenizer((input, stack) => {
  const ch = input.next;
  if (ch !== LBrack && ch !== LParen) return;
  let offset = -1;
  while (true) {
    const prev = input.peek(offset);
    if (prev < 0) return; // start of input
    if (prev === LF || prev === CR) return;
    if (prev !== Space && prev !== Tab) break;
    offset -= 1;
  }
  if (ch === LBrack && stack.canShift(TightLBrack)) {
    input.acceptToken(TightLBrack, 1);
  } else if (ch === LParen && stack.canShift(TightLParen)) {
    input.acceptToken(TightLParen, 1);
  }
});
