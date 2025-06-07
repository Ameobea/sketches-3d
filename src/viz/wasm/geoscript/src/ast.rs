use std::str::FromStr;

use fxhash::FxHashMap;
use itertools::Itertools;
use pest::iterators::Pair;

use crate::{EvalCtx, IntRange, Rule, Value, PRATT_PARSER};

pub struct Program {
  pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, Copy)]
pub enum TypeName {
  Mesh,
  Int,
  Float,
  Num,
  Bool,
  String,
  Seq,
  Callable,
  Nil,
}

impl FromStr for TypeName {
  type Err = String;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s {
      "mesh" => Ok(TypeName::Mesh),
      "int" => Ok(TypeName::Int),
      "float" => Ok(TypeName::Float),
      "num" => Ok(TypeName::Num),
      "bool" => Ok(TypeName::Bool),
      "string" => Ok(TypeName::String),
      "seq" => Ok(TypeName::Seq),
      "fn" | "callable" => Ok(TypeName::Callable),
      "nil" => Ok(TypeName::Nil),
      _ => Err(format!("Unknown type name: {s}")),
    }
  }
}

impl TypeName {
  pub fn validate_val(&self, val: &Value) -> Result<(), String> {
    match (self, val) {
      (TypeName::Mesh, Value::Mesh(_)) => Ok(()),
      (TypeName::Int, Value::Int(_)) => Ok(()),
      (TypeName::Float, Value::Float(_)) => Ok(()),
      (TypeName::Num, Value::Int(_) | Value::Float(_)) => Ok(()),
      (TypeName::Bool, Value::Bool(_)) => Ok(()),
      (TypeName::Seq, Value::Sequence(_)) => Ok(()),
      (TypeName::Callable, Value::Callable(_)) => Ok(()),
      (TypeName::Nil, Value::Nil) => Ok(()),
      _ => Err(format!("Value {val:?} does not match type {self:?}")),
    }
  }
}

#[derive(Clone)]
pub enum Statement {
  Assignment {
    name: String,
    expr: Expr,
    type_hint: Option<TypeName>,
  },
  Expr(Expr),
}

#[derive(Clone)]
pub struct ClosureArg {
  pub name: String,
  pub type_hint: Option<TypeName>,
}

#[derive(Clone)]
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
  FieldAccess {
    obj: Box<Expr>,
    field: String,
  },
  Call(FunctionCall),
  Closure {
    params: Vec<ClosureArg>,
    body: ClosureBody,
    return_type_hint: Option<TypeName>,
  },
  Ident(String),
  Int(i64),
  Float(f32),
  Bool(bool),
  Array(Vec<Expr>),
  Nil,
}

#[derive(Clone)]
pub struct ClosureBody(pub Vec<Statement>);

#[derive(Clone)]
pub struct FunctionCall {
  pub name: String,
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
  BitOr,
  Range,
  RangeInclusive,
  Pipeline,
  Map,
}

fn eval_range(start: Value, end: Value, inclusive: bool) -> Result<Value, String> {
  let Value::Int(start) = start else {
    return Err(format!("Range start must be an integer, found: {start:?}",));
  };
  let Value::Int(end) = end else {
    return Err(format!("Range end must be an integer, found: {end:?}",));
  };

  let mut range = IntRange { start, end };
  if inclusive {
    range.end += 1;
  }

  Ok(Value::Sequence(Box::new(range)))
}

