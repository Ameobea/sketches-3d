//! Shared forward type inference for the geoscript optimizer and the geoscript_analysis IDE
//! walker.  Produces an [`AbstractType`] for any expression in a given [`TypeEnv`].
//!
//! This module is pure — no diagnostics, no symbol-reference recording.  Callers layer their
//! own concerns (ref tracking, diagnostics, function-call recording) on top by consulting
//! [`crate::call_infer::resolve_call`] / [`crate::call_infer::resolve_pipeline`] alongside
//! their own walk.

use fxhash::FxHashMap;

use crate::{
  ast::{
    infer_dynamic_field_access_ty, infer_static_field_access_ty, BinOp, ClosureArg, ClosureBody,
    Expr, FunctionCall, FunctionCallTarget, MapLiteralEntry, ScopeTracker, Statement, TrackedValue,
  },
  builtins::{
    fn_defs::{fn_sigs, get_builtin_fn_sig_entry_ix},
    FUNCTION_ALIASES,
  },
  match_binop_by_arg_types, match_unop_by_arg_types,
  resolve::resolve_global,
  ty::{merge_types, AbstractType, CallableParam, CallableType},
  ArgType, EvalCtx, Sym,
};

/// Scope stack for type inference.  Owned frames hold bindings made during inference; on a
/// miss, lookup falls through to a borrowed optimizer [`ScopeTracker`] chain and then the
/// language's default globals (`pi`, `tau`).  Borrowing instead of snapshotting keeps an env
/// O(1) to build, which matters because the optimizer builds one per type query.
#[derive(Debug)]
pub struct TypeEnv<'a> {
  frames: Vec<FxHashMap<Sym, AbstractType>>,
  base: Option<&'a ScopeTracker<'a>>,
  ctx: &'a EvalCtx,
}

impl<'a> TypeEnv<'a> {
  pub fn new(ctx: &'a EvalCtx) -> Self {
    TypeEnv {
      frames: vec![FxHashMap::default()],
      base: None,
      ctx,
    }
  }

  pub(crate) fn with_base(ctx: &'a EvalCtx, base: &'a ScopeTracker<'a>) -> Self {
    TypeEnv {
      frames: vec![FxHashMap::default()],
      base: Some(base),
      ctx,
    }
  }

  pub fn push_scope(&mut self) {
    self.frames.push(FxHashMap::default());
  }

  pub fn pop_scope(&mut self) {
    self.frames.pop();
  }

  pub fn define(&mut self, name: Sym, ty: AbstractType) {
    if let Some(frame) = self.frames.last_mut() {
      frame.insert(name, ty);
    }
  }

  fn lookup_base(&self, name: Sym) -> Option<AbstractType> {
    let mut cur = self.base;
    while let Some(frame) = cur {
      if let Some(val) = frame.vars.get(&name) {
        return Some(match val {
          TrackedValue::Const(v) => value_type(self.ctx, v),
          TrackedValue::Arg | TrackedValue::Dyn => frame
            .types
            .get(&name)
            .cloned()
            .unwrap_or(AbstractType::Unknown),
        });
      }
      cur = frame.parent;
    }
    None
  }

  fn lookup_default_global(&self, name: Sym) -> Option<AbstractType> {
    let ctx = self.ctx;
    ctx
      .default_global_types
      .get_or_init(|| {
        crate::get_default_globals()
          .iter()
          .map(|(n, v)| (ctx.interned_symbols.intern(n), v.get_type()))
          .collect()
      })
      .iter()
      .find(|(sym, _)| *sym == name)
      .map(|(_, ty)| AbstractType::Concrete(*ty))
  }

  pub fn lookup(&self, name: Sym) -> Option<AbstractType> {
    for frame in self.frames.iter().rev() {
      if let Some(ty) = frame.get(&name) {
        return Some(ty.clone());
      }
    }
    self
      .lookup_base(name)
      .or_else(|| self.lookup_default_global(name))
  }

  pub fn contains(&self, name: Sym) -> bool {
    self.frames.iter().any(|f| f.contains_key(&name)) || self.lookup(name).is_some()
  }
}

use crate::call_infer::{
  builtin_type, callable_param, resolve_call, resolve_literal_call, resolve_pipeline, value_type,
  CallResolution,
};

