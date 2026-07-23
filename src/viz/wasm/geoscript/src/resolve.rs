//! Static name resolution: assigns frame slots to closure params/locals, computes capture
//! specs for free variables, and rewrites identifiers with their resolution so closure
//! invocation can use flat `Vec<Value>` frames instead of hashmap scope chains.
//!
//! Runs as the final phase of `optimize_ast` (idempotent — re-resolves from scratch each
//! time). Closures it can't handle (destructured params, destructure assignments in the
//! body, and anything containing such a closure) are left unresolved and evaluate via the
//! legacy scope-chain path. Semantics notes vs. the legacy path:
//! - captures snapshot referenced free vars at closure creation; mutations of enclosing scopes made
//!   *after* creation through shared parent links are no longer observed
//! - a free var unbound at creation falls back to legacy creation (same runtime behavior)

use std::rc::Rc;

use fxhash::{FxHashMap, FxHashSet};

use crate::{
  ast::{
    CaptureFrom, DestructurePattern, Expr, FunctionCallTarget, MapLiteralEntry, ResolvedBody,
    Statement, TopLevelStatement, VarRes,
  },
  builtins::{fn_defs::get_builtin_fn_sig_entry_ix, resolve_builtin_impl},
  Callable, Closure, EvalCtx, Program, Scope, Sym, Value,
};

#[derive(Default)]
struct ClosureCtx {
  self_name: Option<Sym>,
  /// innermost last; `[0]` is the function level (params + fn-scoped locals)
  blocks: Vec<FxHashMap<Sym, u16>>,
  next_slot: u32,
  captures: Vec<(Sym, CaptureFrom)>,
  slot_inits: Vec<(u16, u16)>,
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

struct Resolver<'a> {
  ctx: &'a EvalCtx,
  stack: Vec<ClosureCtx>,
  top_names: FxHashSet<Sym>,
}

