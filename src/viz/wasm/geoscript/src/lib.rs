#![feature(if_let_guard, impl_trait_in_bindings, adt_const_params, thread_local)]

#[cfg(target_arch = "wasm32")]
use std::cell::UnsafeCell;
use std::{
  any::Any,
  cell::{Cell, RefCell},
  fmt::{Debug, Display},
  rc::Rc,
  sync::Mutex,
};

use ast::{Expr, FunctionCallTarget, Statement};
use fxhash::{FxHashMap, FxHashSet};
use itertools::Itertools;
use mesh::{linked_mesh::Vec3, LinkedMesh};
use nalgebra::{Matrix4, Vector2};
use nanoserde::SerJson;
use parry3d::{
  bounding_volume::Aabb,
  shape::{TriMesh, TriMeshBuilderError},
};
use pest::{
  pratt_parser::{Assoc, Op, PrattParser},
  Parser,
};
use pest_derive::Parser;
use rand_pcg::Pcg32;
use seq::EagerSeq;
use smallvec::SmallVec;

#[cfg(target_arch = "wasm32")]
use crate::mesh_ops::mesh_boolean::get_last_manifold_err;
use crate::{
  ast::{parse_statement, ClosureArg, TypeName},
  builtins::{
    fn_defs::{ArgDef, DefaultValue, FnDef, FN_SIGNATURE_DEFS},
    resolve_builtin_impl, FUNCTION_ALIASES,
  },
  lights::{AmbientLight, Light},
  materials::Material,
  mesh_ops::mesh_boolean::{drop_manifold_mesh_handle, eval_mesh_boolean, MeshBooleanOp},
  seq::{ChainSeq, IntRange, MapSeq},
};

mod ast;
mod builtins;
pub mod lights;
pub mod materials;
pub mod mesh_ops;
pub mod noise;
pub mod path_building;
mod seq;

pub use self::ast::{optimize_ast, traverse_fn_calls, Program};
pub use self::builtins::fn_defs::serialize_fn_defs as get_serialized_builtin_fn_defs;

const PRELUDE: &str = include_str!("prelude.geo");

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

pub trait Sequence: Any + Debug {
  fn clone_box(&self) -> Box<dyn Sequence>;

  fn consume<'a>(
    self: Box<Self>,
    ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a>;
}

pub(crate) fn seq_as_eager<'a>(seq: &'a dyn Sequence) -> Option<&'a EagerSeq> {
  let seq: &dyn Any = &*seq;
  seq.downcast_ref::<EagerSeq>()
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
  captured_scope: Rc<Scope>,
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
  pub inner: Vec<Rc<Callable>>,
}

impl Debug for ComposedFn {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "<composed fn of {} inner callables>", self.inner.len())
  }
}

#[derive(Clone)]
pub struct PreResolvedSignature {
  arg_refs: Vec<ArgRef>,
  def_ix: usize,
}

#[derive(Clone)]
pub enum Callable {
  Builtin {
    name: String,
    fn_impl: fn(
      usize,
      &[ArgRef],
      &[Value],
      &FxHashMap<String, Value>,
      &EvalCtx,
    ) -> Result<Value, ErrorStack>,
    /// This will be set in the case that a single signature can be resolved in advance
    pre_resolved_signature: Option<PreResolvedSignature>,
    fn_signature_defs: &'static [FnDef],
  },
  PartiallyAppliedFn(PartiallyAppliedFn),
  Closure(Closure),
  ComposedFn(ComposedFn),
}

impl Debug for Callable {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Callable::Builtin {
        name,
        pre_resolved_signature,
        ..
      } => Debug::fmt(
        &format!(
          "<built-in fn \"{name}\"{}>",
          match pre_resolved_signature {
            Some(PreResolvedSignature {
              arg_refs: _,
              def_ix,
            }) => format!(" with signature {def_ix}"),
            None => String::new(),
          }
        ),
        f,
      ),
      Callable::PartiallyAppliedFn(paf) => Debug::fmt(&format!("{paf:?}"), f),
      Callable::Closure(closure) => Debug::fmt(&format!("{closure:?}"), f),
      Callable::ComposedFn(composed) => Debug::fmt(&format!("{composed:?}"), f),
    }
  }
}

impl Callable {
  pub fn is_side_effectful(&self) -> bool {
    match self {
      Callable::Builtin { name, .. } => {
        matches!(
          name.as_str(),
          "print" | "render" | "call" | "randv" | "randf" | "randi"
        )
      }
      Callable::PartiallyAppliedFn(paf) => paf.inner.is_side_effectful(),
      Callable::Closure(_) => false,
      Callable::ComposedFn(composed) => composed.inner.iter().any(|c| c.is_side_effectful()),
    }
  }
}

pub struct ManifoldHandle(Cell<usize>);

impl ManifoldHandle {
  pub fn new(handle: usize) -> Self {
    Self(Cell::new(handle))
  }

  pub fn new_empty() -> Self {
    Self(Cell::new(0))
  }

  pub fn get(&self) -> usize {
    self.0.get()
  }

  pub fn set(&self, handle: usize) {
    self.0.set(handle);
  }
}

pub struct MeshHandle {
  pub mesh: Rc<LinkedMesh<()>>,
  pub transform: Matrix4<f32>,
  pub manifold_handle: Rc<ManifoldHandle>,
  /// AABB of the mesh in world space.  Computed as needed.
  pub aabb: RefCell<Option<Aabb>>,
  /// parry3d trimesh representation of the mesh, if set.  Computed as needed - used for
  /// intersection tests and other operations.
  pub trimesh: RefCell<Option<Rc<TriMesh>>>,
  pub material: Option<Rc<Material>>,
}

impl MeshHandle {
  #[cfg(target_arch = "wasm32")]
  fn get_or_create_handle(&self) -> Result<usize, ErrorStack> {
    match self.manifold_handle.get() {
      0 => {
        let raw_mesh = self.mesh.to_raw_indexed(false, false, true);
        assert!(std::mem::size_of::<u32>() == std::mem::size_of::<usize>());
        let indices = unsafe {
          std::slice::from_raw_parts(
            raw_mesh.indices.as_ptr() as *const u32,
            raw_mesh.indices.len(),
          )
        };
        let verts = &raw_mesh.vertices;

        let handle = mesh_ops::mesh_boolean::create_manifold(verts, indices);
        if handle < 0 {
          let err = get_last_manifold_err();
          return Err(ErrorStack::new(err).wrap("Error creating manifold mesh"));
        }
        let handle = handle as usize;
        self.manifold_handle.set(handle);
        Ok(handle)
      }
      handle => Ok(handle),
    }
  }

  fn get_or_compute_aabb(&self) -> Aabb {
    if let Some(aabb) = self.aabb.borrow().as_ref() {
      return aabb.clone();
    }

    let aabb = self.mesh.compute_aabb(&self.transform);
    *self.aabb.borrow_mut() = Some(aabb.clone());
    aabb
  }

  fn get_or_create_trimesh(&self) -> Result<Rc<TriMesh>, TriMeshBuilderError> {
    if let Some(trimesh) = self.trimesh.borrow().as_ref() {
      return Ok(Rc::clone(trimesh));
    }

    let trimesh = Rc::new(self.mesh.build_trimesh(&self.transform)?);
    *self.trimesh.borrow_mut() = Some(Rc::clone(&trimesh));
    Ok(trimesh)
  }

  fn new(mesh: Rc<LinkedMesh<()>>) -> Self {
    Self {
      mesh,
      transform: Matrix4::identity(),
      manifold_handle: Rc::new(ManifoldHandle::new(0)),
      aabb: RefCell::new(None),
      trimesh: RefCell::new(None),
      material: None,
    }
  }

  fn clone(&self, retain_manifold_handle: bool, retain_aabb: bool, retain_trimesh: bool) -> Self {
    Self {
      mesh: Rc::clone(&self.mesh),
      transform: self.transform,
      manifold_handle: if retain_manifold_handle {
        Rc::clone(&self.manifold_handle)
      } else {
        Rc::new(ManifoldHandle::new(0))
      },
      aabb: if retain_aabb {
        self.aabb.clone()
      } else {
        RefCell::new(None)
      },
      trimesh: if retain_trimesh {
        self.trimesh.clone()
      } else {
        RefCell::new(None)
      },
      material: self.material.clone(),
    }
  }
}

impl Debug for MeshHandle {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "LinkedMesh {{ vertices: {}, faces: {}, edges: {}, manifold_handle: {:?} }}",
      self.mesh.vertices.len(),
      self.mesh.faces.len(),
      self.mesh.edges.len(),
      self.manifold_handle.get()
    )
  }
}

