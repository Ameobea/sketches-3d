//! Forward-mode symbolic automatic differentiation for Geoscript.
//!
//! `deriv(f, dir)` transforms a single-parameter closure `f` into a new closure computing the
//! directional derivative of `f` seeded by the tangent `dir` (a JVP).  Differentiation is done by
//! source transformation (AST→AST): the body is rewritten into an A-normal-form tape where each
//! compound node's primal is bound to a fresh temp and its tangent (when non-zero) to another, so
//! the emitted body grows linearly with the source regardless of nesting.  A symbolic-zero-aware
//! [`Tangent`] keeps constant/parameter-independent subterms from emitting dead `0*x` work.

use std::rc::Rc;

use fxhash::FxHashMap;
use mesh::linked_mesh::Vec3;

use crate::{
  ast::{
    BinOp, ClosureBody, DestructurePattern, Expr, FunctionCall, FunctionCallTarget, PrefixOp,
    SourceLoc, Statement, VarRes,
  },
  builtins::{
    fn_defs::{fn_sigs, get_builtin_fn_sig_entry_ix},
    FUNCTION_ALIASES,
  },
  optimizer::optimize_synthesized_closure_body,
  ty::AbstractType,
  type_infer::{infer_expr, TypeEnv},
  ArgType, Callable, Closure, ErrorStack, EvalCtx, Scope, Sym, Value, Vec2,
};

/// A symbolic tangent.  `Zero` reifies to a correctly-typed zero literal only when forced, so
/// parameter-independent subterms never emit `0*x`.
#[derive(Clone, Debug)]
pub enum Tangent {
  Zero,
  Expr(Expr),
}

/// A derivative rule for a builtin: given the primal atoms of the call's arguments, the taped
/// primal *result* (reusable, e.g. `sqrt(a)` for `da/(2·sqrt(a))`), and the arguments' tangents,
/// produces the tangent of the call.  Emitted arithmetic carries `pre_resolved_def_ix: None` so the
/// interpreter re-resolves the overload from operand types.
type DerivRule =
  fn(&mut DerivCtx, &[Expr], &Expr, &[Tangent], SourceLoc) -> Result<Tangent, ErrorStack>;

struct DerivCtx<'a> {
  ctx: &'a EvalCtx,
  gensym: usize,
  /// Maps an original binding name (param / `let`) to its primal atom and tangent.
  env: FxHashMap<Sym, (Expr, Tangent)>,
  /// Emitted ANF statements (the tape).
  tape: Vec<Statement>,
  /// Types of params and taped primal temps, used to synthesize zero tangents.
  type_env: TypeEnv,
  /// Scope used to resolve free identifiers (captured constants) during differentiation.
  captures: Rc<Scope>,
}

fn is_atom(e: &Expr) -> bool {
  matches!(e, Expr::Ident { .. } | Expr::Literal { .. })
}

fn lit(v: Value, loc: SourceLoc) -> Expr {
  Expr::Literal { value: v, loc }
}

fn fl(x: f32, loc: SourceLoc) -> Expr {
  lit(Value::Float(x), loc)
}

fn binop(op: BinOp, lhs: Expr, rhs: Expr, loc: SourceLoc) -> Expr {
  Expr::BinOp {
    op,
    lhs: Box::new(lhs),
    rhs: Box::new(rhs),
    pre_resolved_def_ix: None,
    loc,
  }
}

fn neg(e: Expr, loc: SourceLoc) -> Expr {
  Expr::PrefixOp {
    op: PrefixOp::Neg,
    expr: Box::new(e),
    loc,
  }
}

fn builtin_call(ctx: &EvalCtx, name: &str, args: Vec<Expr>, loc: SourceLoc) -> Expr {
  let name_sym = ctx.interned_symbols.intern(name);
  Expr::Call {
    call: FunctionCall {
      target_res: VarRes::Unresolved,
      target: FunctionCallTarget::Name(name_sym),
      args,
      kwargs: FxHashMap::default(),
    },
    loc,
  }
}

fn t_neg(t: Tangent, loc: SourceLoc) -> Tangent {
  match t {
    Tangent::Zero => Tangent::Zero,
    Tangent::Expr(e) => Tangent::Expr(neg(e, loc)),
  }
}

fn t_add(a: Tangent, b: Tangent, loc: SourceLoc) -> Tangent {
  match (a, b) {
    (Tangent::Zero, x) | (x, Tangent::Zero) => x,
    (Tangent::Expr(a), Tangent::Expr(b)) => Tangent::Expr(binop(BinOp::Add, a, b, loc)),
  }
}

fn t_sub(a: Tangent, b: Tangent, loc: SourceLoc) -> Tangent {
  match (a, b) {
    (Tangent::Zero, Tangent::Zero) => Tangent::Zero,
    (a, Tangent::Zero) => a,
    (Tangent::Zero, Tangent::Expr(b)) => Tangent::Expr(neg(b, loc)),
    (Tangent::Expr(a), Tangent::Expr(b)) => Tangent::Expr(binop(BinOp::Sub, a, b, loc)),
  }
}

/// Scale a tangent by a primal expression (`scale * dt`); `Zero` stays `Zero`.
fn t_scale(scale: Expr, t: Tangent, loc: SourceLoc) -> Tangent {
  match t {
    Tangent::Zero => Tangent::Zero,
    Tangent::Expr(e) => Tangent::Expr(binop(BinOp::Mul, scale, e, loc)),
  }
}

fn t_div_by(t: Tangent, denom: Expr, loc: SourceLoc) -> Tangent {
  match t {
    Tangent::Zero => Tangent::Zero,
    Tangent::Expr(e) => Tangent::Expr(binop(BinOp::Div, e, denom, loc)),
  }
}

fn zero_value(ty: ArgType) -> Option<Value> {
  Some(match ty {
    ArgType::Float | ArgType::Numeric | ArgType::Int => Value::Float(0.),
    ArgType::Vec2 => Value::Vec2(Vec2::new(0., 0.)),
    ArgType::Vec3 => Value::Vec3(Vec3::zeros()),
    _ => return None,
  })
}

impl<'a> DerivCtx<'a> {
  fn err(&self, msg: impl Into<String>, loc: SourceLoc) -> ErrorStack {
    let (line, col) = self.ctx.resolve_loc(loc);
    ErrorStack::new(msg).with_loc(line, col)
  }

  fn fresh(&mut self, prefix: &str) -> Sym {
    let n = self.gensym;
    self.gensym += 1;
    self
      .ctx
      .interned_symbols
      .intern(&format!("__grad_{prefix}_{n}"))
  }

  fn primal_type(&mut self, e: &Expr) -> Option<ArgType> {
    let ctx = self.ctx;
    infer_expr(ctx, &mut self.type_env, e).as_single_arg_type()
  }

  /// Bind a compound primal expression to a fresh temp, recording its type; atoms pass through.
  fn tape_primal(&mut self, expr: Expr, loc: SourceLoc) -> Expr {
    if is_atom(&expr) {
      return expr;
    }
    let ctx = self.ctx;
    let ty = infer_expr(ctx, &mut self.type_env, &expr);
    let sym = self.fresh("t");
    self.type_env.define(sym, ty);
    self.tape.push(Statement::Assignment {
      slot: None,
      name: sym,
      name_loc: loc,
      expr,
      type_hint: None,
    });
    Expr::Ident {
      res: VarRes::Unresolved,
      name: sym,
      loc,
    }
  }

  /// Bind a compound tangent expression to a fresh temp so it can be referenced multiple times
  /// without duplicating work; `Zero` and atoms pass through.
  fn tape_tangent(&mut self, t: Tangent, loc: SourceLoc) -> Tangent {
    match t {
      Tangent::Zero => Tangent::Zero,
      Tangent::Expr(e) if is_atom(&e) => Tangent::Expr(e),
      Tangent::Expr(e) => {
        let sym = self.fresh("d");
        self.tape.push(Statement::Assignment {
          slot: None,
          name: sym,
          name_loc: loc,
          expr: e,
          type_hint: None,
        });
        Tangent::Expr(Expr::Ident {
          res: VarRes::Unresolved,
          name: sym,
          loc,
        })
      }
    }
  }

  /// Force a tangent to a concrete expression, synthesizing a typed zero from `primal`'s type when
  /// the tangent is symbolic-zero.
  fn reify(&mut self, t: Tangent, primal: &Expr, loc: SourceLoc) -> Result<Expr, ErrorStack> {
    match t {
      Tangent::Expr(e) => Ok(e),
      Tangent::Zero => {
        let ty = self.primal_type(primal).ok_or_else(|| {
          self.err(
            "autodiff: could not determine a type to synthesize a zero tangent",
            loc,
          )
        })?;
        let zero = zero_value(ty).ok_or_else(|| {
          self.err(
            format!("autodiff: cannot synthesize a zero tangent for type {ty:?}"),
            loc,
          )
        })?;
        Ok(lit(zero, loc))
      }
    }
  }

