//! Texel-closure auto-vectorization: compiles `t -> |texel| …` bodies into whole-buffer
//! kernel passes over planar per-channel registers instead of W·H interpreter frames.
//! Design, case matrix, and decided constraints live in docs/texture-autovec-plan.md.
//!
//! The contract is bit-identical f32 output vs the per-texel path. What preserves it:
//! uniform subtrees are evaluated by the ordinary interpreter (once instead of per texel —
//! identical by determinism, guarded by an effect fence); varying ops replicate the exact
//! per-arity formula of the scalar arm they displace, pinned by resolving the same def_ix
//! the interpreter would via phantom typed values; anything else bails to the scalar loop.

use std::{
  cell::{Cell, RefCell},
  collections::VecDeque,
  rc::Rc,
};

use arrayvec::ArrayVec;
use fxhash::{FxHashMap, FxHasher};

use crate::{
  ast::{
    BinOp, CaptureFrom, DestructurePattern, Expr, FunctionCall, FunctionCallTarget,
    MapLiteralEntry, PrefixOp, ResolvedBody, Statement, VarRes,
  },
  builtins::{fn_defs::fn_sigs, resolve_tile_period, tex_kernels as kern},
  get_args, noise_batch,
  seq::EagerSeq,
  seq_as_eager, ArgRef, ArgType, Callable, Closure, ErrorStack, EvalCtx, FrameEnv, GetArgsOutput,
  SourceLoc, Sym, TexStorage, TextureHandle, Value, Vec2, Vec3, Vec4, EMPTY_KWARGS,
};

const MIN_TEXELS: usize = 64;
const MAX_CACHED_PLANS: usize = 512;
const REG_BYTE_BUDGET: u64 = 512 << 20;
/// Bail once a body's counters approach the `u16` register/uniform index space.
const MAX_PLAN_SLOTS: usize = 60_000;
/// Longest sequence a texel body may loop over (each element unrolls its body once).
const MAX_UNROLL: usize = 256;
/// Sequence builtins lowered structurally at compile time; `fn` args may be closure literals.
const SEQ_BUILTINS: &[&str] = &[
  "map", "fold", "reduce", "scan", "any", "all", "collect", "first", "last", "take", "skip",
  "reverse", "flatten", "chain",
];
const SEQ_BAILS: &[&str] = &[
  "filter",
  "fold_while",
  "for_each",
  "take_while",
  "skip_while",
];

#[derive(Clone)]
enum PlanEntry {
  Ok(Rc<Plan>),
  /// Compile bailed; the reason is replayed as this body's report on every later run.
  Bail(Rc<str>, (u32, u32)),
}

pub struct VectorizeState {
  plans: RefCell<FxHashMap<PlanKey, PlanEntry>>,
  /// Insertion order over `plans`, so hitting the cap evicts the oldest quarter instead of
  /// wiping the live run's plans.
  plan_order: RefCell<VecDeque<PlanKey>>,
  /// Shared per-(w,h) uv planes: `u = (x+0.5)/w`, `v = (y+0.5)/h` — every generator and
  /// uv-referencing map body at one size reads the same two read-only planes.
  uv_planes: RefCell<VecDeque<((u32, u32), [Rc<Vec<f32>>; 2])>>,
  pub no_vectorize: Cell<bool>,
  pub verify: Cell<bool>,
  /// Render each vectorized body's plan listing with per-step timings into its report.
  /// Off by default: the common path must not even read a clock.
  pub profile: Cell<bool>,
  /// Per-body outcome for diagnostics, keyed by `ResolvedBody::id`; last invocation wins.
  pub reports: RefCell<FxHashMap<u64, VectorizeReport>>,
}

// Bound method call on the global `performance` object; a bare `js_namespace` import calls
// `now` detached from its receiver ("Illegal invocation").
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
  type Performance;
  #[wasm_bindgen(js_name = performance)]
  static PERFORMANCE: Performance;
  #[wasm_bindgen(method)]
  fn now(this: &Performance) -> f64;
}

fn now_ms() -> f64 {
  #[cfg(target_arch = "wasm32")]
  {
    PERFORMANCE.now()
  }
  #[cfg(not(target_arch = "wasm32"))]
  {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
      .get_or_init(std::time::Instant::now)
      .elapsed()
      .as_secs_f64()
      * 1e3
  }
}

impl VectorizeState {
  /// Per-run teardown for a long-lived host ctx. `plans` deliberately survives: closures
  /// replayed from the module-exports cache keep their `ResolvedBody` Rc, so their plans hit
  /// across runs, and body ids are monotonic and never reused, so a stale key can never
  /// collide. `uv_planes` does not survive — 536 MB per 8192² entry, rebuilt in one pass.
  pub fn reset_per_run(&self) {
    self.uv_planes.borrow_mut().clear();
    self.reports.borrow_mut().clear();
  }
}

impl Default for VectorizeState {
  fn default() -> Self {
    #[cfg(not(target_arch = "wasm32"))]
    let env_on = |name: &str| std::env::var_os(name).is_some_and(|v| v != "0");
    #[cfg(target_arch = "wasm32")]
    let env_on = |_name: &str| false;
    VectorizeState {
      plans: RefCell::new(FxHashMap::default()),
      plan_order: RefCell::new(VecDeque::new()),
      uv_planes: RefCell::new(VecDeque::new()),
      no_vectorize: Cell::new(env_on("GEOSCRIPT_NO_VECTORIZE")),
      verify: Cell::new(env_on("GEOSCRIPT_VECTORIZE_VERIFY")),
      profile: Cell::new(false),
      reports: RefCell::new(FxHashMap::default()),
    }
  }
}

