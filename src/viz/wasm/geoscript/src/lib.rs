#![feature(if_let_guard, impl_trait_in_bindings)]

#[cfg(target_arch = "wasm32")]
use std::cell::UnsafeCell;
use std::{
  fmt::{Debug, Display},
  sync::{Arc, Mutex},
};

use ast::{parse_program, Expr, Statement};
use fxhash::FxHashMap;
use mesh::{linked_mesh::Vec3, LinkedMesh};
use pest::{
  pratt_parser::{Assoc, Op, PrattParser},
  Parser,
};
use pest_derive::Parser;
use rand_pcg::Pcg32;
use seq::EagerSeq;

use crate::{
  ast::{ClosureArg, TypeName},
  builtins::{eval_builtin_fn, FN_SIGNATURE_DEFS, FUNCTION_ALIASES},
  seq::{IntRange, MapSeq},
};

mod ast;
mod builtins;
pub mod mesh_ops;
pub mod noise;
pub mod path_building;
mod seq;

#[derive(Parser)]
#[grammar = "src/geoscript.pest"]
pub struct GSParser;

lazy_static::lazy_static! {
  static ref PRATT_PARSER: PrattParser<Rule> = PrattParser::new()
    .op(Op::infix(Rule::range_inclusive_op, Assoc::Left) | Op::infix(Rule::range_op, Assoc::Left))
    .op(Op::infix(Rule::or_op, Assoc::Left))
    .op(Op::infix(Rule::and_op, Assoc::Left))
    .op(Op::infix(Rule::pipeline_op, Assoc::Left) | Op::infix(Rule::map_op, Assoc::Left))
    .op(Op::infix(Rule::bit_and_op, Assoc::Left))
    .op(Op::infix(Rule::eq_op, Assoc::Left) | Op::infix(Rule::neq_op, Assoc::Left))
    .op(
      Op::infix(Rule::lt_op,Assoc::Left)
        | Op::infix(Rule::lte_op, Assoc::Left)
        | Op::infix(Rule::gt_op, Assoc::Left)
        | Op::infix(Rule::gte_op, Assoc::Left)
    )
    .op(
      Op::infix(Rule::add_op, Assoc::Left)
        | Op::infix(Rule::sub_op, Assoc::Left)
    )
    .op(
      Op::infix(Rule::mul_op, Assoc::Left)
        | Op::infix(Rule::div_op, Assoc::Left)
        | Op::infix(Rule::mod_op, Assoc::Left)
    )
    .op(
      Op::prefix(Rule::neg_op)
        | Op::prefix(Rule::pos_op)
        | Op::prefix(Rule::negate_op)
    )
    .op(Op::postfix(Rule::postfix));
}

pub struct ErrorStack {
  pub errors: Vec<String>,
}

impl ErrorStack {
  pub fn new(msg: impl Into<String>) -> Self {
    ErrorStack {
      errors: vec![msg.into()],
    }
  }

  pub fn wrap(mut self, msg: impl Into<String>) -> Self {
    self.errors.push(msg.into());
    self
  }
}

impl Display for ErrorStack {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let indent = "  ";
    for (ix, err) in self.errors.iter().rev().enumerate() {
      let mut lines = err.lines().peekable();
      while let Some(line) = lines.next() {
        for _ in 0..ix {
          write!(f, "{indent}")?;
        }

        write!(f, "{line}")?;

        if lines.peek().is_some() {
          write!(f, "\n")?;
        }
      }

      if ix < self.errors.len() - 1 {
        write!(f, "\n")?;
      }
    }
    Ok(())
  }
}

impl Debug for ErrorStack {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{self}")
  }
}

pub trait Sequence: Debug {
  fn clone_box(&self) -> Box<dyn Sequence>;

  fn consume<'a>(
    self: Box<Self>,
    ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a>;
}

#[derive(Clone)]
pub struct PartiallyAppliedFn {
  inner: Box<Callable>,
  args: Vec<Value>,
  kwargs: FxHashMap<String, Value>,
}

