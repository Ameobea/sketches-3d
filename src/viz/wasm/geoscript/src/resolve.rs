//! Static name resolution: assigns frame slots to closure params/locals, computes capture
//! specs for free variables, and rewrites identifiers with their resolution so closure
//! invocation can use flat `Vec<Value>` frames instead of hashmap scope chains. The program
//! top level is an implicit zero-param closure: top-level bindings get slots and free names
//! become `DefScope` captures materialized from the base bindings at program start.
//!
//! Runs before and after the optimizer pipeline (idempotent — re-resolves from scratch each
//! time); every closure resolves, exceeding the slot limit is a hard error.
//!
//! Assignment binds at the level it appears: rebinding a name already bound in the current
//! innermost level updates that binding; otherwise a fresh binding is created at the current
//! level, shadowing any enclosing binding until the level ends. Semantics notes vs. the
//! historical name-keyed scope chains:
//! - captures snapshot referenced free vars at closure creation; mutations of enclosing scopes made
//!   *after* creation through shared parent links are no longer observed
//! - a free var unbound at creation errors at creation (the optimizer statically rejects
//!   unknown names in user programs first, so this only surfaces on synthesized closures)

use std::rc::Rc;

use fxhash::FxHashMap;

use crate::{
  ast::{
    CaptureFrom, ClosureArg, ClosureBody, Expr, FunctionCallTarget, MapLiteralEntry,
    ProgramResolution, ResolvedBody, Statement, TopLevelStatement, VarRes,
  },
  builtins::{fn_defs::get_builtin_fn_sig_entry_ix, resolve_builtin_impl},
  ArgType, Callable, Closure, ErrorStack, EvalCtx, Program, Scope, Sym, Value,
};

#[derive(Default)]
struct ClosureCtx {
  self_name: Option<Sym>,
  /// innermost last; `[0]` is the function level (params + fn-scoped locals)
  blocks: Vec<FxHashMap<Sym, u16>>,
  next_slot: u32,
  captures: Vec<(Sym, CaptureFrom)>,
  bail: bool,
}

impl ClosureCtx {
  fn new(self_name: Option<Sym>) -> Self {
    ClosureCtx {
      self_name,
      blocks: vec![FxHashMap::default()],
      ..Default::default()
    }
  }

  fn lookup(&self, name: Sym) -> Option<u16> {
    self.blocks.iter().rev().find_map(|b| b.get(&name).copied())
  }

  fn alloc_slot(&mut self) -> u16 {
    let slot = self.next_slot;
    self.next_slot += 1;
    if self.next_slot > u16::MAX as u32 {
      self.bail = true;
    }
    slot as u16
  }

  fn add_capture(&mut self, name: Sym, from: CaptureFrom) -> u16 {
    if let Some(ix) = self
      .captures
      .iter()
      .position(|(n, f)| *n == name && *f == from)
    {
      return ix as u16;
    }
    self.captures.push((name, from));
    (self.captures.len() - 1) as u16
  }
}

#[derive(Default)]
struct Resolver {
  stack: Vec<ClosureCtx>,
  err: Option<ErrorStack>,
}

impl Resolver {
  fn cur(&mut self) -> &mut ClosureCtx {
    self.stack.last_mut().unwrap()
  }

  fn resolve_read(&mut self, name: Sym) -> VarRes {
    let cur = self.stack.len() - 1;
    if let Some(slot) = self.stack[cur].lookup(name) {
      return VarRes::Local(slot);
    }
    if self.stack[cur].self_name == Some(name) {
      return VarRes::SelfRef;
    }

    let mut src = None;
    for lvl in (0..cur).rev() {
      if let Some(slot) = self.stack[lvl].lookup(name) {
        src = Some((lvl, CaptureFrom::Local(slot)));
        break;
      }
      if self.stack[lvl].self_name == Some(name) {
        src = Some((lvl, CaptureFrom::SelfRef));
        break;
      }
    }
    let ix = match src {
      Some((lvl, from)) => {
        let mut ix = self.stack[lvl + 1].add_capture(name, from);
        for l in (lvl + 2)..=cur {
          ix = self.stack[l].add_capture(name, CaptureFrom::Capture(ix));
        }
        ix
      }
      // not lexically bound in any enclosing closure: resolved from the defining scope at
      // creation time, hoisted through intermediate closures as plain captures
      None => {
        let mut ix = self.stack[0].add_capture(name, CaptureFrom::DefScope(name));
        for l in 1..=cur {
          ix = self.stack[l].add_capture(name, CaptureFrom::Capture(ix));
        }
        ix
      }
    };
    VarRes::Capture(ix)
  }

  /// Slot for an assignment target. Assignment binds at the level it appears: a name already
  /// bound in the current innermost level is rebound in place; anything else gets a fresh
  /// slot at the current level (shadowing any enclosing binding until the level ends).
  fn resolve_assign(&mut self, name: Sym) -> u16 {
    let cur = self.cur();
    if let Some(slot) = cur.blocks.last().unwrap().get(&name).copied() {
      return slot;
    }
    let slot = cur.alloc_slot();
    cur.blocks.last_mut().unwrap().insert(name, slot);
    slot
  }