#[derive(Clone, Debug)]
pub struct VectorizeReport {
  pub vectorized: bool,
  /// Bail reason naming the offending construct.
  pub reason: Option<String>,
  /// (line, col) of the offending node (or the body on success), within `module`'s source.
  pub loc: (u32, u32),
  /// Module being evaluated at invocation — the defining module for all but cross-module
  /// helper closures, which is what the host needs to attribute the loc to a node.
  pub module: Option<String>,
  /// Plan listing (+ per-step ms) for this invocation; only with `VectorizeState::profile`.
  pub plan: Option<String>,
}

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
struct PlanKey {
  body_id: u64,
  /// Hash of the entry-point shape: generator, or the ordered per-input channel counts. A
  /// plan compiled for `(1ch, 3ch)` must never be replayed for `(3ch, 1ch)`.
  input_sig: u64,
  capture_sig: u64,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Src {
  Reg(u16),
  /// Plane of the concatenated input textures; each texel param's channels start at a
  /// compile-time offset into this list. Caps the entry point at 256 input planes.
  In(u8),
  /// Channel of the ctx-cached per-(w,h) uv planes (0 = u, 1 = v); read-only, shared.
  Uv(u8),
  /// Channel of a runtime uniform value.
  Uni(u16, u8),
  Const(f32),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum OpKind {
  Neg,
  Abs,
  Sqrt,
  Sin,
  Cos,
  Tan,
  Asin,
  Acos,
  Atan,
  Exp,
  Log2,
  Floor,
  Ceil,
  Round,
  Fract,
  Trunc,
  Sigmoid,
  Add,
  Sub,
  Mul,
  Div,
  Mod,
  Pow,
  Atan2,
  Min,
  Max,
  Clamp,
  SmoothStep,
  /// `((x - e0) / (e1 - e0)).clamp(0., 1.)`, srcs (x, e0, e1).
  LinearStep,
  /// Float-arm lerp: `a + (b - a) * t`, srcs (a, b, t).
  LerpF,
  /// Vec-arm lerp (nalgebra axpy): `t*b + (1-t)*a`, srcs (a, b, t). nalgebra skips the
  /// `beta * y` term entirely when `beta == 0`, so `t == 1` must not read `a` either.
  LerpV,
  // Masks are 1-channel planes holding exactly 0.0/1.0 (compares produce them, logic ops
  // preserve them), which is what lets `and`/`or`/`not` be plain arithmetic.
  Gt,
  Lt,
  Gte,
  Lte,
  Eq,
  Neq,
  And,
  Or,
  Not,
  /// Exact per-texel pick, srcs (mask, then, else). Bitwise, never a lerp — `0 * inf` would
  /// NaN the untaken side — and a uniform mask makes it a register move in the executor.
  Select,
}

enum UniSrc {
  Expr(Expr),
  Const(Value),
  Capture(u16),
  /// Read of a uniform local from the frame's mirror (call targets resolved via slots).
  Slot(u16),
  /// Copy of an earlier uniform-table value (binds uniform args to inlinee param slots).
  UniRef(u16),
  /// `.field` of an earlier uniform-table value, re-evaluated per run.
  SwizzleOf {
    of: u16,
    field: String,
  },
  /// Element `ix` of an earlier uniform-table sequence (length pinned by `UniShape::Seq`;
  /// lazy sequences are consumed once per run).
  SeqElem {
    of: u16,
    ix: u16,
  },
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum UniShape {
  /// Extracted as per-channel f32s with this arity and int/float class — the class steers
  /// the interpreter's def resolution, so a run that flips it must fall back to scalar.
  Num { ar: u8, int: bool },
  /// A builtin callable with this `fn_entry_ix` (validated, never extracted).
  Builtin(usize),
  /// A closure whose body was inlined; validated by `ResolvedBody` id.
  ClosureBody(u64),
  /// A pure dynamic callable with this return arity (validated, invoked per texel).
  Dynamic(u8),
  /// A bool, extracted as a 0/1 mask channel (conditions, logic operands, mask arms).
  Bool,
  /// Used as a raw `Value` (fbm params); validated at the use site.
  Any,
  /// A sequence unrolled to exactly `len` elements: the count is plan structure, so a
  /// different length on a cache-hit run evicts the plan and recompiles.
  Seq { len: u16 },
  /// A texture gathered by `sample`; its channel count sizes the step's outputs, so a
  /// change recompiles like `Seq`.
  Texture(u8),
}

struct UniStep {
  src: UniSrc,
  shape: UniShape,
  frame: u16,
  slot: Option<u16>,
  hint: Option<ArgType>,
  /// Skipped (left `Nil`) on runs whose guard is off — the scalar path never evaluates an
  /// untaken arm either.
  guard: Option<u16>,
  /// Inside a varying conditional's arm or a `&&`/`||` rhs: the scalar path evaluates it
  /// for a texel-dependent subset (possibly none), so an error here aborts to scalar
  /// instead of being reported.
  speculative: bool,
}

/// Run-time arm guard for a uniform-condition conditional: on iff `parent` is on and the
/// cond uniform equals `expect`. Guarded steps and uniforms are skipped when it's off.
struct Guard {
  parent: Option<u16>,
  cond: u16,
  expect: bool,
}

/// An arm that failed to compile. Runs whose guard selects it hand off to the scalar path;
/// `evict` drops the plan so a value-dependent failure gets recompiled under new uniforms.
struct BranchAbort {
  guard: u16,
  reason: Rc<str>,
  loc: (u32, u32),
  evict: bool,
}

/// One interpreter frame the plan evaluates uniform expressions in: frame 0 is the texel
/// closure itself; each closure inline adds one, with its captures/self taken from the
/// callee value at `callee_uix` per run — or from `baked_callee` when the optimizer folded
/// the callee to an AST literal, which has no per-run identity to re-fetch.
struct FrameSpec {
  n_slots: u16,
  callee_uix: Option<u16>,
  baked_callee: Option<Rc<Callable>>,
}

struct FbmStep {
  dim: u8,
  dst: u16,
  pos: [Src; 3],
  /// seed, octaves, frequency, lacunarity, persistence — uniform-table indices.
  params: [u16; 5],
  tileable: Option<u16>,
  /// Index into the run's resolved-fbm-params table.
  rix: u16,
}

enum DynCallee {
  /// Literal callable baked into the AST; identity-stable across invocations.
  Baked(Rc<Callable>),
  /// Re-fetched per run from the uniform table (validated by `UniShape::Dynamic`).
  Uni(u16),
}

/// Per-texel invocation of a pure `Callable::Dynamic` (ramps): ~20 ns/texel for this op
/// while the rest of the body stays vectorized.
struct DynStep {
  callee: DynCallee,
  args: Vec<ArrayVec<Src, 4>>,
  dst: ArrayVec<u16, 4>,
}

/// `sample(tex, uv, filter, wrap)` over a varying coordinate field: one gather pass that
/// resolves each texel's address/weights once and reads every plane of the source.
struct GatherStep {
  /// Uniform-table indices: the texture, and its `filter` / `wrap` string args.
  tex: u16,
  filter: u16,
  wrap: u16,
  u: Src,
  v: Src,
  dst: ArrayVec<u16, 4>,
  /// Index into the run's resolved-gather table.
  rix: u16,
}

enum Step {
  Op {
    kind: OpKind,
    dst: u16,
    a: Src,
    b: Src,
    c: Src,
  },
  Fbm(FbmStep),
  Dyn(DynStep),
  Gather(GatherStep),
}

enum PlanOut {
  Chans(ArrayVec<Src, 4>),
  Uniform(u16),
}

pub(crate) struct Plan {
  frames: Vec<FrameSpec>,
  n_regs: u16,
  steps: Vec<Step>,
  /// Parallel to `steps`.
  step_guards: Vec<Option<u16>>,
  guards: Vec<Guard>,
  branch_aborts: Vec<BranchAbort>,
  unis: Vec<UniStep>,
  n_fbm: u16,
  n_gather: u16,
  /// Step index of the last read per register (`u32::MAX` = output, never freed).
  reg_last: Vec<u32>,
  out: PlanOut,
  /// Diagnostics: ops folded at emission, ops answered by CSE, steps removed as dead.
  n_folded: u16,
  n_cse: u16,
  n_dead: u16,
  /// Peak simultaneously-live registers, for the memory gate.
  peak_regs: u16,
  /// Whether any step or output reads the shared uv planes.
  uses_uv: bool,
  /// Channel count per input texture, in order; empty for a generator.
  input_arities: Vec<u8>,
}

impl Plan {
  /// Decode a `Src::In` plane index back to (input index, channel within that input).
  fn input_chan(&self, plane: u8) -> (usize, usize) {
    let mut plane = plane as usize;
    for (i, &ar) in self.input_arities.iter().enumerate() {
      if plane < ar as usize {
        return (i, plane);
      }
      plane -= ar as usize;
    }
    unreachable!("plane index past the input planes")
  }
}

/// Resolved+validated per-run uniform state; `exec` is infallible given one of these.
struct UniRun {
  vals: Vec<Value>,
  chans: Vec<[f32; 4]>,
  fbm: Vec<FbmResolved>,
  /// `None` for steps whose guard is off this run.
  gather: Vec<Option<GatherResolved>>,
  guards: Vec<bool>,
}

#[derive(Clone)]
struct GatherResolved {
  tex: Rc<TextureHandle>,
  filter: kern::SampleFilter,
  wrap: crate::TextureWrap,
}

#[derive(Clone, Copy)]
struct FbmResolved {
  seed: u32,
  octaves: usize,
  frequency: f32,
  persistence: f32,
  lacunarity: f32,
  tileable: Option<f32>,
}

enum CErr {
  /// Fall back to the scalar loop; reason + location for diagnostics.
  Bail(String, SourceLoc),
  /// Real error the scalar path would also produce (uniform-eval / resolution failure).
  Err(ErrorStack),
}

fn bail<T>(reason: impl Into<String>, loc: SourceLoc) -> Result<T, CErr> {
  Err(CErr::Bail(reason.into(), loc))
}

#[derive(Clone)]
struct VV {
  chans: ArrayVec<Src, 4>,
  /// A varying bool: one 0/1 plane. Never a number — def resolution sees `Value::Bool`.
  mask: bool,
}

impl VV {
  fn num(chans: ArrayVec<Src, 4>) -> AbsVal {
    AbsVal::V(VV { chans, mask: false })
  }

  fn mask(src: Src) -> AbsVal {
    AbsVal::V(VV {
      chans: [src].into_iter().collect(),
      mask: true,
    })
  }
}

#[derive(Clone)]
enum AbsVal {
  U(u16),
  V(VV),
  /// Sequence with compile-time-known elements; loops over it unroll.
  Seq(AbsSeq),
}

/// `eager` mirrors the scalar path's indexability: array literals and `collect`/`reverse`
/// results take `[]`, `->`/`map`/`take`/… results don't.
#[derive(Clone)]
struct AbsSeq {
  els: Rc<Vec<AbsVal>>,
  eager: bool,
}

/// A callback position's callee: a closure literal (inlined by slot renaming, so its
/// captures may be varying) or a uniform callable value.
enum Cb {
  Lit(Rc<Expr>),
  Callable { c: Rc<Callable>, uix: Option<u16> },
}

/// `Src` with `Const` by bit pattern, so it can key a map (and order operands).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum SrcKey {
  Reg(u16),
  In(u8),
  Uv(u8),
  Uni(u16, u8),
  Const(u32),
}

impl From<Src> for SrcKey {
  fn from(s: Src) -> Self {
    match s {
      Src::Reg(r) => SrcKey::Reg(r),
      Src::In(c) => SrcKey::In(c),
      Src::Uv(c) => SrcKey::Uv(c),
      Src::Uni(u, c) => SrcKey::Uni(u, c),
      Src::Const(k) => SrcKey::Const(k.to_bits()),
    }
  }
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum UniKey {
  /// Numeric/bool literal by type tag + bit pattern.
  Val(u8, [u32; 4]),
  /// String literal / default (`filter="nearest"`), so repeated gathers share their args.
  Str(Rc<str>),
  /// Texture literal (a const-folded capture) by handle identity; the AST keeps it alive.
  Tex(usize),
  Capture(u16, u16),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum CseKey {
  Op {
    kind: OpKind,
    srcs: [SrcKey; 3],
    guard: Option<u16>,
  },
  Fbm {
    dim: u8,
    pos: [SrcKey; 3],
    params: [u16; 5],
    tileable: Option<u16>,
    guard: Option<u16>,
  },
  /// One entry per output channel, inserted together.
  Gather {
    tex: u16,
    u: SrcKey,
    v: SrcKey,
    filter: u16,
    wrap: u16,
    chan: u8,
    guard: Option<u16>,
  },
}

#[derive(Clone)]
enum SlotState {
  Unset,
  Uniform,
  Varying(VV),
  Seq(AbsSeq),
  /// Closure literal with varying captures: only callable, inlined by slot renaming.
  Lit(Rc<Expr>),
}

/// Observable-effect fence: uniform subtrees run through the interpreter once instead of
/// W·H times, which is only sound if they're effect-free. Literal effectful builtins are
/// pre-filtered syntactically; this catches everything else (captured effectful/rng
/// callables, however smuggled) at eval time, restores the state, and forces a bail.
struct EffectFence {
  rng: rand_pcg::Pcg32,
  prints: usize,
  meshes: usize,
  lights: usize,
  paths: usize,
  textures: usize,
  gizmos: usize,
  controls: usize,
  next_render_id: u32,
  unnamed_gizmos: u32,
  gizmo_reads: Option<fxhash::FxHashSet<String>>,
  default_material: Option<Rc<crate::materials::Material>>,
  sharp_angle: f32,
  curve_angle: f32,
}

impl EffectFence {
  fn snapshot(ctx: &EvalCtx) -> Self {
    EffectFence {
      rng: ctx.rng_state(),
      prints: ctx.prints.borrow().len(),
      meshes: ctx.rendered_meshes.len(),
      lights: ctx.rendered_lights.len(),
      paths: ctx.rendered_paths.len(),
      textures: ctx.rendered_textures.len(),
      gizmos: ctx.rendered_gizmos.len(),
      controls: ctx.rendered_controls.len(),
      next_render_id: ctx.next_render_id.get(),
      unnamed_gizmos: ctx.current_module_unnamed_gizmo_count.get(),
      gizmo_reads: ctx.current_module_gizmo_reads.borrow().clone(),
      default_material: ctx.default_material.borrow().clone(),
      sharp_angle: *ctx.sharp_angle_threshold_degrees.borrow(),
      curve_angle: *ctx.default_curve_angle_degrees.borrow(),
    }
  }

  /// True if nothing observable moved; otherwise restores the snapshot and returns false.
  fn verify_or_restore(&self, ctx: &EvalCtx) -> bool {
    let clean = ctx.rng_state() == self.rng
      && ctx.prints.borrow().len() == self.prints
      && ctx.rendered_meshes.len() == self.meshes
      && ctx.rendered_lights.len() == self.lights
      && ctx.rendered_paths.len() == self.paths
      && ctx.rendered_textures.len() == self.textures
      && ctx.rendered_gizmos.len() == self.gizmos
      && ctx.rendered_controls.len() == self.controls
      && ctx.next_render_id.get() == self.next_render_id
      && ctx.current_module_unnamed_gizmo_count.get() == self.unnamed_gizmos
      && *ctx.current_module_gizmo_reads.borrow() == self.gizmo_reads
      && match (&*ctx.default_material.borrow(), &self.default_material) {
        (None, None) => true,
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        _ => false,
      }
      && *ctx.sharp_angle_threshold_degrees.borrow() == self.sharp_angle
      && *ctx.default_curve_angle_degrees.borrow() == self.curve_angle;
    if clean {
      return true;
    }
    ctx.set_rng_state(self.rng.clone());
    ctx.prints.borrow_mut().truncate(self.prints);
    ctx.rendered_meshes.inner.borrow_mut().truncate(self.meshes);
    ctx.rendered_lights.inner.borrow_mut().truncate(self.lights);
    ctx.rendered_paths.inner.borrow_mut().truncate(self.paths);
    ctx
      .rendered_textures
      .inner
      .borrow_mut()
      .truncate(self.textures);
    ctx.rendered_gizmos.inner.borrow_mut().truncate(self.gizmos);
    ctx
      .rendered_controls
      .inner
      .borrow_mut()
      .truncate(self.controls);
    ctx.next_render_id.set(self.next_render_id);
    ctx
      .current_module_unnamed_gizmo_count
      .set(self.unnamed_gizmos);
    *ctx.current_module_gizmo_reads.borrow_mut() = self.gizmo_reads.clone();
    *ctx.default_material.borrow_mut() = self.default_material.clone();
    *ctx.sharp_angle_threshold_degrees.borrow_mut() = self.sharp_angle;
    *ctx.default_curve_angle_degrees.borrow_mut() = self.curve_angle;
    false
  }
}

fn num_arity(v: &Value) -> Option<u8> {
  match v {
    Value::Int(_) | Value::Float(_) => Some(1),
    Value::Vec2(_) => Some(2),
    Value::Vec3(_) => Some(3),
    Value::Vec4(_) => Some(4),
    _ => None,
  }
}

fn value_chans(v: &Value) -> Option<([f32; 4], u8)> {
  match v {
    Value::Int(i) => Some(([*i as f32; 4], 1)),
    Value::Float(f) => Some(([*f; 4], 1)),
    Value::Vec2(v) => Some(([v.x, v.y, 0., 0.], 2)),
    Value::Vec3(v) => Some(([v.x, v.y, v.z, 0.], 3)),
    Value::Vec4(v) => Some(([v.x, v.y, v.z, v.w], 4)),
    _ => None,
  }
}

const MAX_CACHED_UV_SIZES: usize = 4;
/// One 8192² pair is 536 MB, so the cache is capped by bytes as well as by count; the
/// newest entry is always kept even when it alone blows the budget.
const UV_PLANE_BYTE_BUDGET: usize = 256 << 20;

fn uv_planes_for(ctx: &EvalCtx, w: usize, h: usize) -> [Rc<Vec<f32>>; 2] {
  let key = (w as u32, h as u32);
  if let Some((_, p)) = ctx
    .tex_vectorize
    .uv_planes
    .borrow()
    .iter()
    .find(|(k, _)| *k == key)
  {
    return p.clone();
  }
  let n = w * h;
  let (mut u, mut v) = (Vec::with_capacity(n), Vec::with_capacity(n));
  for y in 0..h {
    let vy = (y as f32 + 0.5) / h as f32;
    for x in 0..w {
      u.push((x as f32 + 0.5) / w as f32);
      v.push(vy);
    }
  }
  let planes = [Rc::new(u), Rc::new(v)];
  let mut cache = ctx.tex_vectorize.uv_planes.borrow_mut();
  let bytes = |c: &VecDeque<((u32, u32), [Rc<Vec<f32>>; 2])>| {
    c.iter().map(|(_, p)| p[0].len() * 8).sum::<usize>()
  };
  while !cache.is_empty()
    && (cache.len() >= MAX_CACHED_UV_SIZES || bytes(&cache) + n * 8 > UV_PLANE_BYTE_BUDGET)
  {
    cache.pop_front();
  }
  cache.push_back((key, planes.clone()));
  planes
}

fn dynamic_out_arity(c: &Callable) -> Option<u8> {
  let Callable::Dynamic { inner, .. } = c else {
    return None;
  };
  match inner.get_return_type_hint()? {
    ArgType::Float | ArgType::Int | ArgType::Numeric => Some(1),
    ArgType::Vec2 => Some(2),
    ArgType::Vec3 => Some(3),
    ArgType::Vec4 => Some(4),
    _ => None,
  }
}

fn phantom(arity: u8) -> Value {
  match arity {
    1 => Value::Float(0.),
    2 => Value::Vec2(Vec2::zeros()),
    3 => Value::Vec3(Vec3::zeros()),
    _ => Value::Vec4(Rc::new(Vec4::zeros())),
  }
}

fn capture_sig(closure: &Closure) -> u64 {
  use std::hash::{Hash, Hasher};
  let mut h = FxHasher::default();
  for v in closure.captures.iter() {
    v.type_flag_ix().hash(&mut h);
    if let Value::Texture(t) = v {
      (t.channels as u8).hash(&mut h);
    }
    if let Value::Callable(c) = v {
      match &**c {
        Callable::Builtin { fn_entry_ix, .. } => (1u8, *fn_entry_ix as u64).hash(&mut h),
        Callable::Closure(inner) => (2u8, inner.resolved.id).hash(&mut h),
        Callable::Dynamic { .. } => 3u8.hash(&mut h),
        Callable::PartiallyAppliedFn(_) => 4u8.hash(&mut h),
        Callable::ComposedFn(_) => 5u8.hash(&mut h),
      }
    }
  }
  h.finish()
}

// ---------------------------------------------------------------------------------------
// Compiler
// ---------------------------------------------------------------------------------------

/// Compile-time state for one interpreter frame (the texel closure, plus one per inlined
/// closure call).
struct CFrame {
  plan_frame: u16,
  slot_abs: Vec<SlotState>,
  mirror: Rc<RefCell<Vec<Value>>>,
  captures: Rc<[Value]>,
  self_ref: Rc<Callable>,
}

struct Compiler<'a> {
  ctx: &'a EvalCtx,
  steps: Vec<Step>,
  unis: Vec<UniStep>,
  /// Compile-run uniform values, parallel to `unis`; doubles as the first run's table.
  uni_vals: Vec<Value>,
  n_regs: u16,
  n_fbm: u16,
  n_gather: u16,
  /// Ops answered at emission by `peephole` (exact identities / all-constant folds).
  n_folded: u16,
  /// Value-numbering table for CSE: `(op, operands, guard)` → the register already holding
  /// it. A hit under guard `g` may come from `g` or any ancestor guard, never a sibling.
  cse: FxHashMap<CseKey, Src>,
  uni_cse: FxHashMap<(UniKey, Option<u16>), u16>,
  n_cse: u16,
  /// Frame stack; last = the frame currently being compiled.
  frames: Vec<CFrame>,
  plan_frames: Vec<FrameSpec>,
  /// `ResolvedBody` ids of closures currently being inlined (recursion guard).
  inline_stack: Vec<u64>,
  /// Nesting depth of closure-literal inlines (unrolled loop bodies).
  lit_depth: usize,
  uses_uv: bool,
  /// Guard every step/uniform emitted right now carries (innermost uniform-cond arm).
  guard: Option<u16>,
  guards: Vec<Guard>,
  step_guards: Vec<Option<u16>>,
  branch_aborts: Vec<BranchAbort>,
  /// >0 while compiling a varying conditional's arm or a `&&`/`||` rhs.
  spec_depth: u32,
}

const MAX_INLINE_DEPTH: usize = 8;

impl<'a> Compiler<'a> {
  fn cur(&self) -> &CFrame {
    self.frames.last().unwrap()
  }

  fn cur_mut(&mut self) -> &mut CFrame {
    self.frames.last_mut().unwrap()
  }

  fn push_frame(
    &mut self,
    n_slots: u16,
    callee_uix: Option<u16>,
    baked_callee: Option<Rc<Callable>>,
    captures: Rc<[Value]>,
    self_ref: Rc<Callable>,
  ) {
    let plan_frame = self.plan_frames.len() as u16;
    self.plan_frames.push(FrameSpec {
      n_slots,
      callee_uix,
      baked_callee,
    });
    self.frames.push(CFrame {
      plan_frame,
      slot_abs: vec![SlotState::Unset; n_slots as usize],
      mirror: Rc::new(RefCell::new(vec![Value::Nil; n_slots as usize])),
      captures,
      self_ref,
    });
  }

  /// Frames grow when closure literals are inlined into them; the plan's mirror must match.
  fn pop_frame(&mut self) {
    let f = self.frames.pop().unwrap();
    self.plan_frames[f.plan_frame as usize].n_slots = f.slot_abs.len() as u16;
  }

  fn alloc_reg(&mut self) -> u16 {
    let r = self.n_regs;
    self.n_regs += 1;
    r
  }

  fn push_step(&mut self, step: Step) {
    self.steps.push(step);
    self.step_guards.push(self.guard);
  }

  fn push_op(&mut self, kind: OpKind, a: Src, b: Src, c: Src) -> Src {
    if let Some(s) = peephole(kind, a, b, c) {
      self.n_folded += 1;
      return s;
    }
    // Commuting `+`/`*`/`==`/`!=`/mask-and/or only moves which NaN payload survives, which
    // the contract already excludes; `min`/`max` can flip a signed zero, so they stay ordered.
    let (a, b) = match kind {
      OpKind::Add | OpKind::Mul | OpKind::Eq | OpKind::Neq | OpKind::And | OpKind::Or
        if SrcKey::from(b) < SrcKey::from(a) =>
      {
        (b, a)
      }
      _ => (a, b),
    };
    let pad = |i: usize, s: Src| {
      if i < op_arity(kind) {
        SrcKey::from(s)
      } else {
        SrcKey::Const(0)
      }
    };
    let key = |guard| CseKey::Op {
      kind,
      srcs: [pad(0, a), pad(1, b), pad(2, c)],
      guard,
    };
    if let Some(s) = self.cse_lookup(&key) {
      return s;
    }
    let dst = self.alloc_reg();
    self.push_step(Step::Op { kind, dst, a, b, c });
    self.cse.insert(key(self.guard), Src::Reg(dst));
    Src::Reg(dst)
  }

  /// A step emitted under the current guard or any ancestor is executed whenever the
  /// current one is, so its register is safe to reuse; a sibling arm's isn't (it may have
  /// been skipped this run).
  fn cse_lookup(&mut self, key: &impl Fn(Option<u16>) -> CseKey) -> Option<Src> {
    let mut g = self.guard;
    loop {
      if let Some(&s) = self.cse.get(&key(g)) {
        self.n_cse += 1;
        return Some(s);
      }
      g = self.guards[g? as usize].parent;
    }
  }

  /// Literals and captures are immutable, so repeated occurrences (every `3.` in `uv * 3.`
  /// twice, every `fbm` default parameter) share one table entry — which is what lets CSE
  /// see `fbm(pos=uv * 3.)` twice as the same step. Same guard rule as registers.
  fn uni_key(&self, src: &UniSrc, val: &Value) -> Option<UniKey> {
    match src {
      UniSrc::Const(_) | UniSrc::Expr(Expr::Literal { .. }) => {
        let (bits, tag) = match val {
          Value::Float(f) => ([f.to_bits(), 0, 0, 0], 1),
          Value::Int(i) => ([*i as u32, (*i >> 32) as u32, 0, 0], 2),
          Value::Bool(b) => ([*b as u32, 0, 0, 0], 3),
          Value::Vec2(v) => ([v.x.to_bits(), v.y.to_bits(), 0, 0], 4),
          Value::Vec3(v) => ([v.x.to_bits(), v.y.to_bits(), v.z.to_bits(), 0], 5),
          Value::Vec4(v) => (
            [v.x.to_bits(), v.y.to_bits(), v.z.to_bits(), v.w.to_bits()],
            6,
          ),
          Value::String(s) => return Some(UniKey::Str(s.as_str().into())),
          Value::Texture(t) => return Some(UniKey::Tex(Rc::as_ptr(t) as usize)),
          Value::Nil => ([0; 4], 7),
          _ => return None,
        };
        Some(UniKey::Val(tag, bits))
      }
      UniSrc::Capture(ix) => Some(UniKey::Capture(self.cur().plan_frame, *ix)),
      _ => None,
    }
  }

  fn push_uni(&mut self, src: UniSrc, val: Value, slot: Option<u16>, hint: Option<ArgType>) -> u16 {
    let key = if slot.is_none() && hint.is_none() {
      self.uni_key(&src, &val)
    } else {
      None
    };
    if let Some(k) = &key {
      let mut g = self.guard;
      loop {
        if let Some(&uix) = self.uni_cse.get(&(k.clone(), g)) {
          return uix;
        }
        let Some(gi) = g else { break };
        g = self.guards[gi as usize].parent;
      }
    }
    let uix = self.unis.len() as u16;
    if let Some(k) = key {
      self.uni_cse.insert((k, self.guard), uix);
    }
    self.unis.push(UniStep {
      src,
      shape: UniShape::Any,
      frame: self.cur().plan_frame,
      slot,
      hint,
      guard: self.guard,
      speculative: self.spec_depth > 0,
    });
    self.uni_vals.push(val);
    uix
  }

  fn uni_val(&self, uix: u16) -> &Value {
    &self.uni_vals[uix as usize]
  }

  /// A uniform whose value is baked into the plan forever (AST literal or signature
  /// default) — safe for compile-time structural decisions; ordinary uniforms are not,
  /// since cache-hit runs re-evaluate them.
  fn const_uniform_value(&self, v: &AbsVal) -> Option<Value> {
    let AbsVal::U(uix) = v else { return None };
    match &self.unis[*uix as usize].src {
      UniSrc::Const(v) => Some(v.clone()),
      UniSrc::Expr(Expr::Literal { value, .. }) => Some(value.clone()),
      _ => None,
    }
  }

  /// The typed stand-in used for def resolution: real value for uniforms, phantom of the
  /// right arity (or a bool, for masks) for varyings.
  fn typed_value(&self, v: &AbsVal) -> Value {
    match v {
      AbsVal::U(uix) => self.uni_val(*uix).clone(),
      AbsVal::V(vv) if vv.mask => Value::Bool(false),
      AbsVal::V(vv) => phantom(vv.chans.len() as u8),
      AbsVal::Seq(_) => Value::Sequence(Rc::new(EagerSeq {
        inner: Rc::new(Vec::new()),
      })),
    }
  }

  fn arity(&self, v: &AbsVal) -> Result<u8, CErr> {
    match v {
      AbsVal::U(uix) => num_arity(self.uni_val(*uix)).ok_or_else(|| {
        CErr::Bail(
          "non-numeric uniform operand in varying expression".into(),
          SourceLoc::default(),
        )
      }),
      AbsVal::V(vv) if vv.mask => {
        bail("bool used where a number is expected", SourceLoc::default())
      }
      AbsVal::V(vv) => Ok(vv.chans.len() as u8),
      AbsVal::Seq(_) => bail(
        "sequence used where a number is expected",
        SourceLoc::default(),
      ),
    }
  }

  /// A value as a 0/1 mask source: a varying mask's plane, or a uniform bool pinned to
  /// `UniShape::Bool`. Anything else is a type error in the scalar path too; hand off.
  fn mask_src(&mut self, v: &AbsVal, loc: SourceLoc) -> Result<Src, CErr> {
    match v {
      AbsVal::V(vv) if vv.mask => Ok(vv.chans[0]),
      AbsVal::V(_) => bail("expected a bool, found a varying number", loc),
      AbsVal::Seq(_) => bail("expected a bool, found a sequence", loc),
      AbsVal::U(uix) => {
        if !matches!(self.uni_val(*uix), Value::Bool(_)) {
          return bail("expected a bool, found a non-bool uniform", loc);
        }
        self.unis[*uix as usize].shape = UniShape::Bool;
        Ok(Src::Uni(*uix, 0))
      }
    }
  }

  fn hint_fits(hint: ArgType, vv: &VV) -> bool {
    matches!(
      (hint, vv.mask, vv.chans.len()),
      (ArgType::Any, _, _)
        | (ArgType::Bool, true, _)
        | (ArgType::Float | ArgType::Numeric, false, 1)
        | (ArgType::Vec2, false, 2)
        | (ArgType::Vec3, false, 3)
        | (ArgType::Vec4, false, 4)
    )
  }

  /// Compile something the scalar path evaluates for only a texel-dependent subset: a real
  /// error in here may never fire there, so it bails instead of propagating.
  fn speculative(
    &mut self,
    f: impl FnOnce(&mut Self) -> Result<AbsVal, CErr>,
    loc: SourceLoc,
  ) -> Result<AbsVal, CErr> {
    self.spec_depth += 1;
    let r = f(self);
    self.spec_depth -= 1;
    match r {
      Err(CErr::Err(e)) => bail(
        format!("error inside a speculatively-evaluated branch: {e}"),
        loc,
      ),
      r => r,
    }
  }

  /// Channel `c` of a value as a kernel source. For uniforms this pins the extracted-numeric
  /// shape so cache-hit runs validate against it.
  fn chan(&mut self, v: &AbsVal, c: u8) -> Src {
    match v {
      AbsVal::U(uix) => {
        let val = self.uni_val(*uix);
        let ar = num_arity(val).expect("chan() on non-numeric uniform");
        let int = matches!(val, Value::Int(_));
        self.unis[*uix as usize].shape = UniShape::Num { ar, int };
        Src::Uni(*uix, c.min(ar - 1))
      }
      AbsVal::V(vv) => vv.chans[c as usize],
      AbsVal::Seq(_) => unreachable!("arity() rejects sequences"),
    }
  }

  // -------------------------------------------------------------------------------------
  // Uniform-subtree classification
  // -------------------------------------------------------------------------------------

  fn slot_is_abstract(&self, slot: u16) -> bool {
    !matches!(
      self.cur().slot_abs[slot as usize],
      SlotState::Unset | SlotState::Uniform
    )
  }

  fn expr_is_uniform(&self, expr: &Expr) -> bool {
    match expr {
      Expr::Ident { res, .. } => match res {
        VarRes::Local(slot) => !self.slot_is_abstract(*slot),
        _ => true,
      },
      Expr::Literal { .. } => true,
      Expr::BinOp { lhs, rhs, .. } => self.expr_is_uniform(lhs) && self.expr_is_uniform(rhs),
      Expr::PrefixOp { expr, .. } => self.expr_is_uniform(expr),
      Expr::Range { start, end, .. } => {
        self.expr_is_uniform(start) && end.as_deref().map_or(true, |e| self.expr_is_uniform(e))
      }
      Expr::StaticFieldAccess { lhs, .. } => self.expr_is_uniform(lhs),
      Expr::FieldAccess {
        lhs, field, field2, ..
      } => {
        self.expr_is_uniform(lhs)
          && self.expr_is_uniform(field)
          && field2.as_deref().map_or(true, |f| self.expr_is_uniform(f))
      }
      Expr::Call { call, .. } => {
        let target_uniform = match call.target_res {
          VarRes::Local(slot) => !self.slot_is_abstract(slot),
          _ => true,
        };
        target_uniform
          && call.args.iter().all(|a| self.expr_is_uniform(a))
          && call.kwargs.values().all(|a| self.expr_is_uniform(a))
      }
      Expr::Closure { resolved, .. } => resolved.as_ref().is_some_and(|meta| {
        meta.captures.iter().all(|(_, from)| match from {
          CaptureFrom::Local(slot) => !self.slot_is_abstract(*slot),
          _ => true,
        })
      }),
      Expr::ArrayLiteral { elements, .. } => elements.iter().all(|e| self.expr_is_uniform(e)),
      Expr::MapLiteral { entries, .. } => entries.iter().all(|e| self.expr_is_uniform(e.expr())),
      Expr::Conditional {
        cond,
        then,
        else_if_exprs,
        else_expr,
        ..
      } => {
        self.expr_is_uniform(cond)
          && self.expr_is_uniform(then)
          && else_if_exprs
            .iter()
            .all(|(c, e)| self.expr_is_uniform(c) && self.expr_is_uniform(e))
          && else_expr
            .as_deref()
            .map_or(true, |e| self.expr_is_uniform(e))
      }
      Expr::Block { statements, .. } => statements
        .iter()
        .flat_map(|s| s.exprs())
        .all(|e| self.expr_is_uniform(e)),
    }
  }

  fn eval_uniform_now(&self, expr: &Expr) -> Result<Value, CErr> {
    let cur = self.cur();
    let frame = FrameEnv {
      slots: &cur.mirror,
      captures: &cur.captures,
      self_ref: &cur.self_ref,
    };
    self.ctx.eval_expr_env(expr, &frame).map_err(CErr::Err)
  }

  fn emit_uniform(&mut self, expr: &Expr) -> Result<AbsVal, CErr> {
    let val = self.eval_uniform_now(expr)?;
    // A bare capture read takes the keyed form so repeated uses (`sample(src, …)` twice)
    // share one table entry and downstream CSE can see them as equal.
    let src = match expr {
      Expr::Ident {
        res: VarRes::Capture(ix),
        ..
      } => UniSrc::Capture(*ix),
      _ => UniSrc::Expr(expr.clone()),
    };
    Ok(AbsVal::U(self.push_uni(src, val, None, None)))
  }

  // -------------------------------------------------------------------------------------
  // Varying-spine compilation
  // -------------------------------------------------------------------------------------

  fn compile_expr(&mut self, expr: &Expr) -> Result<AbsVal, CErr> {
    self.check_size(expr.loc())?;
    if self.expr_is_uniform(expr) {
      return self.emit_uniform(expr);
    }
    let loc = expr.loc();
    match expr {
      Expr::Ident { res, .. } => match res {
        VarRes::Local(slot) => match &self.cur().slot_abs[*slot as usize] {
          SlotState::Varying(vv) => Ok(AbsVal::V(vv.clone())),
          SlotState::Seq(s) => Ok(AbsVal::Seq(s.clone())),
          SlotState::Lit(_) => bail(
            "closure used as a value in a texel body (only calls are supported)",
            loc,
          ),
          _ => unreachable!("non-varying ident reached varying compile"),
        },
        _ => unreachable!(),
      },
      Expr::BinOp { op, lhs, rhs, .. } => {
        let kind = match op {
          BinOp::Add => OpKind::Add,
          BinOp::Sub => OpKind::Sub,
          BinOp::Mul => OpKind::Mul,
          BinOp::Div => OpKind::Div,
          BinOp::Mod => OpKind::Mod,
          BinOp::Pipeline => {
            let l = self.compile_expr(lhs)?;
            return self.lower_pipeline(l, rhs, loc);
          }
          BinOp::Gt | BinOp::Lt | BinOp::Gte | BinOp::Lte | BinOp::Eq | BinOp::Neq => {
            return self.lower_compare(*op, lhs, rhs, loc)
          }
          BinOp::And | BinOp::Or => return self.lower_logic(*op, lhs, rhs, loc),
          BinOp::Map => {
            let l = self.compile_expr(lhs)?;
            return self.lower_seq_call(
              "map",
              std::slice::from_ref(&**rhs),
              &FxHashMap::default(),
              Some(l),
              loc,
            );
          }
          other => {
            return bail(
              format!("unsupported operator `{other:?}` on varying values"),
              loc,
            )
          }
        };
        let l = self.compile_expr(lhs)?;
        let r = self.compile_expr(rhs)?;
        self.lower_arith(*op, kind, l, r, loc)
      }
      Expr::PrefixOp {
        op, expr: inner, ..
      } => match op {
        PrefixOp::Neg => {
          let v = self.compile_expr(inner)?;
          self.elementwise(OpKind::Neg, &v)
        }
        PrefixOp::Pos => self.compile_expr(inner),
        PrefixOp::Not => {
          let v = self.compile_expr(inner)?;
          let m = self.mask_src(&v, loc)?;
          Ok(VV::mask(self.push_op(
            OpKind::Not,
            m,
            Src::Const(0.),
            Src::Const(0.),
          )))
        }
      },
      Expr::StaticFieldAccess { lhs, field, .. } => {
        let v = self.compile_expr(lhs)?;
        self.lower_swizzle(&v, field, loc)
      }
      Expr::Call { call, .. } => self.lower_call(call, loc),
      Expr::Block { statements, .. } => self.compile_statements(statements, loc),
      Expr::Conditional {
        cond,
        then,
        else_if_exprs,
        else_expr,
        ..
      } => self.lower_conditional(cond, then, else_if_exprs, else_expr.as_deref(), loc),
      Expr::FieldAccess {
        lhs, field, field2, ..
      } => {
        let l = self.compile_expr(lhs)?;
        let AbsVal::Seq(s) = &l else {
          return bail("indexing a varying value", loc);
        };
        if field2.is_some() {
          return bail("two-index access on a sequence", loc);
        }
        if !s.eager {
          return bail(
            "indexing a lazy sequence (the scalar path requires `collect` first)",
            loc,
          );
        }
        let f = self.compile_expr(field)?;
        let Some(Value::Int(i)) = self.const_uniform_value(&f) else {
          return bail("sequence index must be an int literal", loc);
        };
        match usize::try_from(i).ok().and_then(|i| s.els.get(i)) {
          Some(e) => Ok(e.clone()),
          None => bail(
            format!("sequence index {i} out of bounds (len {})", s.els.len()),
            loc,
          ),
        }
      }
      Expr::Range { .. } => bail("range over a varying value", loc),
      Expr::ArrayLiteral { elements, .. } => {
        let mut els = Vec::with_capacity(elements.len());
        for e in elements {
          els.push(self.compile_expr(e)?);
        }
        Ok(AbsVal::Seq(AbsSeq {
          els: Rc::new(els),
          eager: true,
        }))
      }
      Expr::MapLiteral { .. } => bail("map literal containing varying values", loc),
      Expr::Closure { .. } => bail("closure capturing a varying value", loc),
      Expr::Literal { .. } => unreachable!("literals are uniform"),
    }
  }

  /// Register, uniform, and guard indices are `u16`; a pathological body must bail, not wrap.
  fn check_size(&self, loc: SourceLoc) -> Result<(), CErr> {
    if self.n_regs as usize > MAX_PLAN_SLOTS
      || self.unis.len() > MAX_PLAN_SLOTS
      || self.guards.len() > MAX_PLAN_SLOTS
    {
      return bail("texel body too large to vectorize", loc);
    }
    Ok(())
  }

  fn elementwise(&mut self, kind: OpKind, v: &AbsVal) -> Result<AbsVal, CErr> {
    let ar = self.arity(v)?;
    let mut chans = ArrayVec::new();
    for c in 0..ar {
      let s = self.chan(v, c);
      chans.push(self.push_op(kind, s, Src::Const(0.), Src::Const(0.)));
    }
    Ok(VV::num(chans))
  }

  fn lower_arith(
    &mut self,
    op: BinOp,
    kind: OpKind,
    l: AbsVal,
    r: AbsVal,
    loc: SourceLoc,
  ) -> Result<AbsVal, CErr> {
    for side in [&l, &r] {
      if let AbsVal::U(uix) = side {
        if num_arity(self.uni_val(*uix)).is_none() {
          return bail(
            "non-numeric uniform operand of a varying arithmetic op",
            loc,
          );
        }
      }
    }
    // Pin acceptance to the interpreter's own def resolution; a combination it rejects
    // (e.g. `vec3 % float`) must produce the same error here.
    let (lv, rv) = (self.typed_value(&l), self.typed_value(&r));
    op.resolve_def_ix(self.ctx, &lv, &rv)
      .map_err(|e| CErr::Err(e.wrap(format!("Error applying binary operator `{op:?}`"))))?;
    let (la, ra) = (self.arity(&l)?, self.arity(&r)?);
    let out_ar = la.max(ra);
    let mut chans = ArrayVec::new();
    for c in 0..out_ar {
      let a = self.chan(&l, c.min(la - 1));
      let b = self.chan(&r, c.min(ra - 1));
      chans.push(self.push_op(kind, a, b, Src::Const(0.)));
    }
    Ok(VV::num(chans))
  }

  fn lower_swizzle(&mut self, v: &AbsVal, field: &str, loc: SourceLoc) -> Result<AbsVal, CErr> {
    // A varying-classified expression can still produce a uniform value (a block whose
    // non-final statement is varying); swizzle it through the interpreter, once per run.
    let vv = match v {
      AbsVal::V(vv) => vv,
      AbsVal::Seq(_) => return bail("swizzle on a sequence", loc),
      AbsVal::U(uix) => {
        let val = self
          .ctx
          .eval_static_field_access(self.uni_val(*uix), field)
          .map_err(CErr::Err)?;
        return Ok(AbsVal::U(self.push_uni(
          UniSrc::SwizzleOf {
            of: *uix,
            field: field.to_owned(),
          },
          val,
          None,
          None,
        )));
      }
    };
    if vv.mask {
      return bail("swizzle on a bool", loc);
    }
    let ar = vv.chans.len();
    let mut chans = ArrayVec::new();
    for ch in field.chars() {
      let ix = match ch {
        'x' | 'r' => 0usize,
        'y' | 'g' => 1,
        'z' | 'b' => 2,
        'w' | 'a' => 3,
        _ => return bail(format!("unknown swizzle char `{ch}`"), loc),
      };
      if ix >= ar || chans.len() == 4 {
        return bail(format!("invalid swizzle `.{field}` for arity {ar}"), loc);
      }
      chans.push(vv.chans[ix]);
    }
    if chans.is_empty() {
      return bail("empty swizzle", loc);
    }
    Ok(VV::num(chans))
  }

  fn lower_pipeline(&mut self, lhs: AbsVal, rhs: &Expr, loc: SourceLoc) -> Result<AbsVal, CErr> {
    if let Expr::Call { call, .. } = rhs {
      if let Some((c, args, kwargs)) = self.peek_flat(call, loc) {
        if let Some(name) = seq_dispatch(&c, loc)? {
          return self.lower_seq_call(name, &args, &kwargs, Some(lhs), loc);
        }
      }
    }
    if !self.expr_is_uniform(rhs) {
      return bail("varying callee in pipeline", loc);
    }
    let cb = self.callback(rhs, loc)?;
    if let Cb::Callable { c, uix: None } = &cb {
      let (inner, args, kwargs) = flatten_partial(c, loc);
      if let Some(name) = seq_dispatch(&inner, loc)? {
        return self.lower_seq_call(name, &args, &kwargs, Some(lhs), loc);
      }
    }
    self.invoke(&cb, vec![lhs], loc)
  }

  fn lower_call(&mut self, call: &FunctionCall, loc: SourceLoc) -> Result<AbsVal, CErr> {
    if let Some((c, args, kwargs)) = self.peek_flat(call, loc) {
      if let Some(name) = seq_dispatch(&c, loc)? {
        return self.lower_seq_call(name, &args, &kwargs, None, loc);
      }
    }
    let mut args = Vec::with_capacity(call.args.len());
    for a in &call.args {
      args.push(self.compile_expr(a)?);
    }
    let mut kwargs = FxHashMap::default();
    for (k, v) in &call.kwargs {
      kwargs.insert(*k, self.compile_expr(v)?);
    }

    let (callable, callee_uix): (Rc<Callable>, Option<u16>) = match &call.target {
      FunctionCallTarget::Literal(c) => (Rc::clone(c), None),
      FunctionCallTarget::Name(_) => match call.target_res {
        VarRes::Capture(ix) => {
          let val = self.cur().captures[ix as usize].clone();
          let Value::Callable(c) = &val else {
            return bail("call target capture is not a callable", loc);
          };
          let c = c.clone();
          let uix = self.push_uni(UniSrc::Capture(ix), val, None, None);
          (c, Some(uix))
        }
        VarRes::Local(slot) => {
          if let SlotState::Lit(lit) = &self.cur().slot_abs[slot as usize] {
            let lit = Rc::clone(lit);
            return self.inline_literal(&lit, args, kwargs, loc);
          }
          if self.slot_is_abstract(slot) {
            return bail("varying call target", loc);
          }
          let val = self.cur().mirror.borrow()[slot as usize].clone();
          let Value::Callable(c) = &val else {
            return bail("call target local is not a callable", loc);
          };
          let c = c.clone();
          let uix = self.push_uni(UniSrc::Slot(slot), val, None, None);
          (c, Some(uix))
        }
        VarRes::SelfRef => return bail("recursive call on varying values", loc),
        VarRes::Unresolved => return bail("unresolved call target", loc),
      },
    };
    self.lower_callable_call(&callable, callee_uix, args, kwargs, loc)
  }

  fn lower_callable_call(
    &mut self,
    callable: &Rc<Callable>,
    callee_uix: Option<u16>,
    args: Vec<AbsVal>,
    kwargs: FxHashMap<Sym, AbsVal>,
    loc: SourceLoc,
  ) -> Result<AbsVal, CErr> {
    match &**callable {
      Callable::Builtin {
        fn_entry_ix,
        pre_resolved_signature,
        ..
      } => {
        if let Some(uix) = callee_uix {
          self.unis[uix as usize].shape = UniShape::Builtin(*fn_entry_ix);
        }
        self.lower_builtin(
          *fn_entry_ix,
          pre_resolved_signature.as_ref(),
          args,
          kwargs,
          loc,
        )
      }
      Callable::Closure(inner) => {
        if let Some(uix) = callee_uix {
          self.unis[uix as usize].shape = UniShape::ClosureBody(inner.resolved.id);
        }
        self.inline_closure(callable, inner, callee_uix, args, kwargs, loc)
      }
      Callable::Dynamic { name, inner } => {
        if inner.is_side_effectful() || inner.is_rng_dependent() {
          return bail(format!("effectful/rng dynamic callable `{name}`"), loc);
        }
        let Some(out_ar) = dynamic_out_arity(callable) else {
          return bail(
            format!("dynamic callable `{name}` without a usable return type hint"),
            loc,
          );
        };
        if !kwargs.is_empty() {
          return bail(format!("kwargs on dynamic callable `{name}`"), loc);
        }
        if let Some(uix) = callee_uix {
          self.unis[uix as usize].shape = UniShape::Dynamic(out_ar);
        }
        let mut arg_srcs = Vec::with_capacity(args.len());
        for a in &args {
          let ar = self.arity(a)?;
          let mut chans = ArrayVec::new();
          for c in 0..ar {
            chans.push(self.chan(a, c));
          }
          arg_srcs.push(chans);
        }
        let mut dst = ArrayVec::new();
        for _ in 0..out_ar {
          dst.push(self.alloc_reg());
        }
        let callee = match callee_uix {
          Some(uix) => DynCallee::Uni(uix),
          None => DynCallee::Baked(Rc::clone(callable)),
        };
        self.push_step(Step::Dyn(DynStep {
          callee,
          args: arg_srcs,
          dst: dst.clone(),
        }));
        Ok(VV::num(dst.iter().map(|r| Src::Reg(*r)).collect()))
      }
      Callable::PartiallyAppliedFn(_) => {
        if callee_uix.is_some() {
          return bail("partially-applied callable from a non-literal value", loc);
        }
        let (inner, bound, bound_kw) = flatten_partial(callable, loc);
        let mut all = Vec::with_capacity(bound.len() + args.len());
        for e in &bound {
          all.push(self.compile_expr(e)?);
        }
        all.extend(args);
        let mut kw = FxHashMap::default();
        for (k, e) in &bound_kw {
          kw.insert(*k, self.compile_expr(e)?);
        }
        kw.extend(kwargs);
        self.lower_callable_call(&inner, None, all, kw, loc)
      }
      _ => bail("unsupported callee kind on varying values", loc),
    }
  }

  /// Inline a pure captured closure's body: its params bind to the call's abstract args,
  /// its statements compile into the same plan under a fresh frame, and its result value
  /// flows back to the caller's spine.
  fn inline_closure(
    &mut self,
    callable: &Rc<Callable>,
    inner: &Closure,
    callee_uix: Option<u16>,
    args: Vec<AbsVal>,
    kwargs: FxHashMap<Sym, AbsVal>,
    loc: SourceLoc,
  ) -> Result<AbsVal, CErr> {
    let meta = &inner.resolved;
    if self.inline_stack.contains(&meta.id) {
      return bail("recursive closure call", loc);
    }
    if self.inline_stack.len() >= MAX_INLINE_DEPTH {
      return bail("closure inlining too deep", loc);
    }
    prefilter(inner, None)?;
    for param in inner.params.iter() {
      if !matches!(param.ident, crate::ast::DestructurePattern::Ident(_)) {
        return bail("destructuring param on inlined closure", loc);
      }
    }

    self.inline_stack.push(meta.id);
    self.push_frame(
      meta.n_slots,
      callee_uix,
      callee_uix.is_none().then(|| Rc::clone(callable)),
      Rc::clone(&inner.captures),
      Rc::clone(callable),
    );

    let result = self.bind_and_compile_inline(inner, args, kwargs, loc);

    self.pop_frame();
    self.inline_stack.pop();
    let result = result?;
    self.check_return_hint(&result, inner.return_type_hint, loc)?;
    Ok(result)
  }

  fn check_return_hint(
    &mut self,
    result: &AbsVal,
    hint: Option<ArgType>,
    loc: SourceLoc,
  ) -> Result<(), CErr> {
    let Some(hint) = hint else { return Ok(()) };
    match result {
      AbsVal::V(vv) => {
        if !Self::hint_fits(hint, vv) {
          return bail("return type hint mismatch on inlined closure", loc);
        }
      }
      AbsVal::U(uix) => {
        if hint.validate_val(self.uni_val(*uix)).is_err() {
          return bail("return type hint violation on inlined closure", loc);
        }
        let entry = &mut self.unis[*uix as usize];
        entry.hint = entry.hint.or(Some(hint));
      }
      AbsVal::Seq(_) => {
        if !matches!(hint, ArgType::Any | ArgType::Sequence) {
          return bail("return type hint mismatch on inlined closure", loc);
        }
      }
    }
    Ok(())
  }

  fn bind_and_compile_inline(
    &mut self,
    inner: &Closure,
    args: Vec<AbsVal>,
    kwargs: FxHashMap<Sym, AbsVal>,
    loc: SourceLoc,
  ) -> Result<AbsVal, CErr> {
    let meta = &inner.resolved;
    // Mirrors invoke_closure_resolved's binding: kwargs by param name first, then
    // positionals, then defaults; a missing required arg partially applies — bail.
    let mut pos_ix = 0usize;
    for (param_ix, param) in inner.params.iter().enumerate() {
      let slot = meta.param_slots[param_ix];
      let crate::ast::DestructurePattern::Ident(param_name) = &param.ident else {
        unreachable!("checked above");
      };
      let bound = if let Some(v) = kwargs.get(param_name) {
        Some(v.clone())
      } else if pos_ix < args.len() {
        pos_ix += 1;
        Some(args[pos_ix - 1].clone())
      } else {
        None
      };
      let bound = match bound {
        Some(v) => v,
        None => match &param.default_val {
          Some(default_expr) => self.compile_expr(default_expr)?,
          None => return bail("partial application of an inlined closure", loc),
        },
      };
      self.bind_slot(slot, bound, param.type_hint, loc)?;
    }
    self.compile_statements(&inner.body.0, loc)
  }

  /// Bind a frame slot to an abstract value (inlined-closure params), checking its hint.
  fn bind_slot(
    &mut self,
    slot: u16,
    bound: AbsVal,
    hint: Option<ArgType>,
    loc: SourceLoc,
  ) -> Result<(), CErr> {
    match bound {
      AbsVal::U(src_uix) => {
        if hint.is_some_and(|h| h.validate_val(self.uni_val(src_uix)).is_err()) {
          return bail("param type hint violation on inlined closure", loc);
        }
        let val = self.uni_val(src_uix).clone();
        self.cur().mirror.borrow_mut()[slot as usize] = val.clone();
        self.push_uni(UniSrc::UniRef(src_uix), val, Some(slot), hint);
        self.cur_mut().slot_abs[slot as usize] = SlotState::Uniform;
      }
      AbsVal::V(vv) => {
        if hint.is_some_and(|h| !Self::hint_fits(h, &vv)) {
          return bail("param type hint mismatch on inlined closure", loc);
        }
        self.cur_mut().slot_abs[slot as usize] = SlotState::Varying(vv);
      }
      AbsVal::Seq(seq) => {
        if hint.is_some_and(|h| !matches!(h, ArgType::Any | ArgType::Sequence)) {
          return bail("param type hint mismatch on inlined closure", loc);
        }
        self.cur_mut().slot_abs[slot as usize] = SlotState::Seq(seq);
      }
    }
    Ok(())
  }

  /// Inline a closure literal by renaming its slots into the current frame: body locals get
  /// fresh slots appended to the frame, captures resolve to the enclosing frame's own
  /// locals/captures (so they may be varying), and the renamed statements compile in place.
  fn inline_literal(
    &mut self,
    lit: &Expr,
    args: Vec<AbsVal>,
    kwargs: FxHashMap<Sym, AbsVal>,
    loc: SourceLoc,
  ) -> Result<AbsVal, CErr> {
    let Expr::Closure {
      params,
      body,
      return_type_hint,
      resolved: Some(meta),
      ..
    } = lit
    else {
      return bail("unresolved closure literal", loc);
    };
    if self.lit_depth >= MAX_INLINE_DEPTH {
      return bail("closure inlining too deep", loc);
    }
    let base = self.cur().slot_abs.len();
    let n_slots = base + meta.n_slots as usize;
    if n_slots > MAX_PLAN_SLOTS.min(u16::MAX as usize) {
      return bail("texel body too large to vectorize", loc);
    }
    let mut caps = Vec::with_capacity(meta.captures.len());
    for (_, from) in &meta.captures {
      caps.push(match from {
        CaptureFrom::Local(s) => VarRes::Local(*s),
        CaptureFrom::Capture(ix) => VarRes::Capture(*ix),
        CaptureFrom::SelfRef | CaptureFrom::DefScope(_) => {
          return bail("closure literal referencing its enclosing closure", loc)
        }
      });
    }
    let ren = Renamer {
      base: base as u16,
      caps: &caps,
    };
    let stmts = body
      .0
      .iter()
      .map(|s| ren.stmt(s))
      .collect::<Result<Vec<_>, CErr>>()?;
    {
      let f = self.cur_mut();
      f.slot_abs.resize(n_slots, SlotState::Unset);
      f.mirror.borrow_mut().resize(n_slots, Value::Nil);
    }
    let mut pos = 0usize;
    for (i, param) in params.iter().enumerate() {
      let DestructurePattern::Ident(name) = &param.ident else {
        return bail("destructuring param on inlined closure", loc);
      };
      let slot = base as u16 + meta.param_slots[i];
      let bound = if let Some(v) = kwargs.get(name) {
        v.clone()
      } else if pos < args.len() {
        pos += 1;
        args[pos - 1].clone()
      } else {
        match &param.default_val {
          Some(d) => {
            let d = ren.expr(d)?;
            self.compile_expr(&d)?
          }
          None => return bail("partial application of an inlined closure", loc),
        }
      };
      self.bind_slot(slot, bound, param.type_hint, loc)?;
    }
    self.lit_depth += 1;
    let result = self.compile_statements(&stmts, loc);
    self.lit_depth -= 1;
    let result = result?;
    self.check_return_hint(&result, *return_type_hint, loc)?;
    Ok(result)
  }

  fn lower_builtin(
    &mut self,
    fn_entry_ix: usize,
    pre_resolved: Option<&crate::PreResolvedSignature>,
    args: Vec<AbsVal>,
    kwargs: FxHashMap<Sym, AbsVal>,
    loc: SourceLoc,
  ) -> Result<AbsVal, CErr> {
    let (name, def) = &fn_sigs().entries[fn_entry_ix];
    const WHITELIST: &[&str] = &[
      "sin",
      "cos",
      "tan",
      "asin",
      "acos",
      "atan",
      "sqrt",
      "exp",
      "log2",
      "floor",
      "ceil",
      "round",
      "fract",
      "trunc",
      "sigmoid",
      "abs",
      "pow",
      "atan2",
      "min",
      "max",
      "clamp",
      "smoothstep",
      "lerp",
      "len",
      "dot",
      "distance",
      "normalize",
      "vec2",
      "vec3",
      "vec4",
      "fbm",
      "linearstep",
      "remap",
      "add",
      "sub",
      "mul",
      "div",
      "sample",
    ];
    if !WHITELIST.contains(name) {
      return bail(format!("builtin `{name}` is not vectorizable"), loc);
    }

    let (def_ix, arg_refs): (usize, Vec<ArgRef>) = match pre_resolved {
      Some(sig) => (sig.def_ix, sig.arg_refs.clone()),
      None => {
        let vals: Vec<Value> = args.iter().map(|a| self.typed_value(a)).collect();
        let mut kw_vals = FxHashMap::default();
        for (k, v) in &kwargs {
          kw_vals.insert(*k, self.typed_value(v));
        }
        match get_args(self.ctx, name, def.signatures, &vals, &kw_vals) {
          Ok(GetArgsOutput::Valid { def_ix, arg_refs }) => (def_ix, arg_refs.into_vec()),
          Ok(GetArgsOutput::PartiallyApplied) => {
            return bail(
              format!("partial application of `{name}` on varying values"),
              loc,
            )
          }
          Err(e) => return Err(CErr::Err(e)),
        }
      }
    };

    let mut arg = |compiler: &mut Self, i: usize| -> AbsVal {
      match &arg_refs[i] {
        ArgRef::Positional(ix) => args[*ix].clone(),
        ArgRef::Keyword(sym) => kwargs[sym].clone(),
        ArgRef::Default(v) => {
          AbsVal::U(compiler.push_uni(UniSrc::Const(v.clone()), v.clone(), None, None))
        }
      }
    };

    match (*name, def_ix) {
      // The named arithmetic builtins share the operators' signature tables.
      ("add" | "sub" | "mul" | "div", _) => {
        let (a, b) = (arg(self, 0), arg(self, 1));
        let (op, kind) = match *name {
          "add" => (BinOp::Add, OpKind::Add),
          "sub" => (BinOp::Sub, OpKind::Sub),
          "mul" => (BinOp::Mul, OpKind::Mul),
          _ => (BinOp::Div, OpKind::Div),
        };
        self.lower_arith(op, kind, a, b, loc)
      }
      ("sin", _) => {
        let x = arg(self, 0);
        self.elementwise(OpKind::Sin, &x)
      }
      ("cos", _) => {
        let x = arg(self, 0);
        self.elementwise(OpKind::Cos, &x)
      }
      ("tan", _) => {
        let x = arg(self, 0);
        self.elementwise(OpKind::Tan, &x)
      }
      ("asin", _) => {
        let x = arg(self, 0);
        self.elementwise(OpKind::Asin, &x)
      }
      ("acos", _) => {
        let x = arg(self, 0);
        self.elementwise(OpKind::Acos, &x)
      }
      ("atan", _) => {
        let x = arg(self, 0);
        self.elementwise(OpKind::Atan, &x)
      }
      ("sqrt", _) => {
        let x = arg(self, 0);
        self.elementwise(OpKind::Sqrt, &x)
      }
      ("exp", _) => {
        let x = arg(self, 0);
        self.elementwise(OpKind::Exp, &x)
      }
      ("log2", _) => {
        let x = arg(self, 0);
        self.elementwise(OpKind::Log2, &x)
      }
      ("floor", _) => {
        let x = arg(self, 0);
        self.elementwise(OpKind::Floor, &x)
      }
      ("ceil", _) => {
        let x = arg(self, 0);
        self.elementwise(OpKind::Ceil, &x)
      }
      ("round", _) => {
        let x = arg(self, 0);
        self.elementwise(OpKind::Round, &x)
      }
      ("fract", _) => {
        let x = arg(self, 0);
        self.elementwise(OpKind::Fract, &x)
      }
      ("trunc", _) => {
        let x = arg(self, 0);
        self.elementwise(OpKind::Trunc, &x)
      }
      ("sigmoid", _) => {
        let x = arg(self, 0);
        self.elementwise(OpKind::Sigmoid, &x)
      }
      ("abs", 1..=3 | 5) => {
        let x = arg(self, 0);
        self.elementwise(OpKind::Abs, &x)
      }
      ("pow", 0..=2 | 4) => {
        let base = arg(self, 0);
        let expo = arg(self, 1);
        let ar = self.arity(&base)?;
        let e = self.chan(&expo, 0);
        let mut chans = ArrayVec::new();
        for c in 0..ar {
          let b = self.chan(&base, c);
          chans.push(self.push_op(OpKind::Pow, b, e, Src::Const(0.)));
        }
        Ok(VV::num(chans))
      }
      ("atan2", 0) => {
        let y = arg(self, 0);
        let x = arg(self, 1);
        let (ys, xs) = (self.chan(&y, 0), self.chan(&x, 0));
        Ok(VV::num(
          [self.push_op(OpKind::Atan2, ys, xs, Src::Const(0.))]
            .into_iter()
            .collect(),
        ))
      }
      ("atan2", 1) => {
        let v = arg(self, 0);
        let (ys, xs) = (self.chan(&v, 1), self.chan(&v, 0));
        Ok(VV::num(
          [self.push_op(OpKind::Atan2, ys, xs, Src::Const(0.))]
            .into_iter()
            .collect(),
        ))
      }
      ("min", 1..=3 | 7) | ("max", 1..=3 | 7) => {
        let kind = if *name == "min" {
          OpKind::Min
        } else {
          OpKind::Max
        };
        let a = arg(self, 0);
        let b = arg(self, 1);
        let ar = self.arity(&a)?;
        let mut chans = ArrayVec::new();
        for c in 0..ar {
          let (x, y) = (self.chan(&a, c), self.chan(&b, c.min(self.arity(&b)? - 1)));
          chans.push(self.push_op(kind, x, y, Src::Const(0.)));
        }
        Ok(VV::num(chans))
      }
      ("clamp", 1..=3 | 5) => {
        let lo = arg(self, 0);
        let hi = arg(self, 1);
        let x = arg(self, 2);
        let ar = self.arity(&x)?;
        let (ls, hs) = (self.chan(&lo, 0), self.chan(&hi, 0));
        let mut chans = ArrayVec::new();
        for c in 0..ar {
          let xs = self.chan(&x, c);
          chans.push(self.push_op(OpKind::Clamp, xs, ls, hs));
        }
        Ok(VV::num(chans))
      }
      ("smoothstep", 0) => {
        let e0 = arg(self, 0);
        let e1 = arg(self, 1);
        let x = arg(self, 2);
        let (e0s, e1s, xs) = (self.chan(&e0, 0), self.chan(&e1, 0), self.chan(&x, 0));
        Ok(VV::num(
          [self.push_op(OpKind::SmoothStep, xs, e0s, e1s)]
            .into_iter()
            .collect(),
        ))
      }
      ("lerp", 0..=3) => {
        let t = arg(self, 0);
        let a = arg(self, 1);
        let b = arg(self, 2);
        let kind = if def_ix == 1 {
          OpKind::LerpF
        } else {
          OpKind::LerpV
        };
        let ar = self.arity(&a)?;
        let ts = self.chan(&t, 0);
        let mut chans = ArrayVec::new();
        for c in 0..ar {
          let (x, y) = (self.chan(&a, c), self.chan(&b, c));
          chans.push(self.push_op(kind, x, y, ts));
        }
        Ok(VV::num(chans))
      }
      ("len", 0..=1 | 6) => {
        let v = arg(self, 0);
        let s = self.sum_of_products(&v, &v)?;
        Ok(VV::num(
          [self.push_op(OpKind::Sqrt, s, Src::Const(0.), Src::Const(0.))]
            .into_iter()
            .collect(),
        ))
      }
      ("dot", 0..=1 | 3) => {
        let a = arg(self, 0);
        let b = arg(self, 1);
        let s = self.sum_of_products(&a, &b)?;
        Ok(VV::num([s].into_iter().collect()))
      }
      ("distance", 0..=1 | 3) => {
        let a = arg(self, 0);
        let b = arg(self, 1);
        let ar = self.arity(&a)?;
        let mut d = ArrayVec::new();
        for c in 0..ar {
          let (x, y) = (self.chan(&a, c), self.chan(&b, c));
          d.push(self.push_op(OpKind::Sub, x, y, Src::Const(0.)));
        }
        let d = VV::num(d);
        let s = self.sum_of_products(&d, &d)?;
        Ok(VV::num(
          [self.push_op(OpKind::Sqrt, s, Src::Const(0.), Src::Const(0.))]
            .into_iter()
            .collect(),
        ))
      }
      ("normalize", 0..=1 | 3) => {
        let v = arg(self, 0);
        let ar = self.arity(&v)?;
        let s = self.sum_of_products(&v, &v)?;
        let norm = self.push_op(OpKind::Sqrt, s, Src::Const(0.), Src::Const(0.));
        let mut chans = ArrayVec::new();
        for c in 0..ar {
          let x = self.chan(&v, c);
          chans.push(self.push_op(OpKind::Div, x, norm, Src::Const(0.)));
        }
        Ok(VV::num(chans))
      }
      ("vec2", 0) => {
        let (x, y) = (arg(self, 0), arg(self, 1));
        self.construct(&[(x, 0), (y, 0)])
      }
      ("vec2", 1) => {
        let x = arg(self, 0);
        self.construct(&[(x.clone(), 0), (x, 0)])
      }
      ("vec3", 0) => {
        let (x, y, z) = (arg(self, 0), arg(self, 1), arg(self, 2));
        self.construct(&[(x, 0), (y, 0), (z, 0)])
      }
      ("vec3", 1) => {
        let (xy, z) = (arg(self, 0), arg(self, 1));
        self.construct(&[(xy.clone(), 0), (xy, 1), (z, 0)])
      }
      ("vec3", 2) => {
        let (x, yz) = (arg(self, 0), arg(self, 1));
        self.construct(&[(x, 0), (yz.clone(), 0), (yz, 1)])
      }
      ("vec3", 3) => {
        let x = arg(self, 0);
        self.construct(&[(x.clone(), 0), (x.clone(), 0), (x, 0)])
      }
      ("vec4", 0) => {
        let (x, y, z, w) = (arg(self, 0), arg(self, 1), arg(self, 2), arg(self, 3));
        self.construct(&[(x, 0), (y, 0), (z, 0), (w, 0)])
      }
      ("vec4", 1) => {
        let (xyz, w) = (arg(self, 0), arg(self, 1));
        self.construct(&[(xyz.clone(), 0), (xyz.clone(), 1), (xyz, 2), (w, 0)])
      }
      ("vec4", 2) => {
        let (xy, zw) = (arg(self, 0), arg(self, 1));
        self.construct(&[(xy.clone(), 0), (xy, 1), (zw.clone(), 0), (zw, 1)])
      }
      ("vec4", 3) => {
        let x = arg(self, 0);
        self.construct(&[(x.clone(), 0), (x.clone(), 0), (x.clone(), 0), (x, 0)])
      }
      ("linearstep", 0) => {
        let (e0, e1, x) = (arg(self, 0), arg(self, 1), arg(self, 2));
        let (e0s, e1s, xs) = (self.chan(&e0, 0), self.chan(&e1, 0), self.chan(&x, 0));
        Ok(VV::num(
          [self.push_op(OpKind::LinearStep, xs, e0s, e1s)]
            .into_iter()
            .collect(),
        ))
      }
      ("remap", _) => {
        // The scalar formula branches on `in_hi != in_lo` and a clamp flag; that structure
        // is baked into the cached plan, so the bounds must be plan-constant (AST literals
        // or defaults) — ordinary uniforms can change value across cache-hit runs.
        let bounds = [arg(self, 0), arg(self, 1), arg(self, 2), arg(self, 3)];
        let x = arg(self, 4);
        let clamp = arg(self, 5);
        let mut k = [0f32; 4];
        for (i, b) in bounds.iter().enumerate() {
          match self.const_uniform_value(b).and_then(|v| v.as_float()) {
            Some(f) => k[i] = f,
            None => return bail("remap with non-constant bounds", loc),
          }
        }
        let Some(cl) = self.const_uniform_value(&clamp).and_then(|v| v.as_bool()) else {
          return bail("remap with non-constant clamp flag", loc);
        };
        let [in_lo, in_hi, out_lo, out_hi] = k;
        let ar = self.arity(&x)?;
        let mut chans = ArrayVec::new();
        for c in 0..ar {
          let out = if in_hi != in_lo {
            let xs = self.chan(&x, c);
            let t = self.push_op(OpKind::Sub, xs, Src::Const(in_lo), Src::Const(0.));
            let mut t = self.push_op(OpKind::Div, t, Src::Const(in_hi - in_lo), Src::Const(0.));
            if cl {
              t = self.push_op(OpKind::Clamp, t, Src::Const(0.), Src::Const(1.));
            }
            let m = self.push_op(OpKind::Mul, Src::Const(out_hi - out_lo), t, Src::Const(0.));
            self.push_op(OpKind::Add, Src::Const(out_lo), m, Src::Const(0.))
          } else {
            Src::Const(out_lo + (out_hi - out_lo) * 0.)
          };
          chans.push(out);
        }
        Ok(VV::num(chans))
      }
      ("fbm", _) => self.lower_fbm(def_ix, &mut arg, loc),
      ("sample", _) => self.lower_sample(&mut arg, loc),
      _ => bail(format!("`{name}` def {def_ix} is not vectorizable"), loc),
    }
  }

  /// nalgebra's `dot` (hence `len`/`distance`/`normalize`): left-to-right for 2/3-vectors,
  /// but the 4-vector unroll accumulates `(p0 + p2) + (p1 + p3)`.
  fn sum_of_products(&mut self, a: &AbsVal, b: &AbsVal) -> Result<Src, CErr> {
    let ar = self.arity(a)?;
    let mut p = ArrayVec::<Src, 4>::new();
    for c in 0..ar {
      let (x, y) = (self.chan(a, c), self.chan(b, c));
      p.push(self.push_op(OpKind::Mul, x, y, Src::Const(0.)));
    }
    let add = |s: &mut Self, x: Src, y: Src| s.push_op(OpKind::Add, x, y, Src::Const(0.));
    Ok(match p.len() {
      1 => p[0],
      2 => add(self, p[0], p[1]),
      3 => {
        let s01 = add(self, p[0], p[1]);
        add(self, s01, p[2])
      }
      _ => {
        let s02 = add(self, p[0], p[2]);
        let s13 = add(self, p[1], p[3]);
        add(self, s02, s13)
      }
    })
  }

  fn construct(&mut self, comps: &[(AbsVal, u8)]) -> Result<AbsVal, CErr> {
    let mut chans = ArrayVec::new();
    for (v, c) in comps {
      chans.push(self.chan(v, *c));
    }
    Ok(VV::num(chans))
  }

  fn lower_fbm(
    &mut self,
    def_ix: usize,
    arg: &mut dyn FnMut(&mut Self, usize) -> AbsVal,
    loc: SourceLoc,
  ) -> Result<AbsVal, CErr> {
    // Def layouts mirror fbm_impl: 0/2/4 = pos-only (hardcoded defaults), 1/3/5 = full
    // params with pos at index 5; 2/3 add a trailing tileable.
    let (dim, pos_ix, full, tileable_ix): (u8, usize, bool, Option<usize>) = match def_ix {
      0 => (3, 0, false, None),
      1 => (3, 5, true, None),
      2 => (2, 0, false, Some(1)),
      3 => (2, 5, true, Some(6)),
      4 => (1, 0, false, None),
      5 => (1, 5, true, None),
      _ => return bail("unknown fbm signature", loc),
    };
    let pos = arg(self, pos_ix);
    let pos_ar = self.arity(&pos)?;
    if pos_ar != dim {
      return bail("fbm pos arity mismatch", loc);
    }
    let mut pos_srcs = [Src::Const(0.); 3];
    for c in 0..dim {
      pos_srcs[c as usize] = self.chan(&pos, c);
    }

    let uni_of = |v: AbsVal, what: &str| -> Result<u16, CErr> {
      match v {
        AbsVal::U(uix) => Ok(uix),
        AbsVal::V(_) | AbsVal::Seq(_) => bail(format!("varying fbm `{what}` parameter"), loc),
      }
    };

    let params: [u16; 5] = if full {
      let seed = uni_of(arg(self, 0), "seed")?;
      let octaves = uni_of(arg(self, 1), "octaves")?;
      let frequency = uni_of(arg(self, 2), "frequency")?;
      let lacunarity = uni_of(arg(self, 3), "lacunarity")?;
      let persistence = uni_of(arg(self, 4), "persistence")?;
      [seed, octaves, frequency, lacunarity, persistence]
    } else {
      let mut c = |v: Value| self.push_uni(UniSrc::Const(v.clone()), v, None, None);
      [
        c(Value::Int(0)),
        c(Value::Int(4)),
        c(Value::Float(1.)),
        c(Value::Float(2.)),
        c(Value::Float(0.5)),
      ]
    };
    let tileable = match tileable_ix {
      Some(ix) => Some(uni_of(arg(self, ix), "tileable")?),
      None => None,
    };

    let key = |guard| CseKey::Fbm {
      dim,
      pos: [pos_srcs[0].into(), pos_srcs[1].into(), pos_srcs[2].into()],
      params,
      tileable,
      guard,
    };
    if let Some(s) = self.cse_lookup(&key) {
      return Ok(VV::num([s].into_iter().collect()));
    }
    let dst = self.alloc_reg();
    let rix = self.n_fbm;
    self.n_fbm += 1;
    self.push_step(Step::Fbm(FbmStep {
      dim,
      dst,
      pos: pos_srcs,
      params,
      tileable,
      rix,
    }));
    self.cse.insert(key(self.guard), Src::Reg(dst));
    Ok(VV::num([Src::Reg(dst)].into_iter().collect()))
  }

  fn lower_sample(
    &mut self,
    arg: &mut dyn FnMut(&mut Self, usize) -> AbsVal,
    loc: SourceLoc,
  ) -> Result<AbsVal, CErr> {
    let uni_of = |v: AbsVal, what: &str| -> Result<u16, CErr> {
      match v {
        AbsVal::U(uix) => Ok(uix),
        AbsVal::V(_) | AbsVal::Seq(_) => bail(format!("varying sample `{what}` argument"), loc),
      }
    };
    let tex = uni_of(arg(self, 0), "texture")?;
    let channels = match self.uni_val(tex) {
      Value::Texture(t) => t.channels as u8,
      _ => return bail("sample on a non-texture", loc),
    };
    self.unis[tex as usize].shape = UniShape::Texture(channels);
    let uv = arg(self, 1);
    if self.arity(&uv)? != 2 {
      return bail("sample uv must be a vec2", loc);
    }
    let (u, v) = (self.chan(&uv, 0), self.chan(&uv, 1));
    let filter = uni_of(arg(self, 2), "filter")?;
    let wrap = uni_of(arg(self, 3), "wrap")?;

    let key = |chan: u8, guard| CseKey::Gather {
      tex,
      u: u.into(),
      v: v.into(),
      filter,
      wrap,
      chan,
      guard,
    };
    if let Some(s0) = self.cse_lookup(&|g| key(0, g)) {
      let mut chans: ArrayVec<Src, 4> = [s0].into_iter().collect();
      for c in 1..channels {
        chans.push(
          self
            .cse_lookup(&|g| key(c, g))
            .expect("gather channels are inserted together"),
        );
      }
      return Ok(VV::num(chans));
    }
    let dst: ArrayVec<u16, 4> = (0..channels).map(|_| self.alloc_reg()).collect();
    for (c, r) in dst.iter().enumerate() {
      self.cse.insert(key(c as u8, self.guard), Src::Reg(*r));
    }
    let rix = self.n_gather;
    self.n_gather += 1;
    self.push_step(Step::Gather(GatherStep {
      tex,
      filter,
      wrap,
      u,
      v,
      dst: dst.clone(),
      rix,
    }));
    Ok(VV::num(dst.iter().map(|r| Src::Reg(*r)).collect()))
  }

  fn lower_compare(
    &mut self,
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
    loc: SourceLoc,
  ) -> Result<AbsVal, CErr> {
    let l = self.compile_expr(lhs)?;
    let r = self.compile_expr(rhs)?;
    let (lv, rv) = (self.typed_value(&l), self.typed_value(&r));
    let def_ix = op
      .resolve_def_ix(self.ctx, &lv, &rv)
      .map_err(|e| CErr::Err(e.wrap(format!("Error applying binary operator `{op:?}`"))))?;
    // Every comparison's sig 1 is (Numeric, Numeric) compared as f32 via `as_float()`;
    // `eq`/`neq` sig 4 is (Bool, Bool). Ints can't vary, so sig 0 never resolves here; the
    // nil/string arms bail.
    let (a, b) = match (def_ix, op) {
      (1, _) => (self.chan(&l, 0), self.chan(&r, 0)),
      (4, BinOp::Eq | BinOp::Neq) => (self.mask_src(&l, loc)?, self.mask_src(&r, loc)?),
      _ => {
        return bail(
          format!("comparison `{op:?}` on non-numeric varying values"),
          loc,
        )
      }
    };
    let kind = match op {
      BinOp::Gt => OpKind::Gt,
      BinOp::Lt => OpKind::Lt,
      BinOp::Gte => OpKind::Gte,
      BinOp::Lte => OpKind::Lte,
      BinOp::Eq => OpKind::Eq,
      _ => OpKind::Neq,
    };
    Ok(VV::mask(self.push_op(kind, a, b, Src::Const(0.))))
  }

  /// `&&` / `||`. A uniform lhs short-circuits in the scalar path, which is exactly a
  /// uniform-condition conditional (`a && b` ≡ `if a { b } else { false }`), so the rhs
  /// only runs on runs that need it. A varying lhs evaluates both sides everywhere — total
  /// ops, so unobservable — and combines the masks arithmetically.
  fn lower_logic(
    &mut self,
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
    loc: SourceLoc,
  ) -> Result<AbsVal, CErr> {
    let l = self.compile_expr(lhs)?;
    self.lower_logic_abs(op, l, |c| c.compile_expr(rhs), loc)
  }

  fn lower_logic_abs(
    &mut self,
    op: BinOp,
    l: AbsVal,
    rhs: impl FnOnce(&mut Self) -> Result<AbsVal, CErr>,
    loc: SourceLoc,
  ) -> Result<AbsVal, CErr> {
    if let AbsVal::U(_) = l {
      let settled = Value::Bool(op == BinOp::Or);
      let settled = AbsVal::U(self.push_uni(UniSrc::Const(settled.clone()), settled, None, None));
      return if op == BinOp::And {
        self.lower_select(l, rhs, |_| Ok(settled), loc)
      } else {
        self.lower_select(l, |_| Ok(settled), rhs, loc)
      };
    }
    let lm = self.mask_src(&l, loc)?;
    let r = self.speculative(rhs, loc)?;
    let rm = self.mask_src(&r, loc)?;
    let kind = if op == BinOp::And {
      OpKind::And
    } else {
      OpKind::Or
    };
    Ok(VV::mask(self.push_op(kind, lm, rm, Src::Const(0.))))
  }

  // -------------------------------------------------------------------------------------
  // Sequences: loops unroll at compile time
  // -------------------------------------------------------------------------------------

  /// Read-only callee lookup for dispatch decisions; records no uniform.
  fn peek_callee(&self, call: &FunctionCall) -> Option<Rc<Callable>> {
    let val = match &call.target {
      FunctionCallTarget::Literal(c) => return Some(Rc::clone(c)),
      FunctionCallTarget::Name(_) => match call.target_res {
        VarRes::Capture(ix) => self.cur().captures[ix as usize].clone(),
        VarRes::Local(slot) if !self.slot_is_abstract(slot) => {
          self.cur().mirror.borrow()[slot as usize].clone()
        }
        _ => return None,
      },
    };
    match val {
      Value::Callable(c) => Some(c),
      _ => None,
    }
  }

  /// `peek_callee` plus, for literal targets only (baked, so the bound values are plan
  /// constants), partial-application flattening with the call's own args appended.
  fn peek_flat(
    &self,
    call: &FunctionCall,
    loc: SourceLoc,
  ) -> Option<(Rc<Callable>, Vec<Expr>, FxHashMap<Sym, Expr>)> {
    let c = self.peek_callee(call)?;
    let (c, mut args, mut kwargs) = match call.target {
      FunctionCallTarget::Literal(_) => flatten_partial(&c, loc),
      FunctionCallTarget::Name(_) => (c, Vec::new(), FxHashMap::default()),
    };
    args.extend(call.args.iter().cloned());
    kwargs.extend(call.kwargs.iter().map(|(k, v)| (*k, v.clone())));
    Some((c, args, kwargs))
  }

  /// A callback-position expression as a callee. A literal callable is baked into the AST
  /// (no per-run identity to validate); any other uniform re-resolves per run.
  fn callback(&mut self, e: &Expr, loc: SourceLoc) -> Result<Cb, CErr> {
    match e {
      Expr::Closure { .. } => Ok(Cb::Lit(Rc::new(e.clone()))),
      Expr::Ident {
        res: VarRes::Local(slot),
        ..
      } if matches!(self.cur().slot_abs[*slot as usize], SlotState::Lit(_)) => {
        let SlotState::Lit(lit) = &self.cur().slot_abs[*slot as usize] else {
          unreachable!()
        };
        Ok(Cb::Lit(Rc::clone(lit)))
      }
      Expr::Literal {
        value: Value::Callable(c),
        ..
      } => Ok(Cb::Callable {
        c: Rc::clone(c),
        uix: None,
      }),
      _ => {
        if !self.expr_is_uniform(e) {
          return bail("varying callee", loc);
        }
        let val = self.eval_uniform_now(e)?;
        let Value::Callable(c) = &val else {
          return bail("callee is not a callable", loc);
        };
        let c = Rc::clone(c);
        let uix = self.push_uni(UniSrc::Expr(e.clone()), val, None, None);
        Ok(Cb::Callable { c, uix: Some(uix) })
      }
    }
  }

  fn invoke(&mut self, cb: &Cb, args: Vec<AbsVal>, loc: SourceLoc) -> Result<AbsVal, CErr> {
    match cb {
      Cb::Lit(lit) => self.inline_literal(lit, args, FxHashMap::default(), loc),
      Cb::Callable { c, uix } => self.lower_callable_call(c, *uix, args, FxHashMap::default(), loc),
    }
  }

  fn int_uniform(&mut self, i: i64) -> AbsVal {
    AbsVal::U(self.push_uni(UniSrc::Const(Value::Int(i)), Value::Int(i), None, None))
  }

  /// Elements of a sequence-valued abstract value. A uniform sequence is consumed once at
  /// compile time (under the effect fence): a literal's elements become plan constants, any
  /// other's are re-read per run with the length pinned (a change evicts and recompiles).
  fn seq_elements(&mut self, v: &AbsVal, loc: SourceLoc) -> Result<AbsSeq, CErr> {
    let uix = match v {
      AbsVal::Seq(s) => return Ok(s.clone()),
      AbsVal::V(_) => return bail("sequence operation on a varying number", loc),
      AbsVal::U(uix) => *uix,
    };
    let seq = match self.uni_val(uix) {
      Value::Sequence(seq) => Rc::clone(seq),
      Value::Texture(_) => {
        return bail(
          "per-texel map over a captured texture inside a texel body",
          loc,
        )
      }
      _ => return bail("sequence operation on a non-sequence", loc),
    };
    let eager = seq_as_eager(&*seq).is_some();
    let mut items = Vec::new();
    for item in seq.consume(self.ctx).take(MAX_UNROLL + 1) {
      items.push(item.map_err(CErr::Err)?);
    }
    if items.len() > MAX_UNROLL {
      return bail(
        format!("sequence longer than {MAX_UNROLL} elements (or unbounded) in a texel body"),
        loc,
      );
    }
    let constant = self.const_uniform_value(v).is_some();
    if !constant {
      self.unis[uix as usize].shape = UniShape::Seq {
        len: items.len() as u16,
      };
    }
    let els = items
      .into_iter()
      .enumerate()
      .map(|(ix, item)| {
        let src = if constant {
          UniSrc::Const(item.clone())
        } else {
          UniSrc::SeqElem {
            of: uix,
            ix: ix as u16,
          }
        };
        AbsVal::U(self.push_uni(src, item, None, None))
      })
      .collect();
    Ok(AbsSeq {
      els: Rc::new(els),
      eager,
    })
  }

  /// Structural lowering of a sequence builtin: the sequence's elements are known at compile
  /// time, so every callback invocation compiles once per element with the interpreter's
  /// exact argument convention (`map`/`fold`/`scan` pass the index; `reduce` numbers it
  /// from the second element; `any`/`all` pass only the element and short-circuit).
  fn lower_seq_call(
    &mut self,
    name: &'static str,
    args: &[Expr],
    kwargs: &FxHashMap<Sym, Expr>,
    piped: Option<AbsVal>,
    loc: SourceLoc,
  ) -> Result<AbsVal, CErr> {
    enum Arg<'a> {
      Expr(&'a Expr),
      Abs(AbsVal),
    }
    let defs = fn_sigs()
      .entries
      .iter()
      .find(|(n, _)| *n == name)
      .expect("known builtin")
      .1
      .signatures[0]
      .arg_defs;
    // Positionals fill the params kwargs didn't name, in order; the piped value comes last.
    let (mut pos, mut piped) = (0usize, piped);
    let mut bound: Vec<Arg> = Vec::with_capacity(defs.len());
    for d in defs {
      bound.push(if let Some(e) = kwargs.get(&d.interned_name) {
        Arg::Expr(e)
      } else if pos < args.len() {
        pos += 1;
        Arg::Expr(&args[pos - 1])
      } else if let Some(p) = piped.take() {
        Arg::Abs(p)
      } else {
        return bail(format!("missing argument `{}` to `{name}`", d.name), loc);
      });
    }
    if pos < args.len() || piped.is_some() {
      return bail(format!("too many arguments to `{name}`"), loc);
    }
    let val = |c: &mut Self, a: &Arg| -> Result<AbsVal, CErr> {
      match a {
        Arg::Abs(v) => Ok(v.clone()),
        Arg::Expr(e) => c.compile_expr(e),
      }
    };
    let seq = |c: &mut Self, a: &Arg| -> Result<AbsSeq, CErr> {
      let v = val(c, a)?;
      c.seq_elements(&v, loc)
    };
    let cb = |c: &mut Self, a: &Arg| -> Result<Cb, CErr> {
      match a {
        Arg::Expr(e) => c.callback(e, loc),
        Arg::Abs(_) => bail("piped value in a callable position", loc),
      }
    };
    let lazy = |els: Vec<AbsVal>| {
      AbsVal::Seq(AbsSeq {
        els: Rc::new(els),
        eager: false,
      })
    };
    match name {
      "map" => {
        let (f, s) = (cb(self, &bound[0])?, seq(self, &bound[1])?);
        let mut els = Vec::with_capacity(s.els.len());
        for (i, e) in s.els.iter().enumerate() {
          let ix = self.int_uniform(i as i64);
          els.push(self.invoke(&f, vec![e.clone(), ix], loc)?);
        }
        Ok(lazy(els))
      }
      "fold" | "scan" => {
        let (mut acc, f, s) = (
          val(self, &bound[0])?,
          cb(self, &bound[1])?,
          seq(self, &bound[2])?,
        );
        let mut out = Vec::with_capacity(s.els.len());
        for (i, e) in s.els.iter().enumerate() {
          let ix = self.int_uniform(i as i64);
          acc = self.invoke(&f, vec![acc, e.clone(), ix], loc)?;
          out.push(acc.clone());
        }
        Ok(if name == "fold" { acc } else { lazy(out) })
      }
      "reduce" => {
        let (f, s) = (cb(self, &bound[0])?, seq(self, &bound[1])?);
        let Some((first, rest)) = s.els.split_first() else {
          return bail("reduce over an empty sequence", loc);
        };
        let mut acc = first.clone();
        for (i, e) in rest.iter().enumerate() {
          let ix = self.int_uniform(i as i64);
          acc = self.invoke(&f, vec![acc, e.clone(), ix], loc)?;
        }
        Ok(acc)
      }
      "any" | "all" => {
        let (f, s) = (cb(self, &bound[0])?, seq(self, &bound[1])?);
        let op = if name == "any" { BinOp::Or } else { BinOp::And };
        self.fold_logic(op, &f, &s.els, 0, loc)
      }
      "collect" => {
        let s = seq(self, &bound[0])?;
        Ok(AbsVal::Seq(AbsSeq {
          els: s.els,
          eager: true,
        }))
      }
      "reverse" => {
        let s = seq(self, &bound[0])?;
        let mut els = (*s.els).clone();
        els.reverse();
        Ok(AbsVal::Seq(AbsSeq {
          els: Rc::new(els),
          eager: true,
        }))
      }
      "first" | "last" => {
        let s = seq(self, &bound[0])?;
        let e = if name == "first" {
          s.els.first()
        } else {
          s.els.last()
        };
        e.cloned()
          .ok_or_else(|| CErr::Bail(format!("`{name}` of an empty sequence"), loc))
      }
      "take" | "skip" => {
        let (count, s) = (val(self, &bound[0])?, seq(self, &bound[1])?);
        let Some(Value::Int(n)) = self.const_uniform_value(&count) else {
          return bail(format!("`{name}` count must be an int literal"), loc);
        };
        let n = usize::try_from(n).unwrap_or(0).min(s.els.len());
        let els = if name == "take" {
          &s.els[..n]
        } else {
          &s.els[n..]
        };
        Ok(lazy(els.to_vec()))
      }
      "flatten" => {
        let s = seq(self, &bound[0])?;
        let mut els = Vec::new();
        for e in s.els.iter() {
          let is_seq = match e {
            AbsVal::Seq(_) => true,
            AbsVal::U(uix) => matches!(self.uni_val(*uix), Value::Sequence(_)),
            AbsVal::V(_) => false,
          };
          if is_seq {
            els.extend(self.seq_elements(e, loc)?.els.iter().cloned());
          } else {
            els.push(e.clone());
          }
        }
        Ok(lazy(els))
      }
      "chain" => {
        let s = seq(self, &bound[0])?;
        let mut els = Vec::new();
        for e in s.els.iter() {
          els.extend(self.seq_elements(e, loc)?.els.iter().cloned());
        }
        Ok(lazy(els))
      }
      _ => unreachable!("not a structural sequence builtin"),
    }
  }

  fn fold_logic(
    &mut self,
    op: BinOp,
    cb: &Cb,
    els: &[AbsVal],
    k: usize,
    loc: SourceLoc,
  ) -> Result<AbsVal, CErr> {
    if k == els.len() {
      let v = Value::Bool(op == BinOp::And);
      return Ok(AbsVal::U(self.push_uni(
        UniSrc::Const(v.clone()),
        v,
        None,
        None,
      )));
    }
    let l = self.invoke(cb, vec![els[k].clone()], loc)?;
    self.lower_logic_abs(op, l, |c| c.fold_logic(op, cb, els, k + 1, loc), loc)
  }

  /// `else if` chains fold right-to-left: the tail is re-expressed as a nested conditional,
  /// so a fully-uniform tail still gets the interpreter's short-circuit for free.
  fn lower_conditional(
    &mut self,
    cond: &Expr,
    then: &Expr,
    else_ifs: &[(Expr, Expr)],
    else_expr: Option<&Expr>,
    loc: SourceLoc,
  ) -> Result<AbsVal, CErr> {
    let Some(else_expr) = else_expr else {
      return bail("conditional without `else` in a texel body", loc);
    };
    let c = self.compile_expr(cond)?;
    let tail = match else_ifs.split_first() {
      None => else_expr.clone(),
      Some(((c0, e0), rest)) => Expr::Conditional {
        cond: Box::new(c0.clone()),
        then: Box::new(e0.clone()),
        else_if_exprs: rest.to_vec(),
        else_expr: Some(Box::new(else_expr.clone())),
        loc,
      },
    };
    self.lower_select(c, |c| c.compile_expr(then), |c| c.compile_expr(&tail), loc)
  }

  fn new_guard(&mut self, cond: u16, expect: bool) -> u16 {
    self.guards.push(Guard {
      parent: self.guard,
      cond,
      expect,
    });
    (self.guards.len() - 1) as u16
  }

  /// `if cond { then } else { else_ }` over abstract values.
  ///
  /// Uniform cond: the plan is cached across runs whose cond value differs, so each arm
  /// compiles under its own run-time guard (its steps and uniforms are skipped on runs that
  /// don't take it — the scalar path never evaluates them either) and the arms join through
  /// a uniform-mask select, a register move in the executor. An arm that fails to compile
  /// becomes a per-run abort rather than failing the body, so `0..n -> |i| (t -> |v| if i ==
  /// 0 { v } else { v * w[i - 1] })` vectorizes on every slice that compiles.
  ///
  /// Varying cond: both arms run everywhere (total ops) and join through an exact
  /// bit-select.
  fn lower_select(
    &mut self,
    cond: AbsVal,
    then: impl FnOnce(&mut Self) -> Result<AbsVal, CErr>,
    else_: impl FnOnce(&mut Self) -> Result<AbsVal, CErr>,
    loc: SourceLoc,
  ) -> Result<AbsVal, CErr> {
    let AbsVal::U(uix) = cond else {
      let m = self.mask_src(&cond, loc)?;
      let t = self.speculative(then, loc)?;
      let e = self.speculative(else_, loc)?;
      return self.emit_select(m, &t, &e, loc);
    };
    let Value::Bool(cv) = *self.uni_val(uix) else {
      return bail("condition is not a bool", loc);
    };
    if self.const_uniform_value(&cond).is_some() {
      return if cv { then(self) } else { else_(self) };
    }
    self.unis[uix as usize].shape = UniShape::Bool;
    let outer = self.guard;
    let (g_then, g_else) = (self.new_guard(uix, true), self.new_guard(uix, false));
    self.guard = Some(g_then);
    let t = then(self);
    self.guard = Some(g_else);
    let e = else_(self);
    self.guard = outer;
    let (t, e) = match (t, e) {
      (Ok(t), Ok(e)) => (t, e),
      (Ok(t), Err(err)) => {
        self.record_branch_abort(g_else, err, loc);
        (t.clone(), t)
      }
      (Err(err), Ok(e)) => {
        self.record_branch_abort(g_then, err, loc);
        (e.clone(), e)
      }
      (Err(t_err), Err(e_err)) => return Err(if cv { t_err } else { e_err }),
    };
    self.emit_select(Src::Uni(uix, 0), &t, &e, loc)
  }

  fn record_branch_abort(&mut self, guard: u16, err: CErr, loc: SourceLoc) {
    let (reason, loc, evict) = match err {
      CErr::Bail(reason, bail_loc) => (reason, bail_loc, false),
      CErr::Err(e) => (format!("error in conditional branch: {e}"), loc, true),
    };
    self.branch_aborts.push(BranchAbort {
      guard,
      reason: reason.into(),
      loc: self.ctx.resolve_loc(loc),
      evict,
    });
  }

  /// Per-channel select of two arms that must agree in kind: both masks, or both numbers
  /// of one arity. A uniform numeric arm must be float-class — an `Int` arm gives the
  /// scalar path texel-dependent typing (`x / 3` int-divides on some texels) that f32
  /// registers can't express. Mismatches bail rather than error: the scalar path only
  /// errors if some texel actually takes the odd arm.
  fn emit_select(
    &mut self,
    m: Src,
    t: &AbsVal,
    e: &AbsVal,
    loc: SourceLoc,
  ) -> Result<AbsVal, CErr> {
    let is_mask = |c: &Self, v: &AbsVal| match v {
      AbsVal::V(vv) => vv.mask,
      AbsVal::U(uix) => matches!(c.uni_val(*uix), Value::Bool(_)),
      AbsVal::Seq(_) => false,
    };
    if is_mask(self, t) {
      if !is_mask(self, e) {
        return bail("conditional arms mix bool and number", loc);
      }
      let (a, b) = (self.mask_src(t, loc)?, self.mask_src(e, loc)?);
      return Ok(VV::mask(self.push_op(OpKind::Select, m, a, b)));
    }
    for arm in [t, e] {
      if let AbsVal::U(uix) = arm {
        match self.uni_val(*uix) {
          Value::Int(_) => {
            return bail(
              "int-typed arm of a conditional in a texel body (write `0.` not `0`)",
              loc,
            )
          }
          v if num_arity(v).is_none() => {
            return bail("non-numeric arm of a conditional in a texel body", loc)
          }
          _ => {}
        }
      }
    }
    let (ta, ea) = (self.arity(t)?, self.arity(e)?);
    if ta != ea {
      return bail(
        format!("conditional arms differ in arity ({ta} vs {ea})"),
        loc,
      );
    }
    let mut chans = ArrayVec::new();
    for c in 0..ta {
      let (a, b) = (self.chan(t, c), self.chan(e, c));
      chans.push(self.push_op(OpKind::Select, m, a, b));
    }
    Ok(VV::num(chans))
  }

  // -------------------------------------------------------------------------------------
  // Statements
  // -------------------------------------------------------------------------------------

  fn compile_statements(&mut self, stmts: &[Statement], loc: SourceLoc) -> Result<AbsVal, CErr> {
    let mut last: Option<AbsVal> = None;
    for (i, stmt) in stmts.iter().enumerate() {
      self.check_size(loc)?;
      let is_last = i == stmts.len() - 1;
      match stmt {
        Statement::Assignment {
          expr,
          type_hint,
          slot,
          ..
        } => {
          let Some(slot) = slot else {
            return bail("assignment without a resolved slot", expr.loc());
          };
          if self.expr_is_uniform(expr) {
            let val = self.eval_uniform_now(expr)?;
            if let Some(hint) = type_hint {
              if hint.validate_val(&val).is_err() {
                return bail("type hint violation on uniform assignment", expr.loc());
              }
            }
            self.cur().mirror.borrow_mut()[*slot as usize] = val.clone();
            self.push_uni(UniSrc::Expr(expr.clone()), val, Some(*slot), *type_hint);
            self.cur_mut().slot_abs[*slot as usize] = SlotState::Uniform;
          } else if let Expr::Closure { .. } = expr {
            if type_hint.is_some_and(|h| !matches!(h, ArgType::Any | ArgType::Callable)) {
              return bail("type hint mismatch on closure assignment", expr.loc());
            }
            self.cur_mut().slot_abs[*slot as usize] = SlotState::Lit(Rc::new(expr.clone()));
          } else {
            match self.compile_expr(expr)? {
              AbsVal::V(vv) => {
                if type_hint.is_some_and(|h| !Self::hint_fits(h, &vv)) {
                  return bail("type hint mismatch on varying assignment", expr.loc());
                }
                self.cur_mut().slot_abs[*slot as usize] = SlotState::Varying(vv);
              }
              AbsVal::Seq(seq) => {
                if type_hint.is_some_and(|h| !matches!(h, ArgType::Any | ArgType::Sequence)) {
                  return bail("type hint mismatch on sequence assignment", expr.loc());
                }
                self.cur_mut().slot_abs[*slot as usize] = SlotState::Seq(seq);
              }
              // Classified varying but valued uniform (a block with a varying non-final
              // statement and a uniform final expression).
              AbsVal::U(src_uix) => {
                let val = self.uni_val(src_uix).clone();
                if let Some(hint) = type_hint {
                  if hint.validate_val(&val).is_err() {
                    return bail("type hint violation on uniform assignment", expr.loc());
                  }
                }
                self.cur().mirror.borrow_mut()[*slot as usize] = val.clone();
                self.push_uni(UniSrc::UniRef(src_uix), val, Some(*slot), *type_hint);
                self.cur_mut().slot_abs[*slot as usize] = SlotState::Uniform;
              }
            }
          }
          if is_last {
            return bail("closure body ends with an assignment", loc);
          }
        }
        Statement::Expr(expr) => {
          let v = self.compile_expr(expr)?;
          if is_last {
            last = Some(v);
          }
        }
        Statement::DestructureAssignment { rhs, .. } => {
          return bail("destructuring assignment (not yet vectorized)", rhs.loc())
        }
        Statement::Return { .. } | Statement::Break { .. } => {
          unreachable!("exits are desugared in `optimize_ast`")
        }
      }
    }
    last.ok_or_else(|| CErr::Bail("empty closure body".into(), loc))
  }
}

/// A baked partial application flattened to its innermost callee plus the bound args as
/// literal exprs, in the runtime's order (bound args first; outer kwargs override inner).
fn flatten_partial(
  c: &Rc<Callable>,
  loc: SourceLoc,
) -> (Rc<Callable>, Vec<Expr>, FxHashMap<Sym, Expr>) {
  let Callable::PartiallyAppliedFn(p) = &**c else {
    return (Rc::clone(c), Vec::new(), FxHashMap::default());
  };
  let (inner, mut args, mut kwargs) = flatten_partial(&p.inner, loc);
  let lit = |v: &Value| Expr::Literal {
    value: v.clone(),
    loc,
  };
  args.extend(p.args.iter().map(lit));
  kwargs.extend(p.kwargs.iter().map(|(k, v)| (*k, lit(v))));
  (inner, args, kwargs)
}

fn builtin_name(c: &Callable) -> Option<&'static str> {
  let Callable::Builtin { fn_entry_ix, .. } = c else {
    return None;
  };
  Some(fn_sigs().entries[*fn_entry_ix].0)
}

/// `Some(name)` for a structural sequence builtin; a bail for the sequence ops that can't
/// unroll (varying length or early exit); `None` otherwise.
fn seq_dispatch(c: &Callable, loc: SourceLoc) -> Result<Option<&'static str>, CErr> {
  let Some(name) = builtin_name(c) else {
    return Ok(None);
  };
  if SEQ_BAILS.contains(&name) {
    return bail(
      format!("`{name}` is not vectorizable (varying-length or early-exit sequence op)"),
      loc,
    );
  }
  Ok(SEQ_BUILTINS.contains(&name).then_some(name))
}

/// Slot renaming for inlining a closure literal into the frame that created it: the
/// literal's own slots shift by `base`, its captures become the enclosing frame's
/// locals/captures, and nested literals' capture specs are re-pointed the same way (their
/// bodies, which address their own frames, are left alone).
struct Renamer<'a> {
  base: u16,
  caps: &'a [VarRes],
}

