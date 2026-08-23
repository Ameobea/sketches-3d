//! Rewrites `return` and `break` into nested conditionals so the evaluator never sees an exit.
//!
//! Sound because Geoscript has no cross-scope mutation: nothing after a statement can observe it
//! except *whether it exited*, so `if c { return a }; rest` ≡ `if c { a } else { rest }`.
//!
//! Runs at the top of `optimize_ast`, before resolution — the optimizer re-resolves from names,
//! so a slot-based rewrite would be undone. See `docs/control-flow-desugar-plan.md`.

use std::rc::Rc;

use fxhash::FxHashSet;

use crate::{
  ast::{
    ClosureArg, DestructurePattern, Expr, MapLiteralEntry, Program, SourceLoc, Statement,
    TopLevelStatement,
  },
  ErrorStack, EvalCtx, Sym, Value,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
  Return,
  Break,
}

impl Kind {
  fn word(self) -> &'static str {
    match self {
      Kind::Return => "return",
      Kind::Break => "break",
    }
  }
}

const RETURN_OUTSIDE_FN: &str = "`return` outside of a function is not allowed";
const BREAK_OUTSIDE_BLOCK: &str = "`break` used outside of a block";

fn position_err(kind: Kind) -> String {
  format!(
    "`{}` is only allowed as a statement or inside the arms of a conditional that is a statement; \
     move it out of the expression",
    kind.word()
  )
}

pub fn check_exit_positions(program: &Program) -> Vec<(SourceLoc, String)> {
  let mut c = Checker::default();
  for stmt in &program.statements {
    match stmt {
      TopLevelStatement::Statement(s) => c.stmt(s),
      TopLevelStatement::Export { expr, .. } => c.operand(expr),
      TopLevelStatement::Import { .. } => {}
    }
  }
  c.errs
}

pub(crate) fn desugar_exits(ctx: &EvalCtx, program: &mut Program) -> Result<(), ErrorStack> {
  if let Some((loc, msg)) = check_exit_positions(program).into_iter().next() {
    let (line, col) = ctx.resolve_loc(loc);
    return Err(ErrorStack::new(msg).with_loc(line, col));
  }
  let mut w = Walker { ctx };
  for stmt in &mut program.statements {
    match stmt {
      TopLevelStatement::Statement(s) => w.stmt(s)?,
      TopLevelStatement::Export { expr, .. } => w.expr(expr)?,
      TopLevelStatement::Import { .. } => (),
    }
  }
  Ok(())
}

// ---------------------------------------------------------------------------------------------
// Position checking

/// `*_scope` is whether a catch scope of that kind exists at all; `*_ok` whether the path down
/// from it has passed only through allowed positions.
#[derive(Default)]
struct Checker {
  errs: Vec<(SourceLoc, String)>,
  ret_scope: bool,
  ret_ok: bool,
  brk_scope: bool,
  brk_ok: bool,
}

impl Checker {
  fn stmt(&mut self, s: &Statement) {
    match s {
      Statement::Return { value, loc } => {
        if !self.ret_scope {
          self.errs.push((*loc, RETURN_OUTSIDE_FN.to_owned()));
        } else if !self.ret_ok {
          self.errs.push((*loc, position_err(Kind::Return)));
        }
        if let Some(v) = value {
          self.stmt_pos(v);
        }
      }
      Statement::Break { value, loc } => {
        if !self.brk_scope {
          self.errs.push((*loc, BREAK_OUTSIDE_BLOCK.to_owned()));
        } else if !self.brk_ok {
          self.errs.push((*loc, position_err(Kind::Break)));
        }
        if let Some(v) = value {
          self.stmt_pos(v);
        }
      }
      Statement::Assignment { expr, .. } => self.stmt_pos(expr),
      Statement::DestructureAssignment { rhs, .. } => self.stmt_pos(rhs),
      Statement::Expr(e) => self.stmt_pos(e),
    }
  }

  fn stmt_pos(&mut self, e: &Expr) {
    match e {
      Expr::Conditional { .. } => self.conditional(e),
      Expr::Block { statements, .. } => self.block(statements, true),
      _ => self.operand(e),
    }
  }

  /// An arm is transparent: flags carry through from the conditional's own position.
  fn arm(&mut self, e: &Expr) {
    match e {
      Expr::Block { statements, .. } => statements.iter().for_each(|s| self.stmt(s)),
      _ => self.stmt_pos(e),
    }
  }

  fn conditional(&mut self, e: &Expr) {
    let Expr::Conditional {
      cond,
      then,
      else_if_exprs,
      else_expr,
      ..
    } = e
    else {
      unreachable!()
    };
    self.operand(cond);
    self.arm(then);
    for (c, a) in else_if_exprs {
      self.operand(c);
      self.arm(a);
    }
    if let Some(a) = else_expr {
      self.arm(a);
    }
  }

