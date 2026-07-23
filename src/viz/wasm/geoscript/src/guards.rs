//! Extraction of "guard" (switching) functions from closures: the scalar expressions whose zero
//! sets mark C¹ discontinuities in the closure's output — `abs`/`min`/`max`/`clamp` arguments and
//! conditional comparison boundaries.  `embed_path`'s crease-aware refinement (design doc §12)
//! root-finds guard sign changes to place Steiner points exactly on creases.
//!
//! Unlike autodiff this is not a transform: the synthesized closure reuses the original's
//! statements and captured scope verbatim and just returns the lifted guard subexpressions as an
//! array, so there are no type requirements and no capture inlining.  Anything that can't be
//! lifted is skipped — a missed guard degrades to plain a-posteriori refinement, never worse.
//! Guards are lifted to the top level of the body, so a candidate is kept only when its free
//! identifiers all resolve there (params, prior top-level assignments, captures, builtins); the
//! rare block-local shadow slips through as a constant-valued (hence inert) guard.

use std::{cell::RefCell, rc::Rc};

use fxhash::{FxHashMap, FxHashSet};

use crate::{
  ast::{
    BinOp, ClosureBody, Expr, FunctionCall, FunctionCallTarget, MapLiteralEntry, SourceLoc,
    Statement, VarRes,
  },
  builtins::{fn_defs::get_builtin_fn_sig_entry_ix, FUNCTION_ALIASES},
  CapturedScope, Closure, EvalCtx, Scope, Sym, Value,
};

pub(crate) const MAX_GUARDS: usize = 16;

fn sub(lhs: Expr, rhs: Expr, loc: SourceLoc) -> Expr {
  Expr::BinOp {
    op: BinOp::Sub,
    lhs: Box::new(lhs),
    rhs: Box::new(rhs),
    pre_resolved_def_ix: None,
    loc,
  }
}

/// `trig(π·arg)`: a single evaluable guard whose sign flips at every threshold of a step-lattice
/// builtin — `sin` vanishes at all integers of `arg` (floor/ceil/fract), `cos` at all
/// half-integers (round).
fn lattice_guard(ctx: &EvalCtx, trig: &str, arg: Expr, loc: SourceLoc) -> Expr {
  let scaled = Expr::BinOp {
    op: BinOp::Mul,
    lhs: Box::new(Expr::Literal {
      value: Value::Float(std::f32::consts::PI),
      loc,
    }),
    rhs: Box::new(arg),
    pre_resolved_def_ix: None,
    loc,
  };
  Expr::Call {
    call: FunctionCall {
      target_res: VarRes::Unresolved,
      target: FunctionCallTarget::Name(ctx.interned_symbols.intern(trig)),
      args: vec![scaled],
      kwargs: FxHashMap::default(),
    },
    loc,
  }
}

struct GuardCtx<'a> {
  ctx: &'a EvalCtx,
  captures: Rc<Scope>,
  bindable: FxHashSet<Sym>,
  guards: Vec<Expr>,
}

