//! Source-level preprocessor that runs before Pest. Handles:
//!  1. `\n[` / `\n(` — insert `;` to force a statement boundary.
//!  2. Shorthand closure bodies — wrap `|args| body` in `{ ... }`.
//!
//! Emits a sorted list of [`Edit`]s that the source map uses to translate rewritten
//! positions back to original.
//!
//! INVARIANT: replacements contain no `\n`/`\r`, so line numbers are preserved across
//! each edit. The source map and Lezer-side tokenizers rely on this.

use std::ops::Range;

use nanoserde::SerJson;

#[derive(Debug, Clone, PartialEq, Eq, SerJson)]
pub struct Edit {
  pub original_start: usize,
  pub original_end: usize,
  /// Must not contain `\n` or `\r` (see module-level invariant).
  pub replacement: String,
  /// 1-indexed; same in original and rewritten because replacements have no newlines.
  pub line: u32,
  pub col_in_original: u32,
  pub col_in_rewritten: u32,
}

impl Edit {
  pub fn original_range(&self) -> Range<usize> {
    self.original_start..self.original_end
  }
}

#[derive(Debug, Clone, Default, SerJson)]
pub struct Preprocessed {
  pub rewritten: String,
  /// Sorted by `original_start`. Pure insertions have `original_start == original_end`.
  pub edits: Vec<Edit>,
}

#[derive(Debug, Clone)]
pub struct PreprocessError {
  pub message: String,
  pub original_pos: usize,
  pub line: u32,
  pub col: u32,
}

impl PreprocessError {
  fn new(message: impl Into<String>, original_pos: usize, line: u32, col: u32) -> Self {
    PreprocessError {
      message: message.into(),
      original_pos,
      line,
      col,
    }
  }
}

impl Preprocessed {
  /// Positions inside an inserted region collapse to the insertion point.
  pub fn rewritten_to_original(&self, rewritten_pos: usize) -> usize {
    let mut byte_offset: isize = 0;
    for edit in &self.edits {
      let edit_rewritten_start = (edit.original_start as isize + byte_offset) as usize;
      let edit_rewritten_end = edit_rewritten_start + edit.replacement.len();
      if rewritten_pos < edit_rewritten_start {
        break;
      }
      if rewritten_pos < edit_rewritten_end {
        return edit.original_start;
      }
      byte_offset +=
        edit.replacement.len() as isize - (edit.original_end - edit.original_start) as isize;
    }
    (rewritten_pos as isize - byte_offset).max(0) as usize
  }

  /// Positions inside a replaced range collapse to the replacement's start.
  pub fn original_to_rewritten(&self, original_pos: usize) -> usize {
    let mut byte_offset: isize = 0;
    for edit in &self.edits {
      if edit.original_start > original_pos {
        break;
      }
      if edit.original_end <= original_pos {
        byte_offset +=
          edit.replacement.len() as isize - (edit.original_end - edit.original_start) as isize;
      } else {
        return (edit.original_start as isize + byte_offset) as usize;
      }
    }
    (original_pos as isize + byte_offset) as usize
  }

  /// Positions inside an inserted region collapse to the insertion point. Line is
  /// preserved (replacements span no newlines).
  pub fn rewritten_line_col_to_original(&self, line: u32, col: u32) -> (u32, u32) {
    let mut col_shift: i32 = 0;
    for edit in &self.edits {
      if edit.line != line {
        if edit.line > line {
          break;
        }
        continue;
      }
      let edit_end_col = edit.col_in_rewritten + edit.replacement.len() as u32;
      if edit_end_col <= col {
        let original_len = (edit.original_end - edit.original_start) as i32;
        col_shift += edit.replacement.len() as i32 - original_len;
      } else if edit.col_in_rewritten <= col {
        return (line, edit.col_in_original);
      } else {
        break;
      }
    }
    (line, ((col as i32) - col_shift).max(1) as u32)
  }
}

/// Drives the `\n[`/`\n(` rule and the closure-header vs binary-`|` disambiguation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LastSignificant {
  /// After `=`, an operator, an opening bracket, `,`, `;`, or start of input.
  ExprStarter,
  /// After an ident char, digit, `]`, `}`, `)`, or closing quote.
  ExprEnder,
}

/// Mirror of `geoscript.pest`'s string literal rules.
fn scan_string_literal(src: &str, pos: usize) -> Option<usize> {
  let bytes = src.as_bytes();
  if pos >= bytes.len() {
    return None;
  }
  let quote = bytes[pos];
  if quote != b'"' && quote != b'\'' {
    return None;
  }
  let mut i = pos + 1;
  while i < bytes.len() {
    let b = bytes[i];
    if b == b'\\' {
      i += 2;
      continue;
    }
    if b == quote {
      return Some(i + 1);
    }
    i += 1;
  }
  None
}

