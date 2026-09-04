//! Lexical scan of the source up to a cursor offset: bracket nesting, closure param lists,
//! string/comment state and per-frame argument segments, so editor features can locate the
//! enclosing call without a successful parse (Pest has no error recovery).

use std::ops::Range;

/// Byte offset of a 1-based (line, col); cols count chars, as Pest does.  `None` when out of range.
pub fn line_col_to_offset(src: &str, line: u32, col: u32) -> Option<usize> {
  if col == 0 {
    return None;
  }
  let span = line_span(src, line)?;
  let text = &src[span.clone()];
  text
    .char_indices()
    .map(|(i, _)| i)
    .chain(std::iter::once(text.len()))
    .nth(col as usize - 1)
    .map(|i| span.start + i)
}

/// 1-based (line, col) of a byte offset; cols count chars.
pub fn offset_to_line_col(src: &str, offset: usize) -> (u32, u32) {
  let before = &src[..offset.min(src.len())];
  let line_start = before.rfind('\n').map_or(0, |i| i + 1);
  (
    before.matches('\n').count() as u32 + 1,
    before[line_start..].chars().count() as u32 + 1,
  )
}

/// Byte range of a 1-based line's content, excluding its newline.
pub fn line_span(src: &str, line: u32) -> Option<Range<usize>> {
  if line == 0 {
    return None;
  }
  let mut start = 0;
  let mut lines = src.split('\n');
  for _ in 1..line {
    start += lines.next()?.len() + 1;
  }
  let text = lines.next()?;
  Some(start..start + text.len())
}

fn is_ident_byte(b: u8) -> bool {
  b.is_ascii_alphanumeric() || b == b'_'
}

/// End column of the identifier written at `(line, col)`, falling back to `col + fallback_len`
/// when no identifier starts there.
///
/// The interned name of a symbol isn't always the text at its location — the `path { ... }`
/// desugar rewrites draw commands (`bezier` → `path_cubic_bezier`), so using the name's length
/// would stretch hover and goto ranges over the following source.
pub fn ident_end_col(src: &str, line: u32, col: u32, fallback_len: u32) -> u32 {
  let Some(offset) = line_col_to_offset(src, line, col) else {
    return col + fallback_len;
  };
  let bytes = src.as_bytes();
  let mut end = offset;
  while end < bytes.len() && is_ident_byte(bytes[end]) {
    end += 1;
  }
  if end == offset {
    return col + fallback_len;
  }
  col + (end - offset) as u32
}

/// Byte range of the identifier word containing or directly preceding `offset`.
pub fn word_at(src: &str, offset: usize) -> Option<(usize, usize)> {
  let bytes = src.as_bytes();
  if offset > bytes.len() {
    return None;
  }
  let mut start = offset;
  while start > 0 && is_ident_byte(bytes[start - 1]) {
    start -= 1;
  }
  let mut end = offset;
  while end < bytes.len() && is_ident_byte(bytes[end]) {
    end += 1;
  }
  (start < end).then_some((start, end))
}

const KEYWORDS: &[&str] = &["if", "else", "return", "break"];

/// Start of the identifier (with any `@` sigil) ending exactly at `end`, if one does and it
/// isn't a keyword or number.
fn ident_ending_at(src: &str, end: usize) -> Option<usize> {
  let bytes = src.as_bytes();
  let mut start = end;
  while start > 0 && is_ident_byte(bytes[start - 1]) {
    start -= 1;
  }
  if start == end || bytes[start].is_ascii_digit() || KEYWORDS.contains(&&src[start..end]) {
    return None;
  }
  Some(if start > 0 && bytes[start - 1] == b'@' {
    start - 1
  } else {
    start
  })
}