  fn diff_body(&mut self, stmts: &[Statement]) -> Result<(Expr, Tangent), ErrorStack> {
    if stmts.is_empty() {
      return Err(ErrorStack::new(
        "autodiff: cannot differentiate an empty closure body",
      ));
    }
    let n = stmts.len();
    for (i, stmt) in stmts.iter().enumerate() {
      let is_last = i + 1 == n;
      match stmt {
        Statement::Assignment { name, expr, .. } => {
          let pt = self.diff_expr(expr)?;
          self.env.insert(*name, pt);
          if is_last {
            return Err(self.err(
              "autodiff: differentiated closure body must end in an expression, not an assignment",
              expr.loc(),
            ));
          }
        }
        Statement::Expr(expr) => {
          let pt = self.diff_expr(expr)?;
          if is_last {
            return Ok(pt);
          }
        }
        Statement::Return { value } => {
          return match value {
            Some(e) => self.diff_expr(e),
            None => Err(ErrorStack::new(
              "autodiff: differentiated closure cannot `return` without a value",
            )),
          };
        }
        Statement::Break { .. } => {
          return Err(ErrorStack::new(
            "autodiff: `break` is not supported inside a differentiated closure",
          ))
        }
        Statement::DestructureAssignment { rhs, .. } => {
          return Err(self.err(
            "autodiff: destructuring assignment is not supported inside a differentiated closure",
            rhs.loc(),
          ))
        }
      }
    }
    unreachable!("last statement always returns")
  }

  fn diff_expr(&mut self, expr: &Expr) -> Result<(Expr, Tangent), ErrorStack> {
    match expr {
      Expr::Literal { value, loc } => Ok((lit(value.clone(), *loc), Tangent::Zero)),
      Expr::Ident { name, loc, res: _ } => {
        if let Some(pt) = self.env.get(name).map(|(p, t)| (p.clone(), t.clone())) {
          return Ok(pt);
        }
        // Free identifier: a captured constant (inlined as a literal) or a global/builtin left
        // as-is; either way it is constant w.r.t. the differentiation parameter.
        if let Some(val) = self.captures.get(*name) {
          Ok((val.into_literal_expr(*loc), Tangent::Zero))
        } else {
          Ok((expr.clone(), Tangent::Zero))
        }
      }
      Expr::BinOp {
        op, lhs, rhs, loc, ..
      } => {
        let name = op.get_builtin_fn_name();
        match name.and_then(|n| lookup_rule(n).map(|r| (n, r))) {
          Some((_, rule)) => {
            let (lp, ld) = self.diff_expr(lhs)?;
            let (rp, rd) = self.diff_expr(rhs)?;
            let primal = binop(*op, lp.clone(), rp.clone(), *loc);
            let primal_atom = self.tape_primal(primal, *loc);
            let tangent = rule(self, &[lp, rp], &primal_atom, &[ld, rd], *loc)?;
            let tangent = self.tape_tangent(tangent, *loc);
            Ok((primal_atom, tangent))
          }
          None => Err(self.err(
            format!(
              "autodiff: operator `{}` is not differentiable",
              name.unwrap_or("<pipeline/range>")
            ),
            *loc,
          )),
        }
      }
      Expr::PrefixOp {
        op,
        expr: inner,
        loc,
      } => {
        let name = op.get_builtin_fn_name();
        match lookup_rule(name) {
          Some(rule) => {
            let (ip, id) = self.diff_expr(inner)?;
            let primal = Expr::PrefixOp {
              op: *op,
              expr: Box::new(ip.clone()),
              loc: *loc,
            };
            let primal_atom = self.tape_primal(primal, *loc);
            let tangent = rule(self, &[ip], &primal_atom, &[id], *loc)?;
            let tangent = self.tape_tangent(tangent, *loc);
            Ok((primal_atom, tangent))
          }
          None => Err(self.err(
            format!("autodiff: prefix operator `{name}` is not differentiable"),
            *loc,
          )),
        }
      }
      Expr::StaticFieldAccess { lhs, field, loc } => {
        let (lp, lt) = self.diff_expr(lhs)?;
        let primal = Expr::StaticFieldAccess {
          lhs: Box::new(lp.clone()),
          field: field.clone(),
          loc: *loc,
        };
        let primal_atom = self.tape_primal(primal, *loc);
        let tangent = match lt {
          Tangent::Zero => Tangent::Zero,
          Tangent::Expr(e) => {
            // `D(v.field) = (Dv).field` — only valid as a vector swizzle.
            let is_vec = matches!(self.primal_type(&lp), Some(ArgType::Vec2 | ArgType::Vec3));
            if !is_vec {
              return Err(self.err(
                "autodiff: field access on a non-vector value is not differentiable",
                *loc,
              ));
            }
            Tangent::Expr(Expr::StaticFieldAccess {
              lhs: Box::new(e),
              field: field.clone(),
              loc: *loc,
            })
          }
        };
        let tangent = self.tape_tangent(tangent, *loc);
        Ok((primal_atom, tangent))
      }
      Expr::Call { call, loc } => {
        let FunctionCall {
          target,
          args,
          kwargs,
          target_res: _,
        } = call;
        if !kwargs.is_empty() {
          return Err(self.err(
            "autodiff: keyword arguments in a differentiated call are not supported",
            *loc,
          ));
        }
        match target {
          FunctionCallTarget::Literal(callable) => self.diff_callable(callable, args, *loc),
          FunctionCallTarget::Name(name) => {
            if let Some(Value::Callable(callable)) = self.captures.get(*name) {
              return self.diff_callable(&callable, args, *loc);
            }
            let canonical = self.ctx.with_resolved_sym(*name, |s| {
              if get_builtin_fn_sig_entry_ix(s).is_some() {
                Some(s.to_owned())
              } else {
                FUNCTION_ALIASES.get(s).map(|a| a.to_string())
              }
            });
            match canonical {
              Some(builtin) => self.diff_builtin_call(&builtin, args, *loc),
              None => {
                let fn_name = self.ctx.with_resolved_sym(*name, |s| s.to_owned());
                Err(self.err(
                  format!(
                    "autodiff: call to non-constant function `{fn_name}` is not differentiable"
                  ),
                  *loc,
                ))
              }
            }
          }
        }
      }
      Expr::Conditional {
        cond,
        then,
        else_if_exprs,
        else_expr,
        loc,
      } => self.diff_conditional(cond, then, else_if_exprs, else_expr.as_deref(), *loc),
      Expr::Block { statements, .. } => {
        let saved = self.env.clone();
        let r = self.diff_body(statements);
        self.env = saved;
        r
      }
      Expr::Range { loc, .. } => Err(self.err(
        "autodiff: ranges are not supported inside a differentiated closure",
        *loc,
      )),
      Expr::FieldAccess { loc, .. } => Err(self.err(
        "autodiff: dynamic field/index access is not differentiable",
        *loc,
      )),
      Expr::ArrayLiteral { loc, .. } => Err(self.err(
        "autodiff: array literals are not supported inside a differentiated closure",
        *loc,
      )),
      Expr::MapLiteral { loc, .. } => Err(self.err(
        "autodiff: map literals are not supported inside a differentiated closure",
        *loc,
      )),
      Expr::Closure { loc, .. } => Err(self.err(
        "autodiff: nested closures are not supported inside a differentiated closure",
        *loc,
      )),
    }
  }

  fn diff_callable(
    &mut self,
    callable: &Callable,
    args: &[Expr],
    loc: SourceLoc,
  ) -> Result<(Expr, Tangent), ErrorStack> {
    match callable {
      Callable::Builtin { fn_entry_ix, .. } => {
        let name = fn_sigs().entries[*fn_entry_ix].0;
        self.diff_builtin_call(name, args, loc)
      }
      Callable::Closure(closure) => self.diff_inline_closure(closure, args, loc),
      Callable::PartiallyAppliedFn(_) | Callable::ComposedFn(_) | Callable::Dynamic { .. } => {
        Err(self.err(
          "autodiff: only builtins and plain closures can be differentiated (got a \
           partially-applied / composed / dynamic callable)",
          loc,
        ))
      }
    }
  }

  fn diff_builtin_call(
    &mut self,
    name: &str,
    args: &[Expr],
    loc: SourceLoc,
  ) -> Result<(Expr, Tangent), ErrorStack> {
    let rule = lookup_rule(name).ok_or_else(|| {
      self.err(
        format!("autodiff: no derivative rule for builtin `{name}`"),
        loc,
      )
    })?;
    let mut primals = Vec::with_capacity(args.len());
    let mut dargs = Vec::with_capacity(args.len());
    for arg in args {
      let (p, d) = self.diff_expr(arg)?;
      primals.push(p);
      dargs.push(d);
    }
    let primal_call = builtin_call(self.ctx, name, primals.clone(), loc);
    let primal_atom = self.tape_primal(primal_call, loc);
    let tangent = rule(self, &primals, &primal_atom, &dargs, loc)?;
    let tangent = self.tape_tangent(tangent, loc);
    Ok((primal_atom, tangent))
  }