  fn block(&mut self, statements: &[Statement], stmt_pos: bool) {
    let saved = (self.ret_ok, self.brk_scope, self.brk_ok);
    self.ret_ok &= stmt_pos;
    self.brk_scope = true;
    self.brk_ok = true;
    statements.iter().for_each(|s| self.stmt(s));
    (self.ret_ok, self.brk_scope, self.brk_ok) = saved;
  }

  fn closure(&mut self, params: &[ClosureArg], body: &[Statement]) {
    for p in params {
      if let Some(d) = &p.default_val {
        self.operand(d);
      }
    }
    let saved = (self.ret_scope, self.ret_ok, self.brk_scope, self.brk_ok);
    (self.ret_scope, self.ret_ok, self.brk_scope, self.brk_ok) = (true, true, false, false);
    body.iter().for_each(|s| self.stmt(s));
    (self.ret_scope, self.ret_ok, self.brk_scope, self.brk_ok) = saved;
  }

  fn operand(&mut self, e: &Expr) {
    let saved = (self.ret_ok, self.brk_ok);
    (self.ret_ok, self.brk_ok) = (false, false);
    match e {
      Expr::Closure { params, body, .. } => self.closure(params, &body.0),
      Expr::Block { statements, .. } => self.block(statements, false),
      Expr::Conditional { .. } => self.conditional(e),
      _ => for_each_child(e, &mut |c| self.operand(c)),
    }
    (self.ret_ok, self.brk_ok) = saved;
  }
}

// ---------------------------------------------------------------------------------------------
// Rewriting

/// Post-order walk: every nested catch scope is rewritten before the list that contains it, so
/// moving statements can never disturb a break target.
struct Walker<'a> {
  ctx: &'a EvalCtx,
}

impl Walker<'_> {
  fn stmt(&mut self, s: &mut Statement) -> Result<(), ErrorStack> {
    for e in s.exprs_mut() {
      self.expr(e)?;
    }
    Ok(())
  }

  fn stmts(&mut self, stmts: &mut [Statement]) -> Result<(), ErrorStack> {
    stmts.iter_mut().try_for_each(|s| self.stmt(s))
  }

  /// Arm blocks belong to the enclosing catch scope, so they are walked but never rewritten.
  fn arm(&mut self, e: &mut Expr) -> Result<(), ErrorStack> {
    match e {
      Expr::Block { statements, .. } => self.stmts(statements),
      _ => self.expr(e),
    }
  }

  fn expr(&mut self, e: &mut Expr) -> Result<(), ErrorStack> {
    match e {
      Expr::Conditional {
        cond,
        then,
        else_if_exprs,
        else_expr,
        ..
      } => {
        self.expr(cond)?;
        self.arm(then)?;
        for (c, a) in else_if_exprs {
          self.expr(c)?;
          self.arm(a)?;
        }
        if let Some(a) = else_expr {
          self.arm(a)?;
        }
      }
      Expr::Block { statements, .. } => {
        self.stmts(statements)?;
        *statements = rewrite(self.ctx, statements, Kind::Break)?;
      }
      Expr::Closure { body, .. } => {
        let body = Rc::make_mut(body);
        self.stmts(&mut body.0)?;
        body.0 = rewrite(self.ctx, &body.0, Kind::Return)?;
      }
      _ => for_each_child_mut(e, &mut |c| self.expr(c))?,
    }
    Ok(())
  }
}

fn rewrite(ctx: &EvalCtx, stmts: &[Statement], kind: Kind) -> Result<Vec<Statement>, ErrorStack> {
  let Some(i) = stmts.iter().position(|s| stmt_exits(s, kind)) else {
    return Ok(stmts.to_vec());
  };
  let mut out: Vec<Statement> = stmts[..i].to_vec();
  let rest = &stmts[i + 1..];

  // R1: the exit's value becomes the list's tail; anything after it is dead. Re-examining the
  // demoted value is what lets an exit nested inside it (`return { .. return 2 .. }`) rewrite.
  if let Statement::Return { value, loc } | Statement::Break { value, loc } = &stmts[i] {
    let v = value.clone().unwrap_or(nil(*loc));
    out.extend(rewrite(ctx, &[Statement::Expr(v)], kind)?);
    return Ok(out);
  }

  let (bind, rhs) = match &stmts[i] {
    Statement::Expr(e) => (None, e),
    Statement::Assignment {
      name,
      name_loc,
      expr,
      type_hint,
      ..
    } => (Some(Bind::Name(*name, *name_loc, *type_hint)), expr),
    Statement::DestructureAssignment { lhs, rhs, .. } => (Some(Bind::Pattern(lhs.clone())), rhs),
    Statement::Return { .. } | Statement::Break { .. } => unreachable!(),
  };

  match rhs {
    // R2 / R3: `rest` (plus the rebinding, if any) is appended into every arm.
    Expr::Conditional {
      cond,
      then,
      else_if_exprs,
      else_expr,
      loc,
    } => {
      let arm = |a: &Expr| -> Result<Box<Expr>, ErrorStack> {
        let joined = join(ctx, &arm_stmts(a), bind.as_ref(), rest, a.loc());
        Ok(Box::new(block(rewrite(ctx, &joined, kind)?, a.loc())))
      };
      let then = arm(then)?;
      let else_ifs = else_if_exprs
        .iter()
        .map(|(c, a)| Ok((c.clone(), *arm(a)?)))
        .collect::<Result<Vec<_>, ErrorStack>>()?;
      let else_expr = Some(match else_expr {
        Some(a) => arm(a)?,
        None => {
          let joined = join(ctx, &[], bind.as_ref(), rest, *loc);
          let stmts = if joined.is_empty() {
            vec![Statement::Expr(nil(*loc))]
          } else {
            joined
          };
          Box::new(block(rewrite(ctx, &stmts, kind)?, *loc))
        }
      });
      out.push(Statement::Expr(Expr::Conditional {
        cond: cond.clone(),
        then,
        else_if_exprs: else_ifs,
        else_expr,
        loc: *loc,
      }));
    }
    // R4: a block in statement position holding a `return` is spliced into the enclosing list.
    Expr::Block {
      statements, loc, ..
    } => {
      let joined = join(ctx, statements, bind.as_ref(), rest, *loc);
      out.extend(rewrite(ctx, &joined, kind)?);
    }
    other => {
      return Err(ErrorStack::new(format!(
        "internal error: `{}` in an unexpected position survived exit-position checking (near \
         {:?})",
        kind.word(),
        ctx.resolve_loc(other.loc())
      )))
    }
  }
  Ok(out)
}

