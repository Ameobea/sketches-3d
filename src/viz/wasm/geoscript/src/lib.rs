use std::{fmt::Debug, sync::Mutex};

use fxhash::FxHashMap;
use mesh::{linked_mesh::Vec3, LinkedMesh};
use pest::{
  iterators::Pair,
  pratt_parser::{Assoc, Op, PrattParser},
  Parser,
};
use pest_derive::Parser;

use crate::{
  builtins::{eval_builtin_fn, FN_SIGNATURE_DEFS},
  seq::{IntRange, MapSeq},
};

mod builtins;
pub mod mesh_boolean;
mod seq;

#[derive(Parser)]
#[grammar = "src/geoscript.pest"]
pub struct GSParser;

lazy_static::lazy_static! {
  static ref PRATT_PARSER: PrattParser<Rule> = PrattParser::new()
    // TODO: should verify that these match other languages and expectations
    .op(Op::infix(Rule::gte_op, Assoc::Left) | Op::infix(Rule::lte_op, Assoc::Left))
    .op(Op::infix(Rule::gt_op, Assoc::Left) | Op::infix(Rule::lt_op, Assoc::Left))
    .op(Op::infix(Rule::eq_op, Assoc::Left) | Op::infix(Rule::neq_op, Assoc::Left))
    .op(Op::infix(Rule::range_inclusive_op, Assoc::Left))
    .op(Op::infix(Rule::range_op, Assoc::Left))
    .op(Op::infix(Rule::pipeline_op, Assoc::Left))
    .op(Op::infix(Rule::add_op, Assoc::Left) | Op::infix(Rule::sub_op, Assoc::Left))
    .op(Op::infix(Rule::mul_op, Assoc::Left) | Op::infix(Rule::div_op, Assoc::Left))
    .op(Op::prefix(Rule::neg_op) | Op::prefix(Rule::pos_op) | Op::prefix(Rule::negate_op));
}

pub trait Sequence: Debug {
  fn clone_box(&self) -> Box<dyn Sequence>;

  fn consume<'a>(
    self: Box<Self>,
    ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, String>> + 'a>;
}

#[derive(Debug, Clone)]
pub struct PartiallyAppliedFn {
  name: String,
  args: Vec<Value>,
  kwargs: FxHashMap<String, Value>,
}

#[derive(Debug)]
pub enum Value {
  Int(i64),
  Float(f32),
  Vec3(Vec3),
  Mesh(LinkedMesh<()>),
  PartiallyAppliedFn(PartiallyAppliedFn),
  Sequence(Box<dyn Sequence>),
  Bool(bool),
  Nil,
}

impl Clone for Value {
  fn clone(&self) -> Self {
    match self {
      Value::Int(i) => Value::Int(*i),
      Value::Float(f) => Value::Float(*f),
      Value::Vec3(v3) => Value::Vec3(*v3),
      Value::Mesh(mesh) => Value::Mesh(mesh.clone()),
      Value::PartiallyAppliedFn(paf) => Value::PartiallyAppliedFn(paf.clone()),
      Value::Sequence(seq) => Value::Sequence(seq.clone_box()),
      Value::Bool(b) => Value::Bool(*b),
      Value::Nil => Value::Nil,
    }
  }
}

impl Value {
  pub fn as_float(&self) -> Option<f32> {
    match self {
      Value::Float(f) => Some(*f),
      Value::Int(i) => Some(*i as f32),
      _ => None,
    }
  }

  fn as_mesh(&self) -> Option<&LinkedMesh<()>> {
    match self {
      Value::Mesh(mesh) => Some(mesh),
      _ => None,
    }
  }

  fn as_sequence(&self) -> Option<&dyn Sequence> {
    match self {
      Value::Sequence(seq) => Some(seq.as_ref()),
      _ => None,
    }
  }

  fn as_fn(&self) -> Option<&PartiallyAppliedFn> {
    match self {
      Value::PartiallyAppliedFn(paf) => Some(paf),
      _ => None,
    }
  }