/// Returns the offset of the terminating `\n` (or end of input).
fn scan_line_comment(src: &str, pos: usize) -> usize {
  let bytes = src.as_bytes();
  let mut i = pos + 2;
  while i < bytes.len() && bytes[i] != b'\n' {
    i += 1;
  }
  i
}

fn char_class(b: u8) -> LastSignificant {
  match b {
    b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b')' | b']' | b'}' => {
      LastSignificant::ExprEnder
    }
    _ => LastSignificant::ExprStarter,
  }
}

fn is_ident_continue(b: u8) -> bool {
  matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
}

/// Find the closing `|` for an opening `|`. Tracks bracket nesting so a binary `|`
/// inside a default value (e.g. `|x = (a | b)|`) isn't mistaken for the closer.
fn scan_closure_param_list(src: &str, pos: usize) -> Option<usize> {
  let bytes = src.as_bytes();
  if pos >= bytes.len() || bytes[pos] != b'|' {
    return None;
  }
  let mut i = pos + 1;
  let mut paren: i32 = 0;
  let mut curly: i32 = 0;
  while i < bytes.len() {
    let b = bytes[i];
    match b {
      b'(' | b'[' => {
        paren += 1;
        i += 1;
      }
      b')' | b']' => {
        paren -= 1;
        if paren < 0 {
          return None;
        }
        i += 1;
      }
      b'{' => {
        curly += 1;
        i += 1;
      }
      b'}' => {
        curly -= 1;
        if curly < 0 {
          return None;
        }
        i += 1;
      }
      b'"' | b'\'' => {
        i = scan_string_literal(src, i)?;
      }
      b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
        i = scan_line_comment(src, i);
      }
      b'|' if paren == 0 && curly == 0 => return Some(i),
      _ => i += 1,
    }
  }
  None
}

/// One open shorthand-closure body being wrapped in `{`/`}`.
#[derive(Debug, Clone)]
struct ClosureEntry {
  /// Bracket nesting at insertion time. A `)`/`]` that drops below this terminates
  /// the body.
  base_paren: i32,
  /// REAL `{}` nesting at insertion time — the inserted `{` is not counted.
  base_curly: i32,
  /// Tracked to error on empty bodies (e.g. `||\n 1`).
  content_seen: bool,
  /// Header `|` position; used for empty-body error reporting.
  header_close_pos: usize,
  header_close_line: u32,
  header_close_col: u32,
}

struct Scanner<'a> {
  src: &'a str,
  bytes: &'a [u8],
  pos: usize,
  line: u32,
  col: u32,
  last_sig: LastSignificant,
  pending_newline: bool,
  paren_bracket_nesting: i32,
  /// REAL `{}` only — inserted `{` from closure wrapping lives in `closure_stack`.
  curly_nesting: i32,
  closure_stack: Vec<ClosureEntry>,
  edits: Vec<Edit>,
  /// Inserted-byte delta on the current line. Reset on `\n`.
  rewritten_col_shift_this_line: i32,
}

impl<'a> Scanner<'a> {
  fn new(src: &'a str) -> Self {
    Scanner {
      src,
      bytes: src.as_bytes(),
      pos: 0,
      line: 1,
      col: 1,
      last_sig: LastSignificant::ExprStarter,
      pending_newline: false,
      paren_bracket_nesting: 0,
      curly_nesting: 0,
      closure_stack: Vec::new(),
      edits: Vec::new(),
      rewritten_col_shift_this_line: 0,
    }
  }

  fn advance(&mut self, n: usize) {
    for _ in 0..n {
      if self.pos >= self.bytes.len() {
        return;
      }
      let b = self.bytes[self.pos];
      self.pos += 1;
      if b == b'\n' {
        self.line += 1;
        self.col = 1;
        self.rewritten_col_shift_this_line = 0;
      } else {
        self.col += 1;
      }
    }
  }

  fn advance_to(&mut self, target: usize) {
    while self.pos < target {
      self.advance(1);
    }
  }

  fn insert_at_current(&mut self, replacement: &str) {
    debug_assert!(
      !replacement.contains('\n') && !replacement.contains('\r'),
      "preprocessor replacements must not contain newlines",
    );
    self.edits.push(Edit {
      original_start: self.pos,
      original_end: self.pos,
      replacement: replacement.to_owned(),
      line: self.line,
      col_in_original: self.col,
      col_in_rewritten: (self.col as i32 + self.rewritten_col_shift_this_line) as u32,
    });
    self.rewritten_col_shift_this_line += replacement.len() as i32;
  }