/// `name` when `seg` starts with `name =` (and not `name ==`).
fn kwarg_prefix(seg: &str) -> Option<&str> {
  let seg = seg.trim_start();
  let bytes = seg.as_bytes();
  let mut end = 0;
  while end < bytes.len() && is_ident_byte(bytes[end]) {
    end += 1;
  }
  if end == 0 || bytes[0].is_ascii_digit() {
    return None;
  }
  let rest = seg[end..].trim_start().as_bytes();
  (rest.first() == Some(&b'=') && rest.get(1) != Some(&b'=')).then(|| &seg[..end])
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameKind {
  Paren,
  Bracket,
  Brace,
  ClosureParams,
}

struct Frame {
  kind: FrameKind,
  open: usize,
  /// `(` frames: start offset of the tight callee identifier (`@` included), if any.
  callee: Option<usize>,
  is_path_block: bool,
  segment_start: usize,
  /// Completed comma-separated segments before the current one, split by kind.
  positional_before: usize,
  kwargs_before: Vec<(usize, usize)>,
}

impl Frame {
  fn new(kind: FrameKind, open: usize) -> Self {
    Frame {
      kind,
      open,
      callee: None,
      is_path_block: false,
      segment_start: open + 1,
      positional_before: 0,
      kwargs_before: Vec::new(),
    }
  }

  fn end_segment(&mut self, src: &str, comma: usize) {
    let seg = &src[self.segment_start..comma];
    if let Some(name) = kwarg_prefix(seg) {
      let start = self.segment_start + (seg.len() - seg.trim_start().len());
      self.kwargs_before.push((start, start + name.len()));
    } else if !seg.trim().is_empty() {
      self.positional_before += 1;
    }
    self.segment_start = comma + 1;
  }
}

struct Scan {
  frames: Vec<Frame>,
  in_string_or_comment: bool,
}

/// Whether the previous significant token ended an operand.  Decides if a `|` opens a closure
/// param list (expression position) or is the pipeline operator (after an operand).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Prev {
  ExprStart,
  Operand,
}

fn scan_to(src: &str, offset: usize) -> Scan {
  let bytes = src.as_bytes();
  let end = offset.min(bytes.len());
  let mut frames: Vec<Frame> = Vec::new();
  let mut prev = Prev::ExprStart;
  let mut i = 0;
  while i < end {
    let c = bytes[i];
    match c {
      b'"' | b'\'' => {
        i += 1;
        loop {
          if i >= end {
            return Scan {
              frames,
              in_string_or_comment: true,
            };
          }
          match bytes[i] {
            b'\\' => i += 2,
            b if b == c => break,
            _ => i += 1,
          }
        }
        prev = Prev::Operand;
      }
      b'/' if bytes.get(i + 1) == Some(&b'/') => {
        while i < end && bytes[i] != b'\n' {
          i += 1;
        }
        if i >= end {
          return Scan {
            frames,
            in_string_or_comment: true,
          };
        }
        continue;
      }
      b'(' | b'[' | b'{' => {
        let kind = match c {
          b'(' => FrameKind::Paren,
          b'[' => FrameKind::Bracket,
          _ => FrameKind::Brace,
        };
        let mut frame = Frame::new(kind, i);
        match kind {
          FrameKind::Paren => frame.callee = ident_ending_at(src, i),
          FrameKind::Brace => {
            let before = src[..i].trim_end();
            frame.is_path_block = before.ends_with("path")
              && ident_ending_at(src, before.len()) == Some(before.len() - 4);
          }
          _ => {}
        }
        frames.push(frame);
        prev = Prev::ExprStart;
      }
      b')' | b']' | b'}' => {
        let kind = match c {
          b')' => FrameKind::Paren,
          b']' => FrameKind::Bracket,
          _ => FrameKind::Brace,
        };
        if frames.last().is_some_and(|f| f.kind == kind) {
          frames.pop();
        }
        prev = Prev::Operand;
      }
      b',' => {
        if let Some(frame) = frames.last_mut() {
          frame.end_segment(src, i);
        }
        prev = Prev::ExprStart;
      }
      b'|' => {
        if bytes.get(i + 1) == Some(&b'|') {
          // `||`: logical or, or an empty closure param list; an expression follows either way
          i += 2;
          prev = Prev::ExprStart;
          continue;
        }
        if frames
          .last()
          .is_some_and(|f| f.kind == FrameKind::ClosureParams)
        {
          frames.pop();
        } else if prev == Prev::ExprStart {
          frames.push(Frame::new(FrameKind::ClosureParams, i));
        }
        prev = Prev::ExprStart;
      }
      _ if is_ident_byte(c) => {
        let start = i;
        while i < end && is_ident_byte(bytes[i]) {
          i += 1;
        }
        prev = if KEYWORDS.contains(&&src[start..i]) {
          Prev::ExprStart
        } else {
          Prev::Operand
        };
        continue;
      }
      _ if c.is_ascii_whitespace() => {}
      _ => prev = Prev::ExprStart,
    }
    i += 1;
  }
  Scan {
    frames,
    in_string_or_comment: false,
  }
}

