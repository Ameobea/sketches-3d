use std::{cell::RefCell, hash::Hash, ops::ControlFlow, rc::Rc};

use fxhash::FxHashMap;
use siphasher::sip128::{Hasher128, SipHasher};

use crate::{
  ast::{
    eval_range, get_dyn_type, maybe_pre_resolve_bulitin_call_signature, pre_resolve_binop_def_ix,
    pre_resolve_expr_type, BinOp, ClosureArg, ClosureBody, DestructurePattern, DynType, Expr,
    FunctionCall, FunctionCallTarget, MapLiteralEntry, ScopeTracker, Statement, TrackedValue,
    TrackedValueRef, TypeName,
  },
  builtins::{
    fn_defs::{fn_sigs, get_builtin_fn_sig_entry_ix},
    resolve_builtin_impl,
    trace_path::TRACE_PATH_DRAW_COMMAND_NAMES,
    FUNCTION_ALIASES,
  },
  seq::EagerSeq,
  ArgType, Callable, CapturedScope, Closure, ErrorStack, EvalCtx, Program, Scope, Sym, Value,
};

/// This is essentially a `-ffast-math` flag for the optimizer's constant folding of associative
/// operations.  It allows re-ordering of floating point operations which can technically create
/// slightly different results in some cases.  For almost everything done in Geoscript/Geotoy, it's
/// unlikely to matter though.
const FLOAT_ASSOC_FOLDING_ENABLED: bool = true;

fn add_trace_path_draw_commands(ctx: &EvalCtx, scope: &mut ScopeTracker) {
  for name in TRACE_PATH_DRAW_COMMAND_NAMES {
    let sym = ctx.interned_symbols.intern(name);
    if !scope.has(sym) {
      scope.set(
        sym,
        TrackedValue::Const(Value::Callable(Rc::new(Callable::Closure(Closure {
          params: Rc::new(Vec::new()),
          body: Rc::new(ClosureBody(Vec::new())),
          captured_scope: CapturedScope::Strong(Rc::new(Scope::default())),
          arg_placeholder_scope: RefCell::new(None),
          return_type_hint: None,
        })))),
      );
    }
  }
}

fn is_trace_path_callable(callable: &Callable) -> bool {
  match callable {
    Callable::Builtin { fn_entry_ix, .. } => fn_sigs().entries[*fn_entry_ix].0 == "trace_path",
    _ => false,
  }
}

#[derive(Clone, Copy)]
struct ConstEvalCacheLookup {
  key: u128,
  uses_rng: bool,
}

fn const_eval_cache_lookup_with(
  ctx: &EvalCtx,
  allow_rng_const_eval: bool,
  hash_fn: impl FnOnce(&mut SipHasher, &mut bool) -> Option<()>,
) -> Option<ConstEvalCacheLookup> {
  let mut hasher = SipHasher::new_with_keys(0, 0);
  let mut uses_rng = false;
  hash_fn(&mut hasher, &mut uses_rng)?;
  if uses_rng {
    if !allow_rng_const_eval {
      return None;
    }
    let rng_state = ctx.rng_state();
    hash_rng_state(&mut hasher, &rng_state);
  }
  let key = hasher.finish128().as_u128();
  Some(ConstEvalCacheLookup { key, uses_rng })
}

fn const_eval_cache_get(ctx: &EvalCtx, lookup: ConstEvalCacheLookup) -> Option<Value> {
  let hit = ctx.const_eval_cache.borrow_mut().get(lookup.key)?;
  if let Some(rng_end_state) = hit.rng_end_state {
    ctx.set_rng_state(rng_end_state);
  }
  Some(hit.value)
}

fn const_eval_cache_store(ctx: &EvalCtx, lookup: ConstEvalCacheLookup, value: Value) {
  let rng_end_state = if lookup.uses_rng {
    Some(ctx.rng_state())
  } else {
    None
  };
  ctx
    .const_eval_cache
    .borrow_mut()
    .insert(lookup.key, value, rng_end_state);
}

fn can_const_eval_callable(callable: &Callable, allow_rng_const_eval: bool) -> bool {
  if callable.is_rng_dependent() {
    return allow_rng_const_eval;
  }
  if callable.is_side_effectful() {
    return false;
  }
  if allow_rng_const_eval {
    return true;
  }
  is_known_rng_free_callable(callable)
}

fn is_known_rng_free_callable(callable: &Callable) -> bool {
  match callable {
    Callable::Builtin { .. } => !callable.is_side_effectful() && !callable.is_rng_dependent(),
    Callable::PartiallyAppliedFn(paf) => is_known_rng_free_callable(&paf.inner),
    Callable::ComposedFn(composed) => composed
      .inner
      .iter()
      .all(|callable| is_known_rng_free_callable(&*callable)),
    Callable::Closure(_) => false,
    Callable::Dynamic { inner, .. } => !inner.is_side_effectful() && !inner.is_rng_dependent(),
  }
}

fn is_trace_path_closure_effectively_const(
  ctx: &EvalCtx,
  local_scope: &mut ScopeTracker,
  expr: &mut Expr,
  allow_rng_const_eval: bool,
) -> Result<bool, ErrorStack> {
  // we construct a new scope for optimizing the body which contains fake trace path functions that
  // resolve to const callables
  let mut fake_scope = ScopeTracker::wrap(local_scope);
  add_trace_path_draw_commands(ctx, &mut fake_scope);

  let mut test_expr = expr.clone();
  optimize_expr(ctx, &mut fake_scope, &mut test_expr, allow_rng_const_eval)?;

  // if the closure was literalized successfully, we need to swap back the closure body to the
  // original one so the draw commands actually do something and then attempt to re-optimize it so
  // as many optimizations as possibly can apply
  if let Expr::Literal(Value::Callable(callable)) = test_expr {
    if matches!(callable.as_ref(), Callable::Closure(_)) {
      if let Expr::Closure { params, .. } = expr {
        if !params.is_empty() {
          return Err(ErrorStack::new(
            "Trace path closures should have no parameters",
          ));
        }

        return Ok(true);
      }
    }
  }

  Ok(false)
}

/// Special-case for the `trace_path` builtin to allow constifying its closure argument which
/// contains effectful calls.
///
/// The effects of those calls are limited to the function call, so it's safe to constify them.
fn maybe_constify_trace_path_closure_arg(
  ctx: &EvalCtx,
  local_scope: &mut ScopeTracker,
  args: &mut [Expr],
  kwargs: &mut FxHashMap<Sym, Expr>,
  allow_rng_const_eval: bool,
) -> Result<(), ErrorStack> {
  let cb_sym = ctx.interned_symbols.intern("cb");
  let (is_effectively_const, expr_opt) = if let Some(expr) = kwargs.get_mut(&cb_sym) {
    (
      is_trace_path_closure_effectively_const(ctx, local_scope, expr, allow_rng_const_eval)?,
      Some(expr),
    )
  } else if let Some(expr) = args.get_mut(0) {
    (
      is_trace_path_closure_effectively_const(ctx, local_scope, expr, allow_rng_const_eval)?,
      Some(expr),
    )
  } else {
    (false, None)
  };

  // since the closure with the effectful commands removed optimized to a constant, we can
  // also the original closure and assume that it doesn't capture any dynamic environment.
  if is_effectively_const {
    if let Some(expr) = expr_opt {
      if matches!(&expr, Expr::Closure { .. }) {
        optimize_expr(ctx, local_scope, expr, allow_rng_const_eval)?;

        // if the expr wasn't literalized (which it won't be if contained any draw
        // commands...), we force it to be now.
        match expr {
          Expr::Closure {
            params,
            body,
            arg_placeholder_scope,
            return_type_hint,
          } => {
            *expr = Expr::Literal(Value::Callable(Rc::new(Callable::Closure(Closure {
              params: Rc::clone(&params),
              body: Rc::clone(&body),
              // There is no captured scope since everything was const in the test case
              captured_scope: CapturedScope::Strong(Rc::new(Scope::default())),
              arg_placeholder_scope: RefCell::new(Some(std::mem::take(arg_placeholder_scope))),
              return_type_hint: *return_type_hint,
            }))));
          }
          _ => (),
        }
      }
    }
  }

  Ok(())
}

fn callable_requires_rng_state(callable: &Callable) -> bool {
  match callable {
    Callable::Builtin { .. } => callable.is_rng_dependent(),
    _ => true, // TODO: is this really the best we can do?
  }
}

fn hash_rng_state(hasher: &mut SipHasher, rng_state: &impl std::fmt::Debug) {
  let debug = format!("{rng_state:?}");
  debug.hash(hasher);
}

#[derive(Clone, Copy)]
struct ExprHashConfig {
  /// Whether to track RNG usage (for const eval caching).
  /// When true, sets `uses_rng` flag when encountering RNG-dependent callables.
  track_rng: bool,
  /// Whether to allow dynamic expressions (Ident, Conditional, Block, etc.).
  /// When false, returns None for expressions that cannot be const-evaluated.
  allow_dynamic: bool,
}

impl ExprHashConfig {
  /// Config for const eval caching: tracks RNG, rejects dynamic expressions.
  const fn const_eval() -> Self {
    Self {
      track_rng: true,
      allow_dynamic: false,
    }
  }

  /// Config for structural hashing: no RNG tracking, allows all expression types.
  const fn structural() -> Self {
    Self {
      track_rng: false,
      allow_dynamic: true,
    }
  }
}

