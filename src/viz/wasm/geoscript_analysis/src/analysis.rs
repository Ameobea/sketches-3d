use std::collections::hash_map::Entry;

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
  call_infer::{
    builtin_type, callable_param, resolve_call, resolve_literal_call, resolve_pipeline, value_type,
    CallResolution,
  },
  match_binop_by_arg_types, match_unop_by_arg_types,
  ty::{merge_types, AbstractType, CallableParam, CallableType},
  type_infer::{infer_map_op_result_type, refine_reduce_call, resolve_pipeline_call},
  ArgType, EvalCtx, Program, Sym, Value,
};

use crate::{
  scope::{
    DefinitionId, FunctionCallInfo, PipelineInput, SourceRange, SymbolDef, SymbolKind, SymbolRef,
  },
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
  /// Diagnostics emitted during the walk (type-hint mismatches, type errors at known
  /// builtin call sites).  The diagnostics module merges these with its own checks.
  pub diagnostics: Vec<AnalysisDiagnostic>,
}

impl Analysis {
  pub fn all_defs(&self) -> &[SymbolDef] {
    &self.defs
  }

  pub fn definition(&self, id: DefinitionId) -> &SymbolDef {
    &self.defs[id.0]
  }

  pub fn all_refs(&self) -> &[SymbolRef] {
    &self.refs
  }

  /// Definitions in scope at a given source position: those whose enclosing block/closure
  /// contains the position, and which were declared before it (bindings are sequential).
  ///
  /// Shadowed names collapse to the innermost definition.
  pub fn definitions_visible_at(
    &self,
    ctx: &EvalCtx,
    target_line: u32,
    target_col: u32,
  ) -> Vec<&SymbolDef> {
    let mut by_name: FxHashMap<Sym, &SymbolDef> = FxHashMap::default();
    for def in &self.defs {
      // Top-level bindings (including the prelude's and the ambient block's) are always in
      // scope.  Anything nested needs a resolved range around the cursor — an unresolved one
      // means the scope lives in the prelude, where the cursor can't be.
      if def.scope_depth > 0
        && !def
          .scope_range
          .is_some_and(|r| r.contains(target_line, target_col))
      {
        continue;
      }
      // A location of (0, 0) means "no user position" (prelude/ambient) — always in scope.
      let (line, col) = ctx.resolve_loc(def.loc);
      if (line, col) != (0, 0) && (line, col) > (target_line, target_col) {
        continue;
      }
      // The innermost scope wins; later definitions in that scope replace earlier ones.
      match by_name.entry(def.name) {
        Entry::Occupied(mut e) => {
          if def.scope_depth >= e.get().scope_depth {
            e.insert(def);
          }
        }
        Entry::Vacant(e) => {
          e.insert(def);
        }
      }
    }
    by_name.into_values().collect()
  }

  pub fn build(ctx: &EvalCtx, program: &Program) -> Self {
    let mut walker = AnalysisWalker::new(ctx);
    walker.walk_program(program);

    Analysis {
      defs: walker.defs,
      refs: walker.refs,
      unresolved_refs: walker.unresolved_refs,
      function_calls: walker.function_calls,
      diagnostics: walker.diagnostics,
    }
  }
}

struct ScopeBinding {
  ty: AbstractType,
  definition: Option<DefinitionId>,
}

struct ScopeFrame {
  bindings: FxHashMap<Sym, ScopeBinding>,
  depth: u32,
  /// Source extent of the block/closure that opened this frame; `None` for the top level.
  range: Option<SourceRange>,
}

struct AnalysisWalker<'a> {
  ctx: &'a EvalCtx,
  scope_stack: Vec<ScopeFrame>,
  defs: Vec<SymbolDef>,
  refs: Vec<SymbolRef>,
  unresolved_refs: Vec<SymbolRef>,
  function_calls: Vec<FunctionCallInfo>,
  diagnostics: Vec<AnalysisDiagnostic>,
  builtin_syms: FxHashSet<Sym>,
  /// Stack of in-progress closure bodies; populated while walking a closure body and read by
  /// `Statement::Return` to record exit types and validate against a declared return type.
  closure_return_stack: Vec<ClosureReturnContext>,
  /// Break-exit accumulation for enclosing explicit blocks; `None` entries are closure-body
  /// barriers (`break` cannot escape a closure).
  block_break_stack: Vec<Option<Vec<AbstractType>>>,
  /// Set just before walking a `name = |..| { .. }` binding's RHS so the closure can make its
  /// own name visible inside its body for recursive calls, mirroring the resolver's self-name
  /// handling.
  pending_recursive_binding: Option<(Sym, DefinitionId)>,
}

