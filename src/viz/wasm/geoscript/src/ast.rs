use std::{rc::Rc, str::FromStr};

use fxhash::FxHashMap;
use itertools::Itertools;
use pest::iterators::Pair;

use crate::{
  builtins::{
    add_impl, and_impl, bit_and_impl, bit_or_impl, div_impl, eq_impl, fn_defs::FnSignature,
    map_impl, mod_impl, mul_impl, neg_impl, neq_impl, not_impl, numeric_bool_op_impl, or_impl,
    pos_impl, sub_impl, BoolOp,
  },
  get_args, get_binop_def_ix, get_binop_return_ty, get_unop_def_ix, get_unop_return_ty,
  resolve_builtin_impl, ArgType, Callable, Closure, EagerSeq, ErrorStack, EvalCtx, GetArgsOutput,
  IntRange, PreResolvedSignature, Rule, Scope, Value, FN_SIGNATURE_DEFS, FUNCTION_ALIASES,
  PRATT_PARSER,
};

#[derive(Debug)]
pub struct Program {
  pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, Copy)]
pub enum TypeName {
  Mesh,
  Int,
  Float,
  Num,
  Vec3,
  Bool,
  String,
  Seq,
  Callable,
  Nil,
}

impl Into<ArgType> for TypeName {
  fn into(self) -> ArgType {
    match self {
      TypeName::Mesh => ArgType::Mesh,
      TypeName::Int => ArgType::Int,
      TypeName::Float => ArgType::Float,
      TypeName::Num => ArgType::Numeric,
      TypeName::Vec3 => ArgType::Vec3,
      TypeName::Bool => ArgType::Bool,
      TypeName::String => ArgType::String,
      TypeName::Seq => ArgType::Sequence,
      TypeName::Callable => ArgType::Callable,
      TypeName::Nil => ArgType::Nil,
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
      "vec3" => Ok(TypeName::Vec3),
      "bool" => Ok(TypeName::Bool),
      "str" | "string" => Ok(TypeName::String),
      "seq" | "sequence" => Ok(TypeName::Seq),
      "fn" | "callable" => Ok(TypeName::Callable),
      "nil" => Ok(TypeName::Nil),
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
      (TypeName::Nil, Value::Nil) => Ok(()),
      _ => Err(ErrorStack::new(format!(
        "Value {val:?} does not match type {self:?}"
      ))),
    }
  }
}

#[derive(Clone, Debug)]
pub enum Statement {
  Assignment {
    name: String,
    expr: Expr,
    type_hint: Option<TypeName>,
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
  fn inline_const_captures(&mut self, closure_scope: &mut ScopeTracker) -> bool {
    match self {
      Statement::Assignment { expr, .. } => expr.inline_const_captures(closure_scope),
      Statement::Expr(expr) => expr.inline_const_captures(closure_scope),
      Statement::Return { value } => {
        if let Some(expr) = value {
          expr.inline_const_captures(closure_scope)
        } else {
          false
        }
      }
      Statement::Break { value } => {
        if let Some(expr) = value {
          expr.inline_const_captures(closure_scope)
        } else {
          false
        }
      }
    }
  }
}

#[derive(Clone, Debug)]
pub struct ClosureArg {
  pub name: String,
  pub type_hint: Option<TypeName>,
  pub default_val: Option<Expr>,
}

#[derive(Clone, Debug)]
pub enum Expr {
  BinOp {
    op: BinOp,
    lhs: Box<Expr>,
    rhs: Box<Expr>,
  },
  PrefixOp {
    op: PrefixOp,
    expr: Box<Expr>,
  },
  Range {
    start: Box<Expr>,
    end: Box<Expr>,
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
    params: Vec<ClosureArg>,
    body: ClosureBody,
    return_type_hint: Option<TypeName>,
  },
  Ident(String),
  ArrayLiteral(Vec<Expr>),
  MapLiteral(FxHashMap<String, Expr>),
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
  pub fn as_literal(&self) -> Option<Value> {
    match self {
      Expr::Literal(val) => Some(val.clone()),
      _ => None,
    }
  }