enum Bind {
  Name(Sym, SourceLoc, Option<crate::ArgType>),
  Pattern(DestructurePattern),
}

impl Bind {
  fn stmt(&self, expr: Expr) -> Statement {
    match self {
      Bind::Name(name, name_loc, type_hint) => Statement::Assignment {
        name: *name,
        name_loc: *name_loc,
        expr,
        type_hint: *type_hint,
        slot: None,
      },
      Bind::Pattern(lhs) => Statement::DestructureAssignment {
        lhs: lhs.clone(),
        rhs: expr,
        slots: None,
      },
    }
  }

  fn names(&self) -> FxHashSet<Sym> {
    let mut out = FxHashSet::default();
    match self {
      Bind::Name(name, ..) => {
        out.insert(*name);
      }
      Bind::Pattern(lhs) => lhs.visit_idents(&mut |s| {
        out.insert(s);
      }),
    }
    out
  }
}

/// Splices `head` (an arm's or block's statements) in front of `rest`, rebinding `head`'s tail
/// value to `bind` when the exit sat on an assignment. Renames any `head` binding that `rest`
/// mentions, since the join makes it visible where it was not before.
fn join(
  ctx: &EvalCtx,
  head: &[Statement],
  bind: Option<&Bind>,
  rest: &[Statement],
  loc: SourceLoc,
) -> Vec<Statement> {
  let mut head = head.to_vec();
  if !rest.is_empty() {
    rename_conflicts(ctx, &mut head, bind, rest);
  }
  if let Some(bind) = bind {
    let tail = match head.last() {
      Some(Statement::Expr(_)) => match head.pop() {
        Some(Statement::Expr(e)) => e,
        _ => unreachable!(),
      },
      _ => nil(loc),
    };
    head.push(bind.stmt(tail));
  }
  head.extend_from_slice(rest);
  head
}

fn rename_conflicts(
  ctx: &EvalCtx,
  head: &mut [Statement],
  bind: Option<&Bind>,
  rest: &[Statement],
) {
  let mut bound = FxHashSet::default();
  for s in head.iter() {
    binds_of(s, &mut bound);
  }
  if let Some(bind) = bind {
    for n in bind.names() {
      bound.remove(&n);
    }
  }
  if bound.is_empty() {
    return;
  }
  let mut used = FxHashSet::default();
  for s in rest {
    names_used(s, &mut used);
  }
  let mut conflicts: Vec<Sym> = bound.into_iter().filter(|n| used.contains(n)).collect();
  conflicts.sort_by_key(|s| s.0);
  for name in conflicts {
    let (line, col) = binding_loc(head, name).map_or((0, 0), |l| ctx.resolve_loc(l));
    // `with_resolved_sym` holds a borrow of the interner for the duration of its callback.
    let base = ctx.with_resolved_sym(name, |n| n.to_owned());
    let fresh = ctx
      .interned_symbols
      .intern_synthetic(&format!("__geoscript_internal__cf_{line}_{col}_{base}"));
    rename_from(head, name, fresh);
  }
}

fn binding_loc(stmts: &[Statement], name: Sym) -> Option<SourceLoc> {
  stmts.iter().find_map(|s| match s {
    Statement::Assignment {
      name: n, name_loc, ..
    } if *n == name => Some(*name_loc),
    Statement::DestructureAssignment { lhs, rhs, .. } if pattern_binds(lhs, name) => {
      Some(rhs.loc())
    }
    _ => None,
  })
}

