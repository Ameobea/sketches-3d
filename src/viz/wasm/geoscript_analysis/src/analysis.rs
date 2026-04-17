use fxhash::{FxHashMap, FxHashSet};
use geoscript::{
  ast::{
    infer_dynamic_field_access_ty, infer_static_field_access_ty, BinOp, DestructurePattern, Expr,
    FunctionCall, FunctionCallTarget, MapLiteralEntry, SourceLoc, Statement, TopLevelStatement,
  },
  builtins::{
    fn_defs::{fn_sigs, get_builtin_fn_sig_entry_ix},
    FUNCTION_ALIASES,
  },
  match_binop_by_arg_types, match_unop_by_arg_types,
  ty::{merge_types, AbstractType, CallableParam, CallableType, PartialApplication},
  type_infer::{resolve_builtin_call, resolve_paf_call, CallResolution},
  ArgType, EvalCtx, Program, Sym,
};

use crate::{
  scope::{FunctionCallInfo, SymbolDef, SymbolKind, SymbolRef},
  AnalysisDiagnostic, DiagnosticSeverity,
};

/// Tracks state for a closure body we're currently walking, so `Statement::Return` inside
/// nested blocks / conditionals can validate against and contribute to the closure's return type.
struct ClosureReturnContext {
  /// Explicit return type annotation (e.g. `|x|: int { ... }` → Some(Int)).
  declared: Option<ArgType>,
  /// Types observed at explicit `return ...` statements.
  exit_types: Vec<AbstractType>,
}

/// Combined scope + type analysis result for a program.
///
/// Produced in a single AST traversal by `AnalysisWalker`.  Callers (hover, completions,
/// goto, diagnostics) consume this via the accessors below.
pub struct Analysis {
  defs: Vec<SymbolDef>,
  refs: Vec<SymbolRef>,
  pub unresolved_refs: Vec<SymbolRef>,
  pub function_calls: Vec<FunctionCallInfo>,
  /// Inferred type at each definition site, keyed by the definition's SourceLoc.
  pub def_types: FxHashMap<SourceLoc, AbstractType>,
  /// Diagnostics emitted during the walk (type-hint mismatches, type errors at known
  /// builtin call sites).  The diagnostics module merges these with its own checks.
  pub diagnostics: Vec<AnalysisDiagnostic>,
}

impl Analysis {
  pub fn all_defs(&self) -> &[SymbolDef] {
    &self.defs
  }

  pub fn all_refs(&self) -> &[SymbolRef] {
    &self.refs
  }

  /// Return definitions that are visible at a given source position.
  pub fn definitions_visible_at(&self, _target_line: u32, _target_col: u32) -> Vec<&SymbolDef> {
    // TODO: track scope ranges for proper visibility filtering
    self.defs.iter().collect()
  }

  pub fn build(ctx: &EvalCtx, program: &Program) -> Self {
    let mut walker = AnalysisWalker::new(ctx);
    walker.walk_program(program);

    Analysis {
      defs: walker.defs,
      refs: walker.refs,
      unresolved_refs: walker.unresolved_refs,
      function_calls: walker.function_calls,
      def_types: walker.def_types,
      diagnostics: walker.diagnostics,
    }
  }
}

struct ScopeFrame {
  /// Symbols defined in this frame and their inferred types.
  types: FxHashMap<Sym, AbstractType>,
  depth: u32,
}

struct AnalysisWalker<'a> {
  ctx: &'a EvalCtx,
  scope_stack: Vec<ScopeFrame>,
  defs: Vec<SymbolDef>,
  refs: Vec<SymbolRef>,
  unresolved_refs: Vec<SymbolRef>,
  function_calls: Vec<FunctionCallInfo>,
  def_types: FxHashMap<SourceLoc, AbstractType>,
  diagnostics: Vec<AnalysisDiagnostic>,
  builtin_syms: FxHashSet<Sym>,
  /// Stack of in-progress closure bodies; populated while walking a closure body and read by
  /// `Statement::Return` to record exit types and validate against a declared return type.
  closure_return_stack: Vec<ClosureReturnContext>,
}

