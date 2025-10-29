#![feature(
  if_let_guard,
  impl_trait_in_bindings,
  adt_const_params,
  thread_local,
  impl_trait_in_fn_trait_return,
  unsafe_cell_access,
  likely_unlikely
)]

#[cfg(target_arch = "wasm32")]
use std::cell::UnsafeCell;
use std::{
  any::Any,
  cell::{Cell, RefCell},
  fmt::{Debug, Display},
  rc::{self, Rc},
};

use ast::{Expr, FunctionCallTarget, Statement};
use fxhash::{FxHashMap, FxHashSet};
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
  ast::{
    maybe_init_binop_def_shorthands, parse_statement, BinOp, ClosureArg, DestructurePattern,
    FunctionCall, MapLiteralEntry, TypeName,
  },
  builtins::{
    fn_defs::{get_builtin_fn_sig_entry_ix, ArgDef, DefaultValue, FnSignature, FN_SIGNATURE_DEFS},
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

pub const PRELUDE: &str = include_str!("prelude.geo");

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

#[derive(Clone)]
pub struct ErrorStack {
  pub errors: Vec<String>,
}

impl ErrorStack {
  #[cold]
  pub fn new(msg: impl Into<String>) -> Self {
    ErrorStack {
      errors: vec![msg.into()],
    }
  }

  #[cold]
  pub fn wrap(mut self, msg: impl Into<String>) -> Self {
    self.errors.push(msg.into());
    self
  }
}

impl Display for ErrorStack {
  #[cold]
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
  #[cold]
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{self}")
  }
}

pub trait Sequence: Any + Debug {
  fn consume<'a>(
    &self,
    ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a>;
}

pub(crate) fn seq_as_eager<'a>(seq: &'a dyn Sequence) -> Option<&'a EagerSeq> {
  let seq: &dyn Any = &*seq;
  seq.downcast_ref::<EagerSeq>()
}

#[derive(Clone)]
pub struct PartiallyAppliedFn {
  inner: Rc<Callable>,
  args: Vec<Value>,
  kwargs: FxHashMap<Sym, Value>,
}

impl Debug for PartiallyAppliedFn {
  #[cold]
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
enum CapturedScope {
  Strong(Rc<Scope>),
  Weak(rc::Weak<Scope>),
}

impl CapturedScope {
  fn upgrade(&self) -> Option<Rc<Scope>> {
    match self {
      CapturedScope::Strong(scope) => Some(Rc::clone(scope)),
      CapturedScope::Weak(weak) => weak.upgrade(),
    }
  }
}

#[derive(Clone)]
pub struct Closure {
  /// Names of parameters for this closure in order
  params: Vec<ClosureArg>,
  body: Vec<Statement>,
  /// Contains variables captured from the environment when the closure was created
  captured_scope: CapturedScope,
  return_type_hint: Option<TypeName>,
}

impl Debug for Closure {
  #[cold]
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
  #[cold]
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "<composed fn of {} inner callables>", self.inner.len())
  }
}

#[derive(Clone)]
pub struct PreResolvedSignature {
  arg_refs: Vec<ArgRef>,
  def_ix: usize,
}

pub enum Callable {
  Builtin {
    fn_entry_ix: usize,
    fn_impl:
      fn(usize, &[ArgRef], &[Value], &FxHashMap<Sym, Value>, &EvalCtx) -> Result<Value, ErrorStack>,
    /// This will be set in the case that a single signature can be resolved in advance
    pre_resolved_signature: Option<PreResolvedSignature>,
  },
  PartiallyAppliedFn(PartiallyAppliedFn),
  Closure(Closure),
  ComposedFn(ComposedFn),
}

impl Debug for Callable {
  #[cold]
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Callable::Builtin {
        fn_entry_ix,
        pre_resolved_signature,
        ..
      } => {
        let entry = match FN_SIGNATURE_DEFS.entries.get(*fn_entry_ix) {
          Some(entry) => entry,
          None => {
            return Debug::fmt(
              &format!("<built-in fn with invalid entry ix {fn_entry_ix}>"),
              f,
            );
          }
        };

        let name = entry.0;
        Debug::fmt(
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
        )
      }
      Callable::PartiallyAppliedFn(paf) => Debug::fmt(&format!("{paf:?}"), f),
      Callable::Closure(closure) => Debug::fmt(&format!("{closure:?}"), f),
      Callable::ComposedFn(composed) => Debug::fmt(&format!("{composed:?}"), f),
    }
  }
}