/// Sequential: statements before the binding still refer to the enclosing scope's `from`.
/// Once the binding is in effect, every mention below is the renamed one — including inside
/// nested blocks and closures, where a rebinding shadows `to` exactly as it shadowed `from`.
fn rename_from(stmts: &mut [Statement], from: Sym, to: Sym) {
  let mut active = false;
  for s in stmts {
    if active {
      for e in s.exprs_mut() {
        rename_expr(e, from, to);
      }
    }
    match s {
      Statement::Assignment { name, .. } if *name == from => {
        *name = to;
        active = true;
      }
      Statement::DestructureAssignment { lhs, .. } if pattern_binds(lhs, from) => {
        rename_pattern(lhs, from, to);
        active = true;
      }
      _ => (),
    }
  }
}

fn rename_expr(e: &mut Expr, from: Sym, to: Sym) {
  match e {
    Expr::Ident { name, .. } if *name == from => *name = to,
    Expr::Closure { params, body, .. } => {
      // Defaults evaluate in the enclosing scope, so they rename even when a param shadows.
      if params.iter().any(|p| p.default_val.is_some()) {
        for p in Rc::make_mut(params) {
          if let Some(d) = &mut p.default_val {
            rename_expr(d, from, to);
          }
        }
      }
      if !params.iter().any(|p| pattern_binds(&p.ident, from)) {
        for s in &mut Rc::make_mut(body).0 {
          rename_stmt(s, from, to);
        }
      }
    }
    Expr::Block { statements, .. } => statements.iter_mut().for_each(|s| rename_stmt(s, from, to)),
    Expr::Call { call, .. } => {
      if let crate::ast::FunctionCallTarget::Name(n) = &mut call.target {
        if *n == from {
          *n = to;
        }
      }
      for a in &mut call.args {
        rename_expr(a, from, to);
      }
      for v in call.kwargs.values_mut() {
        rename_expr(v, from, to);
      }
    }
    _ => for_each_child_mut_infallible(e, &mut |c| rename_expr(c, from, to)),
  }
}

fn rename_stmt(s: &mut Statement, from: Sym, to: Sym) {
  for e in s.exprs_mut() {
    rename_expr(e, from, to);
  }
  match s {
    Statement::Assignment { name, .. } if *name == from => *name = to,
    Statement::DestructureAssignment { lhs, .. } => rename_pattern(lhs, from, to),
    _ => (),
  }
}

fn rename_pattern(p: &mut DestructurePattern, from: Sym, to: Sym) {
  match p {
    DestructurePattern::Ident(s) => {
      if *s == from {
        *s = to;
      }
    }
    DestructurePattern::Array(ps) => ps.iter_mut().for_each(|p| rename_pattern(p, from, to)),
    DestructurePattern::Map(m) => m.values_mut().for_each(|p| rename_pattern(p, from, to)),
  }
}

fn pattern_binds(p: &DestructurePattern, name: Sym) -> bool {
  let mut found = false;
  p.visit_idents(&mut |s| found |= s == name);
  found
}

fn binds_of(s: &Statement, out: &mut FxHashSet<Sym>) {
  match s {
    Statement::Assignment { name, .. } => {
      out.insert(*name);
    }
    Statement::DestructureAssignment { lhs, .. } => lhs.visit_idents(&mut |s| {
      out.insert(s);
    }),
    _ => (),
  }
}

/// Every name `rest` could possibly resolve — free, bound, or captured. Deliberately
/// over-approximate: a false positive only costs a rename.
fn names_used(s: &Statement, out: &mut FxHashSet<Sym>) {
  binds_of(s, out);
  s.traverse_exprs(&mut |e| match e {
    Expr::Ident { name, .. } => {
      out.insert(*name);
    }
    Expr::Call { call, .. } => {
      if let crate::ast::FunctionCallTarget::Name(n) = &call.target {
        out.insert(*n);
      }
    }
    Expr::Closure { params, .. } => {
      for p in params.iter() {
        p.ident.visit_idents(&mut |s| {
          out.insert(s);
        });
      }
    }
    Expr::Block { statements, .. } => statements.iter().for_each(|s| binds_of(s, out)),
    _ => (),
  });
}

// ---------------------------------------------------------------------------------------------
// Exit search: only looks through positions the checker admits

fn stmt_exits(s: &Statement, kind: Kind) -> bool {
  match s {
    Statement::Return { .. } => kind == Kind::Return,
    Statement::Break { .. } => kind == Kind::Break,
    Statement::Assignment { expr, .. } => expr_exits(expr, kind),
    Statement::DestructureAssignment { rhs, .. } => expr_exits(rhs, kind),
    Statement::Expr(e) => expr_exits(e, kind),
  }
}

fn expr_exits(e: &Expr, kind: Kind) -> bool {
  match e {
    Expr::Conditional {
      then,
      else_if_exprs,
      else_expr,
      ..
    } => {
      arm_exits(then, kind)
        || else_if_exprs.iter().any(|(_, a)| arm_exits(a, kind))
        || else_expr.as_deref().is_some_and(|a| arm_exits(a, kind))
    }
    // Breaks were already caught by the block itself; only `return` escapes.
    Expr::Block { statements, .. } => {
      kind == Kind::Return && statements.iter().any(|s| stmt_exits(s, Kind::Return))
    }
    _ => false,
  }
}