impl Resolver<'_> {
  fn cur(&mut self) -> &mut ClosureCtx {
    self.stack.last_mut().unwrap()
  }

  fn resolve_read(&mut self, name: Sym) -> VarRes {
    if self.stack.is_empty() {
      return VarRes::Unresolved;
    }
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

  fn outer_bound(&self, name: Sym) -> bool {
    let cur = self.stack.len() - 1;
    (0..cur)
      .any(|lvl| self.stack[lvl].lookup(name).is_some() || self.stack[lvl].self_name == Some(name))
      || self.top_names.contains(&name)
  }

  /// Slot for an assignment target inside a closure; `None` at top level.
  fn resolve_assign(&mut self, name: Sym) -> Option<u16> {
    if self.stack.is_empty() {
      self.top_names.insert(name);
      return None;
    }
    let cur = self.stack.len() - 1;
    if let Some(slot) = self.stack[cur].lookup(name) {
      return Some(slot);
    }
    if self.stack[cur].blocks.len() == 1 {
      let slot = self.cur().alloc_slot();
      self.cur().blocks[0].insert(name, slot);
      return Some(slot);
    }
    // Inside a block. Names bound outside the closure mirror the legacy write-back: one
    // function-level slot initialized from the capture at frame entry. Unknown names are
    // block-local (die at block exit).
    if self.stack[cur].self_name == Some(name) {
      // block-assigning the closure's own binding name: rare and awkward to init; bail
      self.cur().bail = true;
      return None;
    }
    if self.outer_bound(name) {
      let VarRes::Capture(cap_ix) = self.resolve_read(name) else {
        self.cur().bail = true;
        return None;
      };
      let slot = self.cur().alloc_slot();
      self.cur().blocks[0].insert(name, slot);
      self.cur().slot_inits.push((slot, cap_ix));
      Some(slot)
    } else {
      let slot = self.cur().alloc_slot();
      self.cur().blocks.last_mut().unwrap().insert(name, slot);
      Some(slot)
    }
  }

  fn walk_top_statement(&mut self, stmt: &mut TopLevelStatement) {
    match stmt {
      TopLevelStatement::Statement(s) => self.walk_statement(s),
      TopLevelStatement::Export { name, expr, .. } => {
        self.walk_expr(expr, Some(*name));
        self.top_names.insert(*name);
      }
      TopLevelStatement::Import { bindings, .. } => {
        bindings.visit_idents(&mut |s| {
          self.top_names.insert(s);
        });
      }
    }
  }

  fn walk_statement(&mut self, stmt: &mut Statement) {
    match stmt {
      Statement::Assignment {
        name, expr, slot, ..
      } => {
        self.walk_expr(expr, Some(*name));
        *slot = self.resolve_assign(*name);
      }
      Statement::DestructureAssignment { lhs, rhs } => {
        self.walk_expr(rhs, None);
        if self.stack.is_empty() {
          lhs.visit_idents(&mut |s| {
            self.top_names.insert(s);
          });
        } else {
          self.cur().bail = true;
        }
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
        match &mut call.target {
          FunctionCallTarget::Name(name) => call.target_res = self.resolve_read(*name),
          FunctionCallTarget::Literal(callable) => self.maybe_resolve_literal_callable(callable),
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
      Expr::Literal { value, .. } => {
        if let Value::Callable(callable) = value {
          self.maybe_resolve_literal_callable(callable);
        }
      }
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
        if self.stack.is_empty() {
          for s in statements {
            self.walk_statement(s);
          }
        } else {
          self.cur().blocks.push(FxHashMap::default());
          for s in statements {
            self.walk_statement(s);
          }
          self.cur().blocks.pop();
        }
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

    let all_simple = params
      .iter()
      .all(|p| matches!(p.ident, DestructurePattern::Ident(_)));

    self.stack.push(ClosureCtx::new(binding_name));
    // defaults evaluate against the captured env (params not visible), so walk them before
    // binding params; their free names become captures of this closure
    for p in Rc::make_mut(params).iter_mut() {
      if let Some(d) = &mut p.default_val {
        self.walk_expr(d, None);
      }
    }
    if all_simple {
      for p in params.iter() {
        let DestructurePattern::Ident(name) = p.ident else {
          unreachable!()
        };
        let slot = self.cur().alloc_slot();
        self.cur().blocks[0].insert(name, slot);
      }
    } else {
      self.cur().bail = true;
    }
    for stmt in &mut Rc::make_mut(body).0 {
      self.walk_statement(stmt);
    }

    let cctx = self.stack.pop().unwrap();
    if cctx.bail {
      *resolved = None;
      // an unresolved closure can't be created from a frame, so the enclosing closure (if
      // any) must fall back to the legacy path too
      if let Some(parent) = self.stack.last_mut() {
        parent.bail = true;
      }
      return;
    }
    *resolved = Some(Rc::new(ResolvedBody {
      n_slots: cctx.next_slot as u16,
      captures: cctx.captures,
      slot_inits: cctx.slot_inits,
    }));
  }

  fn maybe_resolve_literal_callable(&mut self, callable: &mut Rc<Callable>) {
    let Callable::Closure(closure) = &**callable else {
      return;
    };
    if closure.resolved.is_some() {
      return;
    }
    let mut new_closure = closure.clone();
    if resolve_existing_closure(self.ctx, &mut new_closure) {
      *callable = Rc::new(Callable::Closure(new_closure));
    }
  }
}

/// Looks up a free name in `scope` at closure creation, with the same builtin fallback as
/// `eval_ident`.
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

/// Resolves a standalone `Closure` value (optimizer-folded literal or synthesized, e.g. by
/// autodiff): slots the body and materializes captures from the closure's captured scope
/// immediately. Returns false (leaving the closure on the legacy path) if anything is
/// unresolvable.
pub(crate) fn resolve_existing_closure(ctx: &EvalCtx, closure: &mut Closure) -> bool {
  let all_simple = closure
    .params
    .iter()
    .all(|p| matches!(p.ident, DestructurePattern::Ident(_)));
  if !all_simple {
    return false;
  }

  let mut r = Resolver {
    ctx,
    stack: Vec::new(),
    top_names: FxHashSet::default(),
  };
  let mut params = Rc::clone(&closure.params);
  let mut body = Rc::clone(&closure.body);
  r.stack.push(ClosureCtx::new(None));
  for p in Rc::make_mut(&mut params).iter_mut() {
    if let Some(d) = &mut p.default_val {
      r.walk_expr(d, None);
    }
  }
  for p in params.iter() {
    let DestructurePattern::Ident(name) = p.ident else {
      unreachable!()
    };
    let slot = r.cur().alloc_slot();
    r.cur().blocks[0].insert(name, slot);
  }
  for stmt in &mut Rc::make_mut(&mut body).0 {
    r.walk_statement(stmt);
  }
  let cctx = r.stack.pop().unwrap();
  if cctx.bail {
    return false;
  }
  let meta = ResolvedBody {
    n_slots: cctx.next_slot as u16,
    captures: cctx.captures,
    slot_inits: cctx.slot_inits,
  };

  let Some(scope) = closure.captured_scope.upgrade() else {
    return false;
  };
  let mut cap_vals = Vec::with_capacity(meta.captures.len());
  for (name, from) in &meta.captures {
    let CaptureFrom::DefScope(sym) = from else {
      return false;
    };
    debug_assert_eq!(sym, name);
    let Some(v) = resolve_capture_by_name(ctx, &scope, *sym) else {
      return false;
    };
    cap_vals.push(v);
  }

  closure.params = params;
  closure.body = body;
  closure.resolved = Some(Rc::new(meta));
  closure.captures = Rc::from(cap_vals);
  true
}

/// Final phase of `optimize_ast`: resolve every closure in the program. Idempotent.
pub(crate) fn resolve_program(ctx: &EvalCtx, program: &mut Program) {
  let mut r = Resolver {
    ctx,
    stack: Vec::new(),
    top_names: FxHashSet::default(),
  };
  for stmt in &mut program.statements {
    r.walk_top_statement(stmt);
  }
}