/// Unified expression hasher that handles both const eval and structural hashing modes.
fn hash_expr(
  expr: &Expr,
  hasher: &mut SipHasher,
  uses_rng: &mut bool,
  config: ExprHashConfig,
) -> Option<()> {
  std::mem::discriminant(expr).hash(hasher);
  match expr {
    Expr::BinOp {
      op,
      lhs,
      rhs,
      pre_resolved_def_ix: _,
    } => {
      std::mem::discriminant(op).hash(hasher);
      hash_expr(lhs, hasher, uses_rng, config)?;
      hash_expr(rhs, hasher, uses_rng, config)?;
      if config.track_rng && matches!(op, BinOp::Pipeline | BinOp::Map) {
        if let Expr::Literal(Value::Callable(callable)) = rhs.as_ref() {
          if callable_requires_rng_state(callable) {
            *uses_rng = true;
          }
        }
      }
      Some(())
    }
    Expr::PrefixOp { op, expr: inner } => {
      std::mem::discriminant(op).hash(hasher);
      hash_expr(inner, hasher, uses_rng, config)
    }
    Expr::Range {
      start,
      end,
      inclusive,
    } => {
      inclusive.hash(hasher);
      hash_expr(start, hasher, uses_rng, config)?;
      std::mem::discriminant(end).hash(hasher);
      if let Some(end) = end {
        hash_expr(end, hasher, uses_rng, config)?;
      }
      Some(())
    }
    Expr::StaticFieldAccess { lhs, field } => {
      field.hash(hasher);
      hash_expr(lhs, hasher, uses_rng, config)
    }
    Expr::FieldAccess { lhs, field } => {
      hash_expr(lhs, hasher, uses_rng, config)?;
      hash_expr(field, hasher, uses_rng, config)
    }
    Expr::Call(FunctionCall {
      target,
      args,
      kwargs,
    }) => {
      std::mem::discriminant(target).hash(hasher);
      match target {
        FunctionCallTarget::Literal(callable) => {
          hash_callable(callable, hasher, uses_rng, config)?;
          if config.track_rng && callable_requires_rng_state(callable) {
            *uses_rng = true;
          }
        }
        FunctionCallTarget::Name(name) => {
          if !config.allow_dynamic {
            return None;
          }
          name.hash(hasher);
        }
      }
      args.len().hash(hasher);
      for arg in args {
        hash_expr(arg, hasher, uses_rng, config)?;
      }
      let mut keys = kwargs.keys().copied().collect::<Vec<_>>();
      keys.sort_by_key(|k| k.0);
      for key in keys {
        key.hash(hasher);
        let expr = kwargs.get(&key)?;
        hash_expr(expr, hasher, uses_rng, config)?;
      }
      Some(())
    }
    Expr::Closure {
      params,
      body,
      return_type_hint,
      ..
    } => {
      if !config.allow_dynamic {
        return None;
      }
      hash_closure_parts(params, body, return_type_hint, hasher, uses_rng, config)
    }
    Expr::Ident(name) => {
      if !config.allow_dynamic {
        return None;
      }
      name.hash(hasher);
      Some(())
    }
    Expr::ArrayLiteral(exprs) => {
      exprs.len().hash(hasher);
      for expr in exprs {
        hash_expr(expr, hasher, uses_rng, config)?;
      }
      Some(())
    }
    Expr::MapLiteral { entries } => {
      entries.len().hash(hasher);
      for entry in entries {
        std::mem::discriminant(entry).hash(hasher);
        match entry {
          MapLiteralEntry::KeyValue { key, value } => {
            key.hash(hasher);
            hash_expr(value, hasher, uses_rng, config)?;
          }
          MapLiteralEntry::Splat { expr } => {
            hash_expr(expr, hasher, uses_rng, config)?;
          }
        }
      }
      Some(())
    }
    Expr::Literal(value) => hash_value(value, hasher),
    Expr::Conditional {
      cond,
      then,
      else_if_exprs,
      else_expr,
    } => {
      if !config.allow_dynamic {
        return None;
      }
      hash_expr(cond, hasher, uses_rng, config)?;
      hash_expr(then, hasher, uses_rng, config)?;
      else_if_exprs.len().hash(hasher);
      for (cond, expr) in else_if_exprs {
        hash_expr(cond, hasher, uses_rng, config)?;
        hash_expr(expr, hasher, uses_rng, config)?;
      }
      std::mem::discriminant(else_expr).hash(hasher);
      if let Some(expr) = else_expr {
        hash_expr(expr, hasher, uses_rng, config)?;
      }
      Some(())
    }
    Expr::Block { statements } => {
      if !config.allow_dynamic {
        return None;
      }
      statements.len().hash(hasher);
      for stmt in statements {
        hash_statement(stmt, hasher, uses_rng, config)?;
      }
      Some(())
    }
  }
}

fn hash_type_name(type_name: TypeName, hasher: &mut SipHasher) {
  std::mem::discriminant(&type_name).hash(hasher);
}

fn hash_destructure_pattern(pattern: &DestructurePattern, hasher: &mut SipHasher) -> Option<()> {
  std::mem::discriminant(pattern).hash(hasher);
  match pattern {
    DestructurePattern::Ident(ident) => {
      ident.hash(hasher);
      Some(())
    }
    DestructurePattern::Array(items) => {
      items.len().hash(hasher);
      for item in items {
        hash_destructure_pattern(item, hasher)?;
      }
      Some(())
    }
    DestructurePattern::Map(map) => {
      map.len().hash(hasher);
      let mut keys = map.keys().copied().collect::<Vec<_>>();
      keys.sort_by_key(|key| key.0);
      for key in keys {
        key.hash(hasher);
        let value = map.get(&key)?;
        hash_destructure_pattern(value, hasher)?;
      }
      Some(())
    }
  }
}

fn hash_closure_arg(
  arg: &ClosureArg,
  hasher: &mut SipHasher,
  uses_rng: &mut bool,
  config: ExprHashConfig,
) -> Option<()> {
  hash_destructure_pattern(&arg.ident, hasher)?;
  std::mem::discriminant(&arg.type_hint).hash(hasher);
  if let Some(type_hint) = arg.type_hint {
    hash_type_name(type_hint, hasher);
  }
  std::mem::discriminant(&arg.default_val).hash(hasher);
  if let Some(default_val) = &arg.default_val {
    hash_expr(default_val, hasher, uses_rng, config)?;
  }
  Some(())
}

fn hash_statement(
  stmt: &Statement,
  hasher: &mut SipHasher,
  uses_rng: &mut bool,
  config: ExprHashConfig,
) -> Option<()> {
  std::mem::discriminant(stmt).hash(hasher);
  match stmt {
    Statement::Assignment {
      name,
      expr,
      type_hint,
    } => {
      name.hash(hasher);
      std::mem::discriminant(type_hint).hash(hasher);
      if let Some(type_hint) = type_hint {
        hash_type_name(*type_hint, hasher);
      }
      hash_expr(expr, hasher, uses_rng, config)
    }
    Statement::DestructureAssignment { lhs, rhs } => {
      hash_destructure_pattern(lhs, hasher)?;
      hash_expr(rhs, hasher, uses_rng, config)
    }
    Statement::Expr(expr) => hash_expr(expr, hasher, uses_rng, config),
    Statement::Return { value } => {
      std::mem::discriminant(value).hash(hasher);
      if let Some(expr) = value {
        hash_expr(expr, hasher, uses_rng, config)?;
      }
      Some(())
    }
    Statement::Break { value } => {
      std::mem::discriminant(value).hash(hasher);
      if let Some(expr) = value {
        hash_expr(expr, hasher, uses_rng, config)?;
      }
      Some(())
    }
  }
}

fn hash_closure_parts(
  params: &Rc<Vec<ClosureArg>>,
  body: &Rc<crate::ast::ClosureBody>,
  return_type_hint: &Option<TypeName>,
  hasher: &mut SipHasher,
  uses_rng: &mut bool,
  config: ExprHashConfig,
) -> Option<()> {
  params.len().hash(hasher);
  for param in params.iter() {
    hash_closure_arg(param, hasher, uses_rng, config)?;
  }
  std::mem::discriminant(return_type_hint).hash(hasher);
  if let Some(type_hint) = return_type_hint {
    hash_type_name(*type_hint, hasher);
  }
  body.0.len().hash(hasher);
  for stmt in body.0.iter() {
    hash_statement(stmt, hasher, uses_rng, config)?;
  }
  Some(())
}

fn hash_call(
  callable: &Rc<Callable>,
  args: &[Expr],
  kwargs: &FxHashMap<Sym, Expr>,
  hasher: &mut SipHasher,
  uses_rng: &mut bool,
  config: ExprHashConfig,
) -> Option<()> {
  // Hash a marker for function calls
  std::mem::discriminant(&Expr::Call(FunctionCall {
    target: FunctionCallTarget::Literal(Rc::clone(callable)),
    args: Vec::new(),
    kwargs: FxHashMap::default(),
  }))
  .hash(hasher);
  std::mem::discriminant(&FunctionCallTarget::Literal(Rc::clone(callable))).hash(hasher);
  hash_callable(callable, hasher, uses_rng, config)?;
  if config.track_rng && callable_requires_rng_state(callable) {
    *uses_rng = true;
  }
  args.len().hash(hasher);
  for arg in args {
    hash_expr(arg, hasher, uses_rng, config)?;
  }
  let mut keys = kwargs.keys().copied().collect::<Vec<_>>();
  keys.sort_by_key(|k| k.0);
  for key in keys {
    key.hash(hasher);
    let expr = kwargs.get(&key)?;
    hash_expr(expr, hasher, uses_rng, config)?;
  }
  Some(())
}

fn hash_callable(
  callable: &Rc<Callable>,
  hasher: &mut SipHasher,
  uses_rng: &mut bool,
  config: ExprHashConfig,
) -> Option<()> {
  std::mem::discriminant(&**callable).hash(hasher);
  match &**callable {
    Callable::Builtin { fn_entry_ix, .. } => {
      fn_entry_ix.hash(hasher);
      Some(())
    }
    Callable::Closure(closure) => hash_closure_parts(
      &closure.params,
      &closure.body,
      &closure.return_type_hint,
      hasher,
      uses_rng,
      config,
    ),
    _ => {
      (Rc::as_ptr(callable) as usize).hash(hasher);
      Some(())
    }
  }
}

fn const_eval_call_value(
  ctx: &EvalCtx,
  callable: &Rc<Callable>,
  args: &[Expr],
  kwargs: &FxHashMap<Sym, Expr>,
  arg_vals: &[Value],
  kwarg_vals: &FxHashMap<Sym, Value>,
  allow_rng_const_eval: bool,
) -> Result<Option<Value>, ErrorStack> {
  if !can_const_eval_callable(callable, allow_rng_const_eval) {
    return Ok(None);
  }

  let cache_lookup = const_eval_cache_lookup_with(ctx, allow_rng_const_eval, |hasher, uses_rng| {
    hash_call(
      callable,
      args,
      kwargs,
      hasher,
      uses_rng,
      ExprHashConfig::const_eval(),
    )
  });
  if let Some(lookup) = cache_lookup {
    if let Some(cached) = const_eval_cache_get(ctx, lookup) {
      return Ok(Some(cached));
    }
  }

  let evaled = ctx.invoke_callable(callable, arg_vals, kwarg_vals)?;
  if let Some(lookup) = cache_lookup {
    const_eval_cache_store(ctx, lookup, evaled.clone());
  }
  Ok(Some(evaled))
}

