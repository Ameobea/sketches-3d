use std::{cell::RefCell, ops::ControlFlow, rc::Rc};

use fxhash::FxHashMap;

use crate::{
  ast::{
    eval_range, get_dyn_type, maybe_pre_resolve_bulitin_call_signature, pre_resolve_binop_def_ix,
    pre_resolve_expr_type, BinOp, ClosureArg, DestructurePattern, DynType, Expr, FunctionCall,
    FunctionCallTarget, MapLiteralEntry, ScopeTracker, Statement, TrackedValue, TrackedValueRef,
  },
  builtins::{
    fn_defs::{fn_sigs, get_builtin_fn_sig_entry_ix},
    resolve_builtin_impl, FUNCTION_ALIASES,
  },
  seq::EagerSeq,
  ArgType, Callable, CapturedScope, Closure, ErrorStack, EvalCtx, Program, Scope, Sym, Value,
};

// Toggle for reassociation over float/vec values.
const FLOAT_ASSOC_FOLDING_ENABLED: bool = true;

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
) -> Result<(), ErrorStack> {
  fold_constants(ctx, local_scope, expr)
}

fn fold_constants<'a>(
  ctx: &EvalCtx,
  local_scope: &'a mut ScopeTracker,
  expr: &mut Expr,
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
        optimize_expr(ctx, local_scope, lhs)?;
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
        optimize_expr(ctx, local_scope, lhs)?;
      }
      optimize_expr(ctx, local_scope, rhs)?;

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

      if matches!(op, BinOp::Pipeline) {
        if let Value::Callable(callable) = &rhs_val {
          if callable.is_side_effectful() {
            return Ok(());
          }
        }
      }

      let val = op.apply(ctx, lhs_val, rhs_val, *pre_resolved_def_ix)?;
      *expr = val.into_literal_expr();
      Ok(())
    }
    Expr::PrefixOp { op, expr: inner } => {
      optimize_expr(ctx, local_scope, inner)?;

      let Some(val) = inner.as_literal() else {
        return Ok(());
      };
      let val = op.apply(ctx, val)?;
      *expr = val.into_literal_expr();
      Ok(())
    }
    Expr::Range {
      start,
      end,
      inclusive,
    } => {
      optimize_expr(ctx, local_scope, start)?;
      if let Some(end) = end {
        optimize_expr(ctx, local_scope, end)?;
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
      let val = eval_range(start_val, end_val_opt, *inclusive)?;
      *expr = val.into_literal_expr();
      Ok(())
    }
    Expr::StaticFieldAccess { lhs, field } => {
      optimize_expr(ctx, local_scope, lhs)?;

      let Some(lhs_val) = lhs.as_literal() else {
        return Ok(());
      };

      let val = ctx.eval_static_field_access(lhs_val, field)?;
      *expr = val.into_literal_expr();

      Ok(())
    }
    Expr::FieldAccess { lhs, field } => {
      optimize_expr(ctx, local_scope, lhs)?;
      optimize_expr(ctx, local_scope, field)?;

      let (Some(lhs_val), Some(field_val)) = (lhs.as_literal(), field.as_literal()) else {
        return Ok(());
      };

      let val = ctx.eval_field_access(lhs_val, field_val)?;
      *expr = val.into_literal_expr();

      Ok(())
    }
    Expr::Call(FunctionCall {
      target,
      args,
      kwargs,
    }) => {
      for arg in args.iter_mut() {
        optimize_expr(ctx, local_scope, arg)?;
      }
      for (_, expr) in kwargs.iter_mut() {
        optimize_expr(ctx, local_scope, expr)?;
      }

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
          let pre_resolved_signature =
            maybe_pre_resolve_bulitin_call_signature(ctx, local_scope, fn_entry_ix, args, kwargs)?;
          *target = FunctionCallTarget::Literal(Rc::new(Callable::Builtin {
            fn_entry_ix,
            fn_impl,
            pre_resolved_signature,
          }));
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

      match target {
        FunctionCallTarget::Name(name) => {
          if let Some(val) = local_scope.get(*name) {
            match val {
              TrackedValueRef::Const(val) => match val {
                Value::Callable(callable) => {
                  if !callable.is_side_effectful() {
                    let evaled = ctx.invoke_callable(callable, &arg_vals, &kwarg_vals)?;
                    *expr = evaled.into_literal_expr();
                  }
                }
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
              TrackedValueRef::Arg(_) => (),
              TrackedValueRef::Dyn { .. } => (),
            }
            Ok(())
          } else {
            unreachable!(
              "If this was a builtin, it would have been resolved earlier.  If it was undefined, \
               the error would have been raised earlier."
            );
          }
        }
        FunctionCallTarget::Literal(callable) => {
          if !callable.is_side_effectful() {
            let evaled = ctx.invoke_callable(callable, &arg_vals, &kwarg_vals)?;
            *expr = evaled.into_literal_expr();
          }
          Ok(())
        }
      }
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
          optimize_expr(ctx, local_scope, default_val)?;
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
        optimize_statement(ctx, &mut closure_scope, stmt)?;
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
      let body_captures_dyn = body_inner.inline_const_captures(ctx, &mut local_scope_with_args);
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
        optimize_expr(ctx, local_scope, inner)?;
      }

      // if all elements are literals, can fold into an `EagerSeq`
      if exprs.iter().all(|e| e.is_literal()) {
        let values = exprs
          .iter()
          .map(|e| e.as_literal().unwrap().clone())
          .collect::<Vec<_>>();
        *expr = Expr::Literal(Value::Sequence(Rc::new(EagerSeq { inner: values })));
      }

      Ok(())
    }
    Expr::MapLiteral { entries } => {
      for value in entries.iter_mut() {
        match value {
          MapLiteralEntry::KeyValue { key: _, value } => {
            optimize_expr(ctx, local_scope, value)?;
          }
          MapLiteralEntry::Splat { expr } => {
            optimize_expr(ctx, local_scope, expr)?;
          }
        }
      }

      // if all values are literals, can fold into a `Map`
      if entries.iter().all(|e| e.is_literal()) {
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

        *expr = Expr::Literal(Value::Map(Rc::new(map)));
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

      /// If there's an assignment performend to a variable in the parent scope from within one of
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

      optimize_expr(ctx, local_scope, cond)?;
      let mut then_scope = ScopeTracker::wrap(local_scope);
      optimize_expr(ctx, &mut then_scope, then)?;
      let ScopeTracker {
        vars: then_scope_var_names,
        ..
      } = &then_scope;
      deconstify_parent_scope(local_scope, then_scope_var_names.keys().copied());
      for (cond, inner) in else_if_exprs {
        optimize_expr(ctx, local_scope, cond)?;
        let mut else_if_scope = ScopeTracker::wrap(local_scope);
        optimize_expr(ctx, &mut else_if_scope, inner)?;
        let ScopeTracker {
          vars: else_if_scope_var_names,
          ..
        } = &else_if_scope;
        deconstify_parent_scope(local_scope, else_if_scope_var_names.keys().copied());
      }
      if let Some(else_expr) = else_expr {
        let mut else_scope = ScopeTracker::wrap(local_scope);
        optimize_expr(ctx, &mut else_scope, else_expr)?;
        let ScopeTracker {
          vars: else_scope_var_names,
          ..
        } = &else_scope;
        deconstify_parent_scope(local_scope, else_scope_var_names.keys().copied());
      }
      Ok(())
    }
    Expr::Block { statements } => {
      // the `inline_const_captures` checks were built for closure bodies, so they think everything
      // is OK is a local at the most inner scope level is declared but not const-available - those
      // correspond to closure args.

      // For the case of a block inside of a closure, we can get around this by adding one level of
      // fake nesting to the scope

      let mut block_scope = ScopeTracker::wrap(&*local_scope);

      for stmt in statements.iter_mut() {
        optimize_statement(ctx, &mut block_scope, stmt)?;
      }

      // can const-fold the block if all inner statements are const
      let mut captures_dyn = false;
      for stmt in statements {
        captures_dyn |= stmt.inline_const_captures(ctx, &mut block_scope);
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
) -> Result<(), ErrorStack> {
  match stmt {
    Statement::Expr(expr) => optimize_expr(ctx, local_scope, expr),
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

      optimize_expr(ctx, local_scope, expr)?;

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

      optimize_expr(ctx, local_scope, rhs)?;

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
        optimize_expr(ctx, local_scope, expr)?
      }
      Ok(())
    }
    Statement::Break { value } => {
      if let Some(expr) = value {
        optimize_expr(ctx, local_scope, expr)?
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
    optimize_statement(ctx, &mut local_scope, stmt)?;
  }
  Ok(())
}

pub fn optimize_ast(ctx: &EvalCtx, ast: &mut Program) -> Result<(), ErrorStack> {
  default_optimizer_pipeline().run(ctx, ast)
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
