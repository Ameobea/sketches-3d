use std::{borrow::Borrow, cmp::Reverse, ptr::addr_of, rc::Rc, str::FromStr};

use fxhash::FxHashMap;
use pest::iterators::Pair;

use crate::{
  builtins::{
    add_impl, and_impl, bit_and_impl, bit_or_impl, div_impl, eq_impl,
    fn_defs::{fn_sigs, get_builtin_fn_sig_entry_ix, FnSignature},
    map_impl, mod_impl, mul_impl, neg_impl, neq_impl, not_impl, numeric_bool_op_impl, or_impl,
    pos_impl, sub_impl, BoolOp,
  },
  get_args, get_binop_def_ix, get_unop_def_ix, get_unop_return_ty, ArgType, Callable, Closure,
  ErrorStack, EvalCtx, GetArgsOutput, IntRange, PreResolvedSignature, Rule, Sym, Value,
  EMPTY_KWARGS, FUNCTION_ALIASES, PRATT_PARSER,
};

/// Source location index. Points into a SourceMap to retrieve (line, col).
/// A value of 0 indicates an unknown or non-existent location.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SourceLoc(pub u32);

#[derive(Debug)]
pub struct Program {
  pub statements: Vec<Statement>,
}

// TODO: probably should de-dupe this with `ArgType`
#[derive(Debug, Clone, Copy)]
pub enum TypeName {
  Mesh,
  Int,
  Float,
  Num,
  Vec2,
  Vec3,
  Bool,
  String,
  Map,
  Seq,
  Callable,
  Nil,
  Material,
  Light,
}

impl Into<ArgType> for TypeName {
  fn into(self) -> ArgType {
    match self {
      TypeName::Mesh => ArgType::Mesh,
      TypeName::Int => ArgType::Int,
      TypeName::Float => ArgType::Float,
      TypeName::Num => ArgType::Numeric,
      TypeName::Vec2 => ArgType::Vec2,
      TypeName::Vec3 => ArgType::Vec3,
      TypeName::Bool => ArgType::Bool,
      TypeName::String => ArgType::String,
      TypeName::Map => ArgType::Map,
      TypeName::Seq => ArgType::Sequence,
      TypeName::Callable => ArgType::Callable,
      TypeName::Nil => ArgType::Nil,
      TypeName::Material => ArgType::Material,
      TypeName::Light => ArgType::Light,
    }
  }
}

impl Into<Option<TypeName>> for ArgType {
  fn into(self) -> Option<TypeName> {
    match self {
      ArgType::Mesh => Some(TypeName::Mesh),
      ArgType::Int => Some(TypeName::Int),
      ArgType::Float => Some(TypeName::Float),
      ArgType::Numeric => Some(TypeName::Num),
      ArgType::Vec2 => Some(TypeName::Vec2),
      ArgType::Vec3 => Some(TypeName::Vec3),
      ArgType::Bool => Some(TypeName::Bool),
      ArgType::String => Some(TypeName::String),
      ArgType::Map => Some(TypeName::Map),
      ArgType::Sequence => Some(TypeName::Seq),
      ArgType::Callable => Some(TypeName::Callable),
      ArgType::Nil => Some(TypeName::Nil),
      ArgType::Material => Some(TypeName::Material),
      ArgType::Light => Some(TypeName::Light),
      ArgType::Any => None,
    }
  }
}

impl FromStr for TypeName {
  type Err = String;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s {
      "mesh" => Ok(TypeName::Mesh),
      "int" => Ok(TypeName::Int),
      "float" => Ok(TypeName::Float),
      "num" => Ok(TypeName::Num),
      "vec2" => Ok(TypeName::Vec2),
      "vec3" => Ok(TypeName::Vec3),
      "bool" => Ok(TypeName::Bool),
      "str" | "string" => Ok(TypeName::String),
      "map" => Ok(TypeName::Map),
      "seq" | "sequence" => Ok(TypeName::Seq),
      "fn" | "callable" => Ok(TypeName::Callable),
      "nil" => Ok(TypeName::Nil),
      "mat" => Ok(TypeName::Material),
      "light" => Ok(TypeName::Light),
      _ => Err(format!("Unknown type name: {s}")),
    }
  }
}

impl TypeName {
  pub fn validate_val(&self, val: &Value) -> Result<(), ErrorStack> {
    match (self, val) {
      (TypeName::Mesh, Value::Mesh(_)) => Ok(()),
      (TypeName::Int, Value::Int(_)) => Ok(()),
      (TypeName::Float, Value::Float(_)) => Ok(()),
      (TypeName::Num, Value::Int(_) | Value::Float(_)) => Ok(()),
      (TypeName::Vec2, Value::Vec2(_)) => Ok(()),
      (TypeName::Vec3, Value::Vec3(_)) => Ok(()),
      (TypeName::Bool, Value::Bool(_)) => Ok(()),
      (TypeName::Seq, Value::Sequence(_)) => Ok(()),
      (TypeName::Callable, Value::Callable(_)) => Ok(()),
      (TypeName::Map, Value::Map(_)) => Ok(()),
      (TypeName::String, Value::String(_)) => Ok(()),
      (TypeName::Nil, Value::Nil) => Ok(()),
      (TypeName::Material, Value::Material(_)) => Ok(()),
      (TypeName::Light, Value::Light(_)) => Ok(()),
      _ => Err(ErrorStack::new(format!(
        "Value {val:?} does not match type {self:?}"
      ))),
    }
  }
}

#[derive(Clone, Debug)]
pub enum DestructurePattern {
  Ident(Sym),
  Array(Vec<DestructurePattern>),
  Map(FxHashMap<Sym, DestructurePattern>),
}

impl DestructurePattern {
  #[cold]
  pub fn debug(&self, ctx: &EvalCtx) -> String {
    match self {
      DestructurePattern::Ident(id) => ctx.with_resolved_sym(*id, |name| name.to_string()),
      DestructurePattern::Array(items) => {
        let item_strs: Vec<String> = items.iter().map(|item| item.debug(ctx)).collect();
        format!("[{}]", item_strs.join(", "))
      }
      DestructurePattern::Map(hm) => {
        let item_strs: Vec<String> = hm
          .iter()
          .map(|(k, v)| ctx.with_resolved_sym(*k, |key| format!("{key}: {}", v.debug(ctx))))
          .collect();
        format!("{{{}}}", item_strs.join(", "))
      }
    }
  }

  pub fn iter_idents<'a>(&'a self) -> Box<dyn Iterator<Item = Sym> + 'a> {
    match self {
      DestructurePattern::Ident(id) => Box::new(std::iter::once(*id)),
      DestructurePattern::Array(items) => {
        Box::new(items.iter().flat_map(DestructurePattern::iter_idents))
      }
      DestructurePattern::Map(hm) => {
        Box::new(hm.values().flat_map(DestructurePattern::iter_idents))
      }
    }
  }

  pub fn visit_idents(&self, cb: &mut impl FnMut(Sym)) {
    match self {
      DestructurePattern::Ident(ident) => {
        cb(*ident);
      }
      DestructurePattern::Array(destructure_patterns) => {
        for pat in destructure_patterns {
          pat.visit_idents(cb);
        }
      }
      DestructurePattern::Map(pat) => {
        for v in pat.values() {
          v.visit_idents(cb);
        }
      }
    }
  }

  pub fn visit_assignments(
    &self,
    ctx: &EvalCtx,
    rhs: Value,
    cb: &mut impl FnMut(Sym, Value) -> Result<(), ErrorStack>,
  ) -> Result<(), ErrorStack> {
    match self {
      DestructurePattern::Ident(ident) => {
        cb(*ident, rhs)?;
        Ok(())
      }
      DestructurePattern::Array(destructure_patterns) => {
        let Value::Sequence(seq) = rhs else {
          return Err(ErrorStack::new(format!(
            "Cannot destructure non-sequence value {rhs:?} with array pattern {:?}",
            self.debug(ctx)
          )));
        };

        let mut seq = seq.consume(ctx);
        for pat in destructure_patterns {
          let val = match seq.next() {
            Some(res) => res?,
            None => Value::Nil,
          };
          pat.visit_assignments(ctx, val, cb)?;
        }
        Ok(())
      }
      DestructurePattern::Map(pat) => {
        let Value::Map(map) = rhs else {
          return Err(ErrorStack::new(format!(
            "Cannot destructure non-map value {rhs:?} with map pattern {:?}",
            self.debug(ctx)
          )));
        };

        for (k, v) in pat {
          let rhs = ctx.with_resolved_sym(*k, |k_resolved| {
            map.get(k_resolved).cloned().unwrap_or(Value::Nil)
          });
          v.visit_assignments(ctx, rhs, cb)?;
        }
        Ok(())
      }
    }
  }
}

#[derive(Clone, Debug)]
pub enum Statement {
  Assignment {
    name: Sym,
    expr: Expr,
    type_hint: Option<TypeName>,
  },
  DestructureAssignment {
    lhs: DestructurePattern,
    rhs: Expr,
  },
  Expr(Expr),
  Return {
    value: Option<Expr>,
  },
  Break {
    value: Option<Expr>,
  },
}

impl Statement {
  pub(crate) fn analyze_const_captures(
    &self,
    ctx: &EvalCtx,
    closure_scope: &mut ScopeTracker,
    allow_rng_const_eval: bool,
    propagate_closure_captures: bool,
    constify_assignments: bool,
  ) -> bool {
    match self {
      Statement::Assignment { expr, .. } => expr.analyze_const_captures(
        ctx,
        closure_scope,
        allow_rng_const_eval,
        propagate_closure_captures,
        constify_assignments,
      ),
      Statement::DestructureAssignment { lhs: _, rhs } => rhs.analyze_const_captures(
        ctx,
        closure_scope,
        allow_rng_const_eval,
        propagate_closure_captures,
        constify_assignments,
      ),
      Statement::Expr(expr) => expr.analyze_const_captures(
        ctx,
        closure_scope,
        allow_rng_const_eval,
        propagate_closure_captures,
        constify_assignments,
      ),
      Statement::Return { value } => {
        if let Some(expr) = value {
          expr.analyze_const_captures(
            ctx,
            closure_scope,
            allow_rng_const_eval,
            propagate_closure_captures,
            constify_assignments,
          )
        } else {
          false
        }
      }
      Statement::Break { value } => {
        if let Some(expr) = value {
          expr.analyze_const_captures(
            ctx,
            closure_scope,
            allow_rng_const_eval,
            propagate_closure_captures,
            constify_assignments,
          )
        } else {
          false
        }
      }
    }
  }

  pub(crate) fn inline_const_captures(&mut self, ctx: &EvalCtx, closure_scope: &mut ScopeTracker) {
    match self {
      Statement::Assignment { expr, .. } => {
        expr.inline_const_captures(ctx, closure_scope);
      }
      Statement::DestructureAssignment { lhs: _, rhs } => {
        rhs.inline_const_captures(ctx, closure_scope);
      }
      Statement::Expr(expr) => {
        expr.inline_const_captures(ctx, closure_scope);
      }
      Statement::Return { value } => {
        if let Some(expr) = value {
          expr.inline_const_captures(ctx, closure_scope);
        }
      }
      Statement::Break { value } => {
        if let Some(expr) = value {
          expr.inline_const_captures(ctx, closure_scope);
        }
      }
    }
  }

  /// Iterates over all expressions directly contained in this statement (not recursively).
  pub fn exprs(&self) -> impl Iterator<Item = &Expr> {
    let (first, second) = match self {
      Statement::Assignment { expr, .. } => (Some(expr), None),
      Statement::DestructureAssignment { lhs: _, rhs } => (Some(rhs), None),
      Statement::Expr(expr) => (Some(expr), None),
      Statement::Return { value } => (value.as_ref(), None),
      Statement::Break { value } => (value.as_ref(), None),
    };
    first.into_iter().chain(second)
  }

  /// Iterates over all expressions directly contained in this statement (not recursively), mutably.
  pub fn exprs_mut(&mut self) -> impl Iterator<Item = &mut Expr> {
    let (first, second) = match self {
      Statement::Assignment { expr, .. } => (Some(expr), None),
      Statement::DestructureAssignment { lhs: _, rhs } => (Some(rhs), None),
      Statement::Expr(expr) => (Some(expr), None),
      Statement::Return { value } => (value.as_mut(), None),
      Statement::Break { value } => (value.as_mut(), None),
    };
    first.into_iter().chain(second)
  }