  fn inline_const_captures(&mut self, local_scope: &mut ScopeTracker) -> bool {
    match self {
      Expr::BinOp { op: _, lhs, rhs } => {
        let mut captures_dyn = lhs.inline_const_captures(local_scope);
        captures_dyn |= rhs.inline_const_captures(local_scope);
        captures_dyn
      }
      Expr::PrefixOp { op: _, expr } => expr.inline_const_captures(local_scope),
      Expr::Range {
        start,
        end,
        inclusive: _,
      } => {
        let mut captures_dyn = start.inline_const_captures(local_scope);
        captures_dyn |= end.inline_const_captures(local_scope);
        captures_dyn
      }
      Expr::StaticFieldAccess { lhs, field: _ } => lhs.inline_const_captures(local_scope),
      Expr::FieldAccess { lhs, field } => {
        let mut captures_dyn = lhs.inline_const_captures(local_scope);
        captures_dyn |= field.inline_const_captures(local_scope);
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
          captures_dyn |= arg.inline_const_captures(local_scope);
        }
        for kwarg in kwargs.values_mut() {
          captures_dyn |= kwarg.inline_const_captures(local_scope);
        }

        if captures_dyn {
          return true;
        }

        let name = match target {
          FunctionCallTarget::Name(name) => name,
          FunctionCallTarget::Literal(_) => {
            // if the target is a literal callable, we can const-eval it
            return false;
          }
        };

        if let Some(TrackedValueRef::Const(val)) = local_scope.get(name) {
          match val {
            Value::Callable(callable) => {
              *target = FunctionCallTarget::Literal(callable.clone());
              false
            }
            _ => false,
          }
        } else if FN_SIGNATURE_DEFS.contains_key(name) || FUNCTION_ALIASES.contains_key(name) {
          let callable = Callable::Builtin {
            name: name.clone(),
            fn_impl: |_, _, _, _, _| unreachable!(),
            fn_signature_defs: &[],
            pre_resolved_signature: None,
          };
          callable.is_side_effectful()
        } else {
          true
        }
      }
      Expr::Closure {
        params,
        body,
        return_type_hint: _,
      } => {
        let mut captures_dyn = false;

        for param in params.iter_mut() {
          if let Some(default_val) = &mut param.default_val {
            captures_dyn |= default_val.inline_const_captures(local_scope);
          }
        }

        let mut closure_scope = ScopeTracker::wrap(local_scope);
        for param in params.iter() {
          closure_scope.set(param.name.clone(), TrackedValue::Arg(param.clone()));
        }

        captures_dyn || body.inline_const_captures(&mut closure_scope)
      }
      Expr::Ident(id) => match local_scope.vars.get(id) {
        Some(TrackedValue::Const(resolved)) => {
          *self = Expr::Literal(resolved.clone());
          false
        }
        // an arg in the local scope means it's an actual argument rather than a capture or
        // something from an outer scope
        Some(TrackedValue::Arg(_)) => false,
        None => match local_scope.parent {
          Some(parent) => match parent.get(id) {
            Some(TrackedValueRef::Const(resolved)) => {
              *self = Expr::Literal(resolved.clone());
              false
            }
            Some(TrackedValueRef::Arg(_)) => true,
            None => true,
          },
          None => true,
        },
      },
      Expr::ArrayLiteral(exprs) => exprs
        .iter_mut()
        .any(|expr| expr.inline_const_captures(local_scope)),
      Expr::MapLiteral(map) => map
        .values_mut()
        .any(|expr| expr.inline_const_captures(local_scope)),
      Expr::Literal(_) => false,
      Expr::Conditional {
        cond,
        then,
        else_if_exprs,
        else_expr,
      } => {
        if cond.inline_const_captures(local_scope) {
          return true;
        }
        if then.inline_const_captures(local_scope) {
          return true;
        }
        for (cond, expr) in else_if_exprs {
          if cond.inline_const_captures(local_scope) || expr.inline_const_captures(local_scope) {
            return true;
          }
        }
        if let Some(else_expr) = else_expr {
          return else_expr.inline_const_captures(local_scope);
        }
        false
      }
      Expr::Block { statements } => statements
        .iter_mut()
        .any(|stmt| stmt.inline_const_captures(local_scope)),
    }
  }