impl Debug for PartiallyAppliedFn {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "PartiallyAppliedFn(inner={:?} with {} args, {} kwargs)",
      self.inner,
      self.args.len(),
      self.kwargs.len()
    )
  }
}

#[derive(Clone)]
pub struct Closure {
  /// Names of parameters for this closure in order
  params: Vec<ClosureArg>,
  body: Vec<Statement>,
  /// Contains variables captured from the environment when the closure was created
  captured_scope: Arc<Scope>,
  return_type_hint: Option<TypeName>,
}

impl Debug for Closure {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let return_type_hint_formatted = self
      .return_type_hint
      .map(|type_hint| format!(", return type hint: {type_hint:?}"))
      .unwrap_or_default();
    write!(
      f,
      "<closure with {} params{return_type_hint_formatted}>",
      self.params.len(),
    )
  }
}

#[derive(Clone)]
pub struct ComposedFn {
  pub inner: Vec<Callable>,
}

impl Debug for ComposedFn {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "<composed fn of {} inner callables>", self.inner.len())
  }
}

#[derive(Clone)]
pub enum Callable {
  Builtin(String),
  PartiallyAppliedFn(PartiallyAppliedFn),
  Closure(Closure),
  ComposedFn(ComposedFn),
}

impl Debug for Callable {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Callable::Builtin(name) => Debug::fmt(&format!("<built-in fn \"{name}\">"), f),
      Callable::PartiallyAppliedFn(paf) => Debug::fmt(&format!("{paf:?}"), f),
      Callable::Closure(closure) => Debug::fmt(&format!("{closure:?}"), f),
      Callable::ComposedFn(composed) => Debug::fmt(&format!("{composed:?}"), f),
    }
  }
}

pub enum Value {
  Int(i64),
  Float(f32),
  Vec3(Vec3),
  Mesh(Arc<LinkedMesh<()>>),
  Callable(Callable),
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
      Value::Callable(callable) => Value::Callable(callable.clone()),
      Value::Sequence(seq) => Value::Sequence(seq.clone_box()),
      Value::Bool(b) => Value::Bool(*b),
      Value::Nil => Value::Nil,
    }
  }
}

impl Debug for Value {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Value::Int(i) => write!(f, "Int({i})"),
      Value::Float(fl) => write!(f, "Float({fl})"),
      Value::Vec3(v3) => write!(f, "Vec3({}, {}, {})", v3.x, v3.y, v3.z),
      Value::Mesh(mesh) => write!(f, "{mesh:?}"),
      Value::Callable(callable) => write!(f, "{callable:?}"),
      Value::Sequence(seq) => write!(f, "{seq:?}"),
      Value::Bool(b) => write!(f, "Bool({b})"),
      Value::Nil => write!(f, "Nil"),
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

  fn as_mesh(&self) -> Option<Arc<LinkedMesh<()>>> {
    match self {
      Value::Mesh(mesh) => Some(Arc::clone(&mesh)),
      _ => None,
    }
  }

  fn as_sequence(&self) -> Option<&dyn Sequence> {
    match self {
      Value::Sequence(seq) => Some(seq.as_ref()),
      _ => None,
    }
  }

  fn as_callable(&self) -> Option<&Callable> {
    match self {
      Value::Callable(callable) => Some(callable),
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
  Callable,
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
      ArgType::Callable => matches!(arg, Value::Callable { .. }),
      ArgType::Sequence => matches!(arg, Value::Sequence(_)),
      ArgType::Bool => matches!(arg, Value::Bool(_)),
      ArgType::Any => true,
    }
  }

  pub fn any_valid(types: &[ArgType], arg: &Value) -> bool {
    types.iter().any(|t| t.is_valid(arg))
  }