impl<'a> AnalysisWalker<'a> {
  fn new(ctx: &'a EvalCtx) -> Self {
    // `@`-prefixed variants are interned alongside the plain names: the `@` global sigil
    // is kept in the symbol, so refs like `@sin` must match here to avoid undefined-var
    // false positives.
    let mut builtin_syms = FxHashSet::default();
    for (name, _) in fn_sigs().entries() {
      builtin_syms.insert(ctx.interned_symbols.intern(name));
      builtin_syms.insert(ctx.interned_symbols.intern(&format!("@{name}")));
    }
    for (alias, _) in FUNCTION_ALIASES.entries() {
      builtin_syms.insert(ctx.interned_symbols.intern(alias));
      builtin_syms.insert(ctx.interned_symbols.intern(&format!("@{alias}")));
    }

    let mut initial_types = FxHashMap::default();
    for (name, val) in geoscript::get_default_globals() {
      let ty = AbstractType::Concrete(val.get_type());
      for name in [name.to_owned(), format!("@{name}")] {
        initial_types.insert(
          ctx.interned_symbols.intern(&name),
          ScopeBinding {
            ty: ty.clone(),
            definition: None,
          },
        );
      }
    }

    AnalysisWalker {
      ctx,
      scope_stack: vec![ScopeFrame {
        bindings: initial_types,
        depth: 0,
        range: None,
      }],
      defs: Vec::new(),
      refs: Vec::new(),
      unresolved_refs: Vec::new(),
      function_calls: Vec::new(),
      diagnostics: Vec::new(),
      builtin_syms,
      closure_return_stack: Vec::new(),
      block_break_stack: Vec::new(),
      pending_recursive_binding: None,
    }
  }

  fn current_depth(&self) -> u32 {
    self.scope_stack.last().map(|f| f.depth).unwrap_or(0)
  }

  fn push_scope(&mut self, start: SourceLoc, end: SourceLoc) {
    let depth = self.current_depth() + 1;
    self.scope_stack.push(ScopeFrame {
      bindings: FxHashMap::default(),
      depth,
      range: self.resolve_range(start, end),
    });
  }

  /// Resolve a scope's bounds to user-source coordinates.  `None` when either end is unknown —
  /// synthesized nodes and anything in the prelude/ambient region, none of which a cursor in the
  /// user's source can sit inside.
  fn resolve_range(&self, start: SourceLoc, end: SourceLoc) -> Option<SourceRange> {
    let (start_line, start_col) = self.ctx.resolve_loc(start);
    let (end_line, end_col) = self.ctx.resolve_loc(end);
    if (start_line, start_col) == (0, 0) || (end_line, end_col) == (0, 0) {
      return None;
    }
    Some(SourceRange {
      start_line,
      start_col,
      end_line,
      end_col,
    })
  }

  fn pop_scope(&mut self) {
    self.scope_stack.pop();
  }

  fn record_definition(
    &mut self,
    name: Sym,
    loc: SourceLoc,
    kind: SymbolKind,
  ) -> Option<DefinitionId> {
    if self.ctx.interned_symbols.is_synthetic(name) {
      return None;
    }
    let id = DefinitionId(self.defs.len());
    self.defs.push(SymbolDef {
      id,
      name,
      loc,
      ty: AbstractType::Unknown,
      kind,
      scope_depth: self.current_depth(),
      scope_range: self.scope_stack.last().and_then(|f| f.range),
    });
    Some(id)
  }

  fn bind_symbol(&mut self, name: Sym, definition: Option<DefinitionId>, ty: AbstractType) {
    if let Some(id) = definition {
      self.defs[id.0].ty = ty.clone();
    }
    self
      .scope_stack
      .last_mut()
      .unwrap()
      .bindings
      .insert(name, ScopeBinding { ty, definition });
  }

  fn define_symbol(&mut self, name: Sym, loc: SourceLoc, kind: SymbolKind, ty: AbstractType) {
    let definition = self.record_definition(name, loc, kind);
    self.bind_symbol(name, definition, ty);
  }

  fn lookup_binding(&self, name: Sym) -> Option<&ScopeBinding> {
    self
      .scope_stack
      .iter()
      .rev()
      .find_map(|frame| frame.bindings.get(&name))
  }

  fn lookup_type(&self, name: Sym) -> Option<&AbstractType> {
    self.lookup_binding(name).map(|binding| &binding.ty)
  }

  fn is_defined(&self, name: Sym) -> bool {
    self.lookup_binding(name).is_some()
  }