impl Renamer<'_> {
  fn res(&self, r: VarRes, loc: SourceLoc) -> Result<VarRes, CErr> {
    Ok(match r {
      VarRes::Local(s) => VarRes::Local(self.base + s),
      VarRes::Capture(ix) => self.caps[ix as usize],
      VarRes::SelfRef => return bail("recursive closure literal", loc),
      VarRes::Unresolved => VarRes::Unresolved,
    })
  }

  fn cap(&self, from: CaptureFrom, loc: SourceLoc) -> Result<CaptureFrom, CErr> {
    Ok(match from {
      CaptureFrom::Local(s) => CaptureFrom::Local(self.base + s),
      CaptureFrom::Capture(ix) => match self.caps[ix as usize] {
        VarRes::Local(s) => CaptureFrom::Local(s),
        VarRes::Capture(i) => CaptureFrom::Capture(i),
        _ => return bail("unresolvable capture in a nested closure literal", loc),
      },
      CaptureFrom::SelfRef | CaptureFrom::DefScope(_) => {
        return bail("nested closure capturing the inlined closure", loc)
      }
    })
  }

  fn stmt(&self, s: &Statement) -> Result<Statement, CErr> {
    Ok(match s {
      Statement::Assignment {
        name,
        name_loc,
        expr,
        type_hint,
        slot,
      } => Statement::Assignment {
        name: *name,
        name_loc: *name_loc,
        expr: self.expr(expr)?,
        type_hint: *type_hint,
        slot: slot.map(|s| self.base + s),
      },
      Statement::DestructureAssignment { lhs, rhs, slots } => Statement::DestructureAssignment {
        lhs: lhs.clone(),
        rhs: self.expr(rhs)?,
        slots: slots
          .as_ref()
          .map(|v| v.iter().map(|s| self.base + s).collect::<Vec<_>>().into()),
      },
      Statement::Expr(e) => Statement::Expr(self.expr(e)?),
      Statement::Return { value, loc } => Statement::Return {
        value: value.as_ref().map(|e| self.expr(e)).transpose()?,
        loc: *loc,
      },
      Statement::Break { value, loc } => Statement::Break {
        value: value.as_ref().map(|e| self.expr(e)).transpose()?,
        loc: *loc,
      },
    })
  }

  fn boxed(&self, e: &Expr) -> Result<Box<Expr>, CErr> {
    Ok(Box::new(self.expr(e)?))
  }

  fn expr(&self, e: &Expr) -> Result<Expr, CErr> {
    Ok(match e {
      Expr::Ident { name, res, loc } => Expr::Ident {
        name: *name,
        res: self.res(*res, *loc)?,
        loc: *loc,
      },
      Expr::Call { call, loc } => Expr::Call {
        call: FunctionCall {
          target: call.target.clone(),
          // The optimizer folds const captures into literal targets and leaves `target_res`
          // stale, so it only means something for `Name` targets.
          target_res: match call.target {
            FunctionCallTarget::Literal(_) => call.target_res,
            FunctionCallTarget::Name(_) => self.res(call.target_res, *loc)?,
          },
          args: call
            .args
            .iter()
            .map(|a| self.expr(a))
            .collect::<Result<_, _>>()?,
          kwargs: call
            .kwargs
            .iter()
            .map(|(k, v)| Ok((*k, self.expr(v)?)))
            .collect::<Result<_, CErr>>()?,
        },
        loc: *loc,
      },
      Expr::Closure {
        params,
        body,
        return_type_hint,
        resolved,
        loc,
        end_loc,
      } => Expr::Closure {
        params: Rc::clone(params),
        body: Rc::clone(body),
        return_type_hint: *return_type_hint,
        resolved: match resolved {
          Some(m) => Some(Rc::new(ResolvedBody {
            id: m.id,
            n_slots: m.n_slots,
            captures: m
              .captures
              .iter()
              .map(|(n, f)| Ok((*n, self.cap(*f, *loc)?)))
              .collect::<Result<_, CErr>>()?,
            param_slots: m.param_slots.clone(),
          })),
          None => None,
        },
        loc: *loc,
        end_loc: *end_loc,
      },
      Expr::BinOp {
        op,
        lhs,
        rhs,
        pre_resolved_def_ix,
        loc,
      } => Expr::BinOp {
        op: *op,
        lhs: self.boxed(lhs)?,
        rhs: self.boxed(rhs)?,
        pre_resolved_def_ix: *pre_resolved_def_ix,
        loc: *loc,
      },
      Expr::PrefixOp { op, expr, loc } => Expr::PrefixOp {
        op: *op,
        expr: self.boxed(expr)?,
        loc: *loc,
      },
      Expr::Range {
        start,
        end,
        inclusive,
        loc,
      } => Expr::Range {
        start: self.boxed(start)?,
        end: end.as_ref().map(|e| self.boxed(e)).transpose()?,
        inclusive: *inclusive,
        loc: *loc,
      },
      Expr::StaticFieldAccess { lhs, field, loc } => Expr::StaticFieldAccess {
        lhs: self.boxed(lhs)?,
        field: field.clone(),
        loc: *loc,
      },
      Expr::FieldAccess {
        lhs,
        field,
        field2,
        loc,
      } => Expr::FieldAccess {
        lhs: self.boxed(lhs)?,
        field: self.boxed(field)?,
        field2: field2.as_ref().map(|e| self.boxed(e)).transpose()?,
        loc: *loc,
      },
      Expr::ArrayLiteral { elements, loc } => Expr::ArrayLiteral {
        elements: elements
          .iter()
          .map(|e| self.expr(e))
          .collect::<Result<_, _>>()?,
        loc: *loc,
      },
      Expr::MapLiteral { entries, loc } => Expr::MapLiteral {
        entries: entries
          .iter()
          .map(|en| {
            Ok(match en {
              MapLiteralEntry::KeyValue { key, value } => MapLiteralEntry::KeyValue {
                key: key.clone(),
                value: self.expr(value)?,
              },
              MapLiteralEntry::Splat { expr } => MapLiteralEntry::Splat {
                expr: self.expr(expr)?,
              },
            })
          })
          .collect::<Result<_, CErr>>()?,
        loc: *loc,
      },
      Expr::Literal { .. } => e.clone(),
      Expr::Conditional {
        cond,
        then,
        else_if_exprs,
        else_expr,
        loc,
      } => Expr::Conditional {
        cond: self.boxed(cond)?,
        then: self.boxed(then)?,
        else_if_exprs: else_if_exprs
          .iter()
          .map(|(c, b)| Ok((self.expr(c)?, self.expr(b)?)))
          .collect::<Result<_, CErr>>()?,
        else_expr: else_expr.as_ref().map(|e| self.boxed(e)).transpose()?,
        loc: *loc,
      },
      Expr::Block {
        statements,
        loc,
        end_loc,
      } => Expr::Block {
        statements: statements
          .iter()
          .map(|s| self.stmt(s))
          .collect::<Result<_, _>>()?,
        loc: *loc,
        end_loc: *end_loc,
      },
    })
  }
}