  /// Traverses all expressions in this statement recursively, calling `cb` on each.
  pub fn traverse_exprs(&self, cb: &mut impl FnMut(&Expr)) {
    for expr in self.exprs() {
      expr.traverse(cb);
    }
  }

  /// Traverses all expressions in this statement recursively and mutably, calling `cb` on each.
  pub fn traverse_exprs_mut(&mut self, cb: &mut impl FnMut(&mut Expr)) {
    for expr in self.exprs_mut() {
      expr.traverse_mut(cb);
    }
  }
}

#[derive(Clone, Debug)]
pub struct ClosureArg {
  pub ident: DestructurePattern,
  pub type_hint: Option<TypeName>,
  pub default_val: Option<Expr>,
}

#[derive(Clone, Debug)]
pub enum MapLiteralEntry {
  KeyValue { key: String, value: Expr },
  Splat { expr: Expr },
}

impl MapLiteralEntry {
  fn analyze_const_captures(
    &self,
    ctx: &EvalCtx,
    local_scope: &mut ScopeTracker<'_>,
    allow_rng_const_eval: bool,
    propagate_closure_captures: bool,
    constify_assignments: bool,
  ) -> bool {
    match self {
      MapLiteralEntry::KeyValue { key: _, value } => value.analyze_const_captures(
        ctx,
        local_scope,
        allow_rng_const_eval,
        propagate_closure_captures,
        constify_assignments,
      ),
      MapLiteralEntry::Splat { expr } => expr.analyze_const_captures(
        ctx,
        local_scope,
        allow_rng_const_eval,
        propagate_closure_captures,
        constify_assignments,
      ),
    }
  }

  fn inline_const_captures(&mut self, ctx: &EvalCtx, local_scope: &mut ScopeTracker<'_>) {
    match self {
      MapLiteralEntry::KeyValue { key: _, value } => {
        value.inline_const_captures(ctx, local_scope);
      }
      MapLiteralEntry::Splat { expr } => {
        expr.inline_const_captures(ctx, local_scope);
      }
    }
  }

  fn expr(&self) -> &Expr {
    match self {
      MapLiteralEntry::KeyValue { key: _, value } => value,
      MapLiteralEntry::Splat { expr } => expr,
    }
  }

  pub fn is_literal(&self) -> bool {
    self.expr().is_literal()
  }
}

#[derive(Clone, Debug)]
pub enum Expr {
  BinOp {
    op: BinOp,
    lhs: Box<Expr>,
    rhs: Box<Expr>,
    pre_resolved_def_ix: Option<usize>,
    loc: SourceLoc,
  },
  PrefixOp {
    op: PrefixOp,
    expr: Box<Expr>,
    loc: SourceLoc,
  },
  Range {
    start: Box<Expr>,
    end: Option<Box<Expr>>,
    inclusive: bool,
    loc: SourceLoc,
  },
  StaticFieldAccess {
    lhs: Box<Expr>,
    field: String,
    loc: SourceLoc,
  },
  FieldAccess {
    lhs: Box<Expr>,
    field: Box<Expr>,
    loc: SourceLoc,
  },
  Call {
    call: FunctionCall,
    loc: SourceLoc,
  },
  Closure {
    params: Rc<Vec<ClosureArg>>,
    body: Rc<ClosureBody>,
    arg_placeholder_scope: FxHashMap<Sym, Value>,
    return_type_hint: Option<TypeName>,
    loc: SourceLoc,
  },
  Ident {
    name: Sym,
    loc: SourceLoc,
  },
  ArrayLiteral {
    elements: Vec<Expr>,
    loc: SourceLoc,
  },
  MapLiteral {
    entries: Vec<MapLiteralEntry>,
    loc: SourceLoc,
  },
  Literal {
    value: Value,
    loc: SourceLoc,
  },
  Conditional {
    cond: Box<Expr>,
    then: Box<Expr>,
    /// (cond, expr)
    else_if_exprs: Vec<(Expr, Expr)>,
    else_expr: Option<Box<Expr>>,
    loc: SourceLoc,
  },
  Block {
    statements: Vec<Statement>,
    loc: SourceLoc,
  },
}

impl Expr {
  pub fn loc(&self) -> SourceLoc {
    match self {
      Expr::BinOp { loc, .. }
      | Expr::PrefixOp { loc, .. }
      | Expr::Range { loc, .. }
      | Expr::StaticFieldAccess { loc, .. }
      | Expr::FieldAccess { loc, .. }
      | Expr::Call { loc, .. }
      | Expr::Closure { loc, .. }
      | Expr::Ident { loc, .. }
      | Expr::ArrayLiteral { loc, .. }
      | Expr::MapLiteral { loc, .. }
      | Expr::Literal { loc, .. }
      | Expr::Conditional { loc, .. }
      | Expr::Block { loc, .. } => *loc,
    }
  }
}

fn callable_is_dyn_for_const_eval(callable: &Callable, allow_rng_const_eval: bool) -> bool {
  if callable.is_side_effectful() {
    return !(allow_rng_const_eval && callable.is_rng_dependent());
  }
  false
}

impl Expr {
  pub fn as_literal(&self) -> Option<&Value> {
    match self {
      Expr::Literal { value, .. } => Some(value),
      _ => None,
    }
  }

  pub fn is_literal(&self) -> bool {
    matches!(self, Expr::Literal { .. })
  }

  fn analyze_const_captures(
    &self,
    ctx: &EvalCtx,
    local_scope: &mut ScopeTracker,
    allow_rng_const_eval: bool,
    propagate_closure_captures: bool,
    constify_assignments: bool,
  ) -> bool {
    match self {
      Expr::BinOp { lhs, rhs, .. } => {
        let mut captures_dyn = lhs.analyze_const_captures(
          ctx,
          local_scope,
          allow_rng_const_eval,
          propagate_closure_captures,
          constify_assignments,
        );
        captures_dyn |= rhs.analyze_const_captures(
          ctx,
          local_scope,
          allow_rng_const_eval,
          propagate_closure_captures,
          constify_assignments,
        );
        captures_dyn
      }
      Expr::PrefixOp { expr, .. } => expr.analyze_const_captures(
        ctx,
        local_scope,
        allow_rng_const_eval,
        propagate_closure_captures,
        constify_assignments,
      ),
      Expr::Range { start, end, .. } => {
        let mut captures_dyn = start.analyze_const_captures(
          ctx,
          local_scope,
          allow_rng_const_eval,
          propagate_closure_captures,
          constify_assignments,
        );
        if let Some(end) = end {
          captures_dyn |= end.analyze_const_captures(
            ctx,
            local_scope,
            allow_rng_const_eval,
            propagate_closure_captures,
            constify_assignments,
          );
        }
        captures_dyn
      }
      Expr::StaticFieldAccess { lhs, .. } => lhs.analyze_const_captures(
        ctx,
        local_scope,
        allow_rng_const_eval,
        propagate_closure_captures,
        constify_assignments,
      ),
      Expr::FieldAccess { lhs, field, .. } => {
        let mut captures_dyn = lhs.analyze_const_captures(
          ctx,
          local_scope,
          allow_rng_const_eval,
          propagate_closure_captures,
          constify_assignments,
        );
        captures_dyn |= field.analyze_const_captures(
          ctx,
          local_scope,
          allow_rng_const_eval,
          propagate_closure_captures,
          constify_assignments,
        );
        captures_dyn
      }
      Expr::Call {
        call: FunctionCall {
          target,
          args,
          kwargs,
        },
        ..
      } => {
        let mut captures_dyn = false;
        for arg in args.iter() {
          captures_dyn |= arg.analyze_const_captures(
            ctx,
            local_scope,
            allow_rng_const_eval,
            propagate_closure_captures,
            constify_assignments,
          );
        }
        for kwarg in kwargs.values() {
          captures_dyn |= kwarg.analyze_const_captures(
            ctx,
            local_scope,
            allow_rng_const_eval,
            propagate_closure_captures,
            constify_assignments,
          );
        }

        if captures_dyn {
          return true;
        }

        let name = match target {
          FunctionCallTarget::Name(name) => name,
          FunctionCallTarget::Literal(callable) => {
            return callable_is_dyn_for_const_eval(callable, allow_rng_const_eval)
          }
        };

        if let Some(TrackedValueRef::Const(val)) = local_scope.get(*name) {
          match val {
            Value::Callable(callable) => {
              callable_is_dyn_for_const_eval(callable, allow_rng_const_eval)
            }
            _ => false,
          }
        } else {
          ctx.with_resolved_sym(*name, |resolved_name| {
            if fn_sigs().contains_key(resolved_name) || FUNCTION_ALIASES.contains_key(resolved_name)
            {
              let builtin_name = if let Some(alias_target) = FUNCTION_ALIASES.get(resolved_name) {
                alias_target
              } else {
                resolved_name
              };
              let fn_entry_ix = get_builtin_fn_sig_entry_ix(builtin_name).unwrap();
              let callable = Callable::Builtin {
                fn_entry_ix,
                fn_impl: |_, _, _, _, _| unreachable!(),
                pre_resolved_signature: None,
              };
              callable_is_dyn_for_const_eval(&callable, allow_rng_const_eval)
            } else {
              true
            }
          })
        }
      }
      Expr::Closure { params, body, .. } => {
        let mut captures_dyn = false;

        for param in params.iter() {
          if let Some(default_val) = &param.default_val {
            captures_dyn |= default_val.analyze_const_captures(
              ctx,
              local_scope,
              allow_rng_const_eval,
              propagate_closure_captures,
              constify_assignments,
            );
          }
        }

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

        let body_captures_dyn = body.analyze_const_captures(
          ctx,
          &mut closure_scope,
          allow_rng_const_eval,
          propagate_closure_captures,
          constify_assignments,
        );

        if propagate_closure_captures {
          captures_dyn || body_captures_dyn
        } else {
          captures_dyn
        }
      }
      Expr::Ident { name: id, .. } => match local_scope.vars.get(id) {
        Some(TrackedValue::Const(_)) => false,
        Some(TrackedValue::Arg(_)) => false,
        Some(TrackedValue::Dyn { .. }) => true,
        None => match local_scope.parent {
          Some(parent) => match parent.get(*id) {
            Some(TrackedValueRef::Const(_)) => false,
            Some(TrackedValueRef::Arg(_)) => true,
            Some(TrackedValueRef::Dyn { .. }) => true,
            None => true,
          },
          None => true,
        },
      },
      Expr::ArrayLiteral {
        elements: exprs, ..
      } => exprs.iter().any(|expr| {
        expr.analyze_const_captures(
          ctx,
          local_scope,
          allow_rng_const_eval,
          propagate_closure_captures,
          constify_assignments,
        )
      }),
      Expr::MapLiteral { entries, .. } => entries.iter().any(|entry| {
        entry.analyze_const_captures(
          ctx,
          local_scope,
          allow_rng_const_eval,
          propagate_closure_captures,
          constify_assignments,
        )
      }),
      Expr::Literal { .. } => false,
      Expr::Conditional {
        cond,
        then,
        else_if_exprs,
        else_expr,
        ..
      } => {
        let mut captures_dyn = cond.analyze_const_captures(
          ctx,
          local_scope,
          allow_rng_const_eval,
          propagate_closure_captures,
          constify_assignments,
        );
        captures_dyn |= then.analyze_const_captures(
          ctx,
          local_scope,
          allow_rng_const_eval,
          propagate_closure_captures,
          constify_assignments,
        );
        for (cond, expr) in else_if_exprs {
          captures_dyn |= cond.analyze_const_captures(
            ctx,
            local_scope,
            allow_rng_const_eval,
            propagate_closure_captures,
            constify_assignments,
          );
          captures_dyn |= expr.analyze_const_captures(
            ctx,
            local_scope,
            allow_rng_const_eval,
            propagate_closure_captures,
            constify_assignments,
          );
        }
        if let Some(else_expr) = else_expr {
          captures_dyn |= else_expr.analyze_const_captures(
            ctx,
            local_scope,
            allow_rng_const_eval,
            propagate_closure_captures,
            constify_assignments,
          );
        }
        captures_dyn
      }
      Expr::Block { statements, .. } => statements.iter().any(|stmt| {
        stmt.analyze_const_captures(
          ctx,
          local_scope,
          allow_rng_const_eval,
          propagate_closure_captures,
          constify_assignments,
        )
      }),
    }
  }