fn hash_value(value: &Value, hasher: &mut SipHasher) -> Option<()> {
  std::mem::discriminant(value).hash(hasher);
  match value {
    Value::Nil => {}
    Value::Int(val) => {
      val.hash(hasher);
    }
    Value::Float(val) => {
      val.to_bits().hash(hasher);
    }
    Value::Vec2(val) => {
      val.x.to_bits().hash(hasher);
      val.y.to_bits().hash(hasher);
    }
    Value::Vec3(val) => {
      val.x.to_bits().hash(hasher);
      val.y.to_bits().hash(hasher);
      val.z.to_bits().hash(hasher);
    }
    Value::Bool(val) => {
      val.hash(hasher);
    }
    Value::String(val) => {
      val.hash(hasher);
    }
    Value::Mesh(mesh) => {
      (Rc::as_ptr(mesh) as usize).hash(hasher);
    }
    Value::Callable(callable) => {
      // Use structural config for callable hashing within values since we don't track RNG here
      let mut dummy_rng = false;
      hash_callable(
        callable,
        hasher,
        &mut dummy_rng,
        ExprHashConfig::structural(),
      )?;
    }
    Value::Sequence(seq) => {
      (Rc::as_ptr(seq) as *const () as usize).hash(hasher);
    }
    Value::Map(map) => {
      (Rc::as_ptr(map) as *const () as usize).hash(hasher);
    }
    Value::Material(material) => {
      (Rc::as_ptr(material) as usize).hash(hasher);
    }
    Value::Light(light) => {
      (light.as_ref() as *const _ as usize).hash(hasher);
    }
  }
  Some(())
}

fn is_assoc_foldable_literal(val: &Value) -> bool {
  matches!(
    val,
    Value::Int(_) | Value::Float(_) | Value::Vec2(_) | Value::Vec3(_)
  )
}

fn is_float_assoc_literal(val: &Value) -> bool {
  matches!(val, Value::Float(_) | Value::Vec2(_) | Value::Vec3(_))
}

fn expr_requires_float_assoc(ctx: &EvalCtx, local_scope: &ScopeTracker, expr: &Expr) -> bool {
  match pre_resolve_expr_type(ctx, local_scope, expr) {
    Some(ArgType::Int) => false,
    Some(ArgType::Float | ArgType::Vec2 | ArgType::Vec3 | ArgType::Numeric) => true,
    Some(_) => false,
    None => true,
  }
}

fn can_fold_assoc_literals(
  ctx: &EvalCtx,
  local_scope: &ScopeTracker,
  lhs_val: &Value,
  rhs_val: &Value,
  other_expr: &Expr,
) -> bool {
  if !is_assoc_foldable_literal(lhs_val) || !is_assoc_foldable_literal(rhs_val) {
    return false;
  }
  if !FLOAT_ASSOC_FOLDING_ENABLED {
    if is_float_assoc_literal(lhs_val)
      || is_float_assoc_literal(rhs_val)
      || expr_requires_float_assoc(ctx, local_scope, other_expr)
    {
      return false;
    }
  }
  true
}

fn fold_associative_literal_chain(
  ctx: &EvalCtx,
  local_scope: &ScopeTracker,
  op: BinOp,
  lhs: &mut Box<Expr>,
  rhs: &mut Box<Expr>,
) -> Result<bool, ErrorStack> {
  if !matches!(op, BinOp::Add | BinOp::Mul) {
    return Ok(false);
  }

  if let Some(lhs_val) = lhs.as_literal().cloned() {
    let mut rhs_expr = std::mem::replace(rhs, Box::new(Expr::Literal(Value::Nil)));
    let mut changed = false;

    if let Expr::BinOp {
      op: rhs_op,
      lhs: rhs_lhs,
      rhs: rhs_rhs,
      pre_resolved_def_ix: _,
    } = &mut *rhs_expr
    {
      if *rhs_op == op {
        if let Some(rhs_lhs_val) = rhs_lhs.as_literal().cloned() {
          if can_fold_assoc_literals(ctx, local_scope, &lhs_val, &rhs_lhs_val, rhs_rhs.as_ref()) {
            let new_val = op.apply(ctx, &lhs_val, &rhs_lhs_val, None)?;
            *lhs = Box::new(new_val.into_literal_expr());
            rhs_expr = std::mem::replace(rhs_rhs, Box::new(Expr::Literal(Value::Nil)));
            changed = true;
          }
        }
      }
    }

    *rhs = rhs_expr;
    if changed {
      return Ok(true);
    }
  }

  if let Some(rhs_val) = rhs.as_literal().cloned() {
    let mut lhs_expr = std::mem::replace(lhs, Box::new(Expr::Literal(Value::Nil)));
    let mut changed = false;

    if let Expr::BinOp {
      op: lhs_op,
      lhs: lhs_lhs,
      rhs: lhs_rhs,
      pre_resolved_def_ix: _,
    } = &mut *lhs_expr
    {
      if *lhs_op == op {
        if let Some(lhs_rhs_val) = lhs_rhs.as_literal().cloned() {
          if can_fold_assoc_literals(ctx, local_scope, &lhs_rhs_val, &rhs_val, lhs_lhs.as_ref()) {
            let new_val = op.apply(ctx, &lhs_rhs_val, &rhs_val, None)?;
            *rhs = Box::new(new_val.into_literal_expr());
            lhs_expr = std::mem::replace(lhs_lhs, Box::new(Expr::Literal(Value::Nil)));
            changed = true;
          }
        }
      }
    }

    *lhs = lhs_expr;
    if changed {
      return Ok(true);
    }
  }

  Ok(false)
}

pub(crate) fn optimize_expr<'a>(
  ctx: &EvalCtx,
  local_scope: &'a mut ScopeTracker,
  expr: &mut Expr,
  allow_rng_const_eval: bool,
) -> Result<(), ErrorStack> {
  fold_constants(ctx, local_scope, expr, allow_rng_const_eval)
}