impl Drop for ManifoldHandle {
  fn drop(&mut self) {
    let handle = self.0.get();
    if handle != 0 {
      drop_manifold_mesh_handle(handle);
    }
  }
}

pub type Vec2 = Vector2<f32>;

pub enum Value {
  Int(i64),
  Float(f32),
  Vec2(Vec2),
  Vec3(Vec3),
  Mesh(Rc<MeshHandle>),
  Light(Box<Light>),
  Callable(Rc<Callable>),
  Sequence(Box<dyn Sequence>),
  Map(Box<FxHashMap<String, Value>>),
  Bool(bool),
  String(String),
  Material(Rc<Material>),
  Nil,
}

impl Clone for Value {
  fn clone(&self) -> Self {
    match self {
      Value::Int(i) => Value::Int(*i),
      Value::Float(f) => Value::Float(*f),
      Value::Vec2(v2) => Value::Vec2(*v2),
      Value::Vec3(v3) => Value::Vec3(*v3),
      Value::Mesh(mesh) => Value::Mesh(Rc::clone(mesh)),
      Value::Light(light) => Value::Light(light.clone()),
      Value::Callable(callable) => Value::Callable(callable.clone()),
      Value::Sequence(seq) => Value::Sequence(seq.clone_box()),
      Value::Map(map) => Value::Map(map.clone()),
      Value::Bool(b) => Value::Bool(*b),
      Value::String(s) => Value::String(s.clone()),
      Value::Material(material) => Value::Material(Rc::clone(material)),
      Value::Nil => Value::Nil,
    }
  }
}

impl Debug for Value {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Value::Int(i) => write!(f, "Int({i})"),
      Value::Float(fl) => write!(f, "Float({fl})"),
      Value::Vec2(v2) => write!(f, "Vec2({}, {})", v2.x, v2.y),
      Value::Vec3(v3) => write!(f, "Vec3({}, {}, {})", v3.x, v3.y, v3.z),
      Value::Mesh(mesh) => write!(f, "{mesh:?}"),
      Value::Light(light) => write!(f, "{light:?}"),
      Value::Callable(callable) => write!(f, "{callable:?}"),
      Value::Sequence(seq) => write!(f, "{seq:?}"),
      Value::Map(map) => write!(f, "{map:?}"),
      Value::Bool(b) => write!(f, "Bool({b})"),
      Value::String(s) => write!(f, "String({s})"),
      Value::Material(material) => write!(f, "Material({material:?})"),
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

  fn as_mesh(&self) -> Option<&MeshHandle> {
    match self {
      Value::Mesh(mesh) => Some(&mesh),
      _ => None,
    }
  }

  fn as_light(&self) -> Option<&Light> {
    match self {
      Value::Light(light) => Some(light.as_ref()),
      _ => None,
    }
  }

  fn as_sequence(&self) -> Option<&dyn Sequence> {
    match self {
      Value::Sequence(seq) => Some(seq.as_ref()),
      _ => None,
    }
  }

  fn as_map(&self) -> Option<&FxHashMap<String, Value>> {
    match self {
      Value::Map(map) => Some(map.as_ref()),
      _ => None,
    }
  }

  fn as_callable(&self) -> Option<&Callable> {
    match self {
      Value::Callable(callable) => Some(callable),
      _ => None,
    }
  }