  fn inline_const_captures(&mut self, ctx: &EvalCtx, local_scope: &mut ScopeTracker) {
    match self {
      Expr::BinOp { lhs, rhs, .. } => {
        lhs.inline_const_captures(ctx, local_scope);
        rhs.inline_const_captures(ctx, local_scope);
      }
      Expr::PrefixOp { expr, .. } => {
        expr.inline_const_captures(ctx, local_scope);
      }
      Expr::Range { start, end, .. } => {
        start.inline_const_captures(ctx, local_scope);
        if let Some(end) = end {
          end.inline_const_captures(ctx, local_scope);
        }
      }
      Expr::StaticFieldAccess { lhs, .. } => {
        lhs.inline_const_captures(ctx, local_scope);
      }
      Expr::FieldAccess { lhs, field, .. } => {
        lhs.inline_const_captures(ctx, local_scope);
        field.inline_const_captures(ctx, local_scope);
      }
      Expr::Call {
        call: FunctionCall {
          target,
          args,
          kwargs,
        },
        ..
      } => {
        for arg in args.iter_mut() {
          arg.inline_const_captures(ctx, local_scope);
        }
        for kwarg in kwargs.values_mut() {
          kwarg.inline_const_captures(ctx, local_scope);
        }

        let FunctionCallTarget::Name(name) = target else {
          return;
        };

        if let Some(TrackedValueRef::Const(val)) = local_scope.get(*name) {
          if let Value::Callable(callable) = val {
            *target = FunctionCallTarget::Literal(callable.clone());
          }
        }
      }
      Expr::Closure { params, body, .. } => {
        let mut params_inner: Vec<_> = (**params).clone();
        for param in params_inner.iter_mut() {
          if let Some(default_val) = &mut param.default_val {
            default_val.inline_const_captures(ctx, local_scope);
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

        let mut body_inner = (**body).clone();
        body_inner.inline_const_captures(ctx, &mut closure_scope);
        *body = Rc::new(body_inner);
      }
      Expr::Ident { name: id, loc } => match local_scope.vars.get(id) {
        Some(TrackedValue::Const(resolved)) => {
          *self = Expr::Literal {
            value: resolved.clone(),
            loc: *loc,
          };
        }
        Some(TrackedValue::Arg(_)) => {}
        Some(TrackedValue::Dyn { .. }) => {}
        None => match local_scope.parent {
          Some(parent) => match parent.get(*id) {
            Some(TrackedValueRef::Const(resolved)) => {
              *self = Expr::Literal {
                value: resolved.clone(),
                loc: *loc,
              };
            }
            Some(TrackedValueRef::Arg(_)) => {}
            Some(TrackedValueRef::Dyn { .. }) => {}
            None => {}
          },
          None => {}
        },
      },
      Expr::ArrayLiteral {
        elements: exprs, ..
      } => {
        for expr in exprs.iter_mut() {
          expr.inline_const_captures(ctx, local_scope);
        }
      }
      Expr::MapLiteral { entries, .. } => {
        for entry in entries.iter_mut() {
          entry.inline_const_captures(ctx, local_scope);
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
        cond.inline_const_captures(ctx, local_scope);
        then.inline_const_captures(ctx, local_scope);
        for (cond, expr) in else_if_exprs {
          cond.inline_const_captures(ctx, local_scope);
          expr.inline_const_captures(ctx, local_scope);
        }
        if let Some(else_expr) = else_expr {
          else_expr.inline_const_captures(ctx, local_scope);
        }
      }
      Expr::Block { statements, .. } => {
        for stmt in statements.iter_mut() {
          stmt.inline_const_captures(ctx, local_scope);
        }
      }
    }
  }

  pub fn traverse(&self, cb: &mut impl FnMut(&Self)) {
    match self {
      Expr::BinOp { lhs, rhs, .. } => {
        cb(self);
        lhs.traverse(cb);
        rhs.traverse(cb);
      }
      Expr::PrefixOp { expr, .. } => {
        cb(self);
        expr.traverse(cb);
      }
      Expr::Range { start, end, .. } => {
        cb(self);
        start.traverse(cb);
        if let Some(end) = end {
          end.traverse(cb);
        }
      }
      Expr::StaticFieldAccess { lhs, .. } => {
        cb(self);
        lhs.traverse(cb);
      }
      Expr::FieldAccess { lhs, field, .. } => {
        cb(self);
        lhs.traverse(cb);
        field.traverse(cb);
      }
      Expr::Call { call, .. } => {
        cb(self);
        call.args.iter().for_each(|arg| arg.traverse(cb));
        call.kwargs.values().for_each(|kwarg| kwarg.traverse(cb));
      }
      Expr::Closure { body, .. } => {
        cb(self);
        body.0.iter().for_each(|stmt| stmt.traverse_exprs(cb));
      }
      Expr::Ident { .. } | Expr::Literal { .. } => {
        cb(self);
      }
      Expr::ArrayLiteral { elements, .. } => {
        cb(self);
        for expr in elements {
          expr.traverse(cb);
        }
      }
      Expr::MapLiteral { entries, .. } => {
        cb(self);
        for entry in entries {
          match entry {
            MapLiteralEntry::KeyValue { key: _, value } => value.traverse(cb),
            MapLiteralEntry::Splat { expr } => expr.traverse(cb),
          }
        }
      }
      Expr::Conditional {
        cond,
        then,
        else_if_exprs,
        else_expr,
        ..
      } => {
        cb(self);
        cond.traverse(cb);
        then.traverse(cb);
        for (cond, expr) in else_if_exprs {
          cond.traverse(cb);
          expr.traverse(cb);
        }
        if let Some(else_expr) = else_expr {
          else_expr.traverse(cb);
        }
      }
      Expr::Block { statements, .. } => {
        cb(self);
        for stmt in statements {
          stmt.traverse_exprs(cb);
        }
      }
    }
  }

  fn traverse_mut(&mut self, cb: &mut impl FnMut(&mut Self)) {
    match self {
      Expr::BinOp { .. } => {
        cb(self);
        let Expr::BinOp { lhs, rhs, .. } = self else {
          return;
        };
        lhs.traverse_mut(cb);
        rhs.traverse_mut(cb);
      }
      Expr::PrefixOp { .. } => {
        cb(self);
        let Expr::PrefixOp { expr, .. } = self else {
          return;
        };
        expr.traverse_mut(cb);
      }
      Expr::Range { .. } => {
        cb(self);
        let Expr::Range { start, end, .. } = self else {
          return;
        };
        start.traverse_mut(cb);
        if let Some(end) = end {
          end.traverse_mut(cb);
        }
      }
      Expr::StaticFieldAccess { .. } => {
        cb(self);
        let Expr::StaticFieldAccess { lhs, .. } = self else {
          return;
        };
        lhs.traverse_mut(cb);
      }
      Expr::FieldAccess { .. } => {
        cb(self);
        let Expr::FieldAccess { lhs, field, .. } = self else {
          return;
        };
        lhs.traverse_mut(cb);
        field.traverse_mut(cb);
      }
      Expr::Call { .. } => {
        cb(self);
        let Expr::Call { call, .. } = self else {
          return;
        };
        call.args.iter_mut().for_each(|arg| arg.traverse_mut(cb));
        call
          .kwargs
          .values_mut()
          .for_each(|kwarg| kwarg.traverse_mut(cb));
      }
      Expr::Closure { .. } => {
        cb(self);
        let Expr::Closure { body, .. } = self else {
          return;
        };
        Rc::make_mut(body)
          .0
          .iter_mut()
          .for_each(|stmt| stmt.traverse_exprs_mut(cb));
      }
      Expr::Ident { .. } | Expr::Literal { .. } => {
        cb(self);
      }
      Expr::ArrayLiteral { .. } => {
        cb(self);
        let Expr::ArrayLiteral {
          elements: exprs, ..
        } = self
        else {
          return;
        };
        for expr in exprs.iter_mut() {
          expr.traverse_mut(cb);
        }
      }
      Expr::MapLiteral { .. } => {
        cb(self);
        let Expr::MapLiteral { entries, .. } = self else {
          return;
        };
        for entry in entries {
          match entry {
            MapLiteralEntry::KeyValue { key: _, value } => value.traverse_mut(cb),
            MapLiteralEntry::Splat { expr } => expr.traverse_mut(cb),
          }
        }
      }
      Expr::Conditional { .. } => {
        cb(self);
        let Expr::Conditional {
          cond,
          then,
          else_if_exprs,
          else_expr,
          ..
        } = self
        else {
          return;
        };
        cond.traverse_mut(cb);
        then.traverse_mut(cb);
        for (cond, expr) in else_if_exprs {
          cond.traverse_mut(cb);
          expr.traverse_mut(cb);
        }
        if let Some(else_expr) = else_expr {
          else_expr.traverse_mut(cb);
        }
      }
      Expr::Block { .. } => {
        cb(self);
        let Expr::Block { statements, .. } = self else {
          return;
        };
        for stmt in statements {
          stmt.traverse_exprs_mut(cb);
        }
      }
    }
  }
}

#[derive(Clone, Debug)]
pub struct ClosureBody(pub Vec<Statement>);

impl ClosureBody {
  /// Returns `true` if any of the statements in this closure body reference a variable not tracked
  /// in `closure_scope`
  pub(crate) fn analyze_const_captures(
    &self,
    ctx: &EvalCtx,
    closure_scope: &mut ScopeTracker,
    allow_rng_const_eval: bool,
    propagate_closure_captures: bool,
    constify_assignments: bool,
  ) -> bool {
    let mut references_dyn_captures = false;
    for stmt in &self.0 {
      if stmt.analyze_const_captures(
        ctx,
        closure_scope,
        allow_rng_const_eval,
        propagate_closure_captures,
        constify_assignments,
      ) {
        references_dyn_captures = true;
      }
      if let Statement::Assignment {
        name,
        expr,
        type_hint: _,
      } = stmt
      {
        if constify_assignments {
          closure_scope.set(*name, TrackedValue::Const(Value::Nil));
          continue;
        }
        // if this variable has already been de-constified in the scope, we avoid overwriting it
        let is_deconstified = match closure_scope.get(*name) {
          Some(TrackedValueRef::Arg(_) | TrackedValueRef::Dyn { .. }) => true,
          Some(TrackedValueRef::Const(_)) => false,
          None => false,
        };

        if !is_deconstified {
          let tracked_val = match expr.as_literal() {
            Some(literal) => TrackedValue::Const(literal.clone()),
            None => {
              let dyn_type = get_dyn_type(expr, closure_scope);
              match dyn_type {
                DynType::Arg => TrackedValue::Arg(ClosureArg {
                  ident: DestructurePattern::Ident(*name),
                  type_hint: None,
                  default_val: None,
                }),
                DynType::Const | DynType::Dyn => TrackedValue::Dyn {
                  type_hint: match pre_resolve_expr_type(ctx, closure_scope, expr) {
                    Some(ty) => ty.into(),
                    None => None,
                  },
                },
              }
            }
          };
          closure_scope.set(*name, tracked_val);
        }
      }
      // TODO: should de-dupe
      else if let Statement::DestructureAssignment { lhs, rhs } = stmt {
        for name in lhs.iter_idents() {
          if constify_assignments {
            closure_scope.set(name, TrackedValue::Const(Value::Nil));
            continue;
          }
          let is_deconstified = match closure_scope.get(name) {
            Some(TrackedValueRef::Arg(_) | TrackedValueRef::Dyn { .. }) => true,
            Some(TrackedValueRef::Const(_)) => false,
            None => false,
          };

          if !is_deconstified {
            let tracked_val = match rhs.as_literal() {
              Some(literal) => TrackedValue::Const(literal.clone()),
              None => {
                let dyn_type = get_dyn_type(rhs, closure_scope);
                match dyn_type {
                  DynType::Arg => TrackedValue::Arg(ClosureArg {
                    ident: DestructurePattern::Ident(name),
                    type_hint: None,
                    default_val: None,
                  }),
                  DynType::Const | DynType::Dyn => TrackedValue::Dyn {
                    type_hint: match pre_resolve_expr_type(ctx, closure_scope, rhs) {
                      Some(ty) => ty.into(),
                      None => None,
                    },
                  },
                }
              }
            };
            closure_scope.set(name, tracked_val);
          }
        }
      }
    }

    references_dyn_captures
  }

  pub(crate) fn inline_const_captures(&mut self, ctx: &EvalCtx, closure_scope: &mut ScopeTracker) {
    for stmt in &mut self.0 {
      stmt.inline_const_captures(ctx, closure_scope);

      if let Statement::Assignment {
        name,
        expr,
        type_hint: _,
      } = stmt
      {
        let is_deconstified = match closure_scope.get(*name) {
          Some(TrackedValueRef::Arg(_) | TrackedValueRef::Dyn { .. }) => true,
          Some(TrackedValueRef::Const(_)) => false,
          None => false,
        };

        if !is_deconstified {
          let tracked_val = match expr.as_literal() {
            Some(literal) => TrackedValue::Const(literal.clone()),
            None => {
              let dyn_type = get_dyn_type(expr, closure_scope);
              match dyn_type {
                DynType::Arg => TrackedValue::Arg(ClosureArg {
                  ident: DestructurePattern::Ident(*name),
                  type_hint: None,
                  default_val: None,
                }),
                DynType::Const | DynType::Dyn => TrackedValue::Dyn {
                  type_hint: match pre_resolve_expr_type(ctx, closure_scope, expr) {
                    Some(ty) => ty.into(),
                    None => None,
                  },
                },
              }
            }
          };
          closure_scope.set(*name, tracked_val);
        }
      } else if let Statement::DestructureAssignment { lhs, rhs } = stmt {
        for name in lhs.iter_idents() {
          let is_deconstified = match closure_scope.get(name) {
            Some(TrackedValueRef::Arg(_) | TrackedValueRef::Dyn { .. }) => true,
            Some(TrackedValueRef::Const(_)) => false,
            None => false,
          };

          if !is_deconstified {
            let tracked_val = match rhs.as_literal() {
              Some(literal) => TrackedValue::Const(literal.clone()),
              None => {
                let dyn_type = get_dyn_type(rhs, closure_scope);
                match dyn_type {
                  DynType::Arg => TrackedValue::Arg(ClosureArg {
                    ident: DestructurePattern::Ident(name),
                    type_hint: None,
                    default_val: None,
                  }),
                  DynType::Const | DynType::Dyn => TrackedValue::Dyn {
                    type_hint: match pre_resolve_expr_type(ctx, closure_scope, rhs) {
                      Some(ty) => ty.into(),
                      None => None,
                    },
                  },
                }
              }
            };
            closure_scope.set(name, tracked_val);
          }
        }
      }
    }
  }

  pub(crate) fn traverse_exprs_mut(&mut self, mut traverse: impl FnMut(&mut Expr)) {
    for stmt in &mut self.0 {
      for expr in stmt.exprs_mut() {
        traverse(expr);
      }
    }
  }
}

#[derive(Clone, Debug)]
pub enum FunctionCallTarget {
  Name(Sym),
  Literal(Rc<Callable>),
}

#[derive(Clone, Debug)]
pub struct FunctionCall {
  pub target: FunctionCallTarget,
  pub args: Vec<Expr>,
  pub kwargs: FxHashMap<Sym, Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
  Add,
  Sub,
  Mul,
  Div,
  Mod,
  Gt,
  Lt,
  Gte,
  Lte,
  Eq,
  Neq,
  And,
  Or,
  BitAnd,
  Range,
  RangeInclusive,
  Pipeline,
  Map,
}

pub(crate) fn eval_range(
  start: &Value,
  end: Option<&Value>,
  inclusive: bool,
) -> Result<Value, ErrorStack> {
  let Value::Int(start) = start else {
    return Err(ErrorStack::new(format!(
      "Range start must be an integer, found: {start:?}",
    )));
  };
  let end = match end {
    Some(end) => {
      let Value::Int(mut end) = end else {
        return Err(ErrorStack::new(format!(
          "Range end must be an integer, found: {end:?}",
        )));
      };
      if inclusive {
        end += 1;
      }
      Some(end)
    }
    None => None,
  };

  Ok(Value::Sequence(Rc::new(IntRange { start: *start, end })))
}

// it would be great if these could somehow be made const rather than static
static mut OP_DEF_SHORTHANDS_INITIALIZED: bool = false;

static mut BINOP_DEF_IX_TABLE: [(usize, bool); 18] = [(usize::MAX, false); 18];
static mut UNOP_DEF_IX_TABLE: [usize; 3] = [usize::MAX; 3];

pub(crate) fn maybe_init_op_def_shorthands() {
  unsafe {
    if OP_DEF_SHORTHANDS_INITIALIZED {
      return;
    }

    BINOP_DEF_IX_TABLE = [
      (get_builtin_fn_sig_entry_ix("add").unwrap(), false),
      (get_builtin_fn_sig_entry_ix("sub").unwrap(), false),
      (get_builtin_fn_sig_entry_ix("mul").unwrap(), false),
      (get_builtin_fn_sig_entry_ix("div").unwrap(), false),
      (get_builtin_fn_sig_entry_ix("mod").unwrap(), false),
      (get_builtin_fn_sig_entry_ix("gt").unwrap(), false),
      (get_builtin_fn_sig_entry_ix("lt").unwrap(), false),
      (get_builtin_fn_sig_entry_ix("gte").unwrap(), false),
      (get_builtin_fn_sig_entry_ix("lte").unwrap(), false),
      (get_builtin_fn_sig_entry_ix("eq").unwrap(), false),
      (get_builtin_fn_sig_entry_ix("neq").unwrap(), false),
      (get_builtin_fn_sig_entry_ix("and").unwrap(), false),
      (get_builtin_fn_sig_entry_ix("or").unwrap(), false),
      (get_builtin_fn_sig_entry_ix("bit_and").unwrap(), false),
      (0, false),                                              // Range
      (0, false),                                              // RangeInclusive
      (get_builtin_fn_sig_entry_ix("bit_or").unwrap(), false), // Pipeline
      (get_builtin_fn_sig_entry_ix("map").unwrap(), true),
    ];

    UNOP_DEF_IX_TABLE = [
      get_builtin_fn_sig_entry_ix("neg").unwrap(),
      get_builtin_fn_sig_entry_ix("pos").unwrap(),
      get_builtin_fn_sig_entry_ix("not").unwrap(),
    ];
  }
}

impl BinOp {
  pub fn apply(
    &self,
    ctx: &EvalCtx,
    lhs: &Value,
    rhs: &Value,
    pre_resolved_def_ix: Option<usize>,
  ) -> Result<Value, ErrorStack> {
    match *self {
      BinOp::Pipeline => {
        // eval as a pipeline operator if the rhs is a callable
        if let Some(callable) = rhs.as_callable() {
          return ctx
            .invoke_callable(callable, &[lhs.clone()], EMPTY_KWARGS)
            .map_err(|err| err.wrap("Error invoking callable in pipeline"));
        }
      }
      BinOp::Range => return eval_range(lhs, Some(rhs), false),
      BinOp::RangeInclusive => return eval_range(lhs, Some(rhs), true),
      _ => (),
    }

    unsafe {
      let def_ix = match pre_resolved_def_ix {
        Some(pre_resolved_def_ix) => pre_resolved_def_ix,
        None => {
          let (fn_sig_entry_ix, args_flipped) = (addr_of!(BINOP_DEF_IX_TABLE)
            as *const (usize, bool))
            .add(*self as usize)
            .read();
          let (arg1, arg2) = if args_flipped { (rhs, lhs) } else { (lhs, rhs) };
          get_binop_def_ix(ctx, fn_sig_entry_ix, arg1, arg2)?
        }
      };

      match self {
        BinOp::Add => add_impl(def_ix, lhs, rhs),
        BinOp::Sub => sub_impl(ctx, def_ix, lhs, rhs),
        BinOp::Mul => mul_impl(def_ix, lhs, rhs),
        BinOp::Div => div_impl(def_ix, lhs, rhs),
        BinOp::Mod => mod_impl(def_ix, lhs, rhs),
        BinOp::Gt => numeric_bool_op_impl::<{ BoolOp::Gt }>(def_ix, lhs, rhs),
        BinOp::Lt => numeric_bool_op_impl::<{ BoolOp::Lt }>(def_ix, lhs, rhs),
        BinOp::Gte => numeric_bool_op_impl::<{ BoolOp::Gte }>(def_ix, lhs, rhs),
        BinOp::Lte => numeric_bool_op_impl::<{ BoolOp::Lte }>(def_ix, lhs, rhs),
        BinOp::Eq => eq_impl(def_ix, lhs, rhs),
        BinOp::Neq => neq_impl(def_ix, lhs, rhs),
        BinOp::And => and_impl(def_ix, lhs, rhs),
        BinOp::Or => or_impl(def_ix, lhs, rhs),
        BinOp::BitAnd => bit_and_impl(ctx, def_ix, lhs, rhs),
        BinOp::Map => {
          // this operator acts the same as `lhs | map(rhs)`
          map_impl(ctx, def_ix, rhs, lhs)
        }
        // treating as bit-or
        BinOp::Pipeline => bit_or_impl(ctx, def_ix, lhs, rhs),
        BinOp::Range | BinOp::RangeInclusive => unreachable!("previously special-cased"),
      }
    }
  }

  fn get_builtin_fn_name(&self) -> Option<&'static str> {
    let name = match self {
      BinOp::Add => "add",
      BinOp::Sub => "sub",
      BinOp::Mul => "mul",
      BinOp::Div => "div",
      BinOp::Mod => "mod",
      BinOp::Gt => "gt",
      BinOp::Lt => "lt",
      BinOp::Gte => "gte",
      BinOp::Lte => "lte",
      BinOp::Eq => "eq",
      BinOp::Neq => "neq",
      BinOp::And => "and",
      BinOp::Or => "or",
      BinOp::BitAnd => "bit_and",
      BinOp::Range | BinOp::RangeInclusive | BinOp::Pipeline => {
        return None;
      }
      BinOp::Map => "map",
    };
    Some(name)
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixOp {
  Neg,
  Pos,
  Not,
}

impl PrefixOp {
  pub fn apply(&self, ctx: &EvalCtx, val: &Value) -> Result<Value, ErrorStack> {
    let fn_sig_entry_ix = unsafe {
      (addr_of!(UNOP_DEF_IX_TABLE) as *const usize)
        .add(*self as usize)
        .read()
    };
    let def_ix = get_unop_def_ix(ctx, fn_sig_entry_ix, val)?;

    match self {
      PrefixOp::Neg => neg_impl(def_ix, val),
      PrefixOp::Pos => pos_impl(def_ix, val),
      PrefixOp::Not => not_impl(def_ix, val),
    }
  }
}

fn parse_fn_call(ctx: &EvalCtx, func_call: Pair<Rule>) -> Result<Expr, ErrorStack> {
  if func_call.as_rule() != Rule::func_call {
    return Err(ErrorStack::new(format!(
      "`parse_func_call` can only handle `func_call` rules, found: {:?}",
      func_call.as_rule()
    )));
  }

  let (line, col) = func_call.line_col();
  let loc = ctx.add_source_loc(line, col);

  let mut inner = func_call.into_inner();
  // this contains `fn_name(`
  let name = inner.next().unwrap().as_str();
  // trim to just `fn_name`
  let name = &name[..name.len() - 1];
  let name = ctx.interned_symbols.intern(name);

  let mut args: Vec<Expr> = Vec::new();
  let mut kwargs: FxHashMap<Sym, Expr> = FxHashMap::default();
  for arg in inner {
    let arg = arg.into_inner().next().unwrap();
    match arg.as_rule() {
      Rule::keyword_arg => {
        let mut inner = arg.into_inner();
        let id = ctx.interned_symbols.intern(inner.next().unwrap().as_str());
        let value = inner.next().unwrap();
        let value_expr = parse_node(ctx, value)?;
        kwargs.insert(id, value_expr);
      }
      Rule::expr => {
        let expr = parse_expr(ctx, arg)?;
        args.push(expr);
      }
      _ => unreachable!(
        "Unexpected argument rule in function call: {:?}",
        arg.as_rule()
      ),
    }
  }

  Ok(Expr::Call {
    call: FunctionCall {
      target: FunctionCallTarget::Name(name),
      args,
      kwargs,
    },
    loc,
  })
}

fn parse_node(ctx: &EvalCtx, expr: Pair<Rule>) -> Result<Expr, ErrorStack> {
  let (line, col) = expr.line_col();
  let loc = ctx.add_source_loc(line, col);

  match expr.as_rule() {
    Rule::int => {
      let int_str = expr.as_str();
      let normalized = int_str.replace('_', "");
      normalized
        .parse::<i64>()
        .map(|i| Expr::Literal {
          value: Value::Int(i),
          loc,
        })
        .map_err(|_| ErrorStack::new(format!("Invalid integer: {int_str}")))
    }
    Rule::hex_int => {
      let hex_str = expr.as_str();
      let normalized = hex_str.replace('_', "");
      i64::from_str_radix(&normalized[2..], 16)
        .map(|i| Expr::Literal {
          value: Value::Int(i),
          loc,
        })
        .map_err(|_| ErrorStack::new(format!("Invalid hex integer: {hex_str}")))
    }
    Rule::float => {
      let float_str = expr.as_str();
      let normalized = float_str.replace('_', "");
      normalized
        .parse::<f32>()
        .map(|i| Expr::Literal {
          value: Value::Float(i),
          loc,
        })
        .map_err(|_| ErrorStack::new(format!("Invalid float: {float_str}")))
    }
    Rule::ident => Ok(Expr::Ident {
      name: ctx.interned_symbols.intern(expr.as_str()),
      loc,
    }),
    Rule::term => {
      // Delegate to inner; the inner will have its own location
      let inner = expr.into_inner().next().unwrap();
      parse_node(ctx, inner)
    }
    Rule::func_call => parse_fn_call(ctx, expr),
    Rule::expr => parse_expr(ctx, expr),
    Rule::range_literal_expr => {
      let mut inner = expr.into_inner();
      let start = parse_node(ctx, inner.next().unwrap())?;
      let end = match inner.next() {
        Some(end) => Some(Box::new(parse_node(ctx, end)?)),
        None => None,
      };
      Ok(Expr::Range {
        start: Box::new(start),
        end,
        inclusive: false,
        loc,
      })
    }
    Rule::range_inclusive_literal_expr => {
      let mut inner = expr.into_inner();
      let start = parse_node(ctx, inner.next().unwrap())?;
      let end = match inner.next() {
        Some(end) => Some(Box::new(parse_node(ctx, end)?)),
        None => None,
      };
      Ok(Expr::Range {
        start: Box::new(start),
        end,
        inclusive: true,
        loc,
      })
    }
    Rule::closure => {
      let mut inner = expr.into_inner();
      let args_list = inner.next().unwrap();
      let params = args_list
        .into_inner()
        .map(|p| {
          let mut inner = p.into_inner();
          let arg = inner.next().unwrap();
          let ident = parse_destructure_pattern(ctx, arg)?;
          let (type_hint, default_val) = if let Some(next) = inner.next() {
            let mut type_hint = None;
            let mut default_val = None;
            if next.as_rule() == Rule::type_hint {
              type_hint = Some(
                TypeName::from_str(next.into_inner().next().unwrap().as_str()).map_err(|err| {
                  ErrorStack::new(err).wrap(format!(
                    "Invalid type hint for closure arg {:?}",
                    ident.debug(ctx)
                  ))
                })?,
              );
              if let Some(next) = inner.next() {
                if next.as_rule() == Rule::fn_arg_default_val {
                  default_val = Some(parse_node(ctx, next.into_inner().next().unwrap())?);
                } else {
                  unreachable!()
                }
              }
            } else if next.as_rule() == Rule::fn_arg_default_val {
              default_val = Some(parse_node(ctx, next.into_inner().next().unwrap())?);
            } else {
              unreachable!()
            }

            (type_hint, default_val)
          } else {
            (None, None)
          };
          Ok(ClosureArg {
            ident,
            type_hint,
            default_val,
          })
        })
        .collect::<Result<Vec<_>, ErrorStack>>()?;

      let mut next = inner.next().unwrap();
      let return_type_hint = if next.as_rule() == Rule::type_hint {
        let type_hint_str = next.into_inner().next().unwrap().as_str();
        let return_type_hint = TypeName::from_str(type_hint_str)
          .map_err(|err| ErrorStack::new(err).wrap("Invalid type hint for closure return type"))?;
        next = inner.next().unwrap();
        Some(return_type_hint)
      } else {
        None
      };

      let body = match next.as_rule() {
        Rule::simple_closure_body => {
          let expr = parse_expr(ctx, next.clone().into_inner().next().unwrap())?;
          ClosureBody(vec![Statement::Expr(expr)])
        }
        Rule::bracketed_closure_body => {
          let stmts = next
            .into_inner()
            .filter_map(|stmt| match parse_statement(ctx, stmt) {
              Ok(Some(stmt)) => Some(Ok(stmt)),
              Ok(None) => None,
              Err(err) => Some(Err(err)),
            })
            .collect::<Result<Vec<_>, ErrorStack>>()?;

          ClosureBody(stmts)
        }
        _ => unreachable!("Unexpected closure body rule"),
      };

      Ok(Expr::Closure {
        arg_placeholder_scope: Closure::build_arg_placeholder_scope(&params),
        params: Rc::new(params),
        body: Rc::new(body),
        return_type_hint,
        loc,
      })
    }
    Rule::array_literal => {
      let elems = expr
        .into_inner()
        .map(|e| parse_expr(ctx, e))
        .collect::<Result<Vec<_>, ErrorStack>>()?;
      Ok(Expr::ArrayLiteral {
        elements: elems,
        loc,
      })
    }
    Rule::bool_literal => {
      let bool_str = expr.as_str();
      match bool_str {
        "true" => Ok(Expr::Literal {
          value: Value::Bool(true),
          loc,
        }),
        "false" => Ok(Expr::Literal {
          value: Value::Bool(false),
          loc,
        }),
        _ => unreachable!("Unexpected boolean literal: {bool_str}, expected 'true' or 'false'"),
      }
    }
    Rule::nil_literal => Ok(Expr::Literal {
      value: Value::Nil,
      loc,
    }),
    Rule::if_expression => {
      let mut inner = expr.into_inner();
      let cond = parse_expr(ctx, inner.next().unwrap())?;
      let then = parse_expr(ctx, inner.next().unwrap())?;
      let (else_if_exprs, else_expr): (Vec<(Expr, Expr)>, Option<_>) = 'others: {
        let Some(next) = inner.next() else {
          break 'others (Vec::new(), None);
        };
        let mut else_if_exprs = Vec::new();

        match next.as_rule() {
          Rule::else_if_expr => {
            let mut else_if_inner = next.into_inner();
            let cond = parse_expr(ctx, else_if_inner.next().unwrap())?;
            let then = parse_expr(ctx, else_if_inner.next().unwrap())?;
            else_if_exprs.push((cond, then));
          }
          Rule::else_expr => {
            let else_expr = parse_expr(ctx, next.into_inner().next().unwrap())?;
            break 'others (else_if_exprs, Some(Box::new(else_expr)));
          }
          _ => unreachable!("Unexpected rule in if expression: {:?}", next.as_rule()),
        }

        loop {
          let Some(next) = inner.next() else {
            break 'others (else_if_exprs, None);
          };
          match next.as_rule() {
            Rule::else_if_expr => {
              let mut else_if_inner = next.into_inner();
              let cond = parse_expr(ctx, else_if_inner.next().unwrap())?;
              let then = parse_expr(ctx, else_if_inner.next().unwrap())?;
              else_if_exprs.push((cond, then));
            }
            Rule::else_expr => {
              let else_expr = parse_expr(ctx, next.into_inner().next().unwrap())?;
              return Ok(Expr::Conditional {
                cond: Box::new(cond),
                then: Box::new(then),
                else_if_exprs,
                else_expr: Some(Box::new(else_expr)),
                loc,
              });
            }
            _ => unreachable!("Unexpected rule in if expression: {:?}", next.as_rule()),
          }
        }
      };

      Ok(Expr::Conditional {
        cond: Box::new(cond),
        then: Box::new(then),
        else_if_exprs,
        else_expr,
        loc,
      })
    }
    Rule::double_quote_string_literal => {
      let inner = expr.into_inner().next().unwrap();
      if inner.as_rule() != Rule::double_quote_string_inner {
        unreachable!();
      }
      let s = parse_double_quote_string_inner(inner)?;
      Ok(Expr::Literal {
        value: Value::String(s),
        loc,
      })
    }
    Rule::single_quote_string_literal => {
      let inner = expr.into_inner().next().unwrap();
      if inner.as_rule() != Rule::single_quote_string_inner {
        unreachable!();
      }
      let s = parse_single_quote_string_inner(inner)?;
      Ok(Expr::Literal {
        value: Value::String(s),
        loc,
      })
    }
    Rule::map_literal => {
      let entries = expr.into_inner();
      let mut out_entries = Vec::with_capacity(entries.len());
      for entry in entries {
        match entry.as_rule() {
          Rule::map_kv => {
            let mut inner = entry.into_inner();
            let key = inner.next().unwrap();
            let key = match key.as_rule() {
              Rule::double_quote_string_literal => {
                parse_double_quote_string_inner(key.into_inner().next().unwrap())?
              }
              Rule::single_quote_string_literal => {
                parse_single_quote_string_inner(key.into_inner().next().unwrap())?
              }
              Rule::ident => key.as_str().to_owned(),
              _ => unreachable!("Unexpected key rule in map literal: {:?}", key.as_rule()),
            };
            let value = inner.next().unwrap();
            let value_expr = parse_expr(ctx, value)?;
            out_entries.push(MapLiteralEntry::KeyValue {
              key,
              value: value_expr,
            });
          }
          Rule::map_splat => {
            let inner = entry.into_inner().next().unwrap();
            let expr = parse_expr(ctx, inner)?;
            out_entries.push(MapLiteralEntry::Splat { expr });
          }
          _ => panic!("Unexpected rule inside map literal: {:?}", entry.as_rule()),
        }
      }

      Ok(Expr::MapLiteral {
        entries: out_entries,
        loc,
      })
    }
    Rule::block_expr => parse_block_expr(ctx, expr),
    _ => unimplemented!(
      "unimplemented node type for parse_node: {:?}",
      expr.as_rule()
    ),
  }
}

fn parse_double_quote_string_inner(pair: Pair<Rule>) -> Result<String, ErrorStack> {
  if pair.as_rule() != Rule::double_quote_string_inner {
    return Err(ErrorStack::new(format!(
      "`parse_double_quote_string_inner` can only handle `double_quote_string_inner` rules, \
       found: {:?}",
      pair.as_rule()
    )));
  }

  let mut acc = String::new();
  for inner in pair.into_inner() {
    match inner.as_rule() {
      Rule::unescaped_quote_str_content => {
        acc.push_str(inner.as_str());
      }
      Rule::escaped_double_quote => {
        acc.push('"');
      }
      Rule::escaped_backslash => {
        acc.push('\\');
      }
      Rule::double_quote_string_inner => {
        acc.push_str(parse_double_quote_string_inner(inner)?.as_str());
      }
      _ => unreachable!(
        "Unexpected rule in double quote string inner: {:?}",
        inner.as_rule()
      ),
    }
  }

  Ok(acc)
}

fn parse_single_quote_string_inner(pair: Pair<Rule>) -> Result<String, ErrorStack> {
  if pair.as_rule() != Rule::single_quote_string_inner {
    return Err(ErrorStack::new(format!(
      "`parse_single_quote_string_inner` can only handle `single_quote_string_inner` rules, \
       found: {:?}",
      pair.as_rule()
    )));
  }

  let mut acc = String::new();
  for inner in pair.into_inner() {
    match inner.as_rule() {
      Rule::unescaped_apos_str_content => {
        acc.push_str(inner.as_str());
      }
      Rule::escaped_single_quote => {
        acc.push('\'');
      }
      Rule::escaped_backslash => {
        acc.push('\\');
      }
      Rule::single_quote_string_inner => {
        acc.push_str(parse_single_quote_string_inner(inner)?.as_str());
      }
      _ => unreachable!(
        "Unexpected rule in single quote string inner: {:?}",
        inner.as_rule()
      ),
    }
  }

  Ok(acc)
}

fn parse_block_expr(ctx: &EvalCtx, expr: Pair<Rule>) -> Result<Expr, ErrorStack> {
  if expr.as_rule() != Rule::block_expr {
    return Err(ErrorStack::new(format!(
      "`parse_block_expr` can only handle `block_expr` rules, found: {:?}",
      expr.as_rule()
    )));
  }

  let (line, col) = expr.line_col();
  let loc = ctx.add_source_loc(line, col);

  let statements = expr
    .into_inner()
    .map(|stmt| parse_statement(ctx, stmt))
    .filter_map(|res| match res {
      Ok(Some(stmt)) => Some(Ok(stmt)),
      Ok(None) => None,
      Err(err) => Some(Err(err)),
    })
    .collect::<Result<Vec<_>, ErrorStack>>()?;

  Ok(Expr::Block { statements, loc })
}

pub fn parse_expr(ctx: &EvalCtx, expr: Pair<Rule>) -> Result<Expr, ErrorStack> {
  if expr.as_rule() == Rule::block_expr {
    return parse_block_expr(ctx, expr);
  }

  if expr.as_rule() != Rule::expr {
    panic!(
      "`parse_expr` can only handle `expr` rules, found: {:?}",
      expr.as_rule()
    );
  }

  PRATT_PARSER
    .map_primary(|primary| -> Result<Expr, ErrorStack> {
      match primary.as_rule() {
        Rule::term => parse_node(ctx, primary),
        _ => unimplemented!("Unexpected primary rule: {:?}", primary.as_rule()),
      }
    })
    .map_prefix(|op, expr| {
      let (line, col) = op.line_col();
      let loc = ctx.add_source_loc(line, col);
      match op.as_rule() {
        Rule::neg_op => Ok(Expr::PrefixOp {
          op: PrefixOp::Neg,
          expr: Box::new(expr?),
          loc,
        }),
        Rule::pos_op => Ok(Expr::PrefixOp {
          op: PrefixOp::Pos,
          expr: Box::new(expr?),
          loc,
        }),
        Rule::negate_op => Ok(Expr::PrefixOp {
          op: PrefixOp::Not,
          expr: Box::new(expr?),
          loc,
        }),
        _ => unreachable!("Unexpected prefix operator rule: {:?}", op.as_rule()),
      }
    })
    .map_infix(|lhs, op, rhs| {
      let bin_op = match op.as_rule() {
        Rule::add_op => BinOp::Add,
        Rule::sub_op => BinOp::Sub,
        Rule::mul_op => BinOp::Mul,
        Rule::div_op => BinOp::Div,
        Rule::pipeline_op => BinOp::Pipeline,
        Rule::range_inclusive_op => BinOp::RangeInclusive,
        Rule::range_op => BinOp::Range,
        Rule::gte_op => BinOp::Gte,
        Rule::lte_op => BinOp::Lte,
        Rule::gt_op => BinOp::Gt,
        Rule::lt_op => BinOp::Lt,
        Rule::eq_op => BinOp::Eq,
        Rule::neq_op => BinOp::Neq,
        Rule::mod_op => BinOp::Mod,
        Rule::and_op => BinOp::And,
        Rule::bit_and_op => BinOp::BitAnd,
        Rule::or_op => BinOp::Or,
        Rule::map_op => BinOp::Map,
        _ => {
          return Err(ErrorStack::new(format!(
            "Unhandled operator rule: {:?}",
            op.as_rule()
          )))
        }
      };
      let lhs = lhs?;
      let loc = lhs.loc();
      Ok(Expr::BinOp {
        op: bin_op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs?),
        pre_resolved_def_ix: None,
        loc,
      })
    })
    .map_postfix(|expr, op| {
      let expr = expr?;
      let loc = expr.loc();

      if op.as_rule() != Rule::postfix {
        unreachable!("Expected postfix rule, found: {:?}", op.as_rule());
      }

      let inner = op.into_inner().next().unwrap();
      match inner.as_rule() {
        Rule::static_field_access => {
          let field = inner.into_inner().next().unwrap().as_str().to_owned();
          Ok(Expr::StaticFieldAccess {
            lhs: Box::new(expr),
            field,
            loc,
          })
        }
        Rule::field_access => {
          let index_expr = parse_expr(ctx, inner.into_inner().next().unwrap())?;
          Ok(Expr::FieldAccess {
            lhs: Box::new(expr),
            field: Box::new(index_expr),
            loc,
          })
        }
        other => unreachable!("Unexpected postfix rule: {other:?} in expression: {expr:?}",),
      }
    })
    .parse(expr.into_inner())
}

fn parse_assignment(ctx: &EvalCtx, assignment: Pair<Rule>) -> Result<Statement, ErrorStack> {
  if assignment.as_rule() != Rule::assignment {
    return Err(ErrorStack::new(format!(
      "`parse_assignment` can only handle `assignment` rules, found: {:?}",
      assignment.as_rule()
    )));
  }

  let mut inner = assignment.into_inner();
  let name = ctx.interned_symbols.intern(inner.next().unwrap().as_str());

  let mut next = inner.next().unwrap();
  let type_hint = if next.as_rule() == Rule::type_hint {
    let type_hint =
      TypeName::from_str(next.into_inner().next().unwrap().as_str()).map_err(ErrorStack::new)?;
    next = inner.next().unwrap();
    Some(type_hint)
  } else {
    None
  };

  let expr = parse_expr(ctx, next)?;

  Ok(Statement::Assignment {
    name,
    expr,
    type_hint,
  })
}

fn parse_destructure_pattern(
  ctx: &EvalCtx,
  pair: Pair<Rule>,
) -> Result<DestructurePattern, ErrorStack> {
  match pair.as_rule() {
    Rule::ident => Ok(DestructurePattern::Ident(
      ctx.interned_symbols.intern(pair.as_str()),
    )),
    Rule::array_destructure => parse_array_destructure(ctx, pair),
    Rule::map_destructure => parse_map_destructure(ctx, pair),
    _ => Err(ErrorStack::new(format!(
      "Unexpected inner destructure pattern rule: {:?}",
      pair.as_rule()
    ))),
  }
}

fn parse_array_destructure(
  ctx: &EvalCtx,
  lhs: Pair<Rule>,
) -> Result<DestructurePattern, ErrorStack> {
  let elements = lhs
    .into_inner()
    .map(|pat| parse_destructure_pattern(ctx, pat))
    .collect::<Result<Vec<_>, _>>()?;
  Ok(DestructurePattern::Array(elements))
}

fn parse_map_destructure(ctx: &EvalCtx, lhs: Pair<Rule>) -> Result<DestructurePattern, ErrorStack> {
  let pat = lhs
    .into_inner()
    .map(|pair| {
      if pair.as_rule() != Rule::map_destructure_elem {
        unreachable!();
      }
      let inner = pair.into_inner().next().unwrap();
      match inner.as_rule() {
        Rule::ident => {
          let key = ctx.interned_symbols.intern(inner.as_str());
          let pat = DestructurePattern::Ident(key);
          Ok((key, pat))
        }
        Rule::map_destructure_kv => {
          let mut inner = inner.into_inner();
          let lhs = inner.next().unwrap();
          let rhs = parse_destructure_pattern(ctx, inner.next().unwrap())?;
          Ok((ctx.interned_symbols.intern(lhs.as_str()), rhs))
        }
        _ => unreachable!(
          "Unexpected map destructure pattern rule: {:?}",
          inner.as_rule()
        ),
      }
    })
    .collect::<Result<FxHashMap<_, _>, _>>()?;
  Ok(DestructurePattern::Map(pat))
}

fn parse_destructure_lhs(ctx: &EvalCtx, lhs: Pair<Rule>) -> Result<DestructurePattern, ErrorStack> {
  match lhs.as_rule() {
    Rule::array_destructure => parse_array_destructure(ctx, lhs),
    Rule::map_destructure => parse_map_destructure(ctx, lhs),
    _ => unreachable!("Unexpected destructure pattern rule: {:?}", lhs.as_rule()),
  }
}

fn parse_destructure_assignment(
  ctx: &EvalCtx,
  assignment: Pair<Rule>,
) -> Result<Statement, ErrorStack> {
  if assignment.as_rule() != Rule::destructure_assignment {
    return Err(ErrorStack::new(format!(
      "`parse_destructure_assignment` can only handle `destructure_assignment` rules, found: {:?}",
      assignment.as_rule()
    )));
  }

  let mut inner = assignment.into_inner();
  let lhs = parse_destructure_lhs(ctx, inner.next().unwrap())?;

  let rhs = parse_expr(ctx, inner.next().unwrap())?;

  Ok(Statement::DestructureAssignment { lhs, rhs })
}

fn parse_return_statement(ctx: &EvalCtx, return_stmt: Pair<Rule>) -> Result<Statement, ErrorStack> {
  if return_stmt.as_rule() != Rule::return_statement {
    return Err(ErrorStack::new(format!(
      "`parse_return_statement` can only handle `return_statement` rules, found: {:?}",
      return_stmt.as_rule()
    )));
  }

  let mut inner = return_stmt.into_inner();
  let value = if let Some(expr) = inner.next() {
    Some(parse_expr(ctx, expr)?)
  } else {
    None
  };

  Ok(Statement::Return { value })
}

fn parse_break_statement(ctx: &EvalCtx, return_stmt: Pair<Rule>) -> Result<Statement, ErrorStack> {
  if return_stmt.as_rule() != Rule::break_statement {
    return Err(ErrorStack::new(format!(
      "`parse_break_statement` can only handle `break_statement` rules, found: {:?}",
      return_stmt.as_rule()
    )));
  }

  let mut inner = return_stmt.into_inner();
  let value = if let Some(expr) = inner.next() {
    Some(parse_expr(ctx, expr)?)
  } else {
    None
  };

  Ok(Statement::Break { value })
}

pub(crate) fn parse_statement(
  ctx: &EvalCtx,
  stmt: Pair<Rule>,
) -> Result<Option<Statement>, ErrorStack> {
  match stmt.as_rule() {
    Rule::assignment => parse_assignment(ctx, stmt).map(Some),
    Rule::destructure_assignment => parse_destructure_assignment(ctx, stmt).map(Some),
    Rule::expr => Ok(Some(Statement::Expr(parse_expr(ctx, stmt)?))),
    Rule::return_statement => Ok(Some(parse_return_statement(ctx, stmt)?)),
    Rule::break_statement => Ok(Some(parse_break_statement(ctx, stmt)?)),
    Rule::EOI => Ok(None),
    _ => unreachable!("Unexpected statement rule: {:?}", stmt.as_rule()),
  }
}

#[derive(Clone, Debug)]
pub(crate) enum TrackedValue {
  /// Value is const-available and has already been evaluated
  Const(Value),
  /// Value is a closure argument and isn't available during const eval
  Arg(ClosureArg),
  /// Value is a non-const variable, either directly from or derived from an enclosing scope
  Dyn { type_hint: Option<TypeName> },
}

impl TrackedValue {
  pub fn as_ref<'a>(&'a self) -> TrackedValueRef<'a> {
    match self {
      TrackedValue::Const(val) => TrackedValueRef::Const(val),
      TrackedValue::Arg(arg) => TrackedValueRef::Arg(arg),
      TrackedValue::Dyn { type_hint } => TrackedValueRef::Dyn {
        type_hint: *type_hint,
      },
    }
  }
}