  /// Beta-reduce a constant closure callee and differentiate its body inline.
  fn diff_inline_closure(
    &mut self,
    closure: &Closure,
    args: &[Expr],
    loc: SourceLoc,
  ) -> Result<(Expr, Tangent), ErrorStack> {
    if closure.params.len() != args.len() {
      return Err(self.err(
        "autodiff: argument count mismatch when inlining a closure",
        loc,
      ));
    }
    let mut bindings = Vec::with_capacity(args.len());
    for (param, arg) in closure.params.iter().zip(args) {
      let DestructurePattern::Ident(sym) = &param.ident else {
        return Err(self.err(
          "autodiff: destructured parameters in an inlined closure are not supported",
          loc,
        ));
      };
      let pt = self.diff_expr(arg)?;
      bindings.push((*sym, pt));
    }
    let helper_captures = closure.captured_env_scope();

    let saved_env = std::mem::take(&mut self.env);
    let saved_captures = std::mem::replace(&mut self.captures, helper_captures);
    for (sym, pt) in bindings {
      self.env.insert(sym, pt);
    }
    let result = self.diff_body(&closure.body.0);
    self.env = saved_env;
    self.captures = saved_captures;
    result
  }

  /// NOTE: every branch's primal + tangent is taped at the enclosing level, so the derivative
  /// closure evaluates all branches unconditionally.  The selects still pick the right values (and
  /// math builtins yield NaN/inf rather than erroring on bad domains), but `if`-guards do not
  /// protect a branch's work from running.
  fn diff_conditional(
    &mut self,
    cond: &Expr,
    then: &Expr,
    else_if_exprs: &[(Expr, Expr)],
    else_expr: Option<&Expr>,
    loc: SourceLoc,
  ) -> Result<(Expr, Tangent), ErrorStack> {
    let primal_cond = self.subst_primal(cond)?;
    let (then_p, then_t) = self.diff_expr(then)?;

    let mut elif = Vec::with_capacity(else_if_exprs.len());
    for (c, e) in else_if_exprs {
      let pc = self.subst_primal(c)?;
      let (p, t) = self.diff_expr(e)?;
      elif.push((pc, p, t));
    }
    let else_pt = match else_expr {
      Some(e) => Some(self.diff_expr(e)?),
      None => None,
    };

    let primal = Expr::Conditional {
      cond: Box::new(primal_cond.clone()),
      then: Box::new(then_p.clone()),
      else_if_exprs: elif
        .iter()
        .map(|(c, p, _)| (c.clone(), p.clone()))
        .collect(),
      else_expr: else_pt.as_ref().map(|(p, _)| Box::new(p.clone())),
      loc,
    };
    let primal_atom = self.tape_primal(primal, loc);

    let all_zero = matches!(then_t, Tangent::Zero)
      && elif.iter().all(|(_, _, t)| matches!(t, Tangent::Zero))
      && else_pt
        .as_ref()
        .map_or(true, |(_, t)| matches!(t, Tangent::Zero));
    if all_zero {
      return Ok((primal_atom, Tangent::Zero));
    }

    let then_te = self.reify(then_t, &then_p, loc)?;
    let mut elif_te = Vec::with_capacity(elif.len());
    for (c, p, t) in elif {
      let te = self.reify(t, &p, loc)?;
      elif_te.push((c, te));
    }
    let else_te = match else_pt {
      Some((p, t)) => self.reify(t, &p, loc)?,
      // No `else`: the false branch has no defined tangent; a zero of the `then` branch's type is
      // the only sensible choice.
      None => {
        let ty = self
          .primal_type(&then_p)
          .ok_or_else(|| self.err("autodiff: could not determine conditional branch type", loc))?;
        let zero = zero_value(ty).ok_or_else(|| {
          self.err(
            format!("autodiff: cannot synthesize a zero tangent for type {ty:?}"),
            loc,
          )
        })?;
        lit(zero, loc)
      }
    };

    let tangent_cond = Expr::Conditional {
      cond: Box::new(primal_cond),
      then: Box::new(then_te),
      else_if_exprs: elif_te,
      else_expr: Some(Box::new(else_te)),
      loc,
    };
    let tangent = self.tape_tangent(Tangent::Expr(tangent_cond), loc);
    Ok((primal_atom, tangent))
  }

  /// Reconstruct an expression's primal (no differentiation), substituting env-bound names with
  /// their primal atoms.  Used for `if` conditions, which may contain comparisons that are not
  /// themselves differentiable.
  fn subst_primal(&mut self, expr: &Expr) -> Result<Expr, ErrorStack> {
    Ok(match expr {
      Expr::Literal { .. } => expr.clone(),
      Expr::Ident { name, loc, res: _ } => {
        if let Some((p, _)) = self.env.get(name) {
          p.clone()
        } else if let Some(val) = self.captures.get(*name) {
          val.into_literal_expr(*loc)
        } else {
          expr.clone()
        }
      }
      Expr::BinOp {
        op, lhs, rhs, loc, ..
      } => binop(*op, self.subst_primal(lhs)?, self.subst_primal(rhs)?, *loc),
      Expr::PrefixOp {
        op,
        expr: inner,
        loc,
      } => Expr::PrefixOp {
        op: *op,
        expr: Box::new(self.subst_primal(inner)?),
        loc: *loc,
      },
      Expr::StaticFieldAccess { lhs, field, loc } => Expr::StaticFieldAccess {
        lhs: Box::new(self.subst_primal(lhs)?),
        field: field.clone(),
        loc: *loc,
      },
      Expr::FieldAccess { lhs, field, loc } => Expr::FieldAccess {
        lhs: Box::new(self.subst_primal(lhs)?),
        field: Box::new(self.subst_primal(field)?),
        loc: *loc,
      },
      Expr::Call { call, loc } => {
        let FunctionCall {
          target,
          args,
          kwargs,
          target_res: _,
        } = call;
        let args = args
          .iter()
          .map(|a| self.subst_primal(a))
          .collect::<Result<Vec<_>, _>>()?;
        let kwargs = kwargs
          .iter()
          .map(|(k, v)| Ok((*k, self.subst_primal(v)?)))
          .collect::<Result<FxHashMap<_, _>, ErrorStack>>()?;
        Expr::Call {
          call: FunctionCall {
            target_res: VarRes::Unresolved,
            target: target.clone(),
            args,
            kwargs,
          },
          loc: *loc,
        }
      }
      Expr::Conditional {
        cond,
        then,
        else_if_exprs,
        else_expr,
        loc,
      } => Expr::Conditional {
        cond: Box::new(self.subst_primal(cond)?),
        then: Box::new(self.subst_primal(then)?),
        else_if_exprs: else_if_exprs
          .iter()
          .map(|(c, e)| Ok((self.subst_primal(c)?, self.subst_primal(e)?)))
          .collect::<Result<Vec<_>, ErrorStack>>()?,
        else_expr: match else_expr {
          Some(e) => Some(Box::new(self.subst_primal(e)?)),
          None => None,
        },
        loc: *loc,
      },
      other => {
        return Err(self.err(
          "autodiff: unsupported expression in an `if` condition",
          other.loc(),
        ))
      }
    })
  }
}

/// Look up a derivative rule by canonical builtin/operator name.
fn lookup_rule(name: &str) -> Option<DerivRule> {
  DERIV_RULES.get(name).copied()
}

/// True if a builtin has a derivative rule.
pub fn is_differentiable(name: &str) -> bool {
  DERIV_RULES.contains_key(name)
}

fn require_non_mesh(dcx: &mut DerivCtx, e: &Expr, loc: SourceLoc) -> Result<(), ErrorStack> {
  if matches!(dcx.primal_type(e), Some(ArgType::Mesh)) {
    return Err(dcx.err(
      "autodiff: mesh-valued arithmetic is not differentiable",
      loc,
    ));
  }
  Ok(())
}

fn require_scalar(
  dcx: &mut DerivCtx,
  e: &Expr,
  what: &str,
  loc: SourceLoc,
) -> Result<(), ErrorStack> {
  match dcx.primal_type(e) {
    Some(ArgType::Float | ArgType::Numeric | ArgType::Int) | None => Ok(()),
    Some(other) => Err(dcx.err(
      format!(
        "autodiff: `{what}` differentiation is only supported for scalar operands (got {other:?})"
      ),
      loc,
    )),
  }
}

fn d_add(
  _: &mut DerivCtx,
  _a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  Ok(t_add(d[0].clone(), d[1].clone(), loc))
}

fn d_sub(
  _: &mut DerivCtx,
  _a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  Ok(t_sub(d[0].clone(), d[1].clone(), loc))
}

fn d_neg(
  _: &mut DerivCtx,
  _a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  Ok(t_neg(d[0].clone(), loc))
}

fn d_pos(
  _: &mut DerivCtx,
  _a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  _loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  Ok(d[0].clone())
}