impl<'a> AnalysisWalker<'a> {
  fn new(ctx: &'a EvalCtx) -> Self {
    let mut builtin_syms = FxHashSet::default();
    for (name, _) in fn_sigs().entries() {
      builtin_syms.insert(ctx.interned_symbols.intern(name));
    }
    for (alias, _) in FUNCTION_ALIASES.entries() {
      builtin_syms.insert(ctx.interned_symbols.intern(alias));
    }

    let mut initial_types = FxHashMap::default();
    for (name, val) in geoscript::get_default_globals() {
      let sym = ctx.interned_symbols.intern(name);
      initial_types.insert(sym, AbstractType::Concrete(val.get_type()));
    }

    AnalysisWalker {
      ctx,
      scope_stack: vec![ScopeFrame {
        types: initial_types,
        depth: 0,
      }],
      defs: Vec::new(),
      refs: Vec::new(),
      unresolved_refs: Vec::new(),
      function_calls: Vec::new(),
      def_types: FxHashMap::default(),
      diagnostics: Vec::new(),
      builtin_syms,
      closure_return_stack: Vec::new(),
    }
  }

  fn current_depth(&self) -> u32 {
    self.scope_stack.last().map(|f| f.depth).unwrap_or(0)
  }

  fn push_scope(&mut self) {
    let depth = self.current_depth() + 1;
    self.scope_stack.push(ScopeFrame {
      types: FxHashMap::default(),
      depth,
    });
  }

  fn pop_scope(&mut self) {
    self.scope_stack.pop();
  }

  fn define_symbol(&mut self, name: Sym, loc: SourceLoc, kind: SymbolKind, ty: AbstractType) {
    if let Some(frame) = self.scope_stack.last_mut() {
      frame.types.insert(name, ty.clone());
    }
    self.defs.push(SymbolDef {
      name,
      loc,
      kind,
      scope_depth: self.current_depth(),
    });
    if loc != SourceLoc::default() {
      self.def_types.insert(loc, ty);
    }
  }

  fn lookup_type(&self, name: Sym) -> Option<&AbstractType> {
    for frame in self.scope_stack.iter().rev() {
      if let Some(ty) = frame.types.get(&name) {
        return Some(ty);
      }
    }
    None
  }

  fn is_defined(&self, name: Sym) -> bool {
    self.scope_stack.iter().any(|f| f.types.contains_key(&name))
  }

