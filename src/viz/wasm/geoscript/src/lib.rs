#![feature(
  impl_trait_in_bindings,
  adt_const_params,
  likely_unlikely
)]
#![cfg_attr(target_arch = "wasm32", feature(unsafe_cell_access))]
#![cfg_attr(not(target_arch = "wasm32"), feature(thread_local))]

#[cfg(target_arch = "wasm32")]
use std::cell::UnsafeCell;
use std::{
  any::Any,
  cell::{Cell, RefCell},
  collections::VecDeque,
  fmt::{Debug, Display},
  rc::{self, Rc},
};

use arrayvec::ArrayVec;
use ast::{Expr, FunctionCallTarget, Statement};
use fxhash::{FxHashMap, FxHashSet, FxHasher64};
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
    maybe_init_op_def_shorthands, parse_top_level_statement, BinOp, ClosureArg, ClosureBody,
    DestructurePattern, FunctionCall, MapLiteralEntry, TopLevelStatement,
  },
  builtins::{
    fn_defs::{fn_sigs, get_builtin_fn_sig_entry_ix, ArgDef, DefaultValue, FnDef, FnSignature},
    resolve_builtin_impl, FUNCTION_ALIASES,
  },
  lights::Light,
  materials::Material,
  mesh_ops::mesh_boolean::{drop_manifold_mesh_handle, eval_mesh_boolean, MeshBooleanOp},
  optimizer::optimize_ast,
  seq::{ChainSeq, IntRange, MapSeq},
};

pub mod ast;
pub mod builtins;
pub mod lights;
pub mod materials;
pub mod mesh_ops;
pub mod noise;
pub mod optimizer;
pub mod path_building;
pub mod preprocess;
mod seq;
pub mod ty;
pub mod type_infer;

pub use self::ast::{traverse_fn_calls, Program};
pub use self::builtins::fn_defs::serialize_fn_defs as get_serialized_builtin_fn_defs;

pub const PRELUDE: &str = include_str!("prelude.geo");

pub const DEP_BIT_GEODESICS: u32 = 1 << 0;
pub const DEP_BIT_CGAL: u32 = 1 << 1;
pub const DEP_BIT_CLIPPER2: u32 = 1 << 2;
pub const DEP_BIT_TEXT2PATH: u32 = 1 << 3;

// Single-threaded WASM makes a global mutable u32 safe for dep tracking.
#[cfg(target_arch = "wasm32")]
static mut USED_ASYNC_DEPS: u32 = 0;

#[cfg(target_arch = "wasm32")]
#[inline(always)]
pub fn or_async_dep_bit(bit: u32) {
  unsafe { USED_ASYNC_DEPS |= bit; }
}

#[cfg(target_arch = "wasm32")]
pub fn reset_async_dep_bits() {
  unsafe { USED_ASYNC_DEPS = 0; }
}

#[cfg(target_arch = "wasm32")]
pub fn get_async_dep_bits() -> u32 {
  unsafe { USED_ASYNC_DEPS }
}

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
    );
  // Postfixes (`.field`, `[idx]`) live inside `chained_term` instead of `expr`, so no
  // postfix op is registered here.
}

#[derive(Clone)]
pub struct ErrorStack {
  pub errors: Vec<String>,
  /// Source location (line, col) where the error originated. Preserves the innermost location.
  pub loc: Option<(u32, u32)>,
}

impl ErrorStack {
  #[cold]
  pub fn new(msg: impl Into<String>) -> Self {
    ErrorStack {
      errors: vec![msg.into()],
      loc: None,
    }
  }

  #[cold]
  pub fn wrap(mut self, msg: impl Into<String>) -> Self {
    self.errors.push(msg.into());
    self
  }

  /// Attach a source location to this error. Preserves the innermost (first set) location.
  #[cold]
  pub fn with_loc(mut self, line: u32, col: u32) -> Self {
    if self.loc.is_none() {
      self.loc = Some((line, col));
    }
    self
  }

  fn new_uninitialized_module(module_name: &str) -> ErrorStack {
    Self::new(format!("__GEOTOY_UNINITIALIZED_MODULE__:{module_name}"))
  }

  fn new_uninitialized_module_with_args(
    module_name: &str,
    args: impl Iterator<Item = String>,
  ) -> ErrorStack {
    let mut err = format!("__GEOTOY_UNINITIALIZED_MODULE__:{module_name}");
    for arg in args {
      err.push_str("||__||");
      err.push_str(&arg);
    }
    ErrorStack::new(err)
  }
}

impl Display for ErrorStack {
  #[cold]
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    if let Some((line, col)) = self.loc {
      write!(f, "at line {line}, column {col}: ")?;
    }

    let indent = "  ";
    for (ix, err) in self.errors.iter().rev().enumerate() {
      let mut lines = err.lines().peekable();
      while let Some(line) = lines.next() {
        for _ in 0..ix {
          write!(f, "{indent}")?;
        }

        write!(f, "{line}")?;

        if lines.peek().is_some() {
          writeln!(f)?;
        }
      }

      if ix < self.errors.len() - 1 {
        writeln!(f)?;
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
  let seq: &dyn Any = seq;
  seq.downcast_ref::<EagerSeq>()
}

#[derive(Clone)]
pub struct PartiallyAppliedFn {
  inner: Rc<Callable>,
  args: Vec<Value>,
  kwargs: FxHashMap<Sym, Value>,
}

impl PartiallyAppliedFn {
  fn get_return_type_hint(&self) -> Option<ArgType> {
    // TODO: not sure if it's possible to determine this since the return type is dynamic based on
    // what args are passed (either another PAF or the final return type)
    None
  }
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
  params: Rc<Vec<ClosureArg>>,
  body: Rc<ClosureBody>,
  /// Contains variables captured from the environment when the closure was created
  captured_scope: CapturedScope,
  /// A scope pre-populated with `nil` placeholders for all arguments.  This is used when invoking
  /// the closure to help avoid allocations.  This scope should always only contain entries for the
  /// arguments of the closure, never any other values.
  ///
  /// This will be `None` in the case of a recursive call since it's taken by the root call.
  arg_placeholder_scope: RefCell<Option<FxHashMap<Sym, Value>>>,
  return_type_hint: Option<ArgType>,
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

impl Closure {
  fn build_arg_placeholder_scope(params: &[ClosureArg]) -> FxHashMap<Sym, Value> {
    let mut arg_placeholders = FxHashMap::default();
    for param in params {
      param.ident.visit_idents(&mut |ident| {
        arg_placeholders.insert(ident, Value::Nil);
      });
    }

    arg_placeholders
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

pub trait DynamicCallable {
  fn as_any(&self) -> &dyn Any;

  // TODO: these need to get merged into an enum across the whole codebase.
  //
  // I even did that work, but it got lost in a merge conflict
  fn is_side_effectful(&self) -> bool;
  fn is_rng_dependent(&self) -> bool;

  fn invoke(
    &self,
    args: &[Value],
    kwargs: &FxHashMap<Sym, Value>,
    ctx: &EvalCtx,
  ) -> Result<Value, ErrorStack>;

  /// Despite being called a hint, this _MUST_ be correct if provided.  This is used for type
  /// inference and optimization and will cause big problems if incorrectly specified.
  fn get_return_type_hint(&self) -> Option<ArgType>;
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
  /// A dynamically produced callable, used for cases where builtins return callables that may
  /// depend on captured variables or other dynamic internal logic
  Dynamic {
    /// Used for printing and debugging.  Should indicate the source and variant of the dynamic
    /// callable.
    name: String,
    inner: Box<dyn DynamicCallable>,
  },
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
        let entry = match fn_sigs().entries.get(*fn_entry_ix) {
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
      Callable::Dynamic { name, .. } => Debug::fmt(&format!("<dynamic callable: \"{name}\">"), f),
    }
  }
}

impl Callable {
  pub fn is_side_effectful(&self) -> bool {
    match self {
      Callable::Builtin { fn_entry_ix, .. } => {
        let name = fn_sigs().entries[*fn_entry_ix].0;
        matches!(
          name,
          "print"
            | "render"
            | "render_path"
            | "set_default_material"
            | "call"
            | "randv"
            | "randf"
            | "randi"
            | "assert"
            | "set_rng_seed"
            | "set_sharp_angle_threshold"
            | "set_curve_angle_threshold"
            | "gizmo"
            | "gizmo2d"
            | "gizmo1d"
            | "gizmo_transform"
            | "transform_gizmo"
        )
      }
      Callable::PartiallyAppliedFn(paf) => paf.inner.is_side_effectful(),
      Callable::Closure(_) => false,
      Callable::ComposedFn(composed) => composed.inner.iter().any(|c| c.is_side_effectful()),
      Callable::Dynamic { inner, .. } => inner.is_side_effectful(),
    }
  }

  pub fn is_rng_dependent(&self) -> bool {
    match self {
      Callable::Builtin { fn_entry_ix, .. } => {
        let name = fn_sigs().entries[*fn_entry_ix].0;
        is_rng_builtin_name(name)
      }
      Callable::PartiallyAppliedFn(paf) => paf.inner.is_rng_dependent(),
      Callable::Closure(_) => false,
      Callable::ComposedFn(composed) => composed.inner.iter().any(|c| c.is_rng_dependent()),
      Callable::Dynamic { inner, .. } => inner.is_rng_dependent(),
    }
  }

  pub fn get_return_type_hint(&self) -> Option<ArgType> {
    match self {
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
      Callable::PartiallyAppliedFn(paf) => paf.get_return_type_hint(),
      Callable::Closure(Closure {
        return_type_hint, ..
      }) => match return_type_hint {
        Some(ty) => return Some((*ty).into()),
        None => return None,
      },
      Callable::ComposedFn(_) => return None,
      Callable::Dynamic { inner, .. } => return inner.get_return_type_hint(),
    }
  }
}

fn is_rng_builtin_name(name: &str) -> bool {
  matches!(name, "randv" | "randf" | "randi")
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
      return *aabb;
    }

    let aabb = self.mesh.compute_aabb(&self.transform);
    *self.aabb.borrow_mut() = Some(aabb);
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
pub type Mat4 = Matrix4<f32>;

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
  // Boxed out-of-line: a bare `Mat4` (64 bytes) would blow the enum size. On
  // wasm32 the `Rc` is a 4-byte thin pointer, keeping `Value` at 16 bytes.
  // Must stay after discriminant 5 so the `clone` fast path doesn't shallow-copy it.
  Mat4(Rc<Mat4>),
}

// The wasm-side `maybe_init` also asserts this at runtime; this fails the build.
#[cfg(target_arch = "wasm32")]
const _: () = assert!(std::mem::size_of::<Value>() == 16);

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
    Value::Callable(callable) => Value::Callable(Rc::clone(callable)),
    Value::Sequence(seq) => Value::Sequence(Rc::clone(seq)),
    Value::Map(map) => Value::Map(Rc::clone(map)),
    Value::String(s) => Value::String(s.clone()),
    Value::Material(material) => Value::Material(Rc::clone(material)),
    Value::Mat4(mat) => Value::Mat4(Rc::clone(mat)),
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
      Value::Mat4(mat) => write!(f, "Mat4({mat:?})"),
      Value::Nil => write!(f, "Nil"),
    }
  }
}

const CONST_EVAL_CACHE_MAX_ENTRIES: usize = 4096;

pub(crate) struct ConstEvalCacheHit {
  pub value: Value,
  pub rng_end_state: Option<Pcg32>,
}

pub struct ConstEvalCacheEntry {
  pub value: Value,
  rng_end_state: Option<Pcg32>,
  last_access: u64,
}

pub struct ConstEvalCache {
  pub entries: FxHashMap<u128, ConstEvalCacheEntry>,
  access_tick: u64,
  access_queue: VecDeque<(u128, u64)>,
  max_entries: usize,
}

impl Default for ConstEvalCache {
  fn default() -> Self {
    Self {
      entries: FxHashMap::default(),
      access_tick: 0,
      access_queue: VecDeque::new(),
      max_entries: CONST_EVAL_CACHE_MAX_ENTRIES,
    }
  }
}

impl ConstEvalCache {
  pub(crate) fn get(&mut self, key: u128) -> Option<ConstEvalCacheHit> {
    self.access_tick = self.access_tick.wrapping_add(1);
    let stamp = self.access_tick;
    let (value, rng_end_state) = {
      let entry = self.entries.get_mut(&key)?;
      entry.last_access = stamp;
      (entry.value.clone(), entry.rng_end_state.clone())
    };
    self.access_queue.push_back((key, stamp));
    Some(ConstEvalCacheHit {
      value,
      rng_end_state,
    })
  }

  pub(crate) fn insert(&mut self, key: u128, value: Value, rng_end_state: Option<Pcg32>) {
    if self.max_entries == 0 {
      return;
    }

    if let Some(entry) = self.entries.get_mut(&key) {
      self.access_tick = self.access_tick.wrapping_add(1);
      let stamp = self.access_tick;
      entry.value = value;
      entry.rng_end_state = rng_end_state;
      entry.last_access = stamp;
      self.access_queue.push_back((key, stamp));
      return;
    }

    self.access_tick = self.access_tick.wrapping_add(1);
    self.entries.insert(
      key,
      ConstEvalCacheEntry {
        value,
        rng_end_state,
        last_access: self.access_tick,
      },
    );
    self.access_queue.push_back((key, self.access_tick));

    while self.entries.len() > self.max_entries {
      let Some((old_key, stamp)) = self.access_queue.pop_front() else {
        break;
      };
      let should_remove = match self.entries.get(&old_key) {
        Some(entry) => entry.last_access == stamp,
        None => false,
      };
      if should_remove {
        self.entries.remove(&old_key);
      }
    }
  }
}

const INT_FLAG: u16 = 0b0000_0000_0000_0001;
const FLOAT_FLAG: u16 = 0b0000_0000_0000_0010;
const NUMERIC_FLAG: u16 = INT_FLAG | FLOAT_FLAG;
const VEC2_FLAG: u16 = 0b0000_0000_0000_0100;
const VEC3_FLAG: u16 = 0b0000_0000_0000_1000;
const MESH_FLAG: u16 = 0b0000_0000_0001_0000;
const LIGHT_FLAG: u16 = 0b0000_0000_0010_0000;
const CALLABLE_FLAG: u16 = 0b0000_0000_0100_0000;
const SEQUENCE_FLAG: u16 = 0b0000_0000_1000_0000;
const MAP_FLAG: u16 = 0b0000_0001_0000_0000;
const BOOL_FLAG: u16 = 0b0000_0010_0000_0000;
const STRING_FLAG: u16 = 0b0000_0100_0000_0000;
const MATERIAL_FLAG: u16 = 0b0000_1000_0000_0000;
const NIL_FLAG: u16 = 0b0001_0000_0000_0000;
const MAT4_FLAG: u16 = 0b0010_0000_0000_0000;
const ANY_FLAG: u16 = 0xFFFF;

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
      Value::Mesh(mesh) => Some(mesh),
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

  fn as_mat4(&self) -> Option<&Mat4> {
    match self {
      Value::Mat4(m) => Some(m),
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

  fn into_literal_expr(&self, loc: SourceLoc) -> Expr {
    Expr::Literal {
      value: self.clone(),
      loc,
    }
  }

  pub fn get_type(&self) -> ArgType {
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
      Value::Mat4(_) => ArgType::Mat4,
      Value::Nil => ArgType::Nil,
    }
  }

  fn as_bitflags(&self) -> u16 {
    match self {
      Value::Int(_) => INT_FLAG,
      Value::Float(_) => FLOAT_FLAG,
      Value::Vec2(_) => VEC2_FLAG,
      Value::Vec3(_) => VEC3_FLAG,
      Value::Mesh(_) => MESH_FLAG,
      Value::Light(_) => LIGHT_FLAG,
      Value::Callable(_) => CALLABLE_FLAG,
      Value::Sequence(_) => SEQUENCE_FLAG,
      Value::Map(_) => MAP_FLAG,
      Value::Bool(_) => BOOL_FLAG,
      Value::String(_) => STRING_FLAG,
      Value::Material(_) => MATERIAL_FLAG,
      Value::Mat4(_) => MAT4_FLAG,
      Value::Nil => NIL_FLAG,
    }
  }
}

#[derive(Clone, Copy, Debug, SerJson)]
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
  Mat4,
  Nil,
  Any,
}

impl ArgType {
  pub fn any_valid(valid_types_flags: u16, arg: &Value) -> bool {
    valid_types_flags & arg.as_bitflags() != 0
  }

  pub const fn as_bitflags(&self) -> u16 {
    match self {
      ArgType::Int => INT_FLAG,
      ArgType::Float => FLOAT_FLAG,
      ArgType::Numeric => NUMERIC_FLAG,
      ArgType::Vec2 => VEC2_FLAG,
      ArgType::Vec3 => VEC3_FLAG,
      ArgType::Mesh => MESH_FLAG,
      ArgType::Light => LIGHT_FLAG,
      ArgType::Callable => CALLABLE_FLAG,
      ArgType::Sequence => SEQUENCE_FLAG,
      ArgType::Map => MAP_FLAG,
      ArgType::Bool => BOOL_FLAG,
      ArgType::String => STRING_FLAG,
      ArgType::Material => MATERIAL_FLAG,
      ArgType::Mat4 => MAT4_FLAG,
      ArgType::Nil => NIL_FLAG,
      ArgType::Any => ANY_FLAG,
    }
  }