/// The innermost function call whose argument list contains the cursor.
#[derive(Debug)]
pub struct CallContext {
  /// Callee identifier without any `@` sigil.
  pub fn_name: String,
  /// Byte offset where the callee is written (`@` included).
  pub callee_offset: usize,
  pub uses_global_sigil: bool,
  /// Byte offset just past the `(` or the last top-level `,` before the cursor.
  pub segment_start: usize,
  /// Number of completed positional args before the cursor's segment.
  pub positional_index: usize,
  /// Names of completed kwargs before the cursor's segment.
  pub kwargs_before: Vec<String>,
  /// The cursor's segment is a `name = ...` kwarg.
  pub current_kwarg: Option<String>,
  pub in_path_block: bool,
}

/// Find the call whose arg list the cursor is in.  Non-call brackets (array/map literals,
/// parenthesized expressions, closure param lists, blocks) are looked through, so a cursor
/// inside `foo(a, [1, |])` reports `foo` at positional index 1.
pub fn enclosing_call(src: &str, offset: usize) -> Option<CallContext> {
  let scan = scan_to(src, offset);
  if scan.in_string_or_comment {
    return None;
  }
  let in_path_block = scan.frames.iter().any(|f| f.is_path_block);
  let frame = scan.frames.iter().rev().find(|f| f.callee.is_some())?;
  let callee_offset = frame.callee.unwrap();
  let name_start = callee_offset + usize::from(src.as_bytes()[callee_offset] == b'@');
  let name_end = name_start + word_at(src, name_start).map_or(0, |(s, e)| e - s);
  Some(CallContext {
    fn_name: src[name_start..name_end].to_owned(),
    callee_offset,
    uses_global_sigil: name_start != callee_offset,
    segment_start: frame.segment_start,
    positional_index: frame.positional_before,
    kwargs_before: frame
      .kwargs_before
      .iter()
      .map(|&(s, e)| src[s..e].to_owned())
      .collect(),
    current_kwarg: kwarg_prefix(&src[frame.segment_start..]).map(str::to_owned),
    in_path_block,
  })
}

/// If the word at `offset` is written as a kwarg name (`name = ...`) inside a call, return
/// the call context and the name.
pub fn kwarg_at(src: &str, offset: usize) -> Option<(CallContext, String)> {
  let (start, end) = word_at(src, offset)?;
  let call = enclosing_call(src, end)?;
  let name = &src[start..end];
  let leads_segment = src[call.segment_start..start].trim().is_empty();
  (leads_segment && call.current_kwarg.as_deref() == Some(name)).then(|| (call, name.to_owned()))
}

/// Whether `offset` sits inside a `path { ... }` block.  Draw-command rewriting applies to
/// nested blocks and closure bodies too, so any enclosing `path {` counts.
pub fn in_path_block(src: &str, offset: usize) -> bool {
  scan_to(src, offset).frames.iter().any(|f| f.is_path_block)
}

/// Line of the innermost bracket still open at the end of the source, if any.
pub fn unclosed_opener_line(src: &str) -> Option<u32> {
  let scan = scan_to(src, src.len());
  let frame = scan
    .frames
    .iter()
    .rev()
    .find(|f| f.kind != FrameKind::ClosureParams)?;
  Some(offset_to_line_col(src, frame.open).0)
}

#[cfg(test)]
mod tests {
  use super::*;

  const CURSOR: char = '‸';

  fn call_at(src: &str) -> Option<CallContext> {
    let offset = src.find(CURSOR).expect("cursor marker");
    let src = src.replacen(CURSOR, "", 1);
    enclosing_call(&src, offset)
  }

  #[test]
  fn test_line_col_to_offset() {
    let src = "abc\ndef\nghi";
    assert_eq!(line_col_to_offset(src, 1, 1), Some(0));
    assert_eq!(line_col_to_offset(src, 1, 3), Some(2));
    assert_eq!(line_col_to_offset(src, 2, 1), Some(4));
    assert_eq!(line_col_to_offset(src, 3, 2), Some(9));
    assert_eq!(offset_to_line_col(src, 9), (3, 2));
    assert_eq!(offset_to_line_col(src, 0), (1, 1));
    assert_eq!(line_col_to_offset(src, 1, 4), Some(3));
    assert_eq!(line_col_to_offset(src, 1, 5), None);
    assert_eq!(line_col_to_offset(src, 4, 1), None);
    assert_eq!(line_span(src, 2), Some(4..7));

    // cols count chars, not bytes
    let src = "s = \"é\", x";
    assert_eq!(line_col_to_offset(src, 1, 11), Some(11));
    assert_eq!(offset_to_line_col(src, 11), (1, 11));
    let c = call_at("f(\"é\", ‸)").unwrap();
    assert_eq!(c.positional_index, 1);
    assert_eq!(unclosed_opener_line("a = 1\nf(\n  b,\n"), Some(2));
    assert_eq!(unclosed_opener_line("a = f(1)\n"), None);
  }