// ---------------------------------------------------------------------------------------
// `return` desugar
// ---------------------------------------------------------------------------------------

// ---------------------------------------------------------------------------------------
// Pre-filter
// ---------------------------------------------------------------------------------------

/// Syntactic whole-body bails: effectful/rng literal builtin calls anywhere (incl. nested
/// closure bodies — the effect fence backstops non-literal callees at run time) and, for the
/// texel closure itself (`xy_params`), any reference to the `x_ix`/`y_ix` params.
fn prefilter(closure: &Closure, xy_from: Option<usize>) -> Result<(), CErr> {
  let mut bad: Option<(String, SourceLoc)> = None;
  for stmt in &closure.body.0 {
    stmt.traverse_exprs(&mut |e: &Expr| {
      if bad.is_some() {
        return;
      }
      if let Expr::Call {
        call:
          FunctionCall {
            target: FunctionCallTarget::Literal(c),
            ..
          },
        loc,
        ..
      } = e
      {
        if c.is_side_effectful() || c.is_rng_dependent() {
          bad = Some((format!("side-effectful or rng-dependent call: {c:?}"), *loc));
        }
      }
    });
  }

  let mut xy_slots: Vec<u16> = Vec::new();
  if let Some(xy_from) = xy_from {
    for (i, param) in closure.params.iter().enumerate().skip(xy_from) {
      let start = closure.resolved.param_slots[i];
      xy_slots.extend((0..param.ident.iter_idents().count() as u16).map(|k| start + k));
    }
  }
  for stmt in &closure.body.0 {
    if xy_slots.is_empty() {
      break;
    }
    walk_stmt_shallow(stmt, &mut |e: &Expr| {
      if bad.is_some() {
        return;
      }
      match e {
        Expr::Ident {
          res: VarRes::Local(slot),
          loc,
          ..
        } if xy_slots.contains(slot) => {
          bad = Some(("reference to x_ix/y_ix pixel-index param".into(), *loc));
        }
        Expr::Closure {
          resolved: Some(meta),
          loc,
          ..
        } => {
          if meta
            .captures
            .iter()
            .any(|(_, from)| matches!(from, CaptureFrom::Local(s) if xy_slots.contains(s)))
          {
            bad = Some(("closure capturing x_ix/y_ix pixel-index param".into(), *loc));
          }
        }
        _ => {}
      }
    });
  }
  match bad {
    Some((reason, loc)) => Err(CErr::Bail(reason, loc)),
    None => Ok(()),
  }
}

/// Slot-scan walk: visits every expression under `stmt` except the bodies of nested
/// closures, whose `Local(n)` names a slot in a different frame entirely.
fn walk_stmt_shallow(stmt: &Statement, cb: &mut impl FnMut(&Expr)) {
  for e in stmt.exprs() {
    walk_expr_shallow(e, cb);
  }
}

fn walk_expr_shallow(expr: &Expr, cb: &mut impl FnMut(&Expr)) {
  cb(expr);
  match expr {
    Expr::BinOp { lhs, rhs, .. } => {
      walk_expr_shallow(lhs, cb);
      walk_expr_shallow(rhs, cb);
    }
    Expr::PrefixOp { expr, .. } => walk_expr_shallow(expr, cb),
    Expr::Range { start, end, .. } => {
      walk_expr_shallow(start, cb);
      if let Some(e) = end {
        walk_expr_shallow(e, cb);
      }
    }
    Expr::StaticFieldAccess { lhs, .. } => walk_expr_shallow(lhs, cb),
    Expr::FieldAccess {
      lhs, field, field2, ..
    } => {
      walk_expr_shallow(lhs, cb);
      walk_expr_shallow(field, cb);
      if let Some(f) = field2 {
        walk_expr_shallow(f, cb);
      }
    }
    Expr::Call { call, .. } => {
      for a in call.args.iter().chain(call.kwargs.values()) {
        walk_expr_shallow(a, cb);
      }
    }
    Expr::ArrayLiteral { elements, .. } => {
      for e in elements {
        walk_expr_shallow(e, cb);
      }
    }
    Expr::MapLiteral { entries, .. } => {
      for e in entries {
        walk_expr_shallow(e.expr(), cb);
      }
    }
    Expr::Conditional {
      cond,
      then,
      else_if_exprs,
      else_expr,
      ..
    } => {
      walk_expr_shallow(cond, cb);
      walk_expr_shallow(then, cb);
      for (c, e) in else_if_exprs {
        walk_expr_shallow(c, cb);
        walk_expr_shallow(e, cb);
      }
      if let Some(e) = else_expr {
        walk_expr_shallow(e, cb);
      }
    }
    Expr::Block { statements, .. } => {
      for s in statements {
        walk_stmt_shallow(s, cb);
      }
    }
    Expr::Closure { .. } | Expr::Ident { .. } | Expr::Literal { .. } => {}
  }
}

// ---------------------------------------------------------------------------------------
// Uniform evaluation + validation (per run)
// ---------------------------------------------------------------------------------------

enum UniErr {
  /// Fall back to the scalar loop (validation surprise or observable effect).
  Abort,
  /// A pinned sequence length changed: evict and recompile under this run's values.
  Recompile,
  Err(ErrorStack),
}

fn eval_uniforms(
  ctx: &EvalCtx,
  plan: &Plan,
  closure: &Closure,
  callable: &Rc<Callable>,
) -> Result<(Vec<Value>, Vec<bool>), UniErr> {
  let fence = EffectFence::snapshot(ctx);
  let res = eval_uniforms_inner(ctx, plan, closure, callable);
  if !fence.verify_or_restore(ctx) {
    return Err(UniErr::Abort);
  }
  res
}

/// Guard `g` given the uniforms evaluated so far (a guard's cond always precedes what it
/// guards). `None` = the cond isn't a bool; the scalar path's error, hand off.
fn guard_on(plan: &Plan, vals: &[Value], memo: &mut [Option<bool>], g: u16) -> Option<bool> {
  if let Some(v) = memo[g as usize] {
    return Some(v);
  }
  let gd = &plan.guards[g as usize];
  let parent_on = match gd.parent {
    Some(p) => guard_on(plan, vals, memo, p)?,
    None => true,
  };
  let on = parent_on
    && match vals[gd.cond as usize] {
      Value::Bool(b) => b == gd.expect,
      _ => return None,
    };
  memo[g as usize] = Some(on);
  Some(on)
}

fn all_guards(plan: &Plan, vals: &[Value]) -> Option<Vec<bool>> {
  let mut memo = vec![None; plan.guards.len()];
  (0..plan.guards.len() as u16)
    .map(|g| guard_on(plan, vals, &mut memo, g))
    .collect()
}

fn eval_uniforms_inner(
  ctx: &EvalCtx,
  plan: &Plan,
  closure: &Closure,
  callable: &Rc<Callable>,
) -> Result<(Vec<Value>, Vec<bool>), UniErr> {
  // Frame 0 = the texel closure; inline frames resolve their captures/self from the callee
  // value once its uniform entry has been evaluated (always earlier in the table).
  let mirrors: Vec<RefCell<Vec<Value>>> = plan
    .frames
    .iter()
    .map(|f| RefCell::new(vec![Value::Nil; f.n_slots as usize]))
    .collect();
  let mut frame_callees: Vec<Option<Rc<Callable>>> = vec![None; plan.frames.len()];
  let mut vals: Vec<Value> = Vec::with_capacity(plan.unis.len());
  let mut memo = vec![None; plan.guards.len()];
  let mut seqs: FxHashMap<u16, Rc<Vec<Value>>> = FxHashMap::default();

  for (uix, step) in plan.unis.iter().enumerate() {
    if let Some(g) = step.guard {
      match guard_on(plan, &vals, &mut memo, g) {
        Some(true) => {}
        Some(false) => {
          vals.push(Value::Nil);
          continue;
        }
        None => return Err(UniErr::Abort),
      }
    }
    let fi = step.frame as usize;
    let (captures, self_ref): (Rc<[Value]>, Rc<Callable>) = if fi == 0 {
      (Rc::clone(&closure.captures), Rc::clone(callable))
    } else {
      let callee = match &frame_callees[fi] {
        Some(c) => Rc::clone(c),
        None => {
          let spec = &plan.frames[fi];
          let c = match (spec.callee_uix, &spec.baked_callee) {
            (Some(uix), _) => match &vals[uix as usize] {
              Value::Callable(c) => Rc::clone(c),
              _ => return Err(UniErr::Abort),
            },
            (None, Some(baked)) => Rc::clone(baked),
            (None, None) => return Err(UniErr::Abort),
          };
          frame_callees[fi] = Some(Rc::clone(&c));
          c
        }
      };
      let Callable::Closure(inner) = &*callee else {
        return Err(UniErr::Abort);
      };
      (Rc::clone(&inner.captures), callee)
    };

    let val = match &step.src {
      UniSrc::Expr(expr) => {
        let frame = FrameEnv {
          slots: &mirrors[fi],
          captures: &captures,
          self_ref: &self_ref,
        };
        match ctx.eval_expr_env(expr, &frame) {
          Ok(v) => v,
          Err(_) if step.speculative => return Err(UniErr::Abort),
          Err(e) => return Err(UniErr::Err(e)),
        }
      }
      UniSrc::Const(v) => v.clone(),
      UniSrc::Capture(ix) => captures[*ix as usize].clone(),
      UniSrc::Slot(slot) => mirrors[fi].borrow()[*slot as usize].clone(),
      UniSrc::UniRef(uix) => vals[*uix as usize].clone(),
      UniSrc::SwizzleOf { of, field } => {
        match ctx.eval_static_field_access(&vals[*of as usize], field) {
          Ok(v) => v,
          Err(_) if step.speculative => return Err(UniErr::Abort),
          Err(e) => return Err(UniErr::Err(e)),
        }
      }
      UniSrc::SeqElem { of, ix } => match seqs.get(of) {
        Some(items) => items[*ix as usize].clone(),
        None => return Err(UniErr::Abort),
      },
    };
    if let UniShape::Seq { len } = step.shape {
      let Value::Sequence(seq) = &val else {
        return Err(UniErr::Abort);
      };
      let items = seq
        .consume(ctx)
        .take(len as usize + 1)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| {
          if step.speculative {
            UniErr::Abort
          } else {
            UniErr::Err(e)
          }
        })?;
      if items.len() != len as usize {
        return Err(UniErr::Recompile);
      }
      seqs.insert(uix as u16, Rc::new(items));
    }
    if let UniShape::Texture(ch) = step.shape {
      match &val {
        Value::Texture(t) if t.channels as u8 == ch => {}
        Value::Texture(_) => return Err(UniErr::Recompile),
        _ => return Err(UniErr::Abort),
      }
    }
    if let Some(hint) = &step.hint {
      if hint.validate_val(&val).is_err() {
        return Err(UniErr::Abort);
      }
    }
    if let Some(slot) = step.slot {
      mirrors[fi].borrow_mut()[slot as usize] = val.clone();
    }
    vals.push(val);
  }
  let guards = all_guards(plan, &vals).ok_or(UniErr::Abort)?;
  Ok((vals, guards))
}

/// Shape-checks the uniform table against what compile observed and pre-resolves everything
/// `exec` needs, so `exec` is infallible.
fn validate_uniforms(plan: &Plan, vals: Vec<Value>, guards: Vec<bool>) -> Option<UniRun> {
  let mut chans = vec![[0f32; 4]; vals.len()];
  for (i, (step, val)) in plan.unis.iter().zip(&vals).enumerate() {
    if step.guard.is_some_and(|g| !guards[g as usize]) {
      continue;
    }
    match step.shape {
      UniShape::Num { ar, int } => {
        let (c, got_ar) = value_chans(val)?;
        if got_ar != ar || matches!(val, Value::Int(_)) != int {
          return None;
        }
        chans[i] = c;
      }
      UniShape::Builtin(expect_ix) => match val {
        Value::Callable(c) => match &**c {
          Callable::Builtin { fn_entry_ix, .. } if *fn_entry_ix == expect_ix => {}
          _ => return None,
        },
        _ => return None,
      },
      UniShape::ClosureBody(expect_id) => match val {
        Value::Callable(c) => match &**c {
          Callable::Closure(inner) if inner.resolved.id == expect_id => {}
          _ => return None,
        },
        _ => return None,
      },
      UniShape::Dynamic(expect_ar) => match val {
        Value::Callable(c) => match &**c {
          Callable::Dynamic { inner, .. } => {
            if inner.is_side_effectful()
              || inner.is_rng_dependent()
              || dynamic_out_arity(&**c) != Some(expect_ar)
            {
              return None;
            }
          }
          _ => return None,
        },
        _ => return None,
      },
      UniShape::Bool => match val {
        Value::Bool(b) => chans[i] = [*b as u8 as f32; 4],
        _ => return None,
      },
      UniShape::Texture(ch) => match val {
        Value::Texture(t) if t.channels as u8 == ch => {}
        _ => return None,
      },
      // Length-checked during evaluation, where the consumed elements are needed anyway.
      UniShape::Any | UniShape::Seq { .. } => {}
    }
  }

  let mut gather = Vec::with_capacity(plan.n_gather as usize);
  for (ix, step) in plan.steps.iter().enumerate() {
    let Step::Gather(g) = step else { continue };
    debug_assert_eq!(g.rix as usize, gather.len());
    if plan.step_guards[ix].is_some_and(|gd| !guards[gd as usize]) {
      gather.push(None);
      continue;
    }
    let Value::Texture(tex) = &vals[g.tex as usize] else {
      return None;
    };
    let filter = kern::SampleFilter::from_name(vals[g.filter as usize].as_str()?).ok()?;
    let wrap = match &vals[g.wrap as usize] {
      Value::Nil => tex.wrap,
      v => crate::TextureWrap::from_name(v.as_str()?).ok()?,
    };
    gather.push(Some(GatherResolved {
      tex: Rc::clone(tex),
      filter,
      wrap,
    }));
  }

  let mut fbm = vec![
    FbmResolved {
      seed: 0,
      octaves: 0,
      frequency: 0.,
      persistence: 0.,
      lacunarity: 0.,
      tileable: None,
    };
    plan.n_fbm as usize
  ];
  for (ix, step) in plan.steps.iter().enumerate() {
    let Step::Fbm(f) = step else { continue };
    if plan.step_guards[ix].is_some_and(|g| !guards[g as usize]) {
      continue;
    }
    let [seed, octaves, frequency, lacunarity, persistence] = f.params;
    let seed = match vals[seed as usize].as_int() {
      Some(s) if (0..=u32::MAX as i64).contains(&s) => s as u32,
      _ => return None,
    };
    let octaves = match vals[octaves as usize].as_int() {
      Some(o) if (0..=32).contains(&o) => o as usize,
      _ => return None,
    };
    let frequency = vals[frequency as usize].as_float()?;
    let lacunarity = vals[lacunarity as usize].as_float()?;
    let persistence = vals[persistence as usize].as_float()?;
    let tileable = match f.tileable {
      Some(uix) => match resolve_tile_period(&vals[uix as usize]) {
        Ok(t) => t,
        Err(_) => return None,
      },
      None => None,
    };
    fbm[f.rix as usize] = FbmResolved {
      seed,
      octaves,
      frequency,
      persistence,
      lacunarity,
      tileable,
    };
  }
  Some(UniRun {
    vals,
    chans,
    fbm,
    gather,
    guards,
  })
}

// ---------------------------------------------------------------------------------------
// Executor
// ---------------------------------------------------------------------------------------

enum RSrc<'a> {
  S(&'a [f32]),
  K(f32),
}

struct Exec<'a> {
  regs: Vec<Option<Vec<f32>>>,
  pool: Vec<Vec<f32>>,
  uni: &'a UniRun,
  input: &'a [Rc<Vec<f32>>],
  uv: Option<&'a [Rc<Vec<f32>>; 2]>,
}

impl<'a> Exec<'a> {
  fn resolve(&self, s: Src) -> RSrc<'_> {
    match s {
      Src::Reg(r) => RSrc::S(self.regs[r as usize].as_ref().unwrap()),
      Src::In(c) => RSrc::S(&self.input[c as usize]),
      Src::Uv(c) => RSrc::S(&self.uv.unwrap()[c as usize]),
      Src::Uni(uix, c) => RSrc::K(self.uni.chans[uix as usize][c as usize]),
      Src::Const(k) => RSrc::K(k),
    }
  }

  /// Grab a destination buffer: a dying `Reg` src's buffer when possible (in-place), else
  /// from the pool, else empty (filled by collect on first write). A reg appearing in more
  /// than one src slot can't be stolen — the other read would alias the freed slot.
  fn grab_dst(&mut self, plan: &Plan, step_ix: u32, srcs: &[Src]) -> (Vec<f32>, Option<u16>) {
    for s in srcs {
      if let Src::Reg(r) = s {
        let n_uses = srcs
          .iter()
          .filter(|o| matches!(o, Src::Reg(x) if x == r))
          .count();
        if n_uses == 1 && plan.reg_last[*r as usize] == step_ix && self.regs[*r as usize].is_some()
        {
          let buf = self.regs[*r as usize].take().unwrap();
          return (buf, Some(*r));
        }
      }
    }
    (self.pool.pop().unwrap_or_default(), None)
  }

  fn release_dead(&mut self, plan: &Plan, step_ix: u32) {
    for (r, &last) in plan.reg_last.iter().enumerate() {
      if last == step_ix {
        if let Some(buf) = self.regs[r].take() {
          self.pool.push(buf);
        }
      }
    }
  }
}