  pub const fn as_str(&self) -> &'static str {
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
      ArgType::Mat4 => "mat4",
      ArgType::Nil => "nil",
      ArgType::Any => "any",
    }
  }

  pub fn list_from_bitflags(valid_types: u16) -> Vec<ArgType> {
    if valid_types == ANY_FLAG {
      return vec![ArgType::Any];
    }

    let mut types = Vec::new();
    for arg_type in &[
      ArgType::Vec2,
      ArgType::Vec3,
      ArgType::Mesh,
      ArgType::Light,
      ArgType::Callable,
      ArgType::Sequence,
      ArgType::Map,
      ArgType::Bool,
      ArgType::String,
      ArgType::Material,
      ArgType::Mat4,
      ArgType::Nil,
    ] {
      if valid_types & arg_type.as_bitflags() != 0 {
        types.push(*arg_type);
      }
    }

    // check if both int and float match
    if valid_types & (ArgType::Int.as_bitflags() | ArgType::Float.as_bitflags()) != 0 {
      types.push(ArgType::Numeric);
    } else if valid_types & ArgType::Int.as_bitflags() != 0 {
      types.push(ArgType::Int);
    } else if valid_types & ArgType::Float.as_bitflags() != 0 {
      types.push(ArgType::Float);
    }

    types
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
pub(crate) enum GetArgsOutput {
  Valid {
    def_ix: usize,
    arg_refs: SmallVec<[ArgRef; 6]>,
  },
  PartiallyApplied,
}

#[cold]
pub(crate) fn format_fn_signatures(arg_defs: &[FnSignature]) -> String {
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
          let types_str = ArgType::list_from_bitflags(arg_def.valid_types)
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

#[allow(dead_code)]
#[repr(transparent)]
struct SyncValue(Value);

unsafe impl Send for SyncValue {}
unsafe impl Sync for SyncValue {}

static EMPTY_ARGS_INNER: Vec<SyncValue> = Vec::new();
static EMPTY_KWARGS_INNER: FxHashMap<Sym, SyncValue> =
  std::collections::HashMap::with_hasher(fxhash::FxBuildHasher::new());

const EMPTY_ARGS: &'static Vec<Value> = unsafe { std::mem::transmute(&EMPTY_ARGS_INNER) };
const EMPTY_KWARGS: &'static FxHashMap<Sym, Value> =
  unsafe { std::mem::transmute(&EMPTY_KWARGS_INNER) };

pub(crate) fn get_args(
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
          arg_refs: SmallVec::new(),
        });
      }
    }
  }

  for &key in kwargs.keys() {
    if !defs
      .iter()
      .any(|def| def.arg_defs.iter().any(|arg| arg.interned_name == key))
    {
      return ctx.with_resolved_sym(key, |resolved| {
        Err(ErrorStack::new(format!(
          "kwarg `{resolved}` is not valid in any function signature.\n\nAvailable signatures:\n{}",
          format_fn_signatures(defs)
        )))
      });
    }
  }

  let mut arg_refs: SmallVec<[ArgRef; 6]> = SmallVec::new();
  let mut valid_partial: bool = false;
  let any_args_provided = !args.is_empty() || !kwargs.is_empty();
  'def: for (def_ix, def) in defs.iter().enumerate() {
    // if a kwarg was passed which isn't defined in this function signature, skip
    for &kwarg_key in kwargs.keys() {
      if def
        .arg_defs
        .iter()
        .all(|def| def.interned_name != kwarg_key)
      {
        continue 'def;
      }
    }

    let mut pos_arg_ix = 0;
    arg_refs.clear();
    'arg: for ArgDef {
      default_value,
      description: _,
      name: _,
      interned_name: kwarg_sym,
      valid_types,
    } in def.arg_defs
    {
      let (arg, arg_ref) = if let Some(kwarg) = kwargs.get(kwarg_sym) {
        (kwarg, ArgRef::Keyword(*kwarg_sym))
      } else if pos_arg_ix < args.len() {
        let arg = &args[pos_arg_ix];
        let arg_ref = ArgRef::Positional(pos_arg_ix);
        pos_arg_ix += 1;
        (arg, arg_ref)
      } else {
        if let DefaultValue::Optional(get_default) = default_value {
          arg_refs.push(ArgRef::Default(get_default()));
          continue 'arg;
        } else {
          // If any required argument is missing, mark as partial if any args/kwargs were provided
          if any_args_provided {
            valid_partial = true;
          }
          continue 'def;
        }
      };

      if !ArgType::any_valid(*valid_types, arg) {
        continue 'def;
      }

      arg_refs.push(arg_ref);
    }

    // valid args found for the whole def, so the function call is valid
    return Ok(GetArgsOutput::Valid { def_ix, arg_refs });
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
  let fn_entry = &fn_sigs().entries[fn_entry_ix];
  let defs = fn_entry.1.signatures;
  for (def_ix, def) in defs.iter().enumerate() {
    let lhs_def = &def.arg_defs[0];
    let rhs_def = &def.arg_defs[1];
    if ArgType::any_valid(lhs_def.valid_types, lhs) && ArgType::any_valid(rhs_def.valid_types, rhs)
    {
      return Ok(def_ix);
    }
  }

  return Err(build_no_fn_def_found_err(
    ctx,
    fn_entry.0,
    &[lhs.clone(), rhs.clone()],
    EMPTY_KWARGS,
    fn_entry.1.signatures,
  ));
}

/// Specialized version of `get_args` for more efficient unary operator lookup.  Assumes that each
/// def in `defs` has exactly one arg.
fn get_unop_def_ix(ctx: &EvalCtx, fn_entry_ix: usize, arg: &Value) -> Result<usize, ErrorStack> {
  let fn_entry = &fn_sigs().entries[fn_entry_ix];
  let defs = fn_entry.1.signatures;

  for (def_ix, def) in defs.iter().enumerate() {
    let arg_def = &def.arg_defs[0];
    if ArgType::any_valid(arg_def.valid_types, arg) {
      return Ok(def_ix);
    }
  }

  let fn_name = fn_entry.0;
  return Err(build_no_fn_def_found_err(
    ctx,
    fn_name,
    &[arg.clone()],
    EMPTY_KWARGS,
    defs,
  ));
}

/// Result of type-level signature matching.
pub struct SignatureTypeMatch {
  pub def_ix: usize,
  pub arg_refs: SmallVec<[ArgRef; 6]>,
  pub return_type: &'static [ArgType],
}

/// Type-level signature matching: finds the first signature where all provided positional and
/// keyword argument types are compatible with the parameter definitions.
///
/// Returns a `SignatureTypeMatch` on match, or `None` if no signature matches (including partial
/// application cases where fewer args than required are provided).
pub fn match_signature_by_arg_types(
  sigs: &'static [FnSignature],
  positional_types: &[ArgType],
  kwarg_types: &[(Sym, ArgType)],
) -> Option<SignatureTypeMatch> {
  // Dynamic sigs (first arg name empty) can't be type-matched
  if let Some(sig) = sigs.first() {
    if let Some(arg_def) = sig.arg_defs.first() {
      if arg_def.name.is_empty() {
        return None;
      }
    }
  }

  let mut arg_refs: SmallVec<[ArgRef; 6]> = SmallVec::new();

  'sig: for (sig_ix, sig) in sigs.iter().enumerate() {
    // Skip sigs that don't recognize a provided kwarg
    for &(kwarg_sym, _) in kwarg_types {
      if sig
        .arg_defs
        .iter()
        .all(|def| def.interned_name != kwarg_sym)
      {
        continue 'sig;
      }
    }

    let mut pos_ix = 0;
    let mut all_matched = true;
    arg_refs.clear();

    for arg_def in sig.arg_defs {
      // Check kwargs first, then positional, then default
      if let Some(&(kwarg_sym, ty)) = kwarg_types
        .iter()
        .find(|(sym, _)| *sym == arg_def.interned_name)
      {
        if !arg_type_covered(ty.as_bitflags(), arg_def.valid_types) {
          continue 'sig;
        }
        arg_refs.push(ArgRef::Keyword(kwarg_sym));
      } else if pos_ix < positional_types.len() {
        let ty = positional_types[pos_ix];
        if !arg_type_covered(ty.as_bitflags(), arg_def.valid_types) {
          continue 'sig;
        }
        arg_refs.push(ArgRef::Positional(pos_ix));
        pos_ix += 1;
      } else {
        match &arg_def.default_value {
          DefaultValue::Required => {
            // Missing required arg — could be partial application
            all_matched = false;
            break;
          }
          DefaultValue::Optional(get_default) => {
            arg_refs.push(ArgRef::Default(get_default()));
          }
        }
      }
    }

    if all_matched {
      return Some(SignatureTypeMatch {
        def_ix: sig_ix,
        arg_refs: arg_refs.clone(),
        return_type: sig.return_type,
      });
    }
  }

  None
}

/// Type-level binary operator signature matching.  Each signature is assumed to have exactly two
/// arg defs.
/// A statically-inferred arg type may only be pre-resolved to an overload that covers *every*
/// runtime type it could still be. Mere overlap is unsound: a `num` arg overlaps an `int`-only
/// overload, but the baked-in def_ix then panics (`as_int().unwrap()`) when the value turns out to
/// be a float at runtime. Ambiguous types that aren't fully covered fall back to runtime dispatch.
#[inline]
fn arg_type_covered(arg_flags: u16, param_valid: u16) -> bool {
  arg_flags & !param_valid == 0
}

pub fn match_binop_by_arg_types(
  fn_entry_ix: usize,
  lhs_ty: ArgType,
  rhs_ty: ArgType,
) -> Option<(usize, &'static [ArgType])> {
  let fn_entry = &fn_sigs().entries[fn_entry_ix];
  let sigs = fn_entry.1.signatures;
  let lhs_flags = lhs_ty.as_bitflags();
  let rhs_flags = rhs_ty.as_bitflags();

  for (sig_ix, sig) in sigs.iter().enumerate() {
    let lhs_def = &sig.arg_defs[0];
    let rhs_def = &sig.arg_defs[1];
    if arg_type_covered(lhs_flags, lhs_def.valid_types)
      && arg_type_covered(rhs_flags, rhs_def.valid_types)
    {
      return Some((sig_ix, sig.return_type));
    }
  }

  None
}

/// Type-level unary operator signature matching.  Each signature is assumed to have exactly one
/// arg def.
pub fn match_unop_by_arg_types(fn_entry_ix: usize, arg_ty: ArgType) -> Option<&'static [ArgType]> {
  let fn_entry = &fn_sigs().entries[fn_entry_ix];
  let sigs = fn_entry.1.signatures;
  let arg_flags = arg_ty.as_bitflags();

  for sig in sigs {
    let arg_def = &sig.arg_defs[0];
    if arg_type_covered(arg_flags, arg_def.valid_types) {
      return Some(sig.return_type);
    }
  }

  None
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

  pub fn borrow(&self) -> std::cell::Ref<'_, Vec<T>> {
    self.inner.borrow()
  }
}

/// A mesh registered for rendering plus the module that fired the `render` call.
/// `source_module` is `None` for renders fired outside any module (e.g. ambient
/// scope construction); JS drops those. `mesh_id` is stable across cache replays,
/// so JS uses it as the reuse key.
#[derive(Clone)]
pub struct RenderedMesh {
  pub mesh: Rc<MeshHandle>,
  pub source_module: Option<String>,
  pub mesh_id: u32,
}

/// `source_module` is captured for cross-run cache dedupe only: when two cached
/// entries both transitively replay the same dependency, the second hit must
/// skip items whose origin module is already in `replayed_this_run`. JS never
/// reads the field for lights or paths.
#[derive(Clone)]
pub struct RenderedLight {
  pub light: Light,
  pub source_module: Option<String>,
  pub light_id: u32,
}

#[derive(Clone)]
pub struct RenderedPath {
  pub points: Vec<Vec3>,
  pub source_module: Option<String>,
  pub path_id: u32,
}

type RenderedMeshes = AppendOnlyBuffer<RenderedMesh>;
type RenderedLights = AppendOnlyBuffer<RenderedLight>;
type RenderedPaths = AppendOnlyBuffer<RenderedPath>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GizmoKind {
  Vec3,
  Transform,
}

/// A `gizmo(...)` / `gizmo_transform(...)` value site reported to the host so the
/// editor can draw an interactive handle. `current_value` is the value the program
/// actually saw this eval (injected, defaulted, or zero); `resolved_origin` is where
/// the 3D gizmo is drawn. Replayed from the module cache like `RenderedMesh`.
#[derive(Clone)]
pub struct RenderedGizmo {
  pub source_module: Option<String>,
  pub handle_id: String,
  pub kind: GizmoKind,
  pub resolved_origin: Vec3,
  pub current_value: Value,
  /// vec3 `absolute=` kwarg (transform handles are always absolute). Lets the host
  /// resolve delta-vs-absolute without re-parsing source.
  pub absolute: bool,
  /// Per-axis drag mask; `gizmo2d`/`gizmo1d` restrict the live gizmo to a subset.
  pub axes: [bool; 3],
  /// Per-gizmo ghost-render override; `None` defers to the host's global setting.
  pub ghost: Option<bool>,
}

type RenderedGizmos = AppendOnlyBuffer<RenderedGizmo>;