#[derive(Debug)]
pub(crate) enum TrackedValueRef<'a> {
  Const(&'a Value),
  Arg(&'a ClosureArg),
  Dyn { type_hint: Option<TypeName> },
}

#[derive(Default, Debug)]
pub(crate) struct ScopeTracker<'a> {
  pub vars: FxHashMap<Sym, TrackedValue>,
  pub parent: Option<&'a ScopeTracker<'a>>,
}

impl<'a> ScopeTracker<'a> {
  pub fn wrap(parent: &'a ScopeTracker<'a>) -> Self {
    ScopeTracker {
      vars: FxHashMap::default(),
      parent: Some(parent),
    }
  }

  pub fn fork(&self) -> ScopeTracker<'a> {
    ScopeTracker {
      vars: self.vars.clone(),
      parent: self.parent,
    }
  }

  pub fn has<'b>(&'b self, name: Sym) -> bool {
    if self.vars.contains_key(name.borrow()) {
      return true;
    }
    if let Some(parent) = self.parent {
      return parent.has(name);
    }
    false
  }

  pub fn get<'b>(&'b self, name: Sym) -> Option<TrackedValueRef<'b>> {
    if let Some(val) = self.vars.get(name.borrow()) {
      return Some(val.as_ref());
    }
    if let Some(parent) = self.parent {
      return parent.get(name);
    }
    None
  }

  pub fn set(&mut self, name: Sym, value: TrackedValue) {
    self.vars.insert(name, value);
  }
}