impl BinOp {
  pub fn apply(&self, ctx: &EvalCtx, lhs: Value, rhs: Value) -> Result<Value, String> {
    match self {
      BinOp::Add => ctx.eval_fn_call("add", &[lhs, rhs], Default::default(), &ctx.globals, true),
      BinOp::Sub => ctx.eval_fn_call("sub", &[lhs, rhs], Default::default(), &ctx.globals, true),
      BinOp::Mul => ctx.eval_fn_call("mul", &[lhs, rhs], Default::default(), &ctx.globals, true),
      BinOp::Div => ctx.eval_fn_call("div", &[lhs, rhs], Default::default(), &ctx.globals, true),
      BinOp::Mod => ctx.eval_fn_call("mod", &[lhs, rhs], Default::default(), &ctx.globals, true),
      BinOp::Gt => ctx.eval_fn_call("gt", &[lhs, rhs], Default::default(), &ctx.globals, true),
      BinOp::Lt => ctx.eval_fn_call("lt", &[lhs, rhs], Default::default(), &ctx.globals, true),
      BinOp::Gte => ctx.eval_fn_call("gte", &[lhs, rhs], Default::default(), &ctx.globals, true),
      BinOp::Lte => ctx.eval_fn_call("lte", &[lhs, rhs], Default::default(), &ctx.globals, true),
      BinOp::Eq => ctx.eval_fn_call("eq", &[lhs, rhs], Default::default(), &ctx.globals, true),
      BinOp::Neq => ctx.eval_fn_call("neq", &[lhs, rhs], Default::default(), &ctx.globals, true),
      BinOp::And => ctx.eval_fn_call("and", &[lhs, rhs], Default::default(), &ctx.globals, true),
      BinOp::Or => ctx.eval_fn_call("or", &[lhs, rhs], Default::default(), &ctx.globals, true),
      BinOp::BitAnd => ctx.eval_fn_call(
        "bit_and",
        &[lhs, rhs],
        Default::default(),
        &ctx.globals,
        true,
      ),
      BinOp::BitOr => ctx.eval_fn_call(
        "bit_or",
        &[lhs, rhs],
        Default::default(),
        &ctx.globals,
        true,
      ),
      BinOp::Range => eval_range(lhs, rhs, false),
      BinOp::RangeInclusive => eval_range(lhs, rhs, true),
      BinOp::Pipeline => {
        // eval as a pipeline operator if the rhs is a callable
        if let Some(callable) = rhs.as_callable() {
          return ctx
            .invoke_callable(callable, &[lhs], Default::default(), &ctx.globals)
            .map_err(|err| format!("Error invoking callable in pipeline: {err}",));
        }

        // maybe it's a bit-or
        ctx.eval_fn_call(
          "bit_or",
          &[lhs, rhs],
          Default::default(),
          &ctx.globals,
          true,
        )
      }
      BinOp::Map => {
        // this operator acts the same as `lhs | map(rhs)`
        ctx.eval_fn_call("map", &[rhs, lhs], Default::default(), &ctx.globals, true)
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
  pub fn apply(&self, ctx: &EvalCtx, val: Value) -> Result<Value, String> {
    match self {
      PrefixOp::Neg => ctx.eval_fn_call("neg", &[val], Default::default(), &ctx.globals, true),
      PrefixOp::Pos => ctx.eval_fn_call("pos", &[val], Default::default(), &ctx.globals, true),
      PrefixOp::Not => ctx.eval_fn_call("not", &[val], Default::default(), &ctx.globals, true),
    }
  }
}

fn parse_fn_call(func_call: Pair<Rule>) -> Result<Expr, String> {
  if func_call.as_rule() != Rule::func_call {
    return Err(format!(
      "`parse_func_call` can only handle `func_call` rules, found: {:?}",
      func_call.as_rule()
    ));
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

  Ok(Expr::Call(FunctionCall { name, args, kwargs }))
}

fn parse_node(expr: Pair<Rule>) -> Result<Expr, String> {
  match expr.as_rule() {
    Rule::int => {
      let int_str = expr.as_str();
      int_str
        .parse::<i64>()
        .map(Expr::Int)
        .map_err(|_| format!("Invalid integer: {int_str}"))
    }
    Rule::float => {
      let float_str = expr.as_str();
      float_str
        .parse::<f32>()
        .map(Expr::Float)
        .map_err(|_| format!("Invalid float: {float_str}"))
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
          let type_hint = if let Some(type_hint) = inner.next() {
            Some(
              TypeName::from_str(type_hint.into_inner().next().unwrap().as_str())
                .map_err(|err| format!("Invalid type hint for closure arg {name}: {err}"))?,
            )
          } else {
            None
          };
          Ok(ClosureArg { name, type_hint })
        })
        .collect::<Result<Vec<_>, String>>()?;

      let mut next = inner.next().unwrap();
      let return_type_hint = if next.as_rule() == Rule::type_hint {
        let type_hint_str = next.into_inner().next().unwrap().as_str();
        let return_type_hint = TypeName::from_str(type_hint_str)
          .map_err(|err| format!("Invalid type hint for closure return type: {err}"))?;
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
            .collect::<Result<Vec<_>, String>>()?;

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
        .collect::<Result<Vec<_>, String>>()?;
      Ok(Expr::Array(elems))
    }
    Rule::bool_literal => {
      let bool_str = expr.as_str();
      match bool_str {
        "true" => Ok(Expr::Bool(true)),
        "false" => Ok(Expr::Bool(false)),
        _ => unreachable!("Unexpected boolean literal: {bool_str}, expected 'true' or 'false'"),
      }
    }
    _ => unimplemented!(
      "unimplemented node type for parse_node: {:?}",
      expr.as_rule()
    ),
  }
}

pub fn parse_expr(expr: Pair<Rule>) -> Result<Expr, String> {
  if expr.as_rule() != Rule::expr {
    panic!(
      "`parse_expr` can only handle `expr` rules, found: {:?}",
      expr.as_rule()
    );
  }

  PRATT_PARSER
    .map_primary(|primary| -> Result<Expr, String> {
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
        _ => return Err(format!("Unhandled operator rule: {:?}", op.as_rule())),
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

      let mut inner = op.into_inner();
      // skip the `.` token
      inner.next().unwrap();
      let field = inner.next().unwrap().as_str().to_owned();
      Ok(Expr::FieldAccess {
        obj: Box::new(expr),
        field,
      })
    })
    .parse(expr.into_inner())
}

fn parse_assignment(assignment: Pair<Rule>) -> Result<Statement, String> {
  if assignment.as_rule() != Rule::assignment {
    return Err(format!(
      "`parse_assignment` can only handle `assignment` rules, found: {:?}",
      assignment.as_rule()
    ));
  }

  let mut inner = assignment.into_inner();
  let name = inner.next().unwrap().as_str().to_owned();

  let mut next = inner.next().unwrap();
  let type_hint = if next.as_rule() == Rule::type_hint {
    let type_hint = TypeName::from_str(next.into_inner().next().unwrap().as_str())?;
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

fn parse_statement(stmt: Pair<Rule>) -> Result<Option<Statement>, String> {
  match stmt.as_rule() {
    Rule::assignment => parse_assignment(stmt).map(Some),
    Rule::expr => Ok(Some(Statement::Expr(parse_expr(stmt)?))),
    Rule::EOI => Ok(None),
    _ => unreachable!("Unexpected statement rule: {:?}", stmt.as_rule()),
  }
}

pub fn parse_program(program: Pair<Rule>) -> Result<Program, String> {
  if program.as_rule() != Rule::program {
    return Err(format!(
      "`parse_program` can only handle `program` rules, found: {:?}",
      program.as_rule()
    ));
  }

  let statements = program
    .into_inner()
    .map(parse_statement)
    .filter_map_ok(|s| s)
    .collect::<Result<Vec<_>, String>>()?;

  Ok(Program { statements })
}