fn fold_constants<'a>(
  ctx: &EvalCtx,
  local_scope: &'a mut ScopeTracker,
  expr: &mut Expr,
  allow_rng_const_eval: bool,
) -> Result<(), ErrorStack> {
  match expr {
    Expr::BinOp {
      op,
      lhs,
      rhs,
      pre_resolved_def_ix,
    } => {
      // need to special case short-circuiting logical ops
      let mut did_opt_lhs = false;
      if matches!(op, BinOp::And | BinOp::Or) {
        optimize_expr(ctx, local_scope, lhs, allow_rng_const_eval)?;
        did_opt_lhs = true;

        let lhs_lit_opt = lhs.as_literal();
        if let Some(lhs_lit) = lhs_lit_opt {
          let lhs_bool = match lhs_lit {
            Value::Bool(b) => *b,
            _ => {
              return Err(ErrorStack::new(format!(
                "Left-hand side of logical operator must be a boolean, found: {lhs_lit:?}",
              )))
            }
          };

          match op {
            BinOp::And => {
              if !lhs_bool {
                *expr = Value::Bool(false).into_literal_expr();
                return Ok(());
              }
            }
            BinOp::Or => {
              if lhs_bool {
                *expr = Value::Bool(true).into_literal_expr();
                return Ok(());
              }
            }
            _ => unreachable!(),
          }
        }
      }

      if !did_opt_lhs {
        optimize_expr(ctx, local_scope, lhs, allow_rng_const_eval)?;
      }
      optimize_expr(ctx, local_scope, rhs, allow_rng_const_eval)?;

      if matches!(op, BinOp::Add | BinOp::Mul) {
        let mut changed = false;
        while fold_associative_literal_chain(ctx, local_scope, *op, lhs, rhs)? {
          changed = true;
        }
        if changed {
          *pre_resolved_def_ix = None;
        }
      }

      let resolve_opt = pre_resolve_binop_def_ix(ctx, local_scope, op, lhs, rhs);
      if let Some(def_ix) = resolve_opt {
        *pre_resolved_def_ix = Some(def_ix.1);
      }

      let (Some(lhs_val), Some(rhs_val)) = (lhs.as_literal(), rhs.as_literal()) else {
        return Ok(());
      };

      if matches!(op, BinOp::Pipeline | BinOp::Map) {
        if let Value::Callable(callable) = &rhs_val {
          if !can_const_eval_callable(callable, allow_rng_const_eval) {
            return Ok(());
          }
        }
      }

      let binop_discriminant = std::mem::discriminant(&Expr::BinOp {
        op: *op,
        lhs: Box::new(Expr::Literal(Value::Nil)),
        rhs: Box::new(Expr::Literal(Value::Nil)),
        pre_resolved_def_ix: None,
      });
      let op_discriminant = std::mem::discriminant(op);
      let cache_lookup =
        const_eval_cache_lookup_with(ctx, allow_rng_const_eval, |hasher, uses_rng| {
          binop_discriminant.hash(hasher);
          op_discriminant.hash(hasher);
          hash_expr(lhs, hasher, uses_rng, ExprHashConfig::const_eval())?;
          hash_expr(rhs, hasher, uses_rng, ExprHashConfig::const_eval())?;
          // Already handled by hash_expr with const_eval config, but add explicit check for clarity
          if matches!(op, BinOp::Pipeline | BinOp::Map) {
            if let Expr::Literal(Value::Callable(callable)) = rhs.as_ref() {
              if callable_requires_rng_state(callable) {
                *uses_rng = true;
              }
            }
          }
          Some(())
        });
      if let Some(lookup) = cache_lookup {
        if let Some(cached) = const_eval_cache_get(ctx, lookup) {
          *expr = cached.into_literal_expr();
          return Ok(());
        }
      }

      let val = op.apply(ctx, lhs_val, rhs_val, *pre_resolved_def_ix)?;
      if let Some(lookup) = cache_lookup {
        const_eval_cache_store(ctx, lookup, val.clone());
      }
      *expr = val.into_literal_expr();
      Ok(())
    }
    Expr::PrefixOp { op, expr: inner } => {
      optimize_expr(ctx, local_scope, inner, allow_rng_const_eval)?;

      let Some(val) = inner.as_literal() else {
        return Ok(());
      };
      let prefix_discriminant = std::mem::discriminant(&Expr::PrefixOp {
        op: *op,
        expr: Box::new(Expr::Literal(Value::Nil)),
      });
      let op_discriminant = std::mem::discriminant(op);
      let cache_lookup =
        const_eval_cache_lookup_with(ctx, allow_rng_const_eval, |hasher, uses_rng| {
          prefix_discriminant.hash(hasher);
          op_discriminant.hash(hasher);
          hash_expr(inner, hasher, uses_rng, ExprHashConfig::const_eval())
        });
      if let Some(lookup) = cache_lookup {
        if let Some(cached) = const_eval_cache_get(ctx, lookup) {
          *expr = cached.into_literal_expr();
          return Ok(());
        }
      }
      let val = op.apply(ctx, val)?;
      if let Some(lookup) = cache_lookup {
        const_eval_cache_store(ctx, lookup, val.clone());
      }
      *expr = val.into_literal_expr();
      Ok(())
    }
    Expr::Range {
      start,
      end,
      inclusive,
    } => {
      optimize_expr(ctx, local_scope, start, allow_rng_const_eval)?;
      if let Some(end) = end {
        optimize_expr(ctx, local_scope, end, allow_rng_const_eval)?;
      }

      let (Some(start_val), Some(end_val_opt)) = (
        start.as_literal(),
        match end {
          Some(end) => end.as_literal().map(Some),
          None => Some(None),
        },
      ) else {
        return Ok(());
      };
      let cache_lookup = const_eval_cache_lookup_with(ctx, allow_rng_const_eval, |hasher, _| {
        std::mem::discriminant(&Expr::Range {
          start: Box::new(Expr::Literal(Value::Nil)),
          end: None,
          inclusive: false,
        })
        .hash(hasher);
        inclusive.hash(hasher);
        hash_value(start_val, hasher)?;
        std::mem::discriminant(&end_val_opt).hash(hasher);
        if let Some(end_val) = end_val_opt {
          hash_value(end_val, hasher)?;
        }
        Some(())
      });
      if let Some(lookup) = cache_lookup {
        if let Some(cached) = const_eval_cache_get(ctx, lookup) {
          *expr = cached.into_literal_expr();
          return Ok(());
        }
      }
      let val = eval_range(start_val, end_val_opt, *inclusive)?;
      if let Some(lookup) = cache_lookup {
        const_eval_cache_store(ctx, lookup, val.clone());
      }
      *expr = val.into_literal_expr();
      Ok(())
    }
    Expr::StaticFieldAccess { lhs, field } => {
      optimize_expr(ctx, local_scope, lhs, allow_rng_const_eval)?;

      let Some(lhs_val) = lhs.as_literal() else {
        return Ok(());
      };

      let static_access_discriminant = std::mem::discriminant(&Expr::StaticFieldAccess {
        lhs: Box::new(Expr::Literal(Value::Nil)),
        field: field.clone(),
      });
      let cache_lookup =
        const_eval_cache_lookup_with(ctx, allow_rng_const_eval, |hasher, uses_rng| {
          static_access_discriminant.hash(hasher);
          field.hash(hasher);
          hash_expr(lhs, hasher, uses_rng, ExprHashConfig::const_eval())
        });
      if let Some(lookup) = cache_lookup {
        if let Some(cached) = const_eval_cache_get(ctx, lookup) {
          *expr = cached.into_literal_expr();
          return Ok(());
        }
      }

      let val = ctx.eval_static_field_access(lhs_val, field)?;
      if let Some(lookup) = cache_lookup {
        const_eval_cache_store(ctx, lookup, val.clone());
      }
      *expr = val.into_literal_expr();

      Ok(())
    }
    Expr::FieldAccess { lhs, field } => {
      optimize_expr(ctx, local_scope, lhs, allow_rng_const_eval)?;
      optimize_expr(ctx, local_scope, field, allow_rng_const_eval)?;

      let (Some(lhs_val), Some(field_val)) = (lhs.as_literal(), field.as_literal()) else {
        return Ok(());
      };

      let field_access_discriminant = std::mem::discriminant(&Expr::FieldAccess {
        lhs: Box::new(Expr::Literal(Value::Nil)),
        field: Box::new(Expr::Literal(Value::Nil)),
      });
      let cache_lookup =
        const_eval_cache_lookup_with(ctx, allow_rng_const_eval, |hasher, uses_rng| {
          field_access_discriminant.hash(hasher);
          hash_expr(lhs, hasher, uses_rng, ExprHashConfig::const_eval())?;
          hash_expr(field, hasher, uses_rng, ExprHashConfig::const_eval())
        });
      if let Some(lookup) = cache_lookup {
        if let Some(cached) = const_eval_cache_get(ctx, lookup) {
          *expr = cached.into_literal_expr();
          return Ok(());
        }
      }

      let val = ctx.eval_field_access(lhs_val, field_val)?;
      if let Some(lookup) = cache_lookup {
        const_eval_cache_store(ctx, lookup, val.clone());
      }
      *expr = val.into_literal_expr();

      Ok(())
    }
    Expr::Call(FunctionCall {
      target,
      args,
      kwargs,
    }) => {
      // if the function call target is a name, resolve the callable referenced to make calling it
      // more efficient if it's called repeatedly later on
      if let FunctionCallTarget::Name(name) = target {
        if let Some(val) = local_scope.get(*name) {
          match val {
            TrackedValueRef::Const(val) => match val {
              Value::Callable(callable) => {
                *target = FunctionCallTarget::Literal(callable.clone());
              }
              other => {
                return ctx.with_resolved_sym(*name, |name| {
                  Err(ErrorStack::new(format!(
                    "Tried to call non-callable value: {name} = {other:?}",
                  )))
                })
              }
            },
            // calling a closure argument or dynamic captured variable
            TrackedValueRef::Arg(_) => (),
            TrackedValueRef::Dyn { .. } => (),
          }
        } else {
          // try to resolve it as a builtin
          let (fn_entry_ix, fn_impl) =
            ctx.with_resolved_sym(*name, |name| match fn_sigs().get(name) {
              Some(_) => Ok((
                get_builtin_fn_sig_entry_ix(name).unwrap(),
                resolve_builtin_impl(name),
              )),
              None => match FUNCTION_ALIASES.get(name) {
                Some(alias) => Ok((
                  get_builtin_fn_sig_entry_ix(alias).unwrap(),
                  resolve_builtin_impl(alias),
                )),
                None => Err(ErrorStack::new(format!(
                  "Variable or function not found: {name}",
                ))),
              },
            })?;

          *target = FunctionCallTarget::Literal(Rc::new(Callable::Builtin {
            fn_entry_ix,
            fn_impl,
            pre_resolved_signature: None,
          }));
        }
      }

      if let FunctionCallTarget::Literal(callable) = target {
        if is_trace_path_callable(callable) {
          maybe_constify_trace_path_closure_arg(
            ctx,
            local_scope,
            args,
            kwargs,
            allow_rng_const_eval,
          )?;
        }
      }

      for arg in args.iter_mut() {
        optimize_expr(ctx, local_scope, arg, allow_rng_const_eval)?;
      }
      for (_, expr) in kwargs.iter_mut() {
        optimize_expr(ctx, local_scope, expr, allow_rng_const_eval)?;
      }

      if let FunctionCallTarget::Literal(callable) = target {
        if let Callable::Builtin {
          fn_entry_ix,
          fn_impl,
          pre_resolved_signature,
        } = &**callable
        {
          if pre_resolved_signature.is_none() {
            let pre_resolved_signature = maybe_pre_resolve_bulitin_call_signature(
              ctx,
              local_scope,
              *fn_entry_ix,
              args,
              kwargs,
            )?;
            *target = FunctionCallTarget::Literal(Rc::new(Callable::Builtin {
              fn_entry_ix: *fn_entry_ix,
              fn_impl: *fn_impl,
              pre_resolved_signature,
            }));
          }
        }
      }

      let arg_vals = match args
        .iter()
        .map(|arg| arg.as_literal().cloned().ok_or(()))
        .collect::<Result<Vec<_>, _>>()
      {
        Ok(arg_vals) => arg_vals,
        Err(_) => return Ok(()),
      };
      let kwarg_vals = match kwargs
        .iter()
        .map(|(k, v)| v.as_literal().cloned().map(|v| (*k, v)).ok_or(()))
        .collect::<Result<FxHashMap<_, _>, _>>()
      {
        Ok(kwarg_vals) => kwarg_vals,
        Err(_) => return Ok(()),
      };

      let evaled = match target {
        FunctionCallTarget::Name(name) => {
          if let Some(val) = local_scope.get(*name) {
            match val {
              TrackedValueRef::Const(val) => match val {
                Value::Callable(callable) => const_eval_call_value(
                  ctx,
                  callable,
                  args,
                  kwargs,
                  &arg_vals,
                  &kwarg_vals,
                  allow_rng_const_eval,
                )?,
                other => {
                  return ctx
                    .with_resolved_sym(*name, |name| {
                      Err(ErrorStack::new(format!(
                        "Tried to call non-callable value: {name} = {other:?}",
                      )))
                    })
                    .unwrap()
                }
              },
              TrackedValueRef::Arg(_) => None,
              TrackedValueRef::Dyn { .. } => None,
            }
          } else {
            unreachable!(
              "If this was a builtin, it would have been resolved earlier.  If it was undefined, \
               the error would have been raised earlier."
            );
          }
        }
        FunctionCallTarget::Literal(callable) => const_eval_call_value(
          ctx,
          callable,
          args,
          kwargs,
          &arg_vals,
          &kwarg_vals,
          allow_rng_const_eval,
        )?,
      };

      if let Some(evaled) = evaled {
        *expr = evaled.into_literal_expr();
      }
      Ok(())
    }
    Expr::Closure {
      params,
      body,
      arg_placeholder_scope,
      return_type_hint,
    } => {
      let mut params_inner = (**params).clone();
      for param in &mut params_inner {
        if let Some(default_val) = &mut param.default_val {
          optimize_expr(ctx, local_scope, default_val, false)?;
        }
      }
      *params = Rc::new(params_inner);

      let mut closure_scope = ScopeTracker::wrap(local_scope);
      for param in params.iter() {
        for ident in param.ident.iter_idents() {
          closure_scope.set(
            ident,
            TrackedValue::Arg(ClosureArg {
              default_val: None,
              type_hint: match param.ident {
                DestructurePattern::Ident(_) => param.type_hint,
                DestructurePattern::Map(_) => None,
                DestructurePattern::Array(_) => None,
              },
              ident: DestructurePattern::Ident(ident),
            }),
          );
        }
      }

      // We use this scope for const capture checking/inlining to avoid situations where the closure
      // assigns a local variable with the same name as a non-const captured variable, shadowing it.
      //
      // This would hide the capture and cause incorrect behavior.
      let mut local_scope_with_args = ScopeTracker {
        vars: closure_scope.vars.clone(),
        parent: Some(local_scope),
      };

      let mut body_inner = (**body).clone();
      for stmt in &mut body_inner.0 {
        optimize_statement(ctx, &mut closure_scope, stmt, false)?;
      }
      *body = Rc::new(body_inner);

      for (name, val) in closure_scope.vars.iter() {
        match val {
          TrackedValue::Dyn { type_hint } => {
            local_scope_with_args.set(
              *name,
              TrackedValue::Dyn {
                type_hint: *type_hint,
              },
            );
          }
          TrackedValue::Arg(arg) => {
            local_scope_with_args.set(*name, TrackedValue::Arg(arg.clone()));
          }
          TrackedValue::Const(_) => (),
        }
      }

      for param in params.iter() {
        if let Some(default_val) = &param.default_val {
          if default_val.as_literal().is_none() {
            return Ok(());
          }
        }
      }

      let mut body_inner = (**body).clone();
      let mut analysis_scope = local_scope_with_args.fork();
      let mut body_captures_dyn = body_inner.analyze_const_captures(
        ctx,
        &mut analysis_scope,
        allow_rng_const_eval,
        false,
        false,
      );

      // Capture analysis that treats locals as const so nested closures don't mask outer captures.
      let mut capture_scope = ScopeTracker::wrap(local_scope);
      for name in closure_scope.vars.keys() {
        capture_scope.set(*name, TrackedValue::Const(Value::Nil));
      }
      let mut capture_analysis_scope = capture_scope.fork();
      body_captures_dyn |= body_inner.analyze_const_captures(
        ctx,
        &mut capture_analysis_scope,
        allow_rng_const_eval,
        true,
        true,
      );

      body_inner.inline_const_captures(ctx, &mut local_scope_with_args);
      *body = Rc::new(body_inner);
      if body_captures_dyn {
        return Ok(());
      }

      *expr = Expr::Literal(Value::Callable(Rc::new(Callable::Closure(Closure {
        params: Rc::clone(&params),
        body: Rc::clone(&body),
        captured_scope: CapturedScope::Strong(Rc::new(Scope::default())),
        arg_placeholder_scope: RefCell::new(Some(std::mem::take(arg_placeholder_scope))),
        return_type_hint: *return_type_hint,
      }))));

      Ok(())
    }
    &mut Expr::Ident(id) => {
      if let Some(val) = local_scope.get(id) {
        if let TrackedValueRef::Const(val) = val {
          *expr = val.clone().into_literal_expr();
          return Ok(());
        } else {
          return Ok(());
        }
      }

      if let Some(val) = ctx.globals.get(id) {
        *expr = val.clone().into_literal_expr();
        return Ok(());
      }

      let cf = ctx.with_resolved_sym(id, |resolved_name| {
        if fn_sigs().contains_key(resolved_name) || FUNCTION_ALIASES.contains_key(resolved_name) {
          *expr = Expr::Literal(Value::Callable(Rc::new(Callable::Builtin {
            fn_entry_ix: get_builtin_fn_sig_entry_ix(resolved_name).unwrap(),
            fn_impl: resolve_builtin_impl(resolved_name),
            pre_resolved_signature: None,
          })));
          ControlFlow::Break(())
        } else {
          ControlFlow::Continue(())
        }
      });
      match cf {
        ControlFlow::Break(()) => return Ok(()),
        ControlFlow::Continue(()) => (),
      }

      ctx
        .interned_symbols
        .with_resolved(id, |resolved_name| {
          Err(ErrorStack::new(format!(
            "Variable or function not found: {resolved_name}"
          )))
        })
        .unwrap()
    }
    Expr::Literal(_) => Ok(()),
    Expr::ArrayLiteral(exprs) => {
      for inner in exprs.iter_mut() {
        optimize_expr(ctx, local_scope, inner, allow_rng_const_eval)?;
      }

      // if all elements are literals, can fold into an `EagerSeq`
      if exprs.iter().all(|e| e.is_literal()) {
        let array_discriminant = std::mem::discriminant(&Expr::ArrayLiteral(vec![]));
        let cache_lookup =
          const_eval_cache_lookup_with(ctx, allow_rng_const_eval, |hasher, uses_rng| {
            array_discriminant.hash(hasher);
            exprs.len().hash(hasher);
            for inner in exprs.iter() {
              hash_expr(inner, hasher, uses_rng, ExprHashConfig::const_eval())?;
            }
            Some(())
          });
        if let Some(lookup) = cache_lookup {
          if let Some(cached) = const_eval_cache_get(ctx, lookup) {
            *expr = cached.into_literal_expr();
            return Ok(());
          }
        }
        let values = exprs
          .iter()
          .map(|e| e.as_literal().unwrap().clone())
          .collect::<Vec<_>>();
        let val = Value::Sequence(Rc::new(EagerSeq { inner: values }));
        if let Some(lookup) = cache_lookup {
          const_eval_cache_store(ctx, lookup, val.clone());
        }
        *expr = Expr::Literal(val);
      }

      Ok(())
    }
    Expr::MapLiteral { entries } => {
      for value in entries.iter_mut() {
        match value {
          MapLiteralEntry::KeyValue { key: _, value } => {
            optimize_expr(ctx, local_scope, value, allow_rng_const_eval)?;
          }
          MapLiteralEntry::Splat { expr } => {
            optimize_expr(ctx, local_scope, expr, allow_rng_const_eval)?;
          }
        }
      }

      // if all values are literals, can fold into a `Map`
      if entries.iter().all(|e| e.is_literal()) {
        let map_discriminant = std::mem::discriminant(&Expr::MapLiteral { entries: vec![] });
        let cache_lookup =
          const_eval_cache_lookup_with(ctx, allow_rng_const_eval, |hasher, uses_rng| {
            map_discriminant.hash(hasher);
            entries.len().hash(hasher);
            for entry in entries.iter() {
              std::mem::discriminant(entry).hash(hasher);
              match entry {
                MapLiteralEntry::KeyValue { key, value } => {
                  key.hash(hasher);
                  hash_expr(value, hasher, uses_rng, ExprHashConfig::const_eval())?;
                }
                MapLiteralEntry::Splat { expr: splat_expr } => {
                  hash_expr(splat_expr, hasher, uses_rng, ExprHashConfig::const_eval())?;
                }
              }
            }
            Some(())
          });
        if let Some(lookup) = cache_lookup {
          if let Some(cached) = const_eval_cache_get(ctx, lookup) {
            *expr = cached.into_literal_expr();
            return Ok(());
          }
        }
        let mut map = FxHashMap::default();
        for entry in entries {
          match entry {
            MapLiteralEntry::KeyValue { key, value } => {
              map.insert(key.clone(), value.as_literal().cloned().unwrap());
            }
            MapLiteralEntry::Splat { expr } => {
              let literal = expr.as_literal().unwrap();
              let splat = match literal.as_map() {
                Some(map) => map,
                None => {
                  return Err(ErrorStack::new(format!(
                    "Tried to splat value of type {:?} into map; expected a map.",
                    literal.get_type()
                  )))
                }
              };
              for (key, val) in splat {
                map.insert(key.clone(), val.clone());
              }
            }
          }
        }

        let val = Value::Map(Rc::new(map));
        if let Some(lookup) = cache_lookup {
          const_eval_cache_store(ctx, lookup, val.clone());
        }
        *expr = Expr::Literal(val);
      }

      Ok(())
    }
    Expr::Conditional {
      cond,
      then,
      else_if_exprs,
      else_expr,
    } => {
      // TODO: check if conditions are const and elide the whole conditional if they are

      /// If there's an assignment performed to a variable in the parent scope from within one of
      /// the conditional blocks, we can no longer depend on knowing the value of that variable
      /// going forward.
      fn deconstify_parent_scope<'a>(
        parent_scope: &mut ScopeTracker,
        conditional_scope_var_names: impl Iterator<Item = Sym>,
      ) {
        for name in conditional_scope_var_names {
          if let Some(TrackedValueRef::Const(_)) = parent_scope.get(name) {
            parent_scope.set(
              name,
              TrackedValue::Arg(ClosureArg {
                ident: DestructurePattern::Ident(name),
                type_hint: None,
                default_val: None,
              }),
            );
          }
        }
      }

      optimize_expr(ctx, local_scope, cond, allow_rng_const_eval)?;
      let mut then_scope = ScopeTracker::wrap(local_scope);
      optimize_expr(ctx, &mut then_scope, then, false)?;
      let ScopeTracker {
        vars: then_scope_var_names,
        ..
      } = &then_scope;
      deconstify_parent_scope(local_scope, then_scope_var_names.keys().copied());
      for (cond, inner) in else_if_exprs {
        optimize_expr(ctx, local_scope, cond, allow_rng_const_eval)?;
        let mut else_if_scope = ScopeTracker::wrap(local_scope);
        optimize_expr(ctx, &mut else_if_scope, inner, false)?;
        let ScopeTracker {
          vars: else_if_scope_var_names,
          ..
        } = &else_if_scope;
        deconstify_parent_scope(local_scope, else_if_scope_var_names.keys().copied());
      }
      if let Some(else_expr) = else_expr {
        let mut else_scope = ScopeTracker::wrap(local_scope);
        optimize_expr(ctx, &mut else_scope, else_expr, false)?;
        let ScopeTracker {
          vars: else_scope_var_names,
          ..
        } = &else_scope;
        deconstify_parent_scope(local_scope, else_scope_var_names.keys().copied());
      }
      Ok(())
    }
    Expr::Block { statements } => {
      // the const-capture analysis was built for closure bodies, so they think everything
      // is OK if a local at the most inner scope level is declared but not const-available - those
      // correspond to closure args.

      // For the case of a block inside of a closure, we can get around this by adding one level of
      // fake nesting to the scope

      let mut block_scope = ScopeTracker::wrap(&*local_scope);

      for stmt in statements.iter_mut() {
        optimize_statement(ctx, &mut block_scope, stmt, allow_rng_const_eval)?;
      }

      // can const-fold the block if all inner statements are const
      let mut analysis_scope = block_scope.fork();
      let mut captures_dyn = statements.iter().any(|stmt| {
        stmt.analyze_const_captures(ctx, &mut analysis_scope, allow_rng_const_eval, true, false)
      });
      for stmt in statements.iter_mut() {
        stmt.inline_const_captures(ctx, &mut block_scope);
      }

      for (key, val) in block_scope.vars {
        let is_set: bool = local_scope.get(key).is_some();
        if is_set {
          local_scope.vars.insert(key, val);
          captures_dyn = true;
        }
      }

      if captures_dyn {
        return Ok(());
      }

      let evaled = ctx.eval_expr(expr, &ctx.globals, None)?;
      match evaled {
        crate::ControlFlow::Continue(val) | crate::ControlFlow::Break(val) => {
          *expr = Expr::Literal(val)
        }
        crate::ControlFlow::Return(retval) => {
          // replace the block with a new one that just includes the return statement
          *expr = Expr::Block {
            statements: vec![Statement::Return {
              value: Some(Expr::Literal(retval)),
            }],
          };
        }
      }

      Ok(())
    }
  }
}