pub(crate) fn pre_resolve_binop_def_ix(
  ctx: &EvalCtx,
  scope_tracker: &ScopeTracker,
  op: &BinOp,
  lhs: &Expr,
  rhs: &Expr,
) -> Option<(&'static [FnSignature], usize)> {
  let builtin_name = op.get_builtin_fn_name()?;

  let builtin_arg_defs = fn_sigs()[builtin_name].signatures;

  let lhs_ty = pre_resolve_expr_type(ctx, scope_tracker, lhs)?;
  let rhs_ty = pre_resolve_expr_type(ctx, scope_tracker, rhs)?;
  let fn_entry_ix = get_builtin_fn_sig_entry_ix(builtin_name).unwrap();
  get_binop_def_ix(
    ctx,
    fn_entry_ix,
    &lhs_ty.build_example_val()?,
    &rhs_ty.build_example_val()?,
  )
  .ok()
  .map(|def_ix| (builtin_arg_defs, def_ix))
}

pub(crate) fn pre_resolve_expr_type(
  ctx: &EvalCtx,
  scope_tracker: &ScopeTracker,
  arg: &Expr,
) -> Option<ArgType> {
  match arg {
    Expr::Literal { value: v, .. } => Some(v.get_type()),
    Expr::Ident { name: id, .. } => match scope_tracker.get(*id) {
      Some(TrackedValueRef::Const(val)) => Some(val.get_type()),
      Some(TrackedValueRef::Arg(arg)) => arg.type_hint.map(Into::into),
      Some(TrackedValueRef::Dyn { type_hint }) => type_hint.map(Into::into),
      None => None, // error will happen later
    },
    Expr::BinOp {
      op,
      lhs,
      rhs,
      pre_resolved_def_ix,
      ..
    } => {
      match op {
        BinOp::Range | BinOp::RangeInclusive => return Some(ArgType::Sequence),
        BinOp::Pipeline => {
          let rhs_ty = pre_resolve_expr_type(ctx, scope_tracker, rhs)?;
          if matches!(rhs_ty, ArgType::Mesh) {
            return Some(ArgType::Mesh);
          }

          return match &**rhs {
            Expr::Literal {
              value: Value::Callable(callable),
              ..
            } => callable.get_return_type_hint(),
            _ => None,
          };
        }
        BinOp::Map => return Some(ArgType::Sequence),
        _ => (),
      }

      let (builtin_arg_defs, def_ix) = match pre_resolved_def_ix {
        Some(def_ix) => {
          let builtin_name = op.get_builtin_fn_name()?;
          let builtin_arg_defs = fn_sigs()[builtin_name].signatures;
          (builtin_arg_defs, *def_ix)
        }
        None => pre_resolve_binop_def_ix(ctx, scope_tracker, op, lhs, rhs)?,
      };
      let return_tys = builtin_arg_defs[def_ix].return_type;
      match return_tys.len() {
        0 => return None,
        1 => (),
        _ => return None,
      }
      match &return_tys[0] {
        ArgType::Any => None,
        ty => Some(*ty),
      }
    }
    Expr::PrefixOp { op, expr, .. } => {
      let arg_ty = pre_resolve_expr_type(ctx, scope_tracker, expr)?;
      let example_val = arg_ty.build_example_val()?;
      let return_ty_res = match op {
        PrefixOp::Neg => get_unop_return_ty(ctx, "neg", fn_sigs()["neg"].signatures, &example_val),
        PrefixOp::Pos => get_unop_return_ty(ctx, "pos", fn_sigs()["pos"].signatures, &example_val),
        PrefixOp::Not => get_unop_return_ty(ctx, "not", fn_sigs()["not"].signatures, &example_val),
      };
      match return_ty_res {
        Ok(return_tys) => {
          if return_tys.len() == 1 {
            if matches!(return_tys[0], ArgType::Any) {
              None
            } else {
              Some(return_tys[0])
            }
          } else {
            None
          }
        }
        Err(_) => None,
      }
    }
    Expr::Range { .. } => Some(ArgType::Sequence),
    Expr::StaticFieldAccess { lhs, field, .. } => {
      let lhs_ty = pre_resolve_expr_type(ctx, scope_tracker, lhs)?;
      let lhs_val = lhs_ty.build_example_val()?;
      let out_val = match ctx.eval_static_field_access(&lhs_val, field) {
        Ok(out) => out,
        Err(err) => {
          log::error!(
            "Got error when evaluating field access with example values; lhs={lhs:?}, \
             field={field:?}, err={err}"
          );
          return None;
        }
      };
      let out_ty = out_val.get_type();
      if matches!(out_ty, ArgType::Any) {
        return None;
      }
      Some(out_ty)
    }
    Expr::FieldAccess { lhs, field, .. } => {
      let lhs_ty = pre_resolve_expr_type(ctx, scope_tracker, lhs)?;
      let lhs_val = lhs_ty.build_example_val()?;
      let field_ty = pre_resolve_expr_type(ctx, scope_tracker, field)?;
      let field_val = field_ty.build_example_val()?;
      let out_val = match ctx.eval_field_access(&lhs_val, &field_val) {
        Ok(out) => out,
        Err(err) => {
          log::error!(
            "Got error when evaluating array access with example values; lhs={lhs:?}, \
             field={field:?}, err={err}"
          );
          return None;
        }
      };
      let out_ty = out_val.get_type();
      if matches!(out_ty, ArgType::Any) {
        return None;
      }
      Some(out_ty)
    }
    Expr::Call {
      call: FunctionCall {
        target,
        args,
        kwargs,
      },
      ..
    } => match target {
      FunctionCallTarget::Name(_) => None,
      FunctionCallTarget::Literal(callable) => match &**callable {
        Callable::Builtin {
          fn_entry_ix,
          pre_resolved_signature,
          ..
        } => {
          let return_ty = if let Some(sig) = pre_resolved_signature {
            let fn_signature_defs = fn_sigs().entries[*fn_entry_ix].1.signatures;
            fn_signature_defs[sig.def_ix].return_type
          } else {
            match maybe_pre_resolve_bulitin_call_signature(
              ctx,
              scope_tracker,
              *fn_entry_ix,
              args,
              kwargs,
            ) {
              Ok(Some(sig)) => {
                let fn_signature_defs = fn_sigs().entries[*fn_entry_ix].1.signatures;
                fn_signature_defs[sig.def_ix].return_type
              }
              Ok(None) => return None,
              Err(_) => return None,
            }
          };
          match return_ty.len() {
            0 => None,
            1 => {
              if !matches!(return_ty[0], ArgType::Any) {
                Some(return_ty[0])
              } else {
                None
              }
            }
            _ => None,
          }
        }
        Callable::PartiallyAppliedFn(paf) => paf.get_return_type_hint(),
        Callable::Closure(closure) => closure.return_type_hint.map(Into::into),
        Callable::ComposedFn(_) => None,
        Callable::Dynamic { inner, .. } => return inner.get_return_type_hint(),
      },
    },
    Expr::Closure { .. } => Some(ArgType::Callable),
    Expr::ArrayLiteral { .. } => Some(ArgType::Sequence),
    Expr::MapLiteral { .. } => Some(ArgType::Map),
    Expr::Conditional { .. } => None,
    Expr::Block { .. } => None,
  }
}

