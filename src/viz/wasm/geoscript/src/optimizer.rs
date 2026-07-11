use std::{cell::RefCell, hash::Hash, ops::ControlFlow, rc::Rc};

use fxhash::FxHashMap;
use rand::Rng;
use rand_pcg::Pcg32;
use siphasher::sip128::{Hasher128, SipHasher};

use crate::{
  ast::{
    bind_closure_params_into_scope, eval_range, maybe_pre_resolve_builtin_call_signature,
    record_non_const_binding, BinOp, ClosureArg, DestructurePattern, Expr, FunctionCall,
    FunctionCallTarget, MapLiteralEntry, PrefixOp, ScopeTracker, SourceLoc, Statement,
    TopLevelStatement, TrackedValue, TrackedValueRef,
  },
  builtins::{
    fn_defs::{fn_sigs, get_builtin_fn_sig_entry_ix},
    resolve_builtin_impl, FUNCTION_ALIASES,
  },
  match_binop_by_arg_types,
  seq::EagerSeq,
  type_infer::infer_expr,
  ArgType, Callable, CapturedScope, Closure, ErrorStack, EvalCtx, Program, Scope, Sym, Value, Vec2,
  Vec3,
};

/// This is essentially a `-ffast-math` flag for the optimizer's constant folding of associative
/// operations.  It allows re-ordering of floating point operations which can technically create
/// slightly different results in some cases.  For almost everything done in Geoscript/Geotoy, it's
/// unlikely to matter though.
const FLOAT_ASSOC_FOLDING_ENABLED: bool = true;

/// Which pieces of ambient interpreter state a hashed computation transitively consumes.  `rng`
/// mirrors the historical `uses_rng` flag; `settings` covers the sharp/curve angle thresholds.
#[derive(Clone, Copy, Default)]
struct Uses {
  rng: bool,
  settings: bool,
}

#[derive(Clone, Copy)]
struct ConstEvalCacheLookup {
  key: u128,
  uses: Uses,
}

fn const_eval_cache_lookup_with(
  ctx: &EvalCtx,
  allow_rng_const_eval: bool,
  hash_fn: impl FnOnce(&mut SipHasher, &mut Uses) -> Option<()>,
) -> Option<ConstEvalCacheLookup> {
  let mut hasher = SipHasher::new_with_keys(0, 0);
  let mut uses = Uses::default();
  hash_fn(&mut hasher, &mut uses)?;
  if uses.rng {
    if !allow_rng_const_eval {
      return None;
    }
    let rng_state = ctx.rng_state();
    hash_rng_state(&mut hasher, &rng_state);
  }
  // Ambient settings are inputs to computations that read them; mixing the live values into the
  // key lets entries computed under different thresholds coexist (multi-program batch case).
  // Marked as a read since a cache hit skips the impl-level read.
  if uses.settings {
    ctx.mark_settings_read();
    ctx
      .sharp_angle_threshold_degrees
      .borrow()
      .to_bits()
      .hash(&mut hasher);
    ctx
      .default_curve_angle_degrees
      .borrow()
      .to_bits()
      .hash(&mut hasher);
  }
  let key = hasher.finish128().as_u128();
  Some(ConstEvalCacheLookup { key, uses })
}

/// True when executing this fold would consume ambient state the optimizer can no longer prove
/// matches runtime state (see the `fold_*` flags on `EvalCtx`).  An unhashable computation
/// (`lookup` = None) is blocked conservatively whenever any tracking has broken down.
fn ambient_fold_blocked(
  ctx: &EvalCtx,
  lookup: &Option<ConstEvalCacheLookup>,
  allow_rng_const_eval: bool,
) -> bool {
  match lookup {
    Some(l) => {
      (l.uses.settings
        && (ctx.fold_settings_unknown.get()
          || (!allow_rng_const_eval && ctx.fold_settings_deferred_unsafe.get())))
        || (l.uses.rng && ctx.fold_rng_unknown.get())
    }
    None => {
      ctx.fold_settings_unknown.get()
        || ctx.fold_rng_unknown.get()
        || (!allow_rng_const_eval && ctx.fold_settings_deferred_unsafe.get())
    }
  }
}

fn const_eval_cache_get(ctx: &EvalCtx, lookup: ConstEvalCacheLookup) -> Option<Value> {
  let hit = ctx.const_eval_cache.borrow_mut().get(lookup.key)?;
  if let Some(rng_end_state) = hit.rng_end_state {
    ctx.set_rng_state(rng_end_state);
  }
  Some(hit.value)
}