fn d_mul(
  dcx: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  require_non_mesh(dcx, &a[0], loc)?;
  require_non_mesh(dcx, &a[1], loc)?;
  Ok(t_add(
    t_scale(a[1].clone(), d[0].clone(), loc),
    t_scale(a[0].clone(), d[1].clone(), loc),
    loc,
  ))
}

fn d_div(
  dcx: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  require_non_mesh(dcx, &a[0], loc)?;
  require_non_mesh(dcx, &a[1], loc)?;
  let num = t_sub(
    t_scale(a[1].clone(), d[0].clone(), loc),
    t_scale(a[0].clone(), d[1].clone(), loc),
    loc,
  );
  let denom = binop(BinOp::Mul, a[1].clone(), a[1].clone(), loc);
  Ok(t_div_by(num, denom, loc))
}

fn d_sin(
  dcx: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  let scale = builtin_call(dcx.ctx, "cos", vec![a[0].clone()], loc);
  Ok(t_scale(scale, d[0].clone(), loc))
}

fn d_cos(
  dcx: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  let scale = neg(builtin_call(dcx.ctx, "sin", vec![a[0].clone()], loc), loc);
  Ok(t_scale(scale, d[0].clone(), loc))
}

fn d_tan(
  _: &mut DerivCtx,
  _a: &[Expr],
  p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  // sec²(a) = 1 + tan²(a); reuse the primal `tan(a)`.
  let scale = binop(
    BinOp::Add,
    fl(1., loc),
    binop(BinOp::Mul, p.clone(), p.clone(), loc),
    loc,
  );
  Ok(t_scale(scale, d[0].clone(), loc))
}

fn d_sinh(
  dcx: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  let scale = builtin_call(dcx.ctx, "cosh", vec![a[0].clone()], loc);
  Ok(t_scale(scale, d[0].clone(), loc))
}

fn d_cosh(
  dcx: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  let scale = builtin_call(dcx.ctx, "sinh", vec![a[0].clone()], loc);
  Ok(t_scale(scale, d[0].clone(), loc))
}

fn d_tanh(
  _: &mut DerivCtx,
  _a: &[Expr],
  p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  // sech²(a) = 1 − tanh²(a); reuse the primal.
  let scale = binop(
    BinOp::Sub,
    fl(1., loc),
    binop(BinOp::Mul, p.clone(), p.clone(), loc),
    loc,
  );
  Ok(t_scale(scale, d[0].clone(), loc))
}

fn d_asin(
  dcx: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  let one_minus_a2 = binop(
    BinOp::Sub,
    fl(1., loc),
    binop(BinOp::Mul, a[0].clone(), a[0].clone(), loc),
    loc,
  );
  let denom = builtin_call(dcx.ctx, "sqrt", vec![one_minus_a2], loc);
  Ok(t_div_by(d[0].clone(), denom, loc))
}

fn d_acos(
  dcx: &mut DerivCtx,
  a: &[Expr],
  p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  Ok(t_neg(d_asin(dcx, a, p, d, loc)?, loc))
}

fn d_atan(
  _: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  let denom = binop(
    BinOp::Add,
    fl(1., loc),
    binop(BinOp::Mul, a[0].clone(), a[0].clone(), loc),
    loc,
  );
  Ok(t_div_by(d[0].clone(), denom, loc))
}

fn d_exp(
  _: &mut DerivCtx,
  _a: &[Expr],
  p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  Ok(t_scale(p.clone(), d[0].clone(), loc))
}

fn d_ln(
  _: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  Ok(t_div_by(d[0].clone(), a[0].clone(), loc))
}

fn d_log2(
  _: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  let denom = binop(
    BinOp::Mul,
    a[0].clone(),
    fl(std::f32::consts::LN_2, loc),
    loc,
  );
  Ok(t_div_by(d[0].clone(), denom, loc))
}

fn d_log10(
  _: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  let denom = binop(
    BinOp::Mul,
    a[0].clone(),
    fl(std::f32::consts::LN_10, loc),
    loc,
  );
  Ok(t_div_by(d[0].clone(), denom, loc))
}

fn d_sqrt(
  _: &mut DerivCtx,
  _a: &[Expr],
  p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  // da / (2·sqrt(a)); reuse the primal.
  let denom = binop(BinOp::Mul, fl(2., loc), p.clone(), loc);
  Ok(t_div_by(d[0].clone(), denom, loc))
}

fn d_sigmoid(
  _: &mut DerivCtx,
  _a: &[Expr],
  p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  // σ·(1−σ); reuse the primal σ.
  let scale = binop(
    BinOp::Mul,
    p.clone(),
    binop(BinOp::Sub, fl(1., loc), p.clone(), loc),
    loc,
  );
  Ok(t_scale(scale, d[0].clone(), loc))
}

fn d_pow(
  dcx: &mut DerivCtx,
  a: &[Expr],
  p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  let (base, exp) = (&a[0], &a[1]);
  match &d[1] {
    // Constant exponent: b·a^(b−1)·da (safe power rule).
    Tangent::Zero => {
      let exp_minus_1 = binop(BinOp::Sub, exp.clone(), fl(1., loc), loc);
      let pow_term = builtin_call(dcx.ctx, "pow", vec![base.clone(), exp_minus_1], loc);
      let scale = binop(BinOp::Mul, exp.clone(), pow_term, loc);
      Ok(t_scale(scale, d[0].clone(), loc))
    }
    // Non-constant exponent: a^b·(db·ln(a) + b·da/a); positive-base domain.
    Tangent::Expr(_) => {
      let ln_a = builtin_call(dcx.ctx, "ln", vec![base.clone()], loc);
      let term_db = t_scale(ln_a, d[1].clone(), loc);
      let b_over_a = binop(BinOp::Div, exp.clone(), base.clone(), loc);
      let term_da = t_scale(b_over_a, d[0].clone(), loc);
      let inner = t_add(term_db, term_da, loc);
      Ok(t_scale(p.clone(), inner, loc))
    }
  }
}

fn d_abs(
  dcx: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  require_scalar(dcx, &a[0], "abs", loc)?;
  match &d[0] {
    Tangent::Zero => Ok(Tangent::Zero),
    Tangent::Expr(de) => {
      let cond = binop(BinOp::Gte, a[0].clone(), fl(0., loc), loc);
      Ok(Tangent::Expr(Expr::Conditional {
        cond: Box::new(cond),
        then: Box::new(de.clone()),
        else_if_exprs: vec![],
        else_expr: Some(Box::new(neg(de.clone(), loc))),
        loc,
      }))
    }
  }
}

/// Branch-select derivative: `if cond { da } else { db }`.
fn branch_select(
  cond: Expr,
  da: Tangent,
  db: Tangent,
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  if matches!(da, Tangent::Zero) && matches!(db, Tangent::Zero) {
    return Ok(Tangent::Zero);
  }
  Ok(Tangent::Expr(Expr::Conditional {
    cond: Box::new(cond),
    then: Box::new(reify_scalar(da, loc)),
    else_if_exprs: vec![],
    else_expr: Some(Box::new(reify_scalar(db, loc))),
    loc,
  }))
}

fn reify_scalar(t: Tangent, loc: SourceLoc) -> Expr {
  match t {
    Tangent::Zero => fl(0., loc),
    Tangent::Expr(e) => e,
  }
}

fn d_min(
  dcx: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  require_scalar(dcx, &a[0], "min", loc)?;
  require_scalar(dcx, &a[1], "min", loc)?;
  let cond = binop(BinOp::Lte, a[0].clone(), a[1].clone(), loc);
  branch_select(cond, d[0].clone(), d[1].clone(), loc)
}

fn d_max(
  dcx: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  require_scalar(dcx, &a[0], "max", loc)?;
  require_scalar(dcx, &a[1], "max", loc)?;
  let cond = binop(BinOp::Gte, a[0].clone(), a[1].clone(), loc);
  branch_select(cond, d[0].clone(), d[1].clone(), loc)
}

fn d_clamp(
  dcx: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  // clamp(lo, hi, x); derivative selects dlo / dhi / dx by region.
  require_scalar(dcx, &a[2], "clamp", loc)?;
  if matches!(d[0], Tangent::Zero) && matches!(d[1], Tangent::Zero) && matches!(d[2], Tangent::Zero)
  {
    return Ok(Tangent::Zero);
  }
  let (lo, hi, x) = (&a[0], &a[1], &a[2]);
  let below = binop(BinOp::Lt, x.clone(), lo.clone(), loc);
  let above = binop(BinOp::Gt, x.clone(), hi.clone(), loc);
  Ok(Tangent::Expr(Expr::Conditional {
    cond: Box::new(below),
    then: Box::new(reify_scalar(d[0].clone(), loc)),
    else_if_exprs: vec![(above, reify_scalar(d[1].clone(), loc))],
    else_expr: Some(Box::new(reify_scalar(d[2].clone(), loc))),
    loc,
  }))
}