  fn as_vec3(&self) -> Option<&Vec3> {
    match self {
      Value::Vec3(v3) => Some(v3),
      _ => None,
    }
  }

  fn as_int(&self) -> Option<i64> {
    match self {
      Value::Int(i) => Some(*i),
      _ => None,
    }
  }

  fn as_bool(&self) -> Option<bool> {
    match self {
      Value::Bool(b) => Some(*b),
      Value::Int(i) => Some(*i != 0),
      Value::Float(f) => Some(*f != 0.0),
      _ => None,
    }
  }
}

#[derive(Clone, Copy)]
enum ArgType {
  Int,
  Float,
  Numeric,
  Vec3,
  Mesh,
  PartiallyAppliedFn,
  Sequence,
  Bool,
  Any,
}

impl ArgType {
  pub fn is_valid(&self, arg: &Value) -> bool {
    match self {
      ArgType::Int => matches!(arg, Value::Int(_)),
      ArgType::Float => matches!(arg, Value::Float(_)),
      ArgType::Numeric => matches!(arg, Value::Int(_) | Value::Float(_)),
      ArgType::Vec3 => matches!(arg, Value::Vec3(_)),
      ArgType::Mesh => matches!(arg, Value::Mesh(_)),
      ArgType::PartiallyAppliedFn => matches!(arg, Value::PartiallyAppliedFn { .. }),
      ArgType::Sequence => matches!(arg, Value::Sequence(_)),
      ArgType::Bool => matches!(arg, Value::Bool(_)),
      ArgType::Any => true,
    }
  }

  pub fn any_valid(types: &[ArgType], arg: &Value) -> bool {
    types.iter().any(|t| t.is_valid(arg))
  }
}