pub(crate) fn maybe_pre_resolve_bulitin_call_signature(
  ctx: &EvalCtx,
  scope_tracker: &ScopeTracker,
  fn_entry_ix: usize,
  args: &[Expr],
  kwargs: &FxHashMap<Sym, Expr>,
) -> Result<Option<PreResolvedSignature>, ErrorStack> {
  let mut arg_tys: Vec<ArgType> = Vec::with_capacity(args.len());
  for arg in args {
    let Some(ty) = pre_resolve_expr_type(ctx, scope_tracker, arg) else {
      return Ok(None);
    };
    arg_tys.push(ty);
  }

  let mut kwarg_tys: FxHashMap<Sym, ArgType> =
    FxHashMap::with_capacity_and_hasher(kwargs.len(), Default::default());
  for (name, expr) in kwargs {
    let Some(ty) = pre_resolve_expr_type(ctx, scope_tracker, expr) else {
      return Ok(None);
    };
    kwarg_tys.insert(*name, ty);
  }

  // in order to re-use `get_args` code, we create fake args and kwargs of the types we're expecting
  let Ok(args) = arg_tys
    .into_iter()
    .map(|ty| ty.build_example_val().ok_or(()))
    .collect::<Result<Vec<Value>, ()>>()
  else {
    return Ok(None);
  };
  let Ok(kwargs) = kwarg_tys
    .into_iter()
    .map(|(name, ty)| Ok((name, ty.build_example_val().ok_or(())?)))
    .collect::<Result<FxHashMap<_, Value>, ()>>()
  else {
    return Ok(None);
  };

  let (fn_name, def) = &fn_sigs().entries[fn_entry_ix];
  let sigs = &def.signatures;

  let resolved_sig = get_args(ctx, fn_name, sigs, &args, &kwargs)?;
  match resolved_sig {
    GetArgsOutput::Valid { def_ix, arg_refs } => Ok(Some(PreResolvedSignature {
      def_ix,
      arg_refs: arg_refs.into_iter().collect(),
    })),
    GetArgsOutput::PartiallyApplied => Ok(None),
  }
}