fn optimize_statement<'a>(
  ctx: &EvalCtx,
  local_scope: &'a mut ScopeTracker,
  stmt: &mut Statement,
  allow_rng_const_eval: bool,
) -> Result<(), ErrorStack> {
  match stmt {
    Statement::Expr(expr) => optimize_expr(ctx, local_scope, expr, allow_rng_const_eval),
    Statement::Assignment {
      name,
      expr,
      type_hint,
    } => {
      // insert a placeholder for the variable in the local scope to support recursive calls
      // unless we're assigning to an existing variable
      if !local_scope.has(*name) {
        local_scope.set(
          *name,
          TrackedValue::Dyn {
            type_hint: *type_hint,
          },
        );
      }

      optimize_expr(ctx, local_scope, expr, allow_rng_const_eval)?;

      local_scope.set(
        *name,
        match expr.as_literal() {
          Some(val) => TrackedValue::Const(val.clone()),
          None => {
            let dyn_type = get_dyn_type(expr, local_scope);
            let pre_resolved_ty = match *type_hint {
              Some(hint) => Some(hint),
              None => match pre_resolve_expr_type(ctx, local_scope, expr) {
                Some(ty) => ty.into(),
                None => None,
              },
            };
            match dyn_type {
              DynType::Arg => TrackedValue::Arg(ClosureArg {
                ident: DestructurePattern::Ident(*name),
                type_hint: pre_resolved_ty,
                default_val: None,
              }),
              DynType::Const | DynType::Dyn => TrackedValue::Dyn {
                type_hint: pre_resolved_ty,
              },
            }
          }
        },
      );
      Ok(())
    }
    Statement::DestructureAssignment { lhs, rhs } => {
      // insert a placeholder for assigned variables in the local scope to support recursive calls
      // unless we're assigning to an existing variables
      for name in lhs.iter_idents() {
        if !local_scope.has(name) {
          local_scope.set(
            name,
            // no way currently to get type data for stuff inside of maps/arrays
            TrackedValue::Dyn { type_hint: None },
          );
        }
      }

      optimize_expr(ctx, local_scope, rhs, allow_rng_const_eval)?;

      let Some(rhs) = rhs.as_literal() else {
        for name in lhs.iter_idents() {
          local_scope.set(
            name,
            TrackedValue::Arg(ClosureArg {
              ident: DestructurePattern::Ident(name),
              type_hint: None,
              default_val: None,
            }),
          );
        }
        return Ok(());
      };

      lhs
        .visit_assignments(ctx, rhs.clone(), &mut |lhs, rhs| {
          local_scope.set(lhs, TrackedValue::Const(rhs));
          Ok(())
        })
        .map_err(|err| err.wrap("Error evaluating destructure assignment"))?;

      Ok(())
    }
    Statement::Return { value } => {
      if let Some(expr) = value {
        optimize_expr(ctx, local_scope, expr, allow_rng_const_eval)?
      }
      Ok(())
    }
    Statement::Break { value } => {
      if let Some(expr) = value {
        optimize_expr(ctx, local_scope, expr, allow_rng_const_eval)?
      }
      Ok(())
    }
  }
}