  fn reference_symbol(&mut self, name: Sym, loc: SourceLoc) {
    let resolved_def = self
      .lookup_binding(name)
      .and_then(|binding| binding.definition);
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

  /// Reserve the definition before walking its value, but only enter it in scope afterward.
  /// A closure can refer to its own reserved definition while its body is being analyzed.
  fn walk_binding(&mut self, name: Sym, loc: SourceLoc, expr: &Expr, type_hint: Option<&ArgType>) {
    let definition = self.record_definition(name, loc, SymbolKind::Variable);
    if matches!(expr, Expr::Closure { .. }) {
      self.pending_recursive_binding = definition.map(|id| (name, id));
    }
    let inferred = self.walk_expr(expr);
    let ty = self.resolve_with_hint(type_hint, &inferred, expr.loc(), &name);
    self.bind_symbol(name, definition, ty);
  }

  fn walk_top_level_statement(&mut self, stmt: &TopLevelStatement) {
    match stmt {
      TopLevelStatement::Statement(inner) => self.walk_statement(inner),
      TopLevelStatement::Export {
        name,
        name_loc,
        expr,
        type_hint,
        ..
      } => {
        self.walk_binding(*name, *name_loc, expr, type_hint.as_ref());
      }
      TopLevelStatement::Import { bindings, .. } => {
        self.define_destructure_pattern(bindings, SymbolKind::Import, AbstractType::Unknown);
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
        ..
      } => {
        self.walk_binding(*name, *name_loc, expr, type_hint.as_ref());
      }
      Statement::DestructureAssignment { lhs, rhs, .. } => {
        self.walk_expr(rhs);
        self.define_destructure_pattern(lhs, SymbolKind::Variable, AbstractType::Unknown);
      }
      Statement::Expr(expr) => {
        self.walk_expr(expr);
      }
      Statement::Return { value, .. } => {
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
      Statement::Break { value, .. } => {
        let exit_ty = match value {
          Some(expr) => self.walk_expr(expr),
          None => AbstractType::Concrete(ArgType::Nil),
        };
        if let Some(Some(exits)) = self.block_break_stack.last_mut() {
          exits.push(exit_ty);
        }
      }
    }
  }

  /// Walks a statement list: the trailing-expression type (Nil if none) plus whether the
  /// fall-through result is unreachable (list ends in `return`/`break`).
  fn walk_statement_list(&mut self, statements: &[Statement]) -> (AbstractType, bool) {
    let stmt_count = statements.len();
    let mut result = AbstractType::Concrete(ArgType::Nil);
    let mut unreachable = false;
    for (i, stmt) in statements.iter().enumerate() {
      if i + 1 == stmt_count {
        match stmt {
          Statement::Expr(expr) => {
            result = self.walk_expr(expr);
            continue;
          }
          Statement::Return { .. } | Statement::Break { .. } => unreachable = true,
          _ => {}
        }
      }
      self.walk_statement(stmt);
    }
    (result, unreachable)
  }

  /// Mirrors runtime branch transparency: a conditional branch block is NOT a break target,
  /// so `break` inside it contributes to the nearest enclosing explicit block instead.
  fn walk_branch_expr(&mut self, expr: &Expr) -> AbstractType {
    let Expr::Block {
      statements,
      loc,
      end_loc,
    } = expr
    else {
      return self.walk_expr(expr);
    };
    self.push_scope(*loc, *end_loc);
    let (result, unreachable) = self.walk_statement_list(statements);
    self.pop_scope();
    if unreachable {
      AbstractType::Unknown
    } else {
      result
    }
  }

  fn define_destructure_pattern(
    &mut self,
    pattern: &DestructurePattern,
    kind: SymbolKind,
    ty: AbstractType,
  ) {
    pattern.visit_ident_locs(&mut |sym, loc| {
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
      Expr::Literal { value, .. } => value_type(self.ctx, value),
      Expr::Ident { name, loc, .. } => {
        self.reference_symbol(*name, *loc);
        self
          .lookup_type(*name)
          .cloned()
          .or_else(|| builtin_type(self.ctx, *name))
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
      Expr::FieldAccess {
        lhs, field, field2, ..
      } => {
        let lhs_ty = self.walk_expr(lhs);
        let field_ty = self.walk_expr(field);
        let field2_ty = field2.as_ref().map(|f2| self.walk_expr(f2));
        let (Some(lhs_c), Some(field_c)) =
          (lhs_ty.as_single_arg_type(), field_ty.as_single_arg_type())
        else {
          return AbstractType::Unknown;
        };
        let field2_c = match field2_ty {
          Some(t) => match t.as_single_arg_type() {
            Some(t) => Some(t),
            None => return AbstractType::Unknown,
          },
          None => None,
        };
        infer_dynamic_field_access_ty(lhs_c, field, field_c, field2_c)
          .unwrap_or(AbstractType::Unknown)
      }
      Expr::Closure {
        params,
        body,
        return_type_hint,
        loc,
        end_loc,
        ..
      } => {
        let recursive_binding = self.pending_recursive_binding.take();
        // Defaults resolve against captures, before any parameter bindings exist.
        for param in params.iter() {
          if let Some(default) = &param.default_val {
            self.walk_expr(default);
          }
        }
        self.push_scope(*loc, *end_loc);
        let mut callable_params: Vec<CallableParam> = Vec::with_capacity(params.len());
        for param in params.iter() {
          let inferred_param = callable_param(self.ctx, param);
          let ty = inferred_param.ty.clone();
          callable_params.push(inferred_param);
          let binding_ty = match param.ident {
            DestructurePattern::Ident(..) => ty,
            _ => AbstractType::Unknown,
          };
          self.define_destructure_pattern(&param.ident, SymbolKind::ClosureParam, binding_ty);
        }

        if let Some((bind_name, definition)) = recursive_binding {
          let self_ty = AbstractType::Callable(CallableType {
            params: callable_params.clone(),
            return_type: Box::new(AbstractType::Unknown),
          });
          if let Some(frame) = self.scope_stack.last_mut() {
            frame.bindings.entry(bind_name).or_insert(ScopeBinding {
              ty: self_ty,
              definition: Some(definition),
            });
          }
        }

        let declared_arg: Option<ArgType> = *return_type_hint;
        self.closure_return_stack.push(ClosureReturnContext {
          declared: declared_arg,
          exit_types: Vec::new(),
        });
        self.block_break_stack.push(None);

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

        self.block_break_stack.pop();
        let closure_ctx = self.closure_return_stack.pop().expect("stack balanced");
        self.pop_scope();

        let return_ty = if let Some(declared) = declared_arg {
          AbstractType::Concrete(declared)
        } else {
          let mut exits = closure_ctx.exit_types.into_iter();
          let mut acc = if implicit_is_unreachable {
            exits.next().unwrap_or(AbstractType::Unknown)
          } else {
            implicit_return
          };
          for t in exits {
            acc = merge_types(&acc, &t);
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
          self.walk_expr(&el.expr);
        }
        AbstractType::Concrete(ArgType::Sequence)
      }
      Expr::MapLiteral { entries, .. } => {
        for entry in entries {
          match entry {
            MapLiteralEntry::KeyValue { value, .. } => {
              self.walk_expr(value);
            }
            MapLiteralEntry::Computed { key, value } => {
              self.walk_expr(key);
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
        let then_ty = self.walk_branch_expr(then);
        let mut branches: Vec<AbstractType> = vec![then_ty];
        for (c, e) in else_if_exprs {
          self.walk_expr(c);
          branches.push(self.walk_branch_expr(e));
        }
        if let Some(else_expr) = else_expr {
          branches.push(self.walk_branch_expr(else_expr));
        } else {
          // An `if` without `else` can fall through with no value.
          branches.push(AbstractType::Concrete(ArgType::Nil));
        }
        branches
          .into_iter()
          .reduce(|a, b| merge_types(&a, &b))
          .unwrap_or(AbstractType::Unknown)
      }
      Expr::Block {
        statements,
        loc,
        end_loc,
      } => {
        self.push_scope(*loc, *end_loc);
        self.block_break_stack.push(Some(Vec::new()));
        let (result, unreachable) = self.walk_statement_list(statements);
        let break_exits = self.block_break_stack.pop().unwrap().unwrap();
        self.pop_scope();
        // Unreachable fall-through: type comes solely from the break exits. A *reachable*
        // Unknown fall-through must stay Unknown (absorbing), so it can't be the sentinel.
        let mut exits = break_exits.into_iter();
        let mut acc = if unreachable {
          exits.next().unwrap_or(AbstractType::Unknown)
        } else {
          result
        };
        for t in exits {
          acc = merge_types(&acc, &t);
        }
        acc
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
        let lhs_ty = self.walk_expr(lhs);
        let rhs_ty = self.walk_expr(rhs);
        return infer_map_op_result_type(&lhs_ty, &rhs_ty);
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

  fn call_target_type(&self, name: Sym) -> AbstractType {
    self
      .lookup_type(name)
      .cloned()
      .or_else(|| builtin_type(self.ctx, name))
      .unwrap_or(AbstractType::Unknown)
  }

  fn record_call_resolution(
    &mut self,
    resolution: CallResolution,
    loc: SourceLoc,
    width: u32,
  ) -> (AbstractType, Option<usize>) {
    if let Some(message) = resolution.error {
      let (line, col) = self.ctx.resolve_loc(loc);
      if (line, col) != (0, 0) {
        self.diagnostics.push(AnalysisDiagnostic {
          start_line: line,
          start_col: col,
          end_line: line,
          end_col: col + width.max(1),
          severity: DiagnosticSeverity::Error,
          message,
        });
      }
    }
    (resolution.return_ty, resolution.matched_signature)
  }

  fn walk_pipeline(&mut self, lhs: &Expr, rhs: &Expr) -> AbstractType {
    let lhs_ty = self.walk_expr(lhs);
    let Expr::Call { call, loc } = rhs else {
      let rhs_ty = self.walk_expr(rhs);
      let resolution = resolve_pipeline(self.ctx, &lhs_ty, &rhs_ty);
      return self.record_call_resolution(resolution, rhs.loc(), 1).0;
    };
    self.walk_call(call, *loc, Some((lhs_ty, lhs)))
  }

  fn walk_function_call(&mut self, call: &FunctionCall, loc: SourceLoc) -> AbstractType {
    self.walk_call(call, loc, None)
  }

  /// Record editor facts once; all application and RHS-first pipeline semantics live in core.
  fn walk_call(
    &mut self,
    call: &FunctionCall,
    loc: SourceLoc,
    pipeline: Option<(AbstractType, &Expr)>,
  ) -> AbstractType {
    let arg_types: Vec<_> = call.args.iter().map(|arg| self.walk_expr(arg)).collect();
    let kwarg_types: Vec<_> = call
      .kwargs
      .iter()
      .map(|(name, expr)| (*name, self.walk_expr(expr)))
      .collect();
    let (target, mut resolution, name, is_shadowed) = match &call.target {
      FunctionCallTarget::Name(name) => {
        self.reference_symbol(*name, loc);
        let target = self.call_target_type(*name);
        let resolution = resolve_call(self.ctx, &target, &arg_types, &kwarg_types);
        (target, resolution, Some(*name), self.is_defined(*name))
      }
      FunctionCallTarget::Literal(callable) => {
        let resolution = resolve_literal_call(self.ctx, callable, &arg_types, &kwarg_types);
        (AbstractType::Unknown, resolution, None, false)
      }
    };
    refine_reduce_call(
      self.ctx,
      call,
      &target,
      &arg_types,
      |name| self.is_defined(name),
      &mut resolution,
    );
    let mut matched_sig_ix = resolution.matched_signature;
    if let Some((lhs_ty, _)) = &pipeline {
      if resolution.error.is_none() {
        let rhs_ty = resolution.return_ty;
        resolution = resolve_pipeline_call(self.ctx, call, lhs_ty, &rhs_ty, |name| {
          self.is_defined(name)
        });
        if matches!(rhs_ty, AbstractType::PartiallyApplied(_)) {
          matched_sig_ix = resolution.matched_signature;
        }
      }
    }
    if let Some(name) = name {
      self.function_calls.push(FunctionCallInfo {
        name,
        loc,
        arg_count: call.args.len(),
        kwarg_count: call.kwargs.len(),
        kwarg_names: call.kwargs.keys().copied().collect(),
        is_shadowed,
        matched_sig_ix,
        arg_types,
        kwarg_types,
        arg_is_ident: call
          .args
          .iter()
          .map(|arg| matches!(arg, Expr::Ident { .. }))
          .collect(),
        pipeline: pipeline.map(|(ty, lhs)| PipelineInput {
          ty,
          label: match lhs {
            Expr::Ident { name, .. } => self
              .ctx
              .interned_symbols
              .with_resolved(*name, str::to_owned)
              .unwrap_or_else(|| "expression".to_owned()),
            Expr::Literal {
              value: Value::Int(value),
              ..
            } => value.to_string(),
            Expr::Literal {
              value: Value::Float(value),
              ..
            } => value.to_string(),
            _ => "expression".to_owned(),
          },
        }),
      });
    }
    let width = name
      .and_then(|name| {
        self
          .ctx
          .interned_symbols
          .with_resolved(name, |s| s.len() as u32)
      })
      .unwrap_or(1);
    self.record_call_resolution(resolution, loc, width).0
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
}