/// This helps differentiate between "true" dyn values (like the output from `randi()`, for example)
/// and dynamic values that depend only on constants and closure arguments.
///
/// This allows closures to be differentiated between those that are completely const and depend on
/// no non-const external values and those that can be const eval'd but depend on args.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum DynType {
  Const,
  Arg,
  Dyn,
}

impl std::ops::BitOr for DynType {
  type Output = Self;

  fn bitor(self, rhs: Self) -> Self::Output {
    if self == DynType::Dyn || rhs == DynType::Dyn {
      DynType::Dyn
    } else if self == DynType::Arg || rhs == DynType::Arg {
      DynType::Arg
    } else {
      DynType::Const
    }
  }
}

pub(crate) fn get_dyn_type(expr: &Expr, local_scope: &ScopeTracker) -> DynType {
  match expr {
    Expr::BinOp { lhs, rhs, .. } => {
      let lhs_type = get_dyn_type(lhs, local_scope);
      let rhs_type = get_dyn_type(rhs, local_scope);
      lhs_type | rhs_type
    }
    Expr::PrefixOp { expr, .. } => get_dyn_type(expr, local_scope),
    Expr::Range { start, end, .. } => {
      let start_type = get_dyn_type(start, local_scope);
      if let Some(end) = end.as_ref() {
        let end_type = get_dyn_type(end, local_scope);
        start_type | end_type
      } else {
        start_type
      }
    }
    Expr::StaticFieldAccess { lhs, .. } => get_dyn_type(lhs, local_scope),
    Expr::FieldAccess { lhs, field, .. } => {
      get_dyn_type(lhs, local_scope) | get_dyn_type(field, local_scope)
    }
    Expr::Call {
      call: FunctionCall { args, kwargs, .. },
      ..
    } => {
      let mut dyn_type = DynType::Const;
      for arg in args {
        dyn_type = dyn_type | get_dyn_type(arg, local_scope);
      }
      for kwarg in kwargs.values() {
        dyn_type = dyn_type | get_dyn_type(kwarg, local_scope);
      }

      dyn_type
    }
    Expr::Closure { params, body, .. } => {
      let mut dyn_type = DynType::Const;
      for param in &**params {
        if let Some(default_val) = &param.default_val {
          dyn_type = dyn_type | get_dyn_type(default_val, local_scope);
        } else {
          dyn_type = dyn_type | DynType::Arg;
        }
      }
      for stmt in &body.0 {
        for expr in stmt.exprs() {
          dyn_type = dyn_type | get_dyn_type(expr, local_scope);
        }
      }
      dyn_type
    }
    Expr::Ident { name, .. } => match local_scope.vars.get(name) {
      Some(TrackedValue::Const(_)) => DynType::Const,
      Some(TrackedValue::Arg(_)) => DynType::Arg,
      Some(TrackedValue::Dyn { .. }) => DynType::Dyn,
      None => match local_scope.parent {
        Some(parent) => match parent.get(*name) {
          Some(TrackedValueRef::Const(_)) => DynType::Const,
          // closure args from the parent scope aren't part of the pure scope of the current
          // closure, so we have to treat them as true dyn
          Some(TrackedValueRef::Arg(_)) => DynType::Dyn,
          Some(TrackedValueRef::Dyn { .. }) => DynType::Dyn,
          None => DynType::Dyn,
        },
        None => DynType::Dyn,
      },
    },
    Expr::ArrayLiteral {
      elements: exprs, ..
    } => exprs.iter().fold(DynType::Const, |acc, expr| {
      acc | get_dyn_type(expr, local_scope)
    }),
    Expr::MapLiteral { entries, .. } => entries.iter().fold(DynType::Const, |acc, entry| {
      acc | get_dyn_type(entry.expr(), local_scope)
    }),
    Expr::Literal { .. } => DynType::Const,
    Expr::Conditional {
      cond,
      then,
      else_if_exprs,
      else_expr,
      ..
    } => {
      let mut dyn_type = get_dyn_type(cond, local_scope);
      dyn_type = dyn_type | get_dyn_type(then, local_scope);
      for (cond, inner) in else_if_exprs {
        dyn_type = dyn_type | get_dyn_type(cond, local_scope);
        dyn_type = dyn_type | get_dyn_type(inner, local_scope);
      }
      if let Some(else_expr) = else_expr {
        dyn_type = dyn_type | get_dyn_type(else_expr, local_scope);
      }
      dyn_type
    }
    Expr::Block { statements, .. } => statements.iter().fold(DynType::Const, |acc, stmt| {
      stmt
        .exprs()
        .fold(acc, |acc, expr| acc | get_dyn_type(expr, local_scope))
    }),
  }
}

