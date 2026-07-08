//! Shared forward type inference for the geoscript optimizer and the geoscript_analysis IDE
//! walker.  Produces an [`AbstractType`] for any expression in a given [`TypeEnv`].
//!
//! This module is pure — no diagnostics, no symbol-reference recording.  Callers layer their
//! own concerns (ref tracking, diagnostics, function-call recording) on top by consulting the
//! [`resolve_builtin_call`] / [`resolve_paf_call`] helpers alongside their own walk.

use fxhash::FxHashMap;

use crate::{
  ast::{
    infer_dynamic_field_access_ty, infer_static_field_access_ty, BinOp, ClosureArg, ClosureBody,
    Expr, FunctionCall, FunctionCallTarget, MapLiteralEntry, Statement,
  },
  builtins::{
    fn_defs::{fn_sigs, get_builtin_fn_sig_entry_ix, DefaultValue, FnSignature},
    FUNCTION_ALIASES,
  },
  match_binop_by_arg_types, match_signature_by_arg_types, match_unop_by_arg_types,
  ty::{merge_types, AbstractType, CallableParam, CallableType, PartialApplication},
  ArgType, EvalCtx, Sym,
};

/// Scope stack for type inference.  Mirrors the shape of [`crate::ast::ScopeTracker`] but
/// carries only type information.  Also tracks the return-type accumulation of any enclosing
/// closures so `Statement::Return` can contribute at arbitrary depth.
#[derive(Default, Debug)]
pub struct TypeEnv {
  frames: Vec<FxHashMap<Sym, AbstractType>>,
  closure_return_stack: Vec<ClosureReturnTracker>,
}

#[derive(Debug)]
struct ClosureReturnTracker {
  exit_types: Vec<AbstractType>,
}

impl TypeEnv {
  pub fn new() -> Self {
    TypeEnv {
      frames: vec![FxHashMap::default()],
      closure_return_stack: Vec::new(),
    }
  }