#[derive(Debug)]
enum ArgRef {
  Positional(usize),
  Keyword(&'static str),
}

impl ArgRef {
  pub fn resolve<'a>(&self, args: &'a [Value], kwargs: &'a FxHashMap<String, Value>) -> &'a Value {
    match self {
      ArgRef::Positional(ix) => &args[*ix],
      ArgRef::Keyword(name) => kwargs.get(*name).expect("Keyword argument not found"),
    }
  }
}

#[derive(Debug)]
enum GetArgsOutput {
  Valid {
    def_ix: usize,
    arg_refs: Vec<ArgRef>,
  },
  PartiallyApplied,
}

fn get_args(
  fn_name: &str,
  arg_defs: &[&[(&'static str, &[ArgType])]],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<GetArgsOutput, String> {
  // empty defs = any args are valid
  if arg_defs.is_empty() {
    return Ok(GetArgsOutput::Valid {
      def_ix: 0,
      arg_refs: Vec::new(),
    });
  }

  for key in kwargs.keys() {
    if !arg_defs
      .iter()
      .any(|def| def.iter().any(|(name, _)| name == key))
    {
      return Err(format!(
        "kwarg `{key}` is not valid in any function signature",
      ));
    }
  }

  let mut valid_partial: bool = false;
  let any_args_provided = !args.is_empty() || !kwargs.is_empty();
  'def: for (def_ix, &def) in arg_defs.iter().enumerate() {
    let mut pos_arg_ix = 0;
    let mut arg_refs = Vec::with_capacity(def.len());
    for (name, types) in def {
      let (arg, arg_ref) = if let Some(kwarg) = kwargs.get(*name) {
        (kwarg, ArgRef::Keyword(*name))
      } else if pos_arg_ix < args.len() {
        let arg = &args[pos_arg_ix];
        let arg_ref = ArgRef::Positional(pos_arg_ix);
        pos_arg_ix += 1;
        (arg, arg_ref)
      } else {
        // If any required argument is missing, mark as partial if any args/kwargs were provided
        if any_args_provided {
          valid_partial = true;
        }
        continue 'def;
      };

      if !ArgType::any_valid(types, arg) {
        continue 'def;
      }

      arg_refs.push(arg_ref);
    }

    // if we have leftover positional args, not a valid call
    if pos_arg_ix < args.len() {
      continue 'def;
    }

    // valid args found for the whole def, so the function call is valid
    return Ok(GetArgsOutput::Valid { def_ix, arg_refs });
  }

  if valid_partial {
    return Ok(GetArgsOutput::PartiallyApplied);
  }

  Err(format!(
    "No valid function signature found for `{fn_name}` with args: {args:?}, kwargs: {kwargs:?}",
  ))
}

#[derive(Default)]
pub struct RenderedMeshes {
  // Using a mutex here to avoid making the whole `EvalCtx` require `&mut` when evaluating code.
  //
  // This thing is essentially "write-only", and the mutex should become a no-op in Wasm anyway.
  meshes: Mutex<Vec<LinkedMesh<()>>>,
}

impl RenderedMeshes {
  pub fn push(&self, mesh: LinkedMesh<()>) {
    self.meshes.lock().unwrap().push(mesh);
  }

  pub fn into_inner(self) -> Vec<LinkedMesh<()>> {
    self.meshes.into_inner().unwrap()
  }
}

#[derive(Default)]
pub struct Globals {
  // same thing as above for globals
  globals: Mutex<FxHashMap<String, Value>>,
}

impl Globals {
  pub fn insert(&self, key: String, value: Value) {
    self.globals.lock().unwrap().insert(key, value);
  }

  pub fn get(&self, key: &str) -> Option<Value> {
    // TODO: can't be cloning here...
    self.globals.lock().unwrap().get(key).cloned()
  }
}

#[derive(Default)]
pub struct EvalCtx {
  globals: Globals,
  rendered_meshes: RenderedMeshes,
}

enum ValidateFnOutput {
  /// Bad argument types
  Invalid,
  /// Fully valid function call, ready to be executed.  All required args are provided and types
  /// match.
  Valid,
  /// All applied arguments are valid, but not all required args are provided
  PartiallyApplied,
}

fn validate_fn_args(
  name: &str,
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> ValidateFnOutput {
  let Some(&defs) = FN_SIGNATURE_DEFS.get(name) else {
    return ValidateFnOutput::Invalid;
  };

  match get_args(name, defs, args, kwargs) {
    Ok(GetArgsOutput::Valid { .. }) => ValidateFnOutput::Valid,
    Ok(GetArgsOutput::PartiallyApplied) => ValidateFnOutput::PartiallyApplied,
    Err(_) => ValidateFnOutput::Invalid,
  }
}

impl EvalCtx {
  fn fold<'a>(
    &'a self,
    initial_val: Value,
    fn_value: PartiallyAppliedFn,
    iter: impl Iterator<Item = Result<Value, String>> + 'a,
  ) -> Result<Value, String> {
    let mut acc = initial_val;
    for (i, res) in iter.enumerate() {
      let value = res.map_err(|err| format!("Error seq value ix={i} in reduce: {err}"))?;
      let mut args = fn_value.args.clone();
      args.push(acc);
      args.push(value);
      acc = self
        .eval_fn_call(fn_value.name.as_str(), args, Default::default())
        .map_err(|err| format!("Error calling fn `{}` in reduce: {err}", fn_value.name))?;
    }

    Ok(acc)
  }

  fn reduce<'a>(
    &'a self,
    fn_value: PartiallyAppliedFn,
    seq: Box<dyn Sequence>,
  ) -> Result<Value, String> {
    // TODO: fix clone
    let mut iter = seq.clone_box().consume(self);
    let Some(first_value_res) = iter.next() else {
      return Err("empty sequence passed to reduce".to_owned());
    };
    let first_value =
      first_value_res.map_err(|err| format!("Error evaluating initial value in reduce: {err}"))?;

    self.fold(first_value, fn_value.clone(), iter)
  }

  fn eval_fn_call(
    &self,
    name: &str,
    args: Vec<Value>,
    kwargs: FxHashMap<String, Value>,
  ) -> Result<Value, String> {
    let defs_opt = FN_SIGNATURE_DEFS.get(name);
    if let Some(defs) = defs_opt {
      let (def_ix, arg_refs) = match get_args(name, defs, &args, &kwargs)? {
        GetArgsOutput::Valid { def_ix, arg_refs } => (def_ix, arg_refs),
        GetArgsOutput::PartiallyApplied => {
          return Ok(Value::PartiallyAppliedFn(PartiallyAppliedFn {
            name: name.to_owned(),
            args,
            kwargs,
          }));
        }
      };

      return eval_builtin_fn(name, def_ix, &arg_refs, &args, &kwargs, self);
    }

    // might be a partially applied function stored as a global
    let Some(global) = self.globals.get(name) else {
      return Err(format!("Undefined function: {name}"));
    };

    let Value::PartiallyAppliedFn(PartiallyAppliedFn {
      name: builtin_name,
      args: global_args,
      kwargs: global_kwargs,
    }) = global
    else {
      return Err(format!("\"{name}\" is not a function"));
    };

    // TODO: bad clones?
    let mut combined_args = global_args.clone();
    combined_args.extend(args);

    let mut combined_kwargs = global_kwargs.clone();
    for (key, value) in kwargs {
      combined_kwargs.insert(key.clone(), value.clone());
    }

    match validate_fn_args(&builtin_name, &combined_args, &combined_kwargs) {
      ValidateFnOutput::Valid => self.eval_fn_call(&builtin_name, combined_args, combined_kwargs),
      ValidateFnOutput::PartiallyApplied => Ok(Value::PartiallyAppliedFn(PartiallyAppliedFn {
        name: builtin_name.to_owned(),
        args: combined_args,
        kwargs: combined_kwargs,
      })),
      ValidateFnOutput::Invalid => Err(format!(
        "Invalid arguments for function \"{builtin_name}\": args: {:?}, kwargs: {:?}",
        combined_args, combined_kwargs
      )),
    }
  }

  fn parse_fn_call<'a>(
    &self,
    expr: pest::iterators::Pair<'a, Rule>,
  ) -> Result<(&'a str, Vec<Value>, FxHashMap<String, Value>), String> {
    let mut inner = expr.into_inner();
    let func_name = inner
      .next()
      .ok_or("Expected function name in call expression")?;
    if func_name.as_rule() != Rule::ident {
      return Err(format!(
        "Expected identifier for function name, found: {:?}",
        func_name.as_rule()
      ));
    }

    let func_name_str = func_name.as_str();
    let mut args = Vec::new();
    let mut kwargs = FxHashMap::default();

    for arg in inner {
      match arg.as_rule() {
        Rule::arg => {
          let child = arg
            .into_inner()
            .next()
            .expect("All arg nodes must have a child node");

          match child.as_rule() {
            Rule::keyword_arg => {
              let mut kw_inner = child.into_inner();
              let name = kw_inner.next().ok_or("Expected keyword argument name")?;
              if name.as_rule() != Rule::ident {
                return Err(format!(
                  "Expected identifier for keyword argument, found: {:?}",
                  name.as_rule()
                ));
              }
              let name_str = name.as_str().to_owned();

              let value_expr = kw_inner
                .next()
                .ok_or("Expected value expression for keyword argument")?;
              let value = self.eval_pair(value_expr)?;

              kwargs.insert(name_str, value);
            }
            _ => {
              let val = self.eval_pair(child)?;
              args.push(val);
            }
          }
        }
        _ => {
          return Err(format!(
            "Unexpected rule in function call expr: {:?}",
            arg.as_rule()
          ))
        }
      }
    }

    Ok((func_name_str, args, kwargs))
  }

  fn eval_expr(&self, expr: Pair<Rule>) -> Result<Value, String> {
    if expr.as_rule() != Rule::expr {
      return Err(format!(
        "`parse_expr` can only handle `expr` rules, found: {:?}",
        expr.as_rule()
      ));
    }

    PRATT_PARSER
      .map_primary(|primary| -> Result<Value, String> {
        match primary.as_rule() {
          Rule::term => self.eval_pair(primary),
          _ => unimplemented!("Unexpected primary rule: {:?}", primary.as_rule()),
        }
      })
      .map_prefix(|op, expr| match op.as_rule() {
        Rule::neg_op => self.eval_fn_call("neg", vec![expr?], Default::default()),
        Rule::pos_op => self.eval_fn_call("pos", vec![expr?], Default::default()),
        Rule::negate_op => self.eval_fn_call("neg", vec![expr?], Default::default()),
        _ => unreachable!("Unexpected prefix operator rule: {:?}", op.as_rule()),
      })
      .map_infix(|lhs, op, rhs| match op.as_rule() {
        Rule::add_op => self.eval_fn_call("add", vec![lhs?, rhs?], Default::default()),
        Rule::sub_op => self.eval_fn_call("sub", vec![lhs?, rhs?], Default::default()),
        Rule::mul_op => self.eval_fn_call("mul", vec![lhs?, rhs?], Default::default()),
        Rule::div_op => self.eval_fn_call("div", vec![lhs?, rhs?], Default::default()),
        Rule::pipeline_op => {
          let lhs = lhs?;
          let rhs = rhs?;

          let Some(paf) = rhs.as_fn() else {
            return Err(format!(
              "Pipeline operator requires a function on the right, found: {rhs:?}",
            ));
          };

          let mut args = paf.args.clone();
          args.push(lhs);
          let kwargs = paf.kwargs.clone();

          self.eval_fn_call(&paf.name, args, kwargs)
        }
        Rule::range_op => {
          let lhs = lhs?;
          let rhs = rhs?;

          let start = lhs
            .as_int()
            .ok_or_else(|| format!("Range operator requires integer start, found: {lhs:?}",))?;
          let end = rhs
            .as_int()
            .ok_or_else(|| format!("Range operator requires integer end, found: {rhs:?}",))?;

          Ok(Value::Sequence(Box::new(IntRange { start, end })))
        }
        Rule::range_inclusive_op => {
          let lhs = lhs?;
          let rhs = rhs?;

          let start = lhs
            .as_int()
            .ok_or_else(|| format!("Range operator requires integer start, found: {lhs:?}",))?;
          let end = rhs
            .as_int()
            .ok_or_else(|| format!("Range operator requires integer end, found: {rhs:?}",))?;

          Ok(Value::Sequence(Box::new(IntRange {
            start,
            end: end + 1,
          })))
        }
        Rule::gte_op => self.eval_fn_call("gte", vec![lhs?, rhs?], Default::default()),
        Rule::lte_op => self.eval_fn_call("lte", vec![lhs?, rhs?], Default::default()),
        Rule::gt_op => self.eval_fn_call("gt", vec![lhs?, rhs?], Default::default()),
        Rule::lt_op => self.eval_fn_call("lt", vec![lhs?, rhs?], Default::default()),
        Rule::eq_op => self.eval_fn_call("eq", vec![lhs?, rhs?], Default::default()),
        Rule::neq_op => self.eval_fn_call("neq", vec![lhs?, rhs?], Default::default()),
        _ => unreachable!("Unexpected operator rule: {:?}", op.as_rule()),
      })
      .parse(expr.into_inner())
  }

  pub fn eval_pair(&self, pair: Pair<Rule>) -> Result<Value, String> {
    match pair.as_rule() {
      Rule::expr => self.eval_expr(pair),
      Rule::assignment => {
        let mut inner = pair.into_inner();
        let ident = inner.next().ok_or("Expected identifier in assignment")?;
        let value_expr = inner
          .next()
          .ok_or("Expected value expression in assignment")?;

        if ident.as_rule() != Rule::ident {
          return Err(format!("Expected identifier, found: {:?}", ident.as_rule()));
        }

        let val = self.eval_pair(value_expr)?;
        self.globals.insert(ident.as_str().to_owned(), val);
        Ok(Value::Nil)
      }
      Rule::func_call => {
        let (func_name_str, args, kwargs) = self.parse_fn_call(pair)?;
        self
          .eval_fn_call(func_name_str, args, kwargs)
          .map_err(|e| format!("Function call error: {}", e))
      }
      Rule::int => {
        let int_str = pair.as_str();
        int_str
          .parse::<i64>()
          .map(Value::Int)
          .map_err(|_| format!("Invalid integer: {int_str}"))
      }
      Rule::float => {
        let float_str = pair.as_str();
        float_str
          .parse::<f32>()
          .map(Value::Float)
          .map_err(|_| format!("Invalid float: {float_str}"))
      }
      Rule::ident => {
        let ident_str = pair.as_str();
        if ident_str.is_empty() {
          return Err("Identifier cannot be empty".to_owned());
        }

        if let Some(value) = self.globals.get(ident_str) {
          return Ok(value);
        }

        // look it up as a global function
        if FN_SIGNATURE_DEFS.contains_key(ident_str) {
          return Ok(Value::PartiallyAppliedFn(PartiallyAppliedFn {
            name: ident_str.to_owned(),
            args: Vec::new(),
            kwargs: FxHashMap::default(),
          }));
        }

        Err(format!("Undefined identifier: {ident_str}"))
      }
      Rule::term => self.eval_pair(pair.into_inner().next().unwrap()),
      Rule::EOI | Rule::EOL => Ok(Value::Nil),
      Rule::range_inclusive_literal_expr => {
        let mut inner = pair.into_inner();
        let start_expr = inner.next().ok_or("Expected start expression in range")?;
        let end_expr = inner.next().ok_or("Expected end expression in range")?;

        let start_value = self.eval_pair(start_expr)?;
        let end_value = self.eval_pair(end_expr)?;

        match (start_value, end_value) {
          (Value::Int(start), Value::Int(end)) => Ok(Value::Sequence(Box::new(IntRange {
            start,
            end: end + 1,
          }))),
          _ => Err("Range must be defined by two integer values".to_owned()),
        }
      }
      Rule::range_literal_expr => {
        let mut inner = pair.into_inner();
        let start_expr = inner.next().ok_or("Expected start expression in range")?;
        let end_expr = inner.next().ok_or("Expected end expression in range")?;

        let start_value = self.eval_pair(start_expr)?;
        let end_value = self.eval_pair(end_expr)?;

        match (start_value, end_value) {
          (Value::Int(start), Value::Int(end)) => {
            Ok(Value::Sequence(Box::new(IntRange { start, end })))
          }
          _ => Err("Range must be defined by two integer values".to_owned()),
        }
      }
      other => unreachable!("Unexpected rule: {other:?}"),
    }
  }
}

pub fn parse_and_eval_program(src: &str) -> Result<EvalCtx, String> {
  let pairs = GSParser::parse(Rule::program, src).map_err(|e| format!("Parsing error: {}", e))?;

  let ctx = EvalCtx::default();

  for pair in pairs {
    match pair.as_rule() {
      Rule::program => {
        for pair in pair.into_inner() {
          let _val = ctx.eval_pair(pair)?;
        }
      }
      _ => return Err(format!("Unexpected rule in program: {:?}", pair.as_rule())),
    }
  }

  Ok(ctx)
}

#[test]
fn test_parser() {
  let src = r#"
a = sphere(0.5, width_segments = 32)
// comment
// function calls use parenthesis like basically every other popular language.  supports both positional args as well as python-style kwargs

// supports pipeline operator for function chaining
a=a | translate(vec3(4.0, -1, 0))
    | scale(vec3(1.5))

// completely whitespace insensitive

b = box(vec3(4,4,4))

c = union(a,b)"#;

  let result = GSParser::parse(Rule::program, src);
  assert!(result.is_ok(), "Failed to parse: {:?}", result.err());
  let pairs = result.unwrap();
  for pair in pairs {
    println!("Rule: {:?}, Span: {:?}", pair.as_rule(), pair.as_span());
  }
}

#[test]
fn test_eval_box_scale() {
  let src = r#"
b = box(vec3(y=4,4,4))
c = b | scale(1,2.,1.0000)
c | render
"#;

  let result = parse_and_eval_program(src);
  assert!(result.is_ok(), "Failed to evaluate: {:?}", result.err());
  let rendered_meshes = result.unwrap().rendered_meshes.into_inner();
  assert_eq!(rendered_meshes.len(), 1);
  let mesh = &rendered_meshes[0];
  assert_eq!(mesh.vertices.len(), 8);
  for vtx in mesh.vertices.values() {
    assert_eq!(vtx.position.x.abs(), 2.0);
    assert_eq!(vtx.position.y.abs(), 4.0);
    assert_eq!(vtx.position.z.abs(), 2.0);
  }
}

#[test]
fn test_functions_as_values() {
  let src = r#"
my_scale = scale(1,2,3)
a = box(2) | my_scale
my_scale_2 = scale(y=0.5)
a = a | my_scale_2(z=0.5, 0.5)
render(a)
"#;

  let rendered_meshes = parse_and_eval_program(src)
    .unwrap()
    .rendered_meshes
    .into_inner();
  assert_eq!(rendered_meshes.len(), 1);
  let mesh = &rendered_meshes[0];
  assert_eq!(mesh.vertices.len(), 8);
  for vtx in mesh.vertices.values() {
    assert_eq!(vtx.position.x.abs(), 1. / 2.);
    assert_eq!(vtx.position.y.abs(), 2. / 2.);
    assert_eq!(vtx.position.z.abs(), 3. / 2.);
  }
}

#[test]
fn test_partial_application_with_only_kwargs() {
  static ARGS: &[(&'static str, &[ArgType])] = &[("x", &[ArgType::Int]), ("y", &[ArgType::Int])];
  let defs = &[ARGS];
  let args = Vec::new();
  let mut kwargs = FxHashMap::default();
  kwargs.insert("y".to_owned(), Value::Int(1));
  let result = get_args("fn_name", defs, &args, &kwargs);
  match result {
    Ok(GetArgsOutput::PartiallyApplied) => {}
    _ => panic!("Expected PartiallyApplied, got {:?}", result),
  }
}

#[test]
fn test_unknown_kwarg_returns_error() {
  static ARGS: &[(&'static str, &[ArgType])] = &[("x", &[ArgType::Int]), ("y", &[ArgType::Int])];
  let defs = &[ARGS];
  let args = Vec::new();
  let mut kwargs = FxHashMap::default();
  kwargs.insert("z".to_owned(), Value::Int(1));
  let result = get_args("fn_name", defs, &args, &kwargs);
  assert!(result.is_err(), "Expected error for unknown kwarg");
}

#[test]
fn test_seq_reduce() {
  let src = r#"
seq = 0..5 | map(add(1))
result = seq | reduce(add)
print(result, asdf=result, x=4.2)
"#;

  let ctx = parse_and_eval_program(src).unwrap();
  let result = &ctx.globals.get("result").unwrap();
  let result = result.as_int().expect("Expected result to be an Int");
  // (0+1) + (1+1) + (2+1) + (3+1) + (4+1) = 15
  assert_eq!(result, 15);
}

#[test]
fn test_point_distribute() {
  let src = r#"
mesh = box(vec3(2,2,2))
points = mesh | point_distribute(count=3)
print(points | reduce(add))
"#;

  parse_and_eval_program(src).unwrap();
}

#[test]
fn test_infix_ops_and_precedence() {
  let src = r#"
a = 1 + (3 - 2) * 2 + 4
b = (a + 1) / 2.0
c = vec3(1) * 2 + (vec3(4,4,4) / 2)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let a = &ctx.globals.get("a").unwrap();
  let a = a.as_int().expect("Expected result to be an Int");
  assert_eq!(a, 7);

  let b = &ctx.globals.get("b").unwrap();
  let Value::Float(b) = b else {
    panic!("Expected result to be a Float");
  };
  assert_eq!(*b, 4.);

  let c = &ctx.globals.get("c").unwrap();
  let Value::Vec3(c) = c else {
    panic!("Expected result to be a Vec3");
  };
  assert_eq!(
    *c,
    Vec3::new(1., 1., 1.) * 2. + (Vec3::new(4., 4., 4.) / 2.)
  );
}

#[test]
fn test_range_op_edge_cases() {
  let src = r#"
a = 0..5
start = 0
b = start..5
end = 5
c = 0..end
d = start..end

sum = reduce(add)
a = a | sum
b = b | sum
c = c | sum
d = d | sum
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let a = &ctx.globals.get("a").unwrap();
  let a = a.as_int().expect("Expected result to be an Int");
  assert_eq!(a, 10); // 0 + 1 + 2 + 3 + 4

  let b = &ctx.globals.get("b").unwrap();
  let b = b.as_int().expect("Expected result to be an Int");
  assert_eq!(b, 10);

  let c = &ctx.globals.get("c").unwrap();
  let c = c.as_int().expect("Expected result to be an Int");
  assert_eq!(c, 10);

  let d = &ctx.globals.get("d").unwrap();
  let d = d.as_int().expect("Expected result to be an Int");
  assert_eq!(d, 10);
}

#[test]
fn test_lerp() {
  let src = r#"
a = lerp(vec3(0,0,0), vec3(1,1,1), 0.5)
b = 0.5 | lerp(0.0, 1)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let a = &ctx.globals.get("a").unwrap();
  let Value::Vec3(a) = a else {
    panic!("Expected result to be a Vec3");
  };
  assert_eq!(*a, Vec3::new(0.5, 0.5, 0.5));

  let b = &ctx.globals.get("b").unwrap();
  let Value::Float(b) = b else {
    panic!("Expected result to be a Float");
  };
  assert_eq!(*b, 0.5);
}

#[test]
fn test_comparison_ops() {
  let src = r#"
a = 1 < 2
b = 2. < 1
c = 1 <= 1
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let a = &ctx.globals.get("a").unwrap();
  let Value::Bool(a) = a else {
    panic!("Expected result to be a Bool");
  };
  assert!(*a);

  let b = &ctx.globals.get("b").unwrap();
  let Value::Bool(b) = b else {
    panic!("Expected result to be a Bool");
  };
  assert!(!*b);

  let c = &ctx.globals.get("c").unwrap();
  let Value::Bool(c) = c else {
    panic!("Expected result to be a Bool");
  };
  assert!(*c);
}

#[test]
fn test_prefix_ops() {
  let src = r#"
a = -1. - -2
b = -vec3(1,2,3)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let a = &ctx.globals.get("a").unwrap();
  let Value::Float(a) = a else {
    panic!("Expected result to be a Float");
  };
  assert_eq!(*a, 1.0);

  let b = &ctx.globals.get("b").unwrap();
  let Value::Vec3(b) = b else {
    panic!("Expected result to be a Vec3");
  };
  assert_eq!(*b, Vec3::new(-1.0, -2.0, -3.0));
}

#[test]
fn test_filter() {
  let src = r#"
is_even = compose(mod(b=2), eq(0))
seq = 0..10 | filter(is_even) | reduce(add)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let is_even = &ctx.globals.get("is_even").unwrap();
  dbg!(is_even);

  let seq = &ctx.globals.get("seq").unwrap();
  let seq = seq.as_int().expect("Expected result to be an Int");
  assert_eq!(seq, 20); // 0 + 2 + 4 + 6 + 8
}