  fn as_vec2(&self) -> Option<&Vec2> {
    match self {
      Value::Vec2(v2) => Some(v2),
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

  fn is_nil(&self) -> bool {
    matches!(self, Value::Nil)
  }

  fn as_str(&self) -> Option<&str> {
    match self {
      Value::String(s) => Some(s.as_str()),
      _ => None,
    }
  }

  fn as_material(&self, ctx: &EvalCtx) -> Option<Result<Rc<Material>, ErrorStack>> {
    match self {
      Value::Material(mat) => Some(Ok(Rc::clone(mat))),
      Value::String(s) => match ctx.materials.get(s) {
        Some(mat) => Some(Ok(Rc::clone(mat))),
        None => Some(Err(ErrorStack::new(format!(
          "Material not found: \"{s}\"\n\nAvailable materials: {:?}",
          ctx.materials.keys().collect_vec()
        )))),
      },
      _ => None,
    }
  }

  fn into_literal_expr(&self) -> Expr {
    Expr::Literal(self.clone())
  }

  fn get_type(&self) -> ArgType {
    match self {
      Value::Int(_) => ArgType::Int,
      Value::Float(_) => ArgType::Float,
      Value::Vec2(_) => ArgType::Vec2,
      Value::Vec3(_) => ArgType::Vec3,
      Value::Mesh(_) => ArgType::Mesh,
      Value::Light(_) => ArgType::Light,
      Value::Callable(_) => ArgType::Callable,
      Value::Sequence(_) => ArgType::Sequence,
      Value::Map(_) => ArgType::Map,
      Value::Bool(_) => ArgType::Bool,
      Value::String(_) => ArgType::String,
      Value::Material(_) => ArgType::Material,
      Value::Nil => ArgType::Nil,
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, SerJson)]
pub enum ArgType {
  Int,
  Float,
  Numeric,
  Vec2,
  Vec3,
  Mesh,
  Light,
  Callable,
  Sequence,
  Map,
  Bool,
  String,
  Material,
  Nil,
  Any,
}

impl ArgType {
  pub fn is_valid(&self, arg: &Value) -> bool {
    match self {
      ArgType::Int => matches!(arg, Value::Int(_)),
      ArgType::Float => matches!(arg, Value::Float(_)),
      ArgType::Numeric => matches!(arg, Value::Int(_) | Value::Float(_)),
      ArgType::Vec2 => matches!(arg, Value::Vec2(_)),
      ArgType::Vec3 => matches!(arg, Value::Vec3(_)),
      ArgType::Mesh => matches!(arg, Value::Mesh(_)),
      ArgType::Light => matches!(arg, Value::Light(_)),
      ArgType::Callable => matches!(arg, Value::Callable { .. }),
      ArgType::Sequence => matches!(arg, Value::Sequence(_)),
      ArgType::Map => matches!(arg, Value::Map(_)),
      ArgType::Bool => matches!(arg, Value::Bool(_)),
      ArgType::String => matches!(arg, Value::String(_)),
      ArgType::Material => matches!(arg, Value::Material(_) | Value::String(_)),
      ArgType::Nil => matches!(arg, Value::Nil),
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
      ArgType::Vec2 => "vec2",
      ArgType::Vec3 => "vec3",
      ArgType::Mesh => "mesh",
      ArgType::Light => "light",
      ArgType::Callable => "fn",
      ArgType::Sequence => "seq",
      ArgType::Map => "map",
      ArgType::Bool => "bool",
      ArgType::String => "str",
      ArgType::Material => "material",
      ArgType::Nil => "nil",
      ArgType::Any => "any",
    }
  }

  // TODO: this is a hack and limits the kind of type inference we can do.  Should use a fully
  // symbolic function signature resolution method instead of re-using `get_args`
  fn build_example_val(&self) -> Option<Value> {
    match self {
      ArgType::Int => Some(Value::Int(0)),
      ArgType::Float => Some(Value::Float(0.)),
      ArgType::Numeric => None,
      ArgType::Vec2 => Some(Value::Vec2(Vec2::new(0., 0.))),
      ArgType::Vec3 => Some(Value::Vec3(Vec3::new(0., 0., 0.))),
      ArgType::Mesh => Some(Value::Mesh(Rc::new(MeshHandle {
        mesh: Rc::new(LinkedMesh::new(0, 0, None)),
        transform: Matrix4::identity(),
        manifold_handle: Rc::new(ManifoldHandle::new(0)),
        aabb: RefCell::new(None),
        trimesh: RefCell::new(None),
        material: None,
      }))),
      ArgType::Light => Some(Value::Light(Box::new(Light::Ambient(
        AmbientLight::default(),
      )))),
      ArgType::Callable => Some(Value::Callable(Rc::new(Callable::Builtin {
        name: String::new(),
        fn_impl: |_, _, _, _, _| panic!("example callable should never be called"),
        pre_resolved_signature: None,
        fn_signature_defs: &[],
      }))),
      ArgType::Sequence => Some(Value::Sequence(Box::new(EagerSeq { inner: Vec::new() }))),
      ArgType::Map => Some(Value::Map(Box::new(FxHashMap::default()))),
      ArgType::Bool => Some(Value::Bool(false)),
      ArgType::String => Some(Value::String(String::new())),
      ArgType::Material => Some(Value::Material(Rc::new(Material::default()))),
      ArgType::Nil => Some(Value::Nil),
      ArgType::Any => None,
    }
  }
}

enum UnrealizedArgRef {
  Positional(usize),
  Keyword(&'static str),
  Default(fn() -> Value),
}

impl UnrealizedArgRef {
  pub fn realize(self) -> ArgRef {
    match self {
      UnrealizedArgRef::Positional(ix) => ArgRef::Positional(ix),
      UnrealizedArgRef::Keyword(name) => ArgRef::Keyword(name),
      UnrealizedArgRef::Default(get_default) => ArgRef::Default(get_default()),
    }
  }
}

#[derive(Clone, Debug)]
pub enum ArgRef {
  Positional(usize),
  Keyword(&'static str),
  Default(Value),
}

impl ArgRef {
  pub fn resolve<'a>(
    &'a self,
    args: &'a [Value],
    kwargs: &'a FxHashMap<String, Value>,
  ) -> &'a Value {
    match self {
      ArgRef::Positional(ix) => &args[*ix],
      ArgRef::Keyword(name) => kwargs.get(*name).expect("Keyword argument not found"),
      ArgRef::Default(val) => val,
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

fn format_fn_signatures(arg_defs: &[FnDef]) -> String {
  arg_defs
    .iter()
    .map(|def| {
      if def.arg_defs.is_empty() {
        return "  <no args>".to_owned();
      }

      let formatted_def = def
        .arg_defs
        .iter()
        .map(|arg_def| {
          let types_str = arg_def
            .valid_types
            .iter()
            .map(ArgType::as_str)
            .collect::<Vec<_>>()
            .join(" | ");
          format!("{}: {types_str}", arg_def.name)
        })
        .collect::<Vec<_>>()
        .join(", ");
      format!("  {formatted_def}")
    })
    .collect::<Vec<_>>()
    .join("\n")
}

pub(crate) fn build_no_fn_def_found_err(
  fn_name: &str,
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
  defs: &[FnDef],
) -> ErrorStack {
  ErrorStack::new(format!(
    "No valid function signature found for `{fn_name}` with args: {args:?}, kwargs: \
     {kwargs:?}\n\nAvailable signatures:\n{}",
    format_fn_signatures(defs)
  ))
}

fn get_args(
  fn_name: &str,
  defs: &[FnDef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<GetArgsOutput, ErrorStack> {
  // if the name of the first arg is empty, then the function is considered fully dynamic and no
  // type-checking/validation is performed
  if let Some(def) = defs.first() {
    if let Some(def) = def.arg_defs.first() {
      if def.name.is_empty() {
        return Ok(GetArgsOutput::Valid {
          def_ix: 0,
          arg_refs: Vec::new(),
        });
      }
    }
  }

  for key in kwargs.keys() {
    if !defs
      .iter()
      .any(|def| def.arg_defs.iter().any(|arg| arg.name == key))
    {
      return Err(ErrorStack::new(format!(
        "kwarg `{key}` is not valid in any function signature.\n\nAvailable signatures:\n{}",
        format_fn_signatures(defs)
      )));
    }
  }

  let mut arg_refs: SmallVec<[UnrealizedArgRef; 8]> = SmallVec::new();
  let mut valid_partial: bool = false;
  let any_args_provided = !args.is_empty() || !kwargs.is_empty();
  'def: for (def_ix, def) in defs.iter().enumerate() {
    let mut pos_arg_ix = 0;
    arg_refs.clear();
    'arg: for ArgDef {
      default_value,
      description: _,
      name,
      valid_types,
    } in def.arg_defs
    {
      // if a kwarg was passed which isn't defined in this function signature, skip
      for kwarg_key in kwargs.keys() {
        if def.arg_defs.iter().all(|def| def.name != *kwarg_key) {
          continue 'def;
        }
      }

      let (arg, arg_ref) = if let Some(kwarg) = kwargs.get(*name) {
        (kwarg, UnrealizedArgRef::Keyword(*name))
      } else if pos_arg_ix < args.len() {
        let arg = &args[pos_arg_ix];
        let arg_ref = UnrealizedArgRef::Positional(pos_arg_ix);
        pos_arg_ix += 1;
        (arg, arg_ref)
      } else {
        if let DefaultValue::Optional(get_default) = default_value {
          arg_refs.push(UnrealizedArgRef::Default(*get_default));
          continue 'arg;
        } else {
          // If any required argument is missing, mark as partial if any args/kwargs were provided
          if any_args_provided {
            valid_partial = true;
          }
          continue 'def;
        }
      };

      if !ArgType::any_valid(valid_types, arg) {
        continue 'def;
      }

      arg_refs.push(arg_ref);
    }

    // valid args found for the whole def, so the function call is valid
    let realized_arg_defs = arg_refs
      .into_iter()
      .map(UnrealizedArgRef::realize)
      .collect();
    return Ok(GetArgsOutput::Valid {
      def_ix,
      arg_refs: realized_arg_defs,
    });
  }

  if valid_partial {
    return Ok(GetArgsOutput::PartiallyApplied);
  }

  Err(build_no_fn_def_found_err(fn_name, args, kwargs, defs))
}

/// Specialized version of `get_args` for more efficient binary operator lookup.  Assumes that each
/// def in `defs` has exactly two args.
fn get_binop_def_ix(
  name: &str,
  defs: &[FnDef],
  lhs: &Value,
  rhs: &Value,
) -> Result<usize, ErrorStack> {
  for (def_ix, def) in defs.iter().enumerate() {
    let lhs_def = &def.arg_defs[0];
    let rhs_def = &def.arg_defs[1];
    if ArgType::any_valid(&lhs_def.valid_types, lhs)
      && ArgType::any_valid(&rhs_def.valid_types, rhs)
    {
      return Ok(def_ix);
    }
  }

  return Err(build_no_fn_def_found_err(
    name,
    &[lhs.clone(), rhs.clone()],
    &Default::default(),
    FN_SIGNATURE_DEFS[name],
  ));
}

/// Specialized version of `get_args` for more efficient binary operator lookup.  Assumes that each
/// def in `defs` has exactly two args.
///
/// Returns `(def_ix, return_types)`
fn get_binop_return_ty(
  name: &str,
  defs: &[FnDef],
  lhs: &Value,
  rhs: &Value,
) -> Result<&'static [ArgType], ErrorStack> {
  for def in defs {
    let lhs_def = &def.arg_defs[0];
    let rhs_def = &def.arg_defs[1];
    if ArgType::any_valid(&lhs_def.valid_types, lhs)
      && ArgType::any_valid(&rhs_def.valid_types, rhs)
    {
      return Ok(&def.return_type);
    }
  }

  return Err(build_no_fn_def_found_err(
    name,
    &[lhs.clone(), rhs.clone()],
    &Default::default(),
    FN_SIGNATURE_DEFS[name],
  ));
}

/// Specialized version of `get_args` for more efficient unary operator lookup.  Assumes that each
/// def in `defs` has exactly one arg.
fn get_unop_def_ix(name: &str, defs: &[FnDef], arg: &Value) -> Result<usize, ErrorStack> {
  for (def_ix, def) in defs.iter().enumerate() {
    let arg_def = &def.arg_defs[0];
    if ArgType::any_valid(&arg_def.valid_types, arg) {
      return Ok(def_ix);
    }
  }

  return Err(build_no_fn_def_found_err(
    name,
    &[arg.clone()],
    &Default::default(),
    FN_SIGNATURE_DEFS[name],
  ));
}

/// Specialized version of `get_args` for more efficient unary operator lookup.  Assumes that each
/// def in `defs` has exactly one arg.
fn get_unop_return_ty(
  name: &str,
  defs: &[FnDef],
  arg: &Value,
) -> Result<&'static [ArgType], ErrorStack> {
  for def in defs {
    let arg_def = &def.arg_defs[0];
    if ArgType::any_valid(&arg_def.valid_types, arg) {
      return Ok(&def.return_type);
    }
  }

  return Err(build_no_fn_def_found_err(
    name,
    &[arg.clone()],
    &Default::default(),
    FN_SIGNATURE_DEFS[name],
  ));
}

pub struct AppendOnlyBuffer<T> {
  // Using a `RefCell` here to avoid making the whole `EvalCtx` require `&mut` when evaluating
  // code.
  //
  // This thing is essentially "write-only" and this path is pretty cold, so it's fine imo.
  pub inner: RefCell<Vec<T>>,
}

impl<T> Default for AppendOnlyBuffer<T> {
  fn default() -> Self {
    AppendOnlyBuffer {
      inner: RefCell::new(Vec::new()),
    }
  }
}

impl<T> AppendOnlyBuffer<T> {
  pub fn push(&self, mesh: T) {
    self.inner.borrow_mut().push(mesh);
  }

  pub fn into_inner(self) -> Vec<T> {
    self.inner.into_inner()
  }

  pub fn len(&self) -> usize {
    self.inner.borrow().len()
  }
}

type RenderedMeshes = AppendOnlyBuffer<Rc<MeshHandle>>;
type RenderedLights = AppendOnlyBuffer<Light>;
type RenderedPaths = AppendOnlyBuffer<Vec<Vec3>>;

#[derive(Default, Debug)]
pub struct Scope {
  vars: Mutex<FxHashMap<String, Value>>,
  parent: Option<Rc<Scope>>,
}

impl Clone for Scope {
  fn clone(&self) -> Self {
    Scope {
      vars: Mutex::new(self.vars.lock().unwrap().clone()),
      parent: self.parent.as_ref().map(Rc::clone),
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

  fn wrap(parent: &Rc<Scope>) -> Scope {
    Scope {
      vars: Mutex::new(FxHashMap::default()),
      parent: Some(Rc::clone(parent)),
    }
  }
}

pub struct EvalCtx {
  pub globals: Scope,
  pub rendered_meshes: RenderedMeshes,
  pub rendered_lights: RenderedLights,
  pub rendered_paths: RenderedPaths,
  pub log_fn: fn(&str),
  #[cfg(target_arch = "wasm32")]
  rng: UnsafeCell<Pcg32>,
  pub materials: FxHashMap<String, Rc<Material>>,
  pub textures: FxHashSet<String>,
  pub default_material: RefCell<Option<Rc<Material>>>,
}

unsafe impl Send for EvalCtx {}
unsafe impl Sync for EvalCtx {}

impl Default for EvalCtx {
  fn default() -> Self {
    EvalCtx {
      globals: Scope::default_globals(),
      rendered_meshes: RenderedMeshes::default(),
      rendered_lights: RenderedLights::default(),
      rendered_paths: RenderedPaths::default(),
      log_fn: |msg| println!("{msg}"),
      #[cfg(target_arch = "wasm32")]
      rng: UnsafeCell::new(Pcg32::new(7718587666045340534, 17289744314186392832)),
      materials: FxHashMap::default(),
      textures: FxHashSet::default(),
      default_material: RefCell::new(None),
    }
  }
}

pub enum ControlFlow<T> {
  Continue(T),
  Break(T),
  Return(T),
}

#[cfg(not(target_arch = "wasm32"))]
#[thread_local]
static mut THREAD_RNG: Pcg32 =
  unsafe { std::mem::transmute((7718587666045340534u64, 17289744314186392832u64)) };

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
    unsafe { &mut *std::ptr::addr_of_mut!(THREAD_RNG) }
  }

  pub fn eval_expr(&self, expr: &Expr, scope: &Scope) -> Result<ControlFlow<Value>, ErrorStack> {
    match expr {
      Expr::Call(call) => {
        let mut args = Vec::with_capacity(call.args.len());
        for arg in &call.args {
          let val = match self.eval_expr(arg, scope)? {
            ControlFlow::Continue(val) => val,
            early_exit => return Ok(early_exit),
          };
          args.push(val);
        }

        let mut kwargs = FxHashMap::default();
        for (k, v) in &call.kwargs {
          let val = match self.eval_expr(v, scope)? {
            ControlFlow::Continue(val) => val,
            early_exit => return Ok(early_exit),
          };
          kwargs.insert(k.clone(), val);
        }

        match &call.target {
          FunctionCallTarget::Name(name) => {
            if let Some(global) = scope.get(name) {
              let Value::Callable(callable) = global else {
                return Err(ErrorStack::new(format!(
                  "\"{name}\" is not a callable; found: {global:?}"
                )));
              };

              return self
                .invoke_callable(&callable, &args, &kwargs, scope)
                .map_err(|err| err.wrap(format!("Error invoking callable `{name}`")))
                .map(ControlFlow::Continue);
            } else {
              return Err(ErrorStack::new(format!(
                "No variable found with name `{name}`"
              )));
            }
          }
          FunctionCallTarget::Literal(callable) => self
            .invoke_callable(callable, &args, &kwargs, scope)
            .map_err(|err| err.wrap("Error invoking callable")),
        }
        .map(ControlFlow::Continue)
      }
      Expr::BinOp { op, lhs, rhs } => {
        let lhs = match self.eval_expr(lhs, scope)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        let rhs = match self.eval_expr(rhs, scope)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        op.apply(self, lhs, rhs)
          .map(ControlFlow::Continue)
          .map_err(|err| err.wrap(format!("Error applying binary operator `{op:?}`")))
      }
      Expr::PrefixOp { op, expr } => {
        let val = match self.eval_expr(expr, scope)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        op.apply(val)
          .map(ControlFlow::Continue)
          .map_err(|err| err.wrap(format!("Error applying prefix operator `{op:?}`")))
      }
      Expr::Range {
        start,
        end,
        inclusive,
      } => {
        let start = match self.eval_expr(start, scope)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        let Value::Int(start) = start else {
          return Err(ErrorStack::new(format!(
            "Range start must be an integer, found: {start:?}"
          )));
        };
        let end = match self.eval_expr(end, scope)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        let Value::Int(end) = end else {
          return Err(ErrorStack::new(format!(
            "Range end must be an integer, found: {end:?}"
          )));
        };

        Ok(ControlFlow::Continue(if *inclusive {
          Value::Sequence(Box::new(IntRange {
            start,
            end: end + 1,
          }))
        } else {
          Value::Sequence(Box::new(IntRange { start, end }))
        }))
      }
      Expr::Ident(name) => self
        .eval_ident(name.as_str(), scope)
        .map(ControlFlow::Continue),
      Expr::Literal(val) => Ok(ControlFlow::Continue(val.clone())),
      Expr::ArrayLiteral(elems) => {
        let mut evaluated = Vec::with_capacity(elems.len());
        for elem in elems {
          let val = match self.eval_expr(elem, scope)? {
            ControlFlow::Continue(val) => val,
            early_exit => return Ok(early_exit),
          };
          evaluated.push(val);
        }
        Ok(ControlFlow::Continue(Value::Sequence(Box::new(EagerSeq {
          inner: evaluated,
        }))))
      }
      Expr::MapLiteral(map) => {
        let mut evaluated = Box::new(FxHashMap::default());
        for (key, value) in map {
          let val = match self.eval_expr(value, scope)? {
            ControlFlow::Continue(val) => val,
            early_exit => return Ok(early_exit),
          };
          evaluated.insert(key.clone(), val);
        }
        Ok(ControlFlow::Continue(Value::Map(evaluated)))
      }
      Expr::Closure {
        params,
        body,
        return_type_hint,
      } => Ok(ControlFlow::Continue(Value::Callable(Rc::new(
        Callable::Closure(Closure {
          params: params.clone(),
          body: body.0.clone(),
          // cloning the scope here makes the closure function like a rust `move` closure
          // where all the values are cloned before being moved into the closure.
          captured_scope: Rc::new(scope.clone()),
          return_type_hint: return_type_hint.clone(),
        }),
      )))),
      Expr::StaticFieldAccess { lhs: obj, field } => {
        let lhs = match self.eval_expr(obj, scope)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        self
          .eval_static_field_access(lhs, field)
          .map(ControlFlow::Continue)
      }
      Expr::FieldAccess { lhs, field } => {
        let lhs = match self.eval_expr(lhs, scope)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        let field = match self.eval_expr(field, scope)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        self
          .eval_field_access(lhs, field)
          .map(ControlFlow::Continue)
      }
      Expr::Conditional {
        cond,
        then,
        else_if_exprs,
        else_expr,
      } => {
        let cond = match self.eval_expr(cond, scope)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        let Value::Bool(cond) = cond else {
          return Err(ErrorStack::new(format!(
            "Condition passed to if statement must be a boolean; found: {cond:?}"
          )));
        };
        if cond {
          return self.eval_expr(then, scope);
        }
        for (else_if_cond, else_if_body) in else_if_exprs {
          let else_if_cond = match self.eval_expr(else_if_cond, scope)? {
            ControlFlow::Continue(val) => val,
            early_exit => return Ok(early_exit),
          };
          let Value::Bool(else_if_cond) = else_if_cond else {
            return Err(ErrorStack::new(format!(
              "Condition passed to else-if statement must be a boolean; found: {else_if_cond:?}"
            )));
          };
          if else_if_cond {
            return self.eval_expr(else_if_body, scope);
          }
        }
        if let Some(else_expr) = else_expr {
          return self.eval_expr(else_expr, scope);
        }

        Ok(ControlFlow::Continue(Value::Nil))
      }
      Expr::Block { statements } => {
        // TODO: ideally, we'd avoid cloning the scope here and use the scope nesting functionality
        // like closures.  However, adding in references to scopes creates incredibly lifetime
        // headaches across the whole codebase very quickly and just isn't worth it rn
        let block_scope = Scope::wrap(&Rc::new(scope.clone()));
        let mut last_value = Value::Nil;

        for statement in statements {
          last_value = match self.eval_statement(statement, &block_scope)? {
            ControlFlow::Continue(val) => val,
            ControlFlow::Break(val) => {
              last_value = val;
              break;
            }
            ControlFlow::Return(val) => {
              for (key, val) in block_scope.vars.into_inner().unwrap() {
                let exists = scope.get(&key).is_some();
                if exists {
                  scope.insert(key, val);
                }
              }

              return Ok(ControlFlow::Return(val));
            }
          };
          if let Statement::Assignment { .. } = statement {
            last_value = Value::Nil;
          }
        }

        for (key, val) in block_scope.vars.into_inner().unwrap() {
          let exists = scope.get(&key).is_some();
          if exists {
            scope.insert(key, val);
          }
        }

        Ok(ControlFlow::Continue(last_value))
      }
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

  fn eval_statement(
    &self,
    statement: &Statement,
    scope: &Scope,
  ) -> Result<ControlFlow<Value>, ErrorStack> {
    match statement {
      Statement::Expr(expr) => self.eval_expr(expr, scope),
      Statement::Assignment {
        name,
        expr,
        type_hint,
      } => {
        let val = match self.eval_expr(expr, scope)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        self
          .eval_assignment(&name, val, scope, *type_hint)
          .map(ControlFlow::Continue)
      }
      Statement::Return { value } => {
        let value = if let Some(value) = value {
          match self.eval_expr(value, scope)? {
            ControlFlow::Continue(val) => val,
            early_exit => return Ok(early_exit),
          }
        } else {
          Value::Nil
        };
        Ok(ControlFlow::Return(value))
      }
      Statement::Break { value } => {
        let value = if let Some(value) = value {
          match self.eval_expr(value, scope)? {
            ControlFlow::Continue(val) => val,
            early_exit => return Ok(early_exit),
          }
        } else {
          Value::Nil
        };
        Ok(ControlFlow::Break(value))
      }
    }
  }

  fn fold<'a>(
    &'a self,
    initial_val: Value,
    callable: &Callable,
    seq: Box<dyn Sequence>,
  ) -> Result<Value, ErrorStack> {
    // if we're applying a mesh boolean op here, we can use the fast path that avoids the overhead
    // of encoding/decoding intermediate meshes
    if let Callable::Builtin { name, .. } = callable {
      if matches!(name.as_str(), "union" | "difference" | "intersect") {
        let combined_iter = ChainSeq::new(
          self,
          Box::new(EagerSeq {
            inner: vec![initial_val, Value::Sequence(seq)],
          }),
        )
        .map_err(|err| {
          err.wrap("Internal error creating chained sequence when folding mesh boolean op")
        })?;
        return eval_mesh_boolean(
          1,
          &[ArgRef::Positional(0), ArgRef::Positional(1)],
          &[Value::Sequence(Box::new(combined_iter))],
          &Default::default(),
          self,
          MeshBooleanOp::from_str(name),
        )
        .map_err(|err| err.wrap("Error invoking mesh boolean op in `fold`"));
      }
    }

    let mut acc = initial_val;
    let iter = seq.consume(self);
    for (i, res) in iter.enumerate() {
      let value = res.map_err(|err| {
        err.wrap(format!(
          "Error produced when evaluating item ix={i} in seq passed to reduce"
        ))
      })?;
      acc = self
        .invoke_callable(callable, &[acc, value], &Default::default(), &self.globals)
        .map_err(|err| err.wrap("Error invoking callable in reduce".to_owned()))?;
    }

    Ok(acc)
  }

  fn reduce<'a>(
    &'a self,
    fn_value: &Callable,
    seq: Box<dyn Sequence>,
  ) -> Result<Value, ErrorStack> {
    // if we're applying a mesh boolean op here, we can use the fast path that avoids the overhead
    // of encoding/decoding intermediate meshes
    if let Callable::Builtin { name, .. } = fn_value {
      if matches!(name.as_str(), "union" | "difference" | "intersect") {
        return eval_mesh_boolean(
          1,
          &[ArgRef::Positional(0), ArgRef::Positional(1)],
          &[Value::Sequence(seq)],
          &Default::default(),
          self,
          MeshBooleanOp::from_str(name),
        )
        .map_err(|err| err.wrap("Error invoking mesh boolean op in `reduce`"));
      }
    }

    let mut iter = seq.clone_box().consume(self);
    let Some(first_value_res) = iter.next() else {
      return Err(ErrorStack::new("empty sequence passed to reduce"));
    };

    let mut acc =
      first_value_res.map_err(|err| err.wrap("Error evaluating initial value in reduce"))?;
    for (i, res) in iter.enumerate() {
      let value = res.map_err(|err| {
        err.wrap(format!(
          "Error produced when evaluating item ix={i} in seq passed to reduce"
        ))
      })?;
      acc = self
        .invoke_callable(fn_value, &[acc, value], &Default::default(), &self.globals)
        .map_err(|err| err.wrap("Error invoking callable in reduce".to_owned()))?;
    }
    Ok(acc)
  }

  pub(crate) fn eval_ident(&self, name: &str, scope: &Scope) -> Result<Value, ErrorStack> {
    if let Some(local) = scope.get(name) {
      return Ok(local);
    }

    // look it up as a builtin fn
    if FN_SIGNATURE_DEFS.contains_key(&name) {
      return Ok(Value::Callable(Rc::new(Callable::Builtin {
        name: name.to_owned(),
        fn_impl: resolve_builtin_impl(name),
        fn_signature_defs: &FN_SIGNATURE_DEFS[name],
        pre_resolved_signature: None,
      })));
    }

    Err(ErrorStack::new(format!("Variable `{name}` not defined",)))
  }

  fn invoke_callable(
    &self,
    callable: &Callable,
    args: &[Value],
    kwargs: &FxHashMap<String, Value>,
    scope: &Scope,
  ) -> Result<Value, ErrorStack> {
    match callable {
      Callable::Builtin {
        name,
        fn_impl,
        fn_signature_defs,
        pre_resolved_signature,
      } => match pre_resolved_signature {
        Some(PreResolvedSignature { arg_refs, def_ix }) => {
          fn_impl(*def_ix, arg_refs, args, kwargs, self)
        }
        None => {
          let arg_refs = get_args(name, fn_signature_defs, args, kwargs)?;
          let (def_ix, arg_refs) = match arg_refs {
            GetArgsOutput::Valid { def_ix, arg_refs } => (def_ix, arg_refs),
            GetArgsOutput::PartiallyApplied => {
              return Ok(Value::Callable(Rc::new(Callable::PartiallyAppliedFn(
                PartiallyAppliedFn {
                  inner: Box::new(callable.clone()),
                  args: args.to_owned(),
                  kwargs: kwargs.clone(),
                },
              ))))
            }
          };
          fn_impl(def_ix, &arg_refs, args, kwargs, self)
        }
      }
      .map_err(|err| err.wrap(format!("Error invoking builtin function `{name}`"))),
      Callable::PartiallyAppliedFn(paf) => {
        let mut combined_args = paf.args.clone();
        combined_args.extend(args.iter().cloned());

        let mut combined_kwargs = paf.kwargs.clone();
        for (key, value) in kwargs {
          combined_kwargs.insert(key.clone(), value.clone());
        }

        self.invoke_callable(&paf.inner, &combined_args, &combined_kwargs, scope)
      }
      Callable::Closure(closure) => {
        // TODO: should do some basic analysis to see which variables are actually needed and avoid
        // cloning the rest
        let closure_scope = Scope::wrap(&closure.captured_scope);
        let mut pos_arg_ix = 0usize;
        let mut any_args_valid = false;
        let mut invalid_arg_ix = None;
        for arg in &closure.params {
          if let Some(kwarg) = kwargs.get(&arg.name) {
            if let Some(type_hint) = arg.type_hint {
              type_hint
                .validate_val(kwarg)
                .map_err(|err| err.wrap(format!("Type error for closure kwarg `{}`", arg.name)))?;
            }
            closure_scope.insert(arg.name.clone(), kwarg.clone());
            any_args_valid = true;
          } else if pos_arg_ix < args.len() {
            let pos_arg = &args[pos_arg_ix];
            if let Some(type_hint) = arg.type_hint {
              type_hint.validate_val(pos_arg).map_err(|err| {
                err.wrap(format!("Type error for closure pos arg `{}`", arg.name))
              })?;
            }
            closure_scope.insert(arg.name.clone(), pos_arg.clone());
            any_args_valid = true;
            pos_arg_ix += 1;
          } else {
            if let Some(default_val) = &arg.default_val {
              let default_val = self.eval_expr(default_val, &closure_scope)?;
              let default_val = match default_val {
                ControlFlow::Continue(val) => val,
                ControlFlow::Return(_) => {
                  return Err(ErrorStack::new(format!(
                    "`return` isn't valid in arg default value expressions; found in default \
                     value for arg `{}`",
                    arg.name
                  )))
                }
                ControlFlow::Break(_) => {
                  return Err(ErrorStack::new(format!(
                    "`break` isn't valid in arg default value expressions; found in default value \
                     for arg `{}`",
                    arg.name
                  )));
                }
              };
              closure_scope.insert(arg.name.clone(), default_val);
            } else {
              if invalid_arg_ix.is_none() {
                invalid_arg_ix = Some(pos_arg_ix);
              }
            }
          }
        }

        if let Some(invalid_arg_ix) = invalid_arg_ix {
          let invalid_arg = &closure.params[invalid_arg_ix];
          if any_args_valid {
            return Ok(Value::Callable(Rc::new(Callable::PartiallyAppliedFn(
              PartiallyAppliedFn {
                inner: Box::new(Callable::Closure(closure.clone())),
                args: args.to_owned(),
                kwargs: kwargs.clone(),
              },
            ))));
          } else {
            return Err(ErrorStack::new(format!(
              "Missing required argument `{}` for closure",
              invalid_arg.name
            )));
          }
        }

        let mut out: Value = Value::Nil;
        for stmt in &closure.body {
          match stmt {
            Statement::Expr(expr) => match self.eval_expr(expr, &closure_scope)? {
              ControlFlow::Continue(val) => out = val,
              ControlFlow::Return(val) => {
                out = val;
                break;
              }
              ControlFlow::Break(_) => {
                return Err(ErrorStack::new(
                  "`break` isn't valid at the top level of a closure",
                ))
              }
            },
            Statement::Assignment {
              name,
              expr,
              type_hint,
            } => {
              self.eval_assignment(
                name,
                match self.eval_expr(expr, &closure_scope)? {
                  ControlFlow::Continue(val) => val,
                  ControlFlow::Return(val) => {
                    out = val;
                    break;
                  }
                  ControlFlow::Break(_) => {
                    return Err(ErrorStack::new(
                      "`break` isn't valid at the top level of a closure",
                    ))
                  }
                },
                &closure_scope,
                *type_hint,
              )?;
            }
            Statement::Return { value } => {
              out = if let Some(value) = value {
                match self.eval_expr(value, &closure_scope)? {
                  ControlFlow::Continue(val) => val,
                  ControlFlow::Return(val) => {
                    out = val;
                    break;
                  }
                  ControlFlow::Break(_) => {
                    return Err(ErrorStack::new(
                      "`break` isn't valid at the top level of a closure",
                    ))
                  }
                }
              } else {
                Value::Nil
              };
              return Ok(out);
            }
            Statement::Break { .. } => {
              return Err(ErrorStack::new(
                "`break` isn't valid at the top level of a closure",
              ));
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
        let mut acc =
          self.invoke_callable(iter.next().unwrap(), acc, &Default::default(), scope)?;
        for callable in iter {
          acc = self.invoke_callable(callable, &[acc], &Default::default(), scope)?;
        }

        Ok(acc)
      }
    }
  }

  fn eval_static_field_access(&self, lhs: Value, field: &str) -> Result<Value, ErrorStack> {
    match lhs {
      Value::Vec2(v2) => match field.len() {
        1 => match field {
          "x" => Ok(Value::Float(v2.x)),
          "y" => Ok(Value::Float(v2.y)),
          _ => Err(ErrorStack::new(format!("Unknown field `{field}` for Vec2"))),
        },
        2 => {
          let mut chars = field.chars();
          let a = chars.next().unwrap();
          let b = chars.next().unwrap();

          let swiz = |c| match c {
            'x' => Ok(v2.x),
            'y' => Ok(v2.y),
            _ => Err(ErrorStack::new(format!("Unknown field `{c}` for Vec2"))),
          };

          Ok(Value::Vec2(Vec2::new(swiz(a)?, swiz(b)?)))
        }
        _ => Err(ErrorStack::new(format!(
          "invalid swizzle; expected 1 or 2 chars"
        ))),
      },
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
      Value::Map(map) => Ok(if let Some(val) = map.get(field) {
        val.clone()
      } else {
        Value::Nil
      }),
      _ => Err(ErrorStack::new(format!(
        "field access not supported for type: {lhs:?}"
      ))),
    }
  }

  fn eval_field_access(&self, lhs: Value, field: Value) -> Result<Value, ErrorStack> {
    match field {
      Value::String(s) => self.eval_static_field_access(lhs, &s),
      Value::Int(i) => match lhs {
        Value::Vec2(v2) => match i {
          0 => Ok(Value::Float(v2.x)),
          1 => Ok(Value::Float(v2.y)),
          _ => Err(ErrorStack::new(format!("Index {i} out of bounds for Vec2"))),
        },
        Value::Vec3(v3) => match i {
          0 => Ok(Value::Float(v3.x)),
          1 => Ok(Value::Float(v3.y)),
          2 => Ok(Value::Float(v3.z)),
          _ => Err(ErrorStack::new(format!("Index {i} out of bounds for Vec3"))),
        },
        Value::Map(map) => Ok(if let Some(val) = map.get(&i.to_string()) {
          val.clone()
        } else {
          Value::Nil
        }),
        Value::Sequence(seq) => {
          if i < 0 {
            return Err(ErrorStack::new(format!(
              "negative index {i} not supported for sequences"
            )));
          }

          let Some(eager_seq) = seq_as_eager(&*seq) else {
            return Err(ErrorStack::new(
              "sequence is not eager and must be realized with `collect` before indexing",
            ));
          };

          eager_seq.inner.get(i as usize).cloned().ok_or_else(|| {
            ErrorStack::new(format!(
              "Index {i} out of bounds for sequence; len={}",
              eager_seq.inner.len()
            ))
          })
        }
        _ => Err(ErrorStack::new(format!(
          "field access not supported for type: {lhs:?}"
        ))),
      },
      other => Err(ErrorStack::new(format!(
        "field access not supported for {:?}[{:?}]",
        lhs.get_type(),
        other.get_type()
      ))),
    }
  }
}

pub(crate) fn parse_program_src<'a>(src: &'a str) -> Result<Program, ErrorStack> {
  let pairs = GSParser::parse(Rule::program, src)
    .map_err(|err| ErrorStack::new(format!("{err}")).wrap("Syntax error"))?;
  let Some(program) = pairs.into_iter().next() else {
    return Err(ErrorStack::new("No program found in input"));
  };

  if program.as_rule() != Rule::program {
    return Err(ErrorStack::new(format!(
      "`parse_program` can only handle `program` rules, found: {:?}",
      program.as_rule()
    )));
  }

  let statements = program
    .into_inner()
    .map(parse_statement)
    .filter_map_ok(|s| s)
    .collect::<Result<Vec<_>, ErrorStack>>()?;

  Ok(Program { statements })
}

pub fn parse_program_maybe_with_prelude(
  src: String,
  include_prelude: bool,
) -> Result<Program, ErrorStack> {
  let src = if include_prelude {
    format!("{}\n{src}", PRELUDE)
  } else {
    src
  };
  parse_program_src(&src)
}

pub fn eval_program_with_ctx(ctx: &EvalCtx, ast: &Program) -> Result<(), ErrorStack> {
  for statement in &ast.statements {
    let _val = match ctx.eval_statement(&statement, &ctx.globals)? {
      ControlFlow::Continue(val) => val,
      ControlFlow::Break(_) => {
        return Err(ErrorStack::new(
          "`break` outside of a function is not allowed",
        ))
      }
      ControlFlow::Return(_) => {
        return Err(ErrorStack::new(format!(
          "`return` outside of a function is not allowed; found"
        )));
      }
    };
  }

  Ok(())
}

pub fn parse_and_eval_program_with_ctx(
  src: String,
  ctx: &EvalCtx,
  include_prelude: bool,
) -> Result<(), ErrorStack> {
  let mut ast = parse_program_maybe_with_prelude(src, include_prelude)
    .map_err(|err| err.wrap("Error parsing program"))?;

  optimize_ast(ctx, &mut ast)?;

  eval_program_with_ctx(ctx, &ast).map_err(|err| err.wrap("Error evaluating program"))?;

  Ok(())
}

pub fn parse_and_eval_program(src: impl Into<String>) -> Result<EvalCtx, ErrorStack> {
  let ctx = EvalCtx::default();
  parse_and_eval_program_with_ctx(src.into(), &ctx, false)?;
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
  assert_eq!(mesh.mesh.vertices.len(), 8);
  for vtx in mesh.mesh.vertices.values() {
    let pos = (mesh.transform * vtx.position.push(1.)).xyz();
    assert_eq!(pos.x.abs(), 2.0);
    assert_eq!(pos.y.abs(), 4.0);
    assert_eq!(pos.z.abs(), 2.0);
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
  assert_eq!(mesh.mesh.vertices.len(), 8);
  for vtx in mesh.mesh.vertices.values() {
    let pos = (mesh.transform * vtx.position.push(1.)).xyz();
    assert_eq!(pos.x.abs(), 1. / 2.);
    assert_eq!(pos.y.abs(), 2. / 2.);
    assert_eq!(pos.z.abs(), 3. / 2.);
  }
}

#[test]
fn test_partial_application_with_only_kwargs() {
  static ARGS: &[ArgDef] = &[
    ArgDef {
      name: "x",
      valid_types: &[ArgType::Int],
      default_value: builtins::fn_defs::DefaultValue::Required,
      description: "",
    },
    ArgDef {
      name: "y",
      valid_types: &[ArgType::Int],
      default_value: builtins::fn_defs::DefaultValue::Required,
      description: "",
    },
  ];
  let defs = &[FnDef {
    arg_defs: ARGS,
    description: "",
    return_type: &[ArgType::Any],
  }];
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
  static ARGS: &[ArgDef] = &[
    ArgDef {
      name: "x",
      valid_types: &[ArgType::Int],
      default_value: builtins::fn_defs::DefaultValue::Required,
      description: "",
    },
    ArgDef {
      name: "y",
      valid_types: &[ArgType::Int],
      default_value: builtins::fn_defs::DefaultValue::Required,
      description: "",
    },
  ];
  let defs = &[FnDef {
    arg_defs: ARGS,
    description: "",
    return_type: &[ArgType::Any],
  }];

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
a = lerp(0.5, vec3(0,0,0), vec3(1,1,1))
b = 0.5 | lerp(a=0.0, b=1)
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
    let center = mesh
      .mesh
      .vertices
      .values()
      .map(|vtx| {
        println!("xform: {:?}", mesh.transform);
        let pt = vtx.position.push(1.);
        let transformed = mesh.transform * pt;
        let transformed = transformed.xyz();
        println!("{:?} -> {:?}", vtx.position, transformed);
        transformed
      })
      .sum::<Vec3>()
      / mesh.mesh.vertices.len() as f32;
    assert_eq!(center, Vec3::new(i as f32, 0., i as f32));
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
x = pi*2
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

#[test]
fn test_take_while_skip() {
  let src = r#"
vals = [-100,-400,1,2,3,4,5,1];
out = vals | skip(2) | take_while(|x| x < 4) | reduce(add)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let out = ctx.globals.get("out").unwrap();
  let out = out.as_int().expect("Expected result to be an Int");
  assert_eq!(out, 1 + 2 + 3);
}

#[test]
fn test_negative_ints() {
  let src = r#"
a = -1
"#;

  let ctx = parse_and_eval_program(src).unwrap();
  let a = ctx.globals.get("a").unwrap();
  let a = a.as_int().expect("Expected result to be an Int");
  assert_eq!(a, -1);
}

#[test]
fn test_if_elseif_else_return() {
  let src = r#"
out = [1,2,3,4] -> |x| {
  if x == 1 || x == 4 {
    return 0;
  } else if x == 2 {
    return 100
  } else {
    return 200;
  }
  return -100
} | reduce(add)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let out = ctx.globals.get("out").unwrap();
  let out = out
    .as_int()
    .unwrap_or_else(|| panic!("Expected result to be an Int; found: {out:?}"));
  assert_eq!(out, 0 + 100 + 200);
}

#[test]
fn test_funny_return() {
  let src = r#"
fn = || {
  x = 100
  return {
    x = 200
    return 2
    { 1 }
  }
}
out = fn()
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let out = ctx.globals.get("out").unwrap();
  let out = out
    .as_int()
    .unwrap_or_else(|| panic!("Expected result to be an Int; found: {out:?}"));
  assert_eq!(out, 2);
}

#[test]
fn test_invalid_return_outside_fn() {
  let src = r#"
a = if { return true} {1} else {2}
"#;

  assert!(parse_and_eval_program(src).is_err());
}

#[test]
fn test_break_block() {
  let src = r#"
fn = |x| {
  out = {
    if x == 0 {
      break 100
      4 // never reached
    } else {
      x
    }
  }
  out + 1
}
out = [0, 1, 2] -> fn | reduce(add)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let out = ctx.globals.get("out").unwrap();
  let out = out
    .as_int()
    .unwrap_or_else(|| panic!("Expected result to be an Int; found: {out:?}"));
  assert_eq!(out, (100 + 1) + (1 + 1) + (2 + 1));
}

#[test]
fn test_builtin_optional_arg() {
  let src = r#"
x: mesh = [vec3(0), vec3(1)] | extrude_pipe(radius=0.5, resolution=3)
"#;

  parse_and_eval_program(src).unwrap();
}

#[test]
fn test_arg_default_values_syntax() {
  let src = r#"
fn = |x: int = 10| {
  return x + 1
}"#;

  parse_and_eval_program(src).unwrap();
}

#[test]
fn test_closure_def_with_optional_arg() {
  let src = r#"
pre = 400
outer = |x: int = 100, y = pre, z:int={pre -100}, w: int = (1 - 1)| {
  return x + y + z + w
};
result = outer(w=500, 3)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let result = ctx.globals.get("result").unwrap();
  let result = result.as_int().expect("Expected result to be an Int");
  assert_eq!(result, 3 + 400 + 300 + 500);
}

#[test]
fn test_example_programs() {
  use std::fs;

  // either slow examples or those which rely on non-dummy CSG that isn't available outside of wasm
  let parse_only = &["deathstar"];

  let examples_dir = "./examples";
  let fnames = fs::read_dir(examples_dir)
    .expect("Failed to read examples directory")
    .map(|res| res.unwrap())
    .filter(|entry| entry.path().extension().map_or(false, |ext| ext == "geo"))
    .map(|entry| entry.path());

  // every example should parse and evaluate without errors
  for path in fnames {
    let src = fs::read_to_string(&path).expect("Failed to read example file");
    let example_name = path.file_stem().unwrap().to_str().unwrap();
    println!("Testing example: {example_name}");

    if parse_only.contains(&example_name) {
      match GSParser::parse(Rule::program, &src) {
        Ok(_) => println!("Example {example_name} parsed successfully"),
        Err(err) => panic!("Example {example_name} failed to parse with error: {err}"),
      }
      continue;
    }

    match parse_and_eval_program(&src) {
      Ok(_) => println!("Example {example_name} passed"),
      Err(err) => panic!("Example {example_name} failed with error: {err}"),
    }
  }
}

#[test]
fn test_string_literals() {
  let src = r#"
c = "'as\"df'"
a = "asdf"
b = '"as"df"'
d = 'as\'df'
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let (a, b, c, d) = (
    ctx.globals.get("a").unwrap(),
    ctx.globals.get("b").unwrap(),
    ctx.globals.get("c").unwrap(),
    ctx.globals.get("d").unwrap(),
  );
  let (a, b, c, d) = (
    a.as_str().unwrap(),
    b.as_str().unwrap(),
    c.as_str().unwrap(),
    d.as_str().unwrap(),
  );
  assert_eq!(a, "asdf");
  assert_eq!(b, "\"as\"df\"");
  assert_eq!(c, "'as\"df'");
  assert_eq!(d, "as'df");
}

#[test]
fn test_closure_partial_application() {
  let src = r#"
myadd = |a,b| a+b
inc = myadd(1)
out = inc(2)

inc2 = myadd(b=1)
inc2(2) | print
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let out = ctx.globals.get("out").unwrap();
  let out = out.as_int().expect("Expected result to be an Int");
  assert_eq!(out, 3);
}

#[test]
fn test_skip_while() {
  let src = r#"
out = [1,2,3,4,5] | skip_while(|x| x < 3) | reduce(add)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let out = ctx.globals.get("out").unwrap();
  let out = out.as_int().expect("Expected result to be an Int");
  assert_eq!(out, 3 + 4 + 5);
}

#[test]
fn test_chain() {
  let src = r#"
a = 0..=2
b = [3,4,5]
c = (0..=1 | take(2)) -> |x| x + 1
chained = chain([a,b,c])
out = chained | reduce(add)
first = chained | first
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let out = ctx.globals.get("out").unwrap();
  let out = out.as_int().expect("Expected result to be an Int");
  assert_eq!(out, 0 + 1 + 2 + 3 + 4 + 5 + 1 + 2);

  let first = ctx.globals.get("first").unwrap();
  let first = first.as_int().expect("Expected result to be an Int");
  assert_eq!(first, 0, "Expected first element to be 0, found: {first}");
}

#[test]
fn test_map_literal_parsing() {
  let src = r#"
a = { x: 1, "y+": 1+4, z: 3 }
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let a = ctx.globals.get("a").unwrap();
  let Value::Map(a) = a else {
    panic!("Expected result to be a Map");
  };
  assert_eq!(a.len(), 3);
  assert_eq!(a.get("x").unwrap().as_int(), Some(1));
  assert_eq!(a.get("y+").unwrap().as_int(), Some(5));
  assert_eq!(a.get("z").unwrap().as_int(), Some(3));
}

#[test]
fn test_map_field_access() {
  let src = r#"
map = {x:1}
x = map.x
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let x = ctx.globals.get("x").unwrap();
  let x = x.as_int().expect("Expected result to be an Int");
  assert_eq!(x, 1);
}

#[test]
fn test_dynamic_map_access() {
  let src = r#"
map = {x:1, y:2, z:3}
x = map["x"]
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let x = ctx.globals.get("x").unwrap();
  let x = x.as_int().expect("Expected result to be an Int");
  assert_eq!(x, 1);
}

#[test]
fn test_hex_int_literal_parsing() {
  let src = r#"
a = 0x1234
b = 0x001
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let a = ctx.globals.get("a").unwrap();
  let a = a.as_int().expect("Expected result to be an Int");
  assert_eq!(a, 0x1234);

  let b = ctx.globals.get("b").unwrap();
  let b = b.as_int().expect("Expected result to be an Int");
  assert_eq!(b, 0x001);
}

#[test]
fn test_prelude() {
  let src = PRELUDE;
  let mut ctx = EvalCtx::default();
  ctx.materials.insert(
    "default".to_owned(),
    Rc::new(Material::External("default".to_owned())),
  );
  parse_and_eval_program_with_ctx(src.to_owned(), &ctx, false).unwrap();
}

#[test]
fn test_call_callback() {
  let src = r#"
fn = |cb| cb(123)
fn2 = |cb| cb | call(args=[123])

out1 = fn(|x| x + 1)
out2 = fn2(|x| x + 2)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let out1 = ctx.globals.get("out1").unwrap();
  let out1 = out1.as_int().expect("Expected result to be an Int");
  assert_eq!(out1, 124);

  let out2 = ctx.globals.get("out2").unwrap();
  let out2 = out2.as_int().expect("Expected result to be an Int");
  assert_eq!(out2, 125);
}

#[test]
fn test_mesh_map_warp_shorthand() {
  let src = r#"
a = box(1)
b = a -> |v: vec3, norm: vec3|: vec3 vec3(0)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let b = ctx.globals.get("b").unwrap();
  let Value::Mesh(b) = b else {
    panic!("Expected result to be a Mesh");
  };
  assert_eq!(b.mesh.vertices.len(), 8);
  for vtx in b.mesh.vertices.values() {
    assert_eq!(vtx.position, Vec3::new(0., 0., 0.));
  }
}

#[test]
fn test_block_assignment_scoping() {
  let src = r#"
x = 0
{
  x = 1
}
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let x = dbg!(ctx.globals).get("x").unwrap();
  let x = x.as_int().expect("Expected result to be an Int");
  assert_eq!(x, 1);
}

#[test]
fn test_assign_to_arg() {
  let src = r#"
f = |x: int| {
  if x < 0 {
    x = 0
  }
  return x + 1
}

a = f(-1)
b = f(2)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let a = ctx.globals.get("a").unwrap();
  let a = a.as_int().expect("Expected result to be an Int");
  assert_eq!(a, 0 + 1);

  let b = ctx.globals.get("b").unwrap();
  let b = b.as_int().expect("Expected result to be an Int");
  assert_eq!(b, 2 + 1);
}

#[test]
fn test_multi_conditional_const_eval() {
  let src = r#"
f = |x: int| {
  y = 1
  if x == 0 {
    y = 0
  }
  if y == 1 {
    return true
  }
  return false
}
a = f(0)
b = f(1)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let a = ctx.globals.get("a").unwrap();
  let Value::Bool(a) = a else {
    panic!("Expected result to be a Bool");
  };
  assert!(!a, "Expected a to be false");

  let b = ctx.globals.get("b").unwrap();
  let Value::Bool(b) = b else {
    panic!("Expected result to be a Bool");
  };
  assert!(b, "Expected b to be true");
}

#[test]
fn test_mutate_non_const_global() {
  let src = r#"
x = randi(0,0)
sum = 0..2
  -> || {
    // this sets a closure-local variable `x` to 1 every time it's called since
    // variables from outer scopes can't be mutated from within closures.
    x = x + 1
    x
  }
  | reduce(add)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let sum = ctx.globals.get("sum").unwrap();
  let sum = sum.as_int().expect("Expected result to be an int");
  assert_eq!(sum, 2);
}

#[test]
fn test_scan() {
  let src = r#"
cumsum = scan(0, add)
a = 0..5 | cumsum
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let a = ctx.globals.get("a").unwrap();
  let Value::Sequence(a) = a else {
    panic!("Expected result to be a Seq");
  };
  let a = a.consume(&ctx).collect::<Result<Vec<_>, _>>().unwrap();
  assert_eq!(a.len(), 5);
  assert_eq!(a[0].as_int(), Some(0));
  assert_eq!(a[1].as_int(), Some(1));
  assert_eq!(a[2].as_int(), Some(3));
  assert_eq!(a[3].as_int(), Some(6));
  assert_eq!(a[4].as_int(), Some(10));
}

#[test]
fn test_scan_with_ix() {
  let src = r#"
cumsum = scan(0, |x, y, ix| x + y + ix)
a = 0..3 | cumsum
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let a = ctx.globals.get("a").unwrap();
  let Value::Sequence(a) = a else {
    panic!("Expected result to be a Seq");
  };
  let a = a.consume(&ctx).collect::<Result<Vec<_>, _>>().unwrap();
  assert_eq!(a.len(), 3);
  assert_eq!(a[0].as_int(), Some(0 + 0 + 0));
  assert_eq!(a[1].as_int(), Some(0 + 1 + 1));
  assert_eq!(a[2].as_int(), Some(2 + 2 + 2));
}

#[test]
fn test_map_with_ix() {
  let src = r#"
a = 0..5 | map(|x, ix| x + ix)
b = a | reduce(add)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let b = ctx.globals.get("b").unwrap();
  let b = b.as_int().expect("Expected result to be an Int");
  assert_eq!(b, 0 + 1 + 2 + 3 + 4 + 0 + 1 + 2 + 3 + 4);
}

#[test]
fn test_filter_with_ix() {
  let src = r#"
a = 0..10 | filter(|x, ix| x % 2 == 0 && ix < 7)
b = a | reduce(add)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let b = ctx.globals.get("b").unwrap();
  let b = b.as_int().expect("Expected result to be an Int");
  assert_eq!(b, 0 + 2 + 4 + 6);
}

#[test]
fn test_seq_indexing() {
  let src = r#"
a = 0..10 | collect
b = a[2]
c = ([1,2,3])[1]
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let b = ctx.globals.get("b").unwrap();
  let b = b.as_int().expect("Expected result to be an Int");
  assert_eq!(b, 2);

  let c = ctx.globals.get("c").unwrap();
  let c = c.as_int().expect("Expected result to be an Int");
  assert_eq!(c, 2);
}

#[test]
fn test_seq_as_eager() {
  let seq: Box<dyn Sequence + 'static> = Box::new(EagerSeq {
    inner: vec![Value::Int(1)],
  });
  let eager = seq_as_eager(&*seq).unwrap();
  let inner = &eager.inner;
  assert_eq!(inner.len(), 1);
  assert_eq!(inner[0].as_int(), Some(1));
}

#[test]
fn test_seq_out_of_bounds_indexing_error() {
  let src = r#"
a = [1,2,3]
b = a[100]
"#;

  let result = parse_and_eval_program(src);
  assert!(result.is_err(), "Expected out of bounds indexing error");
  if let Err(err) = result {
    assert!(
      err.to_string().contains("out of bounds"),
      "Unexpected error: {}",
      err
    );
  }
}

#[test]
fn test_flatten() {
  let src = r#"
a = [[1, 2], [3, 4], [5], 6, [7, 8]]
b = a | flatten
c = b | reduce(add)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let c = ctx.globals.get("c").unwrap();
  let c = c.as_int().expect("Expected result to be an Int");
  assert_eq!(c, 1 + 2 + 3 + 4 + 5 + 6 + 7 + 8);

  let b = ctx.globals.get("b").unwrap();
  let Value::Sequence(b) = b else {
    panic!("Expected result to be a Seq");
  };
  let b = b.consume(&ctx).collect::<Result<Vec<_>, _>>().unwrap();
  assert_eq!(b.len(), 8);
}