fn coord_plane<'a>(ex: &'a Exec, s: Src, splat: &'a Option<Vec<f32>>) -> &'a [f32] {
  match splat {
    Some(b) => b,
    None => match ex.resolve(s) {
      RSrc::S(sl) => sl,
      RSrc::K(_) => unreachable!("uniform coordinates are splatted"),
    },
  }
}

fn write_map(buf: &mut Vec<f32>, a: &[f32], f: impl Fn(f32) -> f32) {
  if buf.len() == a.len() {
    kern::map_out(buf, a, f);
  } else {
    *buf = kern::map_new(a, f);
  }
}

fn write_zip(buf: &mut Vec<f32>, a: &[f32], b: &[f32], f: impl Fn(f32, f32) -> f32) {
  if buf.len() == a.len() {
    kern::zip_out(buf, a, b, f);
  } else {
    *buf = kern::zip_new(a, b, f);
  }
}

fn write_zip3(
  buf: &mut Vec<f32>,
  a: &[f32],
  b: &[f32],
  c: &[f32],
  f: impl Fn(f32, f32, f32) -> f32,
) {
  if buf.len() == a.len() {
    kern::zip3_out(buf, a, b, c, f);
  } else {
    *buf = kern::zip3_new(a, b, c, f);
  }
}

fn run_binary(buf: &mut Vec<f32>, a: RSrc, b: RSrc, n: usize, f: impl Fn(f32, f32) -> f32) {
  match (a, b) {
    (RSrc::S(a), RSrc::S(b)) => write_zip(buf, a, b, f),
    (RSrc::S(a), RSrc::K(b)) => write_map(buf, a, |x| f(x, b)),
    (RSrc::K(a), RSrc::S(b)) => write_map(buf, b, |x| f(a, x)),
    (RSrc::K(a), RSrc::K(b)) => {
      let v = f(a, b);
      buf.clear();
      buf.resize(n, v);
    }
  }
}

fn run_ternary(
  buf: &mut Vec<f32>,
  a: RSrc,
  b: RSrc,
  c: RSrc,
  n: usize,
  f: impl Fn(f32, f32, f32) -> f32,
) {
  match (a, b, c) {
    (RSrc::S(a), RSrc::S(b), RSrc::S(c)) => write_zip3(buf, a, b, c, f),
    (RSrc::S(a), RSrc::S(b), RSrc::K(c)) => write_zip(buf, a, b, |x, y| f(x, y, c)),
    (RSrc::S(a), RSrc::K(b), RSrc::S(c)) => write_zip(buf, a, c, |x, z| f(x, b, z)),
    (RSrc::K(a), RSrc::S(b), RSrc::S(c)) => write_zip(buf, b, c, |y, z| f(a, y, z)),
    (RSrc::S(a), RSrc::K(b), RSrc::K(c)) => write_map(buf, a, |x| f(x, b, c)),
    (RSrc::K(a), RSrc::S(b), RSrc::K(c)) => write_map(buf, b, |y| f(a, y, c)),
    (RSrc::K(a), RSrc::K(b), RSrc::S(c)) => write_map(buf, c, |z| f(a, b, z)),
    (RSrc::K(a), RSrc::K(b), RSrc::K(c)) => {
      let v = f(a, b, c);
      buf.clear();
      buf.resize(n, v);
    }
  }
}

/// In-place ternary: `buf` holds the operand at position `pos`; `x`, `y` are the other two
/// in argument order. One monomorphic loop per operand shape so each autovectorizes.
fn tern_in_place(buf: &mut [f32], pos: u8, x: RSrc, y: RSrc, f: impl Fn(f32, f32, f32) -> f32) {
  let n = buf.len();
  macro_rules! go {
    ($i:ident, $xi:expr, $yi:expr) => {
      match pos {
        0 => {
          for $i in 0..n {
            buf[$i] = f(buf[$i], $xi, $yi);
          }
        }
        1 => {
          for $i in 0..n {
            buf[$i] = f($xi, buf[$i], $yi);
          }
        }
        _ => {
          for $i in 0..n {
            buf[$i] = f($xi, $yi, buf[$i]);
          }
        }
      }
    };
  }
  match (x, y) {
    (RSrc::S(x), RSrc::S(y)) => {
      let (x, y) = (&x[..n], &y[..n]);
      go!(i, x[i], y[i])
    }
    (RSrc::S(x), RSrc::K(y)) => {
      let x = &x[..n];
      go!(i, x[i], y)
    }
    (RSrc::K(x), RSrc::S(y)) => {
      let y = &y[..n];
      go!(i, x, y[i])
    }
    (RSrc::K(x), RSrc::K(y)) => go!(i, x, y),
  }
}

/// `step_ms`: per-step wall time when profiling (`NaN` for guarded-off steps).
fn exec(
  ctx: &EvalCtx,
  plan: &Plan,
  uni: &UniRun,
  input: &[Rc<Vec<f32>>],
  uv: Option<&[Rc<Vec<f32>>; 2]>,
  n: usize,
  mut step_ms: Option<&mut Vec<f64>>,
) -> Result<Vec<Rc<Vec<f32>>>, ErrorStack> {
  let mut ex = Exec {
    regs: vec![None; plan.n_regs as usize],
    pool: Vec::new(),
    uni,
    input,
    uv,
  };

  for (step_ix, step) in plan.steps.iter().enumerate() {
    if plan.step_guards[step_ix].is_some_and(|g| !uni.guards[g as usize]) {
      ex.release_dead(plan, step_ix as u32);
      if let Some(ms) = step_ms.as_deref_mut() {
        ms.push(f64::NAN);
      }
      continue;
    }
    let t0 = step_ms.is_some().then(now_ms);
    let step_ix = step_ix as u32;
    match step {
      // Uniform-mask select: the pick is known for the whole run — move the picked register
      // (or copy/fill it when it's still live elsewhere); the other arm's steps were skipped.
      Step::Op {
        kind: OpKind::Select,
        dst,
        a: Src::Uni(uix, ch),
        b,
        c,
      } => {
        let pick = if uni.chans[*uix as usize][*ch as usize] != 0. {
          *b
        } else {
          *c
        };
        let (mut buf, stolen) = ex.grab_dst(plan, step_ix, &[pick]);
        if stolen.is_none() {
          buf.clear();
          match ex.resolve(pick) {
            RSrc::S(s) => buf.extend_from_slice(s),
            RSrc::K(k) => buf.resize(n, k),
          }
        }
        ex.regs[*dst as usize] = Some(buf);
        ex.release_dead(plan, step_ix);
      }
      Step::Op { kind, dst, a, b, c } => {
        let (mut buf, stolen) = ex.grab_dst(plan, step_ix, &[*a, *b, *c]);
        exec_op(&mut ex, *kind, &mut buf, stolen, *a, *b, *c, n);
        ex.regs[*dst as usize] = Some(buf);
        ex.release_dead(plan, step_ix);
      }
      Step::Fbm(f) => {
        let p = uni.fbm[f.rix as usize];
        let mut buf = ex.pool.pop().unwrap_or_default();
        buf.clear();
        buf.resize(n, 0.);
        // A uniform coordinate is splatted into a scratch plane so the batch kernels see
        // slices throughout; `fbm_1d` is `fbm_2d` against a zero y.
        let splat = |ex: &mut Exec, s: Src| match ex.resolve(s) {
          RSrc::K(k) => {
            let mut b = ex.pool.pop().unwrap_or_default();
            b.clear();
            b.resize(n, k);
            Some(b)
          }
          RSrc::S(_) => None,
        };
        let srcs: [Src; 3] = match f.dim {
          1 => [f.pos[0], Src::Const(0.), Src::Const(0.)],
          d => [
            f.pos[0],
            f.pos[1],
            if d == 3 { f.pos[2] } else { Src::Const(0.) },
          ],
        };
        let tmp = srcs.map(|s| splat(&mut ex, s));
        {
          let pl: [&[f32]; 3] = [
            coord_plane(&ex, srcs[0], &tmp[0]),
            coord_plane(&ex, srcs[1], &tmp[1]),
            coord_plane(&ex, srcs[2], &tmp[2]),
          ];
          if f.dim == 3 {
            noise_batch::fbm_3d_batch(
              p.seed,
              p.octaves,
              p.frequency,
              p.persistence,
              p.lacunarity,
              pl[0],
              pl[1],
              pl[2],
              &mut buf,
            );
          } else {
            noise_batch::fbm_2d_batch(
              p.seed,
              p.octaves,
              p.frequency,
              p.persistence,
              p.lacunarity,
              p.tileable,
              pl[0],
              pl[1],
              &mut buf,
            );
          }
        }
        ex.pool.extend(tmp.into_iter().flatten());
        ex.regs[f.dst as usize] = Some(buf);
        ex.release_dead(plan, step_ix);
      }
      Step::Gather(g) => {
        let r = uni.gather[g.rix as usize]
          .as_ref()
          .expect("resolved for every unguarded gather");
        let mut outs: Vec<Vec<f32>> = (0..g.dst.len())
          .map(|_| ex.pool.pop().unwrap_or_default())
          .collect();
        // A uniform coordinate (`sample(t, v2(uv.x, .5))`) is splatted into a scratch plane.
        let splat = |ex: &mut Exec, s: Src| match ex.resolve(s) {
          RSrc::K(k) => {
            let mut b = ex.pool.pop().unwrap_or_default();
            b.clear();
            b.resize(n, k);
            Some(b)
          }
          RSrc::S(_) => None,
        };
        let (ut, vt) = (splat(&mut ex, g.u), splat(&mut ex, g.v));
        {
          let (u, v) = (coord_plane(&ex, g.u, &ut), coord_plane(&ex, g.v, &vt));
          let (planes, origin, x_pitch, y_pitch) = r.tex.gather_parts();
          let src = kern::GatherSrc {
            planes: &planes,
            w: r.tex.width,
            h: r.tex.height,
            origin,
            x_pitch,
            y_pitch,
            wrap: r.wrap,
          };
          kern::gather(&src, r.filter, u, v, &mut outs);
        }
        ex.pool.extend(ut.into_iter().chain(vt));
        for (d, o) in g.dst.iter().zip(outs) {
          ex.regs[*d as usize] = Some(o);
        }
        ex.release_dead(plan, step_ix);
      }
      Step::Dyn(d) => {
        let callee = match &d.callee {
          DynCallee::Baked(c) => Rc::clone(c),
          DynCallee::Uni(uix) => match &uni.vals[*uix as usize] {
            Value::Callable(c) => Rc::clone(c),
            _ => unreachable!("validated by UniShape::Dynamic"),
          },
        };
        let Callable::Dynamic { inner, name } = &*callee else {
          unreachable!("validated by UniShape::Dynamic")
        };
        let out_ar = d.dst.len();
        let mut outs: Vec<Vec<f32>> = (0..out_ar)
          .map(|_| {
            let mut b = ex.pool.pop().unwrap_or_default();
            b.clear();
            b.reserve(n);
            b
          })
          .collect();
        let mut argv: Vec<Value> = Vec::with_capacity(d.args.len());
        for i in 0..n {
          argv.clear();
          for srcs in &d.args {
            let mut ch = [0f32; 4];
            for (k, s) in srcs.iter().enumerate() {
              ch[k] = match *s {
                Src::Reg(r) => ex.regs[r as usize].as_ref().unwrap()[i],
                Src::In(c) => ex.input[c as usize][i],
                Src::Uv(c) => ex.uv.unwrap()[c as usize][i],
                Src::Uni(uix, c) => ex.uni.chans[uix as usize][c as usize],
                Src::Const(k) => k,
              };
            }
            argv.push(match srcs.len() {
              1 => Value::Float(ch[0]),
              2 => Value::Vec2(Vec2::new(ch[0], ch[1])),
              3 => Value::Vec3(Vec3::new(ch[0], ch[1], ch[2])),
              _ => Value::Vec4(Rc::new(Vec4::new(ch[0], ch[1], ch[2], ch[3]))),
            });
          }
          let res = inner.invoke(&argv, EMPTY_KWARGS, ctx).map_err(|e| {
            e.wrap(format!(
              "Error invoking dynamic callable `{name}` per texel"
            ))
          })?;
          let Some((ch, ar)) = value_chans(&res) else {
            return Err(ErrorStack::new(format!(
              "dynamic callable `{name}` returned a non-numeric value in a vectorized texel \
               closure: {res:?}"
            )));
          };
          if ar as usize != out_ar {
            return Err(ErrorStack::new(format!(
              "dynamic callable `{name}` returned arity {ar}, expected {out_ar}"
            )));
          }
          for (k, out) in outs.iter_mut().enumerate() {
            out.push(ch[k]);
          }
        }
        for (k, out) in outs.into_iter().enumerate() {
          ex.regs[d.dst[k] as usize] = Some(out);
        }
        ex.release_dead(plan, step_ix);
      }
    }
    if let (Some(ms), Some(t0)) = (step_ms.as_deref_mut(), t0) {
      ms.push(now_ms() - t0);
    }
  }

  // Assemble output planes; duplicated regs (swizzled outputs) share one Rc.
  let out_srcs = match &plan.out {
    PlanOut::Chans(chans) => chans.clone(),
    PlanOut::Uniform(_) => unreachable!("uniform output handled by the caller"),
  };
  let mut taken: FxHashMap<u16, Rc<Vec<f32>>> = FxHashMap::default();
  Ok(
    out_srcs
      .iter()
      .map(|s| match s {
        Src::Reg(r) => taken
          .entry(*r)
          .or_insert_with(|| Rc::new(ex.regs[*r as usize].take().unwrap()))
          .clone(),
        Src::In(c) => Rc::clone(&input[*c as usize]),
        Src::Uv(c) => Rc::clone(&uv.unwrap()[*c as usize]),
        Src::Uni(uix, c) => Rc::new(vec![uni.chans[*uix as usize][*c as usize]; n]),
        Src::Const(k) => Rc::new(vec![*k; n]),
      })
      .collect(),
  )
}

#[allow(clippy::too_many_arguments)]
fn exec_op(
  ex: &mut Exec,
  kind: OpKind,
  buf: &mut Vec<f32>,
  stolen: Option<u16>,
  a: Src,
  b: Src,
  c: Src,
  n: usize,
) {
  // When a src reg was stolen as dst, run the op in place over it.
  let stolen_is = |s: Src| matches!((s, stolen), (Src::Reg(r), Some(st)) if r == st);

  macro_rules! unary {
    ($f:expr) => {{
      if stolen_is(a) {
        kern::map_in(buf, $f);
      } else {
        match ex.resolve(a) {
          RSrc::S(s) => write_map(buf, s, $f),
          RSrc::K(k) => {
            let v = $f(k);
            buf.clear();
            buf.resize(n, v);
          }
        }
      }
    }};
  }
  macro_rules! binary {
    ($f:expr) => {{
      let f = $f;
      if stolen_is(a) {
        match ex.resolve(b) {
          RSrc::S(s) => kern::zip_in_a(buf, s, f),
          RSrc::K(k) => kern::map_in(buf, |x| f(x, k)),
        }
      } else if stolen_is(b) {
        match ex.resolve(a) {
          RSrc::S(s) => kern::zip_in_b(s, buf, f),
          RSrc::K(k) => kern::map_in(buf, |y| f(k, y)),
        }
      } else {
        run_binary(buf, ex.resolve(a), ex.resolve(b), n, f);
      }
    }};
  }
  macro_rules! ternary {
    ($f:expr) => {{
      let f = $f;
      if stolen_is(a) {
        tern_in_place(buf, 0, ex.resolve(b), ex.resolve(c), f);
      } else if stolen_is(b) {
        tern_in_place(buf, 1, ex.resolve(a), ex.resolve(c), f);
      } else if stolen_is(c) {
        tern_in_place(buf, 2, ex.resolve(a), ex.resolve(b), f);
      } else {
        run_ternary(buf, ex.resolve(a), ex.resolve(b), ex.resolve(c), n, f);
      }
    }};
  }

  match kind {
    OpKind::Neg => unary!(sk::neg),
    OpKind::Abs => unary!(sk::abs),
    OpKind::Sqrt => unary!(sk::sqrt),
    OpKind::Sin => unary!(sk::sin),
    OpKind::Cos => unary!(sk::cos),
    OpKind::Tan => unary!(sk::tan),
    OpKind::Asin => unary!(sk::asin),
    OpKind::Acos => unary!(sk::acos),
    OpKind::Atan => unary!(sk::atan),
    OpKind::Exp => unary!(sk::exp),
    OpKind::Log2 => unary!(sk::log2),
    OpKind::Floor => unary!(sk::floor),
    OpKind::Ceil => unary!(sk::ceil),
    OpKind::Round => unary!(sk::round),
    OpKind::Fract => unary!(sk::fract),
    OpKind::Trunc => unary!(sk::trunc),
    OpKind::Sigmoid => unary!(sk::sigmoid),
    OpKind::Not => unary!(sk::not),
    OpKind::Add => binary!(sk::add),
    OpKind::Sub => binary!(sk::sub),
    OpKind::Mul => binary!(sk::mul),
    OpKind::Div => binary!(sk::div),
    OpKind::Mod => binary!(sk::rem),
    OpKind::Pow => binary!(sk::pow),
    OpKind::Atan2 => binary!(sk::atan2),
    OpKind::Min => binary!(sk::min),
    OpKind::Max => binary!(sk::max),
    OpKind::Gt => binary!(sk::gt),
    OpKind::Lt => binary!(sk::lt),
    OpKind::Gte => binary!(sk::gte),
    OpKind::Lte => binary!(sk::lte),
    OpKind::Eq => binary!(sk::eq),
    OpKind::Neq => binary!(sk::neq),
    OpKind::And => binary!(sk::and),
    OpKind::Or => binary!(sk::or),
    OpKind::Clamp => ternary!(sk::clamp),
    OpKind::SmoothStep => ternary!(sk::smoothstep),
    OpKind::LinearStep => ternary!(sk::linearstep),
    OpKind::LerpF => ternary!(sk::lerp_f),
    OpKind::LerpV => ternary!(sk::lerp_v),
    OpKind::Select => ternary!(kern::bitsel),
  }
}

/// The per-element kernels, one fn per `OpKind`. The executor's loops and compile-time
/// constant folding both go through these, so a folded constant is bit-identical to what
/// the loop would have produced.
mod sk {
  #![allow(clippy::missing_inline_in_public_items)]
  use super::kern;

  #[inline(always)]
  pub fn neg(x: f32) -> f32 {
    -x
  }
  #[inline(always)]
  pub fn abs(x: f32) -> f32 {
    x.abs()
  }
  #[inline(always)]
  pub fn sqrt(x: f32) -> f32 {
    x.sqrt()
  }
  #[inline(always)]
  pub fn sin(x: f32) -> f32 {
    x.sin()
  }
  #[inline(always)]
  pub fn cos(x: f32) -> f32 {
    x.cos()
  }
  #[inline(always)]
  pub fn tan(x: f32) -> f32 {
    x.tan()
  }
  #[inline(always)]
  pub fn asin(x: f32) -> f32 {
    x.asin()
  }
  #[inline(always)]
  pub fn acos(x: f32) -> f32 {
    x.acos()
  }
  #[inline(always)]
  pub fn atan(x: f32) -> f32 {
    x.atan()
  }
  #[inline(always)]
  pub fn exp(x: f32) -> f32 {
    x.exp()
  }
  #[inline(always)]
  pub fn log2(x: f32) -> f32 {
    x.log2()
  }
  #[inline(always)]
  pub fn floor(x: f32) -> f32 {
    x.floor()
  }
  #[inline(always)]
  pub fn ceil(x: f32) -> f32 {
    x.ceil()
  }
  #[inline(always)]
  pub fn round(x: f32) -> f32 {
    x.round()
  }
  #[inline(always)]
  pub fn fract(x: f32) -> f32 {
    x.fract()
  }
  #[inline(always)]
  pub fn trunc(x: f32) -> f32 {
    x.trunc()
  }
  #[inline(always)]
  pub fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
  }
  #[inline(always)]
  pub fn not(x: f32) -> f32 {
    1. - x
  }
  #[inline(always)]
  pub fn add(x: f32, y: f32) -> f32 {
    x + y
  }
  #[inline(always)]
  pub fn sub(x: f32, y: f32) -> f32 {
    x - y
  }
  #[inline(always)]
  pub fn mul(x: f32, y: f32) -> f32 {
    x * y
  }
  #[inline(always)]
  pub fn div(x: f32, y: f32) -> f32 {
    x / y
  }
  #[inline(always)]
  pub fn rem(x: f32, y: f32) -> f32 {
    x % y
  }
  #[inline(always)]
  pub fn pow(x: f32, y: f32) -> f32 {
    x.powf(y)
  }
  #[inline(always)]
  pub fn atan2(y: f32, x: f32) -> f32 {
    y.atan2(x)
  }
  #[inline(always)]
  pub fn min(x: f32, y: f32) -> f32 {
    x.min(y)
  }
  #[inline(always)]
  pub fn max(x: f32, y: f32) -> f32 {
    x.max(y)
  }
  #[inline(always)]
  pub fn gt(x: f32, y: f32) -> f32 {
    (x > y) as u32 as f32
  }
  #[inline(always)]
  pub fn lt(x: f32, y: f32) -> f32 {
    (x < y) as u32 as f32
  }
  #[inline(always)]
  pub fn gte(x: f32, y: f32) -> f32 {
    (x >= y) as u32 as f32
  }
  #[inline(always)]
  pub fn lte(x: f32, y: f32) -> f32 {
    (x <= y) as u32 as f32
  }
  #[inline(always)]
  pub fn eq(x: f32, y: f32) -> f32 {
    (x == y) as u32 as f32
  }
  #[inline(always)]
  pub fn neq(x: f32, y: f32) -> f32 {
    (x != y) as u32 as f32
  }
  #[inline(always)]
  pub fn and(x: f32, y: f32) -> f32 {
    x * y
  }
  #[inline(always)]
  pub fn or(x: f32, y: f32) -> f32 {
    x.max(y)
  }
  #[inline(always)]
  pub fn clamp(x: f32, lo: f32, hi: f32) -> f32 {
    crate::builtins::clampf(x, lo, hi)
  }
  #[inline(always)]
  pub fn smoothstep(x: f32, e0: f32, e1: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0., 1.);
    t * t * (3. - 2. * t)
  }
  #[inline(always)]
  pub fn linearstep(x: f32, e0: f32, e1: f32) -> f32 {
    ((x - e0) / (e1 - e0)).clamp(0., 1.)
  }
  #[inline(always)]
  pub fn lerp_f(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
  }
  /// nalgebra axpy skips the `beta * y` term when `beta == 0`, so `t == 1` must not read `a`.
  #[inline(always)]
  pub fn lerp_v(a: f32, b: f32, t: f32) -> f32 {
    let c = 1. - t;
    if c == 0. {
      t * b
    } else {
      t * b + c * a
    }
  }

  pub fn apply(kind: super::OpKind, a: f32, b: f32, c: f32) -> f32 {
    use super::OpKind::*;
    match kind {
      Neg => neg(a),
      Abs => abs(a),
      Sqrt => sqrt(a),
      Sin => sin(a),
      Cos => cos(a),
      Tan => tan(a),
      Asin => asin(a),
      Acos => acos(a),
      Atan => atan(a),
      Exp => exp(a),
      Log2 => log2(a),
      Floor => floor(a),
      Ceil => ceil(a),
      Round => round(a),
      Fract => fract(a),
      Trunc => trunc(a),
      Sigmoid => sigmoid(a),
      Not => not(a),
      Add => add(a, b),
      Sub => sub(a, b),
      Mul => mul(a, b),
      Div => div(a, b),
      Mod => rem(a, b),
      Pow => pow(a, b),
      Atan2 => atan2(a, b),
      Min => min(a, b),
      Max => max(a, b),
      Gt => gt(a, b),
      Lt => lt(a, b),
      Gte => gte(a, b),
      Lte => lte(a, b),
      Eq => eq(a, b),
      Neq => neq(a, b),
      And => and(a, b),
      Or => or(a, b),
      Clamp => clamp(a, b, c),
      SmoothStep => smoothstep(a, b, c),
      LinearStep => linearstep(a, b, c),
      LerpF => lerp_f(a, b, c),
      LerpV => lerp_v(a, b, c),
      Select => kern::bitsel(a, b, c),
    }
  }
}

fn param_slot_referenced(closure: &Closure, slot: usize) -> bool {
  let mut referenced = false;
  for stmt in &closure.body.0 {
    walk_stmt_shallow(stmt, &mut |e: &Expr| match e {
      Expr::Ident {
        res: VarRes::Local(s),
        ..
      } if *s as usize == slot => referenced = true,
      Expr::Closure {
        resolved: Some(m), ..
      } => {
        if m
          .captures
          .iter()
          .any(|(_, f)| matches!(f, CaptureFrom::Local(s) if *s as usize == slot))
        {
          referenced = true;
        }
      }
      _ => {}
    });
  }
  referenced
}

fn body_loc(closure: &Closure) -> SourceLoc {
  closure
    .body
    .0
    .first()
    .and_then(|s| s.exprs().next())
    .map(|e| e.loc())
    .unwrap_or_default()
}

#[derive(Clone, Copy)]
enum CompileKind<'a> {
  /// `t -> |val, uv, x_ix, y_ix| …` (one arity) and
  /// `texture_zip(fn, [a, b, …])` → `|in0, in1, …, uv, x_ix, y_ix|` (N).
  Zip { arities: &'a [u8] },
  /// `texture(w, h, |uv, x_ix, y_ix| …)`
  Generator,
}

impl CompileKind<'_> {
  fn n_inputs(self) -> usize {
    match self {
      CompileKind::Zip { arities } => arities.len(),
      CompileKind::Generator => 0,
    }
  }

  fn sig(self) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = FxHasher::default();
    match self {
      CompileKind::Zip { arities } => (1u8, arities).hash(&mut h),
      CompileKind::Generator => 0u8.hash(&mut h),
    }
    h.finish()
  }
}

fn compile(
  ctx: &EvalCtx,
  callable: &Rc<Callable>,
  closure: &Closure,
  kind: CompileKind,
) -> Result<(Plan, Vec<Value>), CErr> {
  let (xy_from, max_params) = (kind.n_inputs() + 1, kind.n_inputs() + 3);
  prefilter(closure, Some(xy_from))?;
  for param in closure.params.iter() {
    if !matches!(param.ident, crate::ast::DestructurePattern::Ident(_)) {
      return bail("destructuring closure param", SourceLoc::default());
    }
  }
  if closure.params.len() > max_params {
    return bail("texel closure with too many params", SourceLoc::default());
  }

  let fence = EffectFence::snapshot(ctx);
  let meta = &closure.resolved;
  let mut compiler = Compiler {
    ctx,
    steps: Vec::new(),
    unis: Vec::new(),
    uni_vals: Vec::new(),
    n_regs: 0,
    n_fbm: 0,
    n_gather: 0,
    n_folded: 0,
    cse: FxHashMap::default(),
    uni_cse: FxHashMap::default(),
    n_cse: 0,
    frames: Vec::new(),
    plan_frames: Vec::new(),
    inline_stack: vec![meta.id],
    lit_depth: 0,
    uses_uv: false,
    guard: None,
    guards: Vec::new(),
    step_guards: Vec::new(),
    branch_aborts: Vec::new(),
    spec_depth: 0,
  };
  compiler.push_frame(
    meta.n_slots,
    None,
    None,
    Rc::clone(&closure.captures),
    Rc::clone(callable),
  );

  // Params 0..N are the input texels — each a contiguous block of the concatenated input
  // planes — then uv. uv binds to the shared ctx-cached planes (zero ops, zero copies) and
  // only when the body actually references it, so uniform bodies never build the planes.
  if let CompileKind::Zip { arities } = kind {
    let mut base = 0usize;
    for (i, &arity) in arities.iter().enumerate() {
      if i < closure.params.len() {
        let mut chans = ArrayVec::new();
        for c in 0..arity as usize {
          chans.push(Src::In((base + c) as u8));
        }
        compiler.cur_mut().slot_abs[meta.param_slots[i] as usize] =
          SlotState::Varying(VV { chans, mask: false });
      }
      base += arity as usize;
    }
  }
  let uv_param_ix = xy_from - 1;
  if closure.params.len() > uv_param_ix {
    let uv_slot = meta.param_slots[uv_param_ix] as usize;
    if param_slot_referenced(closure, uv_slot) {
      let mut chans = ArrayVec::new();
      chans.push(Src::Uv(0));
      chans.push(Src::Uv(1));
      compiler.cur_mut().slot_abs[uv_slot] = SlotState::Varying(VV { chans, mask: false });
      compiler.uses_uv = true;
    }
  }

  let body_loc = body_loc(closure);
  let result = compiler.compile_statements(&closure.body.0, body_loc);
  compiler.pop_frame();
  if !fence.verify_or_restore(ctx) {
    return bail("uniform subtree performed an observable effect", body_loc);
  }
  let result = result?;

  let out = match result {
    AbsVal::V(vv) if vv.mask => return bail("texel body evaluates to a bool", body_loc),
    AbsVal::V(vv) => PlanOut::Chans(vv.chans),
    AbsVal::U(uix) => PlanOut::Uniform(uix),
    AbsVal::Seq(_) => return bail("texel body evaluates to a sequence", body_loc),
  };

  let out_regs: Vec<u16> = match &out {
    PlanOut::Chans(chans) => chans
      .iter()
      .filter_map(|s| match s {
        Src::Reg(r) => Some(*r),
        _ => None,
      })
      .collect(),
    PlanOut::Uniform(_) => Vec::new(),
  };
  let n_regs = compiler.n_regs as usize;

  // Dead-code elimination: every step is pure, so anything whose results are never read
  // (an unused channel of a `normalize`, a swizzled-away construction, a whole varying
  // spine under a uniform-valued body) simply goes. Steps are in dependency order, so one
  // backward sweep settles liveness.
  let mut live = vec![false; n_regs];
  for &r in &out_regs {
    live[r as usize] = true;
  }
  let mut keep = vec![false; compiler.steps.len()];
  for ix in (0..compiler.steps.len()).rev() {
    let step = &compiler.steps[ix];
    if step_dsts(step).iter().any(|&d| live[d as usize]) {
      keep[ix] = true;
      for s in step_srcs(step) {
        if let Src::Reg(r) = s {
          live[r as usize] = true;
        }
      }
    }
  }
  let n_dead = keep.iter().filter(|k| !**k).count() as u16;
  let mut keep_it = keep.iter();
  compiler.steps.retain(|_| *keep_it.next().unwrap());
  let mut keep_it = keep.iter();
  compiler.step_guards.retain(|_| *keep_it.next().unwrap());

  // Last read per register (write-only dsts free at their own step); outputs never free.
  let mut reg_last = vec![0u32; n_regs];
  for (ix, step) in compiler.steps.iter().enumerate() {
    for &d in step_dsts(step) {
      reg_last[d as usize] = ix as u32;
    }
    for s in step_srcs(step) {
      if let Src::Reg(r) = s {
        reg_last[r as usize] = ix as u32;
      }
    }
  }
  for &r in &out_regs {
    reg_last[r as usize] = u32::MAX;
  }

  // Peak live registers via step-order simulation, for the run-time memory gate.
  let mut live = vec![false; n_regs];
  let mut peak = 0u16;
  let mut cur = 0u16;
  for (step_ix, step) in compiler.steps.iter().enumerate() {
    for &d in step_dsts(step) {
      if !live[d as usize] {
        live[d as usize] = true;
        cur += 1;
        peak = peak.max(cur);
      }
    }
    for (r, l) in live.iter_mut().enumerate() {
      if *l && reg_last[r] == step_ix as u32 {
        *l = false;
        cur -= 1;
      }
    }
  }

  Ok((
    Plan {
      frames: compiler.plan_frames,
      n_regs: compiler.n_regs,
      steps: compiler.steps,
      step_guards: compiler.step_guards,
      guards: compiler.guards,
      branch_aborts: compiler.branch_aborts,
      unis: compiler.unis,
      n_fbm: compiler.n_fbm,
      n_gather: compiler.n_gather,
      reg_last,
      out,
      peak_regs: peak.max(1),
      uses_uv: compiler.uses_uv,
      n_folded: compiler.n_folded,
      n_cse: compiler.n_cse,
      n_dead,
      input_arities: match kind {
        CompileKind::Zip { arities } => arities.to_vec(),
        CompileKind::Generator => Vec::new(),
      },
    },
    compiler.uni_vals,
  ))
}

fn step_dsts(step: &Step) -> &[u16] {
  match step {
    Step::Op { dst, .. } => std::slice::from_ref(dst),
    Step::Fbm(f) => std::slice::from_ref(&f.dst),
    Step::Dyn(d) => &d.dst,
    Step::Gather(g) => &g.dst,
  }
}

fn step_srcs(step: &Step) -> ArrayVec<Src, 16> {
  let mut out = ArrayVec::new();
  match step {
    Step::Op { kind, a, b, c, .. } => out.extend([*a, *b, *c].into_iter().take(op_arity(*kind))),
    Step::Fbm(f) => out.extend(f.pos.iter().copied().take(f.dim as usize)),
    Step::Gather(g) => out.extend([g.u, g.v]),
    Step::Dyn(d) => {
      for chans in &d.args {
        for s in chans {
          if out.try_push(*s).is_err() {
            break;
          }
        }
      }
    }
  }
  out
}