/// Infer the abstract type of an expression in the given environment.  Mutates `env`
/// transiently for blocks / closures but restores it before returning.
pub fn infer_expr(ctx: &EvalCtx, env: &mut TypeEnv, expr: &Expr) -> AbstractType {
  match expr {
    Expr::Literal { value, .. } => value_type(ctx, value),

    Expr::Ident { name, .. } => match env.lookup(*name) {
      Some(ty) => ty,
      None => builtin_type(ctx, *name)
        .or_else(|| {
          ctx
            .interned_symbols
            .with_resolved(*name, |s| s.strip_prefix('@').and_then(resolve_global))
            .flatten()
            .map(|v| value_type(ctx, &v))
        })
        .unwrap_or(AbstractType::Unknown),
    },

    Expr::Call { call, .. } => infer_call_expr(ctx, env, call),

    Expr::BinOp { op, lhs, rhs, .. } => infer_binop(ctx, env, *op, lhs, rhs),

    Expr::PrefixOp {
      op, expr: inner, ..
    } => {
      let arg_ty = infer_expr(ctx, env, inner);
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
      infer_expr(ctx, env, start);
      if let Some(end) = end {
        infer_expr(ctx, env, end);
      }
      AbstractType::Concrete(ArgType::Sequence)
    }

    Expr::StaticFieldAccess { lhs, field, .. } => {
      let lhs_ty = infer_expr(ctx, env, lhs);
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
      let lhs_ty = infer_expr(ctx, env, lhs);
      let field_ty = infer_expr(ctx, env, field);
      let Some(lhs_c) = lhs_ty.as_single_arg_type() else {
        return AbstractType::Unknown;
      };
      let Some(field_c) = field_ty.as_single_arg_type() else {
        return AbstractType::Unknown;
      };
      let field2_c = match field2 {
        Some(f2) => match infer_expr(ctx, env, f2).as_single_arg_type() {
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
      ..
    } => infer_closure(ctx, env, params, body, *return_type_hint),

    Expr::ArrayLiteral { elements, .. } => {
      for el in elements {
        infer_expr(ctx, env, &el.expr);
      }
      AbstractType::Concrete(ArgType::Sequence)
    }

    Expr::MapLiteral { entries, .. } => {
      for entry in entries {
        match entry {
          MapLiteralEntry::KeyValue { value, .. } => {
            infer_expr(ctx, env, value);
          }
          MapLiteralEntry::Computed { key, value } => {
            infer_expr(ctx, env, key);
            infer_expr(ctx, env, value);
          }
          MapLiteralEntry::Splat { expr } => {
            infer_expr(ctx, env, expr);
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
      infer_expr(ctx, env, cond);
      let then_ty = infer_expr(ctx, env, then);
      let mut branches: Vec<AbstractType> = vec![then_ty];
      for (c, e) in else_if_exprs {
        infer_expr(ctx, env, c);
        branches.push(infer_expr(ctx, env, e));
      }
      if let Some(else_expr) = else_expr {
        branches.push(infer_expr(ctx, env, else_expr));
      } else {
        branches.push(AbstractType::Concrete(ArgType::Nil));
      }
      branches
        .into_iter()
        .reduce(|a, b| merge_types(&a, &b))
        .unwrap_or(AbstractType::Unknown)
    }

    Expr::Block { statements, .. } => {
      env.push_scope();
      let result = infer_statement_list(ctx, env, statements);
      env.pop_scope();
      result
    }
  }
}

/// A statement list's type is its trailing expression's (Nil if it doesn't end in one).
fn infer_statement_list(
  ctx: &EvalCtx,
  env: &mut TypeEnv,
  statements: &[Statement],
) -> AbstractType {
  let stmt_count = statements.len();
  let mut result = AbstractType::Concrete(ArgType::Nil);
  for (i, stmt) in statements.iter().enumerate() {
    if i + 1 == stmt_count {
      if let Statement::Expr(expr) = stmt {
        result = infer_expr(ctx, env, expr);
        continue;
      }
    }
    infer_statement(ctx, env, stmt);
  }
  result
}

/// Infer a statement's effect on the environment.  Bindings from assignments persist; the
/// statement itself has no return value.
pub fn infer_statement(ctx: &EvalCtx, env: &mut TypeEnv, stmt: &Statement) {
  match stmt {
    Statement::Assignment {
      name,
      expr,
      type_hint,
      ..
    } => {
      let inferred = infer_expr(ctx, env, expr);
      let ty = match type_hint {
        Some(hint) => AbstractType::Concrete(*hint),
        None => inferred,
      };
      env.define(*name, ty);
    }
    Statement::DestructureAssignment { lhs, rhs, .. } => {
      infer_expr(ctx, env, rhs);
      lhs.visit_idents(&mut |sym| env.define(sym, AbstractType::Unknown));
    }
    Statement::Expr(expr) => {
      infer_expr(ctx, env, expr);
    }
    Statement::Return { .. } | Statement::Break { .. } => {
      unreachable!("exits are desugared in `optimize_ast`")
    }
  }
}

/// `lhs -> rhs` lowers to `map(rhs, lhs)`. Yields a `Sequence`, except `mesh -> |v, n| {..}` is
/// `warp` shorthand and yields a `Mesh` — so resolve the actual `map` signature from the operands.
pub fn infer_map_op_result_type(lhs_ty: &AbstractType, rhs_ty: &AbstractType) -> AbstractType {
  let (Some(seq_c), Some(fn_c)) = (lhs_ty.as_single_arg_type(), rhs_ty.as_single_arg_type()) else {
    return AbstractType::Unknown;
  };
  let Some(entry_ix) = get_builtin_fn_sig_entry_ix("map") else {
    return AbstractType::Unknown;
  };
  match match_binop_by_arg_types(entry_ix, fn_c, seq_c) {
    Some((_def_ix, rt)) => AbstractType::from_return_type(rt),
    None => AbstractType::Unknown,
  }
}

/// Return type of a builtin (resolving aliases), merged across its signatures — a `Union` when
/// they disagree, so an ambiguous reducer stays permissive rather than falsely concrete.
pub fn builtin_fn_return_type(name: &str) -> AbstractType {
  let resolved = FUNCTION_ALIASES.get(name).copied().unwrap_or(name);
  let Some(def) = fn_sigs().get(resolved) else {
    return AbstractType::Unknown;
  };
  let mut merged: Option<AbstractType> = None;
  for sig in def.signatures {
    let rt = AbstractType::from_return_type(sig.return_type);
    merged = Some(match merged {
      Some(acc) => merge_types(&acc, &rt),
      None => rt,
    });
  }
  merged.unwrap_or(AbstractType::Unknown)
}

/// Return type of a reducer (its accumulator type). A bare builtin reference like `reduce(union)`
/// infers to `Unknown`, so its name is passed in `bare_builtin_name` and resolved separately.
fn reducer_result_type(reducer_ty: &AbstractType, bare_builtin_name: Option<&str>) -> AbstractType {
  match reducer_ty {
    AbstractType::Callable(ct) => (*ct.return_type).clone(),
    AbstractType::Builtin(name) => builtin_fn_return_type(name),
    AbstractType::PartiallyApplied(paf) => reducer_result_type(&paf.target, None),
    _ => match bare_builtin_name {
      Some(name) => builtin_fn_return_type(name),
      None => AbstractType::Unknown,
    },
  }
}

/// `reduce`/`fold` declare an `Any` return, but it equals the reducer's return type — so
/// `reduce(union, seq)` yields a `Mesh`. Returns `None` (no refinement) unless this is an
/// unshadowed, *complete* call; a bare `reduce(union)` partial application is left alone.
///
/// `total_positional_args` includes a value piped in via `|`.
pub fn infer_reduce_fold_result(
  ctx: &EvalCtx,
  call: &FunctionCall,
  arg_types: &[AbstractType],
  total_positional_args: usize,
  is_local: impl Fn(Sym) -> bool,
) -> Option<AbstractType> {
  let FunctionCallTarget::Name(name) = &call.target else {
    return None;
  };
  if is_local(*name) {
    return None;
  }
  let name_str = ctx
    .interned_symbols
    .with_resolved(*name, |s| s.strip_prefix('@').unwrap_or(s).to_owned())?;
  let canonical = FUNCTION_ALIASES
    .get(name_str.as_str())
    .copied()
    .unwrap_or(name_str.as_str());
  let (reducer_idx, min_args) = match canonical {
    "reduce" => (0usize, 2usize),
    "fold" => (1usize, 3usize),
    _ => return None,
  };
  if total_positional_args < min_args {
    return None;
  }
  let reducer_expr = call.args.get(reducer_idx)?;
  let reducer_ty = arg_types.get(reducer_idx)?;
  let bare_builtin_name = match reducer_expr {
    Expr::Ident {
      name: reducer_name, ..
    } if !is_local(*reducer_name) => ctx.interned_symbols.with_resolved(*reducer_name, |s| {
      s.strip_prefix('@').unwrap_or(s).to_owned()
    }),
    _ => None,
  };
  match reducer_result_type(reducer_ty, bare_builtin_name.as_deref()) {
    AbstractType::Unknown => None,
    rt => Some(rt),
  }
}

fn infer_binop(
  ctx: &EvalCtx,
  env: &mut TypeEnv,
  op: BinOp,
  lhs: &Expr,
  rhs: &Expr,
) -> AbstractType {
  match op {
    BinOp::Range | BinOp::RangeInclusive => {
      infer_expr(ctx, env, lhs);
      infer_expr(ctx, env, rhs);
      return AbstractType::Concrete(ArgType::Sequence);
    }
    BinOp::Map => {
      let lhs_ty = infer_expr(ctx, env, lhs);
      let rhs_ty = infer_expr(ctx, env, rhs);
      return infer_map_op_result_type(&lhs_ty, &rhs_ty);
    }
    BinOp::Pipeline => return infer_pipeline(ctx, env, lhs, rhs),
    _ => {}
  }
  let lhs_ty = infer_expr(ctx, env, lhs);
  let rhs_ty = infer_expr(ctx, env, rhs);
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

fn infer_pipeline(ctx: &EvalCtx, env: &mut TypeEnv, lhs: &Expr, rhs: &Expr) -> AbstractType {
  let lhs_ty = infer_expr(ctx, env, lhs);
  let rhs_ty = infer_expr(ctx, env, rhs);
  let resolution = match rhs {
    Expr::Call { call, .. } => {
      resolve_pipeline_call(ctx, call, &lhs_ty, &rhs_ty, |name| env.contains(name))
    }
    _ => resolve_pipeline(ctx, &lhs_ty, &rhs_ty),
  };
  resolution.into_abstract_type()
}

/// Piping into a partially applied builtin completes the call, so its reducer refinement applies.
pub fn resolve_pipeline_call(
  ctx: &EvalCtx,
  call: &FunctionCall,
  lhs: &AbstractType,
  rhs: &AbstractType,
  is_local: impl Fn(Sym) -> bool,
) -> CallResolution {
  let mut resolution = resolve_pipeline(ctx, lhs, rhs);
  if let AbstractType::PartiallyApplied(paf) = rhs {
    let mut args = paf.bound_args.clone();
    args.push(lhs.clone());
    refine_reduce_call(ctx, call, &paf.target, &args, is_local, &mut resolution);
  }
  resolution
}

/// Replaces an `Unknown` return of a builtin `reduce`/`fold` call with the reducer's own.
pub fn refine_reduce_call(
  ctx: &EvalCtx,
  call: &FunctionCall,
  target: &AbstractType,
  args: &[AbstractType],
  is_local: impl Fn(Sym) -> bool,
  resolution: &mut CallResolution,
) {
  if resolution.error.is_none()
    && matches!(resolution.return_ty, AbstractType::Unknown)
    && matches!(target, AbstractType::Builtin(_))
  {
    if let Some(ty) = infer_reduce_fold_result(ctx, call, args, args.len(), is_local) {
      resolution.return_ty = ty;
    }
  }
}

fn infer_call_expr(ctx: &EvalCtx, env: &mut TypeEnv, call: &FunctionCall) -> AbstractType {
  let args: Vec<_> = call
    .args
    .iter()
    .map(|arg| infer_expr(ctx, env, arg))
    .collect();
  let kwargs: Vec<_> = call
    .kwargs
    .iter()
    .map(|(name, expr)| (*name, infer_expr(ctx, env, expr)))
    .collect();
  match &call.target {
    FunctionCallTarget::Name(name) => {
      let target = env
        .lookup(*name)
        .or_else(|| builtin_type(ctx, *name))
        .unwrap_or(AbstractType::Unknown);
      let mut resolution = resolve_call(ctx, &target, &args, &kwargs);
      refine_reduce_call(
        ctx,
        call,
        &target,
        &args,
        |name| env.contains(name),
        &mut resolution,
      );
      resolution.into_abstract_type()
    }
    FunctionCallTarget::Literal(callable) => {
      resolve_literal_call(ctx, callable, &args, &kwargs).into_abstract_type()
    }
  }
}

fn infer_closure(
  ctx: &EvalCtx,
  env: &mut TypeEnv,
  params: &[ClosureArg],
  body: &ClosureBody,
  return_type_hint: Option<ArgType>,
) -> AbstractType {
  // The resolver binds defaults in the captured environment, before parameters enter scope.
  for param in params {
    if let Some(default) = &param.default_val {
      infer_expr(ctx, env, default);
    }
  }
  env.push_scope();
  let mut callable_params: Vec<CallableParam> = Vec::with_capacity(params.len());
  for param in params {
    let inferred_param = callable_param(ctx, param);
    let ty = inferred_param.ty.clone();
    callable_params.push(inferred_param);
    let binding_ty = match param.ident {
      crate::ast::DestructurePattern::Ident(..) => ty,
      _ => AbstractType::Unknown,
    };
    param
      .ident
      .visit_idents(&mut |sym| env.define(sym, binding_ty.clone()));
  }

  let implicit_return = infer_statement_list(ctx, env, &body.0);
  env.pop_scope();

  let return_ty = match return_type_hint {
    Some(declared) => AbstractType::Concrete(declared),
    None => implicit_return,
  };

  AbstractType::Callable(CallableType {
    params: callable_params,
    return_type: Box::new(return_ty),
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::ast::TopLevelStatement;
  use crate::parse_program_src;

  /// Inference runs on desugared trees only (the pass is the first step of `optimize_ast`),
  /// so exercise it the way production does.
  fn infer_first_rhs(src: &str) -> AbstractType {
    let ctx = EvalCtx::default();
    let mut ast = parse_program_src(&ctx, src).unwrap();
    crate::desugar::desugar_exits(&ctx, &mut ast).unwrap();
    let TopLevelStatement::Statement(Statement::Assignment { expr, .. }) = &ast.statements[0]
    else {
      panic!("expected assignment statement");
    };
    let mut env = TypeEnv::new(&ctx);
    infer_expr(&ctx, &mut env, expr)
  }

  #[test]
  fn block_types_merge_break_exits() {
    // Break through a transparent branch merges with the fall-through type.
    let ty = format!(
      "{:?}",
      infer_first_rhs("v = {\n  if 1 > 0 { break 1 }\n  2.5\n}")
    );
    assert!(ty.contains("Int") && ty.contains("Float"), "got {ty}");

    // Tail break IS the block's value.
    let ty = format!("{:?}", infer_first_rhs("v = { break 'done' }"));
    assert!(ty.contains("String") && !ty.contains("Nil"), "got {ty}");

    // Closure bodies are a break barrier: a break inside a nested closure's own block cannot
    // reach the outer block. (A break with no block inside the closure is a static error —
    // see `desugar::tests::rejects_exits_in_operands`.)
    let ty = format!(
      "{:?}",
      infer_first_rhs("v = {\n  f = |x| { { if x { break 1.5 }\n  2 } }\n  9\n}")
    );
    assert!(ty.contains("Int") && !ty.contains("Float"), "got {ty}");

    // A reachable-Unknown fall-through absorbs break exits — it must NOT be narrowed to
    // the break type (only an unreachable fall-through takes its type from the exits).
    let ty = format!(
      "{:?}",
      infer_first_rhs("f = |g, c| {\n  v = {\n    if c { break 1 }\n    g(0)\n  }\n  v\n}")
    );
    assert!(ty.contains("Unknown") || !ty.contains("Int"), "got {ty}");
  }
}