  fn mark_content(&mut self) {
    if let Some(top) = self.closure_stack.last_mut() {
      top.content_seen = true;
    }
  }

  /// Emit `}` at the current position. Errors on empty body.
  fn pop_closure(&mut self) -> Result<(), PreprocessError> {
    let entry = self
      .closure_stack
      .pop()
      .expect("pop_closure called with empty stack");
    if !entry.content_seen {
      return Err(PreprocessError::new(
        "shorthand closure has empty body; write `|...| { }` for an explicit empty body or \
         provide an expression",
        entry.header_close_pos,
        entry.header_close_line,
        entry.header_close_col,
      ));
    }
    self.insert_at_current("}");
    Ok(())
  }

  /// Advance from the opening `|` past `close_pos` (the closing `|`), tracking
  /// nesting through brackets in default-value expressions.
  fn advance_through_header(&mut self, close_pos: usize) -> Result<(), PreprocessError> {
    debug_assert!(self.bytes[self.pos] == b'|');
    self.advance(1); // consume opening `|`
    while self.pos <= close_pos {
      let b = self.bytes[self.pos];
      match b {
        b'(' | b'[' => {
          self.paren_bracket_nesting += 1;
          self.advance(1);
        }
        b')' | b']' => {
          self.paren_bracket_nesting -= 1;
          self.advance(1);
        }
        b'{' => {
          self.curly_nesting += 1;
          self.advance(1);
        }
        b'}' => {
          self.curly_nesting -= 1;
          self.advance(1);
        }
        b'"' | b'\'' => {
          let start_line = self.line;
          let start_col = self.col;
          let end = scan_string_literal(self.src, self.pos).ok_or_else(|| {
            PreprocessError::new(
              "unterminated string literal",
              self.pos,
              start_line,
              start_col,
            )
          })?;
          self.advance_to(end);
        }
        b'/' if self.pos + 1 < self.bytes.len() && self.bytes[self.pos + 1] == b'/' => {
          let end = scan_line_comment(self.src, self.pos);
          self.advance_to(end);
        }
        _ => self.advance(1),
      }
    }
    Ok(())
  }

  /// Decide whether to wrap the closure body in `{}`. Body rules:
  /// - Without `:type`, body must be on the same line as the closing `|`.
  /// - With `:type`, newlines between type and body are tolerated (see examples/2.geo).
  /// - A leading `{` means an explicit braced body; no transform.
  fn handle_post_header(&mut self) -> Result<(), PreprocessError> {
    self.skip_inline_whitespace();
    let mut had_type_hint = false;
    if self.pos < self.bytes.len() && self.bytes[self.pos] == b':' {
      had_type_hint = true;
      self.advance(1);
      self.skip_inline_whitespace();
      while self.pos < self.bytes.len() && is_ident_continue(self.bytes[self.pos]) {
        self.advance(1);
      }
      // After a type hint, newlines between the type and the body are allowed.
      self.skip_header_whitespace();
    }
    if self.pos >= self.bytes.len() {
      return Err(PreprocessError::new(
        "shorthand closure has empty body; write `|...| { }` for an explicit empty body or \
         provide an expression",
        self.pos,
        self.line,
        self.col,
      ));
    }
    let b = self.bytes[self.pos];
    if b == b'{' {
      // Braced body. Let Pest handle the rest; this is NOT a shorthand closure.
      self.curly_nesting += 1;
      self.last_sig = LastSignificant::ExprStarter;
      self.pending_newline = false;
      self.advance(1);
      return Ok(());
    }
    if !had_type_hint && (b == b'\n' || b == b'\r') {
      return Err(PreprocessError::new(
        "shorthand closure has empty body before newline; provide an expression on the same line \
         as the closing `|`, or wrap the body in `{ ... }`",
        self.pos,
        self.line,
        self.col,
      ));
    }
    // Don't advance past the `{`; the body chars are handled by the main loop.
    self.mark_content();
    let header_close_pos = self.pos;
    let header_close_line = self.line;
    let header_close_col = self.col;
    self.insert_at_current("{");
    self.closure_stack.push(ClosureEntry {
      base_paren: self.paren_bracket_nesting,
      base_curly: self.curly_nesting,
      content_seen: false,
      header_close_pos,
      header_close_line,
      header_close_col,
    });
    self.last_sig = LastSignificant::ExprStarter;
    self.pending_newline = false;
    Ok(())
  }