impl Callable {
  pub fn is_side_effectful(&self) -> bool {
    match self {
      Callable::Builtin { fn_entry_ix, .. } => {
        let name = FN_SIGNATURE_DEFS.entries[*fn_entry_ix].0;
        matches!(
          name,
          "print" | "render" | "call" | "randv" | "randf" | "randi" | "assert"
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
  #[cold]
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

#[repr(u8)]
pub enum Value {
  Nil,
  Int(i64),
  Float(f32),
  Vec2(Vec2),
  Vec3(Vec3),
  Bool(bool),
  Mesh(Rc<MeshHandle>),
  Callable(Rc<Callable>),
  Sequence(Rc<dyn Sequence>),
  Map(Rc<FxHashMap<String, Value>>),
  Material(Rc<Material>),
  Light(Box<Light>),
  String(String),
}

impl Value {
  fn discriminant(&self) -> u8 {
    unsafe { *<*const _>::from(self).cast::<u8>() }
  }
}

/// asserts invariants depended upon by fast-pathed `Value::clone` impl
#[test]
fn test_value_discriminant_order() {
  let copyable_vals = &[
    Value::Nil,
    Value::Int(0),
    Value::Float(0.),
    Value::Vec2(Vec2::new(0., 0.)),
    Value::Vec3(Vec3::new(0., 0., 0.)),
    Value::Bool(true),
  ];

  for (i, v) in copyable_vals.into_iter().enumerate() {
    assert_eq!(v.discriminant(), i as u8);
  }

  assert_eq!((Value::Bool(false)).discriminant(), 5);
}

#[cold]
#[inline(never)]
fn clone_value_slow(val: &Value) -> Value {
  match val {
    Value::Int(_)
    | Value::Float(_)
    | Value::Vec2(_)
    | Value::Vec3(_)
    | Value::Bool(_)
    | Value::Nil => unsafe { std::hint::unreachable_unchecked() },
    Value::Mesh(mesh) => Value::Mesh(Rc::clone(mesh)),
    Value::Light(light) => Value::Light(light.clone()),
    Value::Callable(callable) => Value::Callable(Rc::clone(&callable)),
    Value::Sequence(seq) => Value::Sequence(Rc::clone(seq)),
    Value::Map(map) => Value::Map(Rc::clone(map)),
    Value::String(s) => Value::String(s.clone()),
    Value::Material(material) => Value::Material(Rc::clone(material)),
  }
}

impl Clone for Value {
  fn clone(&self) -> Self {
    if std::hint::likely(self.discriminant() <= 5) {
      // fast path for copyable variants
      return unsafe { std::ptr::read(self as *const Value) };
    }

    clone_value_slow(self)
  }
}

impl Debug for Value {
  #[cold]
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

  fn as_sequence(&self) -> Option<Rc<dyn Sequence>> {
    match self {
      Value::Sequence(seq) => Some(Rc::clone(seq)),
      _ => None,
    }
  }

  fn as_map(&self) -> Option<&FxHashMap<String, Value>> {
    match self {
      Value::Map(map) => Some(map.as_ref()),
      _ => None,
    }
  }

  fn as_callable(&self) -> Option<&Rc<Callable>> {
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
          ctx.materials.keys().collect::<Vec<_>>()
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
        fn_entry_ix: std::usize::MAX,
        fn_impl: |_, _, _, _, _| panic!("example callable should never be called"),
        pre_resolved_signature: None,
      }))),
      ArgType::Sequence => Some(Value::Sequence(Rc::new(EagerSeq { inner: Vec::new() }))),
      ArgType::Map => Some(Value::Map(Rc::new(FxHashMap::default()))),
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
  Keyword(Sym),
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
  Keyword(Sym),
  Default(Value),
}

impl ArgRef {
  pub fn resolve<'a>(&'a self, args: &'a [Value], kwargs: &'a FxHashMap<Sym, Value>) -> &'a Value {
    match self {
      ArgRef::Positional(ix) => &args[*ix],
      ArgRef::Keyword(name) => kwargs.get(name).expect("Keyword argument not found"),
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

#[cold]
fn format_fn_signatures(arg_defs: &[FnSignature]) -> String {
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

#[cold]
pub(crate) fn build_no_fn_def_found_err(
  ctx: &EvalCtx,
  fn_name: &str,
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
  defs: &[FnSignature],
) -> ErrorStack {
  let desymbolicated_kwargs = ctx.desymbolicate_kwargs(kwargs);
  ErrorStack::new(format!(
    "No valid function signature found for `{fn_name}` with args: {args:?}, kwargs: \
     {desymbolicated_kwargs:?}\n\nAvailable signatures:\n{}",
    format_fn_signatures(defs)
  ))
}

const EMPTY_ARGS: Vec<Value> = Vec::new();
const EMPTY_KWARGS: FxHashMap<Sym, Value> =
  std::collections::HashMap::with_hasher(fxhash::FxBuildHasher::new());

fn get_args(
  ctx: &EvalCtx,
  fn_name: &str,
  defs: &[FnSignature],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
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

  for &key in kwargs.keys() {
    // TODO: again, bad that we have to resolve here for each kwarg
    if ctx
      .with_resolved_sym(key, |resolved_kwarg| {
        !defs
          .iter()
          .any(|def| def.arg_defs.iter().any(|arg| arg.name == resolved_kwarg))
      })
      .unwrap()
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
    // if a kwarg was passed which isn't defined in this function signature, skip
    for &kwarg_key in kwargs.keys() {
      if ctx
        .with_resolved_sym(kwarg_key, |resolved_kwarg_key| {
          def
            .arg_defs
            .iter()
            .all(|def| def.name != resolved_kwarg_key)
        })
        .unwrap()
      {
        continue 'def;
      }
    }

    let mut pos_arg_ix = 0;
    arg_refs.clear();
    'arg: for ArgDef {
      default_value,
      description: _,
      name,
      valid_types,
    } in def.arg_defs
    {
      // TODO: this is not ideal; would be good to pre-resolve these.
      let kwarg_sym = ctx.interned_symbols.intern(name);
      let (arg, arg_ref) = if let Some(kwarg) = kwargs.get(&kwarg_sym) {
        (kwarg, UnrealizedArgRef::Keyword(kwarg_sym))
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

  Err(build_no_fn_def_found_err(ctx, fn_name, args, kwargs, defs))
}

/// Specialized version of `get_args` for more efficient binary operator lookup.  Assumes that each
/// def in `defs` has exactly two args.
fn get_binop_def_ix(
  ctx: &EvalCtx,
  fn_entry_ix: usize,
  lhs: &Value,
  rhs: &Value,
) -> Result<usize, ErrorStack> {
  let fn_entry = &FN_SIGNATURE_DEFS.entries[fn_entry_ix];
  let defs = fn_entry.1.signatures;
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
    ctx,
    fn_entry.0,
    &[lhs.clone(), rhs.clone()],
    &EMPTY_KWARGS,
    fn_entry.1.signatures,
  ));
}

/// Specialized version of `get_args` for more efficient unary operator lookup.  Assumes that each
/// def in `defs` has exactly one arg.
fn get_unop_def_ix(
  ctx: &EvalCtx,
  fn_entry_ix: usize,
  defs: &[FnSignature],
  arg: &Value,
) -> Result<usize, ErrorStack> {
  for (def_ix, def) in defs.iter().enumerate() {
    let arg_def = &def.arg_defs[0];
    if ArgType::any_valid(&arg_def.valid_types, arg) {
      return Ok(def_ix);
    }
  }

  let fn_entry = &FN_SIGNATURE_DEFS.entries[fn_entry_ix];
  return Err(build_no_fn_def_found_err(
    ctx,
    fn_entry.0,
    &[arg.clone()],
    &EMPTY_KWARGS,
    fn_entry.1.signatures,
  ));
}

/// Specialized version of `get_args` for more efficient unary operator lookup.  Assumes that each
/// def in `defs` has exactly one arg.
fn get_unop_return_ty(
  ctx: &EvalCtx,
  name: &str,
  defs: &[FnSignature],
  arg: &Value,
) -> Result<&'static [ArgType], ErrorStack> {
  for def in defs {
    let arg_def = &def.arg_defs[0];
    if ArgType::any_valid(&arg_def.valid_types, arg) {
      return Ok(&def.return_type);
    }
  }

  return Err(build_no_fn_def_found_err(
    ctx,
    name,
    &[arg.clone()],
    &EMPTY_KWARGS,
    FN_SIGNATURE_DEFS[name].signatures,
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

// #[derive(Clone, Debug)]
// enum ScopeVars {
//   Flat {
//     ids: SmallVec<[Sym; 4]>,
//     vals: SmallVec<[Value; 4]>,
//   },
//   Map(FxHashMap<Sym, Value>),
// }

// impl Default for ScopeVars {
//   fn default() -> Self {
//     ScopeVars::Flat {
//       ids: SmallVec::new(),
//       vals: SmallVec::new(),
//     }
//   }
// }

// impl ScopeVars {
//   fn insert(&mut self, key: Sym, value: Value) {
//     match self {
//       ScopeVars::Flat { ids, vals } => {
//         if let Some(ix) = ids.iter().position(|k| *k == key) {
//           vals[ix] = value;
//           return;
//         }

//         if ids.len() < 4 {
//           ids.push(key);
//           vals.push(value);
//           return;
//         }

//         ::log::info!("spilling scope vars to map; {:?}; {:?}", ids, vals);
//         let mut map = FxHashMap::default();
//         for (k, v) in ids.iter().cloned().zip(vals.iter().cloned()) {
//           map.insert(k, v);
//         }
//         map.insert(key, value);
//         *self = ScopeVars::Map(map);
//       }
//       ScopeVars::Map(map) => {
//         map.insert(key, value);
//       }
//     }
//   }

//   fn get(&self, key: &Sym) -> Option<&Value> {
//     match self {
//       ScopeVars::Flat { ids, vals } => {
//         if let Some(ix) = ids.iter().position(|k| k == key) {
//           return Some(&vals[ix]);
//         }
//         None
//       }
//       ScopeVars::Map(map) => map.get(key),
//     }
//   }

//   fn get_mut(&mut self, key: &Sym) -> Option<&mut Value> {
//     match self {
//       ScopeVars::Flat { ids, vals } => {
//         if let Some(ix) = ids.iter().position(|k| k == key) {
//           return Some(&mut vals[ix]);
//         }
//         None
//       }
//       ScopeVars::Map(map) => map.get_mut(key),
//     }
//   }

//   fn contains_key(&self, key: &Sym) -> bool {
//     match self {
//       ScopeVars::Flat { ids, .. } => ids.iter().any(|k| *k == *key),
//       ScopeVars::Map(map) => map.contains_key(&key),
//     }
//   }

//   fn drain_with<F>(&mut self, mut f: F)
//   where
//     F: FnMut(Sym, Value),
//   {
//     match self {
//       ScopeVars::Flat { ids, vals } => {
//         for (k, v) in ids.drain(..).zip(vals.drain(..)) {
//           f(k, v);
//         }
//       }
//       ScopeVars::Map(map) => {
//         for (k, v) in map.drain() {
//           f(k, v);
//         }
//       }
//     }
//   }
// }

#[derive(Default, Debug)]
pub struct Scope {
  // vars: RefCell<ScopeVars>,
  vars: RefCell<FxHashMap<Sym, Value>>,
  parent: Option<Rc<Scope>>,
}

impl Clone for Scope {
  fn clone(&self) -> Self {
    Scope {
      vars: RefCell::new(self.vars.borrow().clone()),
      parent: self.parent.as_ref().map(Rc::clone),
    }
  }
}

fn get_default_globals() -> [(&'static str, Sym, Value); 1] {
  [("pi", Sym(0), Value::Float(std::f32::consts::PI))]
}

impl Scope {
  pub fn default_globals() -> Self {
    let scope = Scope::default();

    for (_name, sym, val) in get_default_globals() {
      scope.insert(sym, val);
    }

    scope
  }

  pub fn insert(&self, key: Sym, value: Value) {
    self.vars.borrow_mut().insert(key, value);
  }

  pub fn get(&self, key: Sym) -> Option<Value> {
    if let Some(val) = self.vars.borrow().get(&key).cloned() {
      return Some(val);
    }

    if let Some(parent) = &self.parent {
      return parent.get(key);
    }

    None
  }

  fn wrap(parent: &Rc<Scope>) -> Scope {
    Scope {
      vars: RefCell::new(Default::default()),
      parent: Some(Rc::clone(parent)),
    }
  }

  fn has(&self, key: Sym) -> bool {
    self.vars.borrow().contains_key(&key)
      || self.parent.as_ref().map(|p| p.has(key)).unwrap_or(false)
  }
}

/// Handle to an interned symbol.
///
/// This is done to speed up variable lookups by avoiding string comparisons.
#[derive(Clone, Debug, Copy, PartialEq, Eq, Hash)]
pub struct Sym(pub usize);

impl Display for Sym {
  #[cold]
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "Sym({})", self.0)
  }
}

pub struct SymbolInterner {
  pub symbols: RefCell<FxHashMap<String, Sym>>,
  pub reverse_symbols: RefCell<FxHashMap<Sym, String>>,
  pub next_sym: Cell<usize>,
}

impl SymbolInterner {
  pub fn intern(&self, name: &str) -> Sym {
    if let Some(sym) = self.symbols.borrow().get(name) {
      return *sym;
    }

    let sym = Sym(self.next_sym.get());
    self.next_sym.set(self.next_sym.get() + 1);
    self.symbols.borrow_mut().insert(name.to_owned(), sym);
    self
      .reverse_symbols
      .borrow_mut()
      .insert(sym, name.to_owned());
    sym
  }

  pub fn with_resolved<F, R>(&self, sym: Sym, f: F) -> Option<R>
  where
    F: FnOnce(&str) -> R,
  {
    self
      .reverse_symbols
      .borrow()
      .get(&sym)
      .map(|name| f(name.as_str()))
  }
}

impl Default for SymbolInterner {
  fn default() -> Self {
    let interner = SymbolInterner {
      symbols: RefCell::new(FxHashMap::default()),
      reverse_symbols: RefCell::new(FxHashMap::default()),
      next_sym: Cell::new(0),
    };

    for (name, _, _) in get_default_globals() {
      interner.intern(name);
    }
    interner
  }
}

pub struct EvalCtx {
  pub globals: Scope,
  pub interned_symbols: SymbolInterner,
  pub rendered_meshes: RenderedMeshes,
  pub rendered_lights: RenderedLights,
  pub rendered_paths: RenderedPaths,
  pub log_fn: fn(&str),
  #[cfg(target_arch = "wasm32")]
  rng: UnsafeCell<Pcg32>,
  pub materials: FxHashMap<String, Rc<Material>>,
  pub textures: FxHashSet<String>,
  pub default_material: RefCell<Option<Rc<Material>>>,
  pub sharp_angle_threshold_degrees: RefCell<f32>,
  scratch_args: RefCell<Vec<Vec<Value>>>,
  scratch_kwargs: RefCell<Vec<FxHashMap<Sym, Value>>>,
}

unsafe impl Send for EvalCtx {}
unsafe impl Sync for EvalCtx {}

impl Default for EvalCtx {
  fn default() -> Self {
    EvalCtx {
      globals: Scope::default_globals(),
      interned_symbols: SymbolInterner::default(),
      rendered_meshes: RenderedMeshes::default(),
      rendered_lights: RenderedLights::default(),
      rendered_paths: RenderedPaths::default(),
      log_fn: |msg| println!("{msg}"),
      #[cfg(target_arch = "wasm32")]
      rng: UnsafeCell::new(Pcg32::new(7718587666045340534, 17289744314186392832)),
      materials: FxHashMap::default(),
      textures: FxHashSet::default(),
      default_material: RefCell::new(None),
      sharp_angle_threshold_degrees: RefCell::new(45.8366),
      scratch_args: RefCell::new(Vec::with_capacity(8)),
      scratch_kwargs: RefCell::new(Vec::new()),
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

  pub fn get_args_scratch(&self) -> Vec<Value> {
    self.scratch_args.borrow_mut().pop().unwrap_or_default()
  }

  pub fn restore_args_scratch(&self, mut args: Vec<Value>) {
    args.clear();
    let mut borrowed = self.scratch_args.borrow_mut();
    if borrowed.len() < 64 {
      borrowed.push(args);
    }
  }

  pub fn get_kwargs_scratch(&self) -> FxHashMap<Sym, Value> {
    self
      .scratch_kwargs
      .borrow_mut()
      .pop()
      .unwrap_or_else(FxHashMap::default)
  }

  pub fn restore_kwargs_scratch(&self, mut kwargs: FxHashMap<Sym, Value>) {
    kwargs.clear();
    let mut borrowed = self.scratch_kwargs.borrow_mut();
    if borrowed.len() < 64 {
      borrowed.push(kwargs);
    }
  }

  fn eval_fn_call(
    &self,
    scope: &Scope,
    call: &FunctionCall,
  ) -> Result<ControlFlow<Value>, ErrorStack> {
    let mut args_opt = None;
    if !call.args.is_empty() {
      let mut args = self.get_args_scratch();
      for arg in &call.args {
        let val = match self.eval_expr(arg, scope, None)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        args.push(val);
      }
      args_opt = Some(args);
    }

    let mut kwargs_opt = None;
    if !call.kwargs.is_empty() {
      let mut kwargs = self.get_kwargs_scratch();
      for (k, v) in &call.kwargs {
        let val = match self.eval_expr(v, scope, None)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        kwargs.insert(*k, val);
      }
      kwargs_opt = Some(kwargs);
    }

    let do_call =
      |callable: &Rc<Callable>, args: Option<Vec<Value>>, kwargs: Option<FxHashMap<Sym, Value>>| {
        let ret = self
          .invoke_callable(
            callable,
            args.as_ref().unwrap_or(&EMPTY_ARGS),
            kwargs.as_ref().unwrap_or(&EMPTY_KWARGS),
          )
          .map_err(|err| err.wrap("Error invoking callable"))
          .map(ControlFlow::Continue);

        if let Some(args) = args {
          self.restore_args_scratch(args);
        }
        if let Some(kwargs) = kwargs {
          self.restore_kwargs_scratch(kwargs);
        }

        ret
      };

    match &call.target {
      FunctionCallTarget::Name(name) => {
        if let Some(global) = scope.get(*name) {
          let Value::Callable(callable) = global else {
            if let Some(args) = args_opt {
              self.restore_args_scratch(args);
            }
            if let Some(kwargs) = kwargs_opt {
              self.restore_kwargs_scratch(kwargs);
            }
            return Err(ErrorStack::new(format!(
              "\"{name}\" is not a callable; found: {global:?}"
            )));
          };

          do_call(&callable, args_opt, kwargs_opt)
        } else {
          return Err(ErrorStack::new(format!(
            "No variable found with name `{name}`"
          )));
        }
      }
      FunctionCallTarget::Literal(callable) => do_call(callable, args_opt, kwargs_opt),
    }
  }

  pub fn eval_expr(
    &self,
    expr: &Expr,
    scope: &Scope,
    binding_name: Option<Sym>,
  ) -> Result<ControlFlow<Value>, ErrorStack> {
    match expr {
      Expr::Call(call) => self.eval_fn_call(scope, call),
      Expr::BinOp {
        op,
        lhs,
        rhs,
        pre_resolved_def_ix,
      } => {
        let lhs = match self.eval_expr(lhs, scope, None)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };

        // special-case short-circuiting for boolean ops
        if matches!(op, BinOp::And | BinOp::Or) {
          let lhs_bool = match lhs.as_bool() {
            Some(b) => b,
            None => {
              return Err(ErrorStack::new(format!(
                "Left-hand side of `{op:?}` must be a boolean, found: {lhs:?}"
              )))
            }
          };

          match op {
            BinOp::And => {
              if !lhs_bool {
                return Ok(ControlFlow::Continue(Value::Bool(false)));
              }
            }
            BinOp::Or => {
              if lhs_bool {
                return Ok(ControlFlow::Continue(Value::Bool(true)));
              }
            }
            _ => unreachable!(),
          }
        }

        let rhs = match self.eval_expr(rhs, scope, None)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        op.apply(self, &lhs, &rhs, *pre_resolved_def_ix)
          .map(ControlFlow::Continue)
          .map_err(|err| err.wrap(format!("Error applying binary operator `{op:?}`")))
      }
      Expr::PrefixOp { op, expr } => {
        let val = match self.eval_expr(expr, scope, None)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        op.apply(self, &val)
          .map(ControlFlow::Continue)
          .map_err(|err| err.wrap(format!("Error applying prefix operator `{op:?}`")))
      }
      Expr::Range {
        start,
        end,
        inclusive,
      } => {
        let start = match self.eval_expr(start, scope, None)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        let Value::Int(start) = start else {
          return Err(ErrorStack::new(format!(
            "Range start must be an integer, found: {start:?}"
          )));
        };
        let end = match end {
          Some(end) => {
            let end = match self.eval_expr(end, scope, None)? {
              ControlFlow::Continue(val) => val,
              early_exit => return Ok(early_exit),
            };
            let Value::Int(mut end) = end else {
              return Err(ErrorStack::new(format!(
                "Range end must be an integer, found: {end:?}"
              )));
            };

            if *inclusive {
              end = end.saturating_add(1);
            }

            Some(end)
          }
          None => None,
        };

        Ok(ControlFlow::Continue(Value::Sequence(Rc::new(IntRange {
          start,
          end,
        }))))
      }
      Expr::Ident(name) => self.eval_ident(*name, scope).map(ControlFlow::Continue),
      Expr::Literal(val) => Ok(ControlFlow::Continue(val.clone())),
      Expr::ArrayLiteral(elems) => {
        let mut evaluated = Vec::with_capacity(elems.len());
        for elem in elems {
          let val = match self.eval_expr(elem, scope, None)? {
            ControlFlow::Continue(val) => val,
            early_exit => return Ok(early_exit),
          };
          evaluated.push(val);
        }
        Ok(ControlFlow::Continue(Value::Sequence(Rc::new(EagerSeq {
          inner: evaluated,
        }))))
      }
      Expr::MapLiteral { entries } => {
        let mut evaluated = FxHashMap::default();
        for entry in entries {
          match entry {
            MapLiteralEntry::KeyValue { key, value } => {
              let val = match self.eval_expr(value, scope, None)? {
                ControlFlow::Continue(val) => val,
                early_exit => return Ok(early_exit),
              };
              evaluated.insert(key.clone(), val);
            }
            MapLiteralEntry::Splat { expr: splat } => {
              let splat = match self.eval_expr(splat, scope, None)? {
                ControlFlow::Continue(val) => val,
                early_exit => return Ok(early_exit),
              };
              let Value::Map(splat) = splat else {
                return Err(ErrorStack::new(format!(
                  "Tried to splat value of type {:?} into map; expected a map.",
                  splat.get_type()
                )));
              };
              for (key, val) in &*splat {
                evaluated.insert(key.clone(), val.clone());
              }
            }
          }
        }
        Ok(ControlFlow::Continue(Value::Map(Rc::new(evaluated))))
      }
      Expr::Closure {
        params,
        body,
        return_type_hint,
      } => {
        // cloning the scope here makes the closure function like a rust `move` closure
        // where all the values are cloned before being moved into the closure.
        let captured_scope = Rc::new(scope.clone());

        if let Some(binding_name) = binding_name {
          // add the closure itself to the scope to support recursive calls
          captured_scope.insert(
            binding_name,
            Value::Callable(Rc::new(Callable::Closure(Closure {
              params: params.clone(),
              body: body.0.clone(),
              // this is going to be immediately overwritten
              captured_scope: CapturedScope::Strong(Rc::new(Scope::default())),
              return_type_hint: return_type_hint.clone(),
            }))),
          );

          let captured_scope_clone = Rc::downgrade(&captured_scope);
          match captured_scope
            .vars
            .borrow_mut()
            .get_mut(&binding_name)
            .unwrap()
          {
            Value::Callable(callable) => {
              let callable =
                Rc::get_mut(callable).expect("Should only be one reference to this Rc");
              let Callable::Closure(closure) = callable else {
                unreachable!();
              };
              // writing this as a weak reference prevents a reference cycle:
              //
              // closure -> captured_scope -> closure_clone
              //                   ^---------------------┘
              //                           ^ this one is weak
              closure.captured_scope = CapturedScope::Weak(captured_scope_clone);
            }
            _ => unreachable!(),
          };
        }

        Ok(ControlFlow::Continue(Value::Callable(Rc::new(
          Callable::Closure(Closure {
            params: params.clone(),
            body: body.0.clone(),
            captured_scope: CapturedScope::Strong(captured_scope),
            return_type_hint: return_type_hint.clone(),
          }),
        ))))
      }
      Expr::StaticFieldAccess { lhs: obj, field } => {
        let lhs = match self.eval_expr(obj, scope, None)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        self
          .eval_static_field_access(&lhs, field)
          .map(ControlFlow::Continue)
      }
      Expr::FieldAccess { lhs, field } => {
        let lhs = match self.eval_expr(lhs, scope, None)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        let field = match self.eval_expr(field, scope, None)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        self
          .eval_field_access(&lhs, &field)
          .map(ControlFlow::Continue)
      }
      Expr::Conditional {
        cond,
        then,
        else_if_exprs,
        else_expr,
      } => {
        let cond = match self.eval_expr(cond, scope, None)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        let Value::Bool(cond) = cond else {
          return Err(ErrorStack::new(format!(
            "Condition passed to if statement must be a boolean; found: {cond:?}"
          )));
        };
        if cond {
          return self.eval_expr(then, scope, None);
        }
        for (else_if_cond, else_if_body) in else_if_exprs {
          let else_if_cond = match self.eval_expr(else_if_cond, scope, None)? {
            ControlFlow::Continue(val) => val,
            early_exit => return Ok(early_exit),
          };
          let Value::Bool(else_if_cond) = else_if_cond else {
            return Err(ErrorStack::new(format!(
              "Condition passed to else-if statement must be a boolean; found: {else_if_cond:?}"
            )));
          };
          if else_if_cond {
            return self.eval_expr(else_if_body, scope, None);
          }
        }
        if let Some(else_expr) = else_expr {
          return self.eval_expr(else_expr, scope, None);
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
              for (key, val) in block_scope.vars.into_inner() {
                let exists = scope.has(key);
                if exists {
                  scope.insert(key, val.clone());
                }
              }

              return Ok(ControlFlow::Return(val));
            }
          };
          if let Statement::Assignment { .. } = statement {
            last_value = Value::Nil;
          }
        }

        for (key, val) in block_scope.vars.into_inner() {
          let exists = scope.has(key);
          if exists {
            scope.insert(key, val.clone());
          }
        }

        Ok(ControlFlow::Continue(last_value))
      }
    }
  }

  fn eval_assignment(
    &self,
    ident: Sym,
    value: Value,
    scope: &Scope,
    type_hint: Option<TypeName>,
  ) -> Result<Value, ErrorStack> {
    if let Some(type_hint) = type_hint {
      type_hint.validate_val(&value)?;
    }

    scope.insert(ident, value);
    Ok(Value::Nil)
  }

  fn eval_statement(
    &self,
    statement: &Statement,
    scope: &Scope,
  ) -> Result<ControlFlow<Value>, ErrorStack> {
    match statement {
      Statement::Expr(expr) => self.eval_expr(expr, scope, None),
      Statement::Assignment {
        name,
        expr,
        type_hint,
      } => {
        let val = match self.eval_expr(expr, scope, Some(*name))? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        self
          .eval_assignment(*name, val, scope, *type_hint)
          .map(ControlFlow::Continue)
      }
      Statement::DestructureAssignment { lhs, rhs } => {
        let rhs = match self.eval_expr(rhs, scope, None)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        lhs
          .visit_assignments(self, rhs, &mut |lhs, rhs| {
            self.eval_assignment(lhs, rhs, scope, None)?;
            Ok(())
          })
          .map_err(|err| err.wrap("Error evaluating destructure assignment"))?;
        Ok(ControlFlow::Continue(Value::Nil))
      }
      Statement::Return { value } => {
        let value = if let Some(value) = value {
          match self.eval_expr(value, scope, None)? {
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
          match self.eval_expr(value, scope, None)? {
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
    callable: &Rc<Callable>,
    seq: Rc<dyn Sequence>,
  ) -> Result<Value, ErrorStack> {
    // if we're applying a mesh boolean op here, we can use the fast path that avoids the overhead
    // of encoding/decoding intermediate meshes
    if let Callable::Builtin { fn_entry_ix, .. } = &**callable {
      let builtin_name = FN_SIGNATURE_DEFS.entries[*fn_entry_ix].0;
      if matches!(builtin_name, "union" | "difference" | "intersect") {
        let combined_iter = ChainSeq::new(
          self,
          Rc::new(EagerSeq {
            inner: vec![initial_val, Value::Sequence(seq)],
          }),
        )
        .map_err(|err| {
          err.wrap("Internal error creating chained sequence when folding mesh boolean op")
        })?;
        return eval_mesh_boolean(
          1,
          &[ArgRef::Positional(0), ArgRef::Positional(1)],
          &[Value::Sequence(Rc::new(combined_iter))],
          &EMPTY_KWARGS,
          self,
          MeshBooleanOp::from_str(builtin_name),
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
        .invoke_callable(callable, &[acc, value], &EMPTY_KWARGS)
        .map_err(|err| err.wrap("Error invoking callable in reduce"))?;
    }

    Ok(acc)
  }

  fn reduce<'a>(
    &'a self,
    fn_value: &Rc<Callable>,
    seq: Rc<dyn Sequence>,
  ) -> Result<Value, ErrorStack> {
    // if we're applying a mesh boolean op here, we can use the fast path that avoids the overhead
    // of encoding/decoding intermediate meshes
    if let Callable::Builtin { fn_entry_ix, .. } = &**fn_value {
      let builtin_name = FN_SIGNATURE_DEFS.entries[*fn_entry_ix].0;
      if matches!(builtin_name, "union" | "difference" | "intersect") {
        return eval_mesh_boolean(
          1,
          &[ArgRef::Positional(0), ArgRef::Positional(1)],
          &[Value::Sequence(seq)],
          &EMPTY_KWARGS,
          self,
          MeshBooleanOp::from_str(builtin_name),
        )
        .map_err(|err| err.wrap("Error invoking mesh boolean op in `reduce`"));
      }
    }

    let mut iter = seq.consume(self);
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
        .invoke_callable(fn_value, &[acc, value], &EMPTY_KWARGS)
        .map_err(|err| err.wrap("Error invoking callable in reduce"))?;
    }
    Ok(acc)
  }

  pub(crate) fn eval_ident(&self, name: Sym, scope: &Scope) -> Result<Value, ErrorStack> {
    if let Some(local) = scope.get(name) {
      return Ok(local);
    }

    // look it up as a builtin fn
    let resolved = self
      .with_resolved_sym(name, |resolved_name| {
        get_builtin_fn_sig_entry_ix(resolved_name)
          .map(|ix| (ix, resolve_builtin_impl(resolved_name)))
      })
      .unwrap();
    if let Some((fn_entry_ix, fn_impl)) = resolved {
      return Ok(Value::Callable(Rc::new(Callable::Builtin {
        fn_entry_ix,
        fn_impl,
        pre_resolved_signature: None,
      })));
    }

    Err(ErrorStack::new(format!("Variable `{name}` not defined",)))
  }

  fn invoke_closure(
    &self,
    closure: &Closure,
    args: &[Value],
    kwargs: &FxHashMap<Sym, Value>,
  ) -> Result<Value, ErrorStack> {
    // TODO: should do some basic analysis to see which variables are actually needed and avoid
    // cloning the rest

    // TODO: should re-use hashmaps here to avoid allocations on each call
    let closure_scope = Scope::wrap(&closure.captured_scope.upgrade().unwrap());
    let mut pos_arg_ix = 0usize;
    let mut any_args_valid = false;
    let mut invalid_arg_ix = None;
    for arg in &closure.params {
      match &arg.ident {
        // there's no way currently to assign a name to destructured args, so they can only be used
        // positionally
        DestructurePattern::Ident(name) => {
          if let Some(kwarg) = kwargs.get(name) {
            if let Some(type_hint) = arg.type_hint {
              type_hint
                .validate_val(kwarg)
                .map_err(|err| err.wrap(format!("Type error for closure kwarg `{name}`")))?;
            }
            closure_scope.insert(*name, kwarg.clone());
            any_args_valid = true;
            continue;
          }
        }
        DestructurePattern::Array(_) => (),
        DestructurePattern::Map(_) => (),
      }

      if pos_arg_ix < args.len() {
        let pos_arg = &args[pos_arg_ix];
        if let Some(type_hint) = arg.type_hint {
          type_hint
            .validate_val(pos_arg)
            .map_err(|err| err.wrap(format!("Type error for closure pos arg `{:?}`", arg.ident)))?;
        }
        arg
          .ident
          .visit_assignments(self, pos_arg.clone(), &mut |k, v| {
            closure_scope.insert(k, v);
            Ok(())
          })?;
        any_args_valid = true;
        pos_arg_ix += 1;
      } else {
        if let Some(default_val) = &arg.default_val {
          let default_val = self.eval_expr(default_val, &closure_scope, None)?;
          let default_val = match default_val {
            ControlFlow::Continue(val) => val,
            ControlFlow::Return(_) => {
              return Err(ErrorStack::new(format!(
                "`return` isn't valid in arg default value expressions; found in default value \
                 for arg `{:?}`",
                arg.ident
              )))
            }
            ControlFlow::Break(_) => {
              return Err(ErrorStack::new(format!(
                "`break` isn't valid in arg default value expressions; found in default value for \
                 arg `{:?}`",
                arg.ident
              )));
            }
          };

          arg
            .ident
            .visit_assignments(self, default_val, &mut |k, v| {
              closure_scope.insert(k, v);
              Ok(())
            })?;
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
            inner: Rc::new(Callable::Closure(closure.clone())),
            args: args.to_owned(),
            kwargs: kwargs.clone(),
          },
        ))));
      } else {
        return Err(ErrorStack::new(format!(
          "Missing required argument `{:?}` for closure",
          invalid_arg.ident
        )));
      }
    }

    let mut out: Value = Value::Nil;
    for stmt in &closure.body {
      match stmt {
        Statement::Expr(expr) => match self.eval_expr(expr, &closure_scope, None)? {
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
          let val = match self.eval_expr(expr, &closure_scope, Some(*name))? {
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
          };
          self.eval_assignment(*name, val, &closure_scope, *type_hint)?;
        }
        Statement::DestructureAssignment { lhs, rhs } => {
          let rhs = match self.eval_expr(rhs, &closure_scope, None)? {
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
          };
          lhs
            .visit_assignments(self, rhs, &mut |lhs, rhs| {
              self.eval_assignment(lhs, rhs, &closure_scope, None)?;
              Ok(())
            })
            .map_err(|err| err.wrap("Error evaluating destructure assignment"))?;
        }
        Statement::Return { value } => {
          out = if let Some(value) = value {
            match self.eval_expr(value, &closure_scope, None)? {
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

  fn invoke_callable(
    &self,
    callable: &Rc<Callable>,
    args: &[Value],
    kwargs: &FxHashMap<Sym, Value>,
  ) -> Result<Value, ErrorStack> {
    match &**callable {
      Callable::Builtin {
        fn_entry_ix,
        fn_impl,
        pre_resolved_signature,
      } => match pre_resolved_signature {
        Some(PreResolvedSignature { arg_refs, def_ix }) => {
          fn_impl(*def_ix, arg_refs, args, kwargs, self)
        }
        None => {
          let entry = &FN_SIGNATURE_DEFS.entries[*fn_entry_ix];
          let builtin_name = entry.0;
          let fn_signature_defs = entry.1.signatures;
          let arg_refs = get_args(self, builtin_name, fn_signature_defs, args, kwargs)?;
          let (def_ix, arg_refs) = match arg_refs {
            GetArgsOutput::Valid { def_ix, arg_refs } => (def_ix, arg_refs),
            GetArgsOutput::PartiallyApplied => {
              return Ok(Value::Callable(Rc::new(Callable::PartiallyAppliedFn(
                PartiallyAppliedFn {
                  inner: Rc::clone(callable),
                  args: args.to_owned(),
                  kwargs: kwargs.clone(),
                },
              ))))
            }
          };
          fn_impl(def_ix, &arg_refs, args, kwargs, self)
        }
      }
      .map_err(|err| {
        err.wrap(format!(
          "Error invoking builtin function `{}`",
          FN_SIGNATURE_DEFS.entries[*fn_entry_ix].0
        ))
      }),
      Callable::PartiallyAppliedFn(paf) => {
        let mut combined_args = paf.args.clone();
        combined_args.extend(args.iter().cloned());

        if kwargs.is_empty() {
          return self.invoke_callable(&paf.inner, &combined_args, &paf.kwargs);
        }

        let mut combined_kwargs = paf.kwargs.clone();
        for (key, value) in kwargs {
          combined_kwargs.insert(key.clone(), value.clone());
        }

        self.invoke_callable(&paf.inner, &combined_args, &combined_kwargs)
      }
      Callable::Closure(closure) => self.invoke_closure(closure, args, kwargs),
      Callable::ComposedFn(ComposedFn { inner }) => {
        let acc = args;
        let mut iter = inner.iter();
        let mut acc = self.invoke_callable(iter.next().unwrap(), acc, &EMPTY_KWARGS)?;
        for callable in iter {
          acc = self.invoke_callable(callable, &[acc], &EMPTY_KWARGS)?;
        }

        Ok(acc)
      }
    }
  }

  fn eval_static_field_access(&self, lhs: &Value, field: &str) -> Result<Value, ErrorStack> {
    match lhs {
      Value::Vec2(v2) => {
        let swiz = |c| match c {
          'x' | 'r' => Ok(v2.x),
          'y' | 'g' => Ok(v2.y),
          _ => Err(ErrorStack::new(format!("Unknown field `{c}` for Vec2"))),
        };

        match field.chars().count() {
          1 => swiz(field.chars().next().unwrap()).map(Value::Float),
          2 => {
            let mut chars = field.chars();
            let a = chars.next().unwrap();
            let b = chars.next().unwrap();

            Ok(Value::Vec2(Vec2::new(swiz(a)?, swiz(b)?)))
          }
          _ => Err(ErrorStack::new(format!(
            "invalid swizzle; expected 1 or 2 chars"
          ))),
        }
      }
      Value::Vec3(v3) => {
        let swiz = |c| match c {
          'x' | 'r' => Ok(v3.x),
          'y' | 'g' => Ok(v3.y),
          'z' | 'b' => Ok(v3.z),
          _ => Err(ErrorStack::new(format!("Unknown field `{c}` for Vec3"))),
        };
        match field.chars().count() {
          1 => swiz(field.chars().next().unwrap()).map(Value::Float),
          2 => {
            let mut chars = field.chars();
            let a = chars.next().unwrap();
            let b = chars.next().unwrap();

            Ok(Value::Vec2(Vec2::new(swiz(a)?, swiz(b)?)))
          }
          3 => {
            let mut chars = field.chars();
            let a = chars.next().unwrap();
            let b = chars.next().unwrap();
            let c = chars.next().unwrap();

            Ok(Value::Vec3(Vec3::new(swiz(a)?, swiz(b)?, swiz(c)?)))
          }
          _ => Err(ErrorStack::new(format!(
            "invalid swizzle; expected 1 to 3 chars"
          ))),
        }
      }
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

  fn eval_field_access(&self, lhs: &Value, field: &Value) -> Result<Value, ErrorStack> {
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
          if *i < 0 {
            return Err(ErrorStack::new(format!(
              "negative index {i} not supported for sequences"
            )));
          }

          let Some(eager_seq) = seq_as_eager(&**seq) else {
            return Err(ErrorStack::new(
              "sequence is not eager and must be realized with `collect` before indexing",
            ));
          };

          eager_seq.inner.get(*i as usize).cloned().ok_or_else(|| {
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

  pub fn with_resolved_sym<F, R>(&self, sym: Sym, f: F) -> Option<R>
  where
    F: FnOnce(&str) -> R,
  {
    self.interned_symbols.with_resolved(sym, f)
  }

  #[cold]
  fn desymbolicate_kwargs(&self, kwargs: &FxHashMap<Sym, Value>) -> FxHashMap<String, Value> {
    kwargs
      .iter()
      .map(|(k, v)| {
        (
          self.with_resolved_sym(*k, |s| s.to_owned()).unwrap(),
          v.clone(),
        )
      })
      .collect()
  }

  #[cfg(test)]
  fn get_global(&self, arg: &str) -> Option<Value> {
    let sym = self.interned_symbols.intern(arg);
    self.globals.get(sym)
  }
}

pub(crate) fn parse_program_src<'a>(ctx: &EvalCtx, src: &'a str) -> Result<Program, ErrorStack> {
  maybe_init_binop_def_shorthands();

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
    .filter_map(|stmt| match parse_statement(ctx, stmt) {
      Ok(Some(statement)) => Some(Ok(statement)),
      Ok(None) => None,
      Err(err) => Some(Err(err.wrap("Error parsing statement"))),
    })
    .collect::<Result<Vec<_>, ErrorStack>>()?;

  Ok(Program { statements })
}

pub fn parse_program_maybe_with_prelude(
  ctx: &EvalCtx,
  src: String,
  include_prelude: bool,
) -> Result<Program, ErrorStack> {
  let src = if include_prelude {
    format!("{}\n{src}", PRELUDE)
  } else {
    src
  };
  parse_program_src(ctx, &src)
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
  let mut ast = parse_program_maybe_with_prelude(ctx, src, include_prelude)
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
  let defs = &[FnSignature {
    arg_defs: ARGS,
    description: "",
    return_type: &[ArgType::Any],
  }];
  let args = Vec::new();
  let mut kwargs = FxHashMap::default();
  let ctx = EvalCtx::default();
  let y_interned = ctx.interned_symbols.intern("y");
  kwargs.insert(y_interned, Value::Int(1));
  let result = get_args(&ctx, "fn_name", defs, &args, &kwargs);
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
  let defs = &[FnSignature {
    arg_defs: ARGS,
    description: "",
    return_type: &[ArgType::Any],
  }];

  let ctx = EvalCtx::default();
  let z_interned = ctx.interned_symbols.intern("z");
  let args = Vec::new();
  let mut kwargs = FxHashMap::default();
  kwargs.insert(z_interned, Value::Int(1));
  let result = get_args(&ctx, "fn_name", defs, &args, &kwargs);
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
  let result = &ctx.get_global("result").unwrap();
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

  let a = &ctx.get_global("a").unwrap();
  let a = a.as_int().expect("Expected result to be an Int");
  assert_eq!(a, 7);

  let b = &ctx.get_global("b").unwrap();
  let Value::Float(b) = b else {
    panic!("Expected result to be a Float");
  };
  assert_eq!(*b, 4.);

  let c = &ctx.get_global("c").unwrap();
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

  let a = &ctx.get_global("a").unwrap();
  let a = a.as_int().expect("Expected result to be an Int");
  assert_eq!(a, 10); // 0 + 1 + 2 + 3 + 4

  let b = &ctx.get_global("b").unwrap();
  let b = b.as_int().expect("Expected result to be an Int");
  assert_eq!(b, 10);

  let c = &ctx.get_global("c").unwrap();
  let c = c.as_int().expect("Expected result to be an Int");
  assert_eq!(c, 10);

  let d = &ctx.get_global("d").unwrap();
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

  let a = &ctx.get_global("a").unwrap();
  let Value::Vec3(a) = a else {
    panic!("Expected result to be a Vec3");
  };
  assert_eq!(*a, Vec3::new(0.5, 0.5, 0.5));

  let b = &ctx.get_global("b").unwrap();
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

  let a = &ctx.get_global("a").unwrap();
  let Value::Bool(a) = a else {
    panic!("Expected result to be a Bool");
  };
  assert!(*a);

  let b = &ctx.get_global("b").unwrap();
  let Value::Bool(b) = b else {
    panic!("Expected result to be a Bool");
  };
  assert!(!*b);

  let c = &ctx.get_global("c").unwrap();
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

  let a = &ctx.get_global("a").unwrap();
  let Value::Float(a) = a else {
    panic!("Expected result to be a Float");
  };
  assert_eq!(*a, 1.0);

  let b = &ctx.get_global("b").unwrap();
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

  let seq = &ctx.get_global("seq").unwrap();
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

  let result = &ctx.get_global("result").unwrap();
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
        let pt = vtx.position.push(1.);
        let transformed = mesh.transform * pt;
        let transformed = transformed.xyz();
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

  let result = &ctx.get_global("result").unwrap();
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

  let result = &ctx.get_global("res").unwrap();
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

  let a = &ctx.get_global("a").unwrap();
  let a = a
    .as_int()
    .unwrap_or_else(|| panic!("Expected result to be an Int; found: {a:?}"));
  assert_eq!(a, 0 * 2 + 1 * 2 + 2 * 2 + 3 * 2 + 4 * 2);

  let b = &ctx.get_global("b").unwrap();
  let b = b
    .as_int()
    .unwrap_or_else(|| panic!("Expected result to be an Int; found: {b:?}"));
  assert_eq!(a, b);

  let c = &ctx.get_global("c").unwrap();
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
  let result_good = ctx_good.get_global("result").unwrap();
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
  let b = ctx.get_global("b").unwrap();
  let Value::Vec3(b) = b else {
    panic!("Expected result to be a Vec3");
  };
  assert_eq!(b, Vec3::new(3.0, 2.0, 1.0));

  let c = ctx.get_global("c").unwrap();
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

  let a = ctx.get_global("a").unwrap();
  let Value::Bool(a) = a else {
    panic!("Expected result to be a Bool");
  };
  assert!(a);

  let b = ctx.get_global("b").unwrap();
  let Value::Bool(b) = b else {
    panic!("Expected result to be a Bool");
  };
  assert!(!b);

  let c = ctx.get_global("c").unwrap();
  let Value::Bool(c) = c else {
    panic!("Expected result to be a Bool");
  };
  assert!(!c);

  let d = ctx.get_global("d").unwrap();
  let Value::Bool(d) = d else {
    panic!("Expected result to be a Bool");
  };
  assert!(d);

  let e = ctx.get_global("e").unwrap();
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

  let out = ctx.get_global("out").unwrap();
  let out = out.as_int().expect("Expected result to be an Int");
  assert_eq!(out, 1 + 2 + 3);
}

#[test]
fn test_negative_ints() {
  let src = r#"
a = -1
"#;

  let ctx = parse_and_eval_program(src).unwrap();
  let a = ctx.get_global("a").unwrap();
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

  let out = ctx.get_global("out").unwrap();
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

  let out = ctx.get_global("out").unwrap();
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

  let out = ctx.get_global("out").unwrap();
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

  let result = ctx.get_global("result").unwrap();
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
    ctx.get_global("a").unwrap(),
    ctx.get_global("b").unwrap(),
    ctx.get_global("c").unwrap(),
    ctx.get_global("d").unwrap(),
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

  let out = ctx.get_global("out").unwrap();
  let out = out.as_int().expect("Expected result to be an Int");
  assert_eq!(out, 3);
}

#[test]
fn test_skip_while() {
  let src = r#"
out = [1,2,3,4,5] | skip_while(|x| x < 3) | reduce(add)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let out = ctx.get_global("out").unwrap();
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

  let out = ctx.get_global("out").unwrap();
  let out = out.as_int().expect("Expected result to be an Int");
  assert_eq!(out, 0 + 1 + 2 + 3 + 4 + 5 + 1 + 2);

  let first = ctx.get_global("first").unwrap();
  let first = first.as_int().expect("Expected result to be an Int");
  assert_eq!(first, 0, "Expected first element to be 0, found: {first}");
}

#[test]
fn test_map_literal_parsing() {
  let src = r#"
a = { x: 1, "y+": 1+4, z: 3 }
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let a = ctx.get_global("a").unwrap();
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

  let x = ctx.get_global("x").unwrap();
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

  let x = ctx.get_global("x").unwrap();
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

  let a = ctx.get_global("a").unwrap();
  let a = a.as_int().expect("Expected result to be an Int");
  assert_eq!(a, 0x1234);

  let b = ctx.get_global("b").unwrap();
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

  let out1 = ctx.get_global("out1").unwrap();
  let out1 = out1.as_int().expect("Expected result to be an Int");
  assert_eq!(out1, 124);

  let out2 = ctx.get_global("out2").unwrap();
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

  let b = ctx.get_global("b").unwrap();
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

  let x = ctx.get_global("x").unwrap();
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

  let a = ctx.get_global("a").unwrap();
  let a = a.as_int().expect("Expected result to be an Int");
  assert_eq!(a, 0 + 1);

  let b = ctx.get_global("b").unwrap();
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

  let a = ctx.get_global("a").unwrap();
  let Value::Bool(a) = a else {
    panic!("Expected result to be a Bool");
  };
  assert!(!a, "Expected a to be false");

  let b = ctx.get_global("b").unwrap();
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

  let sum = ctx.get_global("sum").unwrap();
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

  let a = ctx.get_global("a").unwrap();
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

  let a = ctx.get_global("a").unwrap();
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

  let b = ctx.get_global("b").unwrap();
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

  let b = ctx.get_global("b").unwrap();
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

  let b = ctx.get_global("b").unwrap();
  let b = b.as_int().expect("Expected result to be an Int");
  assert_eq!(b, 2);

  let c = ctx.get_global("c").unwrap();
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

  let c = ctx.get_global("c").unwrap();
  let c = c.as_int().expect("Expected result to be an Int");
  assert_eq!(c, 1 + 2 + 3 + 4 + 5 + 6 + 7 + 8);

  let b = ctx.get_global("b").unwrap();
  let Value::Sequence(b) = b else {
    panic!("Expected result to be a Seq");
  };
  let b = b.consume(&ctx).collect::<Result<Vec<_>, _>>().unwrap();
  assert_eq!(b.len(), 8);
}

#[test]
fn test_shadow_global_in_closure_repro() {
  let src = r#"
resolution = 2
radius = 10

contours = 1..=2 -> |y_ix| {
  radius = radius + sin(y_ix * 1.2) * 1000
  radius
}

a = contours | first
b = contours | skip(1) | first
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let a = ctx.get_global("a").unwrap();
  let a = a.as_float().expect("Expected result to be a Float");
  assert!(a != 10., "Expected a to not be 10, found: {}", a);

  let b = ctx.get_global("b").unwrap();
  let b = b.as_float().expect("Expected result to be a Float");
  assert!(b != 10., "Expected b to not be 10, found: {}", b);

  let seq = match ctx.get_global("contours") {
    Some(Value::Sequence(seq)) => seq,
    other => panic!("Expected contours to be a seq, found: {other:?}"),
  };
  let seq: &dyn Any = &*seq;
  let MapSeq { inner: _, cb }: &MapSeq = seq.downcast_ref::<MapSeq>().unwrap();
  let closure = match &**cb {
    Callable::Closure(closure) => closure,
    _ => unreachable!(),
  };
  let body = &closure.body;
  let assignment_rhs = match body.first().unwrap() {
    Statement::Assignment { expr, .. } => expr,
    _ => unreachable!(),
  };
  // the const radius should be inlined
  let lhs = match assignment_rhs {
    Expr::BinOp { lhs, .. } => &**lhs,
    _ => unreachable!(),
  };
  assert!(matches!(lhs, Expr::Literal(Value::Int(10))));

  // the `radius` returned at the end should not be inlined since its value depends on the closure
  // argument
  match body.last().unwrap() {
    Statement::Expr(expr) => match expr {
      Expr::Ident(ident) => {
        assert_eq!(*ident, ctx.interned_symbols.intern("radius"));
      }
      _ => panic!("Expected last expression to be an ident, found: {expr:?}"),
    },
    _ => unreachable!(),
  }
}

#[test]
fn test_shadow_global_in_closure_repro_2() {
  let src = r#"
radius = 10

contours = 1..2 -> |y_ix| {
  1..2 -> |i| {
    radius = radius + y_ix * 1
    radius
  }
}

x = contours | first | first
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let x = ctx.get_global("x").unwrap();
  let x = x.as_float().expect("Expected result to be a Float");
  assert_eq!(x, 10. + 1. * 1.);
}

#[test]
fn test_assert_non_const() {
  let src = r#"
fn = |cond: bool| {
  if cond {
    assert(false)
  }
}

fn(false)
"#;
  parse_and_eval_program(src).unwrap();
}

#[test]
fn test_string_join() {
  let src = r#"
x = ["a", "b", "c"] | join(", ")
y = ["1", "2"] | join("")
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let x = ctx.get_global("x").unwrap();
  let x = x.as_str().expect("Expected result to be a String");
  assert_eq!(x, "a, b, c");

  let y = ctx.get_global("y").unwrap();
  let y = y.as_str().expect("Expected result to be a String");
  assert_eq!(y, "12");
}

#[test]
fn test_recursive_call() {
  let src = r#"
fn = |x: int| {
  if x <= 0 {
    return 1
  } else {
    fn(x - 1) + 1
  }
}

x = fn(5)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let x = ctx.get_global("x").unwrap();
  let x = x.as_int().expect("Expected result to be an Int");
  assert_eq!(x, 6);
}

#[test]
fn test_closure_scope_reference_counting() {
  let closure = Expr::Closure {
    params: vec![],
    body: crate::ast::ClosureBody(vec![Statement::Expr(Expr::Literal(Value::Int(1)))]),
    return_type_hint: None,
  };
  let ctx = EvalCtx::default();
  let out = ctx
    .eval_expr(
      &closure,
      &ctx.globals,
      Some(ctx.interned_symbols.intern("name")),
    )
    .unwrap();
  let callable = match out {
    ControlFlow::Continue(Value::Callable(callable)) => callable,
    _ => unreachable!(),
  };
  let callable = Rc::try_unwrap(callable).unwrap();
  let Callable::Closure(closure) = callable else {
    unreachable!();
  };
  let CapturedScope::Strong(captured_scope) = closure.captured_scope else {
    unreachable!();
  };
  // this should be the only place it's strongly referenced, so when the closure is dropped the
  // captured scope (which actually contains it) should also be dropped.
  assert_eq!(Rc::strong_count(&captured_scope), 1,);

  // the captured scope recursively refers to the closure to support recursive calls
  let cloned_closure = captured_scope
    .get(ctx.interned_symbols.intern("name"))
    .unwrap();
  let cloned_closure = cloned_closure.as_callable().unwrap();
  let Callable::Closure(cloned_closure) = &**cloned_closure else {
    unreachable!();
  };

  let CapturedScope::Weak(weak_captured_scope) = &cloned_closure.captured_scope else {
    unreachable!();
  };
  assert_eq!(weak_captured_scope.strong_count(), 1);

  // ensure that this weak reference actually points to the same captured scope
  let _ = Rc::try_unwrap(captured_scope).unwrap();

  assert_eq!(weak_captured_scope.strong_count(), 0);
}

#[test]
fn test_append() {
  let src = r#"
arr = chain([[1], [2]])
arr = arr | append(3)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let arr = ctx.get_global("arr").unwrap();
  let arr = arr.as_sequence().unwrap();
  let eager = seq_as_eager(&*arr).unwrap();
  assert_eq!(
    eager
      .inner
      .iter()
      .map(|v| v.as_int().unwrap())
      .collect::<Vec<_>>(),
    vec![1, 2, 3]
  );
}

#[test]
fn test_map_splat() {
  let src = r#"
x = {a: 1}
y = {b: 2}
z = {*x, c: 3, *y}

w = {*x, a: 4}
v = {b: 1, *y}
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let z = ctx.get_global("z").unwrap();
  let Value::Map(z) = z else {
    panic!("Expected result to be a Map");
  };
  assert_eq!(z.len(), 3);
  assert_eq!(z.get("a").unwrap().as_int(), Some(1));
  assert_eq!(z.get("b").unwrap().as_int(), Some(2));
  assert_eq!(z.get("c").unwrap().as_int(), Some(3));

  let w = ctx.get_global("w").unwrap();
  let Value::Map(w) = w else {
    panic!("Expected result to be a Map");
  };
  assert_eq!(w.len(), 1);
  assert_eq!(w.get("a").unwrap().as_int(), Some(4));

  let v = ctx.get_global("v").unwrap();
  let Value::Map(v) = v else {
    panic!("Expected result to be a Map");
  };
  assert_eq!(v.len(), 1);
  assert_eq!(v.get("b").unwrap().as_int(), Some(2));
}

#[test]
fn test_seq_destructure() {
  let src = r#"
x = [1, 2, 3]
[a, b, c] = x
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let a = ctx.get_global("a").unwrap();
  let a = a.as_int().unwrap();
  assert_eq!(a, 1);

  let b = ctx.get_global("b").unwrap();
  let b = b.as_int().unwrap();
  assert_eq!(b, 2);

  let c = ctx.get_global("c").unwrap();
  let c = c.as_int().unwrap();
  assert_eq!(c, 3);
}

#[test]
fn test_map_destructure() {
  let src = r#"
x = {a: 1, b: 2, c: 3}
{a, b, c} = x
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let a = ctx.get_global("a").unwrap();
  let a = a.as_int().unwrap();
  assert_eq!(a, 1);

  let b = ctx.get_global("b").unwrap();
  let b = b.as_int().unwrap();
  assert_eq!(b, 2);

  let c = ctx.get_global("c").unwrap();
  let c = c.as_int().unwrap();
  assert_eq!(c, 3);
}

#[test]
fn test_nested_destructure() {
  let src = r#"
x = [{a: 1}, 2, [{b: 5}]]
[{a, c}, d, [{b}], z] = x
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let a = ctx.get_global("a").unwrap();
  let a = a.as_int().unwrap();
  assert_eq!(a, 1);

  let c = ctx.get_global("c").unwrap();
  assert!(c.is_nil());

  let d = ctx.get_global("d").unwrap();
  let d = d.as_int().unwrap();
  assert_eq!(d, 2);

  let b = ctx.get_global("b").unwrap();
  let b = b.as_int().unwrap();
  assert_eq!(b, 5);

  let z = ctx.get_global("z").unwrap();
  assert!(z.is_nil());
}

#[test]
fn test_advanced_map_destructure_assignments() {
  let src = r#"
m = {a: 1, b: { c: 2 }}
{a: foo, b: { c: bar }} = m
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let foo = ctx.get_global("foo").unwrap();
  let foo = foo.as_int().unwrap();
  assert_eq!(foo, 1);

  let bar = ctx.get_global("bar").unwrap();
  let bar = bar.as_int().unwrap();
  assert_eq!(bar, 2);
}

#[test]
fn test_arg_destructure() {
  let src = r#"
f = |{x: [b], }, [c, {d, e: f}], [[g]] = [['g']]| { b + c + d + f + g }
x = f({x: ['b'] }, ['c', {d: 'd', e: 'e'}])
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let x = ctx.get_global("x").unwrap();
  let x = x.as_str().unwrap();
  assert_eq!(x, "bcdeg");
}

#[test]
fn test_nested_closure_destructure_assignment_repro() {
  let src = r#"
f = || {
  x = |acc| {
    { x } = acc
  }
  x({})
}

f()"#;

  parse_and_eval_program(src).unwrap();
}

#[test]
fn test_vec3_from_vec2_swizzle() {
  let src = r#"
z = v2(1,2)
x = vec3(z, 3)
y = vec3(9, z)
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let x = ctx.get_global("x").unwrap();
  let Value::Vec3(x) = x else {
    panic!("Expected result to be a Vec3");
  };
  assert_eq!(x, Vec3::new(1., 2., 3.));

  let y = ctx.get_global("y").unwrap();
  let Value::Vec3(y) = y else {
    panic!("Expected result to be a Vec3");
  };
  assert_eq!(y, Vec3::new(9., 1., 2.));
}

#[test]
fn test_destructure_type_infer_repro() {
  let src = r#"
x = [[1, [1, 1]]]
  -> |[r, [dx, dy]]| { box(1) | rot(0, r, 0) }
  | collect
  "#;

  parse_and_eval_program(src).unwrap();
}

#[test]
fn test_inverse_trig_buitins() {
  let src = r#"
a = asin(0.5)
b = acos(0.5)
c = atan(1.0)
d = atan2(1.0, 1.0)

e = asin(vec2(0.5, 0.2))
f = acos(vec2(0.5, 0.6))
g = atan(vec2(1.0, 0.3))

h = asin(vec3(0.5, 0.0, 0.7))
i = acos(vec3(0.5, 0.0, 0.5))
j = atan(vec3(1.0, 0.0, 0.1))"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let a = ctx.get_global("a").unwrap().as_float().unwrap();
  assert_eq!(a, (0.5_f32).asin());
  let b = ctx.get_global("b").unwrap().as_float().unwrap();
  assert_eq!(b, (0.5_f32).acos());
  let c = ctx.get_global("c").unwrap().as_float().unwrap();
  assert_eq!(c, (1.0_f32).atan());
  let d = ctx.get_global("d").unwrap().as_float().unwrap();
  assert_eq!(d, (1.0_f32).atan2(1.0));

  let e = ctx.get_global("e").unwrap();
  let e = e.as_vec2().unwrap();
  assert_eq!(e, &Vec2::new(0.5_f32.asin(), 0.2_f32.asin()));
  let f = ctx.get_global("f").unwrap();
  let f = f.as_vec2().unwrap();
  assert_eq!(f, &Vec2::new(0.5_f32.acos(), 0.6_f32.acos()));
  let g = ctx.get_global("g").unwrap();
  let g = g.as_vec2().unwrap();
  assert_eq!(g, &Vec2::new(1.0_f32.atan(), 0.3_f32.atan()));

  let h = ctx.get_global("h").unwrap();
  let h = h.as_vec3().unwrap();
  assert_eq!(
    h,
    &Vec3::new(0.5_f32.asin(), 0.0_f32.asin(), 0.7_f32.asin())
  );
  let i = ctx.get_global("i").unwrap();
  let i = i.as_vec3().unwrap();
  assert_eq!(
    i,
    &Vec3::new(0.5_f32.acos(), 0.0_f32.acos(), 0.5_f32.acos())
  );
  let j = ctx.get_global("j").unwrap();
  let j = j.as_vec3().unwrap();
  assert_eq!(
    j,
    &Vec3::new(1.0_f32.atan(), 0.0_f32.atan(), 0.1_f32.atan())
  );
}

#[test]
fn test_boolean_short_circuiting() {
  let src = r#"
bail = |x| {
  assert(false)
  return 100
}

f = |x| {
  if x == 1 || x == 2 || x == bail(3) {
    return true
  }
  return false
}

a = f(1)
b = f(2)

f2 = |x| {
  if 1 == 1 || x == 2 {
    return true
  }
  return bail()
}

c = f2(123)"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let a = ctx.get_global("a").unwrap();
  let Value::Bool(a) = a else {
    panic!("Expected result to be a Bool");
  };
  assert!(a, "Expected a to be true");

  let b = ctx.get_global("b").unwrap();
  let Value::Bool(b) = b else {
    panic!("Expected result to be a Bool");
  };
  assert!(b, "Expected b to be true");

  let c = ctx.get_global("c").unwrap();
  let Value::Bool(c) = c else {
    panic!("Expected result to be a Bool");
  };
  assert!(c, "Expected c to be true");
}