  fn walk_top_statement(&mut self, stmt: &mut TopLevelStatement) {
    match stmt {
      TopLevelStatement::Statement(s) => self.walk_statement(s),
      TopLevelStatement::Export {
        name, expr, slot, ..
      } => {
        self.walk_expr(expr, Some(*name));
        *slot = Some(self.resolve_assign(*name));
      }
      TopLevelStatement::Import { bindings, slots, .. } => {
        let mut slot_vec: Vec<u16> = Vec::new();
        bindings.visit_idents(&mut |s| slot_vec.push(self.resolve_assign(s)));
        *slots = Some(Rc::from(slot_vec));
      }
    }
  }

  fn walk_statement(&mut self, stmt: &mut Statement) {
    match stmt {
      Statement::Assignment {
        name, expr, slot, ..
      } => {
        self.walk_expr(expr, Some(*name));
        *slot = Some(self.resolve_assign(*name));
      }
      Statement::DestructureAssignment { lhs, rhs, slots } => {
        self.walk_expr(rhs, None);
        let mut slot_vec: Vec<u16> = Vec::new();
        lhs.visit_idents(&mut |s| slot_vec.push(self.resolve_assign(s)));
        *slots = Some(Rc::from(slot_vec));
      }
      Statement::Expr(e) => self.walk_expr(e, None),
      Statement::Return { value } | Statement::Break { value } => {
        if let Some(e) = value {
          self.walk_expr(e, None);
        }
      }
    }
  }

  fn walk_expr(&mut self, expr: &mut Expr, binding_name: Option<Sym>) {
    match expr {
      Expr::Ident { name, res, .. } => *res = self.resolve_read(*name),
      Expr::BinOp { lhs, rhs, .. } => {
        self.walk_expr(lhs, None);
        self.walk_expr(rhs, None);
      }
      Expr::PrefixOp { expr, .. } => self.walk_expr(expr, None),
      Expr::Range { start, end, .. } => {
        self.walk_expr(start, None);
        if let Some(end) = end {
          self.walk_expr(end, None);
        }
      }
      Expr::StaticFieldAccess { lhs, .. } => self.walk_expr(lhs, None),
      Expr::FieldAccess { lhs, field, .. } => {
        self.walk_expr(lhs, None);
        self.walk_expr(field, None);
      }
      Expr::Call { call, .. } => {
        // literal callables are already resolved at construction (fold/autodiff/guards)
        if let FunctionCallTarget::Name(name) = &call.target {
          call.target_res = self.resolve_read(*name);
        }
        for a in &mut call.args {
          self.walk_expr(a, None);
        }
        for (_k, v) in call.kwargs.iter_mut() {
          self.walk_expr(v, None);
        }
      }
      Expr::Closure { .. } => self.resolve_closure_expr(expr, binding_name),
      Expr::ArrayLiteral { elements, .. } => {
        for e in elements {
          self.walk_expr(e, None);
        }
      }
      Expr::MapLiteral { entries, .. } => {
        for entry in entries {
          match entry {
            MapLiteralEntry::KeyValue { value, .. } => self.walk_expr(value, None),
            MapLiteralEntry::Splat { expr } => self.walk_expr(expr, None),
          }
        }
      }
      Expr::Literal { .. } => {}
      Expr::Conditional {
        cond,
        then,
        else_if_exprs,
        else_expr,
        ..
      } => {
        self.walk_expr(cond, None);
        self.walk_expr(then, None);
        for (c, b) in else_if_exprs {
          self.walk_expr(c, None);
          self.walk_expr(b, None);
        }
        if let Some(e) = else_expr {
          self.walk_expr(e, None);
        }
      }
      Expr::Block { statements, .. } => {
        self.cur().blocks.push(FxHashMap::default());
        for s in statements {
          self.walk_statement(s);
        }
        self.cur().blocks.pop();
      }
    }
  }

  fn resolve_closure_expr(&mut self, expr: &mut Expr, binding_name: Option<Sym>) {
    let Expr::Closure {
      params,
      body,
      resolved,
      ..
    } = expr
    else {
      unreachable!()
    };
    *resolved = self.slot_closure(binding_name, params, body).map(Rc::new);
  }

  /// Slots one closure: pushes a `ClosureCtx`, walks defaults (against the captured env —
  /// params not visible, so their free names become captures), binds param slots in
  /// `visit_idents` order, walks the body, pops. `None` on slot overflow (`err` recorded).
  fn slot_closure(
    &mut self,
    self_name: Option<Sym>,
    params: &mut Rc<Vec<ClosureArg>>,
    body: &mut Rc<ClosureBody>,
  ) -> Option<ResolvedBody> {
    self.stack.push(ClosureCtx::new(self_name));
    if params.iter().any(|p| p.default_val.is_some()) {
      for p in Rc::make_mut(params).iter_mut() {
        if let Some(d) = &mut p.default_val {
          self.walk_expr(d, None);
        }
      }
    }
    let mut param_slots = Vec::with_capacity(params.len());
    for p in params.iter() {
      param_slots.push(self.cur().next_slot as u16);
      p.ident.visit_idents(&mut |name| {
        let slot = self.cur().alloc_slot();
        self.cur().blocks[0].insert(name, slot);
      });
    }
    for stmt in &mut Rc::make_mut(body).0 {
      self.walk_statement(stmt);
    }

    let cctx = self.stack.pop().unwrap();
    if cctx.bail {
      if self.err.is_none() {
        self.err = Some(slot_overflow_err());
      }
      return None;
    }
    Some(ResolvedBody {
      n_slots: cctx.next_slot as u16,
      captures: cctx.captures,
      param_slots,
    })
  }
}