fn d_smoothstep(
  dcx: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  // smoothstep(e0, e1, x); v1 supports constant edges only.
  if !matches!(d[0], Tangent::Zero) || !matches!(d[1], Tangent::Zero) {
    return Err(dcx.err(
      "autodiff: `smoothstep` with non-constant edges is not supported",
      loc,
    ));
  }
  require_scalar(dcx, &a[2], "smoothstep", loc)?;
  let dx = match &d[2] {
    Tangent::Zero => return Ok(Tangent::Zero),
    Tangent::Expr(e) => e.clone(),
  };
  let (e0, e1, x) = (&a[0], &a[1], &a[2]);
  let denom = binop(BinOp::Sub, e1.clone(), e0.clone(), loc);
  let t = binop(
    BinOp::Div,
    binop(BinOp::Sub, x.clone(), e0.clone(), loc),
    denom.clone(),
    loc,
  );
  // 6·t·(1−t) / (e1−e0)
  let bump = binop(
    BinOp::Mul,
    fl(6., loc),
    binop(
      BinOp::Mul,
      t.clone(),
      binop(BinOp::Sub, fl(1., loc), t, loc),
      loc,
    ),
    loc,
  );
  let scale = binop(BinOp::Div, bump, denom, loc);
  let inside = binop(
    BinOp::And,
    binop(BinOp::Gt, x.clone(), e0.clone(), loc),
    binop(BinOp::Lt, x.clone(), e1.clone(), loc),
    loc,
  );
  Ok(Tangent::Expr(Expr::Conditional {
    cond: Box::new(inside),
    then: Box::new(binop(BinOp::Mul, scale, dx, loc)),
    else_if_exprs: vec![],
    else_expr: Some(Box::new(fl(0., loc))),
    loc,
  }))
}

fn d_linearstep(
  dcx: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  if !matches!(d[0], Tangent::Zero) || !matches!(d[1], Tangent::Zero) {
    return Err(dcx.err(
      "autodiff: `linearstep` with non-constant edges is not supported",
      loc,
    ));
  }
  let dx = match &d[2] {
    Tangent::Zero => return Ok(Tangent::Zero),
    Tangent::Expr(e) => e.clone(),
  };
  let (e0, e1, x) = (&a[0], &a[1], &a[2]);
  let denom = binop(BinOp::Sub, e1.clone(), e0.clone(), loc);
  let inside = binop(
    BinOp::And,
    binop(BinOp::Gt, x.clone(), e0.clone(), loc),
    binop(BinOp::Lt, x.clone(), e1.clone(), loc),
    loc,
  );
  Ok(Tangent::Expr(Expr::Conditional {
    cond: Box::new(inside),
    then: Box::new(binop(BinOp::Div, dx, denom, loc)),
    else_if_exprs: vec![],
    else_expr: Some(Box::new(fl(0., loc))),
    loc,
  }))
}

fn d_lerp(
  _: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  // lerp(t, a, b) = a·(1−t) + b·t; d = da·(1−t) + db·t + dt·(b−a).
  let (t, av, bv) = (&a[0], &a[1], &a[2]);
  let one_minus_t = binop(BinOp::Sub, fl(1., loc), t.clone(), loc);
  let b_minus_a = binop(BinOp::Sub, bv.clone(), av.clone(), loc);
  let term_a = t_scale(one_minus_t, d[1].clone(), loc);
  let term_b = t_scale(t.clone(), d[2].clone(), loc);
  let term_t = t_scale(b_minus_a, d[0].clone(), loc);
  Ok(t_add(t_add(term_a, term_b, loc), term_t, loc))
}

fn d_atan2(
  dcx: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  // atan2(y, x); d = (x·dy − y·dx) / (x² + y²).  The single-arg vec2 overload is unsupported.
  if a.len() != 2 {
    return Err(dcx.err(
      "autodiff: only the 2-argument form of `atan2` is differentiable",
      loc,
    ));
  }
  require_scalar(dcx, &a[0], "atan2", loc)?;
  require_scalar(dcx, &a[1], "atan2", loc)?;
  let (y, x) = (&a[0], &a[1]);
  let num = t_sub(
    t_scale(x.clone(), d[0].clone(), loc),
    t_scale(y.clone(), d[1].clone(), loc),
    loc,
  );
  let denom = binop(
    BinOp::Add,
    binop(BinOp::Mul, x.clone(), x.clone(), loc),
    binop(BinOp::Mul, y.clone(), y.clone(), loc),
    loc,
  );
  Ok(t_div_by(num, denom, loc))
}

fn d_deg2rad(
  _: &mut DerivCtx,
  _a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  Ok(t_scale(
    fl(std::f32::consts::PI / 180., loc),
    d[0].clone(),
    loc,
  ))
}

fn d_rad2deg(
  _: &mut DerivCtx,
  _a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  Ok(t_scale(
    fl(180. / std::f32::consts::PI, loc),
    d[0].clone(),
    loc,
  ))
}

fn d_dot(
  dcx: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  let t1 = match &d[0] {
    Tangent::Zero => Tangent::Zero,
    Tangent::Expr(e) => Tangent::Expr(builtin_call(
      dcx.ctx,
      "dot",
      vec![e.clone(), a[1].clone()],
      loc,
    )),
  };
  let t2 = match &d[1] {
    Tangent::Zero => Tangent::Zero,
    Tangent::Expr(e) => Tangent::Expr(builtin_call(
      dcx.ctx,
      "dot",
      vec![a[0].clone(), e.clone()],
      loc,
    )),
  };
  Ok(t_add(t1, t2, loc))
}

fn d_cross(
  dcx: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  let t1 = match &d[0] {
    Tangent::Zero => Tangent::Zero,
    Tangent::Expr(e) => Tangent::Expr(builtin_call(
      dcx.ctx,
      "cross",
      vec![e.clone(), a[1].clone()],
      loc,
    )),
  };
  let t2 = match &d[1] {
    Tangent::Zero => Tangent::Zero,
    Tangent::Expr(e) => Tangent::Expr(builtin_call(
      dcx.ctx,
      "cross",
      vec![a[0].clone(), e.clone()],
      loc,
    )),
  };
  Ok(t_add(t1, t2, loc))
}

fn d_normalize(
  dcx: &mut DerivCtx,
  a: &[Expr],
  p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  // n = normalize(v);  d = (dv − n·dot(n, dv)) / ‖v‖
  let dv = match &d[0] {
    Tangent::Zero => return Ok(Tangent::Zero),
    Tangent::Expr(e) => e.clone(),
  };
  let n = p; // primal = normalize(v)
  let len_v = builtin_call(dcx.ctx, "len", vec![a[0].clone()], loc);
  let dot_n_dv = builtin_call(dcx.ctx, "dot", vec![n.clone(), dv.clone()], loc);
  let proj = binop(BinOp::Mul, n.clone(), dot_n_dv, loc);
  let numer = binop(BinOp::Sub, dv, proj, loc);
  Ok(Tangent::Expr(binop(BinOp::Div, numer, len_v, loc)))
}

fn d_len(
  dcx: &mut DerivCtx,
  a: &[Expr],
  p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  // d‖v‖ = dot(v, dv) / ‖v‖; reuse the primal length.
  match &d[0] {
    Tangent::Zero => Ok(Tangent::Zero),
    Tangent::Expr(e) => {
      let dot = builtin_call(dcx.ctx, "dot", vec![a[0].clone(), e.clone()], loc);
      Ok(Tangent::Expr(binop(BinOp::Div, dot, p.clone(), loc)))
    }
  }
}

fn d_distance(
  dcx: &mut DerivCtx,
  a: &[Expr],
  p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  // distance(a, b) = ‖a−b‖;  d = dot(a−b, da−db) / dist
  let ddiff = t_sub(d[0].clone(), d[1].clone(), loc);
  match ddiff {
    Tangent::Zero => Ok(Tangent::Zero),
    Tangent::Expr(e) => {
      let diff = binop(BinOp::Sub, a[0].clone(), a[1].clone(), loc);
      let dot = builtin_call(dcx.ctx, "dot", vec![diff, e], loc);
      Ok(Tangent::Expr(binop(BinOp::Div, dot, p.clone(), loc)))
    }
  }
}

fn vec_constructor(
  dcx: &mut DerivCtx,
  name: &str,
  args: &[Expr],
  dargs: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  if dargs.iter().all(|t| matches!(t, Tangent::Zero)) {
    return Ok(Tangent::Zero);
  }
  let mut comps = Vec::with_capacity(args.len());
  for (arg, d) in args.iter().zip(dargs) {
    comps.push(dcx.reify(d.clone(), arg, loc)?);
  }
  Ok(Tangent::Expr(builtin_call(dcx.ctx, name, comps, loc)))
}

fn d_vec2(
  dcx: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  vec_constructor(dcx, "vec2", a, d, loc)
}

fn d_vec3(
  dcx: &mut DerivCtx,
  a: &[Expr],
  _p: &Expr,
  d: &[Tangent],
  loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  vec_constructor(dcx, "vec3", a, d, loc)
}