impl GuardCtx<'_> {
  fn resolves(&self, sym: Sym) -> bool {
    self.bindable.contains(&sym)
      || self.captures.get(sym).is_some()
      || self.ctx.with_resolved_sym(sym, |s| {
        get_builtin_fn_sig_entry_ix(s).is_some() || FUNCTION_ALIASES.contains_key(s)
      })
  }

  /// A candidate guard is liftable iff every free identifier resolves at the body's top level and
  /// it contains no closure literal (whose params would escape).
  fn liftable(&self, e: &Expr) -> bool {
    match e {
      Expr::Ident { name, .. } => self.resolves(*name),
      Expr::Literal { .. } => true,
      Expr::Closure { .. } => false,
      Expr::BinOp { lhs, rhs, .. } => self.liftable(lhs) && self.liftable(rhs),
      Expr::PrefixOp { expr, .. } => self.liftable(expr),
      Expr::Range { start, end, .. } => {
        self.liftable(start) && end.as_deref().map(|e| self.liftable(e)).unwrap_or(true)
      }
      Expr::StaticFieldAccess { lhs, .. } => self.liftable(lhs),
      Expr::FieldAccess { lhs, field, .. } => self.liftable(lhs) && self.liftable(field),
      Expr::Call { call, .. } => {
        let target_ok = match &call.target {
          FunctionCallTarget::Name(sym) => self.resolves(*sym),
          FunctionCallTarget::Literal(_) => true,
        };
        target_ok
          && call.args.iter().all(|a| self.liftable(a))
          && call.kwargs.values().all(|a| self.liftable(a))
      }
      Expr::ArrayLiteral { elements, .. } => elements.iter().all(|e| self.liftable(e)),
      Expr::MapLiteral { entries, .. } => entries.iter().all(|e| match e {
        MapLiteralEntry::KeyValue { value, .. } => self.liftable(value),
        MapLiteralEntry::Splat { expr } => self.liftable(expr),
      }),
      Expr::Conditional {
        cond,
        then,
        else_if_exprs,
        else_expr,
        ..
      } => {
        self.liftable(cond)
          && self.liftable(then)
          && else_if_exprs
            .iter()
            .all(|(c, e)| self.liftable(c) && self.liftable(e))
          && else_expr
            .as_deref()
            .map(|e| self.liftable(e))
            .unwrap_or(true)
      }
      // Blocks bind locals; lifting one out of context is not worth reasoning about.
      Expr::Block { .. } => false,
    }
  }

  fn push_guard(&mut self, e: Expr) {
    if self.guards.len() < MAX_GUARDS && self.liftable(&e) {
      self.guards.push(e);
    }
  }

  /// Guards hiding in a boolean condition: each ordered comparison contributes `lhs − rhs`.
  fn collect_cond_guards(&mut self, cond: &Expr) {
    match cond {
      Expr::BinOp {
        op, lhs, rhs, loc, ..
      } => match op {
        BinOp::Gt | BinOp::Gte | BinOp::Lt | BinOp::Lte => {
          self.push_guard(sub((**lhs).clone(), (**rhs).clone(), *loc));
        }
        BinOp::And | BinOp::Or => {
          self.collect_cond_guards(lhs);
          self.collect_cond_guards(rhs);
        }
        _ => (),
      },
      Expr::PrefixOp { expr, .. } => self.collect_cond_guards(expr),
      _ => (),
    }
  }

  fn collect_call_guards(&mut self, target: &FunctionCallTarget, args: &[Expr], loc: SourceLoc) {
    let canonical = match target {
      FunctionCallTarget::Name(sym) => {
        if self.bindable.contains(sym) || self.captures.get(*sym).is_some() {
          return; // shadowed / user callable: args were already visited, nothing to lift here
        }
        match self.ctx.with_resolved_sym(*sym, |s| {
          if get_builtin_fn_sig_entry_ix(s).is_some() {
            Some(s.to_owned())
          } else {
            FUNCTION_ALIASES.get(s).map(|a| a.to_string())
          }
        }) {
          Some(name) => name,
          None => return,
        }
      }
      FunctionCallTarget::Literal(callable) => match &**callable {
        crate::Callable::Builtin { fn_entry_ix, .. } => crate::builtins::fn_defs::fn_sigs().entries
          [*fn_entry_ix]
          .0
          .to_owned(),
        _ => return,
      },
    };
    match (canonical.as_str(), args.len()) {
      ("abs", 1) => self.push_guard(args[0].clone()),
      ("min" | "max", 2) => self.push_guard(sub(args[0].clone(), args[1].clone(), loc)),
      // clamp(lo, hi, x): kinks where x crosses either edge
      ("clamp", 3) => {
        self.push_guard(sub(args[2].clone(), args[0].clone(), loc));
        self.push_guard(sub(args[2].clone(), args[1].clone(), loc));
      }
      // Step lattices: discontinuous at every integer (or half-integer for round) of the arg.
      // Note the *value* generally jumps there — alignment bounds and cleans up the jump line but
      // convergence still stalls unless the composite is continuous (e.g. `abs(fract(x) − 0.5)`).
      ("floor" | "ceil" | "fract", 1) => {
        let g = lattice_guard(self.ctx, "sin", args[0].clone(), loc);
        self.push_guard(g);
      }
      ("round", 1) => {
        let g = lattice_guard(self.ctx, "cos", args[0].clone(), loc);
        self.push_guard(g);
      }
      _ => (),
    }
  }

  fn visit_expr(&mut self, e: &Expr) {
    match e {
      Expr::BinOp { lhs, rhs, .. } => {
        self.visit_expr(lhs);
        self.visit_expr(rhs);
      }
      Expr::PrefixOp { expr, .. } => self.visit_expr(expr),
      Expr::Range { start, end, .. } => {
        self.visit_expr(start);
        if let Some(end) = end {
          self.visit_expr(end);
        }
      }
      Expr::StaticFieldAccess { lhs, .. } => self.visit_expr(lhs),
      Expr::FieldAccess { lhs, field, .. } => {
        self.visit_expr(lhs);
        self.visit_expr(field);
      }
      Expr::Call { call, loc } => {
        for a in &call.args {
          self.visit_expr(a);
        }
        for a in call.kwargs.values() {
          self.visit_expr(a);
        }
        self.collect_call_guards(&call.target, &call.args, *loc);
      }
      Expr::Conditional {
        cond,
        then,
        else_if_exprs,
        else_expr,
        ..
      } => {
        self.collect_cond_guards(cond);
        self.visit_expr(cond);
        self.visit_expr(then);
        for (c, e) in else_if_exprs {
          self.collect_cond_guards(c);
          self.visit_expr(c);
          self.visit_expr(e);
        }
        if let Some(e) = else_expr {
          self.visit_expr(e);
        }
      }
      Expr::Block { statements, .. } => {
        for s in statements {
          self.visit_stmt(s, false);
        }
      }
      Expr::ArrayLiteral { elements, .. } => {
        for e in elements {
          self.visit_expr(e);
        }
      }
      Expr::MapLiteral { entries, .. } => {
        for entry in entries {
          match entry {
            MapLiteralEntry::KeyValue { value, .. } => self.visit_expr(value),
            MapLiteralEntry::Splat { expr } => self.visit_expr(expr),
          }
        }
      }
      // Nested closures' bodies reference their own params; guards inside can't be lifted.
      Expr::Closure { .. } => (),
      Expr::Ident { .. } | Expr::Literal { .. } => (),
    }
  }

  fn visit_stmt(&mut self, stmt: &Statement, top_level: bool) {
    match stmt {
      Statement::Assignment { name, expr, .. } => {
        self.visit_expr(expr);
        if top_level {
          self.bindable.insert(*name);
        }
      }
      Statement::DestructureAssignment { lhs, rhs } => {
        self.visit_expr(rhs);
        if top_level {
          lhs.visit_idents(&mut |sym| {
            self.bindable.insert(sym);
          });
        }
      }
      Statement::Expr(e) => self.visit_expr(e),
      Statement::Return { value } | Statement::Break { value } => {
        if let Some(v) = value {
          self.visit_expr(v);
        }
      }
    }
  }
}