  fn find_def_loc(&self, name: Sym) -> Option<SourceLoc> {
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
        type_hint,
      } => {
        let inferred = self.walk_expr(expr);
        let ty = self.resolve_with_hint(type_hint.as_ref(), &inferred, expr.loc(), name);
        self.define_symbol(*name, *name_loc, SymbolKind::Variable, ty);
      }
      TopLevelStatement::Import {
        bindings,
        module_name: _,
      } => {
        self.define_destructure_pattern(
          bindings,
          SourceLoc::default(),
          SymbolKind::Import,
          AbstractType::Unknown,
        );
      }
    }
  }

  fn walk_statement(&mut self, stmt: &Statement) {
    match stmt {
      Statement::Assignment {
        name,
        name_loc,
        expr,
        type_hint,
      } => {
        let inferred = self.walk_expr(expr);
        let ty = self.resolve_with_hint(type_hint.as_ref(), &inferred, expr.loc(), name);
        self.define_symbol(*name, *name_loc, SymbolKind::Variable, ty);
      }
      Statement::DestructureAssignment { lhs, rhs } => {
        self.walk_expr(rhs);
        self.define_destructure_pattern(
          lhs,
          rhs.loc(),
          SymbolKind::Variable,
          AbstractType::Unknown,
        );
      }
      Statement::Expr(expr) => {
        self.walk_expr(expr);
      }
      Statement::Return { value } => {
        let exit_ty = match value {
          Some(expr) => self.walk_expr(expr),
          None => AbstractType::Concrete(ArgType::Nil),
        };
        let declared = self.closure_return_stack.last().and_then(|c| c.declared);
        if let Some(ctx) = self.closure_return_stack.last_mut() {
          ctx.exit_types.push(exit_ty.clone());
        }
        if let Some(declared_ty) = declared {
          let loc = value
            .as_ref()
            .map(|e| e.loc())
            .unwrap_or(SourceLoc::default());
          self.validate_return_against_declared(&exit_ty, declared_ty, loc);
        }
      }
      Statement::Break { value } => {
        if let Some(expr) = value {
          self.walk_expr(expr);
        }
      }
    }
  }

  fn define_destructure_pattern(
    &mut self,
    pattern: &DestructurePattern,
    loc: SourceLoc,
    kind: SymbolKind,
    ty: AbstractType,
  ) {
    pattern.visit_idents(&mut |sym| {
      self.define_symbol(sym, loc, kind, ty.clone());
    });
  }

  /// Resolve the binding type given an optional type hint and the inferred RHS type.
  /// Emits a diagnostic if the hint and inferred type are incompatible concrete types.
  fn resolve_with_hint(
    &mut self,
    type_hint: Option<&ArgType>,
    inferred: &AbstractType,
    rhs_loc: SourceLoc,
    name_sym: &Sym,
  ) -> AbstractType {
    let Some(hint) = type_hint else {
      return inferred.clone();
    };
    let hint_arg: ArgType = *hint;
    if let Some(inferred_concrete) = inferred.as_single_arg_type() {
      if hint_arg.as_bitflags() & inferred_concrete.as_bitflags() == 0 {
        let (line, col) = self.ctx.resolve_loc(rhs_loc);
        if line != 0 || col != 0 {
          let name_str = self
            .ctx
            .interned_symbols
            .with_resolved(*name_sym, |s| s.to_string())
            .unwrap_or_default();
          self.diagnostics.push(AnalysisDiagnostic {
            start_line: line,
            start_col: col,
            end_line: line,
            end_col: col + 1,
            severity: DiagnosticSeverity::Error,
            message: format!(
              "type mismatch: `{name_str}` was annotated as `{}` but the value has type `{}`",
              hint_arg.as_str(),
              inferred_concrete.as_str()
            ),
          });
        }
      }
    }
    AbstractType::Concrete(hint_arg)
  }

  /// Walk an expression, returning its inferred abstract type.
  fn walk_expr(&mut self, expr: &Expr) -> AbstractType {
    match expr {
      Expr::Literal { value, .. } => AbstractType::Concrete(value.get_type()),
      Expr::Ident { name, loc } => {
        self.reference_symbol(*name, *loc);
        self
          .lookup_type(*name)
          .cloned()
          .unwrap_or(AbstractType::Unknown)
      }
      Expr::Call { call, loc } => self.walk_function_call(call, *loc),
      Expr::BinOp { op, lhs, rhs, .. } => self.walk_binop(*op, lhs, rhs),
      Expr::PrefixOp {
        op, expr: inner, ..
      } => {
        let arg_ty = self.walk_expr(inner);
        let Some(arg_concrete) = arg_ty.as_single_arg_type() else {
          return AbstractType::Unknown;
        };
        let Some(entry_ix) = get_builtin_fn_sig_entry_ix(op.get_builtin_fn_name()) else {
          return AbstractType::Unknown;
        };
        match match_unop_by_arg_types(entry_ix, arg_concrete) {
          Some(rt) => AbstractType::from_return_type(rt),
          None => AbstractType::Unknown,
        }
      }
      Expr::Range { start, end, .. } => {
        self.walk_expr(start);
        if let Some(end) = end {
          self.walk_expr(end);
        }
        AbstractType::Concrete(ArgType::Sequence)
      }
      Expr::StaticFieldAccess { lhs, field, .. } => {
        let lhs_ty = self.walk_expr(lhs);
        let Some(lhs_c) = lhs_ty.as_single_arg_type() else {
          return AbstractType::Unknown;
        };
        infer_static_field_access_ty(lhs_c, field)
          .map(AbstractType::Concrete)
          .unwrap_or(AbstractType::Unknown)
      }
      Expr::FieldAccess { lhs, field, .. } => {
        let lhs_ty = self.walk_expr(lhs);
        let field_ty = self.walk_expr(field);
        let (Some(lhs_c), Some(field_c)) =
          (lhs_ty.as_single_arg_type(), field_ty.as_single_arg_type())
        else {
          return AbstractType::Unknown;
        };
        infer_dynamic_field_access_ty(lhs_c, field, field_c)
          .map(AbstractType::Concrete)
          .unwrap_or(AbstractType::Unknown)
      }
      Expr::Closure {
        params,
        body,
        return_type_hint,
        loc,
        ..
      } => {
        self.push_scope();
        let mut callable_params: Vec<CallableParam> = Vec::with_capacity(params.len());
        for param in params.iter() {
          let ty = match &param.type_hint {
            Some(hint) => AbstractType::Concrete(*hint),
            None => AbstractType::Unknown,
          };
          let name = first_ident_name(&param.ident, self.ctx);
          callable_params.push(CallableParam {
            name,
            ty: ty.clone(),
          });
          self.define_destructure_pattern(&param.ident, *loc, SymbolKind::ClosureParam, ty);
          if let Some(default) = &param.default_val {
            self.walk_expr(default);
          }
        }

        let declared_arg: Option<ArgType> = *return_type_hint;
        self.closure_return_stack.push(ClosureReturnContext {
          declared: declared_arg,
          exit_types: Vec::new(),
        });

        // Walk body; if the last statement is `Statement::Expr`, its type is the implicit
        // return.  If it's a `Return`, the implicit tail is unreachable — skip validation.
        let mut implicit_return = AbstractType::Concrete(ArgType::Nil);
        let mut implicit_return_loc: SourceLoc = *loc;
        let mut implicit_is_unreachable = false;
        let stmt_count = body.0.len();
        for (i, stmt) in body.0.iter().enumerate() {
          let is_last = i + 1 == stmt_count;
          if is_last {
            match stmt {
              Statement::Expr(expr) => {
                implicit_return = self.walk_expr(expr);
                implicit_return_loc = expr.loc();
                continue;
              }
              Statement::Return { .. } => {
                implicit_is_unreachable = true;
              }
              _ => {}
            }
          }
          self.walk_statement(stmt);
        }

        if let Some(declared_ty) = declared_arg {
          if !implicit_is_unreachable {
            self.validate_return_against_declared(
              &implicit_return,
              declared_ty,
              implicit_return_loc,
            );
          }
        }

        let closure_ctx = self.closure_return_stack.pop().expect("stack balanced");
        self.pop_scope();

        let return_ty = if let Some(declared) = declared_arg {
          AbstractType::Concrete(declared)
        } else {
          let mut acc = if implicit_is_unreachable {
            AbstractType::Unknown
          } else {
            implicit_return
          };
          for t in &closure_ctx.exit_types {
            acc = match &acc {
              AbstractType::Unknown => t.clone(),
              _ => merge_types(&acc, t),
            };
          }
          acc
        };

        AbstractType::Callable(CallableType {
          params: callable_params,
          return_type: Box::new(return_ty),
        })
      }
      Expr::ArrayLiteral { elements, .. } => {
        for el in elements {
          self.walk_expr(el);
        }
        AbstractType::Concrete(ArgType::Sequence)
      }
      Expr::MapLiteral { entries, .. } => {
        for entry in entries {
          match entry {
            MapLiteralEntry::KeyValue { value, .. } => {
              self.walk_expr(value);
            }
            MapLiteralEntry::Splat { expr } => {
              self.walk_expr(expr);
            }
          }
        }
        AbstractType::Concrete(ArgType::Map)
      }
      Expr::Conditional {
        cond,
        then,
        else_if_exprs,
        else_expr,
        ..
      } => {
        self.walk_expr(cond);
        let then_ty = self.walk_expr(then);
        let mut branches: Vec<AbstractType> = vec![then_ty];
        for (c, e) in else_if_exprs {
          self.walk_expr(c);
          branches.push(self.walk_expr(e));
        }
        if let Some(else_expr) = else_expr {
          branches.push(self.walk_expr(else_expr));
        } else {
          // An `if` without `else` can fall through with no value.
          branches.push(AbstractType::Concrete(ArgType::Nil));
        }
        branches
          .into_iter()
          .reduce(|a, b| merge_types(&a, &b))
          .unwrap_or(AbstractType::Unknown)
      }
      Expr::Block { statements, .. } => {
        self.push_scope();
        let stmt_count = statements.len();
        let mut result = AbstractType::Concrete(ArgType::Nil);
        for (i, stmt) in statements.iter().enumerate() {
          let is_last = i + 1 == stmt_count;
          if is_last {
            if let Statement::Expr(expr) = stmt {
              result = self.walk_expr(expr);
              continue;
            }
          }
          self.walk_statement(stmt);
        }
        self.pop_scope();
        result
      }
    }
  }

  fn walk_binop(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> AbstractType {
    match op {
      BinOp::Range | BinOp::RangeInclusive => {
        self.walk_expr(lhs);
        self.walk_expr(rhs);
        return AbstractType::Concrete(ArgType::Sequence);
      }
      BinOp::Map => {
        self.walk_expr(lhs);
        self.walk_expr(rhs);
        return AbstractType::Concrete(ArgType::Sequence);
      }
      BinOp::Pipeline => {
        return self.walk_pipeline(lhs, rhs);
      }
      _ => {}
    }

    let lhs_ty = self.walk_expr(lhs);
    let rhs_ty = self.walk_expr(rhs);

    let Some(name) = op.get_builtin_fn_name() else {
      return AbstractType::Unknown;
    };
    let (Some(lhs_c), Some(rhs_c)) = (lhs_ty.as_single_arg_type(), rhs_ty.as_single_arg_type())
    else {
      return AbstractType::Unknown;
    };
    let Some(entry_ix) = get_builtin_fn_sig_entry_ix(name) else {
      return AbstractType::Unknown;
    };
    match match_binop_by_arg_types(entry_ix, lhs_c, rhs_c) {
      Some((_def_ix, rt)) => AbstractType::from_return_type(rt),
      None => AbstractType::Unknown,
    }
  }

  /// Pipeline operator `lhs | rhs` evaluates rhs as a callable and invokes it with lhs prepended
  /// to its arguments.  We model this for type inference by treating `lhs | f(args)` as
  /// `f(lhs, args)` and `lhs | f` as `f(lhs)` for builtin-name targets.
  fn walk_pipeline(&mut self, lhs: &Expr, rhs: &Expr) -> AbstractType {
    let lhs_ty = self.walk_expr(lhs);

    match rhs {
      Expr::Call { call, loc } => {
        let arg_types: Vec<AbstractType> = call.args.iter().map(|a| self.walk_expr(a)).collect();
        let kwarg_types: Vec<(Sym, AbstractType)> = call
          .kwargs
          .iter()
          .map(|(&sym, expr)| (sym, self.walk_expr(expr)))
          .collect();

        if let FunctionCallTarget::Name(name) = &call.target {
          let is_shadowed = self.is_defined(*name);
          if !is_shadowed {
            self.reference_symbol(*name, *loc);
          }

          // Pipeline semantics match `PartiallyAppliedFn::invoke`: the lhs is appended after
          // the already-provided positional args.  So `lhs | f(a, b)` becomes `f(a, b, lhs)`.
          let mut piped: Vec<AbstractType> = Vec::with_capacity(arg_types.len() + 1);
          piped.extend(arg_types);
          piped.push(lhs_ty);

          let (return_ty, matched_sig_ix) = if is_shadowed {
            // Shadowing local — try resolving as a PAF or typed-callable user var.
            let var_ty = self.lookup_type(*name).cloned();
            match var_ty {
              Some(AbstractType::PartiallyApplied(paf)) => (
                self.resolve_paf_call_with_diagnostics(&paf, &piped, &kwarg_types, *loc),
                None,
              ),
              Some(AbstractType::Callable(ct)) => ((*ct.return_type).clone(), None),
              _ => (AbstractType::Unknown, None),
            }
          } else {
            self.resolve_builtin_call_with_diagnostics(*name, &piped, &kwarg_types, *loc)
          };

          self.function_calls.push(FunctionCallInfo {
            name: *name,
            loc: *loc,
            arg_count: call.args.len(),
            kwarg_count: call.kwargs.len(),
            kwarg_names: call.kwargs.keys().copied().collect(),
            is_shadowed,
            matched_sig_ix,
          });

          return return_ty;
        }

        AbstractType::Unknown
      }
      Expr::Ident { name, loc } => {
        self.reference_symbol(*name, *loc);
        // Treat `lhs | name` as `name(lhs)` for builtins, PAFs, or typed closure vars.
        if self.builtin_syms.contains(name) {
          let (return_ty, _) =
            self.resolve_builtin_call_with_diagnostics(*name, &[lhs_ty], &[], *loc);
          return return_ty;
        }
        let var_ty = self.lookup_type(*name).cloned();
        match var_ty {
          Some(AbstractType::PartiallyApplied(paf)) => {
            self.resolve_paf_call_with_diagnostics(&paf, &[lhs_ty], &[], *loc)
          }
          Some(AbstractType::Callable(ct)) => (*ct.return_type).clone(),
          _ => AbstractType::Unknown,
        }
      }
      _ => {
        self.walk_expr(rhs);
        AbstractType::Unknown
      }
    }
  }

  fn walk_function_call(&mut self, call: &FunctionCall, loc: SourceLoc) -> AbstractType {
    let arg_types: Vec<AbstractType> = call.args.iter().map(|a| self.walk_expr(a)).collect();
    let kwarg_types: Vec<(Sym, AbstractType)> = call
      .kwargs
      .iter()
      .map(|(&sym, expr)| (sym, self.walk_expr(expr)))
      .collect();

    match &call.target {
      FunctionCallTarget::Name(name) => {
        let is_shadowed = self.is_defined(*name);
        if !is_shadowed {
          self.reference_symbol(*name, loc);
        }

        let (return_ty, matched_sig_ix) = if is_shadowed {
          // Shadowing local — try resolving as a PAF or typed-callable user var.
          let var_ty = self.lookup_type(*name).cloned();
          match var_ty {
            Some(AbstractType::PartiallyApplied(paf)) => (
              self.resolve_paf_call_with_diagnostics(&paf, &arg_types, &kwarg_types, loc),
              None,
            ),
            Some(AbstractType::Callable(ct)) => ((*ct.return_type).clone(), None),
            _ => (AbstractType::Unknown, None),
          }
        } else {
          self.resolve_builtin_call_with_diagnostics(*name, &arg_types, &kwarg_types, loc)
        };

        self.function_calls.push(FunctionCallInfo {
          name: *name,
          loc,
          arg_count: call.args.len(),
          kwarg_count: call.kwargs.len(),
          kwarg_names: call.kwargs.keys().copied().collect(),
          is_shadowed,
          matched_sig_ix,
        });

        return_ty
      }
      FunctionCallTarget::Literal(_) => AbstractType::Unknown,
    }
  }

  /// Validate an exit type (explicit `return` or implicit trailing expression) against the
  /// declared return type of the enclosing closure.  Emits a diagnostic when the concrete
  /// type(s) of `actual` don't fit within `declared`.  No-ops when `actual` is Unknown or
  /// otherwise non-concrete — we can't prove a mismatch in that case.
  fn validate_return_against_declared(
    &mut self,
    actual: &AbstractType,
    declared: ArgType,
    loc: SourceLoc,
  ) {
    let bad = match actual {
      AbstractType::Concrete(t) => {
        if declared.as_bitflags() & t.as_bitflags() == 0 {
          Some(*t)
        } else {
          None
        }
      }
      AbstractType::Union(types) => types
        .iter()
        .copied()
        .find(|t| declared.as_bitflags() & t.as_bitflags() == 0),
      _ => None,
    };
    let Some(bad_ty) = bad else {
      return;
    };
    let (line, col) = self.ctx.resolve_loc(loc);
    if line == 0 && col == 0 {
      return;
    }
    self.diagnostics.push(AnalysisDiagnostic {
      start_line: line,
      start_col: col,
      end_line: line,
      end_col: col + 1,
      severity: DiagnosticSeverity::Error,
      message: format!(
        "return type mismatch: expected `{}` but value has type `{}`",
        declared.as_str(),
        bad_ty.as_str()
      ),
    });
  }

  /// Resolve a call into an existing partial application, delegating matching to the shared
  /// `type_infer::resolve_paf_call`.  Emits a "no overload" diagnostic on `NoMatch`; otherwise
  /// returns the resulting abstract type (resolved return type, refined PAF, or Unknown).
  fn resolve_paf_call_with_diagnostics(
    &mut self,
    paf: &PartialApplication,
    new_pos: &[AbstractType],
    new_kwargs: &[(Sym, AbstractType)],
    call_loc: SourceLoc,
  ) -> AbstractType {
    let resolution = resolve_paf_call(paf, new_pos, new_kwargs);
    if let CallResolution::NoMatch {
      canonical_name,
      concrete_args,
      concrete_kwargs,
    } = &resolution
    {
      self.emit_no_overload_diagnostic(canonical_name, concrete_args, concrete_kwargs, call_loc);
    }
    resolution.into_abstract_type()
  }

  /// Resolve a builtin call via the shared `type_infer::resolve_builtin_call`, emitting a
  /// diagnostic on `NoMatch` and extracting the matched signature index from `Matched`.
  fn resolve_builtin_call_with_diagnostics(
    &mut self,
    name: Sym,
    arg_types: &[AbstractType],
    kwarg_types: &[(Sym, AbstractType)],
    call_loc: SourceLoc,
  ) -> (AbstractType, Option<usize>) {
    if !self.builtin_syms.contains(&name) {
      return (AbstractType::Unknown, None);
    }

    let resolution = resolve_builtin_call(self.ctx, name, arg_types, kwarg_types);
    let matched_sig_ix = match &resolution {
      CallResolution::Matched { def_ix, .. } => Some(*def_ix),
      _ => None,
    };
    if let CallResolution::NoMatch {
      canonical_name,
      concrete_args,
      concrete_kwargs,
    } = &resolution
    {
      self.emit_no_overload_diagnostic(canonical_name, concrete_args, concrete_kwargs, call_loc);
    }
    (resolution.into_abstract_type(), matched_sig_ix)
  }

  fn emit_no_overload_diagnostic(
    &mut self,
    name: &str,
    concrete_args: &[ArgType],
    concrete_kwargs: &[(Sym, ArgType)],
    call_loc: SourceLoc,
  ) {
    let (line, col) = self.ctx.resolve_loc(call_loc);
    if line == 0 && col == 0 {
      return;
    }
    self.diagnostics.push(AnalysisDiagnostic {
      start_line: line,
      start_col: col,
      end_line: line,
      end_col: col + name.len() as u32,
      severity: DiagnosticSeverity::Error,
      message: format_no_signature_match_msg(name, concrete_args, concrete_kwargs),
    });
  }
}

/// Extract the display name for a closure parameter from its destructure pattern — the single
/// ident name when the pattern is just `Ident(sym)`, otherwise None (destructured patterns
/// don't have one name we can surface on hover).
fn first_ident_name(pat: &DestructurePattern, ctx: &EvalCtx) -> Option<String> {
  if let DestructurePattern::Ident(sym) = pat {
    ctx.interned_symbols.with_resolved(*sym, |s| s.to_string())
  } else {
    None
  }
}

fn format_no_signature_match_msg(
  name: &str,
  positional: &[ArgType],
  kwargs: &[(Sym, ArgType)],
) -> String {
  let pos_part: Vec<&str> = positional.iter().map(|t| t.as_str()).collect();
  let kw_part: Vec<String> = kwargs
    .iter()
    .map(|(_, t)| format!("_={}", t.as_str()))
    .collect();
  let mut all = pos_part.join(", ");
  if !kw_part.is_empty() {
    if !all.is_empty() {
      all.push_str(", ");
    }
    all.push_str(&kw_part.join(", "));
  }
  format!("no overload of `{name}` matches argument types ({all})")
}