  /// Spaces, tabs, line comments — but not newlines.
  fn skip_inline_whitespace(&mut self) {
    loop {
      if self.pos >= self.bytes.len() {
        return;
      }
      let b = self.bytes[self.pos];
      if b == b' ' || b == b'\t' || b == b'\r' {
        self.advance(1);
      } else if b == b'/' && self.pos + 1 < self.bytes.len() && self.bytes[self.pos + 1] == b'/' {
        let end = scan_line_comment(self.src, self.pos);
        self.advance_to(end);
      } else {
        return;
      }
    }
  }

  /// Like `skip_inline_whitespace` but also skips newlines (for use after `:type`).
  fn skip_header_whitespace(&mut self) {
    loop {
      if self.pos >= self.bytes.len() {
        return;
      }
      let b = self.bytes[self.pos];
      if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' {
        self.advance(1);
      } else if b == b'/' && self.pos + 1 < self.bytes.len() && self.bytes[self.pos + 1] == b'/' {
        let end = scan_line_comment(self.src, self.pos);
        self.advance_to(end);
      } else {
        return;
      }
    }
  }

  fn run(mut self) -> Result<Preprocessed, PreprocessError> {
    while self.pos < self.bytes.len() {
      let b = self.bytes[self.pos];

      // After an expr-ender, `|` is a binary pipe and `||` is logical-or — consume as
      // a unit so a trailing `|` isn't misread as a closure header. After an
      // expr-starter, `|` opens a closure (including `||` empty-params).
      if b == b'|' {
        if self.last_sig == LastSignificant::ExprEnder {
          let n = if self.pos + 1 < self.bytes.len() && self.bytes[self.pos + 1] == b'|' {
            2
          } else {
            1
          };
          self.advance(n);
          self.last_sig = LastSignificant::ExprStarter;
          self.pending_newline = false;
          continue;
        }
        if let Some(close_pos) = scan_closure_param_list(self.src, self.pos) {
          self.advance_through_header(close_pos)?;
          self.handle_post_header()?;
          continue;
        }
        // Malformed; fall through and let Pest report a syntax error.
      }

      if b == b' ' || b == b'\t' || b == b'\r' {
        self.advance(1);
        continue;
      }
      if b == b'\n' {
        // Newline terminates ALL open shorthand-closure bodies.
        while !self.closure_stack.is_empty() {
          self.pop_closure()?;
        }
        self.pending_newline = true;
        self.advance(1);
        continue;
      }
      if b == b'/' && self.pos + 1 < self.bytes.len() && self.bytes[self.pos + 1] == b'/' {
        let end = scan_line_comment(self.src, self.pos);
        self.advance_to(end);
        continue;
      }
      if b == b'"' || b == b'\'' {
        let start_line = self.line;
        let start_col = self.col;
        let end = scan_string_literal(self.src, self.pos).ok_or_else(|| {
          PreprocessError::new(
            "unterminated string literal",
            self.pos,
            start_line,
            start_col,
          )
        })?;
        self.advance_to(end);
        self.last_sig = LastSignificant::ExprEnder;
        self.pending_newline = false;
        self.mark_content();
        continue;
      }
      // `\n[` / `\n(` rule.
      if (b == b'[' || b == b'(')
        && self.pending_newline
        && self.paren_bracket_nesting == 0
        && self.last_sig == LastSignificant::ExprEnder
      {
        self.insert_at_current(";");
      }
      // Closure body termination on `,` / `)` / `]` / `}`.
      match b {
        b',' => {
          while let Some(top) = self.closure_stack.last() {
            if self.paren_bracket_nesting == top.base_paren && self.curly_nesting == top.base_curly
            {
              self.pop_closure()?;
            } else {
              break;
            }
          }
        }
        b')' | b']' => {
          let new_paren = self.paren_bracket_nesting - 1;
          while let Some(top) = self.closure_stack.last() {
            if new_paren < top.base_paren {
              self.pop_closure()?;
            } else {
              break;
            }
          }
        }
        b'}' => {
          let new_curly = self.curly_nesting - 1;
          while let Some(top) = self.closure_stack.last() {
            if new_curly < top.base_curly {
              self.pop_closure()?;
            } else {
              break;
            }
          }
        }
        _ => {}
      }
      match b {
        b'(' | b'[' => self.paren_bracket_nesting += 1,
        b')' | b']' => self.paren_bracket_nesting = (self.paren_bracket_nesting - 1).max(0),
        b'{' => self.curly_nesting += 1,
        b'}' => self.curly_nesting = (self.curly_nesting - 1).max(0),
        _ => {}
      }
      self.last_sig = char_class(b);
      self.pending_newline = false;
      self.mark_content();
      self.advance(1);
    }
    // End of input: pop any remaining closures.
    while !self.closure_stack.is_empty() {
      self.pop_closure()?;
    }
    Ok(Preprocessed {
      rewritten: build_rewritten(self.src, &self.edits),
      edits: self.edits,
    })
  }
}