struct OptimizationPass {
  name: &'static str,
  run: fn(&EvalCtx, &mut Program) -> Result<(), ErrorStack>,
}

struct OptimizerPipeline {
  passes: Vec<OptimizationPass>,
}

impl OptimizerPipeline {
  fn new() -> Self {
    Self { passes: Vec::new() }
  }

  fn with_pass(mut self, pass: OptimizationPass) -> Self {
    self.passes.push(pass);
    self
  }

  fn run(&self, ctx: &EvalCtx, program: &mut Program) -> Result<(), ErrorStack> {
    for pass in &self.passes {
      (pass.run)(ctx, program)?;
    }
    Ok(())
  }
}

fn default_optimizer_pipeline() -> OptimizerPipeline {
  OptimizerPipeline::new().with_pass(OptimizationPass {
    name: "const_fold",
    run: run_const_folding_pass,
  })
}

fn run_const_folding_pass(ctx: &EvalCtx, ast: &mut Program) -> Result<(), ErrorStack> {
  let mut local_scope = ScopeTracker::default();
  for stmt in &mut ast.statements {
    optimize_statement(ctx, &mut local_scope, stmt, true)?;
  }
  Ok(())
}

pub fn optimize_ast(ctx: &EvalCtx, ast: &mut Program) -> Result<(), ErrorStack> {
  default_optimizer_pipeline().run(ctx, ast)
}

#[test]
fn test_basic_constant_folding() {
  let mut expr = Expr::BinOp {
    op: BinOp::Add,
    lhs: Box::new(Expr::Literal(Value::Int(2))),
    rhs: Box::new(Expr::Literal(Value::Int(3))),
    pre_resolved_def_ix: None,
  };
  let mut local_scope = ScopeTracker::default();
  let ctx = EvalCtx::default();
  optimize_expr(&ctx, &mut local_scope, &mut expr, true).unwrap();
  let Expr::Literal(Value::Int(5)) = expr else {
    panic!("Expected constant folding to produce 5");
  };
}

#[test]
fn test_vec3_const_folding() {
  let code = "vec3(1+2, 2, 3*1+0+1).zyx";

  let ctx = EvalCtx::default();
  let mut ast = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast).unwrap();
  let val = match &ast.statements[0] {
    Statement::Expr(expr) => expr.as_literal().unwrap(),
    _ => unreachable!(),
  };
  assert!(matches!(val, Value::Vec3(v) if v.x == 4. && v.y == 2. && v.z == 3.));
}

#[test]
fn test_const_eval_side_effects() {
  let code = r#"
print(1+2)
//(1+2) | print
fn = || {
  print(1+2)
  return 1+2
}
fn() | print
print(fn())
fn | call | print
"#;

  let ctx = EvalCtx::default();
  let mut ast = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast).unwrap();
}

#[test]
fn test_basic_const_closure_eval() {
  let code = r#"
fn = |x| x + 1
y = fn(2)
"#;

  let ctx = EvalCtx::default();
  let mut ast = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&EvalCtx::default(), &mut ast).unwrap();

  let Statement::Assignment { name, expr, .. } = &ast.statements[1] else {
    panic!("Expected second statement to be an assignment");
  };
  assert_eq!(*name, ctx.interned_symbols.intern("y"));
  let Expr::Literal(Value::Int(3)) = expr else {
    panic!("Expected constant folding to produce 3");
  };
}

#[test]
fn test_basic_const_closure_eval_2() {
  let code = r#"
a = 1
fn = |x| x + a
y = fn(2, a)
"#;

  let ctx = EvalCtx::default();
  let mut ast = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&EvalCtx::default(), &mut ast).unwrap();

  let Statement::Assignment { name, expr, .. } = &ast.statements[2] else {
    panic!("Expected second statement to be an assignment");
  };
  assert_eq!(*name, ctx.interned_symbols.intern("y"));
  match expr {
    Expr::Literal(Value::Int(3)) => {}
    _ => panic!("Expected constant folding to produce 3, found: {expr:?}"),
  };
}

#[test]
fn test_basic_const_closure_eval_3() {
  let code = r#"
xyz=2
x = [
  box(1) | warp(|v| v * 2),
  box(xyz)
] | join
"#;

  let ctx = EvalCtx::default();
  let mut ast = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast).unwrap();

  // the whole thing should get const-eval'd to a mesh at the AST level
  let Statement::Assignment { expr, .. } = &ast.statements[1] else {
    unreachable!();
  };
  assert!(
    matches!(expr, Expr::Literal(Value::Mesh(_))),
    "Expected constant folding to produce a mesh, found: {expr:?}"
  );
}

#[test]
fn test_basic_const_closure_eval_4() {
  let code = r#"
x = 1
fn = |a| {
  foo = 1
  bar = 1
  baz = || x + 1
  return a + x + foo + bar + baz()
}
y = fn(2)
"#;

  let ctx = EvalCtx::default();
  let mut ast = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&EvalCtx::default(), &mut ast).unwrap();

  let Statement::Assignment { name, expr, .. } = &ast.statements[2] else {
    panic!("Expected second statement to be an assignment");
  };
  assert_eq!(*name, ctx.interned_symbols.intern("y"));
  let Expr::Literal(Value::Int(7)) = expr else {
    panic!("Expected constant folding to produce 7, found: {expr:?}");
  };
}

#[test]
fn test_block_const_folding() {
  let code = r#"
{
  a = 1
  b = 2
  c = a + b
  3
}
"#;

  let ctx = EvalCtx::default();
  let mut ast = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast).unwrap();

  let Statement::Expr(expr) = &ast.statements[0] else {
    panic!("Expected first statement to be an expression");
  };
  let Expr::Literal(Value::Int(3)) = expr else {
    panic!("Expected constant folding to produce 3, found: {expr:?}");
  };
}