#[cold]
fn slot_overflow_err() -> ErrorStack {
  ErrorStack::new("Closure exceeds the maximum number of local variables (65535)")
}

/// Slots a standalone statement list (the optimizer's speculative whole-block fold) with no
/// enclosing frame: block-level assignments get slots and free names become `DefScope`
/// captures for the caller to materialize. Walks a clone, so nested `Rc`'d closure bodies
/// copy-on-write rather than mutating the original AST.
pub(crate) fn resolve_standalone_stmts(
  statements: &mut [Statement],
) -> Result<(u16, Vec<(Sym, CaptureFrom)>), ErrorStack> {
  let mut r = Resolver::default();
  r.stack.push(ClosureCtx::new(None));
  for stmt in statements.iter_mut() {
    r.walk_statement(stmt);
  }
  if let Some(err) = r.err {
    return Err(err);
  }
  let cctx = r.stack.pop().unwrap();
  if cctx.bail {
    return Err(slot_overflow_err());
  }
  Ok((cctx.next_slot as u16, cctx.captures))
}

/// Looks up a free name in `scope` at capture materialization, falling back to builtins.
pub(crate) fn resolve_capture_by_name(ctx: &EvalCtx, scope: &Scope, name: Sym) -> Option<Value> {
  if let Some(v) = scope.get(name) {
    return Some(v);
  }
  ctx.with_resolved_sym(name, |name_str| {
    let fn_entry_ix = get_builtin_fn_sig_entry_ix(name_str)?;
    Some(Value::Callable(Rc::new(Callable::Builtin {
      fn_entry_ix,
      fn_impl: resolve_builtin_impl(name_str),
      pre_resolved_signature: None,
    })))
  })
}

/// Builds a resolved `Closure` value from a standalone body (optimizer-folded literal or
/// synthesized, e.g. by autodiff/guards): slots the body and materializes captures from
/// `scope` immediately; a name unbound there is an error.
pub(crate) fn resolve_new_closure(
  ctx: &EvalCtx,
  scope: &Scope,
  mut params: Rc<Vec<ClosureArg>>,
  mut body: Rc<ClosureBody>,
  return_type_hint: Option<ArgType>,
) -> Result<Closure, ErrorStack> {
  let mut r = Resolver::default();
  let meta = r.slot_closure(None, &mut params, &mut body);
  if let Some(err) = r.err {
    return Err(err);
  }
  let meta = meta.unwrap();

  let mut cap_vals = Vec::with_capacity(meta.captures.len());
  for (name, from) in &meta.captures {
    let CaptureFrom::DefScope(sym) = from else {
      return Err(ctx.with_resolved_sym(*name, |name| {
        ErrorStack::new(format!(
          "Internal error: non-def-scope capture `{name}` in standalone closure resolution"
        ))
      }));
    };
    match resolve_capture_by_name(ctx, scope, *sym) {
      Some(v) => cap_vals.push(v),
      None => {
        return Err(ctx.with_resolved_sym(*sym, |name| {
          ErrorStack::new(format!("Variable `{name}` not found"))
        }))
      }
    }
  }

  Ok(Closure {
    params,
    body,
    return_type_hint,
    resolved: Rc::new(meta),
    captures: Rc::from(cap_vals),
  })
}

/// Final phase of `optimize_ast`: resolve every closure in the program, treating the top
/// level as an implicit zero-param closure (slots for top-level bindings, `DefScope`
/// captures for free names). Idempotent.
pub(crate) fn resolve_program(program: &mut Program) -> Result<(), ErrorStack> {
  let mut r = Resolver::default();
  r.stack.push(ClosureCtx::new(None));
  for stmt in &mut program.statements {
    r.walk_top_statement(stmt);
  }
  if let Some(err) = r.err {
    return Err(err);
  }
  let cctx = r.stack.pop().unwrap();
  if cctx.bail {
    return Err(slot_overflow_err());
  }
  let mut name_slots: Vec<(Sym, u16)> = cctx
    .blocks
    .into_iter()
    .next()
    .unwrap()
    .into_iter()
    .collect();
  name_slots.sort_unstable_by_key(|(_, slot)| *slot);
  program.resolution = Some(ProgramResolution {
    n_slots: cctx.next_slot as u16,
    captures: cctx.captures,
    name_slots,
  });
  Ok(())
}