  #[test]
  fn positional_and_kwarg_segments() {
    let c = call_at("translate(mesh, ‸)").unwrap();
    assert_eq!(c.fn_name, "translate");
    assert_eq!(c.positional_index, 1);
    assert_eq!(c.current_kwarg, None);

    let c = call_at("translate(mesh, offset=vec3(1,0,0), ‸)").unwrap();
    assert_eq!(c.positional_index, 1);
    assert_eq!(c.kwargs_before, vec!["offset".to_owned()]);

    let c = call_at("translate(mesh, off‸set = 1)").unwrap();
    assert_eq!(c.current_kwarg.as_deref(), Some("offset"));

    let c = call_at("f(a == ‸b)").unwrap();
    assert_eq!(c.current_kwarg, None);
    assert_eq!(c.positional_index, 0);
  }

  #[test]
  fn nested_calls_and_literals() {
    let c = call_at("foo(bar(x), ba‸z=1)").unwrap();
    assert_eq!(c.fn_name, "foo");
    assert_eq!(c.current_kwarg.as_deref(), Some("baz"));

    let c = call_at("foo(bar(x, ‸y))").unwrap();
    assert_eq!(c.fn_name, "bar");
    assert_eq!(c.positional_index, 1);

    let c = call_at("foo(a, [1, 2, ‸], b)").unwrap();
    assert_eq!(c.fn_name, "foo");
    assert_eq!(c.positional_index, 1);

    let c = call_at("foo(a, {k: 1, ‸").unwrap();
    assert_eq!(c.fn_name, "foo");
    assert_eq!(c.positional_index, 1);

    let c = call_at("foo((a + b), ‸)").unwrap();
    assert_eq!(c.positional_index, 1);
  }

  #[test]
  fn closures_do_not_split_args() {
    let c = call_at("map(|a, b| a + b, ‸xs)").unwrap();
    assert_eq!(c.fn_name, "map");
    assert_eq!(c.positional_index, 1);

    let c = call_at("map(|| 1, ‸xs)").unwrap();
    assert_eq!(c.positional_index, 1);

    let c = call_at("sweep(spine=|u| { v3(u, 0, 0) }, ‸)").unwrap();
    assert_eq!(c.positional_index, 0);
    assert_eq!(c.kwargs_before, vec!["spine".to_owned()]);

    let c = call_at("foo(a | bar(), ‸)").unwrap();
    assert_eq!(c.fn_name, "foo");
    assert_eq!(c.positional_index, 1);

    let c = call_at("foo(a || b, ‸)").unwrap();
    assert_eq!(c.positional_index, 1);
  }

  #[test]
  fn strings_comments_and_closers() {
    let c = call_at("foo(\"a, (b\", ‸)").unwrap();
    assert_eq!(c.positional_index, 1);
    assert!(call_at("foo(\"a, ‸b\")").is_none());
    assert!(call_at("foo(a) // comment (‸").is_none());
    assert!(call_at("foo(a) | ‸bar").is_none());
    assert!(call_at("x = 5 + ‸3").is_none());
    assert!(call_at("if (x‸) { 1 }").is_none());
    assert!(call_at("foo (‸)").is_none());
    let c = call_at("foo(a, // c(\n ‸b)").unwrap();
    assert_eq!(c.positional_index, 1);
  }

  #[test]
  fn global_sigil_and_path_block() {
    let c = call_at("@foo(‸)").unwrap();
    assert_eq!(c.fn_name, "foo");
    assert!(c.uses_global_sigil);
    assert_eq!(c.callee_offset, 0);

    let c = call_at("p = path {\n  move(v2(0, 0))\n  bezier(‸\n}").unwrap();
    assert_eq!(c.fn_name, "bezier");
    assert!(c.in_path_block);
    assert!(!call_at("foo(‸)").unwrap().in_path_block);
    assert!(in_path_block("path { x = [1] |", 16));
    assert!(!in_path_block("path { x } |", 11));
  }

  #[test]
  fn kwarg_hover() {
    let src = "translate(mesh, offset=vec3(1,0,0))";
    let (call, name) = kwarg_at(src, 18).unwrap();
    assert_eq!(call.fn_name, "translate");
    assert_eq!(name, "offset");
    assert!(kwarg_at(src, 3).is_none());
    assert!(kwarg_at("f(x == 1)", 2).is_none());
  }
}