fn arm_exits(e: &Expr, kind: Kind) -> bool {
  match e {
    Expr::Block { statements, .. } => statements.iter().any(|s| stmt_exits(s, kind)),
    _ => expr_exits(e, kind),
  }
}

// ---------------------------------------------------------------------------------------------

fn arm_stmts(a: &Expr) -> Vec<Statement> {
  match a {
    Expr::Block { statements, .. } => statements.clone(),
    _ => vec![Statement::Expr(a.clone())],
  }
}

fn nil(loc: SourceLoc) -> Expr {
  Expr::Literal {
    value: Value::Nil,
    loc,
  }
}

fn block(statements: Vec<Statement>, loc: SourceLoc) -> Expr {
  Expr::Block {
    statements,
    loc,
    end_loc: loc,
  }
}

fn for_each_child(e: &Expr, f: &mut impl FnMut(&Expr)) {
  match e {
    Expr::BinOp { lhs, rhs, .. } => {
      f(lhs);
      f(rhs);
    }
    Expr::PrefixOp { expr, .. } => f(expr),
    Expr::Range { start, end, .. } => {
      f(start);
      if let Some(e) = end {
        f(e);
      }
    }
    Expr::StaticFieldAccess { lhs, .. } => f(lhs),
    Expr::FieldAccess {
      lhs, field, field2, ..
    } => {
      f(lhs);
      f(field);
      if let Some(x) = field2 {
        f(x);
      }
    }
    Expr::Call { call, .. } => {
      call.args.iter().for_each(&mut *f);
      call.kwargs.values().for_each(f);
    }
    Expr::ArrayLiteral { elements, .. } => elements.iter().for_each(f),
    Expr::MapLiteral { entries, .. } => entries.iter().for_each(|en| f(en.expr())),
    Expr::Conditional {
      cond,
      then,
      else_if_exprs,
      else_expr,
      ..
    } => {
      f(cond);
      f(then);
      for (c, a) in else_if_exprs {
        f(c);
        f(a);
      }
      if let Some(a) = else_expr {
        f(a);
      }
    }
    Expr::Block { statements, .. } => statements.iter().for_each(|s| s.exprs().for_each(&mut *f)),
    Expr::Closure { body, .. } => body.0.iter().for_each(|s| s.exprs().for_each(&mut *f)),
    Expr::Ident { .. } | Expr::Literal { .. } => (),
  }
}

fn for_each_child_mut(
  e: &mut Expr,
  f: &mut impl FnMut(&mut Expr) -> Result<(), ErrorStack>,
) -> Result<(), ErrorStack> {
  let mut res = Ok(());
  for_each_child_mut_infallible(e, &mut |c| {
    if res.is_ok() {
      res = f(c);
    }
  });
  res
}