/// Emission-time peephole. Only identities that are bit-exact in IEEE f32 qualify —
/// `x + 0.0` is not one (`-0.0 + 0.0 = +0.0`), nor is `x * 0.0` (NaN/∞, signed zero) —
/// plus all-constant operands, folded through the executor's own scalar kernels.
fn peephole(kind: OpKind, a: Src, b: Src, c: Src) -> Option<Src> {
  use Src::Const as K;
  let ar = op_arity(kind);
  let k = |s: Src| match s {
    K(v) => Some(v),
    _ => None,
  };
  if let (Some(ka), kb, kc) = (k(a), k(b), k(c)) {
    if (ar < 2 || kb.is_some()) && (ar < 3 || kc.is_some()) {
      return Some(K(sk::apply(kind, ka, kb.unwrap_or(0.), kc.unwrap_or(0.))));
    }
  }
  let is = |s: Src, bits: u32| matches!(s, K(v) if v.to_bits() == bits);
  const ONE: u32 = 0x3f80_0000;
  const POS_ZERO: u32 = 0;
  const NEG_ZERO: u32 = 0x8000_0000;
  match kind {
    OpKind::Mul if is(b, ONE) => Some(a),
    OpKind::Mul if is(a, ONE) => Some(b),
    OpKind::Div if is(b, ONE) => Some(a),
    OpKind::Sub if is(b, POS_ZERO) => Some(a),
    OpKind::Add if is(b, NEG_ZERO) => Some(a),
    OpKind::Add if is(a, NEG_ZERO) => Some(b),
    OpKind::Select if b == c => Some(b),
    OpKind::Select => match a {
      K(m) => Some(if m != 0. { b } else { c }),
      _ => None,
    },
    _ => None,
  }
}

/// `Ok(None)` = the body produced a non-numeric uniform value; hand off to the scalar path
/// for its error.
fn plan_output_planes(
  ctx: &EvalCtx,
  plan: &Plan,
  uni: &UniRun,
  input: &[Rc<Vec<f32>>],
  uv: Option<&[Rc<Vec<f32>>; 2]>,
  n: usize,
  step_ms: Option<&mut Vec<f64>>,
) -> Result<Option<Vec<Rc<Vec<f32>>>>, ErrorStack> {
  match &plan.out {
    PlanOut::Uniform(uix) => {
      let Some((chans, ar)) = value_chans(&uni.vals[*uix as usize]) else {
        return Ok(None);
      };
      Ok(Some(
        (0..ar as usize)
          .map(|c| Rc::new(vec![chans[c]; n]))
          .collect(),
      ))
    }
    PlanOut::Chans(_) => exec(ctx, plan, uni, input, uv, n, step_ms).map(Some),
  }
}

#[derive(Clone, Copy)]
enum VecTarget<'a> {
  /// One entry for `map`, N for `texture_zip`; all inputs share dims, arities are free.
  Zip { texs: &'a [&'a TextureHandle] },
  Gen {
    w: usize,
    h: usize,
    wrap: crate::TextureWrap,
  },
}

/// The vectorized fast path for `map` over a texture. `None` ⇒ run the scalar loop.
pub(crate) fn try_vectorized_map(
  ctx: &EvalCtx,
  cb: &Rc<Callable>,
  tex: &TextureHandle,
) -> Option<Result<Value, ErrorStack>> {
  try_vectorized(ctx, cb, VecTarget::Zip { texs: &[tex] })
}

/// The vectorized fast path for `texture_zip`. `None` ⇒ run the scalar loop. Callers
/// validate matching dims and a non-empty input list first.
pub(crate) fn try_vectorized_zip(
  ctx: &EvalCtx,
  cb: &Rc<Callable>,
  texs: &[&TextureHandle],
) -> Option<Result<Value, ErrorStack>> {
  try_vectorized(ctx, cb, VecTarget::Zip { texs })
}

/// The vectorized fast path for `texture(w, h, generator)`. `None` ⇒ run the scalar loop.
pub(crate) fn try_vectorized_texture(
  ctx: &EvalCtx,
  cb: &Rc<Callable>,
  w: usize,
  h: usize,
  wrap: crate::TextureWrap,
) -> Option<Result<Value, ErrorStack>> {
  try_vectorized(ctx, cb, VecTarget::Gen { w, h, wrap })
}

fn cache_plan(state: &VectorizeState, key: PlanKey, entry: PlanEntry) {
  let mut plans = state.plans.borrow_mut();
  let mut order = state.plan_order.borrow_mut();
  if plans.len() >= MAX_CACHED_PLANS {
    for k in order.drain(..MAX_CACHED_PLANS / 4) {
      plans.remove(&k);
    }
  }
  if plans.insert(key, entry).is_none() {
    order.push_back(key);
  }
}

fn try_vectorized(
  ctx: &EvalCtx,
  cb: &Rc<Callable>,
  target: VecTarget,
) -> Option<Result<Value, ErrorStack>> {
  let state = &ctx.tex_vectorize;
  if state.no_vectorize.get() {
    return None;
  }
  let Callable::Closure(closure) = &**cb else {
    return None;
  };
  let (w, h) = match target {
    VecTarget::Zip { texs } => (texs[0].width, texs[0].height),
    VecTarget::Gen { w, h, .. } => (w, h),
  };
  if w * h < MIN_TEXELS {
    return None;
  }

  let arities: Vec<u8> = match target {
    VecTarget::Zip { texs } => texs.iter().map(|t| t.channels as u8).collect(),
    VecTarget::Gen { .. } => Vec::new(),
  };
  let kind = match target {
    VecTarget::Zip { .. } => CompileKind::Zip { arities: &arities },
    VecTarget::Gen { .. } => CompileKind::Generator,
  };
  let key = PlanKey {
    body_id: closure.resolved.id,
    input_sig: kind.sig(),
    capture_sig: capture_sig(closure),
  };
  // Every invocation records an outcome, cache hits and post-compile aborts included — a
  // stale `vectorized: true` from an earlier run is the one thing the diagnostic can't say.
  let loc = ctx.resolve_loc(body_loc(closure));
  let report_with =
    |vectorized: bool, reason: Option<String>, loc: (u32, u32), plan: Option<String>| {
      state.reports.borrow_mut().insert(
        key.body_id,
        VectorizeReport {
          vectorized,
          reason,
          loc,
          module: ctx.current_module.borrow().clone(),
          plan,
        },
      );
    };
  let report = |vectorized: bool, reason: Option<String>, loc: (u32, u32)| {
    report_with(vectorized, reason, loc, None)
  };
  let abort = |reason: &str| {
    report(
      false,
      Some(format!("aborted to scalar this run: {reason}")),
      loc,
    )
  };

  if arities.iter().map(|&a| a as usize).sum::<usize>() > 256 {
    report(false, Some("more than 256 input planes".into()), loc);
    return None;
  }

  let mut cached = state.plans.borrow().get(&key).cloned();
  let mut fresh = false;
  let (plan, uni_vals, guards) = loop {
    let (plan, vals, guards) = match cached.take() {
      Some(PlanEntry::Bail(reason, bail_loc)) => {
        report(false, Some(reason.to_string()), bail_loc);
        return None;
      }
      Some(PlanEntry::Ok(plan)) => match eval_uniforms(ctx, &plan, closure, cb) {
        Ok((vals, guards)) => (plan, vals, guards),
        Err(UniErr::Recompile) => {
          state.plans.borrow_mut().remove(&key);
          state.plan_order.borrow_mut().retain(|k| *k != key);
          continue;
        }
        Err(UniErr::Abort) => {
          abort("uniform subtree changed shape or performed an observable effect");
          return None;
        }
        Err(UniErr::Err(e)) => return Some(Err(e)),
      },
      None => match compile(ctx, cb, closure, kind) {
        Ok((plan, vals)) => {
          let plan = Rc::new(plan);
          cache_plan(state, key, PlanEntry::Ok(Rc::clone(&plan)));
          let guards = all_guards(&plan, &vals).expect("compile saw every cond as a bool");
          fresh = true;
          (plan, vals, guards)
        }
        Err(CErr::Bail(reason, bail_loc)) => {
          let bail_loc = ctx.resolve_loc(bail_loc);
          cache_plan(
            state,
            key,
            PlanEntry::Bail(reason.as_str().into(), bail_loc),
          );
          report(false, Some(reason), bail_loc);
          return None;
        }
        Err(CErr::Err(e)) => return Some(Err(e)),
      },
    };
    let Some(ba) = plan.branch_aborts.iter().find(|b| guards[b.guard as usize]) else {
      break (plan, vals, guards);
    };
    // A value-dependent arm failure (an error under an earlier run's uniforms) is retried
    // once by recompiling under this run's values; a structural one is final.
    if ba.evict && !fresh {
      state.plans.borrow_mut().remove(&key);
      state.plan_order.borrow_mut().retain(|k| *k != key);
      continue;
    }
    report(
      false,
      Some(format!(
        "aborted to scalar this run: selected branch not vectorizable: {}",
        ba.reason
      )),
      ba.loc,
    );
    return None;
  };
  if plan.peak_regs as u64 * w as u64 * h as u64 * 4 > REG_BYTE_BUDGET {
    abort("register budget exceeded");
    return None;
  }
  let Some(uni) = validate_uniforms(&plan, uni_vals, guards) else {
    abort("uniform shape changed");
    return None;
  };
  let input: Vec<Rc<Vec<f32>>> = match target {
    VecTarget::Zip { texs } => texs.iter().flat_map(|t| t.as_planes()).collect(),
    VecTarget::Gen { .. } => Vec::new(),
  };
  let uv = plan.uses_uv.then(|| uv_planes_for(ctx, w, h));
  let profile = state.profile.get();
  let mut step_ms = Vec::new();
  let t0 = profile.then(now_ms);
  let planes = match plan_output_planes(
    ctx,
    &plan,
    &uni,
    &input,
    uv.as_ref(),
    w * h,
    profile.then_some(&mut step_ms),
  ) {
    Ok(Some(planes)) => planes,
    Ok(None) => {
      abort("body produced a non-numeric uniform value");
      return None;
    }
    Err(e) => return Some(Err(e)),
  };
  let listing = t0.map(|t0| {
    let total_ms = now_ms() - t0;
    render_plan(&plan, &uni, &step_ms, total_ms, kind, w, h, !fresh, loc)
  });
  report_with(true, None, loc, listing);

  let channels = planes.len();
  let storage = TexStorage::planes(planes);
  Some(Ok(Value::Texture(Rc::new(match target {
    VecTarget::Zip { texs } => TextureHandle {
      channels,
      storage,
      mips: Default::default(),
      ..texs[0].clone()
    },
    VecTarget::Gen { w, h, wrap } => TextureHandle {
      storage,
      width: w,
      height: h,
      channels,
      wrap,
      min_filter: None,
      mag_filter: None,
      format: None,
      transform: crate::Mat4::identity(),
      mips: Default::default(),
    },
  }))))
}

// ---------------------------------------------------------------------------------------
// Plan listing (diagnostics)
// ---------------------------------------------------------------------------------------

fn src_name(s: Src, plan: &Plan) -> String {
  const CH: [char; 4] = ['x', 'y', 'z', 'w'];
  match s {
    Src::Reg(r) => format!("r{r}"),
    Src::In(c) => {
      let (i, ch) = plan.input_chan(c);
      if plan.input_arities.len() == 1 {
        format!("in.{}", CH[ch])
      } else {
        format!("in{i}.{}", CH[ch])
      }
    }
    Src::Uv(c) => format!("uv.{}", CH[c as usize]),
    Src::Uni(u, c) => match plan.unis[u as usize].shape {
      UniShape::Num { ar, .. } if ar > 1 => format!("u{u}.{}", CH[c as usize]),
      _ => format!("u{u}"),
    },
    Src::Const(k) => format!("{k:?}"),
  }
}

fn op_arity(kind: OpKind) -> usize {
  use OpKind::*;
  match kind {
    Neg | Abs | Sqrt | Sin | Cos | Tan | Asin | Acos | Atan | Exp | Log2 | Floor | Ceil | Round
    | Fract | Trunc | Sigmoid | Not => 1,
    Clamp | SmoothStep | LinearStep | LerpF | LerpV | Select => 3,
    _ => 2,
  }
}

fn value_brief(v: &Value) -> String {
  match v {
    Value::Float(f) => format!("{f:?}"),
    Value::Int(i) => i.to_string(),
    Value::Bool(b) => b.to_string(),
    Value::Vec2(v) => format!("({:?}, {:?})", v.x, v.y),
    Value::Vec3(v) => format!("({:?}, {:?}, {:?})", v.x, v.y, v.z),
    Value::Vec4(v) => format!("({:?}, {:?}, {:?}, {:?})", v.x, v.y, v.z, v.w),
    Value::Texture(t) => format!("texture {}×{}×{}", t.width, t.height, t.channels),
    Value::Nil => "nil".into(),
    other => {
      let d = format!("{other:?}");
      if d.len() > 40 {
        format!("{}…", &d[..d.floor_char_boundary(40)])
      } else {
        d
      }
    }
  }
}

/// Human-readable dump of one invocation: header, uniform table (this run's values), guards,
/// every step with its operands / guard / skip state / wall time, and the output routing.
/// Registers are 1-channel planes, so swizzles/constructions show up as routing, not steps.
#[allow(clippy::too_many_arguments)]
fn render_plan(
  plan: &Plan,
  uni: &UniRun,
  step_ms: &[f64],
  total_ms: f64,
  kind: CompileKind,
  w: usize,
  h: usize,
  cache_hit: bool,
  loc: (u32, u32),
) -> String {
  use std::fmt::Write as _;
  let mut o = String::new();
  let what = match kind {
    CompileKind::Zip { arities: [ar] } => format!("map {ar}ch"),
    CompileKind::Zip { arities } => format!(
      "zip {}",
      arities
        .iter()
        .map(|a| format!("{a}ch"))
        .collect::<Vec<_>>()
        .join(",")
    ),
    CompileKind::Generator => "generator".into(),
  };
  let peak_bytes = plan.peak_regs as f64 * (w * h * 4) as f64;
  let peak = if peak_bytes >= 1024. * 1024. {
    format!("{:.1} MB", peak_bytes / (1024. * 1024.))
  } else {
    format!("{:.0} KB", peak_bytes / 1024.)
  };
  let _ = writeln!(
    o,
    "; body @{}:{} · {what} · {w}×{h} · {} steps ({} folded, {} cse, {} dead removed) · peak {} \
     regs ≈ {peak} · plan cache {} · exec {total_ms:.3} ms",
    loc.0,
    loc.1,
    plan.steps.len(),
    plan.n_folded,
    plan.n_cse,
    plan.n_dead,
    plan.peak_regs,
    if cache_hit { "hit" } else { "miss" },
  );
  if !plan.unis.is_empty() {
    let _ = writeln!(o, "uniforms:");
    for (i, (step, val)) in plan.unis.iter().zip(&uni.vals).enumerate() {
      let off = step.guard.is_some_and(|g| !uni.guards[g as usize]);
      let src = match &step.src {
        UniSrc::Expr(_) => "expr".to_string(),
        UniSrc::Const(_) => "const".into(),
        UniSrc::Capture(c) => format!("capture#{c}"),
        UniSrc::Slot(sl) => format!("local#{sl}"),
        UniSrc::UniRef(u) => format!("= u{u}"),
        UniSrc::SwizzleOf { of, field } => format!("u{of}.{field}"),
        UniSrc::SeqElem { of, ix } => format!("u{of}[{ix}]"),
      };
      let shape = match step.shape {
        UniShape::Num { ar, int } => match (ar, int) {
          (1, true) => "int".into(),
          (1, false) => "float".into(),
          (n, _) => format!("vec{n}"),
        },
        UniShape::Bool => "bool".into(),
        UniShape::Seq { len } => format!("seq[{len}]"),
        UniShape::Builtin(_) => "builtin".into(),
        UniShape::ClosureBody(_) => "closure".into(),
        UniShape::Dynamic(ar) => format!("dynamic→{ar}ch"),
        UniShape::Texture(ch) => format!("texture {ch}ch"),
        UniShape::Any => "raw".into(),
      };
      let guard = match step.guard {
        Some(g) if off => format!(" [g{g} off]"),
        Some(g) => format!(" [g{g}]"),
        None => String::new(),
      };
      let val = if off {
        "—".to_string()
      } else {
        value_brief(val)
      };
      let _ = writeln!(o, "  u{i:<3} = {val:<18} {shape:<7} ({src}){guard}");
    }
  }
  if !plan.guards.is_empty() {
    let _ = writeln!(o, "guards:");
    for (i, g) in plan.guards.iter().enumerate() {
      let parent = g.parent.map(|p| format!(" && g{p}")).unwrap_or_default();
      let _ = writeln!(
        o,
        "  g{i:<3} = u{} == {}{parent}   → {}",
        g.cond,
        g.expect,
        if uni.guards[i] { "on" } else { "off" }
      );
    }
  }
  let _ = writeln!(o, "steps:");
  for (ix, step) in plan.steps.iter().enumerate() {
    let (dst, body) = match step {
      Step::Op { kind, dst, a, b, c } => {
        let srcs: Vec<String> = [*a, *b, *c]
          .iter()
          .take(op_arity(*kind))
          .map(|s| src_name(*s, plan))
          .collect();
        (
          format!("r{dst}"),
          format!(
            "{:<7} {}",
            format!("{kind:?}").to_lowercase(),
            srcs.join(", ")
          ),
        )
      }
      Step::Fbm(f) => {
        let pos: Vec<String> = f
          .pos
          .iter()
          .take(f.dim as usize)
          .map(|s| src_name(*s, plan))
          .collect();
        let [seed, oct, freq, lac, pers] = f.params;
        let tile = f
          .tileable
          .map(|t| format!(" tile=u{t}"))
          .unwrap_or_default();
        (
          format!("r{}", f.dst),
          format!(
            "fbm{}d   ({}) seed=u{seed} oct=u{oct} freq=u{freq} lac=u{lac} pers=u{pers}{tile}",
            f.dim,
            pos.join(", ")
          ),
        )
      }
      Step::Gather(g) => {
        let dst: Vec<String> = g.dst.iter().map(|r| format!("r{r}")).collect();
        let src = match &uni.gather[g.rix as usize] {
          Some(r) => format!(
            "{}x{}x{} {:?} {:?}",
            r.tex.width, r.tex.height, r.tex.channels, r.filter, r.wrap
          ),
          None => "—".into(),
        };
        (
          dst.join(","),
          format!(
            "gather  ({}, {}) tex=u{} {src}",
            src_name(g.u, plan),
            src_name(g.v, plan),
            g.tex
          ),
        )
      }
      Step::Dyn(d) => {
        let name = match &d.callee {
          DynCallee::Baked(c) => match &**c {
            Callable::Dynamic { name, .. } => name.clone(),
            _ => "?".into(),
          },
          DynCallee::Uni(u) => match &uni.vals[*u as usize] {
            Value::Callable(c) => match &**c {
              Callable::Dynamic { name, .. } => name.clone(),
              _ => "?".into(),
            },
            _ => "?".into(),
          },
        };
        let args: Vec<String> = d
          .args
          .iter()
          .map(|chans| {
            let cs: Vec<String> = chans.iter().map(|s| src_name(*s, plan)).collect();
            if cs.len() == 1 {
              cs[0].clone()
            } else {
              format!("({})", cs.join(", "))
            }
          })
          .collect();
        let dst: Vec<String> = d.dst.iter().map(|r| format!("r{r}")).collect();
        (
          dst.join(","),
          format!("dyn     {name}({}) per-texel", args.join(", ")),
        )
      }
    };
    let guard = match plan.step_guards[ix] {
      Some(g) if !uni.guards[g as usize] => format!("[g{g} off]"),
      Some(g) => format!("[g{g}]"),
      None => String::new(),
    };
    let ms = match step_ms.get(ix) {
      Some(t) if t.is_nan() => "—".to_string(),
      Some(t) => format!("{t:.3} ms"),
      None => String::new(),
    };
    let guard_w = if plan.guards.is_empty() { 0 } else { 9 };
    let _ = writeln!(o, "  {dst:<4} = {body:<30} {guard:<guard_w$} {ms:>9}");
  }
  match &plan.out {
    PlanOut::Chans(chans) => {
      let names: Vec<String> = chans.iter().map(|s| src_name(*s, plan)).collect();
      let _ = writeln!(o, "out:   [{}]", names.join(", "));
    }
    PlanOut::Uniform(u) => {
      let _ = writeln!(o, "out:   broadcast u{u}");
    }
  }
  o
}