  /// Build a TypeEnv pre-seeded with the language's default globals (`pi`, `tau`, etc.).
  /// Derived from the same source of truth as [`crate::Scope::default_globals`] so the two
  /// cannot drift apart.
  pub fn with_default_globals(ctx: &EvalCtx) -> Self {
    let mut env = Self::new();
    for (name, val) in crate::get_default_globals() {
      let sym = ctx.interned_symbols.intern(name);
      env.define(sym, AbstractType::Concrete(val.get_type()));
    }
    env
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

  pub fn lookup(&self, name: Sym) -> Option<&AbstractType> {
    for frame in self.frames.iter().rev() {
      if let Some(ty) = frame.get(&name) {
        return Some(ty);
      }
    }
    None
  }

  pub fn contains(&self, name: Sym) -> bool {
    self.frames.iter().any(|f| f.contains_key(&name))
  }
}

/// Result of trying to resolve a builtin or PAF call against a signature list.
///
/// Exposes enough structure for callers (especially the analysis walker) to emit diagnostics
/// on `NoMatch` or record a matched signature index for per-call hover.
pub enum CallResolution {
  /// Call fully matches a specific signature.
  Matched {
    return_ty: AbstractType,
    def_ix: usize,
  },
  /// All arg types concrete, but no signature matches yet — could be completed with more args.
  /// Callers may wrap this back into an [`AbstractType::PartiallyApplied`].
  PartiallyApplied {
    canonical_name: String,
    bound_args: Vec<ArgType>,
    bound_kwargs: Vec<(Sym, ArgType)>,
  },
  /// All arg types concrete, no signature matches, and not a valid prefix.  A genuine error.
  NoMatch {
    canonical_name: String,
    concrete_args: Vec<ArgType>,
    concrete_kwargs: Vec<(Sym, ArgType)>,
  },
  /// Name is not a known builtin (after alias resolution).
  NotBuiltin,
  /// At least one arg type is non-concrete (Union / PAF / Callable / Unknown) — can't decide.
  Indeterminate,
}

impl CallResolution {
  /// Collapse a resolution into an abstract type as seen by a caller that doesn't care about
  /// the detailed matching info.  `Matched` → its return type, `PartiallyApplied` → a
  /// `PartiallyApplied` type, everything else → Unknown.
  pub fn into_abstract_type(self) -> AbstractType {
    match self {
      CallResolution::Matched { return_ty, .. } => return_ty,
      CallResolution::PartiallyApplied {
        canonical_name,
        bound_args,
        bound_kwargs,
      } => AbstractType::PartiallyApplied(PartialApplication {
        name: canonical_name,
        bound_args,
        bound_kwargs,
      }),
      _ => AbstractType::Unknown,
    }
  }
}

/// Attempt to resolve a builtin function call from its name Sym and fully-typed args.
pub fn resolve_builtin_call(
  ctx: &EvalCtx,
  name: Sym,
  arg_types: &[AbstractType],
  kwarg_types: &[(Sym, AbstractType)],
) -> CallResolution {
  let Some(name_str) = ctx.interned_symbols.with_resolved(name, |s| s.to_string()) else {
    return CallResolution::NotBuiltin;
  };
  let (canonical_name, sigs) = if let Some(def) = fn_sigs().get(&name_str) {
    (name_str.clone(), def.signatures)
  } else if let Some(&real_name) = FUNCTION_ALIASES.get(name_str.as_str()) {
    if let Some(def) = fn_sigs().get(real_name) {
      (real_name.to_string(), def.signatures)
    } else {
      return CallResolution::NotBuiltin;
    }
  } else {
    return CallResolution::NotBuiltin;
  };

  resolve_against_sigs(canonical_name, sigs, arg_types, kwarg_types)
}

/// Attempt to resolve a call into an existing partial application: combine bound args with
/// new ones and re-match.
pub fn resolve_paf_call(
  paf: &PartialApplication,
  new_pos: &[AbstractType],
  new_kwargs: &[(Sym, AbstractType)],
) -> CallResolution {
  let mut new_pos_concrete: Vec<ArgType> = Vec::with_capacity(new_pos.len());
  for ty in new_pos {
    match ty.as_single_arg_type() {
      Some(t) => new_pos_concrete.push(t),
      None => return CallResolution::Indeterminate,
    }
  }
  let mut new_kw_concrete: Vec<(Sym, ArgType)> = Vec::with_capacity(new_kwargs.len());
  for (sym, ty) in new_kwargs {
    match ty.as_single_arg_type() {
      Some(t) => new_kw_concrete.push((*sym, t)),
      None => return CallResolution::Indeterminate,
    }
  }

  let Some(def) = fn_sigs().get(paf.name.as_str()) else {
    return CallResolution::NotBuiltin;
  };
  let sigs = def.signatures;

  let mut combined_pos = paf.bound_args.clone();
  combined_pos.extend(new_pos_concrete);
  let mut combined_kw = paf.bound_kwargs.clone();
  combined_kw.extend(new_kw_concrete);

  match match_signature_by_arg_types(sigs, &combined_pos, &combined_kw) {
    Some(sig_match) => CallResolution::Matched {
      return_ty: AbstractType::from_return_type(sig_match.return_type),
      def_ix: sig_match.def_ix,
    },
    None => {
      if is_valid_partial_prefix(sigs, &combined_pos, &combined_kw) {
        CallResolution::PartiallyApplied {
          canonical_name: paf.name.clone(),
          bound_args: combined_pos,
          bound_kwargs: combined_kw,
        }
      } else {
        CallResolution::NoMatch {
          canonical_name: paf.name.clone(),
          concrete_args: combined_pos,
          concrete_kwargs: combined_kw,
        }
      }
    }
  }
}

fn resolve_against_sigs(
  canonical_name: String,
  sigs: &'static [FnSignature],
  arg_types: &[AbstractType],
  kwarg_types: &[(Sym, AbstractType)],
) -> CallResolution {
  let mut concrete_args: Vec<ArgType> = Vec::with_capacity(arg_types.len());
  for ty in arg_types {
    match ty.as_single_arg_type() {
      Some(t) => concrete_args.push(t),
      None => return CallResolution::Indeterminate,
    }
  }
  let mut concrete_kwargs: Vec<(Sym, ArgType)> = Vec::with_capacity(kwarg_types.len());
  for (sym, ty) in kwarg_types {
    match ty.as_single_arg_type() {
      Some(t) => concrete_kwargs.push((*sym, t)),
      None => return CallResolution::Indeterminate,
    }
  }

  match match_signature_by_arg_types(sigs, &concrete_args, &concrete_kwargs) {
    Some(sig_match) => CallResolution::Matched {
      return_ty: AbstractType::from_return_type(sig_match.return_type),
      def_ix: sig_match.def_ix,
    },
    None => {
      if is_valid_partial_prefix(sigs, &concrete_args, &concrete_kwargs) {
        CallResolution::PartiallyApplied {
          canonical_name,
          bound_args: concrete_args,
          bound_kwargs: concrete_kwargs,
        }
      } else {
        CallResolution::NoMatch {
          canonical_name,
          concrete_args,
          concrete_kwargs,
        }
      }
    }
  }
}

/// True if the provided positional + kwarg types match some signature's *prefix* — i.e. the
/// call could become complete by providing more args (partial application).
pub fn is_valid_partial_prefix(
  sigs: &'static [FnSignature],
  positional: &[ArgType],
  kwargs: &[(Sym, ArgType)],
) -> bool {
  // Dynamic signatures (empty first arg name) accept anything; treat as always-valid prefix.
  if let Some(sig) = sigs.first() {
    if let Some(d) = sig.arg_defs.first() {
      if d.name.is_empty() {
        return true;
      }
    }
  }

  for sig in sigs {
    if positional.len() > sig.arg_defs.len() {
      continue;
    }
    let mut ok = true;
    for (i, ty) in positional.iter().enumerate() {
      if sig.arg_defs[i].valid_types & ty.as_bitflags() == 0 {
        ok = false;
        break;
      }
    }
    if !ok {
      continue;
    }
    for (k, kty) in kwargs {
      let arg_def = sig.arg_defs.iter().find(|d| d.interned_name == *k);
      match arg_def {
        Some(d) if d.valid_types & kty.as_bitflags() != 0 => {}
        _ => {
          ok = false;
          break;
        }
      }
    }
    if ok {
      return true;
    }
  }
  false
}

/// How a standalone builtin call `f(args)` resolves, used to model the pipeline operator `lhs |
/// rhs`.
///
/// The runtime evaluates `rhs` to a value *before* deciding how `|` behaves: when `f(args)` is a
/// partial application the value is a callable and the pipeline invokes it with `lhs`; when
/// `f(args)` is complete the value is concrete and `|` degrades to `bit_or(lhs, value)`.  Non-
/// concrete arg types act as wildcards so e.g. `v3(x, y, 10)` with unknown `x`/`y` still classifies
/// as a complete `vec3`.
pub enum PipeRhsKind {
  /// `f(args)` fully binds a signature → a concrete value of this type (and the matched def
  /// index).
  Complete { ty: AbstractType, def_ix: usize },
  /// No signature fully binds but the args are a valid prefix → a callable to pipe into.
  Partial,
  /// Concrete args fit no signature, neither complete nor as a prefix → the call itself is
  /// invalid.
  NoMatch,
  /// Not a resolvable builtin, or it has dynamic signatures we can't reason about.
  Indeterminate,
}

/// True if an abstract arg type is compatible with a parameter's `valid_types` bitflags.  A non-
/// concrete type (Union / partial application / Unknown) fits any param when `require_concrete` is
/// false (wildcard), but fits none when true (we can't prove a match).
fn arg_ty_fits(valid_types: u16, ty: &AbstractType, require_concrete: bool) -> bool {
  match ty.as_single_arg_type() {
    Some(c) => valid_types & c.as_bitflags() != 0,
    None => !require_concrete,
  }
}

/// Whether `sig` is fully satisfied by the given args (all required params bound, types
/// compatible) — i.e. the call would produce a concrete value rather than a partial application.
/// With `require_concrete`, a wildcard arg is *not* allowed to satisfy a param, so this reports
/// only binds the runtime would definitely make regardless of what the wildcards resolve to.
fn sig_fully_binds(
  sig: &FnSignature,
  positional: &[AbstractType],
  kwargs: &[(Sym, AbstractType)],
  require_concrete: bool,
) -> bool {
  for (kw, _) in kwargs {
    if !sig.arg_defs.iter().any(|d| d.interned_name == *kw) {
      return false;
    }
  }
  let mut pos_ix = 0;
  for arg_def in sig.arg_defs {
    if let Some((_, ty)) = kwargs.iter().find(|(s, _)| *s == arg_def.interned_name) {
      if !arg_ty_fits(arg_def.valid_types, ty, require_concrete) {
        return false;
      }
    } else if pos_ix < positional.len() {
      if !arg_ty_fits(arg_def.valid_types, &positional[pos_ix], require_concrete) {
        return false;
      }
      pos_ix += 1;
    } else if matches!(arg_def.default_value, DefaultValue::Required) {
      return false;
    }
  }
  true
}

/// Whether the given args are a valid *prefix* of `sig` (could be completed by supplying more) —
/// i.e. the call would yield a partial application.
fn sig_valid_prefix(
  sig: &FnSignature,
  positional: &[AbstractType],
  kwargs: &[(Sym, AbstractType)],
) -> bool {
  if positional.len() > sig.arg_defs.len() {
    return false;
  }
  for (i, ty) in positional.iter().enumerate() {
    if !arg_ty_fits(sig.arg_defs[i].valid_types, ty, false) {
      return false;
    }
  }
  for (kw, kty) in kwargs {
    match sig.arg_defs.iter().find(|d| d.interned_name == *kw) {
      Some(d) if arg_ty_fits(d.valid_types, kty, false) => {}
      _ => return false,
    }
  }
  true
}

/// Classify the rhs of a pipeline when it's a written-out call `f(args)`, mirroring the runtime's
/// complete-vs-partial decision (see [`PipeRhsKind`]).  The runtime prefers a complete signature
/// match, falling back to a partial application only when no signature fully binds.
pub fn classify_pipe_rhs_call(
  ctx: &EvalCtx,
  name: Sym,
  arg_types: &[AbstractType],
  kwarg_types: &[(Sym, AbstractType)],
) -> PipeRhsKind {
  let Some(name_str) = ctx.interned_symbols.with_resolved(name, |s| s.to_string()) else {
    return PipeRhsKind::Indeterminate;
  };
  let sigs = if let Some(def) = fn_sigs().get(name_str.as_str()) {
    def.signatures
  } else if let Some(&real) = FUNCTION_ALIASES.get(name_str.as_str()) {
    match fn_sigs().get(real) {
      Some(def) => def.signatures,
      None => return PipeRhsKind::Indeterminate,
    }
  } else {
    return PipeRhsKind::Indeterminate;
  };

  // Dynamic signatures (empty first arg name) accept anything — can't reason about completeness.
  if sigs
    .first()
    .and_then(|s| s.arg_defs.first())
    .map_or(true, |d| d.name.is_empty())
  {
    return PipeRhsKind::Indeterminate;
  }

  // A signature bound entirely by concrete args is authoritative: the runtime produces a value
  // here regardless of any longer overload, so `|` degrades to `bit_or`.
  for (def_ix, sig) in sigs.iter().enumerate() {
    if sig_fully_binds(sig, arg_types, kwarg_types, true) {
      return PipeRhsKind::Complete {
        ty: AbstractType::from_return_type(sig.return_type),
        def_ix,
      };
    }
  }
  // The pipe appends `lhs` as one more positional arg.  When some overload accepts the current
  // args as a prefix with a still-unbound param (room for that piped arg), the rhs is a partial
  // application to pipe into — even if a shorter overload would *wildcard*-bind.  Mirrors the
  // runtime, which with concrete arg types leaves the longer overload partially applied.
  if !arg_types.is_empty() || !kwarg_types.is_empty() {
    for sig in sigs {
      if sig_valid_prefix(sig, arg_types, kwarg_types)
        && !sig_fully_binds(sig, arg_types, kwarg_types, false)
      {
        return PipeRhsKind::Partial;
      }
    }
  }
  // No pipe-fillable overload — fall back to a wildcard-permissive full bind.
  for (def_ix, sig) in sigs.iter().enumerate() {
    if sig_fully_binds(sig, arg_types, kwarg_types, false) {
      return PipeRhsKind::Complete {
        ty: AbstractType::from_return_type(sig.return_type),
        def_ix,
      };
    }
  }
  PipeRhsKind::NoMatch
}

/// Infer the abstract type of an expression in the given environment.  Mutates `env`
/// transiently for blocks / closures but restores it before returning.
pub fn infer_expr(ctx: &EvalCtx, env: &mut TypeEnv, expr: &Expr) -> AbstractType {
  match expr {
    Expr::Literal { value, .. } => AbstractType::Concrete(value.get_type()),

    Expr::Ident { name, .. } => env.lookup(*name).cloned().unwrap_or(AbstractType::Unknown),

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

    Expr::FieldAccess { lhs, field, .. } => {
      let lhs_ty = infer_expr(ctx, env, lhs);
      let field_ty = infer_expr(ctx, env, field);
      let Some(lhs_c) = lhs_ty.as_single_arg_type() else {
        return AbstractType::Unknown;
      };
      let Some(field_c) = field_ty.as_single_arg_type() else {
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
      ..
    } => infer_closure(ctx, env, params, body, *return_type_hint),

    Expr::ArrayLiteral { elements, .. } => {
      for el in elements {
        infer_expr(ctx, env, el);
      }
      AbstractType::Concrete(ArgType::Sequence)
    }

    Expr::MapLiteral { entries, .. } => {
      for entry in entries {
        match entry {
          MapLiteralEntry::KeyValue { value, .. } => {
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
      let stmt_count = statements.len();
      let mut result = AbstractType::Concrete(ArgType::Nil);
      for (i, stmt) in statements.iter().enumerate() {
        let is_last = i + 1 == stmt_count;
        if is_last {
          if let Statement::Expr(expr) = stmt {
            result = infer_expr(ctx, env, expr);
            continue;
          }
        }
        infer_statement(ctx, env, stmt);
      }
      env.pop_scope();
      result
    }
  }
}

/// Infer a statement's effect on the environment.  Side-effects on `env` (bindings from
/// assignments, closure-return tracking) persist; the statement itself has no return value.
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
    Statement::DestructureAssignment { lhs, rhs } => {
      infer_expr(ctx, env, rhs);
      lhs.visit_idents(&mut |sym| env.define(sym, AbstractType::Unknown));
    }
    Statement::Expr(expr) => {
      infer_expr(ctx, env, expr);
    }
    Statement::Return { value } => {
      let exit_ty = match value {
        Some(expr) => infer_expr(ctx, env, expr),
        None => AbstractType::Concrete(ArgType::Nil),
      };
      if let Some(tracker) = env.closure_return_stack.last_mut() {
        tracker.exit_types.push(exit_ty);
      }
    }
    Statement::Break { value } => {
      if let Some(expr) = value {
        infer_expr(ctx, env, expr);
      }
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

/// Result type of `lhs | rhs` when `rhs` is not callable: `|` falls through from pipeline to
/// `bit_or` — a boolean union for meshes, a bitwise-or for ints.
pub fn infer_bitor_op_result_type(lhs_ty: &AbstractType, rhs_ty: &AbstractType) -> AbstractType {
  let (Some(lhs_c), Some(rhs_c)) = (lhs_ty.as_single_arg_type(), rhs_ty.as_single_arg_type())
  else {
    return AbstractType::Unknown;
  };
  let Some(entry_ix) = get_builtin_fn_sig_entry_ix("bit_or") else {
    return AbstractType::Unknown;
  };
  match match_binop_by_arg_types(entry_ix, lhs_c, rhs_c) {
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
    AbstractType::PartiallyApplied(paf) => builtin_fn_return_type(&paf.name),
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
    .with_resolved(*name, |s| s.to_string())?;
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
    } if !is_local(*reducer_name) => ctx
      .interned_symbols
      .with_resolved(*reducer_name, |s| s.to_string()),
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

  match rhs {
    Expr::Call { call, .. } => {
      let arg_types: Vec<AbstractType> =
        call.args.iter().map(|a| infer_expr(ctx, env, a)).collect();
      let kwarg_types: Vec<(Sym, AbstractType)> = call
        .kwargs
        .iter()
        .map(|(&sym, e)| (sym, infer_expr(ctx, env, e)))
        .collect();

      if let FunctionCallTarget::Name(name) = &call.target {
        // Pipeline semantics: `lhs | f(a, b)` → `f(a, b, lhs)`.
        let mut piped: Vec<AbstractType> = Vec::with_capacity(arg_types.len() + 1);
        piped.extend(arg_types);
        piped.push(lhs_ty);

        if env.contains(*name) {
          match env.lookup(*name).cloned() {
            Some(AbstractType::PartiallyApplied(paf)) => {
              resolve_paf_call(&paf, &piped, &kwarg_types).into_abstract_type()
            }
            Some(AbstractType::Callable(ct)) => (*ct.return_type).clone(),
            _ => AbstractType::Unknown,
          }
        } else {
          let resolved =
            resolve_builtin_call(ctx, *name, &piped, &kwarg_types).into_abstract_type();
          infer_reduce_fold_result(ctx, call, &piped, piped.len(), |s| env.contains(s))
            .unwrap_or(resolved)
        }
      } else {
        AbstractType::Unknown
      }
    }
    Expr::Ident { name, .. } => {
      if env.contains(*name) {
        match env.lookup(*name).cloned() {
          Some(AbstractType::PartiallyApplied(paf)) => {
            resolve_paf_call(&paf, &[lhs_ty], &[]).into_abstract_type()
          }
          Some(AbstractType::Callable(ct)) => (*ct.return_type).clone(),
          Some(other) => infer_bitor_op_result_type(&lhs_ty, &other),
          None => AbstractType::Unknown,
        }
      } else {
        resolve_builtin_call(ctx, *name, &[lhs_ty], &[]).into_abstract_type()
      }
    }
    Expr::Literal {
      value: crate::Value::Callable(callable),
      ..
    } => callable
      .get_return_type_hint()
      .map(AbstractType::Concrete)
      .unwrap_or(AbstractType::Unknown),
    _ => {
      let rhs_ty = infer_expr(ctx, env, rhs);
      infer_bitor_op_result_type(&lhs_ty, &rhs_ty)
    }
  }
}

fn infer_call_expr(ctx: &EvalCtx, env: &mut TypeEnv, call: &FunctionCall) -> AbstractType {
  let arg_types: Vec<AbstractType> = call.args.iter().map(|a| infer_expr(ctx, env, a)).collect();
  let kwarg_types: Vec<(Sym, AbstractType)> = call
    .kwargs
    .iter()
    .map(|(&sym, e)| (sym, infer_expr(ctx, env, e)))
    .collect();

  match &call.target {
    FunctionCallTarget::Name(name) => {
      if env.contains(*name) {
        match env.lookup(*name).cloned() {
          Some(AbstractType::PartiallyApplied(paf)) => {
            resolve_paf_call(&paf, &arg_types, &kwarg_types).into_abstract_type()
          }
          Some(AbstractType::Callable(ct)) => (*ct.return_type).clone(),
          _ => AbstractType::Unknown,
        }
      } else {
        let resolved =
          resolve_builtin_call(ctx, *name, &arg_types, &kwarg_types).into_abstract_type();
        infer_reduce_fold_result(ctx, call, &arg_types, call.args.len(), |s| env.contains(s))
          .unwrap_or(resolved)
      }
    }
    FunctionCallTarget::Literal(callable) => callable
      .get_return_type_hint()
      .map(AbstractType::Concrete)
      .unwrap_or(AbstractType::Unknown),
  }
}

fn infer_closure(
  ctx: &EvalCtx,
  env: &mut TypeEnv,
  params: &[ClosureArg],
  body: &ClosureBody,
  return_type_hint: Option<ArgType>,
) -> AbstractType {
  env.push_scope();
  let mut callable_params: Vec<CallableParam> = Vec::with_capacity(params.len());
  for param in params {
    let ty = match &param.type_hint {
      Some(hint) => AbstractType::Concrete(*hint),
      None => AbstractType::Unknown,
    };
    let name = first_ident_name(&param.ident, ctx);
    callable_params.push(CallableParam {
      name,
      ty: ty.clone(),
    });
    param
      .ident
      .visit_idents(&mut |sym| env.define(sym, ty.clone()));
    if let Some(default) = &param.default_val {
      infer_expr(ctx, env, default);
    }
  }

  env.closure_return_stack.push(ClosureReturnTracker {
    exit_types: Vec::new(),
  });

  // Walk body statements.  If the last statement is a trailing expression, its type becomes
  // the implicit return; if it's a `Return`, the implicit tail is unreachable.
  let mut implicit_return = AbstractType::Concrete(ArgType::Nil);
  let mut implicit_is_unreachable = false;
  let stmt_count = body.0.len();
  for (i, stmt) in body.0.iter().enumerate() {
    let is_last = i + 1 == stmt_count;
    if is_last {
      match stmt {
        Statement::Expr(expr) => {
          implicit_return = infer_expr(ctx, env, expr);
          continue;
        }
        Statement::Return { .. } => {
          implicit_is_unreachable = true;
        }
        _ => {}
      }
    }
    infer_statement(ctx, env, stmt);
  }

  let tracker = env
    .closure_return_stack
    .pop()
    .expect("closure return stack balanced");
  env.pop_scope();

  let return_ty = if let Some(declared) = return_type_hint {
    AbstractType::Concrete(declared)
  } else {
    let mut acc = if implicit_is_unreachable {
      AbstractType::Unknown
    } else {
      implicit_return
    };
    for t in &tracker.exit_types {
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

fn first_ident_name(pat: &crate::ast::DestructurePattern, ctx: &EvalCtx) -> Option<String> {
  if let crate::ast::DestructurePattern::Ident(sym) = pat {
    ctx.interned_symbols.with_resolved(*sym, |s| s.to_string())
  } else {
    None
  }
}
