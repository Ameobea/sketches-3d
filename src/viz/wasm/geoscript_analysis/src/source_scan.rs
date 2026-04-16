/// Source-text-level helpers for detecting context that isn't easily available from the AST
/// (e.g. "am I inside a function call's argument list?" or "is this word a kwarg name?").
///
/// These work on raw source bytes and do not depend on a successful parse, making them
/// useful during live editing when the AST may be incomplete.

/// Result of scanning for the enclosing function call.
#[derive(Debug)]
pub struct EnclosingCallInfo {
  /// The identifier immediately before the opening `(`.
  pub fn_name: String,
  /// If the cursor is on a kwarg name (an identifier followed by `=` but not `==`).
  pub kwarg_name: Option<String>,
}

/// Convert 1-based (line, col) to a byte offset in `src`.
/// Returns `None` if the position is out of range.
pub fn line_col_to_offset(src: &str, line: u32, col: u32) -> Option<usize> {
  if line == 0 || col == 0 {
    return None;
  }
  let mut current_line = 1u32;
  let mut line_start = 0usize;
  for (i, ch) in src.char_indices() {
    if current_line == line {
      let offset = line_start + (col as usize - 1);
      return if offset <= src.len() { Some(offset) } else { None };
    }
    if ch == '\n' {
      current_line += 1;
      line_start = i + 1;
    }
  }
  if current_line == line {
    let offset = line_start + (col as usize - 1);
    if offset <= src.len() { Some(offset) } else { None }
  } else {
    None
  }
}

fn is_ident_byte(b: u8) -> bool {
  b.is_ascii_alphanumeric() || b == b'_'
}

/// If the cursor is on a kwarg name (identifier followed by `=` but not `==`), return
/// the kwarg name and its byte range.
pub fn detect_kwarg_at(src: &str, offset: usize) -> Option<String> {
  let bytes = src.as_bytes();
  if offset > bytes.len() {
    return None;
  }

  // Find the word containing/adjacent to the offset
  let mut word_start = offset;
  while word_start > 0 && is_ident_byte(bytes[word_start - 1]) {
    word_start -= 1;
  }
  let mut word_end = offset;
  while word_end < bytes.len() && is_ident_byte(bytes[word_end]) {
    word_end += 1;
  }
  if word_start == word_end {
    return None;
  }

  // Check if followed by `=` but not `==`
  let mut after = word_end;
  // skip whitespace
  while after < bytes.len() && bytes[after] == b' ' {
    after += 1;
  }
  if after >= bytes.len() || bytes[after] != b'=' {
    return None;
  }
  if after + 1 < bytes.len() && bytes[after + 1] == b'=' {
    return None; // `==` is comparison, not kwarg assignment
  }

  Some(src[word_start..word_end].to_string())
}

/// Scan backwards from `offset` to find the enclosing function call.
///
/// Returns the function name and, if applicable, the kwarg name at the cursor position.
/// This is a heuristic based on paren-balancing — it doesn't handle parens inside
/// string literals or comments, but works well for typical code.
pub fn find_enclosing_call(src: &str, offset: usize) -> Option<EnclosingCallInfo> {
  let bytes = src.as_bytes();
  if offset > bytes.len() {
    return None;
  }

  let kwarg_name = detect_kwarg_at(src, offset);

  // Scan backwards, balancing parens to find the opening `(`
  let mut depth: i32 = 0;
  let mut i = offset.min(bytes.len());
  while i > 0 {
    i -= 1;
    match bytes[i] {
      b')' => depth += 1,
      b'(' => {
        if depth == 0 {
          // Found the opening paren — extract the identifier before it
          let fn_name = extract_ident_before(src, i)?;
          return Some(EnclosingCallInfo { fn_name, kwarg_name });
        }
        depth -= 1;
      }
      _ => {}
    }
  }

  None
}

/// Extract the identifier immediately before position `pos` (skipping whitespace).
fn extract_ident_before(src: &str, pos: usize) -> Option<String> {
  let bytes = src.as_bytes();
  let mut end = pos;
  // Skip whitespace between identifier and `(`
  while end > 0 && bytes[end - 1] == b' ' {
    end -= 1;
  }
  if end == 0 || !is_ident_byte(bytes[end - 1]) {
    return None;
  }
  let mut start = end;
  while start > 0 && is_ident_byte(bytes[start - 1]) {
    start -= 1;
  }
  // Don't treat bare numbers as function names
  if bytes[start].is_ascii_digit() {
    return None;
  }
  Some(src[start..end].to_string())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_line_col_to_offset() {
    let src = "abc\ndef\nghi";
    assert_eq!(line_col_to_offset(src, 1, 1), Some(0));
    assert_eq!(line_col_to_offset(src, 1, 3), Some(2));
    assert_eq!(line_col_to_offset(src, 2, 1), Some(4));
    assert_eq!(line_col_to_offset(src, 3, 2), Some(9));
  }

  #[test]
  fn test_detect_kwarg_simple() {
    let src = "translate(mesh, offset=vec3(1,0,0))";
    // cursor on 'o' of 'offset'
    let result = detect_kwarg_at(src, 16);
    assert_eq!(result.as_deref(), Some("offset"));
  }

  #[test]
  fn test_detect_kwarg_not_comparison() {
    let src = "x == 5";
    let result = detect_kwarg_at(src, 0);
    assert_eq!(result, None);
  }

  #[test]
  fn test_find_enclosing_call() {
    let src = "translate(mesh, offset=vec3(1,0,0))";
    // cursor inside the call, on 'offset'
    let info = find_enclosing_call(src, 16).unwrap();
    assert_eq!(info.fn_name, "translate");
    assert_eq!(info.kwarg_name.as_deref(), Some("offset"));
  }

  #[test]
  fn test_find_enclosing_call_nested() {
    let src = "foo(bar(x), baz=1)";
    // cursor on 'baz'
    let info = find_enclosing_call(src, 12).unwrap();
    assert_eq!(info.fn_name, "foo");
    assert_eq!(info.kwarg_name.as_deref(), Some("baz"));
  }

  #[test]
  fn test_find_enclosing_call_inner() {
    let src = "foo(bar(x, y))";
    // cursor on 'y' inside bar()
    let info = find_enclosing_call(src, 11).unwrap();
    assert_eq!(info.fn_name, "bar");
    assert_eq!(info.kwarg_name, None);
  }

  #[test]
  fn test_no_enclosing_call() {
    let src = "x = 5 + 3";
    let info = find_enclosing_call(src, 4);
    assert!(info.is_none());
  }
}