  pub fn as_str(&self) -> &'static str {
    match self {
      ArgType::Int => "int",
      ArgType::Float => "float",
      ArgType::Numeric => "num",
      ArgType::Vec3 => "vec3",
      ArgType::Mesh => "mesh",
      ArgType::Callable => "fn",
      ArgType::Sequence => "seq",
      ArgType::Bool => "bool",
      ArgType::Any => "any",
    }
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

fn format_fn_signatures(arg_defs: &[&[(&'static str, &[ArgType])]]) -> String {
  arg_defs
    .iter()
    .map(|def| {
      if def.is_empty() {
        return "  <no args>".to_owned();
      }

      let formatted_def = def
        .iter()
        .map(|(name, types)| {
          let types_str = types
            .iter()
            .map(ArgType::as_str)
            .collect::<Vec<_>>()
            .join(" | ");
          format!("{name}: {types_str}")
        })
        .collect::<Vec<_>>()
        .join(", ");
      format!("  {formatted_def}")
    })
    .collect::<Vec<_>>()
    .join("\n")
}

fn get_args(
  fn_name: &str,
  arg_defs: &[&[(&'static str, &[ArgType])]],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<GetArgsOutput, ErrorStack> {
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
      return Err(ErrorStack::new(format!(
        "kwarg `{key}` is not valid in any function signature.\n\nAvailable signatures:\n{}",
        format_fn_signatures(arg_defs)
      )));
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

  Err(ErrorStack::new(format!(
    "No valid function signature found for `{fn_name}` with args: {args:?}, kwargs: \
     {kwargs:?}\n\nAvailable signatures:\n{}",
    format_fn_signatures(arg_defs)
  )))
}

#[derive(Default)]
pub struct RenderedMeshes {
  // Using a mutex here to avoid making the whole `EvalCtx` require `&mut` when evaluating code.
  //
  // This thing is essentially "write-only", and the mutex should become a no-op in Wasm anyway.
  pub meshes: Mutex<Vec<Arc<LinkedMesh<()>>>>,
}

impl RenderedMeshes {
  pub fn push(&self, mesh: Arc<LinkedMesh<()>>) {
    self.meshes.lock().unwrap().push(mesh);
  }

  pub fn into_inner(self) -> Vec<Arc<LinkedMesh<()>>> {
    self.meshes.into_inner().unwrap()
  }

  pub fn len(&self) -> usize {
    self.meshes.lock().unwrap().len()
  }
}

#[derive(Default, Debug)]
pub struct Scope {
  vars: Mutex<FxHashMap<String, Value>>,
  parent: Option<Arc<Scope>>,
}

impl Clone for Scope {
  fn clone(&self) -> Self {
    Scope {
      vars: Mutex::new(self.vars.lock().unwrap().clone()),
      parent: self.parent.as_ref().map(Arc::clone),
    }
  }
}

impl Scope {
  pub fn default_globals() -> Self {
    let scope = Scope::default();

    // TODO: should move these to a table or something
    scope.insert("pi".to_owned(), Value::Float(std::f32::consts::PI));

    scope
  }

  pub fn insert(&self, key: String, value: Value) {
    self.vars.lock().unwrap().insert(key, value);
  }

  pub fn get(&self, key: &str) -> Option<Value> {
    // TODO: can't be cloning here...
    if let Some(val) = self.vars.lock().unwrap().get(key).cloned() {
      return Some(val);
    }

    if let Some(parent) = &self.parent {
      return parent.get(key);
    }

    None
  }

  fn wrap(parent: &Arc<Scope>) -> Scope {
    Scope {
      vars: Mutex::new(FxHashMap::default()),
      parent: Some(Arc::clone(parent)),
    }
  }
}

pub struct EvalCtx {
  pub globals: Scope,
  pub rendered_meshes: RenderedMeshes,
  pub log_fn: fn(&str),
  #[cfg(target_arch = "wasm32")]
  rng: UnsafeCell<Pcg32>,
}

impl Default for EvalCtx {
  fn default() -> Self {
    EvalCtx {
      globals: Scope::default_globals(),
      rendered_meshes: RenderedMeshes::default(),
      log_fn: |msg| println!("{msg}"),
      #[cfg(target_arch = "wasm32")]
      rng: UnsafeCell::new(Pcg32::new(7718587666045340534, 17289744314186392832)),
    }
  }
}

impl EvalCtx {
  pub fn set_log_fn(mut self, log: fn(&str)) -> EvalCtx {
    self.log_fn = log;
    self
  }

  #[cfg(target_arch = "wasm32")]
  pub fn rng(&self) -> &'static mut Pcg32 {
    unsafe { &mut *self.rng.get() }
  }

  #[cfg(not(target_arch = "wasm32"))]
  pub fn rng(&self) -> &'static mut Pcg32 {
    unimplemented!()
  }

  pub fn eval_expr(&self, expr: &Expr, scope: &Scope) -> Result<Value, ErrorStack> {
    match expr {
      Expr::Call(call) => {
        let args = call
          .args
          .iter()
          .map(|a| self.eval_expr(a, scope))
          .collect::<Result<Vec<_>, _>>()?;
        let kwargs = call
          .kwargs
          .iter()
          .map(|(k, v)| self.eval_expr(v, scope).map(|val| (k.clone(), val)))
          .collect::<Result<FxHashMap<_, _>, _>>()?;
        self
          .eval_fn_call(&call.name, &args, kwargs, scope, false)
          .map_err(|err| err.wrap(format!("Error evaluating function call `{}`", call.name)))
      }
      Expr::BinOp { op, lhs, rhs } => {
        let lhs = self.eval_expr(lhs, scope)?;
        let rhs = self.eval_expr(rhs, scope)?;
        op.apply(self, lhs, rhs)
          .map_err(|err| err.wrap(format!("Error applying binary operator `{op:?}`")))
      }
      Expr::PrefixOp { op, expr } => {
        let val = self.eval_expr(expr, scope)?;
        op.apply(self, val)
          .map_err(|err| err.wrap(format!("Error applying prefix operator `{op:?}`")))
      }
      Expr::Range {
        start,
        end,
        inclusive,
      } => {
        let start = self.eval_expr(start, scope)?;
        let Value::Int(start) = start else {
          return Err(ErrorStack::new(format!(
            "Range start must be an integer, found: {start:?}"
          )));
        };
        let end = self.eval_expr(end, scope)?;
        let Value::Int(end) = end else {
          return Err(ErrorStack::new(format!(
            "Range end must be an integer, found: {end:?}"
          )));
        };

        Ok(if *inclusive {
          Value::Sequence(Box::new(IntRange {
            start,
            end: end + 1,
          }))
        } else {
          Value::Sequence(Box::new(IntRange { start, end }))
        })
      }
      Expr::Ident(name) => {
        if let Some(local) = scope.get(name) {
          return Ok(local);
        }

        // look it up as a builtin fn
        if FN_SIGNATURE_DEFS.contains_key(&name) {
          return Ok(Value::Callable(Callable::Builtin(name.to_owned())));
        }

        Err(ErrorStack::new(format!("Variable not found: {name}")))
      }
      Expr::Int(i) => Ok(Value::Int(*i)),
      Expr::Float(f) => Ok(Value::Float(*f)),
      Expr::Bool(b) => Ok(Value::Bool(*b)),
      Expr::Array(elems) => {
        let elems = elems
          .iter()
          .map(|expr| self.eval_expr(expr, scope))
          .collect::<Result<Vec<_>, _>>()?;
        Ok(Value::Sequence(Box::new(EagerSeq { inner: elems })))
      }
      Expr::Nil => Ok(Value::Nil),
      Expr::Closure {
        params,
        body,
        return_type_hint,
      } => Ok(Value::Callable(Callable::Closure(Closure {
        params: params.clone(),
        body: body.0.clone(),
        // cloning the scope here makes the closure function like a rust `move` closure
        // where all the values are cloned before being moved into the closure.
        captured_scope: Arc::new(scope.clone()),
        return_type_hint: return_type_hint.clone(),
      }))),
      Expr::FieldAccess { obj, field } => self.eval_field_access(obj, field, scope),
    }
  }

  fn eval_assignment(
    &self,
    ident: &str,
    value: Value,
    scope: &Scope,
    type_hint: Option<TypeName>,
  ) -> Result<Value, ErrorStack> {
    if ident.is_empty() {
      return Err(ErrorStack::new(
        "found empty ident in assignment; shouldn't be possible I think",
      ));
    }

    if let Some(type_hint) = type_hint {
      type_hint.validate_val(&value)?;
    }

    scope.insert(ident.to_owned(), value.clone());
    Ok(Value::Nil)
  }

  fn eval_statement(&self, statement: &Statement, scope: &Scope) -> Result<Value, ErrorStack> {
    match statement {
      Statement::Expr(expr) => self.eval_expr(expr, scope),
      Statement::Assignment {
        name,
        expr,
        type_hint,
      } => self.eval_assignment(&name, self.eval_expr(expr, scope)?, scope, *type_hint),
    }
  }

  fn fold<'a>(
    &'a self,
    initial_val: Value,
    callable: &Callable,
    iter: impl Iterator<Item = Result<Value, ErrorStack>> + 'a,
  ) -> Result<Value, ErrorStack> {
    let mut acc = initial_val;
    for (i, res) in iter.enumerate() {
      let value = res.map_err(|err| err.wrap(format!("Error seq value ix={i} in reduce")))?;
      acc = self
        .invoke_callable(callable, &[acc, value], Default::default(), &self.globals)
        .map_err(|err| err.wrap("Error invoking callable in reduce".to_owned()))?;
    }

    Ok(acc)
  }

  fn reduce<'a>(
    &'a self,
    fn_value: &Callable,
    seq: Box<dyn Sequence>,
  ) -> Result<Value, ErrorStack> {
    // TODO: fix clone
    let mut iter = seq.clone_box().consume(self);
    let Some(first_value_res) = iter.next() else {
      return Err(ErrorStack::new("empty sequence passed to reduce"));
    };
    let first_value =
      first_value_res.map_err(|err| err.wrap("Error evaluating initial value in reduce"))?;

    self.fold(first_value, &fn_value, iter)
  }

  fn eval_fn_call(
    &self,
    mut name: &str,
    args: &[Value],
    kwargs: FxHashMap<String, Value>,
    scope: &Scope,
    builtins_only: bool,
  ) -> Result<Value, ErrorStack> {
    if !builtins_only {
      // might be a callable stored as a global
      if let Some(global) = scope.get(name) {
        let Value::Callable(callable) = global else {
          return Err(ErrorStack::new(format!(
            "\"{name}\" is not a callable; found: {global:?}"
          )));
        };

        return self.invoke_callable(&callable, args, kwargs, scope);
      }
    }

    let defs_opt = FN_SIGNATURE_DEFS.get(name);
    let defs_opt = match defs_opt {
      Some(defs) => Some(defs),
      None => {
        if let Some(alias) = FUNCTION_ALIASES.get(name) {
          name = alias;
          FN_SIGNATURE_DEFS.get(name)
        } else {
          None
        }
      }
    };
    if let Some(defs) = defs_opt {
      let (def_ix, arg_refs) = match get_args(name, defs, &args, &kwargs)? {
        GetArgsOutput::Valid { def_ix, arg_refs } => (def_ix, arg_refs),
        GetArgsOutput::PartiallyApplied => {
          return Ok(Value::Callable(Callable::PartiallyAppliedFn(
            PartiallyAppliedFn {
              inner: Box::new(Callable::Builtin(name.to_owned())),
              args: args.to_owned(),
              kwargs,
            },
          )))
        }
      };

      return eval_builtin_fn(name, def_ix, &arg_refs, &args, &kwargs, self);
    }

    return Err(ErrorStack::new(format!("Undefined function: {name}")));
  }

  fn invoke_callable(
    &self,
    callable: &Callable,
    args: &[Value],
    kwargs: FxHashMap<String, Value>,
    scope: &Scope,
  ) -> Result<Value, ErrorStack> {
    match callable {
      Callable::Builtin(name) => self.eval_fn_call(name, args, kwargs, scope, true),
      Callable::PartiallyAppliedFn(paf) => {
        let mut combined_args = paf.args.clone();
        combined_args.extend(args.iter().cloned());

        let mut combined_kwargs = paf.kwargs.clone();
        for (key, value) in kwargs {
          combined_kwargs.insert(key, value);
        }

        self.invoke_callable(&paf.inner, &combined_args, combined_kwargs, scope)
      }
      Callable::Closure(closure) => {
        // TODO: should do some basic analysis to see which variables are actually needed and avoid
        // cloning the rest
        let closure_scope = Scope::wrap(&closure.captured_scope);
        let mut pos_arg_ix = 0usize;
        for arg in &closure.params {
          if let Some(kwarg) = kwargs.get(&arg.name) {
            if let Some(type_hint) = arg.type_hint {
              type_hint
                .validate_val(kwarg)
                .map_err(|err| err.wrap(format!("Type error for closure kwarg `{}`", arg.name)))?;
            }
            closure_scope.insert(arg.name.clone(), kwarg.clone());
          } else if pos_arg_ix < args.len() {
            let pos_arg = &args[pos_arg_ix];
            if let Some(type_hint) = arg.type_hint {
              type_hint.validate_val(pos_arg).map_err(|err| {
                err.wrap(format!("Type error for closure pos arg `{}`", arg.name))
              })?;
            }
            closure_scope.insert(arg.name.clone(), pos_arg.clone());
            pos_arg_ix += 1;
          } else {
            return Err(ErrorStack::new(format!(
              "Missing required argument `{}` for closure",
              arg.name
            )));
          }
        }

        let mut out: Value = Value::Nil;
        for stmt in &closure.body {
          match stmt {
            Statement::Expr(expr) => {
              out = self.eval_expr(expr, &closure_scope)?;
            }
            Statement::Assignment {
              name,
              expr,
              type_hint,
            } => {
              self.eval_assignment(
                name,
                self.eval_expr(expr, &closure_scope)?,
                &closure_scope,
                *type_hint,
              )?;
            }
          }
        }

        if let Some(return_type_hint) = closure.return_type_hint {
          return_type_hint.validate_val(&out)?;
        }

        Ok(out)
      }
      Callable::ComposedFn(ComposedFn { inner }) => {
        let acc = args;
        let mut iter = inner.iter();
        let mut acc = self.invoke_callable(iter.next().unwrap(), acc, Default::default(), scope)?;
        for callable in iter {
          acc = self.invoke_callable(callable, &[acc], Default::default(), scope)?;
        }

        Ok(acc)
      }
    }
  }

  fn eval_field_access(&self, obj: &Expr, field: &str, scope: &Scope) -> Result<Value, ErrorStack> {
    let obj_value = self.eval_expr(obj, scope)?;
    match obj_value {
      Value::Vec3(v3) => match field.len() {
        1 => match field {
          "x" => Ok(Value::Float(v3.x)),
          "y" => Ok(Value::Float(v3.y)),
          "z" => Ok(Value::Float(v3.z)),
          _ => Err(ErrorStack::new(format!("Unknown field `{field}` for Vec3"))),
        },
        2 => Err(ErrorStack::new(format!("No vec2 type currently"))),
        3 => {
          let mut chars = field.chars();
          let a = chars.next().unwrap();
          let b = chars.next().unwrap();
          let c = chars.next().unwrap();

          let swiz = |c| match c {
            'x' => Ok(v3.x),
            'y' => Ok(v3.y),
            'z' => Ok(v3.z),
            _ => Err(ErrorStack::new(format!("Unknown field `{c}` for Vec3"))),
          };

          Ok(Value::Vec3(Vec3::new(swiz(a)?, swiz(b)?, swiz(c)?)))
        }
        _ => Err(ErrorStack::new(format!(
          "invalid swizzle; expected 1 or 3 chars"
        ))),
      },
      _ => Err(ErrorStack::new(format!(
        "field access not supported for type: {obj_value:?}"
      ))),
    }
  }
}

pub fn parse_and_eval_program_with_ctx(src: &str, ctx: &EvalCtx) -> Result<(), ErrorStack> {
  let pairs = GSParser::parse(Rule::program, src)
    .map_err(|err| ErrorStack::new(format!("{err}")).wrap("Syntax error"))?;

  let Some(program) = pairs.into_iter().next() else {
    return Err(ErrorStack::new("No program found in input"));
  };
  if program.as_rule() != Rule::program {
    return Err(ErrorStack::new(format!(
      "Expected top-level rule. Expected program, found: {:?}",
      program.as_rule()
    )));
  }

  let ast = parse_program(program.clone())?;

  for statement in ast.statements {
    let _val = ctx.eval_statement(&statement, &ctx.globals)?;
  }

  Ok(())
}

pub fn parse_and_eval_program(src: &str) -> Result<EvalCtx, ErrorStack> {
  let ctx = EvalCtx::default();
  parse_and_eval_program_with_ctx(src, &ctx)?;
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
is_even = |x| x % 2 == 0
seq = 0..10 | filter(is_even) | reduce(add)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let seq = &ctx.globals.get("seq").unwrap();
  let seq = seq.as_int().expect("Expected result to be an Int");
  assert_eq!(seq, 20); // 0 + 2 + 4 + 6 + 8
}

#[test]
fn test_nested_closures() {
  let src = r#"
a = 5
outer = |x| {
  inner = |y| x + y + a
  inner(2)
}

// `a` should have been captured at the time of closure creation, so modifying
// the value won't affect the result
a = 1000

result = outer(3)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let result = &ctx.globals.get("result").unwrap();
  let result = result.as_int().expect("Expected result to be an Int");
  assert_eq!(result, 10);
}

#[test]
fn test_multi_geometry_with_lambdas() {
  let src = r#"
positions = 0..10 | map(|x| vec3(x, 0, x))
meshes = positions | map(|pos| box(0.5) | translate(pos))
render(meshes)
"#;

  let ctx = parse_and_eval_program(src).unwrap();
  let rendered_meshes = ctx.rendered_meshes.into_inner();
  assert_eq!(rendered_meshes.len(), 10);

  for i in 0..10 {
    let mesh = &rendered_meshes[i];
    let center =
      mesh.vertices.values().map(|vtx| vtx.position).sum::<Vec3>() / mesh.vertices.len() as f32;
    assert_eq!(center, Vec3::new(i as f32, 0.0, i as f32));
  }
}

#[test]
fn test_compose() {
  let src = r#"
a = |x| x + 1
f = mul(2.5)

composed = compose(a, f)
result = composed(3)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let result = &ctx.globals.get("result").unwrap();
  let result = result.as_float().expect("Expected result to be a Float");
  assert_eq!(result, (3. + 1.) * 2.5);
}

#[test]
fn test_array_literals() {
  let src = r#"
fns = [|x| x + 1, |x| x * 2, |x| x - 3]
composed = fns | compose
res = composed(3)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let result = &ctx.globals.get("res").unwrap();
  let result = result.as_int().expect("Expected result to be an int");
  assert_eq!(result, ((3 + 1) * 2) - 3);
}

#[test]
fn test_mesh_bool_infix() {
  let src = r#"
a = box(1) | scale(2) | translate(0.5,0,0)
b = box(1) | scale(2) | translate(-0.5,0,0)
c = box(1) | translate(0, 0, 1)
render((a & b) | c)
"#;

  parse_and_eval_program(src).unwrap();
}

#[test]
fn test_type_hints() {
  let correct_src = r#"
a: int = 1
b: float = 2.5
c: float = b + a
"#;

  parse_and_eval_program(correct_src).unwrap();

  let incorrect_src = r#"
a: int = 1.0
"#;

  assert!(parse_and_eval_program(incorrect_src).is_err());

  let correct_closure_src = r#"
inc = |x|: float x + 1.0
inc(1)
"#;

  parse_and_eval_program(correct_closure_src).unwrap();

  let incorrect_closure_src = r#"
inc = |x|: int x + 1.0
inc(1.0)
"#;

  assert!(parse_and_eval_program(incorrect_closure_src).is_err());
}

#[test]
fn test_map_operator() {
  let src = r#"
a = 0..5 | map(|x| x * 2) | reduce(add)
b = 0..5 -> mul(2) | reduce(add)
c = 0..5 -> |x| { x * 2 } | reduce(add)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let a = &ctx.globals.get("a").unwrap();
  let a = a
    .as_int()
    .unwrap_or_else(|| panic!("Expected result to be an Int; found: {a:?}"));
  assert_eq!(a, 0 * 2 + 1 * 2 + 2 * 2 + 3 * 2 + 4 * 2);

  let b = &ctx.globals.get("b").unwrap();
  let b = b
    .as_int()
    .unwrap_or_else(|| panic!("Expected result to be an Int; found: {b:?}"));
  assert_eq!(a, b);

  let c = &ctx.globals.get("c").unwrap();
  let c = c
    .as_int()
    .unwrap_or_else(|| panic!("Expected result to be an Int; found: {c:?}"));
  assert_eq!(b, c);
}

#[test]
fn test_fn_arg_type_hint() {
  let src_good = r#"
add = |x: int, y: int| x + y
result = add(1, 2)
"#;

  let ctx_good = parse_and_eval_program(src_good).unwrap();
  let result_good = ctx_good.globals.get("result").unwrap();
  let result_good = result_good.as_int().expect("Expected result to be an Int");
  assert_eq!(result_good, 3);

  let src_bad: &'static str = r#"
add = |x: bool, y: bool| { x || y }
result = add(1, 2)
"#;

  assert!(parse_and_eval_program(src_bad).is_err());
}

#[test]
fn test_vec3_swizzle() {
  let src = r#"
a = vec3(1, 2, 3)
b = a.zyx
c = a.y
"#;

  let ctx = parse_and_eval_program(src).unwrap();
  let b = ctx.globals.get("b").unwrap();
  let Value::Vec3(b) = b else {
    panic!("Expected result to be a Vec3");
  };
  assert_eq!(b, Vec3::new(3.0, 2.0, 1.0));

  let c = ctx.globals.get("c").unwrap();
  let Value::Float(c) = c else {
    panic!("Expected result to be a Float");
  };
  assert_eq!(c, 2.);
}

#[test]
fn test_bool_literals() {
  let src = r#"
a = true
b = false
c = a && b
d = a || b
e = !a
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let a = ctx.globals.get("a").unwrap();
  let Value::Bool(a) = a else {
    panic!("Expected result to be a Bool");
  };
  assert!(a);

  let b = ctx.globals.get("b").unwrap();
  let Value::Bool(b) = b else {
    panic!("Expected result to be a Bool");
  };
  assert!(!b);

  let c = ctx.globals.get("c").unwrap();
  let Value::Bool(c) = c else {
    panic!("Expected result to be a Bool");
  };
  assert!(!c);

  let d = ctx.globals.get("d").unwrap();
  let Value::Bool(d) = d else {
    panic!("Expected result to be a Bool");
  };
  assert!(d);

  let e = ctx.globals.get("e").unwrap();
  let Value::Bool(e) = e else {
    panic!("Expected result to be a Bool");
  };
  assert!(!e);
}