#[derive(Default, Debug)]
pub struct Scope {
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

pub fn get_default_globals() -> [(&'static str, Value); 2] {
  [
    ("pi", Value::Float(std::f32::consts::PI)),
    ("tau", Value::Float(std::f32::consts::TAU)),
  ]
}

impl Scope {
  pub fn default_globals(interner: &SymbolInterner) -> Self {
    let scope = Scope::default();

    for (name, val) in get_default_globals() {
      scope.insert(interner.intern(name), val);
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

  fn wrap(parent: Rc<Scope>) -> Scope {
    Scope {
      vars: RefCell::new(Default::default()),
      parent: Some(parent),
    }
  }

  fn has(&self, key: Sym) -> bool {
    self.vars.borrow().contains_key(&key)
      || self.parent.as_ref().map(|p| p.has(key)).unwrap_or(false)
  }

  /// Collects bindings reachable through this scope and its parent chain,
  /// innermost-first (i.e. shadowing wins). Used by the optimizer to seed
  /// its scope tracker from the ambient scope.
  pub fn collect_bindings_innermost_first(&self) -> Vec<(Sym, Value)> {
    let mut seen: FxHashSet<Sym> = FxHashSet::default();
    let mut out: Vec<(Sym, Value)> = Vec::new();
    let mut cur: Option<&Scope> = Some(self);
    while let Some(s) = cur {
      for (sym, val) in s.vars.borrow().iter() {
        if seen.insert(*sym) {
          out.push((*sym, val.clone()));
        }
      }
      cur = s.parent.as_deref();
    }
    out
  }
}

/// Handle to an interned symbol.
///
/// This is done to speed up variable lookups by avoiding string comparisons.
#[derive(Clone, Debug, Copy, PartialEq, Eq, Hash)]
pub struct Sym(pub usize);

#[derive(Clone)]
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

static mut DEFAULT_INTERNER: *const SymbolInterner = std::ptr::null();

#[cfg(not(target_arch = "wasm32"))]
static INTERNER_INIT: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn build_default_symbol_interner() -> SymbolInterner {
  let interner = SymbolInterner {
    symbols: RefCell::new(FxHashMap::default()),
    reverse_symbols: RefCell::new(FxHashMap::default()),
    next_sym: Cell::new(0),
  };

  for (name, _val) in get_default_globals() {
    interner.intern(name);
  }

  for FnDef {
    module: _,
    examples: _,
    signatures,
  } in fn_sigs().values()
  {
    for FnSignature {
      arg_defs,
      description: _,
      return_type: _,
    } in *signatures
    {
      for ArgDef {
        name,
        interned_name,
        valid_types: _,
        default_value: _,
        description: _,
      } in *arg_defs
      {
        let sym = interner.intern(name);
        // don't worry about it
        let addr: *mut Sym = unsafe { std::mem::transmute::<&Sym, *mut Sym>(interned_name) };
        unsafe {
          addr.write(sym);
        }
      }
    }
  }

  interner
}

fn get_or_init_default_symbol_interner() -> SymbolInterner {
  unsafe {
    #[cfg(not(target_arch = "wasm32"))]
    let _lock = INTERNER_INIT.lock().unwrap();

    if !DEFAULT_INTERNER.is_null() {
      return (*DEFAULT_INTERNER).clone();
    }

    let interner = build_default_symbol_interner();
    let out = interner.clone();
    DEFAULT_INTERNER = Box::into_raw(Box::new(interner));

    out
  }
}

impl Default for SymbolInterner {
  fn default() -> Self {
    get_or_init_default_symbol_interner()
  }
}

use crate::ast::SourceLoc;
use crate::preprocess::Edit;

/// Maps SourceLoc indices to (line, col) pairs in *original-source coordinates*.
///
/// Preprocessor edits are applied at `add()` time, not `get()` time: a single
/// `SourceMap` outlives many parses (imports, REPL, ambient scope), and lazy
/// translation would re-apply a later parse's edits to an earlier parse's locs.
#[derive(Default)]
pub struct SourceMap {
  /// (line, col, prelude_offset) triples. Index 0 is the "unknown" sentinel.
  /// `prelude_offset` is recorded per-entry (not globally) so module sources, which
  /// don't include the root prelude, aren't falsely shifted.
  locations: Vec<(u32, u32, u32)>,
  /// Lines of prelude prepended at the time of the current parse. Callers parsing
  /// module sources should save/zero/restore this around the parse.
  pub prelude_line_count: u32,
  /// Active preprocessor edits, set by `parse_program_src` and consumed only by
  /// `add()`. INVARIANT: replacements contain no newlines, so each edit only shifts
  /// columns within a single line.
  pub edits: Vec<Edit>,
}

impl SourceMap {
  pub fn new(prelude_line_count: u32) -> Self {
    let mut map = SourceMap {
      locations: Vec::new(),
      prelude_line_count,
      edits: Vec::new(),
    };
    // Reserve index 0 as "unknown location" sentinel
    map.locations.push((0, 0, 0));
    map
  }

  /// Add a location and return its SourceLoc index. `line`/`col` are in
  /// rewritten-source coords (as Pest reports them); they're translated through the
  /// active preprocessor edits before storage.
  pub fn add(&mut self, line: usize, col: usize) -> SourceLoc {
    let (orig_line, orig_col) = translate_through_edits(&self.edits, line as u32, col as u32);
    let idx = self.locations.len() as u32;
    self
      .locations
      .push((orig_line, orig_col, self.prelude_line_count));
    SourceLoc(idx)
  }

  /// Get the (line, col) for a SourceLoc. Returns (0, 0) for unknown locations.
  pub fn get(&self, loc: SourceLoc) -> (u32, u32) {
    let (line, col, prelude) = match self.locations.get(loc.0 as usize).copied() {
      Some(triple) => triple,
      None => return (0, 0),
    };
    // Locations inside the prepended prelude/ambient region collapse to the (0, 0) sentinel
    // so diagnostics there are dropped (callers guard on `line == 0 && col == 0`).
    let adjusted = line.saturating_sub(prelude);
    if adjusted == 0 {
      (0, 0)
    } else {
      (adjusted, col)
    }
  }
}

fn translate_through_edits(edits: &[Edit], line: u32, col: u32) -> (u32, u32) {
  if edits.is_empty() {
    return (line, col);
  }
  let mut col_shift: i32 = 0;
  for edit in edits {
    if edit.line != line {
      if edit.line > line {
        break;
      }
      continue;
    }
    let edit_end_col_in_rewritten = edit.col_in_rewritten + edit.replacement.len() as u32;
    if edit_end_col_in_rewritten <= col {
      let original_len = (edit.original_end - edit.original_start) as i32;
      col_shift += edit.replacement.len() as i32 - original_len;
    } else if edit.col_in_rewritten <= col {
      return (line, edit.col_in_original);
    } else {
      break;
    }
  }
  (line, ((col as i32) - col_shift).max(1) as u32)
}

/// Cached result of a successful module evaluation. Side effects + RNG/async
/// bookkeeping are stored only for this module's own body — deps' work is
/// reproduced by recursively `resolve_module`ing each direct import on cache
/// hit, which lets the natural `replayed_this_run` short-circuit handle dedup.
pub struct ModuleExportsCacheEntry {
  pub source_hash: u64,
  pub exports: Rc<FxHashMap<String, Value>>,
  pub own_renders: Vec<RenderedMesh>,
  pub own_lights: Vec<RenderedLight>,
  pub own_paths: Vec<RenderedPath>,
  pub own_gizmos: Vec<RenderedGizmo>,
  pub rng_state_at_start: Pcg32,
  pub rng_state_at_end: Pcg32,
  pub direct_imports: Vec<(String, u64)>,
  /// Handle ids this module read via `gizmo(...)`, paired with a content hash of the
  /// injected value it saw. A cache hit requires each handle's current injected value
  /// to still hash the same — so dragging one gizmo re-evals only its owning module.
  pub gizmo_reads: Vec<(String, u64)>,
  pub own_async_deps_bitmask: u32,
}

const MODULE_CACHE_MAX_ENTRIES: usize = 200;

pub struct EvalCtx {
  pub globals: Scope,
  pub interned_symbols: SymbolInterner,
  pub rendered_meshes: RenderedMeshes,
  pub rendered_lights: RenderedLights,
  pub rendered_paths: RenderedPaths,
  pub rendered_gizmos: RenderedGizmos,
  pub log_fn: fn(&str),
  #[cfg(target_arch = "wasm32")]
  rng: UnsafeCell<Pcg32>,
  pub materials: FxHashMap<String, Rc<Material>>,
  pub textures: FxHashSet<String>,
  pub default_material: RefCell<Option<Rc<Material>>>,
  pub sharp_angle_threshold_degrees: RefCell<f32>,
  /// Default `curve_angle_degrees` for discretizing continuous path features when the kwarg is omitted.
  pub default_curve_angle_degrees: RefCell<f32>,
  pub const_eval_cache: RefCell<ConstEvalCache>,
  scratch_args: Box<RefCell<ArrayVec<Vec<Value>, 64>>>,
  scratch_kwargs: Box<RefCell<ArrayVec<FxHashMap<Sym, Value>, 64>>>,
  /// Maps SourceLoc indices to (line, col) pairs for error reporting.
  pub source_map: RefCell<SourceMap>,
  /// Source code for registered modules, keyed by module name.
  pub module_sources: RefCell<FxHashMap<String, String>>,
  /// Per-module source hashes; `setModuleSources` diffs against these to pick
  /// cache entries to evict.
  pub module_source_hashes: RefCell<FxHashMap<String, u64>>,
  /// Cached export maps for modules that have already been executed.
  pub module_exports: RefCell<FxHashMap<String, Rc<ModuleExportsCacheEntry>>>,
  /// LRU order for `module_exports`. Capped to bound long-session growth.
  pub module_exports_lru: RefCell<VecDeque<String>>,
  /// Export map being built during a module eval; `None` for the main program.
  pub current_module_exports: RefCell<Option<FxHashMap<Sym, Value>>>,
  /// Imports observed in the currently-evaluating module body, accumulated for
  /// the cache entry's `direct_imports`. `None` outside module eval.
  pub current_module_imports: RefCell<Option<Vec<(String, u64)>>>,
  /// Optional scope cloned as the base of each module evaluation (including the
  /// main `_root` program) in place of the default `pi`/`tau`-only globals scope.
  /// Built externally (typically from prelude + globals sources) and installed
  /// via `set_ambient_scope`.
  pub ambient_scope: RefCell<Option<Rc<Scope>>>,
  /// Set of module names currently being resolved. Used to detect circular imports.
  pub modules_in_flight: RefCell<FxHashSet<String>>,
  /// Name of the module whose body is currently being evaluated, used to tag each
  /// `render` call with its source module. Set by `resolve_module_inner` while
  /// evaluating a module body, and by the wasm-side top-level eval wrapper to
  /// `_root` for the entry-point program. `None` while building the ambient scope.
  pub current_module: RefCell<Option<String>>,
  /// Modules whose side effects have been produced or replayed in this run.
  /// Skips redundant work when the same module is imported multiple times; reset
  /// at run start and after ambient-scope construction.
  pub replayed_this_run: RefCell<FxHashSet<String>>,
  /// Hash of the most recently installed ambient sources; lets the next install
  /// skip cache invalidation when the sources haven't changed.
  pub last_ambient_hash: RefCell<Option<u64>>,
  /// Monotonic id counter for renders/lights/paths. Preserved across `reset` so
  /// fresh pushes can't collide with cached-replay ids.
  pub next_render_id: Cell<u32>,
  /// Host-injected gizmo values: `module name -> handleId -> Value` (Vec3 or Mat4).
  /// Replaces the entire map per run (set via the wasm boundary before eval); a
  /// missing entry means the call falls back to its `default`/zero.
  pub gizmo_values: RefCell<FxHashMap<String, FxHashMap<String, Value>>>,
  /// Handle ids read by the currently-evaluating module body, accumulated for the
  /// cache entry's `gizmo_reads`. `None` outside module eval.
  pub current_module_gizmo_reads: RefCell<Option<FxHashSet<String>>>,
  /// Per-module-eval counter assigning `@N` ids to unnamed gizmo calls. Saved/reset
  /// by the module-context guard so nested module evals don't interfere.
  pub current_module_unnamed_gizmo_count: Cell<u32>,
}

unsafe impl Send for EvalCtx {}
unsafe impl Sync for EvalCtx {}

impl Default for EvalCtx {
  fn default() -> Self {
    let interned_symbols = SymbolInterner::default();
    let globals = Scope::default_globals(&interned_symbols);
    EvalCtx {
      globals,
      interned_symbols,
      rendered_meshes: RenderedMeshes::default(),
      rendered_lights: RenderedLights::default(),
      rendered_paths: RenderedPaths::default(),
      rendered_gizmos: RenderedGizmos::default(),
      log_fn: |msg| println!("{msg}"),
      #[cfg(target_arch = "wasm32")]
      rng: UnsafeCell::new(Pcg32::new(7718587666045340534, 17289744314186392832)),
      materials: FxHashMap::default(),
      textures: FxHashSet::default(),
      default_material: RefCell::new(None),
      sharp_angle_threshold_degrees: RefCell::new(45.8366),
      default_curve_angle_degrees: RefCell::new(1.0),
      const_eval_cache: RefCell::new(ConstEvalCache::default()),
      scratch_args: Box::new(RefCell::new(ArrayVec::new())),
      scratch_kwargs: Box::new(RefCell::new(ArrayVec::new())),
      source_map: RefCell::new(SourceMap::new(0)),
      module_sources: RefCell::new(FxHashMap::default()),
      module_source_hashes: RefCell::new(FxHashMap::default()),
      module_exports: RefCell::new(FxHashMap::default()),
      module_exports_lru: RefCell::new(VecDeque::new()),
      current_module_exports: RefCell::new(None),
      current_module_imports: RefCell::new(None),
      ambient_scope: RefCell::new(None),
      modules_in_flight: RefCell::new(FxHashSet::default()),
      current_module: RefCell::new(None),
      replayed_this_run: RefCell::new(FxHashSet::default()),
      last_ambient_hash: RefCell::new(None),
      next_render_id: Cell::new(1),
      gizmo_values: RefCell::new(FxHashMap::default()),
      current_module_gizmo_reads: RefCell::new(None),
      current_module_unnamed_gizmo_count: Cell::new(0),
    }
  }
}

impl Debug for EvalCtx {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("EvalCtx")
      .field("globals", &self.globals)
      .finish()
  }
}

#[derive(Debug)]
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

  pub(crate) fn rng_state(&self) -> Pcg32 {
    self.rng().clone()
  }

  pub(crate) fn set_rng_state(&self, state: Pcg32) {
    *self.rng() = state;
  }

  /// Reseed to the default starting state. Determinism here is what lets the
  /// cross-run cache replay produce identical bytes.
  pub fn reset_rng_to_default(&self) {
    self.set_rng_state(Pcg32::new(7718587666045340534, 17289744314186392832));
  }

  pub fn get_args_scratch(&self) -> Vec<Value> {
    self.scratch_args.borrow_mut().pop().unwrap_or_default()
  }

  pub fn restore_args_scratch(&self, mut args: Vec<Value>) {
    args.clear();
    let mut borrowed = self.scratch_args.borrow_mut();
    let _ = borrowed.try_push(args);
  }

  pub fn get_kwargs_scratch(&self) -> FxHashMap<Sym, Value> {
    self
      .scratch_kwargs
      .borrow_mut()
      .pop()
      .unwrap_or_else(FxHashMap::default)
  }

  /// Resolve a SourceLoc to (line, col). Returns (0, 0) for unknown locations.
  pub fn resolve_loc(&self, loc: SourceLoc) -> (u32, u32) {
    self.source_map.borrow().get(loc)
  }

  /// Add a source location to the map and return its SourceLoc index.
  pub fn add_source_loc(&self, line: usize, col: usize) -> SourceLoc {
    self.source_map.borrow_mut().add(line, col)
  }

  pub fn restore_kwargs_scratch(&self, mut kwargs: FxHashMap<Sym, Value>) {
    kwargs.clear();
    let mut borrowed = self.scratch_kwargs.borrow_mut();
    let _ = borrowed.try_push(kwargs);
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
          early_exit => {
            self.restore_args_scratch(args);
            return Ok(early_exit);
          }
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
          early_exit => {
            self.restore_kwargs_scratch(kwargs);
            return Ok(early_exit);
          }
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
            args.as_ref().unwrap_or(EMPTY_ARGS),
            kwargs.as_ref().unwrap_or(EMPTY_KWARGS),
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
            return self.with_resolved_sym(*name, |name| {
              Err(ErrorStack::new(format!(
                "\"{name}\" is not a callable; found: {global:?}"
              )))
            });
          };

          do_call(&callable, args_opt, kwargs_opt)
        } else {
          return self.with_resolved_sym(*name, |name| {
            Err(ErrorStack::new(format!(
              "No variable found with name `{name}`"
            )))
          });
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
    let (line, col) = self.resolve_loc(expr.loc());
    match expr {
      Expr::Call { call, .. } => self
        .eval_fn_call(scope, call)
        .map_err(|err| err.with_loc(line, col)),
      Expr::BinOp {
        op,
        lhs,
        rhs,
        pre_resolved_def_ix,
        ..
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
              return Err(
                ErrorStack::new(format!(
                  "Left-hand side of `{op:?}` must be a boolean, found: {lhs:?}"
                ))
                .with_loc(line, col),
              )
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
          .map_err(|err| {
            err
              .wrap(format!("Error applying binary operator `{op:?}`"))
              .with_loc(line, col)
          })
      }
      Expr::PrefixOp { op, expr, .. } => {
        let val = match self.eval_expr(expr, scope, None)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        op.apply(self, &val)
          .map(ControlFlow::Continue)
          .map_err(|err| {
            err
              .wrap(format!("Error applying prefix operator `{op:?}`"))
              .with_loc(line, col)
          })
      }
      Expr::Range {
        start,
        end,
        inclusive,
        ..
      } => {
        let start = match self.eval_expr(start, scope, None)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        let Value::Int(start) = start else {
          return Err(
            ErrorStack::new(format!("Range start must be an integer, found: {start:?}"))
              .with_loc(line, col),
          );
        };
        let end = match end {
          Some(end) => {
            let end = match self.eval_expr(end, scope, None)? {
              ControlFlow::Continue(val) => val,
              early_exit => return Ok(early_exit),
            };
            let Value::Int(mut end) = end else {
              return Err(
                ErrorStack::new(format!("Range end must be an integer, found: {end:?}"))
                  .with_loc(line, col),
              );
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
      Expr::Ident { name, .. } => self
        .eval_ident(*name, scope)
        .map(ControlFlow::Continue)
        .map_err(|err| err.with_loc(line, col)),
      Expr::Literal { value, .. } => Ok(ControlFlow::Continue(value.clone())),
      Expr::ArrayLiteral {
        elements: elems, ..
      } => {
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
      Expr::MapLiteral { entries, .. } => {
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
                return Err(
                  ErrorStack::new(format!(
                    "Tried to splat value of type {:?} into map; expected a map.",
                    splat.get_type()
                  ))
                  .with_loc(line, col),
                );
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
        arg_placeholder_scope,
        return_type_hint,
        ..
      } => {
        // cloning the scope here makes the closure function like a rust `move` closure
        // where all the values are cloned before being moved into the closure.
        let captured_scope = Rc::new(scope.clone());

        if let Some(binding_name) = binding_name {
          // add the closure itself to the scope to support recursive calls
          captured_scope.insert(
            binding_name,
            Value::Callable(Rc::new(Callable::Closure(Closure {
              params: Rc::clone(params),
              body: Rc::clone(body),
              // writing this as a weak reference prevents a reference cycle:
              //
              // closure -> captured_scope -> closure_clone
              //                   ^---------------------┘
              //                           ^ this one is weak
              captured_scope: CapturedScope::Weak(Rc::downgrade(&captured_scope)),
              arg_placeholder_scope: RefCell::new(None),
              return_type_hint: *return_type_hint,
            }))),
          );
        }

        Ok(ControlFlow::Continue(Value::Callable(Rc::new(
          Callable::Closure(Closure {
            params: Rc::clone(params),
            body: Rc::clone(body),
            captured_scope: CapturedScope::Strong(captured_scope),
            arg_placeholder_scope: RefCell::new(Some(arg_placeholder_scope.clone())),
            return_type_hint: *return_type_hint,
          }),
        ))))
      }
      Expr::StaticFieldAccess {
        lhs: obj, field, ..
      } => {
        let lhs = match self.eval_expr(obj, scope, None)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        self
          .eval_static_field_access(&lhs, field)
          .map(ControlFlow::Continue)
          .map_err(|err| err.with_loc(line, col))
      }
      Expr::FieldAccess { lhs, field, .. } => {
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
          .map_err(|err| err.with_loc(line, col))
      }
      Expr::Conditional {
        cond,
        then,
        else_if_exprs,
        else_expr,
        ..
      } => {
        let cond = match self.eval_expr(cond, scope, None)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        let Value::Bool(cond) = cond else {
          return Err(
            ErrorStack::new(format!(
              "Condition passed to if statement must be a boolean; found: {cond:?}"
            ))
            .with_loc(line, col),
          );
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
            return Err(
              ErrorStack::new(format!(
                "Condition passed to else-if statement must be a boolean; found: {else_if_cond:?}"
              ))
              .with_loc(line, col),
            );
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
      Expr::Block { statements, .. } => {
        // TODO: ideally, we'd avoid cloning the scope here and use the scope nesting functionality
        // like closures.  However, adding in references to scopes creates incredibly lifetime
        // headaches across the whole codebase very quickly and just isn't worth it rn
        let block_scope = Scope::wrap(Rc::new(scope.clone()));
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
    type_hint: Option<ArgType>,
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
        ..
      } => {
        let (line, col) = self.resolve_loc(expr.loc());
        let val = match self.eval_expr(expr, scope, Some(*name))? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        self
          .eval_assignment(*name, val, scope, *type_hint)
          .map(ControlFlow::Continue)
          .map_err(|err| err.with_loc(line, col))
      }
      Statement::DestructureAssignment { lhs, rhs } => {
        let (line, col) = self.resolve_loc(rhs.loc());
        let rhs = match self.eval_expr(rhs, scope, None)? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        lhs
          .visit_assignments(self, rhs, &mut |lhs, rhs| {
            self.eval_assignment(lhs, rhs, scope, None)?;
            Ok(())
          })
          .map_err(|err| {
            err
              .wrap("Error evaluating destructure assignment")
              .with_loc(line, col)
          })?;
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

  fn eval_top_level_statement(
    &self,
    statement: &TopLevelStatement,
    scope: &Scope,
  ) -> Result<ControlFlow<Value>, ErrorStack> {
    match statement {
      TopLevelStatement::Statement(stmt) => self.eval_statement(stmt, scope),
      TopLevelStatement::Export {
        name,
        expr,
        type_hint,
        ..
      } => {
        let (line, col) = self.resolve_loc(expr.loc());
        let val = match self.eval_expr(expr, scope, Some(*name))? {
          ControlFlow::Continue(val) => val,
          early_exit => return Ok(early_exit),
        };
        self
          .eval_assignment(*name, val.clone(), scope, *type_hint)
          .map_err(|err| err.with_loc(line, col))?;

        // Store in module export map if we're inside a module evaluation
        if let Some(map) = self.current_module_exports.borrow_mut().as_mut() {
          map.insert(*name, val);
        }

        Ok(ControlFlow::Continue(Value::Nil))
      }
      TopLevelStatement::Import {
        bindings,
        module_name,
      } => {
        let exports = self.resolve_module(module_name)?;

        let map_value = Value::Map(exports);
        bindings
          .visit_assignments(self, map_value, &mut |sym, val| {
            self.eval_assignment(sym, val, scope, None)?;
            Ok(())
          })
          .map_err(|err| err.wrap(&format!("Error importing from module \"{module_name}\"")))?;

        Ok(ControlFlow::Continue(Value::Nil))
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
      let builtin_name = fn_sigs().entries[*fn_entry_ix].0;
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
          EMPTY_KWARGS,
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
        .invoke_callable(callable, &[acc, value, Value::Int(i as i64)], EMPTY_KWARGS)
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
      let builtin_name = fn_sigs().entries[*fn_entry_ix].0;
      if matches!(builtin_name, "union" | "difference" | "intersect") {
        return eval_mesh_boolean(
          1,
          &[ArgRef::Positional(0), ArgRef::Positional(1)],
          &[Value::Sequence(seq)],
          EMPTY_KWARGS,
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
        .invoke_callable(fn_value, &[acc, value, Value::Int(i as i64)], EMPTY_KWARGS)
        .map_err(|err| err.wrap("Error invoking callable in reduce"))?;
    }
    Ok(acc)
  }

  pub(crate) fn eval_ident(&self, name: Sym, scope: &Scope) -> Result<Value, ErrorStack> {
    if let Some(local) = scope.get(name) {
      return Ok(local);
    }

    // look it up as a builtin fn
    let resolved = self.with_resolved_sym(name, |resolved_name| {
      get_builtin_fn_sig_entry_ix(resolved_name).map(|ix| (ix, resolve_builtin_impl(resolved_name)))
    });
    if let Some((fn_entry_ix, fn_impl)) = resolved {
      return Ok(Value::Callable(Rc::new(Callable::Builtin {
        fn_entry_ix,
        fn_impl,
        pre_resolved_signature: None,
      })));
    }

    self.with_resolved_sym(name, |resolved_name| {
      Err(ErrorStack::new(format!(
        "Variable `{resolved_name}` not defined",
      )))
    })
  }

  fn eval_closure_args(
    &self,
    closure: &Closure,
    captured_scope: &Rc<Scope>,
    args: &[Value],
    kwargs: &FxHashMap<Sym, Value>,
    mut insert_arg: impl FnMut(Sym, Value),
  ) -> Result<(Option<usize>, bool), ErrorStack> {
    let mut pos_arg_ix = 0usize;
    let mut any_args_valid = false;
    let mut invalid_arg_ix = None;
    for arg in &*closure.params {
      match &arg.ident {
        // there's no way currently to assign a name to destructured args, so they can only be used
        // positionally
        DestructurePattern::Ident(name) => {
          if let Some(kwarg) = kwargs.get(name) {
            if let Some(type_hint) = arg.type_hint {
              type_hint.validate_val(kwarg).map_err(|err| {
                self.with_resolved_sym(*name, |name| {
                  err.wrap(format!("Type error for closure kwarg `{name}`"))
                })
              })?;
            }
            insert_arg(*name, kwarg.clone());
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
          type_hint.validate_val(pos_arg).map_err(|err| {
            err.wrap(format!(
              "Type error for positional closure arg `{:?}`",
              arg.ident.debug(self)
            ))
          })?;
        }
        arg
          .ident
          .visit_assignments(self, pos_arg.clone(), &mut |k, v| {
            insert_arg(k, v);
            Ok(())
          })?;
        any_args_valid = true;
        pos_arg_ix += 1;
      } else {
        if let Some(default_val) = &arg.default_val {
          let default_val = self.eval_expr(default_val, &captured_scope, None)?;
          let default_val = match default_val {
            ControlFlow::Continue(val) => val,
            ControlFlow::Return(_) => {
              return Err(ErrorStack::new(format!(
                "`return` isn't valid in arg default value expressions; found in default value \
                 for arg `{:?}`",
                arg.ident.debug(self)
              )))
            }
            ControlFlow::Break(_) => {
              return Err(ErrorStack::new(format!(
                "`break` isn't valid in arg default value expressions; found in default value for \
                 arg `{:?}`",
                arg.ident.debug(self)
              )));
            }
          };

          arg
            .ident
            .visit_assignments(self, default_val, &mut |k, v| {
              insert_arg(k, v);
              Ok(())
            })?;
        } else {
          if invalid_arg_ix.is_none() {
            invalid_arg_ix = Some(pos_arg_ix);
          }
        }
      }
    }

    Ok((invalid_arg_ix, any_args_valid))
  }

  fn invoke_closure(
    &self,
    closure: &Closure,
    args: &[Value],
    kwargs: &FxHashMap<Sym, Value>,
  ) -> Result<Value, ErrorStack> {
    // TODO: should do some basic analysis to see which variables are actually needed and avoid
    // cloning the rest

    let captured_scope = closure.captured_scope.upgrade().unwrap();
    let has_arg_placeholders = closure.arg_placeholder_scope.borrow().is_some();

    let (arg_vars, invalid_arg_ix, any_args_valid) = if has_arg_placeholders {
      let mut arg_vars = closure.arg_placeholder_scope.borrow_mut().take().unwrap();
      let insert_arg = |name: Sym, val: Value| {
        let slot = arg_vars.get_mut(&name).unwrap_or_else(|| {
          panic!("Internal error: closure arg scope missing slot for arg `{name:?}`")
        });
        *slot = val;
      };

      let (invalid_arg_ix, any_args_valid) =
        self.eval_closure_args(closure, &captured_scope, args, kwargs, insert_arg)?;
      (arg_vars, invalid_arg_ix, any_args_valid)
    } else {
      let mut arg_vars = FxHashMap::default();
      let insert_arg = |name: Sym, val: Value| {
        arg_vars.insert(name, val);
      };

      let (invalid_arg_ix, any_args_valid) =
        self.eval_closure_args(closure, &captured_scope, args, kwargs, insert_arg)?;
      (arg_vars, invalid_arg_ix, any_args_valid)
    };

    if let Some(invalid_arg_ix) = invalid_arg_ix {
      let invalid_arg = &closure.params[invalid_arg_ix];
      if any_args_valid {
        {
          let mut arg_placeholders = closure.arg_placeholder_scope.borrow_mut();
          *arg_placeholders = Some(arg_vars);
        }

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
          invalid_arg.ident.debug(self)
        )));
      }
    }

    let args_scope = Rc::new(Scope {
      vars: RefCell::new(arg_vars),
      parent: Some(captured_scope),
    });
    let closure_scope = Scope::wrap(args_scope);

    let mut out: Value = Value::Nil;
    for stmt in &closure.body.0 {
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
          ..
        } => {
          let (line, col) = self.resolve_loc(expr.loc());
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
          self
            .eval_assignment(*name, val, &closure_scope, *type_hint)
            .map_err(|err| err.with_loc(line, col))?;
        }
        Statement::DestructureAssignment { lhs, rhs } => {
          let (line, col) = self.resolve_loc(rhs.loc());
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
            .map_err(|err| {
              err
                .wrap("Error evaluating destructure assignment")
                .with_loc(line, col)
            })?;
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

    if has_arg_placeholders {
      let Scope { parent, vars } = closure_scope;
      drop(vars);
      let args_scope = parent.unwrap();

      match Rc::try_unwrap(args_scope) {
        Ok(args_scope) => {
          let vars = args_scope.vars.into_inner();
          let mut arg_placeholders = closure.arg_placeholder_scope.borrow_mut();
          *arg_placeholders = Some(vars);
        }
        Err(_) => {
          // it's likely that this closure has returned a new closure that wraps the arg scope.
          //
          // So, we can't get those placeholders back and have to give up on the optimization.
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
          let entry = &fn_sigs().entries[*fn_entry_ix];
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
          fn_sigs().entries[*fn_entry_ix].0
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
          combined_kwargs.insert(*key, value.clone());
        }

        self.invoke_callable(&paf.inner, &combined_args, &combined_kwargs)
      }
      Callable::Closure(closure) => self.invoke_closure(closure, args, kwargs),
      Callable::ComposedFn(ComposedFn { inner }) => {
        let acc = args;
        let mut iter = inner.iter();
        let mut acc = self.invoke_callable(iter.next().unwrap(), acc, EMPTY_KWARGS)?;
        for callable in iter {
          acc = self.invoke_callable(callable, &[acc], EMPTY_KWARGS)?;
        }

        Ok(acc)
      }
      Callable::Dynamic { name, inner } => inner
        .invoke(args, kwargs, self)
        .map_err(|err| err.wrap(format!("Error invoking dynamic callable `{name}`"))),
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
          _ => Err(ErrorStack::new("invalid swizzle; expected 1 or 2 chars")),
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
          _ => Err(ErrorStack::new("invalid swizzle; expected 1 to 3 chars")),
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
      Value::String(s) => self.eval_static_field_access(lhs, s),
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

  pub fn with_resolved_sym<F, R>(&self, sym: Sym, f: F) -> R
  where
    F: FnOnce(&str) -> R,
  {
    self.interned_symbols.with_resolved(sym, f).unwrap()
  }

  /// Returns the base scope for a fresh module evaluation: a clone of the ambient
  /// scope if one is installed, otherwise the default `pi`/`tau`-only globals.
  pub fn fresh_module_scope(&self) -> Scope {
    match self.ambient_scope.borrow().as_ref() {
      Some(scope) => (**scope).clone(),
      None => Scope::default_globals(&self.interned_symbols),
    }
  }

  pub fn set_ambient_scope(&self, scope: Scope) {
    *self.ambient_scope.borrow_mut() = Some(Rc::new(scope));
  }

  pub fn clear_ambient_scope(&self) {
    *self.ambient_scope.borrow_mut() = None;
  }

  pub fn invalidate_module_cache(&self) {
    self.module_exports.borrow_mut().clear();
    self.module_exports_lru.borrow_mut().clear();
  }

  pub fn compute_source_hash(src: &str) -> u64 {
    use std::hash::Hasher;
    let mut hasher = FxHasher64::default();
    hasher.write(src.as_bytes());
    hasher.finish()
  }

  pub fn next_render_id(&self) -> u32 {
    let id = self.next_render_id.get();
    self.next_render_id.set(id.wrapping_add(1));
    id
  }

  /// A `curve_angle_degrees` arg: the explicit value if given, else the runtime default
  /// (`set_curve_angle_threshold`, seeded by the prelude).
  pub fn resolve_curve_angle_degrees(&self, v: &Value) -> f32 {
    v.as_float()
      .unwrap_or_else(|| *self.default_curve_angle_degrees.borrow())
  }

  /// Content hash of the injected value a module would see for `handle_id`, used to
  /// validate gizmo-read cache entries. A missing value hashes to a stable sentinel
  /// (the call fell back to its source default, which `source_hash` already covers).
  pub fn gizmo_value_hash(&self, module_name: &str, handle_id: &str) -> u64 {
    use std::hash::Hasher;
    let mut hasher = FxHasher64::default();
    let vals = self.gizmo_values.borrow();
    match vals.get(module_name).and_then(|m| m.get(handle_id)) {
      Some(Value::Vec3(v)) => {
        hasher.write_u8(1);
        for c in [v.x, v.y, v.z] {
          hasher.write_u32(c.to_bits());
        }
      }
      Some(Value::Mat4(m)) => {
        hasher.write_u8(2);
        for c in m.as_slice() {
          hasher.write_u32(c.to_bits());
        }
      }
      _ => hasher.write_u8(0),
    }
    hasher.finish()
  }

  fn touch_module_lru(&self, name: &str) {
    let mut lru = self.module_exports_lru.borrow_mut();
    if let Some(pos) = lru.iter().position(|n| n == name) {
      lru.remove(pos);
    }
    lru.push_back(name.to_owned());
  }

  fn evict_module(&self, name: &str) {
    self.module_exports.borrow_mut().remove(name);
    let mut lru = self.module_exports_lru.borrow_mut();
    if let Some(pos) = lru.iter().position(|n| n == name) {
      lru.remove(pos);
    }
  }

  fn enforce_module_cache_cap(&self) {
    let mut exports = self.module_exports.borrow_mut();
    let mut lru = self.module_exports_lru.borrow_mut();
    let replayed = self.replayed_this_run.borrow();
    // Evicting an already-replayed module mid-run would force a re-eval on the
    // next import of it, double-pushing its side effects.
    let mut scanned = 0;
    while exports.len() > MODULE_CACHE_MAX_ENTRIES && scanned < lru.len() {
      let Some(oldest) = lru.pop_front() else {
        break;
      };
      if replayed.contains(&oldest) {
        lru.push_back(oldest);
        scanned += 1;
        continue;
      }
      exports.remove(&oldest);
    }
  }

  /// Evaluate `source` as a module body and return the resulting scope. The base scope
  /// is taken from `fresh_module_scope()` (so this composes: callers can stack evaluations
  /// by installing the previous result as the ambient scope before the next call). Exports
  /// declared in the source are assigned into the scope as bindings; the export tracking
  /// map is not used here.
  pub fn evaluate_module_to_scope(&self, source: &str) -> Result<Scope, ErrorStack> {
    let scope = self.fresh_module_scope();

    // Module sources do not include the prelude, so locations parsed here should not
    // be offset by the root program's prelude line count.
    let prev_offset = self.source_map.borrow().prelude_line_count;
    self.source_map.borrow_mut().prelude_line_count = 0;
    let parse_res = parse_program_src(self, source);
    self.source_map.borrow_mut().prelude_line_count = prev_offset;

    let mut ast = parse_res?;
    optimizer::optimize_ast(self, &mut ast)?;

    for statement in &ast.statements {
      match self.eval_top_level_statement(statement, &scope)? {
        ControlFlow::Continue(_) => {}
        ControlFlow::Break(_) => {
          return Err(ErrorStack::new("`break` at module top level not allowed"));
        }
        ControlFlow::Return(_) => {
          return Err(ErrorStack::new("`return` at module top level not allowed"));
        }
      }
    }

    Ok(scope)
  }

  fn resolve_module(&self, module_name: &str) -> Result<Rc<FxHashMap<String, Value>>, ErrorStack> {
    // Already replayed this run — skip side effects, just return exports.
    if self.replayed_this_run.borrow().contains(module_name) {
      if let Some(entry) = self.module_exports.borrow().get(module_name) {
        let source_hash = entry.source_hash;
        let exports = Rc::clone(&entry.exports);
        if let Some(tracker) = self.current_module_imports.borrow_mut().as_mut() {
          tracker.push((module_name.to_owned(), source_hash));
        }
        return Ok(exports);
      }
    }

    let cache_entry = self.module_exports.borrow().get(module_name).cloned();
    if let Some(entry) = cache_entry {
      // Walk direct deps before validating: recursion reaches each transitive
      // dep through its own `resolve_module`, so it either replays via cache hit
      // (advancing RNG to its own end-state), no-ops if already replayed, or
      // re-evals. After this loop the deps' side effects have fired exactly once
      // and RNG reflects exactly the path the cached body originally saw.
      let mut stale = false;
      for (dep_name, expected_hash) in &entry.direct_imports {
        self.resolve_module(dep_name)?;
        let actual = self
          .module_exports
          .borrow()
          .get(dep_name)
          .map(|e| e.source_hash);
        if actual != Some(*expected_hash) {
          stale = true;
          break;
        }
      }

      // RNG-free bodies cache-hit unconditionally; RNG-using ones require the
      // current state to match what our body originally observed.
      let rng_ok = entry.rng_state_at_start == entry.rng_state_at_end
        || entry.rng_state_at_start == self.rng_state();

      // Each read handle's injected value must still hash the same, else the body
      // would see a different gizmo value and must re-eval.
      let gizmos_ok = entry
        .gizmo_reads
        .iter()
        .all(|(h, hash)| self.gizmo_value_hash(module_name, h) == *hash);

      if !stale && rng_ok && gizmos_ok {
        for mesh in &entry.own_renders {
          self.rendered_meshes.push(mesh.clone());
        }
        for light in &entry.own_lights {
          self.rendered_lights.push(light.clone());
        }
        for path in &entry.own_paths {
          self.rendered_paths.push(path.clone());
        }
        for gizmo in &entry.own_gizmos {
          self.rendered_gizmos.push(gizmo.clone());
        }
        self.set_rng_state(entry.rng_state_at_end.clone());
        #[cfg(target_arch = "wasm32")]
        or_async_dep_bit(entry.own_async_deps_bitmask);

        self
          .replayed_this_run
          .borrow_mut()
          .insert(module_name.to_owned());
        if let Some(tracker) = self.current_module_imports.borrow_mut().as_mut() {
          tracker.push((module_name.to_owned(), entry.source_hash));
        }
        self.touch_module_lru(module_name);
        return Ok(Rc::clone(&entry.exports));
      }

      self.evict_module(module_name);
    }

    // Circular-import guard. Without this, importing a module that (transitively)
    // imports itself would recurse until the stack overflowed.
    if self.modules_in_flight.borrow().contains(module_name) {
      return Err(ErrorStack::new(format!(
        "Circular module import detected: module \"{module_name}\" is already being evaluated"
      )));
    }

    // Get source code
    let source = self
      .module_sources
      .borrow()
      .get(module_name)
      .cloned()
      .ok_or_else(|| ErrorStack::new(format!("Unknown module \"{module_name}\"")))?;

    self
      .modules_in_flight
      .borrow_mut()
      .insert(module_name.to_owned());

    let result = self.resolve_module_inner(module_name, &source);

    self.modules_in_flight.borrow_mut().remove(module_name);

    if let Ok(_exports) = &result {
      let entry_hash = self
        .module_exports
        .borrow()
        .get(module_name)
        .map(|e| e.source_hash);
      if let Some(hash) = entry_hash {
        if let Some(tracker) = self.current_module_imports.borrow_mut().as_mut() {
          tracker.push((module_name.to_owned(), hash));
        }
      }
      self
        .replayed_this_run
        .borrow_mut()
        .insert(module_name.to_owned());
    }

    result
  }

  fn resolve_module_inner(
    &self,
    module_name: &str,
    source: &str,
  ) -> Result<Rc<FxHashMap<String, Value>>, ErrorStack> {
    // RAII guard: swaps in module context, restores on drop so every early
    // return below is correct by construction.
    struct ModuleCtxGuard<'a> {
      ctx: &'a EvalCtx,
      prev_exports: Option<FxHashMap<Sym, Value>>,
      prev_module: Option<String>,
      prev_imports: Option<Vec<(String, u64)>>,
      prev_gizmo_reads: Option<FxHashSet<String>>,
      prev_unnamed_gizmo_count: u32,
    }
    impl<'a> Drop for ModuleCtxGuard<'a> {
      fn drop(&mut self) {
        *self.ctx.current_module_exports.borrow_mut() = self.prev_exports.take();
        *self.ctx.current_module.borrow_mut() = self.prev_module.take();
        *self.ctx.current_module_imports.borrow_mut() = self.prev_imports.take();
        *self.ctx.current_module_gizmo_reads.borrow_mut() = self.prev_gizmo_reads.take();
        self
          .ctx
          .current_module_unnamed_gizmo_count
          .set(self.prev_unnamed_gizmo_count);
      }
    }
    let _guard = ModuleCtxGuard {
      ctx: self,
      prev_exports: self
        .current_module_exports
        .borrow_mut()
        .replace(FxHashMap::default()),
      prev_module: self
        .current_module
        .borrow_mut()
        .replace(module_name.to_owned()),
      prev_imports: self.current_module_imports.borrow_mut().replace(Vec::new()),
      prev_gizmo_reads: self
        .current_module_gizmo_reads
        .borrow_mut()
        .replace(FxHashSet::default()),
      prev_unnamed_gizmo_count: self.current_module_unnamed_gizmo_count.replace(0),
    };

    let rng_at_start = self.rng_state();

    // Module sources do not include the prelude; suppress any inherited offset around the parse.
    let prev_offset = self.source_map.borrow().prelude_line_count;
    self.source_map.borrow_mut().prelude_line_count = 0;
    let parse_res = parse_program_src(self, source);
    self.source_map.borrow_mut().prelude_line_count = prev_offset;

    let module_scope = self.fresh_module_scope();
    let mut ast = parse_res.map_err(|err| err.wrap(&format!("Error parsing module \"{module_name}\"")))?;
    optimizer::optimize_ast(self, &mut ast)
      .map_err(|err| err.wrap(&format!("Error optimizing module \"{module_name}\"")))?;

    // Snapshot side-effect buffers; diffs become the cache entry's replay set.
    let renders_before = self.rendered_meshes.len();
    let lights_before = self.rendered_lights.len();
    let paths_before = self.rendered_paths.len();
    let gizmos_before = self.rendered_gizmos.len();
    #[cfg(target_arch = "wasm32")]
    let async_before = get_async_dep_bits();

    for statement in &ast.statements {
      match self
        .eval_top_level_statement(statement, &module_scope)
        .map_err(|err| err.wrap(&format!("Error evaluating module \"{module_name}\"")))?
      {
        ControlFlow::Continue(_) => {}
        ControlFlow::Break(_) => {
          return Err(ErrorStack::new(format!(
            "`break` in module \"{module_name}\" top level"
          )));
        }
        ControlFlow::Return(_) => {
          return Err(ErrorStack::new(format!(
            "`return` in module \"{module_name}\" top level"
          )));
        }
      }
    }

    let export_map = self
      .current_module_exports
      .borrow_mut()
      .take()
      .unwrap_or_default();

    // Convert Sym keys to String keys for the Value::Map representation
    let string_map: FxHashMap<String, Value> = export_map
      .into_iter()
      .map(|(sym, val)| {
        let name = self.with_resolved_sym(sym, |s| s.to_owned());
        (name, val)
      })
      .collect();
    let exports = Rc::new(string_map);

    // Recursive imports push their renders tagged with their own module name;
    // filtering on `source_module` cleanly separates our work from theirs.
    let module_name_str = module_name.to_owned();
    let is_own = |sm: &Option<String>| sm.as_deref() == Some(module_name_str.as_str());
    let own_renders: Vec<RenderedMesh> = self
      .rendered_meshes
      .inner
      .borrow()
      .get(renders_before..)
      .map(|s| s.iter().filter(|m| is_own(&m.source_module)).cloned().collect())
      .unwrap_or_default();
    let own_lights: Vec<RenderedLight> = self
      .rendered_lights
      .inner
      .borrow()
      .get(lights_before..)
      .map(|s| s.iter().filter(|l| is_own(&l.source_module)).cloned().collect())
      .unwrap_or_default();
    let own_paths: Vec<RenderedPath> = self
      .rendered_paths
      .inner
      .borrow()
      .get(paths_before..)
      .map(|s| s.iter().filter(|p| is_own(&p.source_module)).cloned().collect())
      .unwrap_or_default();
    let own_gizmos: Vec<RenderedGizmo> = self
      .rendered_gizmos
      .inner
      .borrow()
      .get(gizmos_before..)
      .map(|s| s.iter().filter(|g| is_own(&g.source_module)).cloned().collect())
      .unwrap_or_default();
    let gizmo_reads: Vec<(String, u64)> = self
      .current_module_gizmo_reads
      .borrow_mut()
      .take()
      .unwrap_or_default()
      .into_iter()
      .map(|h| {
        let hash = self.gizmo_value_hash(module_name, &h);
        (h, hash)
      })
      .collect();
    let rng_at_end = self.rng_state();
    #[cfg(target_arch = "wasm32")]
    let own_async_deps_bitmask = get_async_dep_bits() & !async_before;
    #[cfg(not(target_arch = "wasm32"))]
    let own_async_deps_bitmask: u32 = 0;

    let direct_imports = self
      .current_module_imports
      .borrow_mut()
      .take()
      .unwrap_or_default();

    let source_hash = self
      .module_source_hashes
      .borrow()
      .get(module_name)
      .copied()
      .unwrap_or_else(|| Self::compute_source_hash(source));

    let entry = Rc::new(ModuleExportsCacheEntry {
      source_hash,
      exports: Rc::clone(&exports),
      own_renders,
      own_lights,
      own_paths,
      own_gizmos,
      rng_state_at_start: rng_at_start,
      rng_state_at_end: rng_at_end,
      direct_imports,
      gizmo_reads,
      own_async_deps_bitmask,
    });

    self
      .module_exports
      .borrow_mut()
      .insert(module_name.to_owned(), entry);
    self.touch_module_lru(module_name);
    self.enforce_module_cache_cap();

    Ok(exports)
  }

  #[cold]
  fn desymbolicate_kwargs(&self, kwargs: &FxHashMap<Sym, Value>) -> FxHashMap<String, Value> {
    kwargs
      .iter()
      .map(|(k, v)| (self.with_resolved_sym(*k, |s| s.to_owned()), v.clone()))
      .collect()
  }

  #[cfg(test)]
  fn get_global(&self, arg: &str) -> Option<Value> {
    let sym = self.interned_symbols.intern(arg);
    self.globals.get(sym)
  }
}

pub fn parse_program_src<'a>(ctx: &EvalCtx, src: &'a str) -> Result<Program, ErrorStack> {
  maybe_init_op_def_shorthands();

  // Edits are stashed on the source map so locations Pest reports in rewritten coords
  // translate back to the original source.
  let preprocessed = preprocess::preprocess(src).map_err(|err| {
    ErrorStack::new(format!("Preprocessor error: {}", err.message)).with_loc(err.line, err.col)
  })?;
  ctx.source_map.borrow_mut().edits = preprocessed.edits.clone();

  let pairs = GSParser::parse(Rule::program, &preprocessed.rewritten)
    .map_err(|err| ErrorStack::new(format!("{err}")).wrap("Syntax error"))?;

  let program = pairs
    .into_iter()
    .next()
    .ok_or_else(|| ErrorStack::new("No program found in input"))?;

  finalize_program(ctx, &preprocessed.rewritten, program, &preprocessed)
}

fn finalize_program(
  ctx: &EvalCtx,
  src: &str,
  program: pest::iterators::Pair<Rule>,
  preprocessed: &preprocess::Preprocessed,
) -> Result<Program, ErrorStack> {
  if program.as_rule() != Rule::program {
    return Err(ErrorStack::new(format!(
      "`parse_program` can only handle `program` rules, found: {:?}",
      program.as_rule()
    )));
  }

  let stmt_pairs: Vec<_> = program.into_inner().collect();
  check_adjacent_tight_array_literal(src, &stmt_pairs, preprocessed)?;

  let statements = stmt_pairs
    .into_iter()
    .filter_map(|stmt| match parse_top_level_statement(ctx, stmt) {
      Ok(Some(statement)) => Some(Ok(statement)),
      Ok(None) => None,
      Err(err) => Some(Err(err.wrap("Error parsing statement"))),
    })
    .collect::<Result<Vec<_>, ErrorStack>>()?;

  Ok(Program { statements })
}

/// Reject `arr[1,2,3]`: Pest would otherwise backtrack to two adjacent statements
/// (`arr`, then `[1,2,3]`), which the Lezer parser rejects. Aligns the two parsers.
fn check_adjacent_tight_array_literal(
  src: &str,
  stmts: &[pest::iterators::Pair<Rule>],
  preprocessed: &preprocess::Preprocessed,
) -> Result<(), ErrorStack> {
  let bytes = src.as_bytes();
  for window in stmts.windows(2) {
    let prev_end = window[0].as_span().end();
    let next_start = window[1].as_span().start();
    if prev_end > next_start {
      continue;
    }
    if next_start >= bytes.len() || bytes[next_start] != b'[' {
      continue;
    }
    let between = &src[prev_end..next_start];
    if between.bytes().any(|b| b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' || b == b';') {
      continue;
    }
    if between.contains("//") {
      continue;
    }
    let prev_last_byte = if prev_end == 0 { None } else { Some(bytes[prev_end - 1]) };
    let is_expr_ender = matches!(
      prev_last_byte,
      Some(b) if b.is_ascii_alphanumeric() || b == b'_' || b == b')' || b == b']' || b == b'}',
    );
    if !is_expr_ender {
      continue;
    }
    let (line, col) = byte_to_line_col(src, next_start);
    let (orig_line, orig_col) = preprocessed.rewritten_line_col_to_original(line, col);
    return Err(
      ErrorStack::new(
        "`[…]` after an expression must be tight and contain a single value; saw multiple \
         comma-separated values. To write an array literal instead, separate the statements \
         with `;` or a newline.",
      )
      .with_loc(orig_line, orig_col),
    );
  }
  Ok(())
}

fn byte_to_line_col(src: &str, byte: usize) -> (u32, u32) {
  let byte = byte.min(src.len());
  let mut line = 1u32;
  let mut line_start = 0usize;
  for (i, &b) in src.as_bytes()[..byte].iter().enumerate() {
    if b == b'\n' {
      line += 1;
      line_start = i + 1;
    }
  }
  (line, (byte - line_start + 1) as u32)
}

pub fn parse_program_maybe_with_prelude(
  ctx: &EvalCtx,
  src: String,
  include_prelude: bool,
) -> Result<Program, ErrorStack> {
  parse_program_maybe_with_prelude_and_ambient(ctx, src, include_prelude, "")
}

/// Like `parse_program_maybe_with_prelude`, but also prepends an `ambient_src` block (e.g. a
/// Geotoy `_globals` node) after the prelude so its definitions are in scope. The combined
/// prepended-line count is folded into `prelude_line_count`, so locations in the prelude/ambient
/// regions collapse to line 0 (and diagnostics there are dropped) while user-source lines map back.
pub fn parse_program_maybe_with_prelude_and_ambient(
  ctx: &EvalCtx,
  src: String,
  include_prelude: bool,
  ambient_src: &str,
) -> Result<Program, ErrorStack> {
  let mut prefix = String::new();
  if include_prelude {
    prefix.push_str(PRELUDE);
    prefix.push('\n');
  }
  if !ambient_src.is_empty() {
    prefix.push_str(ambient_src);
    prefix.push('\n');
  }
  // The user source begins right after the prefix's last newline, so the prefix's newline
  // count is exactly the line offset (robust whether or not each block ends in a newline).
  let offset = prefix.matches('\n').count() as u32;
  ctx.source_map.borrow_mut().prelude_line_count = offset;
  let full = if prefix.is_empty() {
    src
  } else {
    prefix.push_str(&src);
    prefix
  };
  parse_program_src(ctx, &full)
}

pub fn eval_program_with_ctx(ctx: &EvalCtx, ast: &Program) -> Result<(), ErrorStack> {
  // When an ambient scope is installed, the root program (treated as a module for
  // hierarchy purposes) gets a fresh clone of it as its base scope, rather than the
  // long-lived `ctx.globals` that accumulates user vars across calls.
  let ambient_scope_owned;
  let scope: &Scope = if ctx.ambient_scope.borrow().is_some() {
    ambient_scope_owned = ctx.fresh_module_scope();
    &ambient_scope_owned
  } else {
    &ctx.globals
  };

  for statement in &ast.statements {
    let _val = match ctx.eval_top_level_statement(statement, scope)? {
      ControlFlow::Continue(val) => val,
      ControlFlow::Break(_) => {
        return Err(ErrorStack::new(
          "`break` outside of a function is not allowed",
        ))
      }
      ControlFlow::Return(_) => {
        return Err(ErrorStack::new(
          "`return` outside of a function is not allowed",
        ));
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
fn test_error_loc_basic_binop() {
  let src = r#"fn = |x| {
  x + "hello"
}
fn(1)
"#;
  let err = parse_and_eval_program(src).unwrap_err();
  assert_eq!(err.loc, Some((2, 3)));
  assert!(format!("{err}").contains("at line 2, column 3"));
}

#[test]
fn test_shorthand_closure_trailing_comment_parses() {
  // Regression: the inserted body-closing `}` used to land inside the trailing line comment.
  let src = "print(\n  || 1 // comment\n)";
  assert!(parse_and_eval_program(src).is_ok());
}

#[test]
fn test_error_loc_nested_closure() {
  let src = r#"outer = || {
  inner = || {
    inner2 = |x| {
      1 + x
    }
    inner2("nope")
  }
  inner()
}
outer()
"#;
  let err = parse_and_eval_program(src).unwrap_err();
  assert_eq!(err.loc, Some((4, 7)));
}

#[test]
fn test_source_locs_survive_subsequent_parse_with_different_edits() {
  // Locs minted by parse A must not be re-translated through parse B's edits.
  let ctx = EvalCtx::default();

  let src1 = "f = |x| x + 1\n";
  let ast1 = parse_program_src(&ctx, src1).unwrap();
  let f_name_loc = match &ast1.statements[0] {
    TopLevelStatement::Statement(Statement::Assignment { name_loc, .. }) => *name_loc,
    _ => panic!("expected an Assignment"),
  };
  assert_eq!(ctx.resolve_loc(f_name_loc), (1, 1));

  let src2 = "y = 0\n[1, 2, 3]\n";
  let _ast2 = parse_program_src(&ctx, src2).unwrap();

  assert_eq!(ctx.resolve_loc(f_name_loc), (1, 1));
}

#[test]
fn test_const_eval_cache_preserves_loc_across_runs() {
  let ctx = EvalCtx::default();

  let src1 = r#"a = [1, 2]"#;
  let mut ast1 = parse_program_src(&ctx, src1).unwrap();
  optimize_ast(&ctx, &mut ast1).unwrap();
  let (seq1, loc1) = match &ast1.statements[0] {
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => match expr {
      Expr::Literal {
        value: Value::Sequence(seq),
        loc,
      } => (Rc::clone(seq), *loc),
      _ => panic!("Expected const folding to produce a sequence literal"),
    },
    _ => unreachable!(),
  };
  assert_eq!(ctx.resolve_loc(loc1), (1, 5));

  let src2 = r#"
x = 1
b = [1, 2]"#;
  let mut ast2 = parse_program_src(&ctx, src2).unwrap();
  optimize_ast(&ctx, &mut ast2).unwrap();
  let (seq2, loc2) = match &ast2.statements[1] {
    TopLevelStatement::Statement(Statement::Assignment { expr, .. }) => match expr {
      Expr::Literal {
        value: Value::Sequence(seq),
        loc,
      } => (Rc::clone(seq), *loc),
      _ => panic!("Expected const folding to produce a sequence literal"),
    },
    _ => unreachable!(),
  };
  assert!(Rc::ptr_eq(&seq1, &seq2));
  assert_eq!(ctx.resolve_loc(loc2), (3, 5));
}

/// `path_render` pushes to `rendered_paths` as a side effect, so it must not be const-folded.
/// Folding bakes the effect away after the first run (the repl reuses the ctx + const-eval
/// cache while clearing rendered paths each run), making the path vanish on re-runs.
#[test]
fn test_render_path_survives_rerun() {
  let ctx = EvalCtx::default();
  let src = "build_path(path { rect(v2(10), v2(10)) }) | path_render";
  let mut ast = parse_program_src(&ctx, src).unwrap();

  for run in 1..=2 {
    ctx.rendered_paths.inner.borrow_mut().clear();
    optimize_ast(&ctx, &mut ast).unwrap();
    eval_program_with_ctx(&ctx, &ast).unwrap();
    assert_eq!(
      ctx.rendered_paths.len(),
      1,
      "expected a rendered path on run {run}"
    );
  }
}

/// `set_default_material` mutates `ctx.default_material`, so it must not be const-folded — folding
/// applies the effect once at optimize time and skips it on every later eval of the same program.
#[test]
fn test_set_default_material_survives_rerun() {
  let mut ctx = EvalCtx::default();
  ctx
    .materials
    .insert("mat".to_owned(), Rc::new(Material::External("mat".to_owned())));
  let mut ast = parse_program_src(&ctx, r#"set_default_material("mat")"#).unwrap();

  for run in 1..=2 {
    ctx.default_material.replace(None);
    optimize_ast(&ctx, &mut ast).unwrap();
    eval_program_with_ctx(&ctx, &ast).unwrap();
    assert!(
      ctx.default_material.borrow().is_some(),
      "expected default material to be set on run {run}"
    );
  }
}

/// Pest/Lezer parser parity cases. Mirror in `src/geoscript/parser/parser.test.ts`;
/// keep the two in sync by hand. `Ok(n)` = `n` statements; `Err(needle)` = error
/// message contains `needle`.
#[cfg(test)]
const PARSER_PARITY_CASES: &[(&str, ParseOutcome)] = &[
  ("1", ParseOutcome::Ok(1)),
  ("1 + 2", ParseOutcome::Ok(1)),
  ("a = 1", ParseOutcome::Ok(1)),
  ("f(1, 2)", ParseOutcome::Ok(1)),
  // Tight `[` field access
  ("arr[0]", ParseOutcome::Ok(1)),
  ("arr[0][1]", ParseOutcome::Ok(1)),
  ("arr[0].field", ParseOutcome::Ok(1)),
  ("[1,2,3][0]", ParseOutcome::Ok(1)),
  ("{ 1 }[0]", ParseOutcome::Ok(1)),
  ("f(1)[0]", ParseOutcome::Ok(1)),
  // `.field` is whitespace-permissive
  ("arr.field", ParseOutcome::Ok(1)),
  ("arr .field", ParseOutcome::Ok(1)),
  ("arr\n  .field", ParseOutcome::Ok(1)),
  ("arr.a.b.c", ParseOutcome::Ok(1)),
  ("arr [0]", ParseOutcome::Err("no whitespace before `[`")),
  ("{ 1 } [0]", ParseOutcome::Err("no whitespace before `[`")),
  ("arr[1,2,3]", ParseOutcome::Err("must be tight and contain a single value")),
  // `\n[`/`\n(` — preprocessor splits into two statements.
  ("arr\n[0]", ParseOutcome::Ok(2)),
  ("arr\n[1,2,3]", ParseOutcome::Ok(2)),
  ("foo\n(x)", ParseOutcome::Ok(2)),
  ("{ 1 }\n[0]", ParseOutcome::Ok(2)),
  ("{ 1 }\n[1, 2, 3]", ParseOutcome::Ok(2)),
  ("x = { 1 }\n[1, 2, 3]", ParseOutcome::Ok(2)),
  ("[1,2,3]\n[4,5,6]", ParseOutcome::Ok(2)),
  ("foo()", ParseOutcome::Ok(1)),
  ("foo ()", ParseOutcome::Err("")),
  // Shorthand closures (preprocessor wraps body in `{}`)
  ("|| 1", ParseOutcome::Ok(1)),
  ("|x| x", ParseOutcome::Ok(1)),
  ("|x| x + 1", ParseOutcome::Ok(1)),
  ("|x| x | 1", ParseOutcome::Ok(1)),
  ("|x| x || y", ParseOutcome::Ok(1)),
  ("|x| |y| x + y", ParseOutcome::Ok(1)),
  ("|x = 1| x", ParseOutcome::Ok(1)),
  ("|x: int| x", ParseOutcome::Ok(1)),
  ("foo(x=|a| a + 1, b=2)", ParseOutcome::Ok(1)),
  ("foo(|a| a, |b| b)", ParseOutcome::Ok(1)),
  ("[|x| x + 1, |y| y * 2]", ParseOutcome::Ok(1)),
  ("{key: |x| x + 1}", ParseOutcome::Ok(1)),
  ("x = |a| a + 1\ny = 2", ParseOutcome::Ok(2)),
  ("a || b", ParseOutcome::Ok(1)),
  ("a | b", ParseOutcome::Ok(1)),
  ("x = ||\n 1", ParseOutcome::Err("empty body")),
  // `from` is contextual: a valid identifier/kwarg name except inside an import.
  ("align(from=1, to=2)", ParseOutcome::Ok(1)),
  ("from = 5", ParseOutcome::Ok(1)),
];

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
enum ParseOutcome {
  Ok(usize),
  Err(&'static str),
}

#[test]
fn test_parser_parity_pest() {
  let ctx = EvalCtx::default();
  let mut failures = Vec::new();
  for (src, expected) in PARSER_PARITY_CASES {
    let actual = parse_program_src(&ctx, src);
    let outcome_matches = match (&actual, expected) {
      (Ok(prog), ParseOutcome::Ok(n)) => prog.statements.len() == *n,
      (Err(e), ParseOutcome::Err(needle)) => format!("{e}").contains(needle),
      _ => false,
    };
    if !outcome_matches {
      let actual_desc = match actual {
        Ok(p) => format!("Ok({} statements)", p.statements.len()),
        Err(e) => format!("Err({})", format!("{e}").lines().next().unwrap_or("")),
      };
      failures.push(format!("  {src:?} — expected {expected:?}, got {actual_desc}"));
    }
  }
  assert!(
    failures.is_empty(),
    "Pest parser parity mismatches:\n{}",
    failures.join("\n")
  );
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
  let mesh = &rendered_meshes[0].mesh;
  assert_eq!(mesh.mesh.vertices.len(), 8);
  for vtx in mesh.mesh.vertices.values() {
    let pos = (mesh.transform * vtx.position.push(1.)).xyz();
    assert_eq!(pos.x.abs(), 2.0);
    assert_eq!(pos.y.abs(), 4.0);
    assert_eq!(pos.z.abs(), 2.0);
  }
}

#[test]
fn test_skewer_cube() {
  let src = r#"
b = box(1, 1, 1) | skewer(vec3(0.2, 0, 0.2), vec3(0, 1, 0))
b | render
"#;
  let result = parse_and_eval_program(src);
  assert!(result.is_ok(), "Failed to evaluate: {:?}", result.err());
  let rendered_meshes = result.unwrap().rendered_meshes.into_inner();
  assert_eq!(rendered_meshes.len(), 1);
  let mesh = &rendered_meshes[0].mesh;
  // 12 base tris + 4 from two interior fan-splits
  assert_eq!(mesh.mesh.faces.len(), 16);
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
  let mesh = &rendered_meshes[0].mesh;
  assert_eq!(mesh.mesh.vertices.len(), 8);
  for vtx in mesh.mesh.vertices.values() {
    let pos = (mesh.transform * vtx.position.push(1.)).xyz();
    assert_eq!(pos.x.abs(), 1. / 2.);
    assert_eq!(pos.y.abs(), 2. / 2.);
    assert_eq!(pos.z.abs(), 3. / 2.);
  }
}

#[test]
fn test_dir_light_shadow_camera_auto() {
  use crate::lights::Light;

  let dir = |src: &str| -> crate::lights::DirectionalLight {
    let lights = parse_and_eval_program(src).unwrap().rendered_lights.into_inner();
    assert_eq!(lights.len(), 1);
    match lights.into_iter().next().unwrap().light {
      Light::Directional(d) => d,
      other => panic!("expected directional light, got {other:?}"),
    }
  };

  assert!(dir(r#"dir_light(shadow_camera="auto") | render"#).shadow_camera.auto);
  assert!(dir(r#"dir_light(shadow_camera=nil) | render"#).shadow_camera.auto);
  assert!(dir(r#"dir_light() | render"#).shadow_camera.auto);

  let explicit = dir(
    r#"dir_light(shadow_camera={near: 1, far: 2, left: -3, right: 4, top: 5, bottom: -6}) | render"#,
  );
  assert!(!explicit.shadow_camera.auto);
  assert_eq!(explicit.shadow_camera.near, 1.);
  assert_eq!(explicit.shadow_camera.far, 2.);
  assert_eq!(explicit.shadow_camera.right, 4.);
}

#[test]
fn test_prelude_dir_light_is_auto() {
  use crate::lights::Light;

  let ctx = EvalCtx::default();
  parse_and_eval_program_with_ctx("render(box(1))".to_owned(), &ctx, true).unwrap();
  let dir = ctx
    .rendered_lights
    .borrow()
    .iter()
    .find_map(|l| match &l.light {
      Light::Directional(d) => Some(d.clone()),
      _ => None,
    })
    .expect("prelude should render a directional light");
  assert!(dir.shadow_camera.auto);
}

#[test]
fn test_partial_application_with_only_kwargs() {
  let ctx = EvalCtx::default();
  let interned_x = ctx.interned_symbols.intern("x");
  let interned_y = ctx.interned_symbols.intern("y");

  static mut ARGS: &mut [ArgDef] = &mut [
    ArgDef {
      name: "x",
      interned_name: Sym(0),
      valid_types: ArgType::Int.as_bitflags(),
      default_value: builtins::fn_defs::DefaultValue::Required,
      description: "",
    },
    ArgDef {
      name: "y",
      interned_name: Sym(0),
      valid_types: ArgType::Int.as_bitflags(),
      default_value: builtins::fn_defs::DefaultValue::Required,
      description: "",
    },
  ];

  unsafe {
    ARGS[0].interned_name = interned_x;
    ARGS[1].interned_name = interned_y;
  }

  let defs = &[FnSignature {
    arg_defs: unsafe { ARGS },
    description: "",
    return_type: &[ArgType::Any],
  }];
  let args = Vec::new();
  let mut kwargs = FxHashMap::default();
  kwargs.insert(interned_y, Value::Int(1));
  let result = get_args(&ctx, "fn_name", defs, &args, &kwargs);
  match result {
    Ok(GetArgsOutput::PartiallyApplied) => {}
    _ => panic!("Expected PartiallyApplied, got {:?}", result),
  }
}

#[test]
fn test_unknown_kwarg_returns_error() {
  let ctx = EvalCtx::default();
  let interned_x = ctx.interned_symbols.intern("x");
  let interned_y = ctx.interned_symbols.intern("y");

  static mut ARGS: &mut [ArgDef] = &mut [
    ArgDef {
      name: "x",
      interned_name: Sym(0),
      valid_types: ArgType::Int.as_bitflags(),
      default_value: builtins::fn_defs::DefaultValue::Required,
      description: "",
    },
    ArgDef {
      name: "y",
      interned_name: Sym(0),
      valid_types: ArgType::Int.as_bitflags(),
      default_value: builtins::fn_defs::DefaultValue::Required,
      description: "",
    },
  ];

  unsafe {
    ARGS[0].interned_name = interned_x;
    ARGS[1].interned_name = interned_y;
  }

  let defs = &[FnSignature {
    arg_defs: unsafe { ARGS },
    description: "",
    return_type: &[ArgType::Any],
  }];

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
fn test_numeric_builtins_on_num_typed_float() {
  // Regression: a `num`-typed value that is a float at runtime must not pre-resolve numeric
  // builtins/operators to their int-only overload (which panics via `as_int().unwrap()`).
  // Covers comparison (`<`), unary neg, and `abs`.
  let src = r#"
f = |h: num| {
  ah = if h < 0 { -h } else { h }
  ah + abs(h)
}
neg = f(-2.5)
pos = f(1.5)
"#;
  let ctx = parse_and_eval_program(src).unwrap();
  assert_eq!(ctx.get_global("neg").unwrap().as_float().unwrap(), 5.0);
  assert_eq!(ctx.get_global("pos").unwrap().as_float().unwrap(), 3.0);
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
fn test_dot_cross_builtins() {
  let src = r#"
d3 = dot(vec3(1,2,3), vec3(4,5,6))
d2 = dot(vec2(1,2), vec2(3,4))
d2_pipe = vec2(1,0) | dot(vec2(0,1))
c = cross(vec3(1,0,0), vec3(0,1,0))
c_pipe = vec3(0,0,1) | cross(vec3(1,0,0))
"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let Value::Float(d3) = ctx.get_global("d3").unwrap() else {
    panic!("expected Float");
  };
  assert_eq!(d3, 32.); // 4 + 10 + 18

  let Value::Float(d2) = ctx.get_global("d2").unwrap() else {
    panic!("expected Float");
  };
  assert_eq!(d2, 11.); // 3 + 8

  let Value::Float(d2_pipe) = ctx.get_global("d2_pipe").unwrap() else {
    panic!("expected Float");
  };
  assert_eq!(d2_pipe, 0.);

  let Value::Vec3(c) = ctx.get_global("c").unwrap() else {
    panic!("expected Vec3");
  };
  assert_eq!(c, Vec3::new(0., 0., 1.));

  // pipe appends the piped value as the final arg, so this is `cross(vec3(1,0,0), vec3(0,0,1))`
  let Value::Vec3(c_pipe) = ctx.get_global("c_pipe").unwrap() else {
    panic!("expected Vec3");
  };
  assert_eq!(c_pipe, Vec3::new(0., -1., 0.));
}

#[test]
fn test_look_at() {
  // -Z forward convention: after look_at, the object's local -Z (negated 3rd transform column)
  // points at the target; position and scale are preserved.
  let src = r#"
m = box(1,1,1) | scale(2) | trans_global(0, 5, 0) | look_at(target=vec3(10, 5, 0))
angles = look_at(vec3(0,0,0), vec3(1, 0, 0))
"#;
  let ctx = parse_and_eval_program(src).unwrap();

  let Value::Mesh(m) = ctx.get_global("m").unwrap() else {
    panic!("expected Mesh");
  };
  let t = &m.transform;
  let forward = -t.column(2).xyz().normalize();
  assert!((forward - Vec3::new(1., 0., 0.)).norm() < 1e-5, "forward={forward:?}");
  assert!((t.column(3).xyz() - Vec3::new(0., 5., 0.)).norm() < 1e-5);
  assert!((t.column(0).norm() - 2.).abs() < 1e-5, "scale preserved");

  // Euler-angle form: feeding the result to `from_euler_angles` and rotating local -Z must
  // reproduce the look direction.
  let Value::Vec3(a) = ctx.get_global("angles").unwrap() else {
    panic!("expected Vec3");
  };
  let r = nalgebra::UnitQuaternion::from_euler_angles(a.x, a.y, a.z);
  let fwd = r * Vec3::new(0., 0., -1.);
  assert!((fwd - Vec3::new(1., 0., 0.)).norm() < 1e-5, "fwd={fwd:?}");
}

#[test]
fn test_align() {
  let src = r#"
// default `from` = -Z, pointed at world +X; piped object with positional `to`
a = box(1,1,1) | scale(3) | trans_global(2, 0, 0) | align(vec3(1, 0, 0))
// custom `from`: point local +Z at world +Y
b = box(1,1,1) | align(vec3(0, 1, 0), from=vec3(0, 0, 1))
// absolute: replaces any prior rotation
c = box(1,1,1) | rot(vec3(0.3, 1.0, -0.7)) | align(vec3(1, 0, 0))
// roll control: stand cube on its (1,1,1) corner, with the (1,-1,1) corner toward +X
d = box() | align(from=v3(1,1,1), to=v3(0,1,0), up_from=v3(1,-1,1), up_to=v3(1,0,0))
"#;
  let ctx = parse_and_eval_program(src).unwrap();

  let Value::Mesh(a) = ctx.get_global("a").unwrap() else {
    panic!("expected Mesh");
  };
  let fwd = -a.transform.column(2).xyz().normalize();
  assert!((fwd - Vec3::new(1., 0., 0.)).norm() < 1e-5, "fwd={fwd:?}");
  assert!((a.transform.column(3).xyz() - Vec3::new(2., 0., 0.)).norm() < 1e-5);
  assert!((a.transform.column(0).norm() - 3.).abs() < 1e-5, "scale preserved");

  let Value::Mesh(b) = ctx.get_global("b").unwrap() else {
    panic!("expected Mesh");
  };
  let local_z = b.transform.column(2).xyz().normalize();
  assert!((local_z - Vec3::new(0., 1., 0.)).norm() < 1e-5, "local_z={local_z:?}");

  let Value::Mesh(c) = ctx.get_global("c").unwrap() else {
    panic!("expected Mesh");
  };
  let fwd_c = -c.transform.column(2).xyz().normalize();
  assert!((fwd_c - Vec3::new(1., 0., 0.)).norm() < 1e-5, "fwd_c={fwd_c:?}");

  let Value::Mesh(d) = ctx.get_global("d").unwrap() else {
    panic!("expected Mesh");
  };
  let basis = d.transform.fixed_view::<3, 3>(0, 0).clone_owned();
  let up_corner = basis * Vec3::new(1., 1., 1.);
  assert!(
    (up_corner.normalize() - Vec3::new(0., 1., 0.)).norm() < 1e-5,
    "up_corner={up_corner:?}"
  );
  // (1,-1,1) corner rolled so its horizontal projection lands on +X (z≈0, x>0)
  let x_corner = basis * Vec3::new(1., -1., 1.);
  assert!(x_corner.z.abs() < 1e-5 && x_corner.x > 0., "x_corner={x_corner:?}");
}

#[test]
fn test_platonic_solids() {
  let src = r#"
oct = octahedron(2)
dia = diamond()
cub = cube(3)
tet = tetrahedron()
ico = icosahedron()
dod = dodecahedron()
bip = bipyramid(6, 2, 3)
all_manifold = is_manifold(oct) and is_manifold(dia) and is_manifold(cub) and is_manifold(tet) and is_manifold(ico) and is_manifold(dod) and is_manifold(bip)
"#;
  let ctx = parse_and_eval_program(src).unwrap();

  let get = |name: &str| match ctx.get_global(name).unwrap() {
    Value::Mesh(m) => m,
    other => panic!("{name} is not a Mesh: {other:?}"),
  };
  let counts = |name: &str| {
    let m = get(name);
    (m.mesh.vertices.len(), m.mesh.faces.len())
  };
  assert_eq!(counts("oct"), (6, 8));
  assert_eq!(counts("dia"), (6, 8)); // diamond == octahedron
  assert_eq!(counts("cub"), (8, 12)); // cube == box
  assert_eq!(counts("tet"), (4, 4));
  assert_eq!(counts("ico"), (12, 20));
  assert_eq!(counts("dod"), (20, 36)); // 12 pentagons, fan-triangulated
  assert_eq!(counts("bip"), (8, 12)); // hexagonal bipyramid: 6 equator + 2 apex

  // every solid is a closed 2-manifold (validates winding + the dodecahedron dual)
  let Value::Bool(true) = ctx.get_global("all_manifold").unwrap() else {
    panic!("a generated solid is not manifold");
  };

  // octahedron: all verts on the circumsphere of radius 2, with a vertex straight up
  let oct = get("oct");
  for v in oct.mesh.vertices.values() {
    assert!((v.position.norm() - 2.).abs() < 1e-4, "vtx off sphere: {:?}", v.position);
  }
  assert!(
    oct.mesh.vertices.values().any(|v| (v.position - Vec3::new(0., 2., 0.)).norm() < 1e-4),
    "octahedron should have a vertex pointing straight up"
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
    let mesh = &rendered_meshes[i].mesh;
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
#[ignore]
fn check_all_repo_geo_files_parse() {
  use std::fs;
  use std::path::PathBuf;

  fn collect(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
      let p = entry.path();
      if p.is_dir() {
        collect(&p, out);
      } else if p.extension().map_or(false, |e| e == "geo") {
        out.push(p);
      }
    }
  }

  let mut files = Vec::new();
  collect(
    std::path::Path::new("/home/casey/dream/src/levels"),
    &mut files,
  );
  collect(
    std::path::Path::new("/home/casey/dream/src/viz/wasm/geoscript/examples"),
    &mut files,
  );
  files.push(PathBuf::from("/home/casey/dream/silo.geo"));

  let ctx = EvalCtx::default();
  let mut failures = Vec::new();
  for f in &files {
    let src = fs::read_to_string(f).unwrap_or_default();
    if src.is_empty() {
      continue;
    }
    if let Err(e) = parse_program_src(&ctx, &src) {
      failures.push(format!(
        "FAIL {}: {}",
        f.display(),
        format!("{e}").lines().next().unwrap_or("")
      ));
    }
  }
  if !failures.is_empty() {
    panic!("Failures:\n{}", failures.join("\n"));
  }
  println!("All {} .geo files parsed successfully", files.len());
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
      // Use `parse_program_src` so the preprocessor runs before Pest.
      let ctx = EvalCtx::default();
      match parse_program_src(&ctx, &src) {
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
  let assignment_rhs = match body.0.first().unwrap() {
    Statement::Assignment { expr, .. } => expr,
    _ => unreachable!(),
  };
  // the const radius should be inlined
  let lhs = match assignment_rhs {
    Expr::BinOp { lhs, .. } => &**lhs,
    _ => unreachable!(),
  };
  assert!(matches!(
    lhs,
    Expr::Literal {
      value: Value::Int(10),
      ..
    }
  ));

  // the `radius` returned at the end should not be inlined since its value depends on the closure
  // argument
  match body.0.last().unwrap() {
    Statement::Expr(expr) => match expr {
      Expr::Ident { name: ident, .. } => {
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
    params: Rc::new(vec![]),
    body: Rc::new(crate::ast::ClosureBody(vec![Statement::Expr(
      Expr::Literal {
        value: Value::Int(1),
        loc: SourceLoc::default(),
      },
    )])),
    arg_placeholder_scope: Closure::build_arg_placeholder_scope(&[]),
    return_type_hint: None,
    loc: SourceLoc::default(),
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

#[test]
fn test_mesh_len_vert_count() {
  let src = r#"
m = box(1)
x = len(m)"#;

  let ctx = parse_and_eval_program(src).unwrap();

  let x = ctx.get_global("x").unwrap();
  let x = x.as_int().expect("Expected result to be an Int");
  assert_eq!(x, 8);
}

#[test]
fn test_render_path_sampler() {
  // A path sampler (which implements PathSampler) should be renderable directly.
  // It should produce a single closed path with 10001 points (10000 samples + closing repeat).
  let src = r#"
p = build_path(path {
  move(0, 0)
  line(1, 0)
  line(1, 1)
  line(0, 1)
})
p | render
"#;

  let ctx = parse_and_eval_program(src).unwrap();
  let paths = ctx.rendered_paths.into_inner();
  assert_eq!(paths.len(), 1);
  let path = &paths[0].points;
  // 10000 samples + 1 closing point
  assert_eq!(path.len(), 10001);
  // First and last point should be equal (closed)
  assert_eq!(path[0], path[path.len() - 1]);
  // All points should be in the XZ plane (y == 0)
  for pt in path {
    assert_eq!(pt.y, 0.0);
  }
}

#[test]
fn test_extrude_path_sampler() {
  // Open L-shaped path swept along +Y → 2 quads = 4 triangles, 8 verts.
  let src = r#"
p = build_path(path {
  move(0, 0)
  line(1, 0)
  line(1, 1)
})
m = extrude_path(p, up=vec3(0, 2, 0))
render(m)
"#;

  let ctx = parse_and_eval_program(src).unwrap();
  let rendered = ctx.rendered_meshes.into_inner();
  assert_eq!(rendered.len(), 1);
  let mesh = &rendered[0].mesh.mesh;
  assert_eq!(mesh.vertices.len(), 6);
  assert_eq!(mesh.faces.len(), 4);
  let max_y = mesh
    .vertices
    .iter()
    .map(|(_, v)| v.position.y)
    .fold(f32::NEG_INFINITY, f32::max);
  assert!((max_y - 2.0).abs() < 1e-5, "expected top at y=2, got {max_y}");
}

#[test]
fn test_extrude_path_blackbox_callable() {
  // Black-box `|t|: vec2` callable — straight line along X from 0 to 1.
  let src = r#"
f = |t| vec2(t, 0)
m = extrude_path(f, up=vec3(0, 1, 0), sample_count=5)
render(m)
"#;

  let ctx = parse_and_eval_program(src).unwrap();
  let rendered = ctx.rendered_meshes.into_inner();
  assert_eq!(rendered.len(), 1);
  let mesh = &rendered[0].mesh.mesh;
  // 5 samples → 5 bottom + 5 top = 10 verts, 4 quads = 8 tris.
  assert_eq!(mesh.vertices.len(), 10);
  assert_eq!(mesh.faces.len(), 8);
}

#[test]
fn test_extrude_path_closed_errors() {
  let src = r#"
p = build_path(path {
  move(0, 0)
  line(1, 0)
  line(1, 1)
  line(0, 1)
}, closed=true)
m = extrude_path(p, up=vec3(0, 1, 0))
render(m)
"#;

  let err = parse_and_eval_program(src).expect_err("expected closed-path error");
  assert!(
    format!("{err:?}").contains("closed"),
    "expected error to mention closed; got: {err:?}"
  );
}

#[test]
fn test_fan_fill_path_sampler() {
  // Closed square path sampler -> fan of 4 triangles around centroid in XZ plane.
  let src = r#"
p = build_path(path {
  move(0, 0)
  line(1, 0)
  line(1, 1)
  line(0, 1)
}, closed=true)
m = fan_fill(p)
render(m)
"#;

  let ctx = parse_and_eval_program(src).unwrap();
  let rendered = ctx.rendered_meshes.into_inner();
  assert_eq!(rendered.len(), 1);
  let mesh = &rendered[0].mesh.mesh;
  // 4 perimeter verts + 1 center, 4 triangles.
  assert_eq!(mesh.vertices.len(), 5);
  assert_eq!(mesh.faces.len(), 4);
  for (_, v) in mesh.vertices.iter() {
    assert_eq!(v.position.y, 0.0);
  }
}

#[test]
fn test_fan_fill_blackbox_callable() {
  // Black-box `|t|: vec2` callable approximating a unit circle, sampled uniformly.
  let src = r#"
f = |t| vec2(cos(t * 2 * pi), sin(t * 2 * pi))
m = fan_fill(f, sample_count=16, center=vec2(0, 0))
render(m)
"#;

  let ctx = parse_and_eval_program(src).unwrap();
  let rendered = ctx.rendered_meshes.into_inner();
  assert_eq!(rendered.len(), 1);
  let mesh = &rendered[0].mesh.mesh;
  // 16 samples close -> last duplicates the first; perimeter + center vertex.
  // closed inferred via p(0) ~= p(1).
  assert!(mesh.vertices.len() >= 16);
  assert!(mesh.faces.len() >= 15);
  for (_, v) in mesh.vertices.iter() {
    assert_eq!(v.position.y, 0.0);
  }
}

#[test]
fn test_render_vec2_sequence() {
  // A sequence of Vec2 should be rendered as a path in the XZ plane.
  let src = r#"
pts = [vec2(0, 0), vec2(1, 0), vec2(1, 1), vec2(0, 1)]
pts | render
"#;

  let ctx = parse_and_eval_program(src).unwrap();
  let paths = ctx.rendered_paths.into_inner();
  assert_eq!(paths.len(), 1);
  let path = &paths[0].points;
  assert_eq!(path.len(), 4);
  // All points should be in the XZ plane (y == 0)
  for pt in path {
    assert_eq!(pt.y, 0.0);
  }
  // Check XZ coords match the input
  assert_eq!(path[0].x, 0.0);
  assert_eq!(path[0].z, 0.0);
  assert_eq!(path[1].x, 1.0);
  assert_eq!(path[1].z, 0.0);
  assert_eq!(path[2].x, 1.0);
  assert_eq!(path[2].z, 1.0);
}

#[test]
fn test_module_basic_export_import() {
  let ctx = EvalCtx::default();

  ctx.module_sources.borrow_mut().insert(
    "shapes".to_string(),
    "export width = 10\nexport height = 20".to_string(),
  );

  let src = r#"
import { width, height } from "shapes"
result = width + height
"#;

  parse_and_eval_program_with_ctx(src.to_string(), &ctx, false).unwrap();
  assert_eq!(ctx.get_global("result").unwrap().as_int().unwrap(), 30);
}

#[test]
fn test_module_import_rename_and_caching() {
  let ctx = EvalCtx::default();

  ctx
    .module_sources
    .borrow_mut()
    .insert("math".to_string(), "export val = 42".to_string());

  // Tests renaming on import and that a second import reuses the cached module
  let src = r#"
import { val: first } from "math"
import { val: second } from "math"
result = first + second
"#;

  parse_and_eval_program_with_ctx(src.to_string(), &ctx, false).unwrap();
  assert_eq!(ctx.get_global("result").unwrap().as_int().unwrap(), 84);
  assert!(ctx.module_exports.borrow().contains_key("math"));
}

#[test]
fn test_random_module_cache_requires_matching_rng_state() {
  let ctx = EvalCtx::default();
  ctx.reset_rng_to_default();

  ctx
    .module_sources
    .borrow_mut()
    .insert("rng".to_string(), "export x = 2".to_string());

  let rng_start = ctx.rng_state();
  let mut rng_end = rng_start.clone();
  rng_end.advance(1);

  let mut stale_exports = FxHashMap::default();
  stale_exports.insert("x".to_string(), Value::Int(1));
  let stale_entry = Rc::new(ModuleExportsCacheEntry {
    source_hash: EvalCtx::compute_source_hash("export x = 2"),
    exports: Rc::new(stale_exports),
    own_renders: Vec::new(),
    own_lights: Vec::new(),
    own_paths: Vec::new(),
    own_gizmos: Vec::new(),
    rng_state_at_start: rng_start,
    rng_state_at_end: rng_end,
    direct_imports: Vec::new(),
    gizmo_reads: Vec::new(),
    own_async_deps_bitmask: 0,
  });
  ctx
    .module_exports
    .borrow_mut()
    .insert("rng".to_string(), stale_entry);
  ctx
    .module_exports_lru
    .borrow_mut()
    .push_back("rng".to_string());

  ctx.reset_rng_to_default();
  ctx.rng().advance(2);
  let actual_exports = ctx.resolve_module("rng").unwrap();
  let actual = actual_exports.get("x").unwrap().as_int().unwrap();

  assert_eq!(actual, 2);
}

#[cfg(test)]
fn inject_gizmo(ctx: &EvalCtx, module: &str, handle: &str, value: Value) {
  ctx
    .gizmo_values
    .borrow_mut()
    .entry(module.to_string())
    .or_default()
    .insert(handle.to_string(), value);
}

#[test]
fn test_gizmo_returns_injected_vec3() {
  let ctx = EvalCtx::default();
  ctx
    .module_sources
    .borrow_mut()
    .insert("node".to_string(), "export p = gizmo(\"cut1\")".to_string());
  inject_gizmo(&ctx, "node", "cut1", Value::Vec3(Vec3::new(1., 2., 3.)));

  parse_and_eval_program_with_ctx(
    "import { p } from \"node\"\nresult = p".to_string(),
    &ctx,
    false,
  )
  .unwrap();
  assert_eq!(
    ctx.get_global("result").unwrap().as_vec3().unwrap(),
    &Vec3::new(1., 2., 3.)
  );
}

#[test]
fn test_gizmo_unset_returns_default() {
  let ctx = EvalCtx::default();
  ctx.module_sources.borrow_mut().insert(
    "node".to_string(),
    "export p = gizmo(\"x\", default=vec3(4, 5, 6))".to_string(),
  );
  parse_and_eval_program_with_ctx(
    "import { p } from \"node\"\nresult = p".to_string(),
    &ctx,
    false,
  )
  .unwrap();
  assert_eq!(
    ctx.get_global("result").unwrap().as_vec3().unwrap(),
    &Vec3::new(4., 5., 6.)
  );
}

/// Changing the injected value must re-eval the owning module — which also proves the
/// `gizmo` call wasn't const-folded into a stale literal (it sits in an otherwise-const expr).
#[test]
fn test_gizmo_reinjection_invalidates_cache() {
  let ctx = EvalCtx::default();
  ctx.module_sources.borrow_mut().insert(
    "node".to_string(),
    "export p = gizmo(\"a\") + vec3(10, 0, 0)".to_string(),
  );

  let run = |x: f32| -> f32 {
    inject_gizmo(&ctx, "node", "a", Value::Vec3(Vec3::new(x, 0., 0.)));
    ctx.replayed_this_run.borrow_mut().clear();
    parse_and_eval_program_with_ctx(
      "import { p } from \"node\"\nresult = p".to_string(),
      &ctx,
      false,
    )
    .unwrap();
    ctx.get_global("result").unwrap().as_vec3().unwrap().x
  };

  assert_eq!(run(1.), 11.);
  assert_eq!(run(2.), 12.);
}

/// An unrelated handle changing must NOT change a module that only reads a different one.
#[test]
fn test_gizmo_unrelated_handle_change_keeps_value() {
  let ctx = EvalCtx::default();
  ctx
    .module_sources
    .borrow_mut()
    .insert("node".to_string(), "export p = gizmo(\"a\")".to_string());
  inject_gizmo(&ctx, "node", "a", Value::Vec3(Vec3::new(5., 0., 0.)));

  let run = || -> f32 {
    ctx.replayed_this_run.borrow_mut().clear();
    parse_and_eval_program_with_ctx(
      "import { p } from \"node\"\nresult = p".to_string(),
      &ctx,
      false,
    )
    .unwrap();
    ctx.get_global("result").unwrap().as_vec3().unwrap().x
  };

  assert_eq!(run(), 5.);
  inject_gizmo(&ctx, "node", "b", Value::Vec3(Vec3::new(9., 0., 0.)));
  assert_eq!(run(), 5.);
}

#[test]
fn test_unnamed_gizmo_positional_ids() {
  let ctx = EvalCtx::default();
  ctx.module_sources.borrow_mut().insert(
    "node".to_string(),
    "export a = gizmo()\nexport b = gizmo()".to_string(),
  );
  inject_gizmo(&ctx, "node", "@0", Value::Vec3(Vec3::new(1., 0., 0.)));
  inject_gizmo(&ctx, "node", "@1", Value::Vec3(Vec3::new(2., 0., 0.)));

  parse_and_eval_program_with_ctx(
    "import { a, b } from \"node\"\nra = a\nrb = b".to_string(),
    &ctx,
    false,
  )
  .unwrap();
  assert_eq!(ctx.get_global("ra").unwrap().as_vec3().unwrap().x, 1.);
  assert_eq!(ctx.get_global("rb").unwrap().as_vec3().unwrap().x, 2.);
}

#[test]
fn test_gizmo_transform_returns_injected_mat4_and_applies() {
  let ctx = EvalCtx::default();
  ctx.module_sources.borrow_mut().insert(
    "node".to_string(),
    "export m = gizmo_transform(\"t\")".to_string(),
  );
  let mut t = Mat4::identity();
  t[(0, 3)] = 7.;
  inject_gizmo(&ctx, "node", "t", Value::Mat4(Rc::new(t)));

  parse_and_eval_program_with_ctx(
    "import { m } from \"node\"\nresult = m".to_string(),
    &ctx,
    false,
  )
  .unwrap();
  let Value::Mat4(m) = ctx.get_global("result").unwrap() else {
    panic!("expected mat4");
  };
  assert_eq!(m[(0, 3)], 7.);
}

#[test]
fn test_gizmo2d_projects_to_vec2_and_records_mask() {
  let ctx = EvalCtx::default();
  ctx
    .module_sources
    .borrow_mut()
    .insert("node".to_string(), "export p = gizmo2d(\"g\")".to_string());
  inject_gizmo(&ctx, "node", "g", Value::Vec3(Vec3::new(1., 2., 3.)));

  parse_and_eval_program_with_ctx(
    "import { p } from \"node\"\nresult = p".to_string(),
    &ctx,
    false,
  )
  .unwrap();
  // Default axes XZ → the vec2 is (x, z), dropping y.
  let Value::Vec2(v) = ctx.get_global("result").unwrap() else {
    panic!("expected vec2");
  };
  assert_eq!(v, Vec2::new(1., 3.));

  let gizmos = ctx.rendered_gizmos.inner.borrow();
  assert_eq!(gizmos.iter().find(|g| g.handle_id == "g").unwrap().axes, [true, false, true]);
}

#[test]
fn test_gizmo1d_projects_to_num_with_y_default() {
  let ctx = EvalCtx::default();
  ctx
    .module_sources
    .borrow_mut()
    .insert("node".to_string(), "export p = gizmo1d(\"g\")".to_string());
  inject_gizmo(&ctx, "node", "g", Value::Vec3(Vec3::new(1., 2., 3.)));

  parse_and_eval_program_with_ctx(
    "import { p } from \"node\"\nresult = p".to_string(),
    &ctx,
    false,
  )
  .unwrap();
  assert_eq!(ctx.get_global("result").unwrap().as_float().unwrap(), 2.);
  let gizmos = ctx.rendered_gizmos.inner.borrow();
  assert_eq!(gizmos.iter().find(|g| g.handle_id == "g").unwrap().axes, [false, true, false]);
}

/// `axes=` override + the `giz2d` alias both resolve to the same masked, projected result.
#[test]
fn test_gizmo2d_axes_override_via_alias() {
  let ctx = EvalCtx::default();
  ctx
    .module_sources
    .borrow_mut()
    .insert("node".to_string(), "export p = giz2d(\"g\", axes=\"xy\")".to_string());
  inject_gizmo(&ctx, "node", "g", Value::Vec3(Vec3::new(1., 2., 3.)));

  parse_and_eval_program_with_ctx(
    "import { p } from \"node\"\nresult = p".to_string(),
    &ctx,
    false,
  )
  .unwrap();
  let Value::Vec2(v) = ctx.get_global("result").unwrap() else {
    panic!("expected vec2");
  };
  assert_eq!(v, Vec2::new(1., 2.));
  let gizmos = ctx.rendered_gizmos.inner.borrow();
  assert_eq!(gizmos.iter().find(|g| g.handle_id == "g").unwrap().axes, [true, true, false]);
}

#[test]
fn test_gizmo_ghost_kwarg_passthrough() {
  let ctx = EvalCtx::default();
  ctx.module_sources.borrow_mut().insert(
    "node".to_string(),
    "export a = gizmo(\"on\", ghost=true)\nexport b = gizmo(\"off\", ghost=false)\nexport c = gizmo(\"def\")"
      .to_string(),
  );
  parse_and_eval_program_with_ctx(
    "import { a, b, c } from \"node\"\nra = a\nrb = b\nrc = c".to_string(),
    &ctx,
    false,
  )
  .unwrap();
  let gizmos = ctx.rendered_gizmos.inner.borrow();
  let ghost = |id: &str| gizmos.iter().find(|g| g.handle_id == id).unwrap().ghost;
  assert_eq!(ghost("on"), Some(true));
  assert_eq!(ghost("off"), Some(false));
  assert_eq!(ghost("def"), None);
}

#[test]
fn test_set_curve_angle_threshold_sets_default_and_explicit_overrides() {
  let ctx = EvalCtx::default();
  assert_eq!(ctx.resolve_curve_angle_degrees(&Value::Nil), 1.0);

  parse_and_eval_program_with_ctx("set_curve_angle_threshold(12)".to_string(), &ctx, false).unwrap();
  assert_eq!(*ctx.default_curve_angle_degrees.borrow(), 12.0);
  // Omitted (nil) follows the runtime default; an explicit value overrides it.
  assert_eq!(ctx.resolve_curve_angle_degrees(&Value::Nil), 12.0);
  assert_eq!(ctx.resolve_curve_angle_degrees(&Value::Float(3.0)), 3.0);
}

#[test]
fn test_set_curve_angle_threshold_rejects_nonpositive() {
  let ctx = EvalCtx::default();
  assert!(parse_and_eval_program_with_ctx("set_curve_angle_threshold(0)".to_string(), &ctx, false).is_err());
}

#[test]
fn test_apply_mat4_accepts_mat4_overload() {
  let ctx = EvalCtx::default();
  parse_and_eval_program_with_ctx(
    "m = gizmo_transform(\"t\")\nresult = box(1) | apply_mat4(m)".to_string(),
    &ctx,
    false,
  )
  .unwrap();
  assert!(matches!(
    ctx.get_global("result").unwrap(),
    Value::Mesh(_)
  ));
}

#[test]
fn test_repeated_randv_calls_return_distinct_values() {
  let ctx = EvalCtx::default();
  ctx.reset_rng_to_default();
  parse_and_eval_program_with_ctx(
    r#"a = randv()
b = randv()
"#
    .to_string(),
    &ctx,
    false,
  )
  .unwrap();
  let a = ctx.get_global("a").unwrap().as_vec3().unwrap().clone();
  let b = ctx.get_global("b").unwrap().as_vec3().unwrap().clone();
  assert_ne!(a, b, "two randv() calls produced identical values: {a:?}");
}

#[test]
fn test_module_scope_isolation_and_export_in_main() {
  let ctx = EvalCtx::default();

  // Module should not see the main program's variables
  ctx
    .module_sources
    .borrow_mut()
    .insert("isolated".to_string(), "export val = 100".to_string());

  // `export` in main program just acts as assignment (no error, no module context)
  let src = r#"
export outer_var = 999
import { val } from "isolated"
result = val + outer_var
"#;

  parse_and_eval_program_with_ctx(src.to_string(), &ctx, false).unwrap();
  assert_eq!(ctx.get_global("result").unwrap().as_int().unwrap(), 1099);
  assert_eq!(ctx.get_global("outer_var").unwrap().as_int().unwrap(), 999);
}

#[test]
fn test_export_not_allowed_in_closure_or_block() {
  let ctx = EvalCtx::default();

  // export inside a closure body should be a parse error
  let src = "fn = || { export x = 1 }";
  let result = parse_and_eval_program_with_ctx(src.to_string(), &ctx, false);
  assert!(
    result.is_err(),
    "export inside closure should be a parse error"
  );

  // export inside a block expression should be a parse error
  let src = "y = { export x = 1\nx }";
  let result = parse_and_eval_program_with_ctx(src.to_string(), &ctx, false);
  assert!(
    result.is_err(),
    "export inside block should be a parse error"
  );
}

#[test]
fn test_import_unknown_module_error() {
  let ctx = EvalCtx::default();
  let src = r#"import { x } from "nonexistent""#;
  let result = parse_and_eval_program_with_ctx(src.to_string(), &ctx, false);
  assert!(result.is_err());
  let err = format!("{}", result.unwrap_err());
  assert!(
    err.contains("Unknown module"),
    "Error should mention unknown module, got: {err}"
  );
}

#[test]
fn test_side_effect_only_import() {
  let ctx = EvalCtx::default();
  ctx
    .module_sources
    .borrow_mut()
    .insert("side".to_string(), "x = 42".to_string());
  // `import { } from "..."` should parse and run the module body for its side
  // effects without binding any names. Used by treeCodegen to evaluate root nodes
  // that have no `export mesh` (e.g. legacy `box(1) | render` sources).
  let src = r#"import { } from "side""#;
  parse_and_eval_program_with_ctx(src.to_string(), &ctx, false).unwrap();
}

#[test]
fn test_evaluate_module_to_scope() {
  let ctx = EvalCtx::default();

  // Plain assignment makes the binding available in the returned scope.
  let scope = ctx.evaluate_module_to_scope("a = 7\nb = a * 2").unwrap();
  let a_sym = ctx.interned_symbols.intern("a");
  let b_sym = ctx.interned_symbols.intern("b");
  assert_eq!(scope.get(a_sym).unwrap().as_int().unwrap(), 7);
  assert_eq!(scope.get(b_sym).unwrap().as_int().unwrap(), 14);

  // Exports are assigned into the scope like any other binding.
  let scope = ctx
    .evaluate_module_to_scope("export foo = 42")
    .unwrap();
  let foo_sym = ctx.interned_symbols.intern("foo");
  assert_eq!(scope.get(foo_sym).unwrap().as_int().unwrap(), 42);
}

#[test]
fn test_ambient_scope_visible_in_imported_module() {
  let ctx = EvalCtx::default();

  // Build an ambient scope containing `my_const = 42` and install it.
  let ambient = ctx.evaluate_module_to_scope("my_const = 42").unwrap();
  ctx.set_ambient_scope(ambient);

  // A module's body should see `my_const` via the ambient scope.
  ctx
    .module_sources
    .borrow_mut()
    .insert("user_mod".to_string(), "export val = my_const + 1".to_string());

  let src = r#"
import { val } from "user_mod"
result = val
"#;
  parse_and_eval_program_with_ctx(src.to_string(), &ctx, false).unwrap();
  // result lives in the per-eval scope, not in ctx.globals when ambient is set.
  // We instead verify by checking the cached module export.
  let exports = ctx
    .module_exports
    .borrow()
    .get("user_mod")
    .cloned()
    .unwrap();
  assert_eq!(exports.exports.get("val").unwrap().as_int().unwrap(), 43);
}

#[test]
fn test_circular_import_detected() {
  let ctx = EvalCtx::default();

  ctx.module_sources.borrow_mut().insert(
    "a".to_string(),
    r#"import { x } from "b"
export y = x"#
      .to_string(),
  );
  ctx.module_sources.borrow_mut().insert(
    "b".to_string(),
    r#"import { y } from "a"
export x = y"#
      .to_string(),
  );

  let src = r#"import { y } from "a""#;
  let result = parse_and_eval_program_with_ctx(src.to_string(), &ctx, false);
  assert!(result.is_err(), "circular import should fail, not infinite-loop");
  let err = format!("{}", result.unwrap_err());
  assert!(
    err.contains("Circular module import"),
    "error should mention circular import, got: {err}"
  );
}

#[test]
fn test_module_error_reports_module_name_and_local_line() {
  let ctx = EvalCtx::default();

  // Module on line 2 has a binop type error. Root program is parsed without prelude,
  // but we still want the reported line to be the *module's* line, not flattened.
  ctx.module_sources.borrow_mut().insert(
    "broken".to_string(),
    // Line 1: comment-ish; Line 2: the error.
    "y = 1\nexport z = y + \"hello\"".to_string(),
  );

  let src = r#"import { z } from "broken""#;
  let result = parse_and_eval_program_with_ctx(src.to_string(), &ctx, false);
  assert!(result.is_err());
  let err = format!("{}", result.unwrap_err());
  assert!(
    err.contains("broken"),
    "error should mention the failing module name, got: {err}"
  );
}

#[test]
fn test_parse_without_prelude_resets_source_map_offset() {
  let ctx = EvalCtx::default();

  parse_program_maybe_with_prelude(&ctx, "x = 1".to_string(), true).unwrap();
  assert!(ctx.source_map.borrow().prelude_line_count > 0);

  parse_program_maybe_with_prelude(&ctx, "x = 1".to_string(), false).unwrap();
  assert_eq!(ctx.source_map.borrow().prelude_line_count, 0);
}
