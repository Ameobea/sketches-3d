use geoscript::{ast::SourceLoc, Sym};

/// What kind of symbol definition this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
  Variable,
  ClosureParam,
  Import,
}

/// A symbol definition found during analysis.
#[derive(Clone, Debug)]
pub struct SymbolDef {
  pub name: Sym,
  pub loc: SourceLoc,
  pub kind: SymbolKind,
  /// Scope depth where this was defined (0 = top-level).
  pub scope_depth: u32,
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
