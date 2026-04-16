use fxhash::FxHashSet;
use geoscript::{
  ast::{
    DestructurePattern, Expr, FunctionCall, FunctionCallTarget, MapLiteralEntry, SourceLoc,
    Statement, TopLevelStatement,
  },
  builtins::{fn_defs::fn_sigs, FUNCTION_ALIASES},
  EvalCtx, Program, Sym,
};

/// What kind of symbol definition this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
  Variable,
  ClosureParam,
  Import,
}

/// A symbol definition found during scope analysis.
#[derive(Clone, Debug)]
pub struct SymbolDef {
  pub name: Sym,
  pub loc: SourceLoc,
  pub kind: SymbolKind,
  /// Scope depth where this was defined (0 = top-level).
  pub scope_depth: u32,
}

/// A symbol reference (usage) found during scope analysis.
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
}

/// Result of scope analysis on a program.
pub struct ScopeAnalysis {
  /// All symbol definitions found.
  defs: Vec<SymbolDef>,
  /// All symbol references found.
  refs: Vec<SymbolRef>,
  /// References that could not be resolved to any definition or builtin.
  pub unresolved_refs: Vec<SymbolRef>,
  /// All function calls found (for argument checking).
  pub function_calls: Vec<FunctionCallInfo>,
}

impl ScopeAnalysis {
  pub fn all_defs(&self) -> &[SymbolDef] {
    &self.defs
  }

  pub fn all_refs(&self) -> &[SymbolRef] {
    &self.refs
  }

  /// Return definitions that are visible at a given source position.
  /// This is an approximation: we return all defs whose scope contains the target
  /// position (definitions that appear before the target line/col at the same or
  /// outer scope depth).
  pub fn definitions_visible_at(&self, _target_line: u32, _target_col: u32) -> Vec<&SymbolDef> {
    // For now, return all definitions.
    // TODO: track scope ranges for proper visibility filtering
    self.defs.iter().collect()
  }

  pub fn build(ctx: &EvalCtx, program: &Program) -> Self {
    let mut walker = ScopeWalker::new(ctx);
    walker.walk_program(program);

    ScopeAnalysis {
      defs: walker.defs,
      refs: walker.refs,
      unresolved_refs: walker.unresolved_refs,
      function_calls: walker.function_calls,
    }
  }
}

/// Tracks what's defined in the current scope during the AST walk.
struct ScopeFrame {
  /// Symbols defined in this scope frame.
  defined: FxHashSet<Sym>,
  depth: u32,
}

struct ScopeWalker<'a> {
  #[allow(dead_code)]
  ctx: &'a EvalCtx,
  /// Stack of scope frames.  The last element is the current innermost scope.
  scope_stack: Vec<ScopeFrame>,
  /// All definitions collected.
  defs: Vec<SymbolDef>,
  /// All references collected.
  refs: Vec<SymbolRef>,
  /// References that didn't resolve to anything.
  unresolved_refs: Vec<SymbolRef>,
  /// Function calls collected for arg checking.
  function_calls: Vec<FunctionCallInfo>,
  /// Set of builtin function names (interned) for quick lookup.
  builtin_syms: FxHashSet<Sym>,
}

impl<'a> ScopeWalker<'a> {
  fn new(ctx: &'a EvalCtx) -> Self {
    let mut builtin_syms = FxHashSet::default();
    for (name, _) in fn_sigs().entries() {
      builtin_syms.insert(ctx.interned_symbols.intern(name));
    }
    for (alias, _) in FUNCTION_ALIASES.entries() {
      builtin_syms.insert(ctx.interned_symbols.intern(alias));
    }

    // Also add default globals (pi, tau)
    let mut initial_defined = FxHashSet::default();
    initial_defined.insert(ctx.interned_symbols.intern("pi"));
    initial_defined.insert(ctx.interned_symbols.intern("tau"));

    ScopeWalker {
      ctx,
      scope_stack: vec![ScopeFrame {
        defined: initial_defined,
        depth: 0,
      }],
      defs: Vec::new(),
      refs: Vec::new(),
      unresolved_refs: Vec::new(),
      function_calls: Vec::new(),
      builtin_syms,
    }
  }

  fn current_depth(&self) -> u32 {
    self.scope_stack.last().map(|f| f.depth).unwrap_or(0)
  }

  fn push_scope(&mut self) {
    let depth = self.current_depth() + 1;
    self.scope_stack.push(ScopeFrame {
      defined: FxHashSet::default(),
      depth,
    });
  }

  fn pop_scope(&mut self) {
    self.scope_stack.pop();
  }

  fn define_symbol(&mut self, name: Sym, loc: SourceLoc, kind: SymbolKind) {
    if let Some(frame) = self.scope_stack.last_mut() {
      frame.defined.insert(name);
    }
    self.defs.push(SymbolDef {
      name,
      loc,
      kind,
      scope_depth: self.current_depth(),
    });
  }

  /// Check if a symbol is defined in any enclosing scope.
  fn is_defined(&self, name: Sym) -> bool {
    for frame in self.scope_stack.iter().rev() {
      if frame.defined.contains(&name) {
        return true;
      }
    }
    false
  }

  /// Check if a symbol is defined in a local scope (not just builtins/globals).
  fn is_locally_defined(&self, name: Sym) -> bool {
    self.is_defined(name)
  }

  /// Find the definition location for a symbol by searching scopes outward.
  fn find_def_loc(&self, name: Sym) -> Option<SourceLoc> {
    // Search definitions in reverse order (most recent first)
    // and within scope hierarchy
    for def in self.defs.iter().rev() {
      if def.name == name {
        return Some(def.loc);
      }
    }
    None
  }