fn d_const_zero(
  _: &mut DerivCtx,
  _a: &[Expr],
  _p: &Expr,
  _d: &[Tangent],
  _loc: SourceLoc,
) -> Result<Tangent, ErrorStack> {
  Ok(Tangent::Zero)
}

static DERIV_RULES: phf::Map<&'static str, DerivRule> = phf::phf_map! {
  "add" => d_add as DerivRule,
  "sub" => d_sub,
  "mul" => d_mul,
  "div" => d_div,
  "neg" => d_neg,
  "pos" => d_pos,
  "sin" => d_sin,
  "cos" => d_cos,
  "tan" => d_tan,
  "sinh" => d_sinh,
  "cosh" => d_cosh,
  "tanh" => d_tanh,
  "asin" => d_asin,
  "acos" => d_acos,
  "atan" => d_atan,
  "exp" => d_exp,
  "ln" => d_ln,
  "log2" => d_log2,
  "log10" => d_log10,
  "sqrt" => d_sqrt,
  "sigmoid" => d_sigmoid,
  "pow" => d_pow,
  "abs" => d_abs,
  "min" => d_min,
  "max" => d_max,
  "clamp" => d_clamp,
  "smoothstep" => d_smoothstep,
  "linearstep" => d_linearstep,
  "atan2" => d_atan2,
  "lerp" => d_lerp,
  "deg2rad" => d_deg2rad,
  "rad2deg" => d_rad2deg,
  "fract" => d_pos,
  "floor" => d_const_zero,
  "ceil" => d_const_zero,
  "round" => d_const_zero,
  "trunc" => d_const_zero,
  "signum" => d_const_zero,
  "dot" => d_dot,
  "cross" => d_cross,
  "normalize" => d_normalize,
  "len" => d_len,
  "distance" => d_distance,
  "vec2" => d_vec2,
  "vec3" => d_vec3,
};

// ---------------------------------------------------------------------------
// Top-level entry point
// ---------------------------------------------------------------------------

fn seed_matches_param(seed: &Value, param_ty: ArgType) -> bool {
  matches!(
    (param_ty, seed),
    (ArgType::Vec2, Value::Vec2(_))
      | (ArgType::Vec3, Value::Vec3(_))
      | (
        ArgType::Float | ArgType::Numeric | ArgType::Int,
        Value::Float(_) | Value::Int(_)
      )
  )
}

/// Transform `input` into a closure computing its directional derivative seeded by `seed`.
///
/// The result has the same parameter as `input` and computes the JVP `Jf · seed`.  Used by the
/// `deriv` builtin and (later) by `embed_path`'s analytic-frame path.
pub(crate) fn build_directional_derivative(
  ctx: &EvalCtx,
  input: &Closure,
  seed: &Value,
) -> Result<Closure, ErrorStack> {
  if input.params.len() != 1 {
    return Err(ErrorStack::new(format!(
      "autodiff: `deriv` requires a single-parameter closure, found {} parameters",
      input.params.len()
    )));
  }
  let param = &input.params[0];
  let DestructurePattern::Ident(param_sym) = &param.ident else {
    return Err(ErrorStack::new(
      "autodiff: `deriv` requires a closure with a single named (non-destructured) parameter",
    ));
  };
  let Some(param_ty) = param.type_hint else {
    return Err(ErrorStack::new(
      "autodiff: the differentiated closure's parameter must have an explicit type annotation \
       (e.g. `|p: vec2| ...`)",
    ));
  };
  if !seed_matches_param(seed, param_ty) {
    return Err(ErrorStack::new(format!(
      "autodiff: seed direction type {:?} does not match the closure parameter type {param_ty:?}",
      seed.get_type()
    )));
  }
  let captured = input.captured_env_scope();

  let mut type_env = TypeEnv::with_default_globals(ctx);
  type_env.push_scope();
  type_env.define(*param_sym, AbstractType::Concrete(param_ty));

  let mut dcx = DerivCtx {
    ctx,
    gensym: 0,
    env: FxHashMap::default(),
    tape: Vec::new(),
    type_env,
    captures: Rc::clone(&captured),
  };

  // Seed: `d_p = <dir literal>`, baked so it const-folds through the derivative body.
  let seed_sym = dcx.fresh("d");
  dcx.tape.push(Statement::Assignment {
    slot: None,
    name: seed_sym,
    name_loc: SourceLoc::default(),
    expr: lit(seed.clone(), SourceLoc::default()),
    type_hint: None,
  });
  dcx.env.insert(
    *param_sym,
    (
      Expr::Ident {
        res: VarRes::Unresolved,
        name: *param_sym,
        loc: SourceLoc::default(),
      },
      Tangent::Expr(Expr::Ident {
        res: VarRes::Unresolved,
        name: seed_sym,
        loc: SourceLoc::default(),
      }),
    ),
  );

  let (result_primal, result_tangent) = dcx.diff_body(&input.body.0)?;
  let result_expr = dcx.reify(result_tangent, &result_primal, SourceLoc::default())?;

  let mut stmts = std::mem::take(&mut dcx.tape);
  stmts.push(Statement::Expr(result_expr));

  let captured_consts = captured.own_bindings();
  optimize_synthesized_closure_body(ctx, &input.params, &captured_consts, &mut stmts)?;

  crate::resolve::resolve_new_closure(
    ctx,
    &captured,
    Rc::clone(&input.params),
    Rc::new(ClosureBody(stmts)),
    None,
  )
}

