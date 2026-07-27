use geoscript::{ast::SourceLoc, Sym};

/// What kind of symbol definition this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
  Variable,
  ClosureParam,
  Import,
}

/// Source extent of a block or closure, in user-source coordinates.
#[derive(Clone, Copy, Debug)]
pub struct SourceRange {
  pub start_line: u32,
  pub start_col: u32,
  pub end_line: u32,
  pub end_col: u32,
}

impl SourceRange {
  /// Half-open at the end: `end_line`/`end_col` point just past the closing brace, so a cursor
  /// sitting there has already left the scope.
  pub fn contains(&self, line: u32, col: u32) -> bool {
    (line, col) >= (self.start_line, self.start_col) && (line, col) < (self.end_line, self.end_col)
  }
}

/// A symbol definition found during analysis.
#[derive(Clone, Debug)]
pub struct SymbolDef {
  pub name: Sym,
  pub loc: SourceLoc,
  pub kind: SymbolKind,
  /// Scope depth where this was defined (0 = top-level).
  pub scope_depth: u32,
  /// Extent of the block/closure this was defined in; `None` at the top level, and for scopes
  /// whose bounds couldn't be resolved (e.g. anything inside the prelude).
  pub scope_range: Option<SourceRange>,
}

/// A symbol reference (usage) found during analysis.
#[derive(Clone, Debug)]
pub struct SymbolRef {
  pub name: Sym,
  pub loc: SourceLoc,
  /// The definition location this reference resolves to, if any.
  pub resolved_def: Option<SourceLoc>,
}

/// Information about a function call for argument checking.
#[derive(Clone, Debug)]
pub struct FunctionCallInfo {
  pub name: Sym,
  pub loc: SourceLoc,
  pub arg_count: usize,
  pub kwarg_count: usize,
  /// The interned names of kwargs provided at this call site.
  pub kwarg_names: Vec<Sym>,
  /// Whether the function name is shadowed by a local definition.
  pub is_shadowed: bool,
  /// Index of the signature that matched at this call site, if any.  `None` when no exact
  /// match could be determined (shadowed call, Unknown arg types, partial application, or
  /// no overload matches).
  pub matched_sig_ix: Option<usize>,
}