#[test]
fn test_pre_resolve_builtin_signature() {
  let code = r#"
cb = |x: int| add(x+1, 1)
y = cb(2)
"#;

  let ctx = EvalCtx::default();
  let mut ast = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast).unwrap();

  let st1 = ast.statements[0].clone();
  let closure_body = match st1 {
    Statement::Assignment { expr, .. } => match expr {
      Expr::Literal(Value::Callable(callable)) => match &*callable {
        Callable::Closure(closure) => closure.body.clone(),
        _ => unreachable!(),
      },
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };
  let call_target = match &closure_body.0[0] {
    Statement::Expr(expr) => match expr {
      Expr::Call(FunctionCall {
        target: FunctionCallTarget::Literal(target),
        ..
      }) => target,
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };
  let pre_resolved_sig = match &**call_target {
    Callable::Builtin {
      pre_resolved_signature,
      ..
    } => pre_resolved_signature,
    _ => unreachable!(),
  };
  let pre_resolved_sig = pre_resolved_sig.as_ref().unwrap();
  assert_eq!(pre_resolved_sig.def_ix, 3);
  assert_eq!(pre_resolved_sig.arg_refs.len(), 2);
  assert!(matches!(
    &pre_resolved_sig.arg_refs[0],
    crate::ArgRef::Positional(0)
  ));
  assert!(matches!(
    &pre_resolved_sig.arg_refs[1],
    crate::ArgRef::Positional(1)
  ));
}

#[test]
fn test_const_eval_with_local_shadowing() {
  let src = r#"
x = 1
fn = |a| {
  x = x + a
  x
}
y = fn(2)
"#;

  let ctx = EvalCtx::default();
  let mut ast = crate::parse_program_src(&ctx, src).unwrap();
  optimize_ast(&ctx, &mut ast).unwrap();

  let Statement::Assignment { name, expr, .. } = &ast.statements[2] else {
    panic!("Expected second statement to be an assignment");
  };
  assert_eq!(*name, ctx.interned_symbols.intern("y"));
  let Expr::Literal(Value::Int(3)) = expr else {
    panic!("Expected constant folding to produce 3, found: {expr:?}");
  };
}

#[test]
fn test_preresolve_binop_def_ix_basic() {
  let src = r#"
f = |a: int, b: int| { a + b }
x = f(2, 3)
"#;

  let ctx = EvalCtx::default();
  let mut ast = crate::parse_program_src(&ctx, src).unwrap();
  optimize_ast(&ctx, &mut ast).unwrap();

  let st1 = ast.statements[0].clone();
  let closure_body = match st1 {
    Statement::Assignment { expr, .. } => match expr {
      Expr::Literal(Value::Callable(callable)) => match &*callable {
        Callable::Closure(closure) => closure.body.clone(),
        _ => unreachable!(),
      },
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };
  let binop_def_ix = match &closure_body.0[0] {
    Statement::Expr(expr) => match expr {
      Expr::BinOp {
        pre_resolved_def_ix,
        ..
      } => pre_resolved_def_ix,
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };
  assert_eq!(*binop_def_ix, Some(3)); // int + int
}

#[test]
fn test_preresolve_binop_def_ix_advanced() {
  let src = r#"
fn = || {
  0..
    -> || { randv(-5, 5) }
    | filter(|p: vec3| {
      noise = fbm(p * 0.0283)
      noise > 0.5
      // (noise > -0.31 && noise < -0.3) ||
      //   (noise > 0.01 && noise < 0.02) ||
      //   (noise > 0.21 && noise < 0.22) ||
      //   (noise > 0.48 && noise < 0.49) ||
      //   (noise > 0.78 && noise < 0.79)
    })
    | take(550)
    | convex_hull
}"#;

  let ctx = EvalCtx::default();
  let mut ast = crate::parse_program_src(&ctx, src).unwrap();
  optimize_ast(&ctx, &mut ast).unwrap();

  let st1 = ast.statements[0].clone();
  let closure_body = match st1 {
    Statement::Assignment { expr, .. } => match expr {
      Expr::Literal(Value::Callable(callable)) => match &*callable {
        Callable::Closure(closure) => closure.body.clone(),
        _ => unreachable!(),
      },
      Expr::Closure { body, .. } => body.clone(),
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };
  let expr = match &closure_body.0[0] {
    Statement::Expr(expr) => expr.clone(),
    Statement::Return { value: Some(expr) } => expr.clone(),
    _ => unreachable!(),
  };
  let expr = match expr {
    Expr::BinOp {
      op: BinOp::Pipeline,
      lhs,
      rhs: _, // convex_hull
      pre_resolved_def_ix: _,
    } => (*lhs).clone(),
    _ => unreachable!(),
  };
  let expr = match expr {
    Expr::BinOp {
      op: BinOp::Pipeline,
      lhs,
      rhs: _, // take
      pre_resolved_def_ix: _,
    } => (*lhs).clone(),
    _ => unreachable!(),
  };
  let expr = match expr {
    Expr::BinOp {
      op: BinOp::Pipeline,
      lhs: _, // range
      rhs,
      pre_resolved_def_ix: _,
    } => (*rhs).clone(),
    _ => unreachable!(),
  };

  let filter_paf = match expr {
    Expr::Literal(Value::Callable(callable)) => match &*callable {
      Callable::PartiallyAppliedFn(paf) => paf.clone(),
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };
  let cb = match filter_paf.args[0].clone() {
    Value::Callable(callable) => match &*callable {
      Callable::Closure(closure) => closure.clone(),
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  let stmt0 = &cb.body.0[0];
  match stmt0 {
    Statement::Assignment {
      name: _,
      expr,
      type_hint: _,
    } => match expr {
      Expr::Call(FunctionCall {
        args,
        kwargs: _,
        target,
      }) => {
        let arg0 = &args[0];
        match arg0 {
          Expr::BinOp {
            op: BinOp::Mul,
            pre_resolved_def_ix,
            ..
          } => {
            assert!(pre_resolved_def_ix.is_some());
          }
          _ => unreachable!(),
        }

        match target {
          FunctionCallTarget::Literal(callable) => match &**callable {
            Callable::Builtin {
              pre_resolved_signature,
              ..
            } => assert!(pre_resolved_signature.is_some()),
            _ => unreachable!(),
          },
          _ => unreachable!(),
        }
      }
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  // noise > 0.5
  let stmt1 = &cb.body.0[1];
  match stmt1 {
    Statement::Expr(expr) => match expr {
      Expr::BinOp {
        op: BinOp::Gt,
        pre_resolved_def_ix,
        ..
      } => {
        assert!(pre_resolved_def_ix.is_some());
      }
      _ => unreachable!(),
    },
    _ => unreachable!(),
  }
}

#[test]
fn test_assoc_constant_folding_right_chain() {
  let code = r#"
fn = |x| 1 + (1 + (1 + x))
"#;

  let ctx = EvalCtx::default();
  let mut ast = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast).unwrap();

  let st0 = ast.statements[0].clone();
  let closure_body = match st0 {
    Statement::Assignment { expr, .. } => match expr {
      Expr::Literal(Value::Callable(callable)) => match &*callable {
        Callable::Closure(closure) => closure.body.clone(),
        _ => unreachable!(),
      },
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  let expr = match &closure_body.0[0] {
    Statement::Expr(expr) => expr,
    Statement::Return { value: Some(expr) } => expr,
    _ => unreachable!(),
  };
  match expr {
    Expr::BinOp {
      op: BinOp::Add,
      lhs,
      rhs,
      pre_resolved_def_ix: _,
    } => {
      assert!(matches!(lhs.as_literal(), Some(Value::Int(3))));
      assert!(matches!(
        **rhs,
        Expr::Ident(id) if id == ctx.interned_symbols.intern("x")
      ));
    }
    _ => panic!("Expected an add expression, found: {expr:?}"),
  };
}

#[test]
fn test_assoc_constant_folding_left_chain() {
  let code = r#"
fn = |x| (x + 1) + 1
"#;

  let ctx = EvalCtx::default();
  let mut ast = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast).unwrap();

  let st0 = ast.statements[0].clone();
  let closure_body = match st0 {
    Statement::Assignment { expr, .. } => match expr {
      Expr::Literal(Value::Callable(callable)) => match &*callable {
        Callable::Closure(closure) => closure.body.clone(),
        _ => unreachable!(),
      },
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  let expr = match &closure_body.0[0] {
    Statement::Expr(expr) => expr,
    _ => unreachable!(),
  };
  match expr {
    Expr::BinOp {
      op: BinOp::Add,
      lhs,
      rhs,
      pre_resolved_def_ix: _,
    } => {
      assert!(matches!(
        **lhs,
        Expr::Ident(id) if id == ctx.interned_symbols.intern("x")
      ));
      assert!(matches!(rhs.as_literal(), Some(Value::Int(2))));
    }
    _ => panic!("Expected an add expression, found: {expr:?}"),
  };
}

#[test]
fn test_assoc_constant_folding_float_right_chain() {
  let code = r#"
fn = |x| 1.5 + (2.5 + x)
"#;

  let ctx = EvalCtx::default();
  let mut ast = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast).unwrap();

  let st0 = ast.statements[0].clone();
  let closure_body = match st0 {
    Statement::Assignment { expr, .. } => match expr {
      Expr::Literal(Value::Callable(callable)) => match &*callable {
        Callable::Closure(closure) => closure.body.clone(),
        _ => unreachable!(),
      },
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  let expr = match &closure_body.0[0] {
    Statement::Expr(expr) => expr,
    _ => unreachable!(),
  };
  match expr {
    Expr::BinOp {
      op: BinOp::Add,
      lhs,
      rhs,
      pre_resolved_def_ix: _,
    } => {
      assert!(matches!(lhs.as_literal(), Some(Value::Float(f)) if *f == 4.0));
      assert!(matches!(
        **rhs,
        Expr::Ident(id) if id == ctx.interned_symbols.intern("x")
      ));
    }
    _ => panic!("Expected an add expression, found: {expr:?}"),
  };
}

#[test]
fn test_assoc_constant_folding_vec2_add() {
  let code = r#"
fn = |v: vec2| vec2(1, 2) + (vec2(3, 4) + v)
"#;

  let ctx = EvalCtx::default();
  let mut ast = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast).unwrap();

  let st0 = ast.statements[0].clone();
  let closure_body = match st0 {
    Statement::Assignment { expr, .. } => match expr {
      Expr::Literal(Value::Callable(callable)) => match &*callable {
        Callable::Closure(closure) => closure.body.clone(),
        _ => unreachable!(),
      },
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  let expr = match &closure_body.0[0] {
    Statement::Expr(expr) => expr,
    _ => unreachable!(),
  };
  match expr {
    Expr::BinOp {
      op: BinOp::Add,
      lhs,
      rhs,
      pre_resolved_def_ix: _,
    } => {
      assert!(matches!(
        lhs.as_literal(),
        Some(Value::Vec2(v)) if v.x == 4. && v.y == 6.
      ));
      assert!(matches!(
        **rhs,
        Expr::Ident(id) if id == ctx.interned_symbols.intern("v")
      ));
    }
    _ => panic!("Expected an add expression, found: {expr:?}"),
  };
}

#[test]
fn test_assoc_constant_folding_vec3_mul() {
  let code = r#"
fn = |v: vec3| 2. * (3. * v)
"#;

  let ctx = EvalCtx::default();
  let mut ast = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast).unwrap();

  let st0 = ast.statements[0].clone();
  let closure_body = match st0 {
    Statement::Assignment { expr, .. } => match expr {
      Expr::Literal(Value::Callable(callable)) => match &*callable {
        Callable::Closure(closure) => closure.body.clone(),
        _ => unreachable!(),
      },
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  let expr = match &closure_body.0[0] {
    Statement::Expr(expr) => expr,
    _ => unreachable!(),
  };
  match expr {
    Expr::BinOp {
      op: BinOp::Mul,
      lhs,
      rhs,
      pre_resolved_def_ix: _,
    } => {
      assert!(matches!(lhs.as_literal(), Some(Value::Float(f)) if *f == 6.));
      assert!(matches!(
        **rhs,
        Expr::Ident(id) if id == ctx.interned_symbols.intern("v")
      ));
    }
    _ => panic!("Expected a mul expression, found: {expr:?}"),
  };
}

#[test]
fn test_rng_const_folding_top_level() {
  let code = "a = randf()";

  let ctx = EvalCtx::default();
  let mut ast = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast).unwrap();

  match &ast.statements[0] {
    Statement::Assignment { expr, .. } => {
      assert!(matches!(expr, Expr::Literal(Value::Float(_))));
    }
    _ => unreachable!(),
  }
}

#[test]
fn test_const_eval_cache_persists_across_runs() {
  let code = "a = box(1) | scale(0.2) | trans(0, 10, 0)";

  let ctx = EvalCtx::default();
  let mut ast1 = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast1).unwrap();

  let mesh1 = match &ast1.statements[0] {
    Statement::Assignment { expr, .. } => match expr {
      Expr::Literal(Value::Mesh(mesh)) => Rc::clone(mesh),
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  let mut ast2 = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast2).unwrap();

  let mesh2 = match &ast2.statements[0] {
    Statement::Assignment { expr, .. } => match expr {
      Expr::Literal(Value::Mesh(mesh)) => Rc::clone(mesh),
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  assert!(Rc::ptr_eq(&mesh1, &mesh2));
}

#[test]
fn test_const_eval_cache_persists_across_runs_with_closure_map() {
  let code = "a = 0..4 -> |x| x + 1";

  let ctx = EvalCtx::default();
  let mut ast1 = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast1).unwrap();

  let seq1 = match &ast1.statements[0] {
    Statement::Assignment { expr, .. } => match expr {
      Expr::Literal(Value::Sequence(seq)) => Rc::clone(seq),
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  let mut ast2 = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast2).unwrap();

  let seq2 = match &ast2.statements[0] {
    Statement::Assignment { expr, .. } => match expr {
      Expr::Literal(Value::Sequence(seq)) => Rc::clone(seq),
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  assert!(Rc::ptr_eq(&seq1, &seq2));
}

#[test]
fn test_const_eval_cache_persists_across_runs_with_closure_const_capture() {
  let code = r#"
offset = 2
a = 0..4 -> |x| x + offset
"#;

  let ctx = EvalCtx::default();
  let mut ast1 = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast1).unwrap();

  let seq1 = match &ast1.statements[1] {
    Statement::Assignment { expr, .. } => match expr {
      Expr::Literal(Value::Sequence(seq)) => Rc::clone(seq),
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  let mut ast2 = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast2).unwrap();

  let seq2 = match &ast2.statements[1] {
    Statement::Assignment { expr, .. } => match expr {
      Expr::Literal(Value::Sequence(seq)) => Rc::clone(seq),
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  assert!(Rc::ptr_eq(&seq1, &seq2));
}

#[test]
fn test_const_eval_cache_persists_across_runs_with_trace_path() {
  let code = r#"
distance = 1
path_sampler = trace_path(|| {
  move(0, 0)
  line(distance, 0)
  line(distance, distance)
})
"#;

  let ctx = EvalCtx::default();
  let mut ast1 = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast1).unwrap();

  let sampler1 = match &ast1.statements[1] {
    Statement::Assignment { expr, .. } => match expr {
      Expr::Literal(Value::Callable(callable)) => Rc::clone(callable),
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  let mut ast2 = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast2).unwrap();

  let sampler2 = match &ast2.statements[1] {
    Statement::Assignment { expr, .. } => match expr {
      Expr::Literal(Value::Callable(callable)) => Rc::clone(callable),
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  assert!(Rc::ptr_eq(&sampler1, &sampler2));

  // should be functional
  let ctx = crate::parse_and_eval_program(code).unwrap();
  let sampler = ctx.get_global("path_sampler").unwrap();
  let sampler = sampler.as_callable().unwrap();
  let p0 = ctx
    .invoke_callable(sampler, &[Value::Float(0.)], crate::EMPTY_KWARGS)
    .unwrap();
  let p1 = ctx
    .invoke_callable(sampler, &[Value::Float(0.25)], crate::EMPTY_KWARGS)
    .unwrap();
  let p2 = ctx
    .invoke_callable(sampler, &[Value::Float(0.5)], crate::EMPTY_KWARGS)
    .unwrap();
  let p3 = ctx
    .invoke_callable(sampler, &[Value::Float(1.)], crate::EMPTY_KWARGS)
    .unwrap();
  assert_eq!(*p0.as_vec2().unwrap(), crate::Vec2::new(0., 0.));
  assert_eq!(*p1.as_vec2().unwrap(), crate::Vec2::new(0.5, 0.));
  assert_eq!(*p2.as_vec2().unwrap(), crate::Vec2::new(1., 0.));
  assert_eq!(*p3.as_vec2().unwrap(), crate::Vec2::new(1., 1.));
}

// just because we can
#[test]
fn test_trace_path_sneaky_ref_const_eval() {
  let code = r#"
m = move
l2 = line
path_sampler = trace_path(|| {
  // helper functions that call draw commands can be defined, but they must
  // be defined within the `trace_path` closure
  l = |x, y| {
    l2(x, y)
  }

  m(0, 0)
  l(10, 0)
})
out = path_sampler(0.5)
"#;

  let ctx = crate::parse_and_eval_program(code).unwrap();
  let out = ctx.get_global("out").unwrap();
  let out = out.as_vec2().unwrap();
  assert_eq!(*out, crate::Vec2::new(5., 0.));
}

#[cfg(test)]
fn optimize_and_get_mesh(ctx: &EvalCtx, code: &str, stmt_index: usize) -> Rc<crate::MeshHandle> {
  let mut ast = crate::parse_program_src(ctx, code).unwrap();
  optimize_ast(ctx, &mut ast).unwrap();
  match &ast.statements[stmt_index] {
    Statement::Assignment { expr, .. } => match expr {
      Expr::Literal(Value::Mesh(mesh)) => Rc::clone(mesh),
      _ => unreachable!(),
    },
    _ => unreachable!(),
  }
}

#[cfg(test)]
fn optimize_and_get_sequence(
  ctx: &EvalCtx,
  code: &str,
  stmt_index: usize,
) -> Rc<dyn crate::Sequence> {
  let mut ast = crate::parse_program_src(ctx, code).unwrap();
  optimize_ast(ctx, &mut ast).unwrap();
  match &ast.statements[stmt_index] {
    Statement::Assignment { expr, .. } => match expr {
      Expr::Literal(Value::Sequence(seq)) => Rc::clone(seq),
      _ => unreachable!(),
    },
    _ => unreachable!(),
  }
}

#[test]
fn test_const_eval_cache_persists_across_runs_with_rng_closure_pipeline() {
  let code = r#"
points = 0..200
  -> || randv(0, 10)
  | filter(|v| len(v) < 20)
  | convex_hull
"#;

  let ctx = EvalCtx::default();
  let rng_state = ctx.rng_state();

  let mesh1 = optimize_and_get_mesh(&ctx, code, 0);
  ctx.set_rng_state(rng_state);
  let mesh2 = optimize_and_get_mesh(&ctx, code, 0);

  assert!(Rc::ptr_eq(&mesh1, &mesh2));
}

#[test]
fn test_const_eval_cache_persists_across_runs_with_rng_closure_const_capture() {
  let code = r#"
max = 10
points = 0..200
  -> || randv(0, max)
  | filter(|v| len(v) < 20)
  | convex_hull
"#;

  let ctx = EvalCtx::default();
  let rng_state = ctx.rng_state();

  let mesh1 = optimize_and_get_mesh(&ctx, code, 1);
  ctx.set_rng_state(rng_state);
  let mesh2 = optimize_and_get_mesh(&ctx, code, 1);

  assert!(Rc::ptr_eq(&mesh1, &mesh2));
}

#[test]
fn test_const_eval_cache_persists_across_runs_with_nested_closure_map() {
  let code = r#"
num_contours = 3
points_per_contour = 4

contours = 0..num_contours -> |i| {
  a = i + 1
  0..points_per_contour -> |j| { a + j }
}
"#;

  let ctx = EvalCtx::default();

  let seq1 = optimize_and_get_sequence(&ctx, code, 2);
  let seq2 = optimize_and_get_sequence(&ctx, code, 2);

  assert!(Rc::ptr_eq(&seq1, &seq2));
}

#[test]
fn test_closure_hash_distinguishes_body() {
  let ctx = EvalCtx::default();

  let code1 = "a = 0..4 -> |x| x + 1";
  let mut ast1 = crate::parse_program_src(&ctx, code1).unwrap();
  optimize_ast(&ctx, &mut ast1).unwrap();
  let seq1 = match &ast1.statements[0] {
    Statement::Assignment { expr, .. } => match expr {
      Expr::Literal(Value::Sequence(seq)) => Rc::clone(seq),
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  let code2 = "a = 0..4 -> |x| x + 2";
  let mut ast2 = crate::parse_program_src(&ctx, code2).unwrap();
  optimize_ast(&ctx, &mut ast2).unwrap();
  let seq2 = match &ast2.statements[0] {
    Statement::Assignment { expr, .. } => match expr {
      Expr::Literal(Value::Sequence(seq)) => Rc::clone(seq),
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  assert!(!Rc::ptr_eq(&seq1, &seq2));
}

#[test]
fn test_bad_optim_repro() {
  let code = r#"
build_curl = || {
  build_path = || [v3(0), v3(1)]

  contours = [build_path()]
    -> collect
    | collect

  0..len(contours[0])
    -> |i| { 0..4 -> |j| contours[0][i] }
    | stitch_contours
}

build_curl()"#;

  crate::parse_and_eval_program(code).unwrap();
}