fn const_eval_cache_store(ctx: &EvalCtx, lookup: ConstEvalCacheLookup, value: Value) {
  let rng_end_state = if lookup.uses.rng {
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

/// The three builtins that mutate ambient interpreter state consumed by other builtins.  The
/// const-folding pass executes analyzable top-level setter statements at fold time so fold-time
/// state tracks runtime statement order; anything it can't analyze flips the matching
/// `EvalCtx::fold_*_unknown` flag instead.
#[derive(Clone, Copy, PartialEq, Debug)]
enum AmbientSetter {
  SharpAngle,
  CurveAngle,
  RngSeed,
}

impl AmbientSetter {
  const fn name(self) -> &'static str {
    match self {
      Self::SharpAngle => "set_sharp_angle_threshold",
      Self::CurveAngle => "set_curve_angle_threshold",
      Self::RngSeed => "set_rng_seed",
    }
  }

  fn from_name(name: &str) -> Option<Self> {
    Some(match name {
      "set_sharp_angle_threshold" => Self::SharpAngle,
      "set_curve_angle_threshold" => Self::CurveAngle,
      "set_rng_seed" => Self::RngSeed,
      _ => return None,
    })
  }

  fn of_callable(callable: &Callable) -> Option<Self> {
    match callable {
      Callable::Builtin { fn_entry_ix, .. } => Self::from_name(fn_sigs().entries[*fn_entry_ix].0),
      Callable::PartiallyAppliedFn(paf) => Self::of_callable(&paf.inner),
      Callable::ComposedFn(composed) => composed.inner.iter().find_map(|c| Self::of_callable(c)),
      _ => None,
    }
  }

  fn to_callable(self) -> Rc<Callable> {
    Rc::new(Callable::Builtin {
      fn_entry_ix: get_builtin_fn_sig_entry_ix(self.name()).unwrap(),
      fn_impl: resolve_builtin_impl(self.name()),
      pre_resolved_signature: None,
    })
  }

  fn mark_unknown(self, ctx: &EvalCtx) {
    match self {
      Self::SharpAngle | Self::CurveAngle => ctx.fold_settings_unknown.set(true),
      Self::RngSeed => ctx.fold_rng_unknown.set(true),
    }
  }
}

fn callable_requires_rng_state(callable: &Callable) -> bool {
  match callable {
    Callable::Builtin { .. } => callable.is_rng_dependent(),
    _ => true, // TODO: is this really the best we can do?
  }
}

fn hash_rng_state(hasher: &mut SipHasher, rng_state: &Pcg32) {
  // `Pcg32` has a custom `Debug` impl that hides its internal state and no
  // public accessors for it, so fingerprint by pulling bits from a clone.
  let mut clone = rng_state.clone();
  clone.next_u64().hash(hasher);
  clone.next_u64().hash(hasher);
}

#[derive(Clone, Copy)]
struct ExprHashConfig {
  /// Whether to track RNG usage (for const eval caching).
  /// When true, sets `Uses::rng` when encountering RNG-dependent callables.
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

  /// Structural traversal that still tracks ambient-state usage — for hashing callable *values*
  /// (closure args etc.) whose bodies contain dynamic exprs but whose rng/settings reads must
  /// influence the enclosing fold's cache key and gating.
  const fn const_eval_value() -> Self {
    Self {
      track_rng: true,
      allow_dynamic: true,
    }
  }
}

/// Unified expression hasher that handles both const eval and structural hashing modes.
fn hash_expr(
  expr: &Expr,
  hasher: &mut SipHasher,
  uses: &mut Uses,
  config: ExprHashConfig,
) -> Option<()> {
  std::mem::discriminant(expr).hash(hasher);
  match expr {
    Expr::BinOp { op, lhs, rhs, .. } => {
      std::mem::discriminant(op).hash(hasher);
      hash_expr(lhs, hasher, uses, config)?;
      hash_expr(rhs, hasher, uses, config)?;
      if config.track_rng && matches!(op, BinOp::Pipeline | BinOp::Map) {
        if let Expr::Literal {
          value: Value::Callable(callable),
          ..
        } = rhs.as_ref()
        {
          if callable_requires_rng_state(callable) {
            uses.rng = true;
          }
          uses.settings |= callable.reads_ctx_settings();
        }
      }
      Some(())
    }
    Expr::PrefixOp {
      op, expr: inner, ..
    } => {
      std::mem::discriminant(op).hash(hasher);
      hash_expr(inner, hasher, uses, config)
    }
    Expr::Range {
      start,
      end,
      inclusive,
      ..
    } => {
      inclusive.hash(hasher);
      hash_expr(start, hasher, uses, config)?;
      std::mem::discriminant(end).hash(hasher);
      if let Some(end) = end {
        hash_expr(end, hasher, uses, config)?;
      }
      Some(())
    }
    Expr::StaticFieldAccess { lhs, field, .. } => {
      field.hash(hasher);
      hash_expr(lhs, hasher, uses, config)
    }
    Expr::FieldAccess { lhs, field, .. } => {
      hash_expr(lhs, hasher, uses, config)?;
      hash_expr(field, hasher, uses, config)
    }
    Expr::Call {
      call: FunctionCall {
        target,
        args,
        kwargs,
      },
      ..
    } => {
      std::mem::discriminant(target).hash(hasher);
      match target {
        FunctionCallTarget::Literal(callable) => {
          hash_callable(callable, hasher, uses, config)?;
          if config.track_rng && callable_requires_rng_state(callable) {
            uses.rng = true;
          }
          if config.track_rng {
            uses.settings |= callable.reads_ctx_settings();
          }
        }
        FunctionCallTarget::Name(name) => {
          if !config.allow_dynamic {
            return None;
          }
          name.hash(hasher);
          // An unresolved call target could invoke anything at runtime.
          if config.track_rng {
            uses.rng = true;
            uses.settings = true;
          }
        }
      }
      args.len().hash(hasher);
      for arg in args {
        hash_expr(arg, hasher, uses, config)?;
      }
      let mut keys = kwargs.keys().copied().collect::<Vec<_>>();
      keys.sort_by_key(|k| k.0);
      for key in keys {
        key.hash(hasher);
        let expr = kwargs.get(&key)?;
        hash_expr(expr, hasher, uses, config)?;
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
      hash_closure_parts(params, body, return_type_hint, hasher, uses, config)
    }
    Expr::Ident { name, .. } => {
      if !config.allow_dynamic {
        return None;
      }
      name.hash(hasher);
      Some(())
    }
    Expr::ArrayLiteral {
      elements: exprs, ..
    } => {
      exprs.len().hash(hasher);
      for expr in exprs {
        hash_expr(expr, hasher, uses, config)?;
      }
      Some(())
    }
    Expr::MapLiteral { entries, .. } => {
      entries.len().hash(hasher);
      for entry in entries {
        std::mem::discriminant(entry).hash(hasher);
        match entry {
          MapLiteralEntry::KeyValue { key, value } => {
            key.hash(hasher);
            hash_expr(value, hasher, uses, config)?;
          }
          MapLiteralEntry::Splat { expr } => {
            hash_expr(expr, hasher, uses, config)?;
          }
        }
      }
      Some(())
    }
    Expr::Literal { value, .. } => hash_value(value, hasher, uses),
    Expr::Conditional {
      cond,
      then,
      else_if_exprs,
      else_expr,
      ..
    } => {
      if !config.allow_dynamic {
        return None;
      }
      hash_expr(cond, hasher, uses, config)?;
      hash_expr(then, hasher, uses, config)?;
      else_if_exprs.len().hash(hasher);
      for (cond, expr) in else_if_exprs {
        hash_expr(cond, hasher, uses, config)?;
        hash_expr(expr, hasher, uses, config)?;
      }
      std::mem::discriminant(else_expr).hash(hasher);
      if let Some(expr) = else_expr {
        hash_expr(expr, hasher, uses, config)?;
      }
      Some(())
    }
    Expr::Block { statements, .. } => {
      if !config.allow_dynamic {
        return None;
      }
      statements.len().hash(hasher);
      for stmt in statements {
        hash_statement(stmt, hasher, uses, config)?;
      }
      Some(())
    }
  }
}

fn hash_type_name(type_name: ArgType, hasher: &mut SipHasher) {
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
  uses: &mut Uses,
  config: ExprHashConfig,
) -> Option<()> {
  hash_destructure_pattern(&arg.ident, hasher)?;
  std::mem::discriminant(&arg.type_hint).hash(hasher);
  if let Some(type_hint) = arg.type_hint {
    hash_type_name(type_hint, hasher);
  }
  std::mem::discriminant(&arg.default_val).hash(hasher);
  if let Some(default_val) = &arg.default_val {
    hash_expr(default_val, hasher, uses, config)?;
  }
  Some(())
}

fn hash_statement(
  stmt: &Statement,
  hasher: &mut SipHasher,
  uses: &mut Uses,
  config: ExprHashConfig,
) -> Option<()> {
  std::mem::discriminant(stmt).hash(hasher);
  match stmt {
    Statement::Assignment {
      name,
      expr,
      type_hint,
      ..
    } => {
      name.hash(hasher);
      std::mem::discriminant(type_hint).hash(hasher);
      if let Some(type_hint) = type_hint {
        hash_type_name(*type_hint, hasher);
      }
      hash_expr(expr, hasher, uses, config)
    }
    Statement::DestructureAssignment { lhs, rhs } => {
      hash_destructure_pattern(lhs, hasher)?;
      hash_expr(rhs, hasher, uses, config)
    }
    Statement::Expr(expr) => hash_expr(expr, hasher, uses, config),
    Statement::Return { value } => {
      std::mem::discriminant(value).hash(hasher);
      if let Some(expr) = value {
        hash_expr(expr, hasher, uses, config)?;
      }
      Some(())
    }
    Statement::Break { value } => {
      std::mem::discriminant(value).hash(hasher);
      if let Some(expr) = value {
        hash_expr(expr, hasher, uses, config)?;
      }
      Some(())
    }
  }
}

fn hash_closure_parts(
  params: &Rc<Vec<ClosureArg>>,
  body: &Rc<crate::ast::ClosureBody>,
  return_type_hint: &Option<ArgType>,
  hasher: &mut SipHasher,
  uses: &mut Uses,
  config: ExprHashConfig,
) -> Option<()> {
  params.len().hash(hasher);
  for param in params.iter() {
    hash_closure_arg(param, hasher, uses, config)?;
  }
  std::mem::discriminant(return_type_hint).hash(hasher);
  if let Some(type_hint) = return_type_hint {
    hash_type_name(*type_hint, hasher);
  }
  body.0.len().hash(hasher);
  for stmt in body.0.iter() {
    hash_statement(stmt, hasher, uses, config)?;
  }
  Some(())
}

fn hash_call(
  callable: &Rc<Callable>,
  args: &[Expr],
  kwargs: &FxHashMap<Sym, Expr>,
  hasher: &mut SipHasher,
  uses: &mut Uses,
  config: ExprHashConfig,
) -> Option<()> {
  // Hash a marker for function calls
  std::mem::discriminant(&Expr::Call {
    call: FunctionCall {
      target: FunctionCallTarget::Literal(Rc::clone(callable)),
      args: Vec::new(),
      kwargs: FxHashMap::default(),
    },
    loc: SourceLoc::default(),
  })
  .hash(hasher);
  std::mem::discriminant(&FunctionCallTarget::Literal(Rc::clone(callable))).hash(hasher);
  hash_callable(callable, hasher, uses, config)?;
  if config.track_rng && callable_requires_rng_state(callable) {
    uses.rng = true;
  }
  if config.track_rng {
    uses.settings |= callable.reads_ctx_settings();
  }
  args.len().hash(hasher);
  for arg in args {
    hash_expr(arg, hasher, uses, config)?;
  }
  let mut keys = kwargs.keys().copied().collect::<Vec<_>>();
  keys.sort_by_key(|k| k.0);
  for key in keys {
    key.hash(hasher);
    let expr = kwargs.get(&key)?;
    hash_expr(expr, hasher, uses, config)?;
  }
  Some(())
}

fn hash_callable(
  callable: &Rc<Callable>,
  hasher: &mut SipHasher,
  uses: &mut Uses,
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
      uses,
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

  let cache_lookup = const_eval_cache_lookup_with(ctx, allow_rng_const_eval, |hasher, uses| {
    hash_call(
      callable,
      args,
      kwargs,
      hasher,
      uses,
      ExprHashConfig::const_eval(),
    )
  });
  if ambient_fold_blocked(ctx, &cache_lookup, allow_rng_const_eval) {
    return Ok(None);
  }
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

fn hash_value(value: &Value, hasher: &mut SipHasher, uses: &mut Uses) -> Option<()> {
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
      // Closure bodies are walked with tracking on, so their rng/settings usage propagates
      // precisely; opaque callables (partials, composed, dynamic) get conservative flags.
      hash_callable(callable, hasher, uses, ExprHashConfig::const_eval_value())?;
      if !matches!(&**callable, Callable::Closure(_)) {
        uses.rng |= callable_requires_rng_state(callable);
        uses.settings |= callable.reads_ctx_settings();
      }
    }
    Value::Sequence(seq) => {
      // Opaque: may wrap closures that draw rng / read settings when consumed.
      (Rc::as_ptr(seq) as *const () as usize).hash(hasher);
      uses.rng = true;
      uses.settings = true;
    }
    Value::Map(map) => {
      (Rc::as_ptr(map) as *const () as usize).hash(hasher);
    }
    Value::Mat4(mat) => {
      (Rc::as_ptr(mat) as *const () as usize).hash(hasher);
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
  let mut env = local_scope.build_type_env(ctx);
  match infer_expr(ctx, &mut env, expr).as_single_arg_type() {
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
    let lhs_loc = lhs.loc();
    let mut rhs_expr = std::mem::replace(
      rhs,
      Box::new(Expr::Literal {
        value: Value::Nil,
        loc: SourceLoc::default(),
      }),
    );
    let mut changed = false;

    if let Expr::BinOp {
      op: rhs_op,
      lhs: rhs_lhs,
      rhs: rhs_rhs,
      ..
    } = &mut *rhs_expr
    {
      if *rhs_op == op {
        if let Some(rhs_lhs_val) = rhs_lhs.as_literal().cloned() {
          if can_fold_assoc_literals(ctx, local_scope, &lhs_val, &rhs_lhs_val, rhs_rhs.as_ref()) {
            let new_val = op.apply(ctx, &lhs_val, &rhs_lhs_val, None).map_err(|err| {
              let (line, col) = ctx.resolve_loc(lhs_loc);
              err.with_loc(line, col)
            })?;
            *lhs = Box::new(new_val.into_literal_expr(lhs_loc));
            rhs_expr = std::mem::replace(
              rhs_rhs,
              Box::new(Expr::Literal {
                value: Value::Nil,
                loc: SourceLoc::default(),
              }),
            );
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
    let rhs_loc = rhs.loc();
    let mut lhs_expr = std::mem::replace(
      lhs,
      Box::new(Expr::Literal {
        value: Value::Nil,
        loc: SourceLoc::default(),
      }),
    );
    let mut changed = false;

    if let Expr::BinOp {
      op: lhs_op,
      lhs: lhs_lhs,
      rhs: lhs_rhs,
      ..
    } = &mut *lhs_expr
    {
      if *lhs_op == op {
        if let Some(lhs_rhs_val) = lhs_rhs.as_literal().cloned() {
          if can_fold_assoc_literals(ctx, local_scope, &lhs_rhs_val, &rhs_val, lhs_lhs.as_ref()) {
            let new_val = op.apply(ctx, &lhs_rhs_val, &rhs_val, None).map_err(|err| {
              let (line, col) = ctx.resolve_loc(rhs_loc);
              err.with_loc(line, col)
            })?;
            *rhs = Box::new(new_val.into_literal_expr(rhs_loc));
            lhs_expr = std::mem::replace(
              lhs_lhs,
              Box::new(Expr::Literal {
                value: Value::Nil,
                loc: SourceLoc::default(),
              }),
            );
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

fn same_arg_type(a: Option<ArgType>, b: Option<ArgType>) -> bool {
  match (a, b) {
    (Some(a), Some(b)) => a.as_bitflags() == b.as_bitflags(),
    _ => false,
  }
}

fn is_scalar_zero(v: &Value) -> bool {
  matches!(v, Value::Int(0)) || matches!(v, Value::Float(f) if *f == 0.)
}

fn is_scalar_one(v: &Value) -> bool {
  matches!(v, Value::Int(1)) || matches!(v, Value::Float(f) if *f == 1.)
}

fn typed_zero(ty: ArgType) -> Option<Value> {
  Some(match ty {
    ArgType::Int => Value::Int(0),
    ArgType::Float | ArgType::Numeric => Value::Float(0.),
    ArgType::Vec2 => Value::Vec2(Vec2::new(0., 0.)),
    ArgType::Vec3 => Value::Vec3(Vec3::zeros()),
    _ => return None,
  })
}

/// Type-preserving algebraic identities (`x±0→x`, `x*1→x`, `x*0→0`, `x/1→x`, `0/x→0`).  Only fires
/// when exactly one operand is a literal, and never changes the result type (so vector widths are
/// preserved).  `x*0→0` / `0/x→0` assume finite operands, matching the optimizer's existing
/// fast-math folding.
fn try_identity_peephole(
  ctx: &EvalCtx,
  scope: &mut ScopeTracker,
  op: BinOp,
  lhs: &Expr,
  rhs: &Expr,
  loc: SourceLoc,
) -> Option<Expr> {
  if !matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div) {
    return None;
  }
  let lhs_lit = lhs.as_literal().cloned();
  let rhs_lit = rhs.as_literal().cloned();
  // Both-literal cases are handled exactly by the const-fold path below; skip them here.
  if lhs_lit.is_some() && rhs_lit.is_some() {
    return None;
  }
  if lhs_lit.is_none() && rhs_lit.is_none() {
    return None;
  }
  // Only 0/1 literals can match an identity; bail before the type inference + subtree cloning
  // below, which would otherwise tax every one-literal binop in the program.
  let lit = lhs_lit.as_ref().or(rhs_lit.as_ref()).unwrap();
  if !is_scalar_zero(lit) && !is_scalar_one(lit) {
    return None;
  }

  let infer = |scope: &mut ScopeTracker, e: &Expr| {
    let mut env = scope.build_type_env(ctx);
    infer_expr(ctx, &mut env, e).as_single_arg_type()
  };
  let result_ty = {
    let node = Expr::BinOp {
      op,
      lhs: Box::new(lhs.clone()),
      rhs: Box::new(rhs.clone()),
      pre_resolved_def_ix: None,
      loc,
    };
    infer(scope, &node)
  };

  match op {
    BinOp::Add => {
      if rhs_lit.as_ref().is_some_and(is_scalar_zero) && same_arg_type(infer(scope, lhs), result_ty)
      {
        return Some(lhs.clone());
      }
      if lhs_lit.as_ref().is_some_and(is_scalar_zero) && same_arg_type(infer(scope, rhs), result_ty)
      {
        return Some(rhs.clone());
      }
    }
    BinOp::Sub => {
      if rhs_lit.as_ref().is_some_and(is_scalar_zero) && same_arg_type(infer(scope, lhs), result_ty)
      {
        return Some(lhs.clone());
      }
      if lhs_lit.as_ref().is_some_and(is_scalar_zero) && same_arg_type(infer(scope, rhs), result_ty)
      {
        return Some(Expr::PrefixOp {
          op: PrefixOp::Neg,
          expr: Box::new(rhs.clone()),
          loc,
        });
      }
    }
    BinOp::Mul => {
      if rhs_lit.as_ref().is_some_and(is_scalar_one) && same_arg_type(infer(scope, lhs), result_ty)
      {
        return Some(lhs.clone());
      }
      if lhs_lit.as_ref().is_some_and(is_scalar_one) && same_arg_type(infer(scope, rhs), result_ty)
      {
        return Some(rhs.clone());
      }
      if rhs_lit.as_ref().is_some_and(is_scalar_zero)
        || lhs_lit.as_ref().is_some_and(is_scalar_zero)
      {
        if let Some(zero) = result_ty.and_then(typed_zero) {
          return Some(zero.into_literal_expr(loc));
        }
      }
    }
    BinOp::Div => {
      if rhs_lit.as_ref().is_some_and(is_scalar_one) && same_arg_type(infer(scope, lhs), result_ty)
      {
        return Some(lhs.clone());
      }
      if lhs_lit.as_ref().is_some_and(is_scalar_zero) {
        if let Some(zero) = result_ty.and_then(typed_zero) {
          return Some(zero.into_literal_expr(loc));
        }
      }
    }
    _ => {}
  }
  None
}

/// Optimize the body of a compiler-synthesized closure (e.g. an autodiff derivative).  Seeds the
/// scope tracker with the closure's params (as args) and captured constants so `optimize_expr`
/// resolves every identifier, then const-folds each statement in order.
pub(crate) fn optimize_synthesized_closure_body(
  ctx: &EvalCtx,
  params: &[ClosureArg],
  captured_consts: &[(Sym, Value)],
  stmts: &mut [Statement],
) -> Result<(), ErrorStack> {
  let mut scope = ScopeTracker::default();
  for (sym, val) in captured_consts {
    scope
      .vars
      .entry(*sym)
      .or_insert_with(|| TrackedValue::Const(val.clone()));
  }
  bind_closure_params_into_scope(&mut scope, params);
  for stmt in stmts.iter_mut() {
    optimize_statement(ctx, &mut scope, stmt, false)?;
  }
  Ok(())
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
      loc,
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
              let (line, col) = ctx.resolve_loc(*loc);
              return Err(
                ErrorStack::new(format!(
                  "Left-hand side of logical operator must be a boolean, found: {lhs_lit:?}",
                ))
                .with_loc(line, col),
              );
            }
          };

          match op {
            BinOp::And => {
              if !lhs_bool {
                *expr = Value::Bool(false).into_literal_expr(*loc);
                return Ok(());
              }
            }
            BinOp::Or => {
              if lhs_bool {
                *expr = Value::Bool(true).into_literal_expr(*loc);
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

      if let Some(name) = op.get_builtin_fn_name() {
        if let Some(entry_ix) = get_builtin_fn_sig_entry_ix(name) {
          let mut env = local_scope.build_type_env(ctx);
          if let (Some(lhs_ty), Some(rhs_ty)) = (
            infer_expr(ctx, &mut env, lhs).as_single_arg_type(),
            infer_expr(ctx, &mut env, rhs).as_single_arg_type(),
          ) {
            if let Some((def_ix, _)) = match_binop_by_arg_types(entry_ix, lhs_ty, rhs_ty) {
              *pre_resolved_def_ix = Some(def_ix);
            }
          }
        }
      }

      if let Some(simplified) = try_identity_peephole(ctx, local_scope, *op, lhs, rhs, *loc) {
        *expr = simplified;
        return Ok(());
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
        lhs: Box::new(Expr::Literal {
          value: Value::Nil,
          loc: SourceLoc::default(),
        }),
        rhs: Box::new(Expr::Literal {
          value: Value::Nil,
          loc: SourceLoc::default(),
        }),
        pre_resolved_def_ix: None,
        loc: SourceLoc::default(),
      });
      let op_discriminant = std::mem::discriminant(op);
      let expr_loc = *loc;
      let cache_lookup = const_eval_cache_lookup_with(ctx, allow_rng_const_eval, |hasher, uses| {
        binop_discriminant.hash(hasher);
        op_discriminant.hash(hasher);
        hash_expr(lhs, hasher, uses, ExprHashConfig::const_eval())?;
        hash_expr(rhs, hasher, uses, ExprHashConfig::const_eval())?;
        // Already handled by hash_expr with const_eval config, but add explicit check for clarity
        if matches!(op, BinOp::Pipeline | BinOp::Map) {
          if let Expr::Literal {
            value: Value::Callable(callable),
            ..
          } = rhs.as_ref()
          {
            if callable_requires_rng_state(callable) {
              uses.rng = true;
            }
            uses.settings |= callable.reads_ctx_settings();
          }
        }
        Some(())
      });
      if ambient_fold_blocked(ctx, &cache_lookup, allow_rng_const_eval) {
        return Ok(());
      }
      if let Some(lookup) = cache_lookup {
        if let Some(cached) = const_eval_cache_get(ctx, lookup) {
          *expr = cached.into_literal_expr(expr_loc);
          return Ok(());
        }
      }

      let val = op
        .apply(ctx, lhs_val, rhs_val, *pre_resolved_def_ix)
        .map_err(|err| {
          let (line, col) = ctx.resolve_loc(*loc);
          err.with_loc(line, col)
        })?;
      if let Some(lookup) = cache_lookup {
        const_eval_cache_store(ctx, lookup, val.clone());
      }
      *expr = val.into_literal_expr(expr_loc);
      Ok(())
    }
    Expr::PrefixOp {
      op,
      expr: inner,
      loc,
    } => {
      optimize_expr(ctx, local_scope, inner, allow_rng_const_eval)?;

      // `neg(neg x) -> x`
      if matches!(op, PrefixOp::Neg) {
        if let Expr::PrefixOp {
          op: PrefixOp::Neg,
          expr: inner_inner,
          ..
        } = inner.as_ref()
        {
          *expr = (**inner_inner).clone();
          return Ok(());
        }
      }

      let Some(val) = inner.as_literal() else {
        return Ok(());
      };
      let prefix_discriminant = std::mem::discriminant(&Expr::PrefixOp {
        op: *op,
        expr: Box::new(Expr::Literal {
          value: Value::Nil,
          loc: SourceLoc::default(),
        }),
        loc: SourceLoc::default(),
      });
      let op_discriminant = std::mem::discriminant(op);
      let expr_loc = *loc;
      let cache_lookup = const_eval_cache_lookup_with(ctx, allow_rng_const_eval, |hasher, uses| {
        prefix_discriminant.hash(hasher);
        op_discriminant.hash(hasher);
        hash_expr(inner, hasher, uses, ExprHashConfig::const_eval())
      });
      if let Some(lookup) = cache_lookup {
        if let Some(cached) = const_eval_cache_get(ctx, lookup) {
          *expr = cached.into_literal_expr(expr_loc);
          return Ok(());
        }
      }
      let val = op.apply(ctx, val)?;
      if let Some(lookup) = cache_lookup {
        const_eval_cache_store(ctx, lookup, val.clone());
      }
      *expr = val.into_literal_expr(expr_loc);
      Ok(())
    }
    Expr::Range {
      start,
      end,
      inclusive,
      loc,
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
      let cache_lookup = const_eval_cache_lookup_with(ctx, allow_rng_const_eval, |hasher, uses| {
        std::mem::discriminant(&Expr::Range {
          start: Box::new(Expr::Literal {
            value: Value::Nil,
            loc: SourceLoc::default(),
          }),
          end: None,
          inclusive: false,
          loc: SourceLoc::default(),
        })
        .hash(hasher);
        inclusive.hash(hasher);
        hash_value(start_val, hasher, uses)?;
        std::mem::discriminant(&end_val_opt).hash(hasher);
        if let Some(end_val) = end_val_opt {
          hash_value(end_val, hasher, uses)?;
        }
        Some(())
      });
      if let Some(lookup) = cache_lookup {
        if let Some(cached) = const_eval_cache_get(ctx, lookup) {
          *expr = cached.into_literal_expr(*loc);
          return Ok(());
        }
      }
      let val = eval_range(start_val, end_val_opt, *inclusive).map_err(|err| {
        let (line, col) = ctx.resolve_loc(*loc);
        err.with_loc(line, col)
      })?;
      if let Some(lookup) = cache_lookup {
        const_eval_cache_store(ctx, lookup, val.clone());
      }
      *expr = val.into_literal_expr(*loc);
      Ok(())
    }
    Expr::StaticFieldAccess { lhs, field, loc } => {
      optimize_expr(ctx, local_scope, lhs, allow_rng_const_eval)?;

      let Some(lhs_val) = lhs.as_literal() else {
        return Ok(());
      };

      let static_access_discriminant = std::mem::discriminant(&Expr::StaticFieldAccess {
        lhs: Box::new(Expr::Literal {
          value: Value::Nil,
          loc: SourceLoc::default(),
        }),
        field: field.clone(),
        loc: SourceLoc::default(),
      });
      let cache_lookup = const_eval_cache_lookup_with(ctx, allow_rng_const_eval, |hasher, uses| {
        static_access_discriminant.hash(hasher);
        field.hash(hasher);
        hash_expr(lhs, hasher, uses, ExprHashConfig::const_eval())
      });
      if let Some(lookup) = cache_lookup {
        if let Some(cached) = const_eval_cache_get(ctx, lookup) {
          *expr = cached.into_literal_expr(*loc);
          return Ok(());
        }
      }

      let val = ctx.eval_static_field_access(lhs_val, field)?;
      if let Some(lookup) = cache_lookup {
        const_eval_cache_store(ctx, lookup, val.clone());
      }
      *expr = val.into_literal_expr(*loc);

      Ok(())
    }
    Expr::FieldAccess { lhs, field, loc } => {
      optimize_expr(ctx, local_scope, lhs, allow_rng_const_eval)?;
      optimize_expr(ctx, local_scope, field, allow_rng_const_eval)?;

      let (Some(lhs_val), Some(field_val)) = (lhs.as_literal(), field.as_literal()) else {
        return Ok(());
      };

      let field_access_discriminant = std::mem::discriminant(&Expr::FieldAccess {
        lhs: Box::new(Expr::Literal {
          value: Value::Nil,
          loc: SourceLoc::default(),
        }),
        field: Box::new(Expr::Literal {
          value: Value::Nil,
          loc: SourceLoc::default(),
        }),
        loc: SourceLoc::default(),
      });
      let cache_lookup = const_eval_cache_lookup_with(ctx, allow_rng_const_eval, |hasher, uses| {
        field_access_discriminant.hash(hasher);
        hash_expr(lhs, hasher, uses, ExprHashConfig::const_eval())?;
        hash_expr(field, hasher, uses, ExprHashConfig::const_eval())
      });
      if let Some(lookup) = cache_lookup {
        if let Some(cached) = const_eval_cache_get(ctx, lookup) {
          *expr = cached.into_literal_expr(*loc);
          return Ok(());
        }
      }

      let val = ctx.eval_field_access(lhs_val, field_val)?;
      if let Some(lookup) = cache_lookup {
        const_eval_cache_store(ctx, lookup, val.clone());
      }
      *expr = val.into_literal_expr(*loc);

      Ok(())
    }
    Expr::Call {
      call: FunctionCall {
        target,
        args,
        kwargs,
      },
      loc,
    } => {
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
                let (line, col) = ctx.resolve_loc(*loc);
                return ctx.with_resolved_sym(*name, |name| {
                  Err(
                    ErrorStack::new(format!(
                      "Tried to call non-callable value: {name} = {other:?}",
                    ))
                    .with_loc(line, col),
                  )
                });
              }
            },
            // calling a closure argument or dynamic captured variable
            TrackedValueRef::Arg => (),
            TrackedValueRef::Dyn => (),
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
                None => {
                  let (line, col) = ctx.resolve_loc(*loc);
                  Err(
                    ErrorStack::new(format!("Variable or function not found: {name}"))
                      .with_loc(line, col),
                  )
                }
              },
            })?;

          *target = FunctionCallTarget::Literal(Rc::new(Callable::Builtin {
            fn_entry_ix,
            fn_impl,
            pre_resolved_signature: None,
          }));
        }
      }

      for arg in args.iter_mut() {
        optimize_expr(ctx, local_scope, arg, allow_rng_const_eval)?;
      }
      for (_, expr) in kwargs.iter_mut() {
        optimize_expr(ctx, local_scope, expr, allow_rng_const_eval)?;
      }

      // Ambient setters reaching here are in non-statement position (sanctioned top-level setter
      // statements bypass `fold_constants` entirely).  Threshold setters are restricted to
      // top-level statements; `set_rng_seed` is legal anywhere but makes the fold-time rng stream
      // unknowable from this point on.
      if let FunctionCallTarget::Literal(callable) = target {
        match AmbientSetter::of_callable(callable) {
          Some(AmbientSetter::RngSeed) => ctx.fold_rng_unknown.set(true),
          Some(setter) => {
            let (line, col) = ctx.resolve_loc(*loc);
            return Err(
              ErrorStack::new(format!(
                "`{}` must be called as a top-level statement, not inside a closure, conditional, \
                 or other expression.  Move the call to the program's top level, or pass the \
                 angle directly to the function that needs it (e.g. `compute_normals(mesh, \
                 angle)` or a `curve_angle_degrees` kwarg).",
                setter.name()
              ))
              .with_loc(line, col),
            );
          }
          None => {}
        }
      }

      if let FunctionCallTarget::Literal(callable) = target {
        if let Callable::Builtin {
          fn_entry_ix,
          fn_impl,
          pre_resolved_signature,
        } = &**callable
        {
          if pre_resolved_signature.is_none() {
            let pre_resolved_signature = maybe_pre_resolve_builtin_call_signature(
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
                )
                .map_err(|err| {
                  let (line, col) = ctx.resolve_loc(*loc);
                  err.with_loc(line, col)
                })?,
                other => {
                  let (line, col) = ctx.resolve_loc(*loc);
                  return ctx
                    .with_resolved_sym(*name, |name| {
                      Err(
                        ErrorStack::new(format!(
                          "Tried to call non-callable value: {name} = {other:?}",
                        ))
                        .with_loc(line, col),
                      )
                    })
                    .unwrap();
                }
              },
              TrackedValueRef::Arg => None,
              TrackedValueRef::Dyn => None,
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
        )
        .map_err(|err| {
          let (line, col) = ctx.resolve_loc(*loc);
          err.with_loc(line, col)
        })?,
      };

      if let Some(evaled) = evaled {
        *expr = evaled.into_literal_expr(*loc);
      }
      Ok(())
    }
    Expr::Closure {
      params,
      body,
      arg_placeholder_scope,
      return_type_hint,
      loc,
    } => {
      let mut params_inner = (**params).clone();
      for param in &mut params_inner {
        if let Some(default_val) = &mut param.default_val {
          optimize_expr(ctx, local_scope, default_val, false)?;
        }
      }
      *params = Rc::new(params_inner);

      let mut closure_scope = ScopeTracker::wrap(local_scope);
      bind_closure_params_into_scope(&mut closure_scope, &params);

      // We use this scope for const capture checking/inlining to avoid situations where the closure
      // assigns a local variable with the same name as a non-const captured variable, shadowing it.
      //
      // This would hide the capture and cause incorrect behavior.
      let mut local_scope_with_args = ScopeTracker {
        vars: closure_scope.vars.clone(),
        types: closure_scope.types.clone(),
        parent: Some(local_scope),
      };

      let mut body_inner = (**body).clone();
      for stmt in &mut body_inner.0 {
        optimize_statement(ctx, &mut closure_scope, stmt, false)?;
      }
      *body = Rc::new(body_inner);

      for (name, val) in closure_scope.vars.iter() {
        match val {
          TrackedValue::Dyn | TrackedValue::Arg => {
            let ty = closure_scope
              .types
              .get(name)
              .cloned()
              .unwrap_or(crate::ty::AbstractType::Unknown);
            local_scope_with_args.set_with_type(*name, val.clone(), ty);
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

      *expr = Expr::Literal {
        value: Value::Callable(Rc::new(Callable::Closure(Closure {
          params: Rc::clone(&params),
          body: Rc::clone(&body),
          captured_scope: CapturedScope::Strong(Rc::new(Scope::default())),
          arg_placeholder_scope: RefCell::new(Some(std::mem::take(arg_placeholder_scope))),
          return_type_hint: *return_type_hint,
        }))),
        loc: *loc,
      };

      Ok(())
    }
    &mut Expr::Ident { name: id, loc } => {
      if let Some(val) = local_scope.get(id) {
        if let TrackedValueRef::Const(val) = val {
          *expr = val.clone().into_literal_expr(loc);
          return Ok(());
        } else {
          return Ok(());
        }
      }

      if let Some(val) = ctx.globals.get(id) {
        *expr = val.clone().into_literal_expr(loc);
        return Ok(());
      }

      let cf = ctx.with_resolved_sym(id, |resolved_name| {
        let builtin_name = if fn_sigs().contains_key(resolved_name) {
          Some(resolved_name)
        } else {
          FUNCTION_ALIASES.get(resolved_name).copied()
        };
        if let Some(builtin_name) = builtin_name {
          let Some(fn_entry_ix) = get_builtin_fn_sig_entry_ix(builtin_name) else {
            return ControlFlow::Continue(());
          };
          *expr = Expr::Literal {
            value: Value::Callable(Rc::new(Callable::Builtin {
              fn_entry_ix,
              fn_impl: resolve_builtin_impl(builtin_name),
              pre_resolved_signature: None,
            })),
            loc,
          };
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
          let (line, col) = ctx.resolve_loc(loc);
          Err(
            ErrorStack::new(format!("Variable or function not found: {resolved_name}"))
              .with_loc(line, col),
          )
        })
        .unwrap()
    }
    Expr::Literal { .. } => Ok(()),
    Expr::ArrayLiteral {
      elements: exprs,
      loc,
    } => {
      for inner in exprs.iter_mut() {
        optimize_expr(ctx, local_scope, inner, allow_rng_const_eval)?;
      }

      // if all elements are literals, can fold into an `EagerSeq`
      if exprs.iter().all(|e| e.is_literal()) {
        let array_discriminant = std::mem::discriminant(&Expr::ArrayLiteral {
          elements: vec![],
          loc: SourceLoc::default(),
        });
        let cache_lookup =
          const_eval_cache_lookup_with(ctx, allow_rng_const_eval, |hasher, uses| {
            array_discriminant.hash(hasher);
            exprs.len().hash(hasher);
            for inner in exprs.iter() {
              hash_expr(inner, hasher, uses, ExprHashConfig::const_eval())?;
            }
            Some(())
          });
        if let Some(lookup) = cache_lookup {
          if let Some(cached) = const_eval_cache_get(ctx, lookup) {
            *expr = cached.into_literal_expr(*loc);
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
        *expr = Expr::Literal {
          value: val,
          loc: *loc,
        };
      }

      Ok(())
    }
    Expr::MapLiteral { entries, loc } => {
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
        let map_discriminant = std::mem::discriminant(&Expr::MapLiteral {
          entries: vec![],
          loc: SourceLoc::default(),
        });
        let cache_lookup =
          const_eval_cache_lookup_with(ctx, allow_rng_const_eval, |hasher, uses| {
            map_discriminant.hash(hasher);
            entries.len().hash(hasher);
            for entry in entries.iter() {
              std::mem::discriminant(entry).hash(hasher);
              match entry {
                MapLiteralEntry::KeyValue { key, value } => {
                  key.hash(hasher);
                  hash_expr(value, hasher, uses, ExprHashConfig::const_eval())?;
                }
                MapLiteralEntry::Splat { expr: splat_expr } => {
                  hash_expr(splat_expr, hasher, uses, ExprHashConfig::const_eval())?;
                }
              }
            }
            Some(())
          });
        if let Some(lookup) = cache_lookup {
          if let Some(cached) = const_eval_cache_get(ctx, lookup) {
            *expr = cached.into_literal_expr(*loc);
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
                  let (line, col) = ctx.resolve_loc(*loc);
                  return Err(
                    ErrorStack::new(format!(
                      "Tried to splat value of type {:?} into map; expected a map.",
                      literal.get_type()
                    ))
                    .with_loc(line, col),
                  );
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
        *expr = Expr::Literal {
          value: val,
          loc: *loc,
        };
      }

      Ok(())
    }
    Expr::Conditional {
      cond,
      then,
      else_if_exprs,
      else_expr,
      ..
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
            parent_scope.set(name, TrackedValue::Arg);
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
    Expr::Block { statements, loc } => {
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

      let evaled_expr = Expr::Block {
        statements: statements.clone(),
        loc: *loc,
      };
      let evaled = ctx.eval_expr(&evaled_expr, &ctx.globals, None)?;
      match evaled {
        crate::ControlFlow::Continue(val) | crate::ControlFlow::Break(val) => {
          *expr = Expr::Literal {
            value: val,
            loc: *loc,
          }
        }
        crate::ControlFlow::Return(retval) => {
          // replace the block with a new one that just includes the return statement
          *expr = Expr::Block {
            statements: vec![Statement::Return {
              value: Some(Expr::Literal {
                value: retval,
                loc: *loc,
              }),
            }],
            loc: *loc,
          };
        }
      }

      Ok(())
    }
  }
}

/// Shared logic for `Statement::Assignment` and `TopLevelStatement::Export`: optimize the RHS,
/// then either store a folded literal as const or delegate to [`record_non_const_binding`].
fn optimize_simple_assignment(
  ctx: &EvalCtx,
  local_scope: &mut ScopeTracker,
  name: Sym,
  expr: &mut Expr,
  type_hint: Option<ArgType>,
  allow_rng_const_eval: bool,
) -> Result<(), ErrorStack> {
  // insert a placeholder for the variable in the local scope to support recursive calls
  // unless we're assigning to an existing variable
  if !local_scope.has(name) {
    match type_hint {
      Some(ty) => local_scope.set_with_type(
        name,
        TrackedValue::Dyn,
        crate::ty::AbstractType::Concrete(ty),
      ),
      None => local_scope.set(name, TrackedValue::Dyn),
    }
  }

  optimize_expr(ctx, local_scope, expr, allow_rng_const_eval)?;

  match expr.as_literal() {
    Some(val) => local_scope.set(name, TrackedValue::Const(val.clone())),
    None => record_non_const_binding(ctx, local_scope, name, expr, type_hint),
  }
  Ok(())
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
      ..
    } => optimize_simple_assignment(
      ctx,
      local_scope,
      *name,
      expr,
      *type_hint,
      allow_rng_const_eval,
    ),
    Statement::DestructureAssignment { lhs, rhs } => {
      // insert a placeholder for assigned variables in the local scope to support recursive calls
      // unless we're assigning to an existing variables
      for name in lhs.iter_idents() {
        if !local_scope.has(name) {
          // no way currently to get type data for stuff inside of maps/arrays
          local_scope.set(name, TrackedValue::Dyn);
        }
      }

      let (line, col) = ctx.resolve_loc(rhs.loc());
      optimize_expr(ctx, local_scope, rhs, allow_rng_const_eval)?;

      let Some(rhs) = rhs.as_literal() else {
        for name in lhs.iter_idents() {
          local_scope.set(name, TrackedValue::Arg);
        }
        return Ok(());
      };

      lhs
        .visit_assignments(ctx, rhs.clone(), &mut |lhs, rhs| {
          local_scope.set(lhs, TrackedValue::Const(rhs));
          Ok(())
        })
        .map_err(|err| {
          err
            .wrap("Error evaluating destructure assignment")
            .with_loc(line, col)
        })?;

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

fn optimize_top_level_statement<'a>(
  ctx: &EvalCtx,
  local_scope: &'a mut ScopeTracker,
  stmt: &mut TopLevelStatement,
  allow_rng_const_eval: bool,
) -> Result<(), ErrorStack> {
  match stmt {
    TopLevelStatement::Statement(inner) => {
      optimize_statement(ctx, local_scope, inner, allow_rng_const_eval)
    }
    TopLevelStatement::Export {
      name,
      expr,
      type_hint,
      ..
    } => optimize_simple_assignment(
      ctx,
      local_scope,
      *name,
      expr,
      *type_hint,
      allow_rng_const_eval,
    ),
    TopLevelStatement::Import { bindings, .. } => {
      // Cannot const-fold imports; mark all bound names as dynamic
      for name in bindings.iter_idents() {
        local_scope.set(name, TrackedValue::Dyn);
      }
      Ok(())
    }
  }
}

struct OptimizationPass {
  #[allow(dead_code)]
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

/// Resets the ambient-state fold flags for this pass, then walks the program for references that
/// invalidate parts of the tracking up front: any threshold-setter presence (or an import, whose
/// module may mutate thresholds when it evaluates) makes settings reads inside closure bodies
/// unsafe to fold, and a setter *referenced as a value* can be invoked anywhere at runtime, so the
/// matching state is unknowable for the whole program.
fn prescan_ambient_state(ctx: &EvalCtx, ast: &Program) {
  let setter_syms = [
    (
      ctx.interned_symbols.intern("set_sharp_angle_threshold"),
      AmbientSetter::SharpAngle,
    ),
    (
      ctx.interned_symbols.intern("set_curve_angle_threshold"),
      AmbientSetter::CurveAngle,
    ),
    (
      ctx.interned_symbols.intern("set_rng_seed"),
      AmbientSetter::RngSeed,
    ),
  ];
  let (mut has_threshold_setter, mut settings_unknown, mut rng_unknown) = (false, false, false);
  let mut cb = |expr: &Expr| {
    let (called, referenced) = match expr {
      Expr::Call {
        call:
          FunctionCall {
            target: FunctionCallTarget::Name(name),
            ..
          },
        ..
      } => (Some(*name), None),
      Expr::Ident { name, .. } => (None, Some(*name)),
      _ => (None, None),
    };
    for (sym, setter) in setter_syms {
      if called == Some(sym) && setter != AmbientSetter::RngSeed {
        has_threshold_setter = true;
      }
      if referenced == Some(sym) {
        match setter {
          AmbientSetter::RngSeed => rng_unknown = true,
          _ => settings_unknown = true,
        }
      }
    }
  };
  let mut has_import = false;
  for stmt in &ast.statements {
    has_import |= matches!(stmt, TopLevelStatement::Import { .. });
    stmt.traverse_exprs(&mut cb);
  }

  ctx.fold_settings_unknown.set(settings_unknown);
  ctx.fold_rng_unknown.set(rng_unknown);
  ctx
    .fold_settings_deferred_unsafe
    .set(has_threshold_setter || settings_unknown || has_import);
}

/// If `expr` is a call to an ambient setter (in sanctioned top-level statement position), handle
/// it: execute at fold time when the args are const so fold-time state tracks runtime statement
/// order, else mark the state unknowable.  The statement is kept either way; eval re-runs it
/// (idempotently) after the per-run state reset.
fn fold_exec_ambient_setter_stmt(
  ctx: &EvalCtx,
  local_scope: &mut ScopeTracker,
  expr: &mut Expr,
) -> Result<bool, ErrorStack> {
  let Expr::Call { call, loc } = expr else {
    return Ok(false);
  };
  let setter = match &call.target {
    FunctionCallTarget::Literal(callable) => AmbientSetter::of_callable(callable),
    FunctionCallTarget::Name(name) => match local_scope.get(*name) {
      Some(TrackedValueRef::Const(Value::Callable(callable))) => {
        AmbientSetter::of_callable(callable)
      }
      Some(_) => None,
      None => ctx.with_resolved_sym(*name, |name| {
        AmbientSetter::from_name(FUNCTION_ALIASES.get(name).copied().unwrap_or(name))
      }),
    },
  };
  let Some(setter) = setter else {
    return Ok(false);
  };
  // Eval expects builtin call targets to have been resolved to literals during optimization.
  call.target = FunctionCallTarget::Literal(setter.to_callable());

  for arg in call.args.iter_mut() {
    optimize_expr(ctx, local_scope, arg, true)?;
  }
  for kwarg in call.kwargs.values_mut() {
    optimize_expr(ctx, local_scope, kwarg, true)?;
  }

  let arg_vals: Option<Vec<Value>> = call.args.iter().map(|a| a.as_literal().cloned()).collect();
  let kwarg_vals: Option<FxHashMap<Sym, Value>> = call
    .kwargs
    .iter()
    .map(|(k, v)| v.as_literal().cloned().map(|v| (*k, v)))
    .collect();
  match (arg_vals, kwarg_vals) {
    (Some(arg_vals), Some(kwarg_vals)) => {
      ctx
        .invoke_callable(&setter.to_callable(), &arg_vals, &kwarg_vals)
        .map_err(|err| {
          let (line, col) = ctx.resolve_loc(*loc);
          err.with_loc(line, col)
        })?;
    }
    _ => setter.mark_unknown(ctx),
  }
  Ok(true)
}

fn run_const_folding_pass(ctx: &EvalCtx, ast: &mut Program) -> Result<(), ErrorStack> {
  prescan_ambient_state(ctx, ast);

  let mut local_scope = ScopeTracker::default();
  // Seed the tracker with the ambient scope's bindings
  if let Some(ambient) = ctx.ambient_scope.borrow().as_ref() {
    for (sym, val) in ambient.collect_bindings_innermost_first() {
      local_scope
        .vars
        .entry(sym)
        .or_insert(TrackedValue::Const(val));
    }
  }
  for stmt in &mut ast.statements {
    if let TopLevelStatement::Statement(Statement::Expr(expr)) = stmt {
      if fold_exec_ambient_setter_stmt(ctx, &mut local_scope, expr)? {
        continue;
      }
    } else if matches!(stmt, TopLevelStatement::Import { .. }) {
      // The module body may mutate thresholds when it evaluates at import time.
      ctx.fold_settings_unknown.set(true);
    }
    optimize_top_level_statement(ctx, &mut local_scope, stmt, true)?;
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
    lhs: Box::new(Expr::Literal {
      value: Value::Int(2),
      loc: SourceLoc::default(),
    }),
    rhs: Box::new(Expr::Literal {
      value: Value::Int(3),
      loc: SourceLoc::default(),
    }),
    pre_resolved_def_ix: None,
    loc: SourceLoc::default(),
  };
  let mut local_scope = ScopeTracker::default();
  let ctx = EvalCtx::default();
  optimize_expr(&ctx, &mut local_scope, &mut expr, true).unwrap();
  let Expr::Literal {
    value: Value::Int(5),
    ..
  } = expr
  else {
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
    TopLevelStatement::Statement(Statement::Expr(expr)) => expr.as_literal().unwrap(),
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

/// Side-effectful setters must survive const folding: their effect must run at eval time so it
/// takes hold during tree-mode module evaluation, not be baked away at fold time (then lost to
/// the per-run `reset`). Regressing `is_side_effectful` would silently fold these to `nil`.
#[test]
fn test_threshold_setters_not_const_folded() {
  let ctx = EvalCtx::default();
  for code in [
    "set_curve_angle_threshold(7)",
    "set_sharp_angle_threshold(30)",
  ] {
    let mut ast = crate::parse_program_src(&ctx, code).unwrap();
    optimize_ast(&ctx, &mut ast).unwrap();
    let TopLevelStatement::Statement(Statement::Expr(expr)) = &ast.statements[0] else {
      panic!(
        "expected an expression statement for `{code}`, got: {:?}",
        ast.statements[0]
      );
    };
    assert!(
      matches!(expr, Expr::Call { .. }),
      "`{code}` must not be const-folded, got: {expr:?}"
    );
  }
}

#[test]
fn test_builtin_alias_ident_optimizes_to_callable() {
  let ctx = EvalCtx::default();
  let mut ast = crate::parse_program_src(&ctx, "fn = trans").unwrap();
  optimize_ast(&ctx, &mut ast).unwrap();

  let TopLevelStatement::Statement(Statement::Assignment { expr, .. }) = &ast.statements[0] else {
    panic!("Expected assignment");
  };
  assert!(matches!(expr.as_literal(), Some(Value::Callable(_))));
}

#[test]
fn test_basic_const_closure_eval() {
  let code = r#"
fn = |x| x + 1
y = fn(2)
"#;

  let ctx = EvalCtx::default();
  let mut ast = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast).unwrap();

  let TopLevelStatement::Statement(Statement::Assignment { name, expr, .. }) = &ast.statements[1]
  else {
    panic!("Expected second statement to be an assignment");
  };
  assert_eq!(*name, ctx.interned_symbols.intern("y"));
  let Expr::Literal {
    value: Value::Int(3),
    ..
  } = expr
  else {
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
  optimize_ast(&ctx, &mut ast).unwrap();

  let TopLevelStatement::Statement(Statement::Assignment { name, expr, .. }) = &ast.statements[2]
  else {
    panic!("Expected second statement to be an assignment");
  };
  assert_eq!(*name, ctx.interned_symbols.intern("y"));
  match expr {
    Expr::Literal {
      value: Value::Int(3),
      ..
    } => {}
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
  let TopLevelStatement::Statement(Statement::Assignment { expr, .. }) = &ast.statements[1] else {
    unreachable!();
  };
  assert!(
    matches!(
      expr,
      Expr::Literal {
        value: Value::Mesh(_),
        ..
      }
    ),
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
  optimize_ast(&ctx, &mut ast).unwrap();

  let TopLevelStatement::Statement(Statement::Assignment { name, expr, .. }) = &ast.statements[2]
  else {
    panic!("Expected second statement to be an assignment");
  };
  assert_eq!(*name, ctx.interned_symbols.intern("y"));
  let Expr::Literal {
    value: Value::Int(7),
    ..
  } = expr
  else {
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

  let TopLevelStatement::Statement(Statement::Expr(expr)) = &ast.statements[0] else {
    panic!("Expected first statement to be an expression");
  };
  let Expr::Literal {
    value: Value::Int(3),
    ..
  } = expr
  else {
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
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => match expr {
      Expr::Literal {
        value: Value::Callable(callable),
        ..
      } => match &*callable {
        Callable::Closure(closure) => closure.body.clone(),
        _ => unreachable!(),
      },
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };
  let call_target = match &closure_body.0[0] {
    Statement::Expr(expr) => match expr {
      Expr::Call {
        call:
          FunctionCall {
            target: FunctionCallTarget::Literal(target),
            ..
          },
        ..
      } => target,
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

  let TopLevelStatement::Statement(Statement::Assignment { name, expr, .. }) = &ast.statements[2]
  else {
    panic!("Expected second statement to be an assignment");
  };
  assert_eq!(*name, ctx.interned_symbols.intern("y"));
  let Expr::Literal {
    value: Value::Int(3),
    ..
  } = expr
  else {
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
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => match expr {
      Expr::Literal {
        value: Value::Callable(callable),
        ..
      } => match &*callable {
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
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => match expr {
      Expr::Literal {
        value: Value::Callable(callable),
        ..
      } => match &*callable {
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
      ..
    } => (*lhs).clone(),
    _ => unreachable!(),
  };
  let expr = match expr {
    Expr::BinOp {
      op: BinOp::Pipeline,
      lhs,
      rhs: _, // take
      pre_resolved_def_ix: _,
      ..
    } => (*lhs).clone(),
    _ => unreachable!(),
  };
  let expr = match expr {
    Expr::BinOp {
      op: BinOp::Pipeline,
      lhs: _, // range
      rhs,
      pre_resolved_def_ix: _,
      ..
    } => (*rhs).clone(),
    _ => unreachable!(),
  };

  let filter_paf = match expr {
    Expr::Literal {
      value: Value::Callable(callable),
      ..
    } => match &*callable {
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
      ..
    } => match expr {
      Expr::Call {
        call: FunctionCall {
          args,
          kwargs: _,
          target,
        },
        ..
      } => {
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
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => match expr {
      Expr::Literal {
        value: Value::Callable(callable),
        ..
      } => match &*callable {
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
      ..
    } => {
      assert!(matches!(lhs.as_literal(), Some(Value::Int(3))));
      assert!(matches!(
        **rhs,
        Expr::Ident { name: id, .. } if id == ctx.interned_symbols.intern("x")
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
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => match expr {
      Expr::Literal {
        value: Value::Callable(callable),
        ..
      } => match &*callable {
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
      ..
    } => {
      assert!(matches!(
        **lhs,
        Expr::Ident { name: id, .. } if id == ctx.interned_symbols.intern("x")
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
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => match expr {
      Expr::Literal {
        value: Value::Callable(callable),
        ..
      } => match &*callable {
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
      ..
    } => {
      assert!(matches!(lhs.as_literal(), Some(Value::Float(f)) if *f == 4.0));
      assert!(matches!(
        **rhs,
        Expr::Ident { name: id, .. } if id == ctx.interned_symbols.intern("x")
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
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => match expr {
      Expr::Literal {
        value: Value::Callable(callable),
        ..
      } => match &*callable {
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
      ..
    } => {
      assert!(matches!(
        lhs.as_literal(),
        Some(Value::Vec2(v)) if v.x == 4. && v.y == 6.
      ));
      assert!(matches!(
        **rhs,
        Expr::Ident { name: id, .. } if id == ctx.interned_symbols.intern("v")
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
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => match expr {
      Expr::Literal {
        value: Value::Callable(callable),
        ..
      } => match &*callable {
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
      ..
    } => {
      assert!(matches!(lhs.as_literal(), Some(Value::Float(f)) if *f == 6.));
      assert!(matches!(
        **rhs,
        Expr::Ident { name: id, .. } if id == ctx.interned_symbols.intern("v")
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
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => {
      assert!(matches!(
        expr,
        Expr::Literal {
          value: Value::Float(_),
          ..
        }
      ));
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
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => match expr {
      Expr::Literal {
        value: Value::Mesh(mesh),
        ..
      } => Rc::clone(mesh),
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  let mut ast2 = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast2).unwrap();

  let mesh2 = match &ast2.statements[0] {
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => match expr {
      Expr::Literal {
        value: Value::Mesh(mesh),
        ..
      } => Rc::clone(mesh),
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
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => match expr {
      Expr::Literal {
        value: Value::Sequence(seq),
        ..
      } => Rc::clone(seq),
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  let mut ast2 = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast2).unwrap();

  let seq2 = match &ast2.statements[0] {
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => match expr {
      Expr::Literal {
        value: Value::Sequence(seq),
        ..
      } => Rc::clone(seq),
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
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => match expr {
      Expr::Literal {
        value: Value::Sequence(seq),
        ..
      } => Rc::clone(seq),
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  let mut ast2 = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast2).unwrap();

  let seq2 = match &ast2.statements[1] {
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => match expr {
      Expr::Literal {
        value: Value::Sequence(seq),
        ..
      } => Rc::clone(seq),
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  assert!(Rc::ptr_eq(&seq1, &seq2));
}

#[test]
fn test_const_eval_cache_persists_across_runs_with_path_block() {
  let code = r#"
distance = 1
path_sampler = build_path(path {
  move(0, 0)
  line(distance, 0)
  line(distance, distance)
})
"#;

  let ctx = EvalCtx::default();
  let mut ast1 = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast1).unwrap();

  let sampler1 = match &ast1.statements[1] {
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => match expr {
      Expr::Literal {
        value: Value::Callable(callable),
        ..
      } => Rc::clone(callable),
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  let mut ast2 = crate::parse_program_src(&ctx, code).unwrap();
  optimize_ast(&ctx, &mut ast2).unwrap();

  let sampler2 = match &ast2.statements[1] {
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => match expr {
      Expr::Literal {
        value: Value::Callable(callable),
        ..
      } => Rc::clone(callable),
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

/// Helper closures defined inside a `path { ... }` block can still call rewritten draw
/// commands (`move`, `line`, etc.) — the rewriting pass recurses into nested closure bodies.
#[test]
fn test_path_block_nested_closure_rewrite() {
  let code = r#"
path_sampler = build_path(path {
  l = |x, y| line(x, y)

  move(0, 0)
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
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => match expr {
      Expr::Literal {
        value: Value::Mesh(mesh),
        ..
      } => Rc::clone(mesh),
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
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => match expr {
      Expr::Literal {
        value: Value::Sequence(seq),
        ..
      } => Rc::clone(seq),
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
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => match expr {
      Expr::Literal {
        value: Value::Sequence(seq),
        ..
      } => Rc::clone(seq),
      _ => unreachable!(),
    },
    _ => unreachable!(),
  };

  let code2 = "a = 0..4 -> |x| x + 2";
  let mut ast2 = crate::parse_program_src(&ctx, code2).unwrap();
  optimize_ast(&ctx, &mut ast2).unwrap();
  let seq2 = match &ast2.statements[0] {
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => match expr {
      Expr::Literal {
        value: Value::Sequence(seq),
        ..
      } => Rc::clone(seq),
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

/// E2E coverage of the ambient-state model: analyzable top-level setter statements execute at
/// fold time (so readers fold correctly *and* stay cached per settings/seed), everything the
/// optimizer can't order-track degrades to runtime evaluation, and threshold setters are
/// rejected outside top-level statement position.
#[cfg(test)]
mod ambient_state_tests {
  use super::*;
  use crate::{parse_and_eval_program, parse_and_eval_program_with_ctx, parse_program_src};

  const EMBED_PLATE: &str = r#"
mesh = embed_path(
  path=[vec2(0, 0), vec2(2, 0), vec2(2, 2), vec2(0, 2)],
  embed=|p| v3(p.x, 0, p.y),
  thickness=0.5,
  split_seams=true
)
"#;

  /// Mimics `geoscript_repl_reset`'s per-run ambient state restoration.
  fn repl_reset(ctx: &EvalCtx) {
    *ctx.sharp_angle_threshold_degrees.borrow_mut() = 45.8366;
    *ctx.default_curve_angle_degrees.borrow_mut() = 1.0;
    ctx.reset_rng_to_default();
  }

  fn plate_vert_count(ctx: &EvalCtx, src: &str) -> usize {
    repl_reset(ctx);
    parse_and_eval_program_with_ctx(src.to_owned(), ctx, false).unwrap();
    let mesh = ctx.get_global("mesh").unwrap();
    mesh.as_mesh().unwrap().mesh.vertices.len()
  }

  fn eval_x(ctx: &EvalCtx, src: &str) -> f32 {
    repl_reset(ctx);
    parse_and_eval_program_with_ctx(src.to_owned(), ctx, false).unwrap();
    ctx.get_global("x").unwrap().as_float().unwrap()
  }

  fn assignment_is_folded(src: &str, name: &str) -> bool {
    let ctx = EvalCtx::default();
    let mut ast = parse_program_src(&ctx, src).unwrap();
    optimize_ast(&ctx, &mut ast).unwrap();
    ast
      .statements
      .iter()
      .find_map(|s| match s {
        TopLevelStatement::Statement(Statement::Assignment { name: n, expr, .. })
          if ctx.with_resolved_sym(*n, |s| s == name) =>
        {
          Some(expr.as_literal().is_some())
        }
        _ => None,
      })
      .unwrap()
  }

  /// A const-arg threshold setter executes at fold time: the downstream reader still const-folds
  /// AND the folded result reflects the runtime setting (179° suppresses the cap/wall creases the
  /// default threshold produces).
  #[test]
  fn threshold_setter_folds_and_is_honored() {
    let ctx = EvalCtx::default();
    let creased = plate_vert_count(&ctx, EMBED_PLATE);
    let smooth = plate_vert_count(
      &ctx,
      &format!("set_sharp_angle_threshold(179)\n{EMBED_PLATE}"),
    );
    assert!(smooth < creased, "{smooth} !< {creased}");
    assert!(
      assignment_is_folded(
        &format!("set_sharp_angle_threshold(179)\n{EMBED_PLATE}"),
        "mesh"
      ),
      "reader after an analyzable setter must still const-fold"
    );
  }

  /// The batch case: one persistent ctx running programs with different thresholds must not serve
  /// cache entries across settings (keys include the live threshold values).
  #[test]
  fn threshold_cache_isolated_across_batch_runs() {
    let ctx = EvalCtx::default();
    let smooth_a = plate_vert_count(
      &ctx,
      &format!("set_sharp_angle_threshold(179)\n{EMBED_PLATE}"),
    );
    let creased = plate_vert_count(&ctx, EMBED_PLATE);
    let smooth_b = plate_vert_count(
      &ctx,
      &format!("set_sharp_angle_threshold(179)\n{EMBED_PLATE}"),
    );
    assert!(smooth_a < creased);
    assert_eq!(smooth_a, smooth_b);
  }

  /// Threshold setters outside top-level statement position are compile errors pointing at the
  /// alternatives; `set_rng_seed` stays legal anywhere.
  #[test]
  fn threshold_setter_rejected_off_top_level() {
    for src in [
      "f = || set_sharp_angle_threshold(60)",
      "x = if 1 < 2 { set_curve_angle_threshold(5) } else { nil }",
      "f = set_sharp_angle_threshold\ng = || f(60)",
    ] {
      let err = parse_and_eval_program(src).unwrap_err();
      let msg = format!("{err}");
      assert!(msg.contains("top-level statement"), "`{src}`: {msg}");
    }
    parse_and_eval_program("f = || set_rng_seed(5)\nf()").unwrap();
  }

  /// A setter whose argument is only known at runtime still takes effect: the reader stops
  /// folding and evaluates after the setter runs.
  #[test]
  fn threshold_setter_dynamic_arg_honored_at_runtime() {
    let ctx = EvalCtx::default();
    let creased = plate_vert_count(&ctx, EMBED_PLATE);
    let smooth = plate_vert_count(
      &ctx,
      &format!("set_sharp_angle_threshold(input_float(\"t\", 179))\n{EMBED_PLATE}"),
    );
    assert!(smooth < creased, "{smooth} !< {creased}");
  }

  /// `set_rng_seed` executes at fold time: a downstream const-arg rng call folds, reproduces
  /// across reruns of a shared ctx, and differs across seeds.
  #[test]
  fn rng_seed_folds_deterministically() {
    let ctx = EvalCtx::default();
    let a1 = eval_x(&ctx, "set_rng_seed(7)\nx = randf(0, 1)");
    let b = eval_x(&ctx, "set_rng_seed(8)\nx = randf(0, 1)");
    let a2 = eval_x(&ctx, "set_rng_seed(7)\nx = randf(0, 1)");
    assert_eq!(a1, a2);
    assert_ne!(a1, b);
    assert!(assignment_is_folded(
      "set_rng_seed(7)\nx = randf(0, 1)",
      "x"
    ));
  }

  /// The seed anchors downstream fold cache keys: rng draws added/removed *before* the seed don't
  /// change what a seeded computation produces (composition 077's memoization contract).
  #[test]
  fn rng_seed_anchors_downstream_cache() {
    let ctx = EvalCtx::default();
    let plain = eval_x(&ctx, "set_rng_seed(7)\nx = randf(0, 1)");
    let shifted = eval_x(&ctx, "junk = randf(0, 1)\nset_rng_seed(7)\nx = randf(0, 1)");
    assert_eq!(plain, shifted);
  }

  /// A seed the optimizer can't order-track (inside control flow / a closure) poisons rng folding
  /// from that point, so downstream draws happen at runtime after the seed has taken effect —
  /// producing the same value as the analyzable top-level form.
  #[test]
  fn rng_seed_legal_anywhere_scoped_correctly() {
    let ctx = EvalCtx::default();
    let baseline = eval_x(&ctx, "set_rng_seed(7)\nx = randf(0, 1)");
    let via_cond = eval_x(&ctx, "if 1 < 2 { set_rng_seed(7) }\nx = randf(0, 1)");
    let via_closure = eval_x(&ctx, "f = || set_rng_seed(7)\nf()\nx = randf(0, 1)");
    assert_eq!(baseline, via_cond);
    assert_eq!(baseline, via_closure);
  }

  /// Settings readers inside closure bodies can't fold at definition position when the program
  /// mutates thresholds — the closure may run after the setter.
  #[test]
  fn closure_interior_reader_defers_when_settings_mutated() {
    let ctx = EvalCtx::default();
    let direct = plate_vert_count(
      &ctx,
      &format!("set_sharp_angle_threshold(179)\n{EMBED_PLATE}"),
    );
    let via_closure = plate_vert_count(
      &ctx,
      r#"
f = || {
  embed_path(
    path=[vec2(0, 0), vec2(2, 0), vec2(2, 2), vec2(0, 2)],
    embed=|p| v3(p.x, 0, p.y),
    thickness=0.5,
    split_seams=true
  )
}
set_sharp_angle_threshold(179)
mesh = f()
"#,
    );
    assert_eq!(direct, via_closure);
  }
}