/// Bit-exact comparison for `GEOSCRIPT_VECTORIZE_VERIFY`, modulo NaN payload: when both
/// operands of a commutative op are NaN, which payload survives depends on operand order,
/// which LLVM canonicalizes differently per inlining context (observed on float `lerp`'s
/// `a + (b - a) * t`). Any NaN therefore matches any NaN; everything else must match bitwise.
pub fn assert_bit_identical(vec_val: &Value, scalar_val: &Value) -> Result<(), ErrorStack> {
  let (Value::Texture(a), Value::Texture(b)) = (vec_val, scalar_val) else {
    return Err(ErrorStack::new(format!(
      "VECTORIZE_VERIFY: non-texture results: {vec_val:?} vs {scalar_val:?}"
    )));
  };
  if (a.width, a.height, a.channels) != (b.width, b.height, b.channels) {
    return Err(ErrorStack::new(format!(
      "VECTORIZE_VERIFY: shape mismatch: {}x{}x{} (vectorized) vs {}x{}x{} (scalar)",
      a.width, a.height, a.channels, b.width, b.height, b.channels
    )));
  }
  let (pa, pb) = (a.as_planes(), b.as_planes());
  for (c, (x, y)) in pa.iter().zip(&pb).enumerate() {
    for (i, (&va, &vb)) in x.iter().zip(y.iter()).enumerate() {
      if va.to_bits() != vb.to_bits() && !(va.is_nan() && vb.is_nan()) {
        return Err(ErrorStack::new(format!(
          "VECTORIZE_VERIFY: first differing texel: chan {c}, texel {i} ({}, {}): vectorized \
           {va:?} (bits {:#010x}) vs scalar {vb:?} (bits {:#010x})",
          i % a.width,
          i / a.width,
          va.to_bits(),
          vb.to_bits()
        )));
      }
    }
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::parse_and_eval_program;

  /// Both paths pinned explicitly: an ambient `GEOSCRIPT_NO_VECTORIZE=1` would otherwise
  /// turn every differential test into scalar-vs-scalar.
  fn eval_both(src: &str) -> (crate::EvalCtx, crate::EvalCtx) {
    let vec_ctx = EvalCtx::default();
    vec_ctx.tex_vectorize.no_vectorize.set(false);
    crate::parse_and_eval_program_with_ctx(src.to_string(), &vec_ctx, false).unwrap();
    let scalar_ctx = EvalCtx::default();
    scalar_ctx.tex_vectorize.no_vectorize.set(true);
    crate::parse_and_eval_program_with_ctx(src.to_string(), &scalar_ctx, false).unwrap();
    (vec_ctx, scalar_ctx)
  }

  fn assert_identical_outputs(vec_ctx: &EvalCtx, scalar_ctx: &EvalCtx) {
    let (a, b) = (
      vec_ctx.rendered_textures.borrow(),
      scalar_ctx.rendered_textures.borrow(),
    );
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(b.iter()) {
      assert_bit_identical(
        &Value::Texture(Rc::clone(&x.texture)),
        &Value::Texture(Rc::clone(&y.texture)),
      )
      .unwrap();
    }
  }

  fn reports(ctx: &EvalCtx) -> Vec<VectorizeReport> {
    ctx
      .tex_vectorize
      .reports
      .borrow()
      .values()
      .cloned()
      .collect()
  }

  #[test]
  fn vectorizes_canonical_body_bit_identical() {
    let src = r#"
h = texture(16, 16, |uv| fbm(pos=uv * 3., seed=7))
cap = 0.35
out = h -> |v, uv| {
  base = sigmoid(v * 3. - cap)
  n = fbm(pos=uv * 8. + v2(v, 0.), octaves=3)
  v3(base, clamp(0., 1., n * 0.5 + 0.5), uv.x) * v3(1., 0.8, 0.6)
}
out | render_texture(name="o")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    let reps = reports(&vec_ctx);
    assert!(
      reps.iter().any(|r| r.vectorized),
      "expected the map body to vectorize; reports: {reps:?}"
    );
  }

  #[test]
  fn sample_gathers_bit_identical() {
    let src = r#"
src = texture(16, 12, |uv| v3(uv.x, uv.y, fbm(pos=uv * 3.)))
g = src.r
w1 = texture(16, 12, |uv| sample(src, uv + v2(0.05 * sin(uv.y * tau), 0.)))
w2 = texture(16, 12, |uv| sample(src, uv * 1.5 - 0.25, filter="nearest", wrap="clamp"))
w3 = texture(16, 12, |uv| sample(flip_x(src), v2(uv.y, uv.x) * 2., wrap="mirror"))
w4 = texture(16, 12, |uv| sample(transpose(src), uv.yx * 3. - 1., filter="nearest", wrap="mirror"))
w5 = texture(16, 12, |uv| sample(g, v2(uv.x, 0.5), filter="nearest"))
w6 = src -> |p, uv| p * sample(src, uv + v2(0.1, 0.)).bgr
w7 = [src, g] | texture_zip(|p, m, uv| sample(src, uv + v2(m, m) * 0.1) + p * 0.5)
w8 = texture(16, 12, |uv| { s = sample(src, uv * 2.); s.r + sample(src, uv * 2.).g })
warp = |t| texture(16, 12, |uv| sample(t, uv * 2.))
w9 = warp(g)
w10 = warp(src)
w11 = warp(g.rrr)
texs = [g, src]
mk = |i| texture(16, 12, |uv| sample(texs[i], uv * 1.5))
w12 = mk(0)
w13 = mk(1)
w1 | render_texture(name="w1")
w2 | render_texture(name="w2")
w3 | render_texture(name="w3")
w4 | render_texture(name="w4")
w5 | render_texture(name="w5")
w6 | render_texture(name="w6")
w7 | render_texture(name="w7")
w8 | render_texture(name="w8")
w9 | render_texture(name="w9")
w10 | render_texture(name="w10")
w11 | render_texture(name="w11")
w12 | render_texture(name="w12")
w13 | render_texture(name="w13")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    let reps = reports(&vec_ctx);
    assert!(
      reps.len() >= 10 && reps.iter().all(|r| r.vectorized),
      "{reps:?}"
    );
    let plans = vec_ctx.tex_vectorize.plans.borrow();
    let gathers: usize = plans
      .values()
      .map(|p| match p {
        PlanEntry::Ok(p) => p
          .steps
          .iter()
          .filter(|s| matches!(s, Step::Gather(_)))
          .count(),
        PlanEntry::Bail(..) => 0,
      })
      .sum();
    assert!(gathers >= 10, "{gathers}");
    drop(plans);

    // Two reads at the same coordinate share one gather step through CSE.
    let (vec_ctx, _) = eval_both(
      r#"
src = texture(16, 12, |uv| v3(uv.x, uv.y, 0.5))
w8 = texture(16, 12, |uv| { s = sample(src, uv * 2.); s.r + sample(src, uv * 2.).g })
w8 | render_texture(name="w8")
"#,
    );
    let plans = vec_ctx.tex_vectorize.plans.borrow();
    let gathers: Vec<usize> = plans
      .values()
      .filter_map(|p| match p {
        PlanEntry::Ok(p) => Some(
          p.steps
            .iter()
            .filter(|s| matches!(s, Step::Gather(_)))
            .count(),
        ),
        PlanEntry::Bail(..) => None,
      })
      .collect();
    assert_eq!(gathers.iter().sum::<usize>(), 1, "{gathers:?}");
  }

  /// Nearest sampling at texel centers is an exact identity, and whole-pixel offsets match
  /// `roll` bit-for-bit — the property that makes `sample` usable as an exact gather.
  #[test]
  fn sample_nearest_is_exact() {
    let src = r#"
src = texture(16, 12, |uv| v3(uv.x, uv.y, fbm(pos=uv * 3.)))
id = texture(16, 12, |uv| sample(src, uv, filter="nearest"))
rolled = texture_roll(3, -2, src)
shifted = texture(16, 12, |uv| sample(src, uv - v2(3., -2.) / v2(16., 12.), filter="nearest"))
"#;
    let (vec_ctx, _) = eval_both(src);
    let get = |n: &str| vec_ctx.get_global(n).unwrap();
    assert_bit_identical(&get("id"), &get("src")).unwrap();
    assert_bit_identical(&get("shifted"), &get("rolled")).unwrap();
  }

  #[test]
  fn full_surface_bit_identical() {
    let src = r#"
h = texture(16, 16, |uv| fbm(pos=uv * 4.))
g = texture(16, 16, |uv| uv.y - 0.5)
rgb = h -> |v, uv| v3(v, uv.x, uv.y)
o1 = h -> |v| lerp(0.3, v, sqrt(abs(v)) + pow(v * v, 2.))
o2 = h -> |v| atan2(v, 0.5) + atan2(0.25, v) - sin(cos(tan(v)))
o3 = h -> |v| min(max(v, 0.1), 0.9) % 0.4 + floor(v * 4.) - ceil(v) + round(v) + fract(v) - trunc(v)
o4 = rgb -> |c| normalize(c + 0.1) * len(c) + v3(dot(c, c.bgr), distance(c, c * 0.5), c.g)
o5 = rgb -> |c| c.zyx - -c.rgb * 2.
o6 = h -> |v| smoothstep(0.2, 0.8, v) + exp(-v) * log2(v + 2.) + asin(clamp(-1., 1., v)) + acos(clamp(-1., 1., v))
o7 = h -> |v, uv| lerp(v, uv.x, uv.y)
o8 = rgb -> |c| lerp(c.r, c, c * 0.5 + 0.1)
o1 | render_texture(name="o1")
o2 | render_texture(name="o2")
o3 | render_texture(name="o3")
o4 | render_texture(name="o4")
o5 | render_texture(name="o5")
o6 | render_texture(name="o6")
o7 | render_texture(name="o7")
o8 | render_texture(name="o8")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    let reps = reports(&vec_ctx);
    let n_vec = reps.iter().filter(|r| r.vectorized).count();
    assert!(
      n_vec >= 8,
      "expected all bodies to vectorize; reports: {reps:?}"
    );
  }

  #[test]
  fn uniform_body_broadcast_fills() {
    let src = r#"
h = texture(16, 16, |uv| uv.x)
cap = 2.5
(h -> |v| cap * 2. + 1.) | render_texture(name="o")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    let t = &vec_ctx.rendered_textures.borrow()[0].texture.clone();
    assert!(t.as_planes()[0].iter().all(|&x| x == 6.));
  }

  /// One case per bail row of the case matrix; each must fall back to the scalar path
  /// (bit-identical, checked via eval_both in the passing tests) AND report a reason
  /// naming the offending construct.
  #[test]
  fn bails_report_reasons() {
    for (src, needle) in [
      (
        r#"t = texture(16, 16, |uv| uv.x)
(t -> |v| { if v > 0.5 { 1. }
v }) | render_texture(name="o")"#,
        "without `else`",
      ),
      (
        r#"t = texture(16, 16, |uv| uv.x)
(t -> |v, uv, x| v + float(x)) | render_texture(name="o")"#,
        "x_ix",
      ),
      (
        r#"t = texture(16, 16, |uv| uv.x)
(t -> |v| v3(v, 1., 2.)[0] + v) | render_texture(name="o")"#,
        "indexing a varying",
      ),
      (
        r#"t = texture(16, 16, |uv| uv.x)
(t -> |v| { [a, b] = [v, v * 2.]
a + b }) | render_texture(name="o")"#,
        "destructuring",
      ),
      (
        r#"t = texture(16, 16, |uv| uv.x)
(t -> |v| { print(v)
v }) | render_texture(name="o")"#,
        "side-effectful",
      ),
      (
        r#"set_rng_seed(3)
t = texture(16, 16, |uv| uv.x)
(t -> |v| v + randf()) | render_texture(name="o")"#,
        "rng",
      ),
      (
        r#"t = texture(16, 16, |uv| uv.x)
(t -> |v| v > 0.5) | render_texture(name="o")"#,
        "!err",
      ),
      (
        r#"t = texture(16, 16, |uv| uv.x)
(t -> |v| if v { 1. } else { 0. }) | render_texture(name="o")"#,
        "!err",
      ),
      (
        r#"t = texture(16, 16, |uv| uv.x)
(t -> |v| (v > 0.5) + 1.) | render_texture(name="o")"#,
        "!err",
      ),
    ] {
      if needle == "!err" {
        // body returns a bool: both paths must error
        for no_vec in [false, true] {
          let ctx = EvalCtx::default();
          ctx.tex_vectorize.no_vectorize.set(no_vec);
          assert!(crate::parse_and_eval_program_with_ctx(src.to_string(), &ctx, false).is_err());
        }
        continue;
      }
      let (ctx, scalar_ctx) = eval_both(src);
      assert_identical_outputs(&ctx, &scalar_ctx);
      let reps = reports(&ctx);
      if needle.is_empty() {
        assert!(
          reps.iter().any(|r| !r.vectorized),
          "expected a bail; reports: {reps:?}\n{src}"
        );
      } else {
        assert!(
          reps
            .iter()
            .any(|r| !r.vectorized && r.reason.as_deref().is_some_and(|s| s.contains(needle))),
          "expected bail reason containing {needle:?}; reports: {reps:?}\n{src}"
        );
      }
    }
  }

  /// A captured rng-drawing closure invoked from a "uniform" position must not be hoisted:
  /// the effect fence detects the draw, restores the rng, and bails to the scalar path.
  #[test]
  fn effect_fence_catches_captured_rng() {
    let src = r#"
set_rng_seed(5)
place = || randf()
t = texture(16, 16, |uv| uv.x)
(t -> |v| v + place()) | render_texture(name="o")
end = randf()
print(end)
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    assert_eq!(
      *vec_ctx.prints.borrow(),
      *scalar_ctx.prints.borrow(),
      "rng stream must be preserved exactly"
    );
    let reps = reports(&vec_ctx);
    assert!(
      reps.iter().any(|r| !r.vectorized
        && r
          .reason
          .as_deref()
          .is_some_and(|s| s.contains("observable effect"))),
      "map body must bail via the effect fence: {reps:?}"
    );
  }

  /// One body, multiple invocations with different captured uniforms (the stack-slice
  /// shape): the compiled plan is reused and every invocation stays bit-identical.
  #[test]
  fn plan_cache_reuses_across_captures() {
    let src = r#"
h = texture(16, 16, |uv| fbm(pos=uv * 3.))
slices = 0..4 -> |i| (h -> |v| 1. - smoothstep(i * 0.07, i * 0.07 + 0.4, v))
slices | render_texture_stack(name="s")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    {
      let (a, b) = (
        vec_ctx.rendered_textures.borrow(),
        scalar_ctx.rendered_textures.borrow(),
      );
      for (x, y) in [(&a[0], &b[0])] {
        assert_bit_identical(
          &Value::Texture(Rc::clone(&x.texture)),
          &Value::Texture(Rc::clone(&y.texture)),
        )
        .unwrap();
        for (xs, ys) in x.extra_slices.iter().zip(&y.extra_slices) {
          assert_bit_identical(
            &Value::Texture(Rc::clone(xs)),
            &Value::Texture(Rc::clone(ys)),
          )
          .unwrap();
        }
      }
    }
    assert!(reports(&vec_ctx).iter().any(|r| r.vectorized));
    // one plan for the shared map body across all 4 captures + one for the generator
    assert_eq!(vec_ctx.tex_vectorize.plans.borrow().len(), 2);
  }

  #[test]
  fn inlines_pure_helper_closures() {
    let src = r#"
ridge = |x| 1. - abs(x * 2. - 1.)
tint = |c, amt| c * v3(1., amt, amt * amt)
h = texture(16, 16, |uv| fbm(pos=uv * 3.))
(h -> |v| ridge(sigmoid(v))) | render_texture(name="a")
(h -> |v, uv| tint(v3(v, uv.x, uv.y), ridge(v))) | render_texture(name="b")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    let reps = reports(&vec_ctx);
    assert_eq!(
      reps.iter().filter(|r| r.vectorized).count(),
      3,
      "helper-closure calls must inline, not bail (2 maps + generator): {reps:?}"
    );
  }

  #[test]
  fn inline_uses_helper_captures_and_defaults() {
    let src = r#"
k = 0.75
scaled = |x, s = (k * 2.)| x * s + k
h = texture(16, 16, |uv| uv.x)
(h -> |v| scaled(v)) | render_texture(name="a")
(h -> |v| scaled(v, 0.25)) | render_texture(name="b")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    assert!(reports(&vec_ctx).iter().filter(|r| r.vectorized).count() >= 2);
  }

  /// A terminating recursive helper bails (via its conditional — any terminating recursion
  /// has one) and the scalar fallback stays bit-identical.
  #[test]
  fn recursive_closure_bails() {
    let src = r#"
rec = |x| if x > 100. { x } else { rec(x + 100.) }
h = texture(16, 16, |uv| uv.x)
(h -> |v| rec(v)) | render_texture(name="a")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    assert!(reports(&vec_ctx).iter().any(|r| !r.vectorized));
  }

  /// The recursion guard proper: an unconditionally-recursive helper would inline forever
  /// (and stack-overflow the scalar path if executed), so compile it directly without ever
  /// running the map and assert the bail names the recursion.
  #[test]
  fn recursion_guard_bails_at_compile() {
    let src = r#"
rec = |x| x + rec(x)
t = texture(16, 16, |uv| uv.x)
f = |v| rec(v)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let Some(Value::Callable(f)) = ctx.get_global("f") else {
      panic!("f missing")
    };
    let Some(Value::Texture(t)) = ctx.get_global("t") else {
      panic!("t missing")
    };
    assert!(try_vectorized_map(&ctx, &f, &t).is_none());
    let reps = reports(&ctx);
    assert!(
      reps
        .iter()
        .any(|r| !r.vectorized && r.reason.as_deref().is_some_and(|s| s.contains("recursive"))),
      "expected a recursion bail: {reps:?}"
    );
  }

  /// Ramps are `Callable::Dynamic`: invoked per texel while the rest of the body stays
  /// vectorized. Covers both the direct-call and the pipeline (`| shade`) shapes.
  #[test]
  fn dynamic_ramp_per_texel_kernel() {
    let src = r#"
shade = color_ramp(stops=[srgb(0x201510), srgb(0xd0b090), srgb(0xffffff)])
level = ramp(stops=[[0., 0.], [0.4, 0.9], [1., 1.]])
h = texture(16, 16, |uv| fbm(pos=uv * 3.) * 0.5 + 0.5)
(h -> |v| shade(sigmoid(v * 4. - 2.))) | render_texture(name="a")
(h -> |v| v * 0.5 + level(v) | shade) | render_texture(name="b")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    let reps = reports(&vec_ctx);
    assert_eq!(
      reps.iter().filter(|r| r.vectorized).count(),
      3,
      "ramp calls must use the per-texel dynamic kernel, not bail (2 maps + generator): {reps:?}"
    );
  }

  /// clamp() must never panic, whatever the bounds (std's clamp asserts min <= max).
  #[test]
  fn clamp_is_total() {
    let src = r#"
h = texture(16, 16, |uv| uv.x * 4. - 2.)
(h -> |v| clamp(2., 1., v)) | render_texture(name="a")
(h -> |v| clamp(v * 3., v, v * 2. - 1.)) | render_texture(name="b")
scalar = clamp(2., 1., 1.5)
vec = clamp(1., 0., v3(-1., 0.5, 2.))
i = clamp(5, 3, 4)
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    assert_eq!(vec_ctx.get_global("scalar").unwrap().as_float(), Some(2.));
    assert_eq!(vec_ctx.get_global("i").unwrap().as_int(), Some(5));
  }

  /// `pow`'s vec2 signature declared Vec3 (a verbatim dup of the vec3 one) while its impl
  /// arm read `as_vec2()`, so `pow(v2(..), e)` was a resolution error with a dead arm.
  #[test]
  fn pow_vec2_arm_resolves() {
    let src = r#"
h = texture(16, 16, |uv| uv.x + 0.25)
(h -> |v| pow(v2(v, v * 2.), 3.)) | render_texture(name="a")
scalar = pow(v2(2., 3.), 2.)
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    assert_eq!(
      vec_ctx.get_global("scalar").unwrap().as_vec2(),
      Some(&crate::Vec2::new(4., 9.))
    );
  }

  #[test]
  fn view_input_and_identity_passthrough() {
    let src = r#"
rgb = texture(16, 16, |uv| v3(uv.x, uv.y, 1.))
flipped = flip_x(rgb)
(flipped -> |c| c) | render_texture(name="ident")
(flipped -> |c| c.bgr * 2.) | render_texture(name="swiz")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
  }

  /// The motivating composition's generator body (Phase 3): remap with literal bounds,
  /// mix/lerp, sigmoid — plus wrap metadata parity.
  #[test]
  fn generator_vectorizes_bit_identical() {
    let src = r#"
out = texture(16, 16, |uv| {
  y = cos(uv.x * pi * 38.)
  mix(0.5, remap(-1., 1., 0., 1., y), sigmoid(y * 3.))
})
out | render_texture(name="o")
rgb = texture(16, 16, |uv| v3(uv.x, uv.y, fbm(pos=uv * 4.)), wrap="clamp")
rgb | render_texture(name="rgb")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    let reps = reports(&vec_ctx);
    assert!(
      reps.iter().filter(|r| r.vectorized).count() >= 2,
      "generator bodies must vectorize: {reps:?}"
    );
    let rgb = vec_ctx.rendered_textures.borrow()[1].texture.clone();
    assert_eq!(rgb.wrap, crate::TextureWrap::Clamp);
    assert_eq!(rgb.channels, 3);
  }

  #[test]
  fn generator_xy_params_bail() {
    let src = r#"
t = texture(16, 16, |uv, x, y| uv.x + float(x + y))
t | render_texture(name="o")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    let reps = reports(&vec_ctx);
    assert!(
      reps
        .iter()
        .any(|r| !r.vectorized && r.reason.as_deref().is_some_and(|s| s.contains("x_ix"))),
      "generator x/y params must bail: {reps:?}"
    );
  }

  /// A generator that never touches uv broadcast-fills and must not build uv planes.
  #[test]
  fn uniform_generator_broadcasts_without_uv() {
    let src = r#"
k = 0.25
t = texture(16, 16, |uv| k * 2.)
t | render_texture(name="o")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    let t = vec_ctx.rendered_textures.borrow()[0].texture.clone();
    assert!(t.as_planes()[0].iter().all(|&x| x == 0.5));
    assert_eq!(vec_ctx.tex_vectorize.uv_planes.borrow().len(), 0);
  }

  /// One uv-plane pair per size, shared across every generator at that size; an identity
  /// generator's output planes ARE the cached planes (zero copies).
  #[test]
  fn uv_planes_cached_and_shared() {
    let src = r#"
a = texture(16, 16, |uv| uv.x + uv.y)
b = texture(16, 16, |uv| uv.x * uv.y)
c = texture(8, 8, |uv| uv)
(a + b) | render_texture(name="ab")
c | render_texture(name="c")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    assert_eq!(vec_ctx.tex_vectorize.uv_planes.borrow().len(), 2);
    let c = vec_ctx.rendered_textures.borrow()[1].texture.clone();
    assert_eq!(c.channels, 2);
    let cached = vec_ctx
      .tex_vectorize
      .uv_planes
      .borrow()
      .iter()
      .find(|(k, _)| *k == (8, 8))
      .unwrap()
      .1
      .clone();
    assert!(Rc::ptr_eq(&c.as_planes()[0], &cached[0]));
    assert!(Rc::ptr_eq(&c.as_planes()[1], &cached[1]));
  }

  /// Capture-free top-level helpers get const-folded to `Expr::Literal(Callable::Closure)`,
  /// so their inlined frames have no `callee_uix`; the second invocation of one body took
  /// the plan-cache path and had to resolve those frames' captures from the baked callable.
  #[test]
  fn literal_inlined_helper_survives_cache_hit() {
    let src = r#"
ridge = |x| 1. - abs(x * 2. - 1.)
f = |v| ridge(v)
h1 = texture(16, 16, |uv| uv.x)
h2 = texture(16, 16, |uv| uv.y)
(h1 -> f) | render_texture(name="a")
(h2 -> f) | render_texture(name="b")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    assert!(reports(&vec_ctx).iter().all(|r| r.vectorized));
  }

  #[test]
  fn literal_inlined_helper_with_defaults_survives_cache_hit() {
    let src = r#"
k = 0.75
scaled = |x, s = (k * 2.)| x * s + k
h1 = texture(16, 16, |uv| uv.x)
h2 = texture(16, 16, |uv| uv.y)
apply = |v| scaled(v)
(h1 -> apply) | render_texture(name="a")
(h2 -> apply) | render_texture(name="b")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
  }

  /// A block whose non-final statement is varying but whose value is uniform classifies
  /// non-uniform yet compiles to `AbsVal::U` — both the assignment and the swizzle arm
  /// have to accept that.
  #[test]
  fn varying_block_with_uniform_value() {
    let src = r#"
h = texture(16, 16, |uv| uv.x)
(h -> |v| { b = { junk = v * 2.
5. }
b + v }) | render_texture(name="a")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
  }

  #[test]
  fn inlined_helper_with_uniform_value_body() {
    let src = r#"
helper = |x| { junk = x * 2.
v3(5., 6., 7.) }
h = texture(16, 16, |uv| uv.x)
(h -> |v| { b = helper(v)
b.x + v }) | render_texture(name="a")
(h -> |v| helper(v).y + v) | render_texture(name="b")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
  }

  /// A capture the plan key can't tell apart (a map/array carries one type flag) can still
  /// change the uniform's *shape* between runs; the cache-hit run must abort to scalar and
  /// say so, not report the previous run's success.
  #[test]
  fn uniform_shape_drift_aborts_and_reports() {
    let src = r#"
h = texture(16, 16, |uv| uv.x)
f = |a| (h -> |v| v * a[0])
f([2.]) | render_texture(name="x")
f([v2(1., 3.)]) | render_texture(name="y")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    let reps = reports(&vec_ctx);
    assert!(
      reps.iter().any(|r| !r.vectorized
        && r
          .reason
          .as_deref()
          .is_some_and(|s| s.contains("aborted to scalar this run"))),
      "the drifting run must report its abort: {reps:?}"
    );
  }

  /// Cache-hit runs must still record an outcome, and successful reports must carry the
  /// body's real source location rather than (0, 0).
  #[test]
  fn cache_hit_runs_report_success_with_a_loc() {
    let src = r#"
h1 = texture(16, 16, |uv| uv.x)
h2 = texture(16, 16, |uv| uv.y)
f = |v| v * 2. + 0.25
(h1 -> f) | render_texture(name="a")
(h2 -> f) | render_texture(name="b")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    let reps = reports(&vec_ctx);
    assert!(reps.iter().all(|r| r.vectorized), "{reps:?}");
    assert!(
      reps.iter().all(|r| r.loc != (0, 0)),
      "successful reports need the body loc: {reps:?}"
    );
  }

  /// The plan/report caches outlive a run on the host's long-lived ctx; a second
  /// parse+eval of the same source on one ctx must produce identical pixels.
  #[test]
  fn same_source_twice_on_one_ctx() {
    let src = r#"
h = texture(16, 16, |uv| fbm(pos=uv * 3.))
(h -> |v, uv| v3(v, uv.x, sigmoid(v * 2.))) | render_texture(name="o")
"#;
    let ctx = EvalCtx::default();
    ctx.tex_vectorize.no_vectorize.set(false);
    let run = |ctx: &EvalCtx| {
      ctx.rendered_textures.inner.borrow_mut().clear();
      ctx.tex_vectorize.reset_per_run();
      crate::parse_and_eval_program_with_ctx(src.to_string(), ctx, false).unwrap();
      Rc::clone(&ctx.rendered_textures.borrow()[0].texture)
    };
    let first = run(&ctx);
    let second = run(&ctx);
    assert_bit_identical(&Value::Texture(first), &Value::Texture(second)).unwrap();
    assert!(reports(&ctx).iter().all(|r| r.vectorized));
  }

  /// nalgebra's vec `lerp` short-circuits when `1 - t == 0` and never touches `a`, so the
  /// kernel must branch identically or `t == 1` diverges on non-finite / signed-zero `a`.
  #[test]
  fn vec_lerp_at_t_one_is_bit_identical() {
    let src = r#"
h = texture(16, 16, |uv| uv.x)
(h -> |v| { inf3 = v3(1. / (v - v), 0., 0.)
t = smoothstep(0.4, 0.5, v)
lerp(t, inf3, v3(v, v, v)) }) | render_texture(name="a")
(h -> |v| lerp(smoothstep(0.4, 0.5, v), v3(1., 2., 3.), v3(0., 0., 0.) * -1.)) | render_texture(name="b")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
  }

  fn vectorized_count(ctx: &EvalCtx) -> usize {
    reports(ctx).iter().filter(|r| r.vectorized).count()
  }

  /// Phase 2 surface: varying/uniform conditions, `else if` chains, `&&`/`||`/`!`, mask
  /// locals (typed), masks through inlined helpers, uniform-vs-varying arm broadcast,
  /// every comparison op incl. int literals and bool `==`/`!=`, generator entry.
  #[test]
  fn conditionals_bit_identical() {
    let src = r#"
h = texture(16, 16, |uv| fbm(pos=uv * 3.) * 1.5)
rgb = texture(16, 16, |uv| v3(uv.x, uv.y, fbm(pos=uv * 4.)))
flag = true
cap = 0.25
pick = |m: bool, a, b| if m {{ a }} else {{ b }}
step3 = |x| if x < 0.2 { 0. } else if x < 0.5 { 0.5 } else { 1. }
o1 = h -> |v| if v > 0.5 { v * 2. } else { v - 1. }
o2 = h -> |v| if v > 0.5 { 1. } else if v > 0. { 0.5 } else if v > -0.5 { 0.25 } else { 0. }
o3 = h -> |v| { m: bool = v > 0.5 && v < 1.
n = v < -0.2 || !m
if m == n { v } else if m != n && !n { v * 3. } else { cap } }
o4 = h -> |v, uv| if flag { v + uv.x } else { v }
o5 = h -> |v| if v >= 0.5 { cap } else { v }
o6 = rgb -> |c| if len(c) > 1. { c * 0.5 } else { c.bgr + v3(0.1, 0.2, 0.3) }
o7 = h -> |v| pick(v <= 0.3, sigmoid(v), step3(v))
o8 = h -> |v| if v == 1 { 5. } else if v != 0 { v } else { -1. }
o9 = h -> |v| if v > 0.5 { if v > 0.75 { 3. } else { 2. } } else { if flag { 1. } else { 0. } }
o10 = h -> |v| if v > 0.5 { cap } else { cap * 2. }
g = texture(16, 16, |uv| if uv.x > uv.y { uv.x } else { uv.y * 2. })
o1 | render_texture(name="o1")
o2 | render_texture(name="o2")
o3 | render_texture(name="o3")
o4 | render_texture(name="o4")
o5 | render_texture(name="o5")
o6 | render_texture(name="o6")
o7 | render_texture(name="o7")
o8 | render_texture(name="o8")
o9 | render_texture(name="o9")
o10 | render_texture(name="o10")
g | render_texture(name="g")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    let reps = reports(&vec_ctx);
    assert_eq!(
      vectorized_count(&vec_ctx),
      13,
      "every conditional body must vectorize (10 maps + 3 generators): {reps:?}"
    );
  }

  /// The load-bearing exactness rule: select is a bitwise pick, never a lerp. `1 / x` is
  /// ±inf on the untaken side (a lerp would NaN), and NaN payloads / signed zeros must
  /// survive the untaken arm too.
  #[test]
  fn select_is_exact_not_a_blend() {
    let src = r#"
h = texture(64, 64, |uv| floor(uv.x * 4.))
safe = h -> |x| if x > 0. { 1. / x } else { 0. }
m = texture_mean(safe)
print(m)
nz = h -> |x| if x > 1. { -0. } else { 0. / (x - x) }
safe | render_texture(name="safe")
nz | render_texture(name="nz")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    assert_eq!(*vec_ctx.prints.borrow(), *scalar_ctx.prints.borrow());
    assert!(
      vec_ctx.prints.borrow()[0].contains("0.45833334"),
      "{:?}",
      vec_ctx.prints.borrow()
    );
    assert_eq!(vectorized_count(&vec_ctx), 3);
  }

  /// A uniform condition picks its arm per run through a guard, so one cached plan serves
  /// both arm choices across cache-hit runs — and the untaken arm's steps never execute.
  #[test]
  fn uniform_cond_guards_flip_across_cache_hits() {
    let src = r#"
h = texture(16, 16, |uv| uv.x)
f = |flag| (h -> |v| if flag && v > 0.5 { v * 2. } else { v + 1. })
slices = 0..4 -> |i| (h -> |v| if i % 2 == 0 { sin(v * float(i)) } else { cos(v) })
slices | render_texture_stack(name="s")
f(true) | render_texture(name="t")
f(false) | render_texture(name="f")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    {
      let (a, b) = (
        vec_ctx.rendered_textures.borrow(),
        scalar_ctx.rendered_textures.borrow(),
      );
      for (xs, ys) in a[0].extra_slices.iter().zip(&b[0].extra_slices) {
        assert_bit_identical(
          &Value::Texture(Rc::clone(xs)),
          &Value::Texture(Rc::clone(ys)),
        )
        .unwrap();
      }
    }
    assert!(
      reports(&vec_ctx).iter().all(|r| r.vectorized),
      "{:?}",
      reports(&vec_ctx)
    );
    // generator + slice body + f's body: one plan each, reused across both guard states
    assert_eq!(vec_ctx.tex_vectorize.plans.borrow().len(), 3);
  }

  /// The stack-slice idiom whose untaken arm *errors* under the first slice's uniforms
  /// (`w[i - 1]` at `i == 0`): the arm is skipped on that run, the plan aborts+evicts on
  /// the first run that selects it, recompiles, and every slice stays bit-identical.
  #[test]
  fn untaken_uniform_arm_error_is_skipped_then_recompiled() {
    let src = r#"
h = texture(16, 16, |uv| uv.x)
w = [0.5, 0.25, 0.125]
slices = 0..4 -> |i| (h -> |v| if i == 0 { v } else { v * w[i - 1] })
slices | render_texture_stack(name="s")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    {
      let (a, b) = (
        vec_ctx.rendered_textures.borrow(),
        scalar_ctx.rendered_textures.borrow(),
      );
      assert_bit_identical(
        &Value::Texture(Rc::clone(&a[0].texture)),
        &Value::Texture(Rc::clone(&b[0].texture)),
      )
      .unwrap();
      assert_eq!(a[0].extra_slices.len(), 3);
      for (xs, ys) in a[0].extra_slices.iter().zip(&b[0].extra_slices) {
        assert_bit_identical(
          &Value::Texture(Rc::clone(xs)),
          &Value::Texture(Rc::clone(ys)),
        )
        .unwrap();
      }
    }
    // last invocation (i == 3) is a cache hit on the recompiled plan
    assert!(
      reports(&vec_ctx).iter().all(|r| r.vectorized),
      "{:?}",
      reports(&vec_ctx)
    );
    assert_eq!(vec_ctx.tex_vectorize.plans.borrow().len(), 2);
  }

  /// A structurally unvectorizable arm only costs the runs that select it; the plan stays
  /// cached and the other arm keeps vectorizing.
  #[test]
  fn unvectorizable_uniform_arm_aborts_only_when_selected() {
    // Distinct args per call (a repeated `f(0)` would be replayed from the const-eval cache).
    let prog = |n: usize| {
      let calls: Vec<String> = [0, 5, 1][..n]
        .iter()
        .map(|k| format!("f({k}) | render_texture(name=\"r{k}\")"))
        .collect();
      format!(
        "h = texture(16, 16, |uv| uv.x)\nf = |i| (h -> |v| if i < 2 {{ v * 2. }} else {{ v3(v, v, \
         v)[0] }})\n{}",
        calls.join("\n")
      )
    };
    // Reports keep the last outcome per body, so observe each invocation via program length.
    for (n, needle) in [(1, None), (2, Some("indexing a varying")), (3, None)] {
      let (vec_ctx, scalar_ctx) = eval_both(&prog(n));
      assert_identical_outputs(&vec_ctx, &scalar_ctx);
      let reps = reports(&vec_ctx);
      match needle {
        None => assert!(reps.iter().all(|r| r.vectorized), "run {n}: {reps:?}"),
        Some(needle) => assert!(
          reps.iter().any(|r| !r.vectorized
            && r
              .reason
              .as_deref()
              .is_some_and(|s| s.contains("selected branch") && s.contains(needle))),
          "run {n}: {reps:?}"
        ),
      }
      assert_eq!(vec_ctx.tex_vectorize.plans.borrow().len(), 2);
    }
  }

  /// Arms that can't be expressed bail rather than error: the scalar path only errors if
  /// some texel actually takes the odd arm, and here none does.
  #[test]
  fn odd_arms_bail_to_scalar() {
    for (src, needle) in [
      (
        r#"h = texture(16, 16, |uv| uv.x)
(h -> |v| if v > 100. { v3(v, v, v) } else { v }) | render_texture(name="o")"#,
        "arity",
      ),
      (
        r#"h = texture(16, 16, |uv| uv.x)
(h -> |v| if v > 0.5 { v } else { 0 }) | render_texture(name="o")"#,
        "int-typed",
      ),
      (
        r#"h = texture(16, 16, |uv| uv.x)
(h -> |v| if v > 100. { "s" } else { v }) | render_texture(name="o")"#,
        "non-numeric",
      ),
      (
        r#"h = texture(16, 16, |uv| uv.x)
w = [1., 2.]
f = |k| (h -> |v| if v > 100. { v * w[k] } else { v })
f(5) | render_texture(name="o")"#,
        "speculatively",
      ),
    ] {
      let (vec_ctx, scalar_ctx) = eval_both(src);
      assert_identical_outputs(&vec_ctx, &scalar_ctx);
      let reps = reports(&vec_ctx);
      assert!(
        reps
          .iter()
          .any(|r| !r.vectorized && r.reason.as_deref().is_some_and(|s| s.contains(needle))),
        "expected bail containing {needle:?}: {reps:?}\n{src}"
      );
    }
  }

  /// A speculative uniform that errors only on a cache-hit run's values (`w[k]` with `k`
  /// out of range, inside an arm no texel takes) must abort to scalar, not error.
  #[test]
  fn speculative_uniform_error_on_cache_hit_aborts() {
    let src = r#"
h = texture(16, 16, |uv| uv.x)
w = [1., 2.]
f = |k| (h -> |v| if v > 100. { v * w[k] } else { v })
f(0) | render_texture(name="a")
f(5) | render_texture(name="b")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    let reps = reports(&vec_ctx);
    assert!(
      reps
        .iter()
        .any(|r| !r.vectorized && r.reason.as_deref().is_some_and(|s| s.contains("aborted"))),
      "{reps:?}"
    );
  }

  /// The vec4 overloads (unary math, abs, pow, min/max, clamp, len/dot/distance/normalize)
  /// lower through the same arms as vec3; a 4-channel body must vectorize end to end.
  /// `return` desugars to nested conditionals: top-level, varying/uniform conditions,
  /// else-if ladders, nested arms, inside inlined helpers (incl. a mask-valued one),
  /// assignment and block forms, else-less assignment, `break` out of a block, vec arms,
  /// generator entry.
  #[test]
  fn early_returns_bit_identical() {
    let src = r#"
h = texture(16, 16, |uv| fbm(pos=uv * 3.) * 1.5)
rgb = texture(16, 16, |uv| v3(uv.x, uv.y, fbm(pos=uv * 4.)))
cap = 0.25
flag = true
helper = |x| { if x > 0.5 { return x * 2. }
x - 1. }
pred = |x| { if x > 0.8 { return true }
x < 0.1 }
o1 = h -> |v| { return v * 2. }
o2 = h -> |v| { if v > 0.5 { return 1. }
v * 2. }
o3 = h -> |v| { if v > 0.75 { return 3. } else if v > 0.5 { return 2. }
if v < 0.1 { return 0. }
v }
o4 = h -> |v| { if v > 0.2 { if v > 0.6 { return v * 3. }
w = v * 2.
if w > 0.9 { return w } }
v }
o5 = h -> |v| { if v > 0.5 { return cap }
if flag { return v + 1. }
v }
o6 = h -> |v| helper(v) + 1.
o7 = h -> |v| { y = if v > 0.3 { return 0. } else { v * 4. }
y + 1. }
o8 = h -> |v| { y = { if v > 0.3 { return 0. }
v * 5. }
y + 1. }
o9 = rgb -> |c| { if len(c) > 1. { return c * 0.5 }
c.bgr }
o10 = h -> |v| if pred(v) { 1. } else { 0. }
o11 = h -> |v| { if v > 0.5 { return v } else { return v * 2. } }
o12 = h -> |v| { if v > 0.4 { z = v * 3.
if z > 2. { return z } else { return z * 0.5 } }
v }
o13 = h -> |v| { y = if v > 5. { return 1. }
v }
o14 = h -> |v| { y = { if v > 0.5 { break 1. }
v }
y }
g = texture(16, 16, |uv| { if uv.x > uv.y { return uv.x }
uv.y * 2. })
o1 | render_texture(name="o1")
o2 | render_texture(name="o2")
o3 | render_texture(name="o3")
o4 | render_texture(name="o4")
o5 | render_texture(name="o5")
o6 | render_texture(name="o6")
o7 | render_texture(name="o7")
o8 | render_texture(name="o8")
o9 | render_texture(name="o9")
o10 | render_texture(name="o10")
o11 | render_texture(name="o11")
o12 | render_texture(name="o12")
o13 | render_texture(name="o13")
o14 | render_texture(name="o14")
g | render_texture(name="g")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    let reps = reports(&vec_ctx);
    assert_eq!(
      vectorized_count(&vec_ctx),
      17,
      "every return body must vectorize (14 maps + 3 generators): {reps:?}"
    );
  }

  /// The early-return spelling of the guarded-arm shape keeps its guarantees: the tail
  /// after `if i == 0 {{ return v }}` is guarded off on the `i == 0` run (its `w[-1]` is
  /// never evaluated), the value-dependent arm failure recompiles once, and later slices
  /// hit the cache.
  #[test]
  fn uniform_return_guards_skip_tail_and_recompile() {
    let src = r#"
h = texture(16, 16, |uv| uv.x)
w = [0.5, 0.25, 0.125]
f = |i| { h -> |v| { if i == 0 { return v }
v * w[i - 1] } }
slices = 0..4 -> f
slices | render_texture_stack(name="s")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    {
      let (a, b) = (
        vec_ctx.rendered_textures.borrow(),
        scalar_ctx.rendered_textures.borrow(),
      );
      assert_bit_identical(
        &Value::Texture(Rc::clone(&a[0].texture)),
        &Value::Texture(Rc::clone(&b[0].texture)),
      )
      .unwrap();
      assert_eq!(a[0].extra_slices.len(), 3);
      for (xs, ys) in a[0].extra_slices.iter().zip(&b[0].extra_slices) {
        assert_bit_identical(
          &Value::Texture(Rc::clone(xs)),
          &Value::Texture(Rc::clone(ys)),
        )
        .unwrap();
      }
    }
    assert!(
      reports(&vec_ctx).iter().all(|r| r.vectorized),
      "{:?}",
      reports(&vec_ctx)
    );
    assert_eq!(vec_ctx.tex_vectorize.plans.borrow().len(), 2);
  }

  /// Shapes the desugar declines (each valid on the scalar path) name the construct.
  #[test]
  fn return_bails_report_reasons() {
    for (body, needle) in [
      // Desugars to `if v > 5. { nil } else { v }`.
      (
        "{ if v > 5. { return }
v }",
        "non-numeric arm",
      ),
    ] {
      let src =
        format!("t = texture(16, 16, |uv| uv.x)\n(t -> |v| {body}) | render_texture(name=\"o\")");
      let (ctx, scalar_ctx) = eval_both(&src);
      assert_identical_outputs(&ctx, &scalar_ctx);
      let reps = reports(&ctx);
      assert!(
        reps
          .iter()
          .any(|r| !r.vectorized && r.reason.as_deref().is_some_and(|s| s.contains(needle))),
        "expected bail reason containing {needle:?}; reports: {reps:?}\n{src}"
      );
    }
  }

  /// Loops unroll at compile time: `fold`/`reduce`/`->`/`scan`/`any`/`all` over literal
  /// ranges, varying array literals, captured arrays and ranges; closure literals with
  /// varying captures (incl. local helpers and `return` inside them), captured closures and
  /// builtins as callbacks; nested maps + `flatten`; structural ops and eager indexing;
  /// `reduce`'s index convention; uniform conditions inside loop bodies; loops inside
  /// inlined helpers; generator entry.
  #[test]
  fn loops_unroll_bit_identical() {
    let src = r#"
h = texture(16, 16, |uv| fbm(pos=uv * 3.) * 1.5)
rgb = texture(16, 16, |uv| v3(uv.x, uv.y, fbm(pos=uv * 4.)))
w = [0.5, 0.25, 0.125]
n = 3
add_sq = |acc, x| acc + x * x
helper_loop = |x| (0..3 -> |o| { x * float(o) }) | reduce(add)
o1 = h -> |v| fold(0., |acc, o| { acc + sin(v * pow(2., float(o))) * pow(0.5, float(o)) }, 0..4)
o2 = h -> |v| (0..4 -> |o| { sin(v * float(o + 1)) }) | reduce(add)
o3 = h -> |v| ([v, v * 2., v * 3.] -> |x| { x * x }) | reduce(max)
o4 = rgb -> |c| [c.x, c.y, c.z] | reduce(min)
o5 = h -> |v| { s = (0..3 -> |i| { 0..2 -> |j| { v * float(i) + float(j) } }) | flatten
s | reduce(add) }
o6 = h -> |v| (0..4 -> |o| { v * float(o) }) | scan(0., |acc, x| { acc + x }) | last
o7 = h -> |v| { a = any(|x| x > 0.5, [v, v * 2.])
b = all(|x| x > 0.1, [v, v * 2.])
if a && b { 1. } else if a { 0.5 } else { 0. } }
o8 = h -> |v| { s = [v, v * 2., v * 3., v * 4.]
s[1] + (s | reverse)[0] + (s | take(2) | last) + (s | skip(3) | first) + (s | collect)[3] }
o9 = h -> |v| (w -> |x| { x * v }) | reduce(add)
o10 = h -> |v| fold(v, |acc, i| { acc * w[i] }, 0..3)
o11 = h -> |v| { f = |x| x * v
(0..3 -> f) | reduce(add) }
o12 = h -> |v| fold(0., add_sq, [v, v * 2.])
o13 = h -> |v| { step = |acc, x| { if x > 0.5 { return acc }
acc + x }
fold(0., step, [v, v * 2., v * 3.]) }
o14 = h -> |v| reduce(|acc, x, i| { acc + x * float(i) }, [v, v * 2., v * 3.])
o15 = h -> |v| (0..3 -> |o| { if o == 0 { v } else { v * w[o - 1] } }) | reduce(add)
o16 = h -> |v, uv| ([uv.x, uv.y, v] -> |x, i| { x * float(i + 1) }) | reduce(add)
o17 = h -> |v| helper_loop(v) + 1.
o18 = h -> |v| (0..n -> |o| { v * float(o) }) | reduce(add)
o19 = h -> |v| { step = |acc, x| { if x > v { return acc }
acc + x }
fold(0., step, [v * 0.5, v * 2., v * 3.]) }
o20 = h -> |v| { s = [v, [v * 2., v * 3.], v * 4.] | flatten
chain([s, [v * 5.]]) | reduce(add) }
g = texture(16, 16, |uv| fold(0., |acc, o| { acc + fbm(pos=uv * pow(2., float(o))) * pow(0.5, float(o)) }, 0..3))
o1 | render_texture(name="o1")
o2 | render_texture(name="o2")
o3 | render_texture(name="o3")
o4 | render_texture(name="o4")
o5 | render_texture(name="o5")
o6 | render_texture(name="o6")
o7 | render_texture(name="o7")
o8 | render_texture(name="o8")
o9 | render_texture(name="o9")
o10 | render_texture(name="o10")
o11 | render_texture(name="o11")
o12 | render_texture(name="o12")
o13 | render_texture(name="o13")
o14 | render_texture(name="o14")
o15 | render_texture(name="o15")
o16 | render_texture(name="o16")
o17 | render_texture(name="o17")
o18 | render_texture(name="o18")
o19 | render_texture(name="o19")
o20 | render_texture(name="o20")
g | render_texture(name="g")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    let reps = reports(&vec_ctx);
    assert_eq!(
      vectorized_count(&vec_ctx),
      23,
      "every loop body must vectorize (20 maps + 3 generators): {reps:?}"
    );
  }

  /// A run-time-uniform loop bound pins the unroll count; a different bound on a cache hit
  /// evicts and recompiles in the same invocation, so every call stays vectorized.
  #[test]
  fn seq_length_pin_recompiles() {
    let src = r#"
h = texture(16, 16, |uv| uv.x)
f = |n| { h -> |v| (0..n -> |o| { v * float(o) }) | reduce(add) }
f(2) | render_texture(name="a")
f(3) | render_texture(name="b")
f(4) | render_texture(name="c")
f(4) | render_texture(name="d")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    assert!(
      reports(&vec_ctx).iter().all(|r| r.vectorized),
      "{:?}",
      reports(&vec_ctx)
    );
    // generator + map body: one key each, the map's recompiled in place twice
    assert_eq!(vec_ctx.tex_vectorize.plans.borrow().len(), 2);
  }

  #[test]
  fn seq_bails_report_reasons() {
    for (body, needle) in [
      (
        "(0.. -> |o| { v * float(o) }) | take(3) | reduce(add)",
        "unbounded",
      ),
      (
        "(0..300 -> |o| { v * float(o) }) | reduce(add)",
        "longer than",
      ),
      (
        "fold_while(0., |acc, x| { if acc > 1. { nil } else { acc + x } }, [v, v * 2.])",
        "fold_while",
      ),
      ("[v, v * 2.] | filter(|x| x > -1.) | reduce(add)", "filter"),
    ] {
      let src =
        format!("t = texture(16, 16, |uv| uv.x)\n(t -> |v| {body}) | render_texture(name=\"o\")");
      let (ctx, scalar_ctx) = eval_both(&src);
      assert_identical_outputs(&ctx, &scalar_ctx);
      let reps = reports(&ctx);
      assert!(
        reps
          .iter()
          .any(|r| !r.vectorized && r.reason.as_deref().is_some_and(|s| s.contains(needle))),
        "expected bail reason containing {needle:?}; reports: {reps:?}\n{src}"
      );
    }
    // Indexing a lazy sequence and returning a closure error on both paths.
    for body in ["(0..3 -> |o| { v * float(o) })[1]", "{ f = |x| x * v\nf }"] {
      let src =
        format!("t = texture(16, 16, |uv| uv.x)\n(t -> |v| {body}) | render_texture(name=\"o\")");
      for no_vec in [false, true] {
        let ctx = EvalCtx::default();
        ctx.tex_vectorize.no_vectorize.set(no_vec);
        assert!(
          crate::parse_and_eval_program_with_ctx(src.clone(), &ctx, false).is_err(),
          "{src}"
        );
      }
    }
  }

  #[test]
  fn vec4_builtins_vectorize() {
    let src = r#"
rgba = texture(16, 16, |uv| v4(uv.x, uv.y, fbm(pos=uv * 3.), 1. - uv.x))
o = rgba -> |c| {
  a = clamp(0., 1., abs(sin(c) * 3.)) + sqrt(fract(c) + 0.5) - floor(c)
  b = min(c, c.wzyx) * max(c, pow(c, 2.)) + normalize(c + 0.1) * len(c)
  if dot(c, c) > distance(c, c.yxwz) { a + b } else { sigmoid(a) }
}
o | render_texture(name="o")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    assert_eq!(vectorized_count(&vec_ctx), 2, "{:?}", reports(&vec_ctx));
  }

  fn compiled_plans(ctx: &EvalCtx) -> Vec<Rc<Plan>> {
    ctx
      .tex_vectorize
      .plans
      .borrow()
      .values()
      .filter_map(|e| match e {
        PlanEntry::Ok(p) => Some(Rc::clone(p)),
        _ => None,
      })
      .collect()
  }

  /// DCE drops the unread `normalize` channels; the emission peephole removes `remap`'s
  /// `* 1.0` / `/ 1.0` and folds all-constant ops; `x + 0.0` must survive (−0.0).
  #[test]
  fn texture_zip_bit_identical() {
    let src = r#"
n = 16
a = texture(n, n, |uv| v4(uv.x, uv.y, fbm(pos=uv * 3.), fbm(pos=uv * 5., seed=2)))
b = texture(n, n, |uv| v4(fbm(pos=uv * 2.), uv.y, uv.x, fbm(pos=uv * 7., seed=3)))
blend = |t0: vec4, t1: vec4|: vec4 {
  if t0.a > 0.7 {
    t0
  } else if t1.a < 0.3 {
    t0 * 0.5
  } else {
    v4((t0.rgb + t1.rgb) / 2., 1.)
  }
}
[a, b] | texture_zip(blend) | render_texture(name="blend")

rgb0 = texture(n, n, |uv| v3(uv.x, uv.y, 0.25))
rgb1 = texture(n, n, |uv| v3(fbm(pos=uv * 4.), uv.x, uv.y))
mask = texture(n, n, |uv| fbm(pos=uv * 6., seed=9))
[rgb0, rgb1, mask] | texture_zip(|p, q, m| mix(smoothstep(0.3, 0.7, m), p, q))
  | render_texture(name="masked")

[mask] | texture_zip(|v, uv| v + uv.x) | render_texture(name="single_input")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    // 5 generators + the 3 zip bodies; the count pins that no body silently went unreported.
    let reps = reports(&vec_ctx);
    assert_eq!(reps.len(), 8, "reports: {reps:?}");
    assert!(
      reps.iter().all(|r| r.vectorized),
      "every zip body should vectorize; reports: {reps:?}"
    );
  }

  /// The plan cache must key on the ordered input arities: one body, two different input
  /// shapes. A cache that ignored the shape would replay the first plan's plane offsets
  /// against the second call's inputs.
  #[test]
  fn texture_zip_arity_swap_keys_the_plan_cache() {
    let src = r#"
n = 16
m = texture(n, n, |uv| uv.x)
c = texture(n, n, |uv| v3(uv.x, uv.y, 0.5))
f = |a, b| a * b
[m, c] | texture_zip(f) | render_texture(name="mask_first")
[c, m] | texture_zip(f) | render_texture(name="color_first")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    let outs = vec_ctx.rendered_textures.borrow();
    assert_eq!(outs.len(), 2);
    assert!(outs.iter().all(|t| t.texture.channels == 3));
  }

  /// `Src::In` is a u8 plane index, so past 256 input planes the entry point hands off to
  /// the scalar path rather than wrapping around.
  #[test]
  fn texture_zip_plane_ceiling_bails() {
    let src = r#"
tex = |i| texture(8, 8, |uv| v4(uv.x + i, uv.y, 0.5, 0.25))
(0..65 -> tex) | texture_zip(|a, b| a + b) | render_texture(name="o")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    let reps = reports(&vec_ctx);
    assert!(
      reps.iter().any(|r| !r.vectorized
        && r
          .reason
          .as_deref()
          .is_some_and(|s| s.contains("256 input planes"))),
      "reports: {reps:?}"
    );
  }

  #[test]
  fn texture_zip_shape_errors() {
    let cases = [
      (
        r#"a = texture(16, 16, |uv| uv.x)
b = texture(16, 8, |uv| uv.y)
[a, b] | texture_zip(|x, y| x + y) | render_texture(name="o")"#,
        "matching dims",
      ),
      (
        r#"a = texture(16, 16, |uv| uv.x)
[a, 3.] | texture_zip(|x, y| x + y) | render_texture(name="o")"#,
        "index 1",
      ),
      (
        r#"[] | texture_zip(|x| x) | render_texture(name="o")"#,
        "at least one texture",
      ),
    ];
    for (src, expected) in cases {
      let err = crate::parse_and_eval_program(src).expect_err("expected an error");
      let msg = format!("{err}");
      assert!(msg.contains(expected), "expected {expected:?} in: {msg}");
    }
  }

  #[test]
  fn dce_and_exact_peepholes() {
    let src = r#"
h = texture(16, 16, |uv| uv.x - 0.5)
nz = h -> |v| normalize(v3(v, v * 2., v * 3.)).z
rm = h -> |v| remap(-1., 1., 0., 1., v)
neg0 = h -> |v| -(v - v) + 0.
nz | render_texture(name="nz")
rm | render_texture(name="rm")
neg0 | render_texture(name="neg0")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    let plans = compiled_plans(&vec_ctx);
    let shapes: Vec<(usize, u16, u16)> = plans
      .iter()
      .map(|p| (p.steps.len(), p.n_folded, p.n_dead))
      .collect();
    // nz: 2 muls, 3 squares, 2 adds, sqrt, 1 div (the x/y divs are dead)
    let nz = plans
      .iter()
      .find(|p| p.n_dead == 2)
      .unwrap_or_else(|| panic!("{shapes:?}"));
    assert_eq!(nz.steps.len(), 9, "{shapes:?}");
    // rm: sub, div (÷ 2.0 kept), [clamp], add 0.0 (kept: −0.0 + 0.0 = +0.0); `* 1.0` folded
    let rm = plans
      .iter()
      .find(|p| p.n_folded == 1)
      .unwrap_or_else(|| panic!("{shapes:?}"));
    assert!(rm.steps.len() == 3 || rm.steps.len() == 4, "{shapes:?}");
    assert!(rm
      .steps
      .iter()
      .any(|s| matches!(s, Step::Op { kind: OpKind::Add, a, b, .. }
      if [a, b].iter().any(|x| matches!(x, Src::Const(k) if k.to_bits() == 0)))));
    // neg0 output must be +0.0 everywhere in both paths (the `+ 0.` normalizes the −0.0)
    let t = vec_ctx.rendered_textures.borrow()[2].texture.clone();
    assert!(t.as_planes()[0].iter().all(|x| x.to_bits() == 0));
  }

  #[test]
  fn peephole_rules_are_exact_identities() {
    let r = |k| Src::Reg(k);
    let c = Src::Const;
    assert_eq!(peephole(OpKind::Mul, r(1), c(1.), c(0.)), Some(r(1)));
    assert_eq!(peephole(OpKind::Mul, c(1.), r(1), c(0.)), Some(r(1)));
    assert_eq!(peephole(OpKind::Div, r(1), c(1.), c(0.)), Some(r(1)));
    assert_eq!(peephole(OpKind::Sub, r(1), c(0.), c(0.)), Some(r(1)));
    assert_eq!(peephole(OpKind::Add, r(1), c(-0.), c(0.)), Some(r(1)));
    assert_eq!(peephole(OpKind::Add, r(1), c(0.), c(0.)), None);
    assert_eq!(peephole(OpKind::Mul, r(1), c(0.), c(0.)), None);
    assert_eq!(peephole(OpKind::Mul, r(1), c(-1.), c(0.)), None);
    assert_eq!(peephole(OpKind::Sub, r(1), c(-0.), c(0.)), None);
    assert_eq!(peephole(OpKind::Select, r(0), r(1), r(1)), Some(r(1)));
    assert_eq!(peephole(OpKind::Select, c(1.), r(1), r(2)), Some(r(1)));
    assert_eq!(peephole(OpKind::Select, c(0.), r(1), r(2)), Some(r(2)));
    assert_eq!(peephole(OpKind::Add, c(1.5), c(2.25), c(0.)), Some(c(3.75)));
    assert_eq!(
      peephole(OpKind::Sqrt, c(-1.), c(0.), c(0.))
        .map(|s| matches!(s, Src::Const(v) if v.is_nan())),
      Some(true)
    );
    assert_eq!(peephole(OpKind::Add, r(1), Src::Uni(0, 0), c(0.)), None);
  }

  /// CSE shares `len`/`dot`/`normalize`'s squares+sums and duplicate `fbm` calls, reuses an
  /// unguarded value inside a guarded arm, and never shares across sibling arms (a sibling's
  /// register may be unwritten on runs that skipped it).
  #[test]
  fn cse_shares_within_dominating_guards_only() {
    let src = r#"
h = texture(16, 16, |uv| uv.x - 0.5)
shared = h -> |v, uv| { p = v3(v, uv.x, uv.y)
len(p) * dot(p, p) + normalize(p).x }
noise2 = h -> |v, uv| fbm(pos=uv * 3.) + fbm(pos=uv * 3.) * v
pick = |v, d, flag| if flag { d + 1. } else { v * 2. }
mk_arms = |flag| (h -> |v| if flag { v * 2. } else { v * 2. + 1. })
mk_dom = |flag| (h -> |v| pick(v, v * 2., flag))
arms = mk_arms(true)
dom = mk_dom(true)
shared | render_texture(name="shared")
noise2 | render_texture(name="noise2")
arms | render_texture(name="arms")
dom | render_texture(name="dom")
"#;
    let (vec_ctx, scalar_ctx) = eval_both(src);
    assert_identical_outputs(&vec_ctx, &scalar_ctx);
    let plans = compiled_plans(&vec_ctx);
    let shapes: Vec<(usize, u16, u16, usize)> = plans
      .iter()
      .map(|p| {
        (
          p.steps.len(),
          p.n_cse,
          p.n_dead,
          p.steps
            .iter()
            .filter(|s| {
              matches!(
                s,
                Step::Op {
                  kind: OpKind::Mul,
                  ..
                }
              )
            })
            .count(),
        )
      })
      .collect();
    // shared: 3 squares + 2 adds + sqrt (len) reused by dot (5 hits) and normalize (6 hits);
    // normalize's y/z divs are dead → 3 mul + 2 add + sqrt + div + mul + add = 9 live steps
    let shared = plans
      .iter()
      .find(|p| p.n_cse == 11)
      .unwrap_or_else(|| panic!("{shapes:?}"));
    assert_eq!((shared.steps.len(), shared.n_dead), (9, 2), "{shapes:?}");
    let fbm_steps = |p: &Plan| p.steps.iter().filter(|s| matches!(s, Step::Fbm(_))).count();
    assert!(
      plans.iter().any(|p| fbm_steps(p) == 1 && p.n_cse >= 3),
      "{shapes:?}"
    );
    let muls = |p: &Plan| {
      p.steps
        .iter()
        .filter(|s| {
          matches!(
            s,
            Step::Op {
              kind: OpKind::Mul,
              ..
            }
          )
        })
        .count()
    };
    // arms: `v * 2.` under each sibling guard stays separate (2 muls); dom: the arm reuses
    // the unguarded `d` (1 mul)
    assert!(
      plans
        .iter()
        .any(|p| !p.guards.is_empty() && muls(p) == 2 && p.n_cse == 0),
      "{shapes:?}"
    );
    assert!(
      plans
        .iter()
        .any(|p| !p.guards.is_empty() && muls(p) == 1 && p.n_cse == 1),
      "{shapes:?}"
    );
  }
}

