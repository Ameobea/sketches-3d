/**
 * Adapted from the Rust Lezer grammar to work around the range/float ambiguity:
 * https://github.com/lezer-parser/rust/blob/41493cbd5190c72212908cb66c372f4ae3a13cd1/src/tokens.js#L1
 */

import { ExternalTokenizer } from '@lezer/lr';
import { Float } from './geoscript.terms';

const Dot = 46;

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