/// Build the gradient of a scalar-output closure: for a `vecN`-input `f`, a closure returning the
/// `vecN` of `f`'s partials; for a scalar-input `f`, just `f'`.  Sugar over
/// [`build_directional_derivative`] with the standard basis seeds.
pub(crate) fn build_gradient(ctx: &EvalCtx, input: &Closure) -> Result<Value, ErrorStack> {
  if input.params.len() != 1 {
    return Err(ErrorStack::new(format!(
      "autodiff: `grad` requires a single-parameter closure, found {} parameters",
      input.params.len()
    )));
  }
  let param = &input.params[0];
  let DestructurePattern::Ident(param_sym) = &param.ident else {
    return Err(ErrorStack::new(
      "autodiff: `grad` requires a closure with a single named (non-destructured) parameter",
    ));
  };
  let Some(param_ty) = param.type_hint else {
    return Err(ErrorStack::new(
      "autodiff: the `grad` closure's parameter must have an explicit type annotation",
    ));
  };
  let seeds: Vec<Value> = match param_ty {
    ArgType::Float | ArgType::Numeric | ArgType::Int => vec![Value::Float(1.)],
    ArgType::Vec2 => vec![
      Value::Vec2(Vec2::new(1., 0.)),
      Value::Vec2(Vec2::new(0., 1.)),
    ],
    ArgType::Vec3 => vec![
      Value::Vec3(Vec3::new(1., 0., 0.)),
      Value::Vec3(Vec3::new(0., 1., 0.)),
      Value::Vec3(Vec3::new(0., 0., 1.)),
    ],
    other => {
      return Err(ErrorStack::new(format!(
        "autodiff: `grad` parameter must be numeric, vec2, or vec3 (got {other:?})"
      )))
    }
  };

  let partials = seeds
    .iter()
    .map(|s| build_directional_derivative(ctx, input, s))
    .collect::<Result<Vec<_>, _>>()?;

  if partials.len() == 1 {
    let only = partials.into_iter().next().unwrap();
    return Ok(Value::Callable(Rc::new(Callable::Closure(only))));
  }

  let vec_name = if partials.len() == 2 { "vec2" } else { "vec3" };
  let cap = Rc::new(Scope::default());
  let mut comps = Vec::with_capacity(partials.len());
  for (i, partial) in partials.into_iter().enumerate() {
    let sym = ctx.interned_symbols.intern(&format!("__grad_partial_{i}"));
    cap.insert(sym, Value::Callable(Rc::new(Callable::Closure(partial))));
    comps.push(Expr::Call {
      call: FunctionCall {
        target_res: VarRes::Unresolved,
        target: FunctionCallTarget::Name(sym),
        args: vec![Expr::Ident {
          res: VarRes::Unresolved,
          name: *param_sym,
          loc: SourceLoc::default(),
        }],
        kwargs: FxHashMap::default(),
      },
      loc: SourceLoc::default(),
    });
  }
  let body = builtin_call(ctx, vec_name, comps, SourceLoc::default());
  let mut stmts = vec![Statement::Expr(body)];
  let captured_consts = cap.own_bindings();
  optimize_synthesized_closure_body(ctx, &input.params, &captured_consts, &mut stmts)?;

  let closure = crate::resolve::resolve_new_closure(
    ctx,
    &cap,
    Rc::clone(&input.params),
    Rc::new(ClosureBody(stmts)),
    None,
  )?;
  Ok(Value::Callable(Rc::new(Callable::Closure(closure))))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    ast::{Statement, TopLevelStatement},
    optimizer::optimize_ast,
    parse_and_eval_program, parse_program_src, EvalCtx, Value, Vec2, Vec3, EMPTY_KWARGS,
  };

  fn call_v3(ctx: &EvalCtx, f: &Value, p: Vec2) -> Vec3 {
    let Value::Callable(c) = f else {
      panic!("expected a callable, got {f:?}")
    };
    let out = ctx
      .invoke_callable(c, &[Value::Vec2(p)], EMPTY_KWARGS)
      .unwrap();
    *out.as_vec3().unwrap()
  }

  fn call_scalar(ctx: &EvalCtx, f: &Value, x: f32) -> f32 {
    let Value::Callable(c) = f else {
      panic!("expected a callable, got {f:?}")
    };
    let out = ctx
      .invoke_callable(c, &[Value::Float(x)], EMPTY_KWARGS)
      .unwrap();
    out.as_float().unwrap()
  }

  fn assert_close_v3(a: Vec3, b: Vec3, tol: f32, msg: &str) {
    let d = (a - b).norm();
    assert!(d < tol, "{msg}: {a:?} vs {b:?} (|Δ|={d})");
  }

  /// Analytic directional derivative of a `vec2 -> vec3` embedding should match central
  /// differences.
  #[test]
  fn test_embedding_partials_match_finite_diff() {
    let src = r#"
phi = |p: vec2|: vec3 {
  r2 = p.x*p.x + p.y*p.y
  vec3(p.x, 2*exp(-r2/2), p.y)
}
du = deriv(phi, vec2(1, 0))
dv = deriv(phi, vec2(0, 1))
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let phi = ctx.get_global("phi").unwrap();
    let du = ctx.get_global("du").unwrap();
    let dv = ctx.get_global("dv").unwrap();

    let h = 1e-3f32;
    for &(x, y) in &[
      (0.3, 0.7),
      (-0.5, 0.2),
      (1.1, -0.4),
      (0.0, 0.0),
      (-1.0, -1.0),
    ] {
      let p = Vec2::new(x, y);
      let fd_u = (call_v3(&ctx, &phi, p + Vec2::new(h, 0.))
        - call_v3(&ctx, &phi, p - Vec2::new(h, 0.)))
        / (2. * h);
      let fd_v = (call_v3(&ctx, &phi, p + Vec2::new(0., h))
        - call_v3(&ctx, &phi, p - Vec2::new(0., h)))
        / (2. * h);
      assert_close_v3(call_v3(&ctx, &du, p), fd_u, 1e-2, "du mismatch");
      assert_close_v3(call_v3(&ctx, &dv, p), fd_v, 1e-2, "dv mismatch");
    }
  }

  #[test]
  fn test_plane_and_cylinder_partials() {
    let src = r#"
plane = |p: vec2|: vec3 { vec3(p.x, 0, p.y) }
cyl = |p: vec2|: vec3 { vec3(cos(p.x), p.y, sin(p.x)) }
plane_du = deriv(plane, vec2(1, 0))
cyl_du = deriv(cyl, vec2(1, 0))
cyl_dv = deriv(cyl, vec2(0, 1))
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let plane_du = ctx.get_global("plane_du").unwrap();
    let cyl = ctx.get_global("cyl").unwrap();
    let cyl_du = ctx.get_global("cyl_du").unwrap();
    let cyl_dv = ctx.get_global("cyl_dv").unwrap();

    // plane ∂u is constant (1, 0, 0)
    assert_close_v3(
      call_v3(&ctx, &plane_du, Vec2::new(0.4, -0.2)),
      Vec3::new(1., 0., 0.),
      1e-5,
      "plane du",
    );

    let h = 1e-3f32;
    for &(x, y) in &[(0.3, 0.7), (2.1, -0.4), (-1.2, 0.9)] {
      let p = Vec2::new(x, y);
      let fd_u = (call_v3(&ctx, &cyl, p + Vec2::new(h, 0.))
        - call_v3(&ctx, &cyl, p - Vec2::new(h, 0.)))
        / (2. * h);
      let fd_v = (call_v3(&ctx, &cyl, p + Vec2::new(0., h))
        - call_v3(&ctx, &cyl, p - Vec2::new(0., h)))
        / (2. * h);
      assert_close_v3(call_v3(&ctx, &cyl_du, p), fd_u, 1e-2, "cyl du");
      assert_close_v3(call_v3(&ctx, &cyl_dv, p), fd_v, 1e-2, "cyl dv");
    }
  }

  /// Per-rule scalar micro-tests: each `f'` matches central differences.
  #[test]
  fn test_scalar_rule_micro() {
    let cases: &[(&str, &[f32])] = &[
      ("|x: float| sin(x)", &[0.2, 1.0, -0.7]),
      ("|x: float| cos(x)", &[0.2, 1.0, -0.7]),
      ("|x: float| tan(x)", &[0.2, 0.9, -0.5]),
      ("|x: float| sinh(x)", &[0.2, -0.9, 1.1]),
      ("|x: float| cosh(x)", &[0.2, -0.9, 1.1]),
      ("|x: float| tanh(x)", &[0.2, -1.0, 0.7]),
      ("|x: float| asin(x)", &[-0.6, 0.1, 0.8]),
      ("|x: float| acos(x)", &[-0.6, 0.1, 0.8]),
      ("|x: float| atan(x)", &[0.5, -1.3, 2.3]),
      ("|x: float| exp(x)", &[0.2, 1.0, -0.7]),
      ("|x: float| ln(x)", &[0.5, 1.0, 2.3]),
      ("|x: float| sqrt(x)", &[0.5, 1.0, 2.3]),
      ("|x: float| sigmoid(x)", &[0.2, -1.0, 0.7]),
      ("|x: float| pow(x, 3)", &[0.5, 1.0, 2.3]),
      ("|x: float| pow(2, x)", &[0.5, 1.0, 2.3]),
      ("|x: float| abs(x)", &[0.5, -1.3, 2.3]),
      ("|x: float| min(x, 1)", &[0.5, 1.3, -0.2]),
      ("|x: float| max(x, 1)", &[0.5, 1.3, -0.2]),
      ("|x: float| clamp(0, 1, x)", &[-0.5, 0.3, 1.7]),
      ("|x: float| smoothstep(0, 1, x)", &[-0.5, 0.3, 0.8, 1.7]),
      ("|x: float| linearstep(0, 1, x)", &[-0.5, 0.3, 0.8, 1.7]),
      ("|x: float| fract(x)", &[0.3, -0.6, 2.4]),
      ("|x: float| atan2(x, 2)", &[0.5, -1.3, 2.3]),
      ("|x: float| atan2(1.5, x)", &[0.5, 2.3, -1.1]),
      ("|x: float| x*x*x + 2*x", &[0.5, -1.3, 2.3]),
      ("|x: float| 1 / x", &[0.5, 2.3, -1.1]),
      ("|x: float| deg2rad(x)", &[10., 90.]),
    ];
    let h = 1e-3f32;
    for (body, pts) in cases {
      let src = format!("f = {body}\ndf = deriv(f, 1.0)");
      let ctx = parse_and_eval_program(&src).unwrap();
      let f = ctx.get_global("f").unwrap();
      let df = ctx.get_global("df").unwrap();
      for &x in *pts {
        let fd = (call_scalar(&ctx, &f, x + h) - call_scalar(&ctx, &f, x - h)) / (2. * h);
        let an = call_scalar(&ctx, &df, x);
        assert!(
          (fd - an).abs() < 3e-2,
          "rule `{body}` @ {x}: analytic {an} vs finite-diff {fd}"
        );
      }
    }
  }

  #[test]
  fn test_vector_rules_dot_normalize_len() {
    let src = r#"
f = |p: vec3|: num { len(p) }
g = |p: vec3|: vec3 { normalize(p) }
df = deriv(f, vec3(1, 0, 0))
dg = deriv(g, vec3(1, 0, 0))
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let f = ctx.get_global("f").unwrap();
    let df = ctx.get_global("df").unwrap();
    let g = ctx.get_global("g").unwrap();
    let dg = ctx.get_global("dg").unwrap();

    let call_v3_from_v3 = |f: &Value, p: Vec3| -> Vec3 {
      let Value::Callable(c) = f else { panic!() };
      let out = ctx
        .invoke_callable(c, &[Value::Vec3(p)], EMPTY_KWARGS)
        .unwrap();
      *out.as_vec3().unwrap()
    };
    let call_f_from_v3 = |f: &Value, p: Vec3| -> f32 {
      let Value::Callable(c) = f else { panic!() };
      let out = ctx
        .invoke_callable(c, &[Value::Vec3(p)], EMPTY_KWARGS)
        .unwrap();
      out.as_float().unwrap()
    };

    let h = 1e-3f32;
    for &p in &[Vec3::new(0.3, 0.7, -0.2), Vec3::new(1.1, -0.4, 0.9)] {
      let fd_len = (call_f_from_v3(&f, p + Vec3::new(h, 0., 0.))
        - call_f_from_v3(&f, p - Vec3::new(h, 0., 0.)))
        / (2. * h);
      let an_len = call_f_from_v3(&df, p);
      assert!(
        (fd_len - an_len).abs() < 1e-2,
        "len du: {an_len} vs {fd_len}"
      );

      let fd_n = (call_v3_from_v3(&g, p + Vec3::new(h, 0., 0.))
        - call_v3_from_v3(&g, p - Vec3::new(h, 0., 0.)))
        / (2. * h);
      assert_close_v3(call_v3_from_v3(&dg, p), fd_n, 1e-2, "normalize du");
    }
  }

  /// Piecewise-constant builtins carry a symbolic-zero tangent, so they no longer error and
  /// contribute exactly 0.
  #[test]
  fn test_piecewise_constant_zero_tangent() {
    let src = r#"
f = |x: float| floor(x) + ceil(x) + round(x) + trunc(x) + signum(x) + x*x
df = deriv(f, 1.0)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let df = ctx.get_global("df").unwrap();
    for &x in &[0.3f32, -1.7, 2.4] {
      assert_eq!(call_scalar(&ctx, &df, x), 2. * x);
    }
  }

  /// A `let` binding referenced multiple times differentiates correctly (subsumed into the tape).
  #[test]
  fn test_let_sharing() {
    let src = r#"
phi = |p: vec2|: vec3 {
  a = p.x * p.x
  vec3(a, a, p.y)
}
du = deriv(phi, vec2(1, 0))
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let phi = ctx.get_global("phi").unwrap();
    let du = ctx.get_global("du").unwrap();
    let h = 1e-3f32;
    let p = Vec2::new(0.6, -0.3);
    let fd = (call_v3(&ctx, &phi, p + Vec2::new(h, 0.))
      - call_v3(&ctx, &phi, p - Vec2::new(h, 0.)))
      / (2. * h);
    assert_close_v3(call_v3(&ctx, &du, p), fd, 1e-2, "let-shared du");
  }

  /// Inlining a const helper closure and differentiating through it.
  #[test]
  fn test_const_closure_inlining() {
    let src = r#"
sq = |x: float| x * x
phi = |p: vec2|: vec3 { vec3(sq(p.x), 0, p.y) }
du = deriv(phi, vec2(1, 0))
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let phi = ctx.get_global("phi").unwrap();
    let du = ctx.get_global("du").unwrap();
    let h = 1e-3f32;
    let p = Vec2::new(0.6, -0.3);
    let fd = (call_v3(&ctx, &phi, p + Vec2::new(h, 0.))
      - call_v3(&ctx, &phi, p - Vec2::new(h, 0.)))
      / (2. * h);
    assert_close_v3(call_v3(&ctx, &du, p), fd, 1e-2, "inlined du");
  }

  #[test]
  fn test_grad_scalar_and_vector() {
    let src = r#"
f = |p: vec2|: num { p.x*p.x + 3*p.y }
gf = grad(f)
g = |x: float| x*x
dg = grad(g)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let gf = ctx.get_global("gf").unwrap();
    // ∇f = (2x, 3)
    let Value::Callable(c) = &gf else { panic!() };
    let out = ctx
      .invoke_callable(c, &[Value::Vec2(Vec2::new(0.5, 2.0))], EMPTY_KWARGS)
      .unwrap();
    let g2 = *out.as_vec2().unwrap();
    assert!(
      (g2.x - 1.0).abs() < 1e-4 && (g2.y - 3.0).abs() < 1e-4,
      "grad f = {g2:?}"
    );

    let dg = ctx.get_global("dg").unwrap();
    assert!((call_scalar(&ctx, &dg, 3.0) - 6.0).abs() < 1e-3);
  }

  /// When the closure is dynamic (captures a runtime value), `deriv` cannot const-fold and runs at
  /// eval time instead — the same transform, with the capture inlined as a literal.
  #[test]
  fn test_dynamic_deriv_eval_path() {
    let src = r#"
make = |scale: float| {
  f = |p: vec2|: vec3 { vec3(p.x * scale, 0, p.y) }
  deriv(f, vec2(1, 0))
}
du = make(2.0)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let du = ctx.get_global("du").unwrap();
    // ∂/∂u of vec3(2·x, 0, y) is the constant (2, 0, 0).
    assert_close_v3(
      call_v3(&ctx, &du, Vec2::new(0.4, -0.9)),
      Vec3::new(2., 0., 0.),
      1e-5,
      "dynamic deriv",
    );
  }

  /// A static `deriv(...)` must const-fold to a `Value::Callable` literal during optimization.
  #[test]
  fn test_static_deriv_const_folds() {
    let src = r#"
phi = |p: vec2|: vec3 { vec3(p.x, p.x*p.y, p.y) }
du = deriv(phi, vec2(1, 0))
"#;
    let ctx = EvalCtx::default();
    let mut ast = parse_program_src(&ctx, src).unwrap();
    optimize_ast(&ctx, &mut ast).unwrap();
    let du_expr = ast
      .statements
      .iter()
      .find_map(|s| match s {
        TopLevelStatement::Statement(Statement::Assignment { name, expr, .. })
          if ctx.with_resolved_sym(*name, |n| n == "du") =>
        {
          Some(expr)
        }
        _ => None,
      })
      .expect("du assignment");
    assert!(
      matches!(du_expr.as_literal(), Some(Value::Callable(_))),
      "expected `deriv(...)` to const-fold to a Callable literal, got {du_expr:?}"
    );
  }

  /// Phase 2 milestone: after peephole + symbolic-zero, the folded derivative body carries no dead
  /// `x±0` / `x*1` / `x*0` / `x/1` identities.
  #[test]
  fn test_deriv_body_is_simplified() {
    let src = r#"
phi = |p: vec2|: vec3 {
  r2 = p.x*p.x + p.y*p.y
  vec3(p.x, 2*exp(-r2/2), p.y)
}
du = deriv(phi, vec2(1, 0))
"#;
    let ctx = EvalCtx::default();
    let mut ast = parse_program_src(&ctx, src).unwrap();
    optimize_ast(&ctx, &mut ast).unwrap();
    let du = ast
      .statements
      .iter()
      .find_map(|s| match s {
        TopLevelStatement::Statement(Statement::Assignment { name, expr, .. })
          if ctx.with_resolved_sym(*name, |n| n == "du") =>
        {
          expr.as_literal().cloned()
        }
        _ => None,
      })
      .expect("du assignment");
    let Value::Callable(c) = &du else {
      panic!("du not a callable")
    };
    let Callable::Closure(closure) = &**c else {
      panic!("du not a closure")
    };

    let is_zero = |e: &Expr| {
      matches!(e.as_literal(), Some(Value::Int(0)))
        || matches!(e.as_literal(), Some(Value::Float(f)) if *f == 0.)
    };
    let is_one = |e: &Expr| {
      matches!(e.as_literal(), Some(Value::Int(1)))
        || matches!(e.as_literal(), Some(Value::Float(f)) if *f == 1.)
    };
    let mut dead = 0;
    for stmt in &closure.body.0 {
      for expr in stmt.exprs() {
        expr.traverse(&mut |e| {
          if let Expr::BinOp { op, lhs, rhs, .. } = e {
            let hit = match op {
              BinOp::Add | BinOp::Sub => is_zero(lhs) || is_zero(rhs),
              BinOp::Mul => is_zero(lhs) || is_zero(rhs) || is_one(lhs) || is_one(rhs),
              BinOp::Div => is_zero(lhs) || is_one(rhs),
              _ => false,
            };
            if hit {
              dead += 1;
            }
          }
        });
      }
    }
    assert_eq!(dead, 0, "derivative body still contains dead identity ops");
  }

  fn deriv_err(src: &str) -> ErrorStack {
    parse_and_eval_program(src).unwrap_err()
  }

  #[test]
  fn test_bailouts() {
    let cases: &[(&str, &str)] = &[
      (
        "f = |a: float, b: float| a + b\nd = deriv(f, 1.0)",
        "single-parameter",
      ),
      ("f = |p| p * 2\nd = deriv(f, 1.0)", "type annotation"),
      (
        "f = |p: vec2|: vec3 { vec3(p.x, 0, p.y) }\nd = deriv(f, 1.0)",
        "does not match",
      ),
      (
        "f = |p: vec3| fbm(p)\nd = deriv(f, vec3(1, 0, 0))",
        "no derivative rule for builtin `fbm`",
      ),
      (
        "f = |p: float| p % 2\nd = deriv(f, 1.0)",
        "not differentiable",
      ),
    ];
    for (src, needle) in cases {
      let err = deriv_err(src);
      let msg = format!("{err}");
      assert!(
        msg.contains(needle),
        "expected error containing `{needle}`, got: {msg}"
      );
    }
  }

  /// Node-located bail-outs carry a real source location (not the default 0).
  #[test]
  fn test_bailout_has_source_loc() {
    let err = deriv_err("f = |p: vec3| fbm(p)\nd = deriv(f, vec3(1, 0, 0))");
    assert!(
      err.loc.is_some() && err.loc != Some((0, 0)),
      "expected a non-default source loc, got {:?}",
      err.loc
    );
  }
}