  fn reference_symbol(&mut self, name: Sym, loc: SourceLoc) {
    let resolved_def = self.find_def_loc(name);
    let is_defined = self.is_defined(name);
    let is_builtin = self.builtin_syms.contains(&name);

    let sym_ref = SymbolRef {
      name,
      loc,
      resolved_def,
    };

    self.refs.push(sym_ref.clone());

    if !is_defined && !is_builtin {
      self.unresolved_refs.push(sym_ref);
    }
  }

  fn walk_program(&mut self, program: &Program) {
    for stmt in &program.statements {
      self.walk_top_level_statement(stmt);
    }
  }

  fn walk_top_level_statement(&mut self, stmt: &TopLevelStatement) {
    match stmt {
      TopLevelStatement::Statement(inner) => self.walk_statement(inner),
      TopLevelStatement::Export {
        name,
        name_loc,
        expr,
        type_hint: _,
      } => {
        self.walk_expr(expr);
        self.define_symbol(*name, *name_loc, SymbolKind::Variable);
      }
      TopLevelStatement::Import {
        bindings,
        module_name: _,
      } => {
        self.define_destructure_pattern(bindings, SourceLoc::default(), SymbolKind::Import);
      }
    }
  }

  fn walk_statement(&mut self, stmt: &Statement) {
    match stmt {
      Statement::Assignment {
        name,
        name_loc,
        expr,
        type_hint: _,
      } => {
        // Walk the RHS first (the variable isn't in scope during its own initialization)
        self.walk_expr(expr);
        self.define_symbol(*name, *name_loc, SymbolKind::Variable);
      }
      Statement::DestructureAssignment { lhs, rhs } => {
        self.walk_expr(rhs);
        self.define_destructure_pattern(lhs, rhs.loc(), SymbolKind::Variable);
      }
      Statement::Expr(expr) => {
        self.walk_expr(expr);
      }
      Statement::Return { value } => {
        if let Some(expr) = value {
          self.walk_expr(expr);
        }
      }
      Statement::Break { value } => {
        if let Some(expr) = value {
          self.walk_expr(expr);
        }
      }
    }
  }

  fn define_destructure_pattern(&mut self, pattern: &DestructurePattern, loc: SourceLoc, kind: SymbolKind) {
    pattern.visit_idents(&mut |sym| {
      self.define_symbol(sym, loc, kind);
    });
  }

  fn walk_expr(&mut self, expr: &Expr) {
    match expr {
      Expr::Ident { name, loc } => {
        self.reference_symbol(*name, *loc);
      }
      Expr::Call { call, loc } => {
        self.walk_function_call(call, *loc);
      }
      Expr::BinOp { lhs, rhs, .. } => {
        self.walk_expr(lhs);
        self.walk_expr(rhs);
      }
      Expr::PrefixOp { expr: inner, .. } => {
        self.walk_expr(inner);
      }
      Expr::Range { start, end, .. } => {
        self.walk_expr(start);
        if let Some(end) = end {
          self.walk_expr(end);
        }
      }
      Expr::StaticFieldAccess { lhs, .. } => {
        self.walk_expr(lhs);
      }
      Expr::FieldAccess { lhs, field, .. } => {
        self.walk_expr(lhs);
        self.walk_expr(field);
      }
      Expr::Closure {
        params,
        body,
        loc,
        ..
      } => {
        self.push_scope();
        // Define closure parameters
        for param in params.iter() {
          self.define_destructure_pattern(&param.ident, *loc, SymbolKind::ClosureParam);
          // Walk default values in the *outer* scope — but for simplicity we do it here
          // since default values typically reference outer scope variables which are
          // already defined.  This is a minor inaccuracy.
          if let Some(default) = &param.default_val {
            self.walk_expr(default);
          }
        }
        // Walk closure body
        for stmt in &body.0 {
          self.walk_statement(stmt);
        }
        self.pop_scope();
      }
      Expr::ArrayLiteral { elements, .. } => {
        for el in elements {
          self.walk_expr(el);
        }
      }
      Expr::MapLiteral { entries, .. } => {
        for entry in entries {
          match entry {
            MapLiteralEntry::KeyValue { value, .. } => self.walk_expr(value),
            MapLiteralEntry::Splat { expr } => self.walk_expr(expr),
          }
        }
      }
      Expr::Literal { .. } => {}
      Expr::Conditional {
        cond,
        then,
        else_if_exprs,
        else_expr,
        ..
      } => {
        self.walk_expr(cond);
        self.walk_expr(then);
        for (cond, expr) in else_if_exprs {
          self.walk_expr(cond);
          self.walk_expr(expr);
        }
        if let Some(else_expr) = else_expr {
          self.walk_expr(else_expr);
        }
      }
      Expr::Block { statements, .. } => {
        self.push_scope();
        for stmt in statements {
          self.walk_statement(stmt);
        }
        self.pop_scope();
      }
    }
  }

  fn walk_function_call(&mut self, call: &FunctionCall, loc: SourceLoc) {
    // Walk arguments first
    for arg in &call.args {
      self.walk_expr(arg);
    }
    for kwarg in call.kwargs.values() {
      self.walk_expr(kwarg);
    }

    // Record the call for arg checking
    match &call.target {
      FunctionCallTarget::Name(name) => {
        let is_shadowed = self.is_locally_defined(*name);
        if !is_shadowed {
          // Also record as a reference
          self.reference_symbol(*name, loc);
        }

        self.function_calls.push(FunctionCallInfo {
          name: *name,
          loc,
          arg_count: call.args.len(),
          kwarg_count: call.kwargs.len(),
          kwarg_names: call.kwargs.keys().copied().collect(),
          is_shadowed,
        });
      }
      FunctionCallTarget::Literal(_) => {
        // Inline callable — nothing to resolve
      }
    }
  }
}