fn build_rewritten(src: &str, edits: &[Edit]) -> String {
  if edits.is_empty() {
    return src.to_owned();
  }
  let extra: usize = edits.iter().map(|e| e.replacement.len()).sum();
  let mut out = String::with_capacity(src.len() + extra);
  let mut cursor = 0usize;
  for edit in edits {
    out.push_str(&src[cursor..edit.original_start]);
    out.push_str(&edit.replacement);
    cursor = edit.original_end;
  }
  out.push_str(&src[cursor..]);
  out
}

/// Run the preprocessor on `src`.
pub fn preprocess(src: &str) -> Result<Preprocessed, PreprocessError> {
  Scanner::new(src).run()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn rewritten(src: &str) -> String {
    preprocess(src).unwrap().rewritten
  }

  #[test]
  fn no_change_for_arr_tight() {
    assert_eq!(rewritten("arr[0]"), "arr[0]");
  }

  #[test]
  fn no_change_for_arr_space() {
    // A space alone doesn't insert `;` — AST tightness catches that case later.
    assert_eq!(rewritten("arr [0]"), "arr [0]");
  }

  #[test]
  fn inserts_semicolon_for_newline_lbrack() {
    assert_eq!(rewritten("arr\n[0]"), "arr\n;[0]");
  }

  #[test]
  fn inserts_semicolon_for_newline_lparen() {
    assert_eq!(rewritten("foo\n(x)"), "foo\n;(x)");
  }

  #[test]
  fn no_insert_inside_brackets() {
    assert_eq!(rewritten("foo(\n[1,2,3]\n)"), "foo(\n[1,2,3]\n)");
  }

  #[test]
  fn no_insert_after_operator() {
    assert_eq!(rewritten("a +\n[1]"), "a +\n[1]");
  }

  #[test]
  fn no_insert_at_start_of_program() {
    assert_eq!(rewritten("\n[1,2,3]"), "\n[1,2,3]");
  }

  #[test]
  fn no_insert_inside_string() {
    assert_eq!(rewritten("\"a\n[\""), "\"a\n[\"");
  }

  #[test]
  fn no_insert_inside_line_comment() {
    assert_eq!(rewritten("// arr\n[1,2,3]"), "// arr\n[1,2,3]");
  }

  #[test]
  fn closing_bracket_then_newline_lbrack() {
    assert_eq!(rewritten("foo()\n[1,2,3]"), "foo()\n;[1,2,3]");
  }

  #[test]
  fn original_bug_case() {
    let src = "x = { 1 }\n[1, 2, 3]";
    assert_eq!(rewritten(src), "x = { 1 }\n;[1, 2, 3]");
  }

  #[test]
  fn rewritten_to_original_simple() {
    // "arr\n[0]" → "arr\n;[0]", `;` inserted at original byte 4.
    let p = preprocess("arr\n[0]").unwrap();
    assert_eq!(p.rewritten, "arr\n;[0]");
    assert_eq!(p.rewritten_to_original(0), 0);
    assert_eq!(p.rewritten_to_original(3), 3);
    assert_eq!(p.rewritten_to_original(4), 4); // inside insertion → collapses
    assert_eq!(p.rewritten_to_original(5), 4);
    assert_eq!(p.rewritten_to_original(7), 6);
  }

  #[test]
  fn original_to_rewritten_simple() {
    let p = preprocess("arr\n[0]").unwrap();
    assert_eq!(p.original_to_rewritten(0), 0);
    assert_eq!(p.original_to_rewritten(3), 3);
    assert_eq!(p.original_to_rewritten(4), 5);
    assert_eq!(p.original_to_rewritten(6), 7);
  }

  #[test]
  fn line_col_translation_within_same_line() {
    let p = preprocess("arr\n[0]").unwrap();
    assert_eq!(p.rewritten_line_col_to_original(1, 1), (1, 1));
    assert_eq!(p.rewritten_line_col_to_original(1, 3), (1, 3));
    assert_eq!(p.rewritten_line_col_to_original(2, 1), (2, 1)); // inside insertion
    assert_eq!(p.rewritten_line_col_to_original(2, 2), (2, 1));
    assert_eq!(p.rewritten_line_col_to_original(2, 3), (2, 2));
  }
}