/// Extracts the switching functions of `input` as a synthesized closure with the same params and
/// captured scope whose body re-runs the original top-level statements and returns all guards as
/// an array of scalars.  `None` when the closure has no liftable guards (or an early `return`/
/// `break` makes appending a result expression unsound).
pub(crate) fn extract_guards(ctx: &EvalCtx, input: &Closure) -> Option<Closure> {
  let captured = input.captured_env_scope()?;
  if input
    .body
    .0
    .iter()
    .any(|s| matches!(s, Statement::Return { .. } | Statement::Break { .. }))
  {
    return None;
  }

  let mut gcx = GuardCtx {
    ctx,
    captures: Rc::clone(&captured),
    bindable: FxHashSet::default(),
    guards: Vec::new(),
  };
  for param in input.params.iter() {
    param.ident.visit_idents(&mut |sym| {
      gcx.bindable.insert(sym);
    });
  }
  for stmt in &input.body.0 {
    gcx.visit_stmt(stmt, true);
  }
  if gcx.guards.is_empty() {
    return None;
  }

  // Keep every statement except a trailing bare result expression (its value is unused), then
  // return the guard array.
  let mut stmts = input.body.0.clone();
  if matches!(stmts.last(), Some(Statement::Expr(_))) {
    stmts.pop();
  }
  stmts.push(Statement::Expr(Expr::ArrayLiteral {
    elements: gcx.guards,
    loc: SourceLoc::default(),
  }));

  // Resolves builtin call targets (eval-time name lookup is scope-only, so synthesized calls like
  // the lattice guards' `sin` would otherwise not resolve) and const-folds, same as autodiff.
  let captured_consts = captured.collect_bindings_innermost_first();
  crate::optimizer::optimize_synthesized_closure_body(
    ctx,
    &input.params,
    &captured_consts,
    &mut stmts,
  )
  .ok()?;

  Some(Closure {
    params: Rc::clone(&input.params),
    body: Rc::new(ClosureBody(stmts)),
    captured_scope: CapturedScope::Strong(captured),
    arg_placeholder_scope: RefCell::new(None),
    return_type_hint: None,
    resolved: None,
    captures: Rc::from(Vec::new()),
  })
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
  use super::*;
  use crate::{parse_and_eval_program, Callable, Value, Vec2, EMPTY_KWARGS};

  fn get_closure(ctx: &EvalCtx, name: &str) -> Rc<Callable> {
    let Value::Callable(cb) = ctx.get_global(name).unwrap() else {
      panic!("{name} is not callable");
    };
    cb
  }

  fn eval_guards_at(ctx: &EvalCtx, guards: &Closure, args: &[Value]) -> Vec<f32> {
    let cb = Rc::new(Callable::Closure(guards.clone()));
    let out = ctx.invoke_callable(&cb, args, EMPTY_KWARGS).unwrap();
    let seq = out.as_sequence().expect("guards should return a sequence");
    seq
      .consume(ctx)
      .map(|v| v.unwrap().as_float().expect("guard should be a float"))
      .collect()
  }

  #[test]
  fn extracts_abs_guard_through_intermediates() {
    let src = r#"
scale = 2
f = |p: vec2|: num { t = cos(p.x * scale) * cos(p.y * scale)
0.1 + abs(t) }
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let Callable::Closure(closure) = &*get_closure(&ctx, "f") else {
      panic!()
    };
    let guards = extract_guards(&ctx, closure).expect("abs should yield a guard");
    for &(x, y) in &[(0.3f32, -0.2f32), (1.1, 0.9), (2.0, 0.4)] {
      let vals = eval_guards_at(&ctx, &guards, &[Value::Vec2(Vec2::new(x, y))]);
      assert_eq!(vals.len(), 1);
      let expected = (x * 2.).cos() * (y * 2.).cos();
      assert!(
        (vals[0] - expected).abs() < 1e-5,
        "at ({x},{y}): {} vs {expected}",
        vals[0]
      );
    }
  }

  #[test]
  fn extracts_min_clamp_and_conditional_guards() {
    let src = r#"
f = |p: vec2| min(p.x, p.y)
g = |p: vec2| clamp(0, 1, p.x)
h = |p: vec2| if p.x > 1 { p.x } else { 2 - p.x }
"#;
    let ctx = parse_and_eval_program(src).unwrap();

    let Callable::Closure(c) = &*get_closure(&ctx, "f") else {
      panic!()
    };
    let gf = extract_guards(&ctx, c).unwrap();
    let v = eval_guards_at(&ctx, &gf, &[Value::Vec2(Vec2::new(3., 1.))]);
    assert_eq!(v, vec![2.]); // x − y

    let Callable::Closure(c) = &*get_closure(&ctx, "g") else {
      panic!()
    };
    let gg = extract_guards(&ctx, c).unwrap();
    let v = eval_guards_at(&ctx, &gg, &[Value::Vec2(Vec2::new(0.25, 0.))]);
    assert_eq!(v, vec![0.25, -0.75]); // x − lo, x − hi

    let Callable::Closure(c) = &*get_closure(&ctx, "h") else {
      panic!()
    };
    let gh = extract_guards(&ctx, c).unwrap();
    let v = eval_guards_at(&ctx, &gh, &[Value::Vec2(Vec2::new(1.5, 0.))]);
    assert_eq!(v, vec![0.5]); // x − 1
  }

  #[test]
  fn skips_unliftable_and_returns_none_when_smooth() {
    // abs inside a nested closure can't be lifted; the outer body has no direct guards.
    let src = r#"
f = |p: vec2| {
  inner = |q: num| abs(q)
  inner(p.x)
}
smooth = |p: vec2| p.x * 2 + sin(p.y)
blocky = |p: vec2| {
  if p.x > 0 {
    t = cos(p.x)
    abs(t)
  } else {
    1
  }
}
"#;
    let ctx = parse_and_eval_program(src).unwrap();

    let Callable::Closure(c) = &*get_closure(&ctx, "f") else {
      panic!()
    };
    assert!(
      extract_guards(&ctx, c).is_none(),
      "nested-closure abs must not be lifted"
    );

    let Callable::Closure(c) = &*get_closure(&ctx, "smooth") else {
      panic!()
    };
    assert!(
      extract_guards(&ctx, c).is_none(),
      "smooth closure has no guards"
    );

    // The `abs(t)` guard references the branch-local `t` (unliftable), but the `p.x > 0`
    // comparison still contributes its boundary.
    let Callable::Closure(c) = &*get_closure(&ctx, "blocky") else {
      panic!()
    };
    let gb = extract_guards(&ctx, c).unwrap();
    let v = eval_guards_at(&ctx, &gb, &[Value::Vec2(Vec2::new(0.7, 0.))]);
    assert_eq!(v, vec![0.7]);
  }

  #[test]
  fn lattice_guards_for_step_builtins() {
    let src = r#"
tri = |p: vec2| abs(fract(p.x) - 0.5)
rnd = |p: vec2| round(p.x)
"#;
    let ctx = parse_and_eval_program(src).unwrap();

    // fract → sin(π·x) (zeros at integers) + abs → fract(x) − 0.5 (zeros at half-integers).
    let Callable::Closure(c) = &*get_closure(&ctx, "tri") else {
      panic!()
    };
    let g = extract_guards(&ctx, c).unwrap();
    let at = |x: f32| eval_guards_at(&ctx, &g, &[Value::Vec2(Vec2::new(x, 0.))]);
    let (below, on, above) = (at(0.9), at(1.0), at(1.1));
    assert_eq!(on.len(), 2);
    assert!(on[0].abs() < 1e-5, "sin(π·1) should vanish, got {}", on[0]);
    assert!(
      below[0] > 0. && above[0] < 0.,
      "lattice guard must flip sign across the integer"
    );
    assert!(
      at(0.5)[1].abs() < 1e-5,
      "abs guard should vanish at the half-integer"
    );

    // round → cos(π·x) (zeros at half-integers).
    let Callable::Closure(c) = &*get_closure(&ctx, "rnd") else {
      panic!()
    };
    let g = extract_guards(&ctx, c).unwrap();
    let v = eval_guards_at(&ctx, &g, &[Value::Vec2(Vec2::new(1.5, 0.))]);
    assert_eq!(v.len(), 1);
    assert!(v[0].abs() < 1e-5, "cos(π·1.5) should vanish, got {}", v[0]);
  }

  #[test]
  fn two_arg_thickness_guard_shape() {
    let src = "t = |pos: vec3, uv: vec2| 0.1 + abs(cos(uv.x * 2) * cos(uv.y * 2))";
    let ctx = parse_and_eval_program(src).unwrap();
    let Callable::Closure(c) = &*get_closure(&ctx, "t") else {
      panic!()
    };
    let g = extract_guards(&ctx, c).unwrap();
    let v = eval_guards_at(
      &ctx,
      &g,
      &[
        Value::Vec3(crate::Vec3::new(9., 9., 9.)),
        Value::Vec2(Vec2::new(0.785398, 0.)),
      ],
    );
    assert_eq!(v.len(), 1);
    assert!(
      v[0].abs() < 1e-4,
      "guard should vanish at u=π/4, got {}",
      v[0]
    );
  }
}