/// Property-based differential fuzz: random whitelist-only texel bodies, both paths, bit
/// equality — the only thing that finds combinations hand-written fixtures miss. The
/// generator emits only vectorizable constructs, so a silent bail is also a failure.
#[cfg(test)]
mod property_tests {
  use super::*;
  use rand::RngExt;
  use rand_pcg::Pcg32;

  struct Gen {
    rng: Pcg32,
    /// (name, arity) of in-scope locals, including params.
    vars: Vec<(String, u8)>,
    /// In-scope bool locals (masks).
    bvars: Vec<String>,
    next_local: usize,
    /// Conditional arms must not be bare int literals: an `Int` arm bails (texel-dependent
    /// typing), and the generator only emits must-vectorize programs.
    no_int_leaf: bool,
  }

  impl Gen {
    fn lit(&mut self) -> String {
      let v = (self.rng.random::<f32>() - 0.4) * 4.;
      format!("{v:?}")
    }

    /// A random bool expression: compares (every op, int literal rhs sometimes), `&&`/`||`
    /// /`!`, uniform bool captures (→ guards), and mask locals.
    fn cond(&mut self, depth: u8) -> String {
      let d = depth.saturating_sub(1);
      match self
        .rng
        .random_range(0..(if depth == 0 { 4 } else { 9 }) as u32)
      {
        0 => {
          let op = ["<", "<=", ">", ">=", "==", "!="][self.rng.random_range(0..6usize)];
          let rhs = if self.rng.random_range(0..4u32) == 0 {
            format!("{}", self.rng.random_range(-1..3i64))
          } else {
            self.expr(1, d)
          };
          format!("({} {op} {rhs})", self.expr(1, d))
        }
        1 => ["flag_t", "flag_f", "(cap > 1.)"][self.rng.random_range(0..3usize)].to_string(),
        2 if !self.bvars.is_empty() => {
          self.bvars[self.rng.random_range(0..self.bvars.len())].clone()
        }
        2 | 3 => format!("({} > {})", self.expr(1, d), self.lit()),
        4 | 5 => format!("({} && {})", self.cond(d), self.cond(d)),
        6 => format!("({} || {})", self.cond(d), self.cond(d)),
        7 => format!("!({})", self.cond(d)),
        _ => format!(
          "pickb({}, {}, {})",
          self.cond(d),
          self.cond(d),
          self.cond(d)
        ),
      }
    }

    fn arm(&mut self, arity: u8, depth: u8) -> String {
      let prev = std::mem::replace(&mut self.no_int_leaf, true);
      let e = self.expr(arity, depth);
      self.no_int_leaf = prev;
      e
    }

    /// A random expression of the requested arity.
    fn expr(&mut self, arity: u8, depth: u8) -> String {
      if depth == 0 {
        return self.leaf(arity);
      }
      let d = depth - 1;
      match self.rng.random_range(0..13u32) {
        0..=2 => {
          // arithmetic; scalar broadcast only in the directions the language defines
          let op = ["+", "-", "*", "/"][self.rng.random_range(0..4usize)];
          if arity > 1 && self.rng.random_range(0..3u32) == 0 {
            let (a, b) = (self.expr(arity, d), self.leaf(1));
            format!("({a} {op} {b})")
          } else if arity == 1 && op == "*" && self.rng.random_range(0..4u32) == 0 {
            let (a, b) = (self.leaf(1), self.expr(1, d));
            format!("({a} * {b})")
          } else {
            let (a, b) = (self.expr(arity, d), self.expr(arity, d));
            format!("({a} {op} {b})")
          }
        }
        3 => {
          let f = [
            "sin", "cos", "sqrt", "abs", "exp", "floor", "fract", "sigmoid", "round",
          ][self.rng.random_range(0..9usize)];
          format!("{f}({})", self.expr(arity, d))
        }
        4 => format!("-({})", self.expr(arity, d)),
        5 => {
          // clamp / smoothstep-ish scalar shapes, plus the identity / remap forms the
          // emission peephole rewrites (`* 1.`, `/ 1.`, `- 0.`, `+ 0.` must stay exact)
          if arity == 1 {
            match self.rng.random_range(0..7u32) {
              0 => format!(
                "smoothstep({}, {}, {})",
                self.lit(),
                self.lit(),
                self.expr(1, d)
              ),
              1 => format!("atan2({}, {})", self.expr(1, d), self.expr(1, d)),
              2 => format!("({} % {})", self.expr(1, d), self.lit()),
              3 => format!(
                "({} {} {})",
                self.expr(1, d),
                ["* 1.", "/ 1.", "- 0.", "+ 0.", "* -1.", "+ -0."]
                  [self.rng.random_range(0..6usize)],
                ""
              ),
              4 => format!("remap(-1., 1., 0., 1., {})", self.expr(1, d)),
              5 => format!("remap(0., 2., -1., 3., {}, clamp=false)", self.expr(1, d)),
              _ => format!("-({} - {})", self.expr(1, d), self.expr(1, d)),
            }
          } else {
            format!(
              "clamp({}, {}, {})",
              self.lit(),
              self.lit(),
              self.expr(arity, d)
            )
          }
        }
        6 => format!(
          "lerp({}, {}, {})",
          self.expr(1, d),
          self.expr(arity, d),
          self.expr(arity, d)
        ),
        7 => {
          // build the arity via construction or reduce a vec to scalar
          match arity {
            1 => {
              let src_ar = self.rng.random_range(2..=4u8);
              match self.rng.random_range(0..3u32) {
                0 => format!("len({})", self.expr(src_ar, d)),
                1 => format!("dot({}, {})", self.expr(src_ar, d), self.expr(src_ar, d)),
                _ => format!(
                  "distance({}, {})",
                  self.expr(src_ar, d),
                  self.expr(src_ar, d)
                ),
              }
            }
            2 => format!("v2({}, {})", self.expr(1, d), self.expr(1, d)),
            3 => match self.rng.random_range(0..3u32) {
              0 => format!(
                "v3({}, {}, {})",
                self.expr(1, d),
                self.expr(1, d),
                self.expr(1, d)
              ),
              1 => format!("v3({}, {})", self.expr(2, d), self.expr(1, d)),
              _ => format!("normalize({})", self.expr(3, d)),
            },
            _ => match self.rng.random_range(0..3u32) {
              0 => format!("v4({}, {})", self.expr(2, d), self.expr(2, d)),
              1 => format!("normalize({})", self.expr(4, d)),
              _ => format!("min({}, {})", self.expr(4, d), self.expr(4, d)),
            },
          }
        }
        8 => {
          // swizzle out of a wider vec
          let src_ar = self.rng.random_range(arity.max(2)..=4u8);
          let chars = ["x", "y", "z", "w"];
          let sw: String = (0..arity)
            .map(|_| chars[self.rng.random_range(0..src_ar as usize)])
            .collect();
          format!("({}).{sw}", self.expr(src_ar, d))
        }
        9 => {
          if arity == 1 && self.rng.random_range(0..2u32) == 0 {
            format!(
              "fbm(pos=({}) * 4., octaves=2, seed={})",
              self.expr(2, d.min(1)),
              self.rng.random_range(0..100i64)
            )
          } else {
            self.leaf(arity)
          }
        }
        10 => match arity {
          // captured helper closures (inlined) and ramps (per-texel dynamic kernel)
          1 => match self.rng.random_range(0..3u32) {
            0 => format!("helper1({})", self.expr(1, d)),
            1 => format!("helper2({}, {})", self.expr(1, d), self.expr(1, d)),
            _ => format!("shade1({})", self.expr(1, d)),
          },
          3 => format!("shade3({})", self.expr(1, d)),
          _ => self.leaf(arity),
        },
        _ => {
          // conditionals: plain, else-if chains, and through a mask-param helper
          let c = self.cond(d.min(2));
          match self.rng.random_range(0..3u32) {
            0 => format!("pick({c}, {}, {})", self.arm(arity, d), self.arm(arity, d)),
            1 => format!(
              "if {c} {{ {} }} else if {} {{ {} }} else {{ {} }}",
              self.arm(arity, d),
              self.cond(d.min(2)),
              self.arm(arity, d),
              self.arm(arity, d)
            ),
            _ => format!(
              "if {c} {{ {} }} else {{ {} }}",
              self.arm(arity, d),
              self.arm(arity, d)
            ),
          }
        }
      }
    }

    fn leaf(&mut self, arity: u8) -> String {
      let candidates: Vec<&(String, u8)> = self.vars.iter().filter(|(_, a)| *a == arity).collect();
      if !candidates.is_empty() && self.rng.random_range(0..3u32) > 0 {
        return candidates[self.rng.random_range(0..candidates.len())]
          .0
          .clone();
      }
      match arity {
        1 => {
          if !self.no_int_leaf && self.rng.random_range(0..4u32) == 0 {
            format!("{}", self.rng.random_range(-3..7i64))
          } else {
            self.lit()
          }
        }
        2 => format!("v2({}, {})", self.lit(), self.lit()),
        3 => format!("v3({}, {}, {})", self.lit(), self.lit(), self.lit()),
        _ => format!(
          "v4({}, {}, {}, {})",
          self.lit(),
          self.lit(),
          self.lit(),
          self.lit()
        ),
      }
    }

    fn body(&mut self, out_arity: u8) -> String {
      let mut stmts = Vec::new();
      let mut has_return = false;
      for _ in 0..self.rng.random_range(0..4u32) {
        let name = format!("l{}", self.next_local);
        self.next_local += 1;
        if self.rng.random_range(0..4u32) == 0 {
          let (c, r) = (self.cond(2), self.arm(out_arity, 2));
          stmts.push(if self.rng.random_range(0..2u32) == 0 {
            format!("if {c} {{ return {r} }}")
          } else {
            let (c2, r2) = (self.cond(1), self.arm(out_arity, 1));
            format!("if {c} {{\n    return {r}\n  }} else if {c2} {{\n    return {r2}\n  }}")
          });
          has_return = true;
          continue;
        }
        if self.rng.random_range(0..5u32) == 0 {
          let ar = self.rng.random_range(1..=4u8);
          let init = self.expr(ar, 1);
          self.vars.push(("acc".to_string(), ar));
          let step = self.expr(ar, 2);
          self.vars.pop();
          let k = self.rng.random_range(1..=3u32);
          stmts.push(format!(
            "{name} = fold({init}, |acc, o| {{ ({step}) * 0.5 + acc * (0.75 + float(o) * 0.01) \
             }}, 0..{k})"
          ));
          self.vars.push((name, ar));
          continue;
        }
        if self.rng.random_range(0..3u32) == 0 {
          let c = self.cond(2);
          let hint = if self.rng.random_range(0..2u32) == 0 {
            ": bool"
          } else {
            ""
          };
          stmts.push(format!("{name}{hint} = {c}"));
          self.bvars.push(name);
          continue;
        }
        let ar = self.rng.random_range(1..=4u8);
        let e = self.expr(ar, 2);
        stmts.push(format!("{name} = {e}"));
        self.vars.push((name, ar));
      }
      // A returning body's fall-through is a select arm, so no bare int leaf. Bound to a
      // name because a line starting with an operator continues the previous statement.
      let e = if has_return {
        self.arm(out_arity, 3)
      } else {
        self.expr(out_arity, 3)
      };
      stmts.push(format!("res = {e}\n  res"));
      stmts.join("\n  ")
    }
  }

  /// Source for one `texture_zip` input of the given arity; `i`/`k` vary the pixels.
  fn zip_src(arity: u8, i: usize, k: usize) -> String {
    let f = 2. + i as f32 + k as f32 * 0.5;
    match arity {
      1 => format!("texture(12, 9, |uv| fbm(pos=uv * {f:?}) * 2.)"),
      2 => format!("texture(12, 9, |uv| v2(uv.x * {f:?}, fbm(pos=uv * {f:?})))"),
      3 => format!("texture(12, 9, |uv| v3(uv.x, uv.y * {f:?}, fbm(pos=uv * {f:?})))"),
      _ => format!("texture(12, 9, |uv| v4(uv.x, uv.y, fbm(pos=uv * {f:?}), {f:?} * 0.1))"),
    }
  }

  #[test]
  fn differential_fuzz() {
    let n_seeds: u64 = std::env::var("GEOSCRIPT_FUZZ_SEEDS")
      .ok()
      .and_then(|s| s.parse().ok())
      .unwrap_or(120);
    let mut skipped = 0u64;
    for seed in 0..n_seeds {
      // Seeds rotate through all three entry points: the generator (`texture(w, h, |uv| …)`,
      // no texel param), the map, and `texture_zip` with 2–4 inputs of mixed arity.
      let shape = seed % 3;
      let zip_arities: Vec<u8> = (0..2 + (seed / 3) % 3)
        .map(|i| 1 + ((seed / 3 + i * 7) % 4) as u8)
        .collect();
      let mut vars = vec![
        ("uv".to_string(), 2),
        ("cap".to_string(), 1),
        ("cap3".to_string(), 3),
      ];
      match shape {
        0 => {}
        1 => vars.push(("v".to_string(), 1)),
        _ => vars.extend(
          zip_arities
            .iter()
            .enumerate()
            .map(|(i, &ar)| (format!("p{i}"), ar)),
        ),
      }
      let mut g = Gen {
        rng: Pcg32::new(0xcafef00dd15ea5e5 ^ seed, 0xa02bdbf7bb3c0a7 ^ (seed << 17)),
        vars,
        bvars: Vec::new(),
        next_local: 0,
        no_int_leaf: false,
      };
      let out_arity = g.rng.random_range(1..=4u8);
      let body = g.body(out_arity);
      // Every body is invoked twice from one binding — the second invocation takes the
      // plan-cache path, which is where inlined-closure frames and uniform re-evaluation
      // live and where hand-written fixtures had no coverage.
      let entry = match shape {
        0 => format!(
          "gen = |uv| {{\n  {body}\n}}\nout = texture(12, 9, gen)\nout2 = texture(9, 12, gen)"
        ),
        1 => format!(
          "t = texture(12, 9, |uv| fbm(pos=uv * 3.) * 2.)\nt2 = texture(12, 9, |uv| uv.x * 1.7 - \
           0.3)\nf = |v, uv| {{\n  {body}\n}}\nout = t -> f\nout2 = t2 -> f"
        ),
        // Two same-shaped input sets so `out2` takes the plan-cache path under the same input
        // signature, the way the map shape reuses one body across `t` and `t2`.
        _ => {
          let decls: String = zip_arities
            .iter()
            .enumerate()
            .flat_map(|(i, &ar)| (0..2).map(move |k| format!("z{i}_{k} = {}\n", zip_src(ar, i, k))))
            .collect();
          let params: Vec<String> = (0..zip_arities.len()).map(|i| format!("p{i}")).collect();
          let list = |k: usize| {
            (0..zip_arities.len())
              .map(|i| format!("z{i}_{k}"))
              .collect::<Vec<_>>()
              .join(", ")
          };
          format!(
            "{decls}f = |{}, uv| {{\n  {body}\n}}\nout = [{}] | texture_zip(f)\nout2 = [{}] | \
             texture_zip(f)",
            params.join(", "),
            list(0),
            list(1)
          )
        }
      };
      let src = format!(
        r#"
cap = 1.37
cap3 = v3(0.2, -1.1, 0.6)
flag_t = cap > 1.
flag_f = cap < 0.
helper1 = |x| sigmoid(x * 2.) - 0.25
helper2 = |a, b| a * b + abs(a)
pick = |m: bool, a, b| if m {{ a }} else {{ b }}
pickb = |m: bool, a: bool, b: bool| if m {{ a }} else {{ b }}
shade1 = ramp(stops=[[0., 0.], [0.5, 0.8], [1., 1.]])
shade3 = color_ramp(stops=[srgb(0x102030), srgb(0xf0e0d0)])
{entry}
out | render_texture(name="o")
out2 | render_texture(name="o2")
"#
      );

      let vec_ctx = EvalCtx::default();
      let vec_res = crate::parse_and_eval_program_with_ctx(src.clone(), &vec_ctx, false);
      let scalar_ctx = EvalCtx::default();
      scalar_ctx.tex_vectorize.no_vectorize.set(true);
      let scalar_res = crate::parse_and_eval_program_with_ctx(src.clone(), &scalar_ctx, false);
      match (vec_res, scalar_res) {
        (Ok(_), Ok(_)) => {}
        // Both paths reject the program (a generator type gap, e.g. an overload that doesn't
        // exist); counted so a generator regression can't silently hollow out the sweep.
        (Err(_), Err(_)) => {
          skipped += 1;
          continue;
        }
        (a, b) => panic!(
          "path disagreement on seed {seed}: vectorized {:?} vs scalar {:?}\nprogram:\n{src}",
          a.map(|_| ()),
          b.map(|_| ())
        ),
      }

      let (a, b) = (
        vec_ctx.rendered_textures.borrow(),
        scalar_ctx.rendered_textures.borrow(),
      );
      assert_eq!(a.len(), b.len());
      for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        assert_bit_identical(
          &Value::Texture(Rc::clone(&x.texture)),
          &Value::Texture(Rc::clone(&y.texture)),
        )
        .unwrap_or_else(|e| panic!("seed {seed}, output {i}: {e}\nprogram:\n{src}"));
      }

      let reps: Vec<_> = vec_ctx
        .tex_vectorize
        .reports
        .borrow()
        .values()
        .cloned()
        .collect();
      assert!(
        !reps.is_empty() && reps.iter().all(|r| r.vectorized),
        "seed {seed}: generated whitelist-only body failed to vectorize: {reps:?}\nprogram:\n{src}"
      );
    }
    assert!(
      skipped * 10 < n_seeds,
      "{skipped}/{n_seeds} generated programs were rejected by both paths"
    );
  }
}