  fn traverse(&self, cb: &mut impl FnMut(&Self)) {
    fn traverse_stmt(stmt: &Statement, cb: &mut impl FnMut(&Expr)) {
      match stmt {
        Statement::Assignment { expr, .. } => expr.traverse(cb),
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
        end.traverse(cb);
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
      Expr::Ident(_) | Expr::Literal(_) | Expr::ArrayLiteral(_) | Expr::MapLiteral(_) => {
        cb(self);
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
  fn inline_const_captures(&mut self, closure_scope: &mut ScopeTracker) -> bool {
    let mut references_dyn_captures = false;
    for stmt in &mut self.0 {
      if stmt.inline_const_captures(closure_scope) {
        references_dyn_captures = true;
      }
    }

    references_dyn_captures
  }
}

#[derive(Clone, Debug)]
pub enum FunctionCallTarget {
  // TODO: we should aim to phase out name and resolve callables during const eval
  Name(String),
  Literal(Rc<Callable>),
}

#[derive(Clone, Debug)]
pub struct FunctionCall {
  pub target: FunctionCallTarget,
  pub args: Vec<Expr>,
  pub kwargs: FxHashMap<String, Expr>,
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

fn eval_range(start: Value, end: Value, inclusive: bool) -> Result<Value, ErrorStack> {
  let Value::Int(start) = start else {
    return Err(ErrorStack::new(format!(
      "Range start must be an integer, found: {start:?}",
    )));
  };
  let Value::Int(end) = end else {
    return Err(ErrorStack::new(format!(
      "Range end must be an integer, found: {end:?}",
    )));
  };

  let mut range = IntRange { start, end };
  if inclusive {
    range.end += 1;
  }

  Ok(Value::Sequence(Box::new(range)))
}

// TODO: should do more efficient version of this
lazy_static::lazy_static! {
  static ref ADD_ARG_DEFS: &'static [FnSignature] = &FN_SIGNATURE_DEFS["add"].signatures;
  static ref SUB_ARG_DEFS: &'static [FnSignature] = &FN_SIGNATURE_DEFS["sub"].signatures;
  static ref MUL_ARG_DEFS: &'static [FnSignature] = &FN_SIGNATURE_DEFS["mul"].signatures;
  static ref DIV_ARG_DEFS: &'static [FnSignature] = &FN_SIGNATURE_DEFS["div"].signatures;
  static ref MOD_ARG_DEFS: &'static [FnSignature] = &FN_SIGNATURE_DEFS["mod"].signatures;
  static ref GT_ARG_DEFS: &'static [FnSignature] = &FN_SIGNATURE_DEFS["gt"].signatures;
  static ref LT_ARG_DEFS: &'static [FnSignature] = &FN_SIGNATURE_DEFS["lt"].signatures;
  static ref GTE_ARG_DEFS: &'static [FnSignature] = &FN_SIGNATURE_DEFS["gte"].signatures;
  static ref LTE_ARG_DEFS: &'static [FnSignature] = &FN_SIGNATURE_DEFS["lte"].signatures;
  static ref EQ_ARG_DEFS: &'static [FnSignature] = &FN_SIGNATURE_DEFS["eq"].signatures;
  static ref NEQ_ARG_DEFS: &'static [FnSignature] = &FN_SIGNATURE_DEFS["neq"].signatures;
}

impl BinOp {
  pub fn apply(&self, ctx: &EvalCtx, lhs: Value, rhs: Value) -> Result<Value, ErrorStack> {
    match self {
      BinOp::Add => {
        let def_ix = get_binop_def_ix("add", &*ADD_ARG_DEFS, &lhs, &rhs)?;
        add_impl(def_ix, &lhs, &rhs)
      }
      BinOp::Sub => {
        let def_ix = get_binop_def_ix("sub", &*SUB_ARG_DEFS, &lhs, &rhs)?;
        sub_impl(ctx, def_ix, &lhs, &rhs)
      }
      BinOp::Mul => {
        let def_ix = get_binop_def_ix("mul", &*MUL_ARG_DEFS, &lhs, &rhs)?;
        mul_impl(def_ix, &lhs, &rhs)
      }
      BinOp::Div => {
        let def_ix = get_binop_def_ix("div", &*DIV_ARG_DEFS, &lhs, &rhs)?;
        div_impl(def_ix, &lhs, &rhs)
      }
      BinOp::Mod => {
        let def_ix = get_binop_def_ix("mod", &*MOD_ARG_DEFS, &lhs, &rhs)?;
        mod_impl(def_ix, &lhs, &rhs)
      }
      BinOp::Gt => {
        let def_ix = get_binop_def_ix("gt", &*GT_ARG_DEFS, &lhs, &rhs)?;
        numeric_bool_op_impl::<{ BoolOp::Gt }>(def_ix, &lhs, &rhs)
      }
      BinOp::Lt => {
        let def_ix = get_binop_def_ix("lt", &*LT_ARG_DEFS, &lhs, &rhs)?;
        numeric_bool_op_impl::<{ BoolOp::Lt }>(def_ix, &lhs, &rhs)
      }
      BinOp::Gte => {
        let def_ix = get_binop_def_ix("gte", &*GTE_ARG_DEFS, &lhs, &rhs)?;
        numeric_bool_op_impl::<{ BoolOp::Gte }>(def_ix, &lhs, &rhs)
      }
      BinOp::Lte => {
        let def_ix = get_binop_def_ix("lte", &*LTE_ARG_DEFS, &lhs, &rhs)?;
        numeric_bool_op_impl::<{ BoolOp::Lte }>(def_ix, &lhs, &rhs)
      }
      BinOp::Eq => {
        let def_ix = get_binop_def_ix("eq", &*EQ_ARG_DEFS, &lhs, &rhs)?;
        eq_impl(def_ix, &lhs, &rhs)
      }
      BinOp::Neq => {
        let def_ix = get_binop_def_ix("neq", &*NEQ_ARG_DEFS, &lhs, &rhs)?;
        neq_impl(def_ix, &lhs, &rhs)
      }
      BinOp::And => {
        let def_ix = get_binop_def_ix("and", &FN_SIGNATURE_DEFS["and"].signatures, &lhs, &rhs)?;
        and_impl(ctx, def_ix, &lhs, &rhs)
      }
      BinOp::Or => {
        let def_ix = get_binop_def_ix("or", &FN_SIGNATURE_DEFS["or"].signatures, &lhs, &rhs)?;
        or_impl(ctx, def_ix, &lhs, &rhs)
      }
      BinOp::BitAnd => {
        let def_ix = get_binop_def_ix(
          "bit_and",
          &FN_SIGNATURE_DEFS["bit_and"].signatures,
          &lhs,
          &rhs,
        )?;
        bit_and_impl(ctx, def_ix, &lhs, &rhs)
      }
      BinOp::Range => eval_range(lhs, rhs, false),
      BinOp::RangeInclusive => eval_range(lhs, rhs, true),
      BinOp::Pipeline => {
        // eval as a pipeline operator if the rhs is a callable
        if let Some(callable) = rhs.as_callable() {
          return ctx
            .invoke_callable(callable, &[lhs], &Default::default(), &ctx.globals)
            .map_err(|err| err.wrap("Error invoking callable in pipeline".to_owned()));
        }

        // maybe it's a bit-or
        let def_ix = get_binop_def_ix(
          "bit_or",
          &FN_SIGNATURE_DEFS["bit_or"].signatures,
          &lhs,
          &rhs,
        )?;
        bit_or_impl(ctx, def_ix, &lhs, &rhs)
      }
      BinOp::Map => {
        // this operator acts the same as `lhs | map(rhs)`
        let def_ix = get_binop_def_ix("map", &FN_SIGNATURE_DEFS["map"].signatures, &rhs, &lhs)?;
        map_impl(ctx, def_ix, &rhs, &lhs)
      }
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixOp {
  Neg,
  Pos,
  Not,
}

impl PrefixOp {
  pub fn apply(&self, val: Value) -> Result<Value, ErrorStack> {
    match self {
      PrefixOp::Neg => {
        let def_ix = get_unop_def_ix("neg", &FN_SIGNATURE_DEFS["neg"].signatures, &val)?;
        neg_impl(def_ix, &val)
      }
      PrefixOp::Pos => {
        let def_ix = get_unop_def_ix("pos", &FN_SIGNATURE_DEFS["pos"].signatures, &val)?;
        pos_impl(def_ix, &val)
      }
      PrefixOp::Not => {
        let def_ix = get_unop_def_ix("not", &FN_SIGNATURE_DEFS["not"].signatures, &val)?;
        not_impl(def_ix, &val)
      }
    }
  }
}

fn parse_fn_call(func_call: Pair<Rule>) -> Result<Expr, ErrorStack> {
  if func_call.as_rule() != Rule::func_call {
    return Err(ErrorStack::new(format!(
      "`parse_func_call` can only handle `func_call` rules, found: {:?}",
      func_call.as_rule()
    )));
  }

  let mut inner = func_call.into_inner();
  let name = inner.next().unwrap().as_str().to_owned();

  let mut args: Vec<Expr> = Vec::new();
  let mut kwargs: FxHashMap<String, Expr> = FxHashMap::default();
  for arg in inner {
    let arg = arg.into_inner().next().unwrap();
    match arg.as_rule() {
      Rule::keyword_arg => {
        let mut inner = arg.into_inner();
        let id = inner.next().unwrap().as_str().to_owned();
        let value = inner.next().unwrap();
        let value_expr = parse_node(value)?;
        kwargs.insert(id, value_expr);
      }
      Rule::expr => {
        let expr = parse_expr(arg)?;
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

fn parse_node(expr: Pair<Rule>) -> Result<Expr, ErrorStack> {
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
    Rule::ident => Ok(Expr::Ident(expr.as_str().to_owned())),
    Rule::term => {
      let inner = expr.into_inner().next().unwrap();
      parse_node(inner)
    }
    Rule::func_call => parse_fn_call(expr),
    Rule::expr => parse_expr(expr),
    Rule::range_literal_expr => {
      let mut inner = expr.into_inner();
      let start = parse_node(inner.next().unwrap())?;
      let end = parse_node(inner.next().unwrap())?;
      Ok(Expr::Range {
        start: Box::new(start),
        end: Box::new(end),
        inclusive: false,
      })
    }
    Rule::range_inclusive_literal_expr => {
      let mut inner = expr.into_inner();
      let start = parse_node(inner.next().unwrap())?;
      let end = parse_node(inner.next().unwrap())?;
      Ok(Expr::Range {
        start: Box::new(start),
        end: Box::new(end),
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
          let name = inner.next().unwrap().as_str().to_owned();
          let (type_hint, default_val) = if let Some(next) = inner.next() {
            let mut type_hint = None;
            let mut default_val = None;
            if next.as_rule() == Rule::type_hint {
              type_hint = Some(
                TypeName::from_str(next.into_inner().next().unwrap().as_str()).map_err(|err| {
                  ErrorStack::new(err).wrap(format!("Invalid type hint for closure arg {name}"))
                })?,
              );
              if let Some(next) = inner.next() {
                if next.as_rule() == Rule::fn_arg_default_val {
                  default_val = Some(parse_node(next.into_inner().next().unwrap())?);
                } else {
                  unreachable!()
                }
              }
            } else if next.as_rule() == Rule::fn_arg_default_val {
              default_val = Some(parse_node(next.into_inner().next().unwrap())?);
            } else {
              unreachable!()
            }

            (type_hint, default_val)
          } else {
            (None, None)
          };
          Ok(ClosureArg {
            name,
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
          let expr = parse_expr(next)?;
          let stmt = Statement::Expr(expr);
          ClosureBody(vec![stmt])
        }
        Rule::bracketed_closure_body => {
          let stmts = next
            .into_inner()
            .map(parse_statement)
            .filter_map_ok(|s| s)
            .collect::<Result<Vec<_>, ErrorStack>>()?;

          ClosureBody(stmts)
        }
        _ => unreachable!("Unexpected closure body rule"),
      };

      Ok(Expr::Closure {
        params,
        body,
        return_type_hint,
      })
    }
    Rule::array_literal => {
      let elems = expr
        .into_inner()
        .map(parse_expr)
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
      let cond = parse_expr(inner.next().unwrap())?;
      let then = parse_expr(inner.next().unwrap())?;
      let (else_if_exprs, else_expr): (Vec<(Expr, Expr)>, Option<_>) = 'others: {
        let Some(next) = inner.next() else {
          break 'others (Vec::new(), None);
        };
        let mut else_if_exprs = Vec::new();

        match next.as_rule() {
          Rule::else_if_expr => {
            let mut else_if_inner = next.into_inner();
            let cond = parse_expr(else_if_inner.next().unwrap())?;
            let then = parse_expr(else_if_inner.next().unwrap())?;
            else_if_exprs.push((cond, then));
          }
          Rule::else_expr => {
            let else_expr = parse_expr(next.into_inner().next().unwrap())?;
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
              let cond = parse_expr(else_if_inner.next().unwrap())?;
              let then = parse_expr(else_if_inner.next().unwrap())?;
              else_if_exprs.push((cond, then));
            }
            Rule::else_expr => {
              let else_expr = parse_expr(next.into_inner().next().unwrap())?;
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
      let mut map = FxHashMap::default();
      for entry in entries {
        assert_eq!(entry.as_rule(), Rule::map_entry);
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
        let value_expr = parse_expr(value)?;
        map.insert(key, value_expr);
      }

      Ok(Expr::MapLiteral(map))
    }
    Rule::block_expr => parse_block_expr(expr),
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

fn parse_block_expr(expr: Pair<Rule>) -> Result<Expr, ErrorStack> {
  if expr.as_rule() != Rule::block_expr {
    return Err(ErrorStack::new(format!(
      "`parse_block_expr` can only handle `block_expr` rules, found: {:?}",
      expr.as_rule()
    )));
  }

  let statements = expr
    .into_inner()
    .map(parse_statement)
    .filter_map_ok(|s| s)
    .collect::<Result<Vec<_>, ErrorStack>>()?;

  Ok(Expr::Block { statements })
}

pub fn parse_expr(expr: Pair<Rule>) -> Result<Expr, ErrorStack> {
  if expr.as_rule() == Rule::block_expr {
    return parse_block_expr(expr);
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
        Rule::term => parse_node(primary),
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
        Rule::and_op | Rule::bit_and_op => BinOp::And,
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
          let index_expr = parse_expr(inner.into_inner().next().unwrap())?;
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

fn parse_assignment(assignment: Pair<Rule>) -> Result<Statement, ErrorStack> {
  if assignment.as_rule() != Rule::assignment {
    return Err(ErrorStack::new(format!(
      "`parse_assignment` can only handle `assignment` rules, found: {:?}",
      assignment.as_rule()
    )));
  }

  let mut inner = assignment.into_inner();
  let name = inner.next().unwrap().as_str().to_owned();

  let mut next = inner.next().unwrap();
  let type_hint = if next.as_rule() == Rule::type_hint {
    let type_hint =
      TypeName::from_str(next.into_inner().next().unwrap().as_str()).map_err(ErrorStack::new)?;
    next = inner.next().unwrap();
    Some(type_hint)
  } else {
    None
  };

  let expr = parse_expr(next)?;

  Ok(Statement::Assignment {
    name,
    expr,
    type_hint,
  })
}

fn parse_return_statement(return_stmt: Pair<Rule>) -> Result<Statement, ErrorStack> {
  if return_stmt.as_rule() != Rule::return_statement {
    return Err(ErrorStack::new(format!(
      "`parse_return_statement` can only handle `return_statement` rules, found: {:?}",
      return_stmt.as_rule()
    )));
  }

  let mut inner = return_stmt.into_inner();
  let value = if let Some(expr) = inner.next() {
    Some(parse_expr(expr)?)
  } else {
    None
  };

  Ok(Statement::Return { value })
}

fn parse_break_statement(return_stmt: Pair<Rule>) -> Result<Statement, ErrorStack> {
  if return_stmt.as_rule() != Rule::break_statement {
    return Err(ErrorStack::new(format!(
      "`parse_break_statement` can only handle `break_statement` rules, found: {:?}",
      return_stmt.as_rule()
    )));
  }

  let mut inner = return_stmt.into_inner();
  let value = if let Some(expr) = inner.next() {
    Some(parse_expr(expr)?)
  } else {
    None
  };

  Ok(Statement::Break { value })
}

pub(crate) fn parse_statement(stmt: Pair<Rule>) -> Result<Option<Statement>, ErrorStack> {
  match stmt.as_rule() {
    Rule::assignment => parse_assignment(stmt).map(Some),
    Rule::expr => Ok(Some(Statement::Expr(parse_expr(stmt)?))),
    Rule::return_statement => Ok(Some(parse_return_statement(stmt)?)),
    Rule::break_statement => Ok(Some(parse_break_statement(stmt)?)),
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
}

impl TrackedValue {
  pub fn as_ref<'a>(&'a self) -> TrackedValueRef<'a> {
    match self {
      TrackedValue::Const(val) => TrackedValueRef::Const(val),
      TrackedValue::Arg(arg) => TrackedValueRef::Arg(arg),
    }
  }
}

#[derive(Debug)]
enum TrackedValueRef<'a> {
  Const(&'a Value),
  Arg(&'a ClosureArg),
}

#[derive(Default, Debug)]
struct ScopeTracker<'a> {
  vars: FxHashMap<String, TrackedValue>,
  parent: Option<&'a ScopeTracker<'a>>,
}

impl<'a> ScopeTracker<'a> {
  pub fn wrap(parent: &'a ScopeTracker<'a>) -> Self {
    ScopeTracker {
      vars: FxHashMap::default(),
      parent: Some(parent),
    }
  }

  pub fn get<'b>(&'b self, name: &str) -> Option<TrackedValueRef<'b>> {
    if let Some(val) = self.vars.get(name) {
      return Some(val.as_ref());
    }
    if let Some(parent) = self.parent {
      return parent.get(name);
    }
    None
  }

  pub fn set(&mut self, name: String, value: TrackedValue) {
    self.vars.insert(name, value);
  }
}

fn pre_resolve_expr_type(
  ctx: &EvalCtx,
  scope_tracker: &ScopeTracker,
  arg: &Expr,
) -> Option<ArgType> {
  match arg {
    Expr::Literal(v) => Some(v.get_type()),
    Expr::Ident(id) => match scope_tracker.get(id) {
      Some(TrackedValueRef::Const(val)) => Some(val.get_type()),
      Some(TrackedValueRef::Arg(arg)) => {
        if let Some(type_hint) = &arg.type_hint {
          Some((*type_hint).into())
        } else {
          return None;
        }
      }
      None => {
        // error will happen later
        return None;
      }
    },
    Expr::BinOp { op, lhs, rhs } => {
      let builtin_name = match op {
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
        BinOp::Range => return Some(ArgType::Sequence),
        BinOp::RangeInclusive => return Some(ArgType::Sequence),
        BinOp::Pipeline => {
          let rhs_ty = pre_resolve_expr_type(ctx, scope_tracker, rhs)?;
          match rhs_ty {
            ArgType::Mesh => return Some(ArgType::Mesh),
            _ => (),
          }

          match &**rhs {
            Expr::Literal(Value::Callable(callable)) => match &**callable {
              Callable::Builtin {
                name: _,
                fn_impl: _,
                pre_resolved_signature,
                fn_signature_defs,
              } => match pre_resolved_signature {
                Some(sig) => {
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
      };
      let builtin_arg_defs = FN_SIGNATURE_DEFS[builtin_name].signatures;

      let lhs_ty = pre_resolve_expr_type(ctx, scope_tracker, lhs)?;
      let rhs_ty = pre_resolve_expr_type(ctx, scope_tracker, rhs)?;
      let return_tys = get_binop_return_ty(
        builtin_name,
        builtin_arg_defs,
        &lhs_ty.build_example_val()?,
        &rhs_ty.build_example_val()?,
      )
      .ok()?;
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
        PrefixOp::Neg => {
          get_unop_return_ty("neg", &FN_SIGNATURE_DEFS["neg"].signatures, &example_val)
        }
        PrefixOp::Pos => {
          get_unop_return_ty("pos", &FN_SIGNATURE_DEFS["pos"].signatures, &example_val)
        }
        PrefixOp::Not => {
          get_unop_return_ty("not", &FN_SIGNATURE_DEFS["not"].signatures, &example_val)
        }
      };
      match return_ty_res {
        Ok(return_tys) => {
          if return_tys.len() == 1 {
            if return_tys[0] == ArgType::Any {
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
      let out_val = match ctx.eval_static_field_access(lhs_val, field) {
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
      if out_ty == ArgType::Any {
        return None;
      }
      Some(out_ty)
    }
    Expr::FieldAccess { lhs, field } => {
      let lhs_ty = pre_resolve_expr_type(ctx, scope_tracker, lhs)?;
      let lhs_val = lhs_ty.build_example_val()?;
      let field_ty = pre_resolve_expr_type(ctx, scope_tracker, field)?;
      let field_val = field_ty.build_example_val()?;
      let out_val = match ctx.eval_field_access(lhs_val, field_val) {
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
      if out_ty == ArgType::Any {
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
          name,
          pre_resolved_signature,
          fn_signature_defs,
          ..
        } => {
          let return_ty = if let Some(sig) = pre_resolved_signature {
            fn_signature_defs[sig.def_ix].return_type
          } else {
            match maybe_pre_resolve_bulitin_call_signature(ctx, scope_tracker, name, args, kwargs) {
              Ok(Some(sig)) => fn_signature_defs[sig.def_ix].return_type,
              Ok(None) => return None,
              Err(_) => return None,
            }
          };
          match return_ty.len() {
            0 => None,
            1 => {
              if return_ty[0] != ArgType::Any {
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
    Expr::MapLiteral(_) => Some(ArgType::Map),
    Expr::Conditional { .. } => None,
    Expr::Block { .. } => None,
  }
}

fn maybe_pre_resolve_bulitin_call_signature(
  ctx: &EvalCtx,
  scope_tracker: &ScopeTracker,
  name: &str,
  args: &[Expr],
  kwargs: &FxHashMap<String, Expr>,
) -> Result<Option<PreResolvedSignature>, ErrorStack> {
  let mut arg_tys: Vec<ArgType> = Vec::with_capacity(args.len());
  for arg in args {
    let Some(ty) = pre_resolve_expr_type(ctx, scope_tracker, arg) else {
      return Ok(None);
    };
    arg_tys.push(ty);
  }

  let mut kwarg_tys: FxHashMap<String, ArgType> =
    FxHashMap::with_capacity_and_hasher(kwargs.len(), Default::default());
  for (name, expr) in kwargs {
    let Some(ty) = pre_resolve_expr_type(ctx, scope_tracker, expr) else {
      return Ok(None);
    };
    kwarg_tys.insert(name.clone(), ty);
  }

  // in order to re-use `get_args` code, we create fake args and kwargs of the types we're expecting
  let Ok(args) = arg_tys
    .into_iter()
    .map(|ty| Ok(ty.build_example_val().ok_or(())?))
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

  let sigs = match FN_SIGNATURE_DEFS.get(name) {
    Some(defs) => &defs.signatures,
    None => match FUNCTION_ALIASES.get(name) {
      Some(alias) => {
        &FN_SIGNATURE_DEFS
          .get(alias)
          .unwrap_or_else(|| {
            panic!("Alias {name} points to unknown function {alias}");
          })
          .signatures
      }
      None => {
        return Err(ErrorStack::new(format!(
          "Variable or function not found: {name}",
        )))
      }
    },
  };

  let resolved_sig = get_args(name, sigs, &args, &kwargs)?;
  match resolved_sig {
    GetArgsOutput::Valid { def_ix, arg_refs } => {
      Ok(Some(PreResolvedSignature { def_ix, arg_refs }))
    }
    GetArgsOutput::PartiallyApplied => Ok(None),
  }
}

fn fold_constants<'a>(
  ctx: &EvalCtx,
  local_scope: &'a mut ScopeTracker,
  expr: &mut Expr,
) -> Result<(), ErrorStack> {
  match expr {
    Expr::BinOp { op, lhs, rhs } => {
      optimize_expr(ctx, local_scope, lhs)?;
      optimize_expr(ctx, local_scope, rhs)?;

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

      let val = op.apply(&ctx, lhs_val, rhs_val)?;
      *expr = val.into_literal_expr();
      Ok(())
    }
    Expr::PrefixOp { op, expr: inner } => {
      optimize_expr(ctx, local_scope, inner)?;

      let Some(val) = inner.as_literal() else {
        return Ok(());
      };
      let val = op.apply(val)?;
      *expr = val.into_literal_expr();
      Ok(())
    }
    Expr::Range {
      start,
      end,
      inclusive,
    } => {
      optimize_expr(ctx, local_scope, start)?;
      optimize_expr(ctx, local_scope, end)?;

      let (Some(start_val), Some(end_val)) = (start.as_literal(), end.as_literal()) else {
        return Ok(());
      };
      let val = eval_range(start_val, end_val, *inclusive)?;
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
        if let Some(val) = local_scope.get(name) {
          match val {
            TrackedValueRef::Const(val) => match val {
              Value::Callable(callable) => {
                *target = FunctionCallTarget::Literal(callable.clone());
              }
              other => {
                return Err(ErrorStack::new(format!(
                  "Tried to call non-callable value: {name} = {other:?}",
                )))
              }
            },
            // calling a closure argument or dynamic captured variable
            TrackedValueRef::Arg(_) => (),
          }
        } else {
          // try to resolve it as a builtin
          let defs = match FN_SIGNATURE_DEFS.get(name) {
            Some(defs) => &defs.signatures,
            None => match FUNCTION_ALIASES.get(name) {
              Some(alias) => {
                &FN_SIGNATURE_DEFS
                  .get(alias)
                  .unwrap_or_else(|| {
                    panic!("Alias {name} points to unknown function {alias}");
                  })
                  .signatures
              }
              None => {
                return Err(ErrorStack::new(format!(
                  "Variable or function not found: {name}",
                )))
              }
            },
          };
          let fn_impl = resolve_builtin_impl(name);
          let name = name.clone();
          let pre_resolved_signature =
            maybe_pre_resolve_bulitin_call_signature(ctx, local_scope, &name, &args, &kwargs)?;
          *target = FunctionCallTarget::Literal(Rc::new(Callable::Builtin {
            name,
            fn_impl,
            fn_signature_defs: defs,
            pre_resolved_signature,
          }));
        }
      }

      let arg_vals = match args
        .iter()
        .map(|arg| arg.as_literal().ok_or(()))
        .collect::<Result<Vec<_>, _>>()
      {
        Ok(arg_vals) => arg_vals,
        Err(_) => return Ok(()),
      };
      let kwarg_vals = match kwargs
        .iter()
        .map(|(k, v)| v.as_literal().map(|v| (k.clone(), v)).ok_or(()))
        .collect::<Result<FxHashMap<_, _>, _>>()
      {
        Ok(kwarg_vals) => kwarg_vals,
        Err(_) => return Ok(()),
      };

      match target {
        FunctionCallTarget::Name(name) => {
          if let Some(val) = local_scope.get(name) {
            match val {
              TrackedValueRef::Const(val) => match val {
                Value::Callable(callable) => {
                  if !callable.is_side_effectful() {
                    let evaled =
                      ctx.invoke_callable(callable, &arg_vals, &kwarg_vals, &ctx.globals)?;
                    *expr = evaled.into_literal_expr();
                  }
                }
                other => {
                  return Err(ErrorStack::new(format!(
                    "Tried to call non-callable value: {name} = {other:?}",
                  )))
                }
              },
              TrackedValueRef::Arg(_) => (),
            }
            return Ok(());
          } else {
            unreachable!(
              "If this was a builtin, it would have been resolved earlier.  If it was undefined, \
               the error would have been raised earlier."
            );
          }
        }
        FunctionCallTarget::Literal(callable) => {
          if !callable.is_side_effectful() {
            let evaled = ctx.invoke_callable(callable, &arg_vals, &kwarg_vals, &ctx.globals)?;
            *expr = evaled.into_literal_expr();
          }
          Ok(())
        }
      }
    }
    Expr::Closure {
      params,
      body,
      return_type_hint,
    } => {
      for param in params.iter_mut() {
        if let Some(default_val) = &mut param.default_val {
          optimize_expr(ctx, local_scope, default_val)?;
        }
      }

      let mut closure_scope = ScopeTracker::wrap(local_scope);
      for param in params.iter() {
        closure_scope.set(param.name.clone(), TrackedValue::Arg(param.clone()));
      }

      // We use this scope for const capture checking/inlining to avoid situations where the closure
      // assigns a local variable with the same name as a non-const captured variable, shadowing it.
      //
      // This would hide the capture and cause incorrect behavior.
      let mut local_scope_with_args = ScopeTracker {
        vars: closure_scope.vars.clone(),
        parent: Some(local_scope),
      };

      for stmt in &mut body.0 {
        optimize_statement(ctx, &mut closure_scope, stmt)?;
      }

      for param in params.iter() {
        if let Some(default_val) = &param.default_val {
          if default_val.as_literal().is_none() {
            return Ok(());
          }
        }
      }

      if body.inline_const_captures(&mut local_scope_with_args) {
        return Ok(());
      }

      *expr = Expr::Literal(Value::Callable(Rc::new(Callable::Closure(Closure {
        params: params.clone(),
        body: body.0.clone(),
        captured_scope: Rc::new(Scope::default()),
        return_type_hint: return_type_hint.clone(),
      }))));

      Ok(())
    }
    Expr::Ident(id) => {
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

      if FN_SIGNATURE_DEFS.contains_key(id) || FUNCTION_ALIASES.contains_key(id) {
        let fn_signature_defs = match FN_SIGNATURE_DEFS.get(id) {
          Some(defs) => &defs.signatures,
          None => match FUNCTION_ALIASES.get(id) {
            Some(alias) => {
              FN_SIGNATURE_DEFS
                .get(alias)
                .unwrap_or_else(|| {
                  panic!("Alias {id} points to unknown function {alias}");
                })
                .signatures
            }
            None => unreachable!(),
          },
        };
        *expr = Expr::Literal(Value::Callable(Rc::new(Callable::Builtin {
          name: id.to_owned(),
          fn_impl: resolve_builtin_impl(id),
          fn_signature_defs,
          pre_resolved_signature: None,
        })));
        return Ok(());
      }

      Err(ErrorStack::new(format!(
        "Variable or function not found: {id}"
      )))
    }
    Expr::Literal(_) => Ok(()),
    Expr::ArrayLiteral(exprs) => {
      for inner in exprs.iter_mut() {
        optimize_expr(ctx, local_scope, inner)?;
      }

      // if all elements are literals, can fold into an `EagerSeq`
      if exprs.iter().all(|e| e.as_literal().is_some()) {
        let values = exprs
          .iter()
          .map(|e| e.as_literal().unwrap().clone())
          .collect::<Vec<_>>();
        *expr = Expr::Literal(Value::Sequence(Box::new(EagerSeq { inner: values })));
      }

      Ok(())
    }
    Expr::MapLiteral(map) => {
      for value in map.values_mut() {
        optimize_expr(ctx, local_scope, value)?;
      }

      // if all values are literals, can fold into a `Map`
      if map.values().all(|e| e.as_literal().is_some()) {
        let values = map
          .iter()
          .map(|(k, v)| (k.clone(), v.as_literal().unwrap().clone()))
          .collect::<FxHashMap<_, _>>();
        *expr = Expr::Literal(Value::Map(Box::new(values)));
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
        conditional_scope_var_names: impl Iterator<Item = &'a str>,
      ) {
        for name in conditional_scope_var_names {
          if let Some(val) = parent_scope.get(name) {
            if let TrackedValueRef::Const(_) = val {
              parent_scope.set(
                name.to_owned(),
                TrackedValue::Arg(ClosureArg {
                  name: name.to_owned(),
                  type_hint: None,
                  default_val: None,
                }),
              );
            }
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
      deconstify_parent_scope(local_scope, then_scope_var_names.keys().map(String::as_str));
      for (cond, inner) in else_if_exprs {
        optimize_expr(ctx, local_scope, cond)?;
        let mut else_if_scope = ScopeTracker::wrap(local_scope);
        optimize_expr(ctx, &mut else_if_scope, inner)?;
        let ScopeTracker {
          vars: else_if_scope_var_names,
          ..
        } = &else_if_scope;
        deconstify_parent_scope(
          local_scope,
          else_if_scope_var_names.keys().map(String::as_str),
        );
      }
      if let Some(else_expr) = else_expr {
        let mut else_scope = ScopeTracker::wrap(local_scope);
        optimize_expr(ctx, &mut else_scope, else_expr)?;
        let ScopeTracker {
          vars: else_scope_var_names,
          ..
        } = &else_scope;
        deconstify_parent_scope(local_scope, else_scope_var_names.keys().map(String::as_str));
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
        captures_dyn |= stmt.inline_const_captures(&mut block_scope);
      }

      for (key, val) in block_scope.vars {
        let is_set: bool = local_scope.get(&key).is_some();
        if is_set {
          local_scope.vars.insert(key, val);
          captures_dyn = true;
        }
      }

      if captures_dyn {
        return Ok(());
      }

      let evaled = ctx.eval_expr(expr, &ctx.globals)?;
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
      type_hint: _,
    } => {
      optimize_expr(ctx, local_scope, expr)?;
      local_scope.set(
        name.clone(),
        match expr.as_literal() {
          Some(val) => TrackedValue::Const(val),
          None => TrackedValue::Arg(ClosureArg {
            name: name.clone(),
            type_hint: None,
            default_val: None,
          }),
        },
      );
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
    optimize_statement(&ctx, &mut local_scope, stmt)?;
  }
  Ok(())
}

/// This doesn't disambiguate between builtin fn calls and calling user-defined functions.
pub fn traverse_fn_calls(program: &Program, mut cb: impl FnMut(&str)) {
  let mut cb = move |expr: &Expr| {
    if let Expr::Call(FunctionCall {
      target: FunctionCallTarget::Name(name),
      ..
    }) = expr
    {
      cb(name)
    }
  };

  for stmt in &program.statements {
    match stmt {
      Statement::Expr(expr) => expr.traverse(&mut cb),
      Statement::Assignment { expr, .. } => expr.traverse(&mut cb),
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

  let mut ast = crate::parse_program_src(code).unwrap();
  optimize_ast(&EvalCtx::default(), &mut ast).unwrap();
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

  let mut ast = crate::parse_program_src(code).unwrap();
  optimize_ast(&EvalCtx::default(), &mut ast).unwrap();
}

#[test]
fn test_basic_const_closure_eval() {
  let code = r#"
fn = |x| x + 1
y = fn(2)
"#;

  let mut ast = crate::parse_program_src(code).unwrap();
  optimize_ast(&EvalCtx::default(), &mut ast).unwrap();

  let Statement::Assignment { name, expr, .. } = &ast.statements[1] else {
    panic!("Expected second statement to be an assignment");
  };
  assert_eq!(name, "y");
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

  let mut ast = crate::parse_program_src(code).unwrap();
  optimize_ast(&EvalCtx::default(), &mut ast).unwrap();

  let Statement::Assignment { name, expr, .. } = &ast.statements[2] else {
    panic!("Expected second statement to be an assignment");
  };
  assert_eq!(name, "y");
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

  let mut ast = crate::parse_program_src(code).unwrap();
  optimize_ast(&EvalCtx::default(), &mut ast).unwrap();

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

  let mut ast = crate::parse_program_src(code).unwrap();
  optimize_ast(&EvalCtx::default(), &mut ast).unwrap();

  let Statement::Assignment { name, expr, .. } = &ast.statements[2] else {
    panic!("Expected second statement to be an assignment");
  };
  assert_eq!(name, "y");
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

  let mut ast = crate::parse_program_src(code).unwrap();
  optimize_ast(&EvalCtx::default(), &mut ast).unwrap();

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

  let mut ast = crate::parse_program_src(code).unwrap();
  optimize_ast(&EvalCtx::default(), &mut ast).unwrap();

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
  let call_target = match &closure_body[0] {
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