fn for_each_child_mut_infallible(e: &mut Expr, f: &mut impl FnMut(&mut Expr)) {
  match e {
    Expr::BinOp { lhs, rhs, .. } => {
      f(lhs);
      f(rhs);
    }
    Expr::PrefixOp { expr, .. } => f(expr),
    Expr::Range { start, end, .. } => {
      f(start);
      if let Some(e) = end {
        f(e);
      }
    }
    Expr::StaticFieldAccess { lhs, .. } => f(lhs),
    Expr::FieldAccess {
      lhs, field, field2, ..
    } => {
      f(lhs);
      f(field);
      if let Some(x) = field2 {
        f(x);
      }
    }
    Expr::Call { call, .. } => {
      call.args.iter_mut().for_each(&mut *f);
      call.kwargs.values_mut().for_each(f);
    }
    Expr::ArrayLiteral { elements, .. } => elements.iter_mut().for_each(f),
    Expr::MapLiteral { entries, .. } => entries.iter_mut().for_each(|en| match en {
      MapLiteralEntry::KeyValue { value, .. } => f(value),
      MapLiteralEntry::Splat { expr } => f(expr),
    }),
    Expr::Conditional {
      cond,
      then,
      else_if_exprs,
      else_expr,
      ..
    } => {
      f(cond);
      f(then);
      for (c, a) in else_if_exprs {
        f(c);
        f(a);
      }
      if let Some(a) = else_expr {
        f(a);
      }
    }
    Expr::Block { statements, .. } => statements
      .iter_mut()
      .for_each(|s| s.exprs_mut().for_each(&mut *f)),
    Expr::Closure { body, .. } => Rc::make_mut(body)
      .0
      .iter_mut()
      .for_each(|s| s.exprs_mut().for_each(&mut *f)),
    Expr::Ident { .. } | Expr::Literal { .. } => (),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{parse_and_eval_program, parse_program_src};

  fn desugared(src: &str) -> (EvalCtx, Program) {
    let ctx = EvalCtx::default();
    let mut program = parse_program_src(&ctx, src).unwrap();
    desugar_exits(&ctx, &mut program).unwrap();
    (ctx, program)
  }

  fn count_exits(program: &Program) -> usize {
    fn in_stmt(s: &Statement, n: &mut usize) {
      if matches!(s, Statement::Return { .. } | Statement::Break { .. }) {
        *n += 1;
      }
      s.traverse_exprs(&mut |e| match e {
        Expr::Block { statements, .. } => statements.iter().for_each(|s| in_stmt(s, n)),
        Expr::Closure { body, .. } => body.0.iter().for_each(|s| in_stmt(s, n)),
        _ => (),
      });
    }
    let mut n = 0;
    for s in &program.statements {
      match s {
        TopLevelStatement::Statement(s) => in_stmt(s, &mut n),
        TopLevelStatement::Export { expr, .. } => expr.traverse(&mut |e| match e {
          Expr::Block { statements, .. } => statements.iter().for_each(|s| in_stmt(s, &mut n)),
          Expr::Closure { body, .. } => body.0.iter().for_each(|s| in_stmt(s, &mut n)),
          _ => (),
        }),
        TopLevelStatement::Import { .. } => (),
      }
    }
    n
  }

  fn has_synthetic(ctx: &EvalCtx, program: &Program) -> bool {
    let mut found = false;
    let mut check = |s: Sym| found |= ctx.interned_symbols.is_synthetic(s);
    fn walk(s: &Statement, check: &mut impl FnMut(Sym)) {
      match s {
        Statement::Assignment { name, .. } => check(*name),
        Statement::DestructureAssignment { lhs, .. } => lhs.visit_idents(check),
        _ => (),
      }
      s.traverse_exprs(&mut |e| match e {
        Expr::Ident { name, .. } => check(*name),
        Expr::Block { statements, .. } => statements.iter().for_each(|s| walk(s, check)),
        Expr::Closure { body, .. } => body.0.iter().for_each(|s| walk(s, check)),
        _ => (),
      });
    }
    for s in &program.statements {
      if let TopLevelStatement::Statement(s) = s {
        walk(s, &mut check);
      }
    }
    found
  }

  fn int(src: &str, name: &str) -> i64 {
    parse_and_eval_program(src.to_owned())
      .unwrap_or_else(|e| panic!("eval failed: {e}\n{src}"))
      .get_global(name)
      .unwrap()
      .as_int()
      .unwrap()
  }

  /// The invariant the evaluator relies on: after the pass no exit statement survives anywhere.
  #[test]
  fn no_exits_survive() {
    for src in [
      "f = || { return 1 }",
      "f = |x| { if x { return 1 }\n2 }",
      "f = |x| { if x { return 1 } else if x { return 2 } else { return 3 } }",
      "f = |x| { y = if x { return 1 } else { 2 }\ny + 1 }",
      "f = |x| { y = { if x { return 1 }\n2 }\ny + 1 }",
      "x = { if true { break 5 }\n10 }",
      "f = |x| { r = { if x { break 1 }\n2 }\nif r > 1 { return 0 }\nr }",
      "f = || { x = 100\nreturn { x = 200\nreturn 2\n{ 1 } } }",
      "f = |x| { g = |y| { if y { return 1 }\n2 }\ng(x) }",
    ] {
      let (_ctx, program) = desugared(src);
      assert_eq!(count_exits(&program), 0, "exit survived in:\n{src}");
    }
  }

  /// R1–R6 preserve today's values, including the cases the old evaluator had special rules for.
  #[test]
  fn rules_preserve_values() {
    // R1: unconditional exit, rest is dead.
    assert_eq!(int("f = |x| { return 1\n2 }\nout = f(0)", "out"), 1);
    // R2: else-if ladder; the trailing statement is dead.
    let ladder = "f = |x| { if x == 1 || x == 4 { return 0 } else if x == 2 { return 100 } else { \
                  return 200 }\nreturn -100 }\n";
    assert_eq!(int(&format!("{ladder}out = f(1)"), "out"), 0);
    assert_eq!(int(&format!("{ladder}out = f(2)"), "out"), 100);
    assert_eq!(int(&format!("{ladder}out = f(3)"), "out"), 200);
    // R2 with no else: the non-exiting path keeps falling through to `rest`.
    assert_eq!(
      int("f = |x| { if x { return 1 }\n2 }\nout = f(false)", "out"),
      2
    );
    // R2 with no else and no rest: value is Nil, so the fallback is observable as `is_nil`.
    assert_eq!(
      int(
        "f = |x| { if x { return 1 } }\nout = if f(false) == nil { 7 } else { 8 }",
        "out"
      ),
      7
    );
    // R3: assignment rhs; the non-exiting arm rebinds and continues.
    assert_eq!(
      int(
        "f = |x| { y = if x { return 1 } else { 4 }\ny + 1 }\nout = f(false)",
        "out"
      ),
      5
    );
    assert_eq!(
      int(
        "f = |x| { y = if x { return 1 } else { 4 }\ny + 1 }\nout = f(true)",
        "out"
      ),
      1
    );
    // R3 with no else: the missing arm binds nil.
    assert_eq!(
      int(
        "f = |x| { y = if x { return 1 }\nif y == nil { 9 } else { 8 } }\nout = f(false)",
        "out"
      ),
      9
    );
    // R4: a block in statement position holding a return is spliced.
    assert_eq!(
      int(
        "f = |x| { y = { if x { return 1 }\n4 }\ny + 1 }\nout = f(false)",
        "out"
      ),
      5
    );
    assert_eq!(
      int(
        "f = |x| { y = { if x { return 1 }\n4 }\ny + 1 }\nout = f(true)",
        "out"
      ),
      1
    );
    // R5: break targets the nearest block; arms are transparent.
    assert_eq!(
      int(
        "f = |x| { out = { if x == 0 { break 100\n4 } else { x } }\nout }\nout = f(0)",
        "out"
      ),
      100
    );
    assert_eq!(
      int(
        "f = |x| { { if x > 0 { if x > 10 { break 1000 }\nbreak 100 }\nx } }\nout = f(20)",
        "out"
      ),
      1000
    );
    assert_eq!(
      int(
        "f = |x| { { if x > 0 { if x > 10 { break 1000 }\nbreak 100 }\nx } }\nout = f(5)",
        "out"
      ),
      100
    );
    assert_eq!(
      int(
        "f = |x| { { if x > 0 { if x > 10 { break 1000 }\nbreak 100 }\nx } }\nout = f(-1)",
        "out"
      ),
      -1
    );
    // Nested blocks keep distinct break targets.
    assert_eq!(
      int(
        "out = { inner = { if true { break 7 }\n0 }\ninner + 1 }",
        "out"
      ),
      8
    );
    // Top-level break inside a block still works.
    assert_eq!(int("out = { if true { break 5 }\n10 }", "out"), 5);
    // R1 clause (d): an exit inside an exit's value rewrites; the inner one wins.
    assert_eq!(
      int(
        "f = || { x = 100\nreturn { x = 200\nreturn 2\n{ 1 } } }\nout = f()",
        "out"
      ),
      2
    );
    assert_eq!(
      int(
        "f = |x| { return if x { return 1 } else { 2 } }\nout = f(false)",
        "out"
      ),
      2
    );
    assert_eq!(
      int(
        "f = |x| { return if x { return 1 } else { 2 } }\nout = f(true)",
        "out"
      ),
      1
    );
  }

  /// R7: joining `rest` into a scope must not let that scope's bindings shadow it.
  #[test]
  fn renames_only_on_conflict() {
    let conflict = "f = |t, c, d| { if c { t = 5\nif d { return 0 } }\nt }\n";
    // The `t = 5` is arm-local today, so the tail still sees the parameter.
    assert_eq!(int(&format!("{conflict}out = f(1, true, false)"), "out"), 1);
    assert_eq!(
      int(&format!("{conflict}out = f(1, false, false)"), "out"),
      1
    );
    assert_eq!(int(&format!("{conflict}out = f(1, true, true)"), "out"), 0);
    let (ctx, program) = desugared(conflict);
    assert!(
      has_synthetic(&ctx, &program),
      "expected a rename in:\n{conflict}"
    );

    // Splicing a block (R4) exposes its bindings to the enclosing tail the same way.
    let block_conflict = "f = |x, c| { y = 1\nz = { y = 2\nif c { return 0 }\n3 }\ny + z + x }\n";
    assert_eq!(
      int(&format!("{block_conflict}out = f(10, false)"), "out"),
      14
    );
    assert_eq!(int(&format!("{block_conflict}out = f(10, true)"), "out"), 0);
    let (ctx, program) = desugared(block_conflict);
    assert!(
      has_synthetic(&ctx, &program),
      "expected a rename in:\n{block_conflict}"
    );

    // Nothing to shadow — output must stay gensym-free.
    for clear in [
      "f = |x| { if x { a = 5\nreturn a }\n2 }",
      "f = |x| { y = if x { return 1 } else { 4 }\ny + 1 }",
      "f = |x| { y = { if x { return 1 }\n4 }\ny + 1 }",
    ] {
      let (ctx, program) = desugared(clear);
      assert!(
        !has_synthetic(&ctx, &program),
        "unexpected rename in:\n{clear}"
      );
    }
  }

  /// Exits are legal only in statement positions (plus an exit's own value); everything else is
  /// rejected before evaluation, with a location.
  #[test]
  fn rejects_exits_in_operands() {
    for (src, needle) in [
      (
        "f = |x| { 1 + if x { return 1 } else { 2 } }",
        "only allowed as a statement",
      ),
      (
        "f = |x| { abs(if x { return 1 } else { 2 }) }",
        "only allowed as a statement",
      ),
      (
        "f = |x| { [if x { return 1 } else { 2 }] }",
        "only allowed as a statement",
      ),
      (
        "f = |x| { m = { a: if x { return 1 } else { 2 } }\nm }",
        "only allowed as a statement",
      ),
      (
        "f = |x| { 0..(if x { return 1 } else { 2 }) }",
        "only allowed as a statement",
      ),
      (
        "f = |x| { a = [1, 2]\na[if x { return 1 } else { 0 }] }",
        "only allowed as a statement",
      ),
      (
        "f = |x| { 1 | add(if x { return 1 } else { 2 }) }",
        "only allowed as a statement",
      ),
      (
        "f = |x| { if (if x { return 1 } else { false }) { 2 } else { 3 } }",
        "only allowed as a statement",
      ),
      (
        "g = || { f = |x = (if true { return 1 } else { 2 })| { x }\nf() }",
        "only allowed as a statement",
      ),
      (
        "out = { 1 + if true { break 2 } else { 3 } }",
        "only allowed as a statement",
      ),
      // No catch scope at all — these keep the evaluator's old wording.
      ("if true { return 1 }", "outside of a function"),
      (
        "a = if { return true } { 1 } else { 2 }",
        "outside of a function",
      ),
      (
        "f = |x| { if x > 0 { break 1 }\nx }\nout = f(1)",
        "outside of a block",
      ),
      ("x = if true { break 1 } else { 2 }", "outside of a block"),
    ] {
      let err = parse_and_eval_program(src.to_owned())
        .err()
        .unwrap_or_else(|| panic!("expected an error for:\n{src}"));
      let msg = format!("{err}");
      assert!(
        msg.contains(needle),
        "expected {needle:?} in {msg:?}\n{src}"
      );
      assert!(err.loc.is_some(), "expected a location for:\n{src}");
    }
  }

  /// The pass is idempotent, so a program optimized twice is not a hazard.
  #[test]
  fn idempotent() {
    let (ctx, mut program) = desugared("f = |x| { if x { return 1 }\n2 }");
    desugar_exits(&ctx, &mut program).unwrap();
    assert_eq!(count_exits(&program), 0);
  }
}

/// Not a correctness check: reports how much the rewrite grows real programs and how often the
/// R7 renamer fires. Run with `cargo test --lib corpus_growth -- --ignored --nocapture`.
#[cfg(test)]
#[test]
#[ignore]
fn corpus_growth() {
  use crate::{parse_program_src, EvalCtx};

  fn size(program: &Program) -> usize {
    fn expr(e: &Expr, n: &mut usize) {
      *n += 1;
      match e {
        Expr::Block { statements, .. } => statements.iter().for_each(|s| stmt(s, n)),
        Expr::Closure { body, .. } => body.0.iter().for_each(|s| stmt(s, n)),
        _ => for_each_child(e, &mut |c| expr(c, n)),
      }
    }
    fn stmt(s: &Statement, n: &mut usize) {
      *n += 1;
      s.exprs().for_each(|e| expr(e, n));
    }
    let mut n = 0;
    for s in &program.statements {
      if let TopLevelStatement::Statement(s) = s {
        stmt(s, &mut n);
      }
    }
    n
  }

  let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../..");
  let mut files: Vec<std::path::PathBuf> = Vec::new();
  for dir in ["geo_compositions", "src/viz/wasm/geoscript/examples"] {
    let mut stack = vec![std::path::PathBuf::from(root).join(dir)];
    while let Some(d) = stack.pop() {
      let Ok(rd) = std::fs::read_dir(&d) else {
        continue;
      };
      for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
          stack.push(p);
        } else if p.extension().is_some_and(|x| x == "geo") {
          files.push(p);
        }
      }
    }
  }
  files.sort();

  let (mut total_before, mut total_after, mut renamed, mut parsed, mut worst) =
    (0, 0, 0, 0, (0.0, String::new()));
  for f in &files {
    let Ok(src) = std::fs::read_to_string(f) else {
      continue;
    };
    let ctx = EvalCtx::default();
    let Ok(mut program) = parse_program_src(&ctx, &src) else {
      continue;
    };
    parsed += 1;
    let before = size(&program);
    let syms_before = ctx.interned_symbols.synthetic_count();
    if desugar_exits(&ctx, &mut program).is_err() {
      println!("POSITION ERROR: {}", f.display());
      continue;
    }
    let after = size(&program);
    if ctx.interned_symbols.synthetic_count() > syms_before {
      renamed += 1;
      println!("renamed: {}", f.display());
    }
    total_before += before;
    total_after += after;
    let ratio = after as f64 / before.max(1) as f64;
    if ratio > worst.0 {
      worst = (ratio, f.file_name().unwrap().to_string_lossy().into_owned());
    }
  }
  println!(
    "corpus: {parsed}/{} parsed, nodes {total_before} -> {total_after} ({:+.2}%), worst file {} \
     ({:.2}x), renames {renamed}",
    files.len(),
    (total_after as f64 / total_before as f64 - 1.0) * 100.0,
    worst.1,
    worst.0
  );
}
