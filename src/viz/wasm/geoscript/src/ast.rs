use std::{borrow::Borrow, cell::RefCell, ops::ControlFlow, ptr::addr_of, rc::Rc, str::FromStr};

use fxhash::FxHashMap;
use pest::{iterators::Pair, Parser};

use crate::{
  builtins::{
    add_impl, and_impl, bit_and_impl, bit_or_impl, div_impl, eq_impl,
    fn_defs::{fn_sigs, get_builtin_fn_sig_entry_ix, FnSignature},
    map_impl, mod_impl, mul_impl, neg_impl, neq_impl, not_impl, numeric_bool_op_impl, or_impl,
    pos_impl, sub_impl, BoolOp,
  },
  get_args, get_binop_def_ix, get_unop_def_ix, get_unop_return_ty, resolve_builtin_impl, ArgType,
  Callable, CapturedScope, Closure, EagerSeq, ErrorStack, EvalCtx, GSParser, GetArgsOutput,
  IntRange, PreResolvedSignature, Rule, Scope, Sym, Value, EMPTY_KWARGS, FUNCTION_ALIASES,
  PRATT_PARSER,
};

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
      (TypeName::Vec3, Value::Vec3(_)) => Ok(()),
      (TypeName::Bool, Value::Bool(_)) => Ok(()),
      (TypeName::Seq, Value::Sequence(_)) => Ok(()),
      (TypeName::Callable, Value::Callable(_)) => Ok(()),
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

  fn iter_idents<'a>(&'a self) -> Box<dyn Iterator<Item = Sym> + 'a> {
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
  fn inline_const_captures(&mut self, ctx: &EvalCtx, closure_scope: &mut ScopeTracker) -> bool {
    match self {
      Statement::Assignment { expr, .. } => expr.inline_const_captures(ctx, closure_scope),
      Statement::DestructureAssignment { lhs: _, rhs } => {
        rhs.inline_const_captures(ctx, closure_scope)
      }
      Statement::Expr(expr) => expr.inline_const_captures(ctx, closure_scope),
      Statement::Return { value } => {
        if let Some(expr) = value {
          expr.inline_const_captures(ctx, closure_scope)
        } else {
          false
        }
      }
      Statement::Break { value } => {
        if let Some(expr) = value {
          expr.inline_const_captures(ctx, closure_scope)
        } else {
          false
        }
      }
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
  fn inline_const_captures(&mut self, ctx: &EvalCtx, local_scope: &mut ScopeTracker<'_>) -> bool {
    match self {
      MapLiteralEntry::KeyValue { key: _, value } => value.inline_const_captures(ctx, local_scope),
      MapLiteralEntry::Splat { expr } => expr.inline_const_captures(ctx, local_scope),
    }
  }

  fn expr(&self) -> &Expr {
    match self {
      MapLiteralEntry::KeyValue { key: _, value } => value,
      MapLiteralEntry::Splat { expr } => expr,
    }
  }

  fn is_literal(&self) -> bool {
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
  },
  PrefixOp {
    op: PrefixOp,
    expr: Box<Expr>,
  },
  Range {
    start: Box<Expr>,
    end: Option<Box<Expr>>,
    inclusive: bool,
  },
  StaticFieldAccess {
    lhs: Box<Expr>,
    field: String,
  },
  FieldAccess {
    lhs: Box<Expr>,
    field: Box<Expr>,
  },
  Call(FunctionCall),
  Closure {
    params: Rc<Vec<ClosureArg>>,
    body: Rc<ClosureBody>,
    arg_placeholder_scope: FxHashMap<Sym, Value>,
    return_type_hint: Option<TypeName>,
  },
  Ident(Sym),
  ArrayLiteral(Vec<Expr>),
  MapLiteral {
    entries: Vec<MapLiteralEntry>,
  },
  Literal(Value),
  Conditional {
    cond: Box<Expr>,
    then: Box<Expr>,
    /// (cond, expr)
    else_if_exprs: Vec<(Expr, Expr)>,
    else_expr: Option<Box<Expr>>,
  },
  Block {
    statements: Vec<Statement>,
  },
}

impl Expr {
  pub fn as_literal(&self) -> Option<&Value> {
    match self {
      Expr::Literal(val) => Some(val),
      _ => None,
    }
  }

  pub fn is_literal(&self) -> bool {
    matches!(self, Expr::Literal(_))
  }

  fn inline_const_captures(&mut self, ctx: &EvalCtx, local_scope: &mut ScopeTracker) -> bool {
    match self {
      Expr::BinOp {
        op: _,
        lhs,
        rhs,
        pre_resolved_def_ix: _,
      } => {
        let mut captures_dyn = lhs.inline_const_captures(ctx, local_scope);
        captures_dyn |= rhs.inline_const_captures(ctx, local_scope);
        captures_dyn
      }
      Expr::PrefixOp { op: _, expr } => expr.inline_const_captures(ctx, local_scope),
      Expr::Range {
        start,
        end,
        inclusive: _,
      } => {
        let mut captures_dyn = start.inline_const_captures(ctx, local_scope);
        if let Some(end) = end {
          captures_dyn |= end.inline_const_captures(ctx, local_scope);
        }
        captures_dyn
      }
      Expr::StaticFieldAccess { lhs, field: _ } => lhs.inline_const_captures(ctx, local_scope),
      Expr::FieldAccess { lhs, field } => {
        let mut captures_dyn = lhs.inline_const_captures(ctx, local_scope);
        captures_dyn |= field.inline_const_captures(ctx, local_scope);
        captures_dyn
      }
      Expr::Call(FunctionCall {
        target,
        args,
        kwargs,
      }) => {
        // if all args are const and we can resolve the callable `name` references as const,
        // we can const-eval the call

        let mut captures_dyn = false;
        for arg in args.iter_mut() {
          captures_dyn |= arg.inline_const_captures(ctx, local_scope);
        }
        for kwarg in kwargs.values_mut() {
          captures_dyn |= kwarg.inline_const_captures(ctx, local_scope);
        }

        if captures_dyn {
          return true;
        }

        let name = match target {
          FunctionCallTarget::Name(name) => name,
          FunctionCallTarget::Literal(callable) => return callable.is_side_effectful(),
        };

        if let Some(TrackedValueRef::Const(val)) = local_scope.get(*name) {
          match val {
            Value::Callable(callable) => {
              *target = FunctionCallTarget::Literal(callable.clone());
              false
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
              callable.is_side_effectful()
            } else {
              true
            }
          })
        }
      }
      Expr::Closure {
        params,
        body,
        arg_placeholder_scope: _,
        return_type_hint: _,
      } => {
        let mut captures_dyn = false;

        let mut params_inner: Vec<_> = (**params).clone();
        for param in params_inner.iter_mut() {
          if let Some(default_val) = &mut param.default_val {
            captures_dyn |= default_val.inline_const_captures(ctx, local_scope);
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
        let body_captures_dyn = body_inner.inline_const_captures(ctx, &mut closure_scope);
        *body = Rc::new(body_inner);

        captures_dyn || body_captures_dyn
      }
      Expr::Ident(id) => match local_scope.vars.get(id) {
        Some(TrackedValue::Const(resolved)) => {
          *self = Expr::Literal(resolved.clone());
          false
        }
        // an arg in the local scope means it's an actual argument rather than a capture or
        // something from an outer scope
        Some(TrackedValue::Arg(_)) => false,
        Some(TrackedValue::Dyn { .. }) => true,
        None => match local_scope.parent {
          Some(parent) => match parent.get(*id) {
            Some(TrackedValueRef::Const(resolved)) => {
              *self = Expr::Literal(resolved.clone());
              false
            }
            // arg in the parent scope is dyn
            Some(TrackedValueRef::Arg(_)) => true,
            Some(TrackedValueRef::Dyn { .. }) => true,
            None => true,
          },
          None => true,
        },
      },
      Expr::ArrayLiteral(exprs) => exprs
        .iter_mut()
        .any(|expr| expr.inline_const_captures(ctx, local_scope)),
      Expr::MapLiteral { entries } => entries
        .iter_mut()
        .any(|expr| expr.inline_const_captures(ctx, local_scope)),
      Expr::Literal(_) => false,
      Expr::Conditional {
        cond,
        then,
        else_if_exprs,
        else_expr,
      } => {
        if cond.inline_const_captures(ctx, local_scope) {
          // TODO: should we really be early returning here?
          return true;
        }
        if then.inline_const_captures(ctx, local_scope) {
          return true;
        }
        for (cond, expr) in else_if_exprs {
          if cond.inline_const_captures(ctx, local_scope)
            || expr.inline_const_captures(ctx, local_scope)
          {
            return true;
          }
        }
        if let Some(else_expr) = else_expr {
          return else_expr.inline_const_captures(ctx, local_scope);
        }
        false
      }
      Expr::Block { statements } => statements
        .iter_mut()
        .any(|stmt| stmt.inline_const_captures(ctx, local_scope)),
    }
  }

  fn traverse(&self, cb: &mut impl FnMut(&Self)) {
    fn traverse_stmt(stmt: &Statement, cb: &mut impl FnMut(&Expr)) {
      match stmt {
        Statement::Assignment { expr, .. } => expr.traverse(cb),
        Statement::DestructureAssignment { lhs: _, rhs } => rhs.traverse(cb),
        Statement::Expr(expr) => expr.traverse(cb),
        Statement::Return { value } => {
          if let Some(expr) = value {
            expr.traverse(cb);
          }
        }
        Statement::Break { value } => {
          if let Some(expr) = value {
            expr.traverse(cb);
          }
        }
      }
    }

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
      Expr::FieldAccess { lhs, field } => {
        cb(self);
        lhs.traverse(cb);
        field.traverse(cb);
      }
      Expr::Call(call) => {
        cb(self);
        call.args.iter().for_each(|arg| arg.traverse(cb));
        call.kwargs.values().for_each(|kwarg| kwarg.traverse(cb));
      }
      Expr::Closure { body, .. } => {
        cb(self);
        body.0.iter().for_each(|stmt| traverse_stmt(stmt, cb));
      }
      Expr::Ident(_) | Expr::Literal(_) | Expr::ArrayLiteral(_) => {
        cb(self);
      }
      Expr::MapLiteral { entries } => {
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
      Expr::Block { statements } => {
        cb(self);
        for stmt in statements {
          traverse_stmt(stmt, cb);
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
  fn inline_const_captures(&mut self, ctx: &EvalCtx, closure_scope: &mut ScopeTracker) -> bool {
    let mut references_dyn_captures = false;
    for stmt in &mut self.0 {
      if stmt.inline_const_captures(ctx, closure_scope) {
        references_dyn_captures = true;
      }
      if let Statement::Assignment {
        name,
        expr,
        type_hint: _,
      } = stmt
      {
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
}

#[derive(Clone, Debug)]
pub enum FunctionCallTarget {
  // TODO: we should aim to phase out name and resolve callables during const eval
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

fn eval_range(start: &Value, end: Option<&Value>, inclusive: bool) -> Result<Value, ErrorStack> {
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

  Ok(Expr::Call(FunctionCall {
    target: FunctionCallTarget::Name(name),
    args,
    kwargs,
  }))
}

fn parse_node(ctx: &EvalCtx, expr: Pair<Rule>) -> Result<Expr, ErrorStack> {
  match expr.as_rule() {
    Rule::int => {
      let int_str = expr.as_str();
      int_str
        .parse::<i64>()
        .map(|i| Expr::Literal(Value::Int(i)))
        .map_err(|_| ErrorStack::new(format!("Invalid integer: {int_str}")))
    }
    Rule::hex_int => {
      let hex_str = expr.as_str();
      i64::from_str_radix(&hex_str[2..], 16)
        .map(|i| Expr::Literal(Value::Int(i)))
        .map_err(|_| ErrorStack::new(format!("Invalid hex integer: {hex_str}")))
    }
    Rule::float => {
      let float_str = expr.as_str();
      float_str
        .parse::<f32>()
        .map(|i| Expr::Literal(Value::Float(i)))
        .map_err(|_| ErrorStack::new(format!("Invalid float: {float_str}")))
    }
    Rule::ident => Ok(Expr::Ident(ctx.interned_symbols.intern(expr.as_str()))),
    Rule::term => {
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
        Rule::expr => {
          let expr = parse_expr(ctx, next)?;
          ClosureBody(vec![Statement::Expr(expr)])
        }
        Rule::until_eol_closure_body => {
          let pairs = GSParser::parse(Rule::standalone_expr, next.as_str()).map_err(|err| {
            ErrorStack::new(format!("{err}")).wrap("Syntax error while parsing single-line closure")
          })?;
          let Some(expr) = pairs.into_iter().next() else {
            return Err(ErrorStack::new("No expr found in input"));
          };
          let expr = expr.into_inner().next().unwrap();
          let expr = parse_expr(ctx, expr)?;
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
      })
    }
    Rule::array_literal => {
      let elems = expr
        .into_inner()
        .map(|e| parse_expr(ctx, e))
        .collect::<Result<Vec<_>, ErrorStack>>()?;
      Ok(Expr::ArrayLiteral(elems))
    }
    Rule::bool_literal => {
      let bool_str = expr.as_str();
      match bool_str {
        "true" => Ok(Expr::Literal(Value::Bool(true))),
        "false" => Ok(Expr::Literal(Value::Bool(false))),
        _ => unreachable!("Unexpected boolean literal: {bool_str}, expected 'true' or 'false'"),
      }
    }
    Rule::nil_literal => Ok(Expr::Literal(Value::Nil)),
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
      })
    }
    Rule::double_quote_string_literal => {
      let inner = expr.into_inner().next().unwrap();
      if inner.as_rule() != Rule::double_quote_string_inner {
        unreachable!();
      }
      let s = parse_double_quote_string_inner(inner)?;
      Ok(Expr::Literal(Value::String(s)))
    }
    Rule::single_quote_string_literal => {
      let inner = expr.into_inner().next().unwrap();
      if inner.as_rule() != Rule::single_quote_string_inner {
        unreachable!();
      }
      let s = parse_single_quote_string_inner(inner)?;
      Ok(Expr::Literal(Value::String(s)))
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

  let statements = expr
    .into_inner()
    .map(|stmt| parse_statement(ctx, stmt))
    .filter_map(|res| match res {
      Ok(Some(stmt)) => Some(Ok(stmt)),
      Ok(None) => None,
      Err(err) => Some(Err(err)),
    })
    .collect::<Result<Vec<_>, ErrorStack>>()?;

  Ok(Expr::Block { statements })
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
    .map_prefix(|op, expr| match op.as_rule() {
      Rule::neg_op => Ok(Expr::PrefixOp {
        op: PrefixOp::Neg,
        expr: Box::new(expr?),
      }),
      Rule::pos_op => Ok(Expr::PrefixOp {
        op: PrefixOp::Pos,
        expr: Box::new(expr?),
      }),
      Rule::negate_op => Ok(Expr::PrefixOp {
        op: PrefixOp::Not,
        expr: Box::new(expr?),
      }),
      _ => unreachable!("Unexpected prefix operator rule: {:?}", op.as_rule()),
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
      Ok(Expr::BinOp {
        op: bin_op,
        lhs: Box::new(lhs?),
        rhs: Box::new(rhs?),
        pre_resolved_def_ix: None,
      })
    })
    .map_postfix(|expr, op| {
      let expr = expr?;

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
          })
        }
        Rule::field_access => {
          let index_expr = parse_expr(ctx, inner.into_inner().next().unwrap())?;
          Ok(Expr::FieldAccess {
            lhs: Box::new(expr),
            field: Box::new(index_expr),
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
enum TrackedValue {
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
enum TrackedValueRef<'a> {
  Const(&'a Value),
  Arg(&'a ClosureArg),
  Dyn { type_hint: Option<TypeName> },
}

#[derive(Default, Debug)]
struct ScopeTracker<'a> {
  vars: FxHashMap<Sym, TrackedValue>,
  parent: Option<&'a ScopeTracker<'a>>,
}

impl<'a> ScopeTracker<'a> {
  pub fn wrap(parent: &'a ScopeTracker<'a>) -> Self {
    ScopeTracker {
      vars: FxHashMap::default(),
      parent: Some(parent),
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

fn pre_resolve_binop_def_ix(
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

fn pre_resolve_expr_type(
  ctx: &EvalCtx,
  scope_tracker: &ScopeTracker,
  arg: &Expr,
) -> Option<ArgType> {
  match arg {
    Expr::Literal(v) => Some(v.get_type()),
    Expr::Ident(id) => match scope_tracker.get(*id) {
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
    } => {
      match op {
        BinOp::Range | BinOp::RangeInclusive => return Some(ArgType::Sequence),
        BinOp::Pipeline => {
          let rhs_ty = pre_resolve_expr_type(ctx, scope_tracker, rhs)?;
          if matches!(rhs_ty, ArgType::Mesh) {
            return Some(ArgType::Mesh);
          }

          match &**rhs {
            Expr::Literal(Value::Callable(callable)) => match &**callable {
              Callable::Builtin {
                fn_entry_ix,
                fn_impl: _,
                pre_resolved_signature,
              } => match pre_resolved_signature {
                Some(sig) => {
                  let fn_signature_defs = &fn_sigs().entries[*fn_entry_ix].1.signatures;
                  let return_ty = fn_signature_defs[sig.def_ix].return_type;
                  if return_ty.len() == 1 {
                    return Some(return_ty[0]);
                  } else {
                    return None;
                  }
                }
                None => {
                  return None;
                }
              },
              Callable::PartiallyAppliedFn(_) => {
                // TODO: should eventually be able to resolve this
                return None;
              }
              Callable::Closure(Closure {
                return_type_hint, ..
              }) => match return_type_hint {
                Some(ty) => return Some((*ty).into()),
                None => return None,
              },
              Callable::ComposedFn(_) => return None,
            },
            _ => return None,
          }
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
    Expr::PrefixOp { op, expr } => {
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
    Expr::StaticFieldAccess { lhs, field } => {
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
    Expr::FieldAccess { lhs, field } => {
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
    Expr::Call(FunctionCall {
      target,
      args,
      kwargs,
    }) => match target {
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
        Callable::PartiallyAppliedFn(_) => None,
        Callable::Closure(closure) => closure.return_type_hint.map(Into::into),
        Callable::ComposedFn(_) => None,
      },
    },
    Expr::Closure { .. } => Some(ArgType::Callable),
    Expr::ArrayLiteral(_) => Some(ArgType::Sequence),
    Expr::MapLiteral { .. } => Some(ArgType::Map),
    Expr::Conditional { .. } => None,
    Expr::Block { .. } => None,
  }
}

fn maybe_pre_resolve_bulitin_call_signature(
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

fn optimize_expr<'a>(
  ctx: &EvalCtx,
  local_scope: &'a mut ScopeTracker,
  expr: &mut Expr,
) -> Result<(), ErrorStack> {
  fold_constants(ctx, local_scope, expr)
}

/// This helps differentiate between "true" dyn values (like the output from `randi()`, for example)
/// and dynamic values that depend only on constants and closure arguments.
///
/// This allows closures to be differentiated between those that are completely const and depend on
/// no non-const external values and those that can be const eval'd but depend on args.
#[derive(Clone, Copy, PartialEq, Debug)]
enum DynType {
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

fn get_dyn_type(expr: &Expr, local_scope: &ScopeTracker) -> DynType {
  match expr {
    Expr::BinOp {
      op: _,
      lhs,
      rhs,
      pre_resolved_def_ix: _,
    } => {
      let lhs_type = get_dyn_type(lhs, local_scope);
      let rhs_type = get_dyn_type(rhs, local_scope);
      lhs_type | rhs_type
    }
    Expr::PrefixOp { op: _, expr } => get_dyn_type(expr, local_scope),
    Expr::Range {
      start,
      end,
      inclusive: _,
    } => {
      let start_type = get_dyn_type(start, local_scope);
      if let Some(end) = end.as_ref() {
        let end_type = get_dyn_type(end, local_scope);
        start_type | end_type
      } else {
        start_type
      }
    }
    Expr::StaticFieldAccess { lhs, field: _ } => get_dyn_type(lhs, local_scope),
    Expr::FieldAccess { lhs, field } => {
      get_dyn_type(lhs, local_scope) | get_dyn_type(field, local_scope)
    }
    Expr::Call(FunctionCall {
      target: _,
      args,
      kwargs,
    }) => {
      let mut dyn_type = DynType::Const;
      for arg in args {
        dyn_type = dyn_type | get_dyn_type(arg, local_scope);
      }
      for kwarg in kwargs.values() {
        dyn_type = dyn_type | get_dyn_type(kwarg, local_scope);
      }

      dyn_type
    }
    Expr::Closure {
      params,
      body,
      arg_placeholder_scope: _,
      return_type_hint: _,
    } => {
      let mut dyn_type = DynType::Const;
      for param in &**params {
        if let Some(default_val) = &param.default_val {
          dyn_type = dyn_type | get_dyn_type(default_val, local_scope);
        } else {
          dyn_type = dyn_type | DynType::Arg;
        }
      }
      for stmt in &body.0 {
        match stmt {
          Statement::Expr(expr) => {
            dyn_type = dyn_type | get_dyn_type(expr, local_scope);
          }
          Statement::DestructureAssignment { lhs: _, rhs } => {
            dyn_type = dyn_type | get_dyn_type(rhs, local_scope);
          }
          Statement::Assignment { expr, .. } => {
            dyn_type = dyn_type | get_dyn_type(expr, local_scope);
          }
          Statement::Return { value } => {
            if let Some(value) = value {
              dyn_type = dyn_type | get_dyn_type(value, local_scope);
            }
          }
          Statement::Break { value } => {
            if let Some(value) = value {
              dyn_type = dyn_type | get_dyn_type(value, local_scope);
            }
          }
        }
      }
      dyn_type
    }
    Expr::Ident(name) => match local_scope.vars.get(name) {
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
    Expr::ArrayLiteral(exprs) => exprs.iter().fold(DynType::Const, |acc, expr| {
      acc | get_dyn_type(expr, local_scope)
    }),
    Expr::MapLiteral { entries } => entries.iter().fold(DynType::Const, |acc, entry| {
      acc | get_dyn_type(entry.expr(), local_scope)
    }),
    Expr::Literal(_) => DynType::Const,
    Expr::Conditional {
      cond,
      then,
      else_if_exprs,
      else_expr,
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
    Expr::Block { statements } => statements.iter().fold(DynType::Const, |acc, stmt| {
      let dyn_type = match stmt {
        Statement::Expr(expr) => get_dyn_type(expr, local_scope),
        Statement::DestructureAssignment { lhs: _, rhs } => get_dyn_type(rhs, local_scope),
        Statement::Assignment { expr, .. } => get_dyn_type(expr, local_scope),
        Statement::Return { value } => {
          if let Some(value) = value {
            get_dyn_type(value, local_scope)
          } else {
            DynType::Const
          }
        }
        Statement::Break { value } => {
          if let Some(value) = value {
            get_dyn_type(value, local_scope)
          } else {
            DynType::Const
          }
        }
      };
      acc | dyn_type
    }),
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

pub fn optimize_ast(ctx: &EvalCtx, ast: &mut Program) -> Result<(), ErrorStack> {
  let mut local_scope = ScopeTracker::default();
  for stmt in &mut ast.statements {
    optimize_statement(ctx, &mut local_scope, stmt)?;
  }
  Ok(())
}

/// This doesn't disambiguate between builtin fn calls and calling user-defined functions.
pub fn traverse_fn_calls(program: &Program, mut cb: impl FnMut(Sym)) {
  let mut cb = move |expr: &Expr| {
    if let Expr::Call(FunctionCall {
      target: FunctionCallTarget::Name(name),
      ..
    }) = expr
    {
      cb(*name)
    }
  };

  for stmt in &program.statements {
    match stmt {
      Statement::Expr(expr) => expr.traverse(&mut cb),
      Statement::Assignment { expr, .. } => expr.traverse(&mut cb),
      Statement::DestructureAssignment { lhs: _, rhs } => rhs.traverse(&mut cb),
      Statement::Return { value } => {
        if let Some(expr) = value {
          expr.traverse(&mut cb);
        }
      }
      Statement::Break { value } => {
        if let Some(expr) = value {
          expr.traverse(&mut cb);
        }
      }
    }
  }
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
  optimize_expr(&ctx, &mut local_scope, &mut expr).unwrap();
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
  | alpha_wrap(alpha=1/140, offset=1/250)
  | render"#;

  let ctx = EvalCtx::default();
  let mut ast = crate::parse_program_src(&ctx, src).unwrap();
  optimize_ast(&ctx, &mut ast).unwrap();

  let st1 = ast.statements[0].clone();
  let expr = match st1 {
    Statement::Expr(expr) => expr,
    _ => unreachable!(),
  };
  let expr = match expr {
    Expr::BinOp {
      op: BinOp::Pipeline,
      lhs,
      rhs: _, // render
      pre_resolved_def_ix: _,
    } => (*lhs).clone(),
    _ => unreachable!(),
  };
  let expr = match expr {
    Expr::BinOp {
      op: BinOp::Pipeline,
      lhs,
      rhs: _, // alpha_wrap
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