/// This doesn't disambiguate between builtin fn calls and calling user-defined functions.
pub fn traverse_fn_calls(program: &Program, mut cb: impl FnMut(Sym)) {
  let mut cb = move |expr: &Expr| {
    if let Expr::Call {
      call: FunctionCall {
        target: FunctionCallTarget::Name(name),
        ..
      },
      ..
    } = expr
    {
      cb(*name)
    }
  };

  for stmt in &program.statements {
    stmt.traverse_exprs(&mut cb);
  }
}

/// In order to simplify the syntax and make it more ergonomic to chain unbracketed closures in
/// pipelines, we apply a transform to transform non-bracketed closures into bracketed ones,
/// treating newline characters as ending the closure body.
pub(crate) fn bracketify_closures(
  ctx: &EvalCtx,
  program: &mut Pair<Rule>,
  src: &str,
) -> Result<String, ErrorStack> {
  #[derive(Clone, Copy, PartialEq, Eq, Debug)]
  enum CurlyBracketType {
    Open,
    Close,
  }

  let mut curly_bracket_positions: Vec<(usize, CurlyBracketType)> = Vec::new();

  fn traverse(
    ctx: &EvalCtx,
    curly_bracket_positions: &mut Vec<(usize, CurlyBracketType)>,
    pair: Pair<Rule>,
  ) -> Result<(), ErrorStack> {
    match &pair.as_rule() {
      Rule::simple_closure_body => {
        let expr = parse_expr(ctx, pair.clone().into_inner().next().unwrap())?;
        match expr {
          Expr::BinOp { .. } => (),
          _ => return Ok(()),
        }

        let start_pos = pair.as_span().start();
        match pair.as_str().split_once('\n') {
          Some((body, _rest)) => {
            let end_pos = start_pos + body.len();
            curly_bracket_positions.push((start_pos, CurlyBracketType::Open));
            curly_bracket_positions.push((end_pos, CurlyBracketType::Close));
          }
          None => (),
        }
      }
      _ => (),
    }

    for inner_pair in pair.into_inner() {
      traverse(ctx, curly_bracket_positions, inner_pair)?;
    }

    Ok(())
  }

  for pair in program.clone().into_inner() {
    traverse(ctx, &mut curly_bracket_positions, pair)?;
  }

  curly_bracket_positions.sort_unstable_by_key(|(pos, _)| Reverse(*pos));
  let mut transformed_src = String::new();

  transformed_src.push_str(&src[..]);
  for (pos, bracket_type) in curly_bracket_positions {
    match bracket_type {
      CurlyBracketType::Open => {
        transformed_src.insert(pos, '{');
      }
      CurlyBracketType::Close => {
        transformed_src.insert(pos, '}');
      }
    }
  }

  Ok(transformed_src)
}

#[test]
fn test_inline_const_captures_blocks_side_effectful_alias() {
  let ctx = EvalCtx::default();
  let sym = ctx.interned_symbols.intern("p");
  let fn_entry_ix = get_builtin_fn_sig_entry_ix("print").unwrap();
  let callable = Callable::Builtin {
    fn_entry_ix,
    fn_impl: crate::resolve_builtin_impl("print"),
    pre_resolved_signature: None,
  };
  let mut scope = ScopeTracker::default();
  scope.set(sym, TrackedValue::Const(Value::Callable(Rc::new(callable))));

  let expr = Expr::Call {
    call: FunctionCall {
      target: FunctionCallTarget::Name(sym),
      args: vec![Expr::Literal {
        value: Value::Int(1),
        loc: SourceLoc::default(),
      }],
      kwargs: FxHashMap::default(),
    },
    loc: SourceLoc::default(),
  };

  let captures_dyn = expr.analyze_const_captures(&ctx, &mut scope, true, false, false);
  assert!(
    captures_dyn,
    "Expected side-effectful callable aliases to block const capture"
  );
}

#[test]
fn test_inline_const_captures_visits_all_conditional_branches() {
  let ctx = EvalCtx::default();
  let cond_sym = ctx.interned_symbols.intern("cond");
  let then_sym = ctx.interned_symbols.intern("then_val");
  let else_sym = ctx.interned_symbols.intern("else_val");

  let mut scope = ScopeTracker::default();
  scope.set(cond_sym, TrackedValue::Dyn { type_hint: None });
  scope.set(then_sym, TrackedValue::Const(Value::Int(1)));
  scope.set(else_sym, TrackedValue::Const(Value::Int(2)));

  let mut expr = Expr::Conditional {
    cond: Box::new(Expr::Ident {
      name: cond_sym,
      loc: SourceLoc::default(),
    }),
    then: Box::new(Expr::Ident {
      name: then_sym,
      loc: SourceLoc::default(),
    }),
    else_if_exprs: Vec::new(),
    else_expr: Some(Box::new(Expr::Ident {
      name: else_sym,
      loc: SourceLoc::default(),
    })),
    loc: SourceLoc::default(),
  };

  let captures_dyn = expr.analyze_const_captures(&ctx, &mut scope, true, false, false);
  assert!(captures_dyn);
  expr.inline_const_captures(&ctx, &mut scope);

  match expr {
    Expr::Conditional {
      then, else_expr, ..
    } => {
      assert!(matches!(
        *then,
        Expr::Literal {
          value: Value::Int(1),
          ..
        }
      ));
      let Some(else_expr) = else_expr else {
        panic!("Expected else branch to be preserved");
      };
      assert!(matches!(
        *else_expr,
        Expr::Literal {
          value: Value::Int(2),
          ..
        }
      ));
    }
    _ => panic!("Expected a conditional expression"),
  }
}

#[test]
fn test_space_between_fn_call_parens_not_allowed() {
  let code = "x = add (1, 2)";

  let ctx = EvalCtx::default();
  crate::parse_program_src(&ctx, code).unwrap_err();
}

#[test]
fn test_if_with_parens_condition() {
  let code = r#"
x = if (1 == 1) {
  1
} else {
  0
}"#;

  let ctx = super::parse_and_eval_program(code).unwrap();
  let x = ctx.get_global("x").unwrap().as_int().unwrap();
  assert_eq!(x, 1);
}

#[test]
fn test_numeric_separators_in_int_literals() {
  let code = r#"
a = 100_000
b = 0xFF_FF
"#;

  let ctx = super::parse_and_eval_program(code).unwrap();
  let a = ctx.get_global("a").unwrap().as_int().unwrap();
  let b = ctx.get_global("b").unwrap().as_int().unwrap();
  assert_eq!(a, 100000);
  assert_eq!(b, 0xFFFF);
}

#[test]
fn test_numeric_separators_in_float_literals() {
  let code = r#"
x = 1_234.5_6
y = 10_0.
"#;

  let ctx = super::parse_and_eval_program(code).unwrap();
  let x = ctx.get_global("x").unwrap().as_float().unwrap();
  let y = ctx.get_global("y").unwrap().as_float().unwrap();
  assert_eq!(x, 1234.56_f32);
  assert_eq!(y, 100.0_f32);
}

#[test]
fn test_single_line_closure_chaining() {
  let code = r#"
x = [1,2,3]
  -> |x| x * 2
  -> |x| x + 1
  | reduce(add)
"#;

  let ctx = super::parse_and_eval_program(code).unwrap();
  let x = ctx.get_global("x").unwrap().as_int().unwrap();
  assert_eq!(x, 1 * 2 + 1 + 2 * 2 + 1 + 3 * 2 + 1);
}

#[test]
fn test_single_line_closure_chaining_2() {
  let code = r#"
x = |x: int| {
  0..=2
    -> |i: int| add(i, x)
    -> |v: int| v * 2
}

x = x(3) | reduce(add)
"#;

  let ctx = super::parse_and_eval_program(code).unwrap();
  let x = ctx.get_global("x").unwrap().as_int().unwrap();
  assert_eq!(x, (0 + 3) * 2 + (1 + 3) * 2 + (2 + 3) * 2);
}

#[test]
fn test_single_line_closure_chaining_3() {
  let code = r#"
chip = |a: num, b: num, x_rad: num, y_rad: num, n_contours: int, resolution: int|: mesh {
  shape = |t: num|: vec3 {
    x = x_rad * cos(2 * pi * t)
    y = y_rad * sin(2 * pi * t)
    z = ((x*x) / (a*a)) - ((y*y) / (b*b))
    v3(x, y, z*0.03)
  }

  0..=n_contours
    -> |contour_ix: int| {
      0..resolution
        -> |i: int| shape(i / resolution)
        -> |v: vec3| v * (contour_ix / n_contours)
    }
    | stitch_contours
    | extrude(up=v3(0,0,0.1))
    | rot(-pi/2, 0, 0)
}
x = chip(1, 1, 1, 1, 10, 10)
"#;

  let ctx = super::parse_and_eval_program(code).unwrap();
  let _x = ctx.get_global("x").unwrap().as_mesh().unwrap();
}

#[test]
fn test_parenthesized_expr_fn_call_disambiguation() {
  let code = r#"
x = box(1)
foo = box(2)
(x + foo) | render
"#;

  let ctx = super::parse_and_eval_program(code).unwrap();
  let _rendered = ctx.rendered_meshes.into_inner().into_iter().next().unwrap();
}
