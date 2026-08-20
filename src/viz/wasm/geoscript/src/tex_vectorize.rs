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
  rc::Rc,
};

use arrayvec::ArrayVec;
use fxhash::{FxHashMap, FxHasher};

use crate::{
  ast::{BinOp, CaptureFrom, Expr, FunctionCall, FunctionCallTarget, PrefixOp, Statement, VarRes},
  builtins::{fn_defs::fn_sigs, resolve_tile_period, tex_kernels as kern},
  get_args,
  noise::{fbm_1d, fbm_2d, fbm_2d_tileable, fbm_3d},
  ArgRef, ArgType, Callable, Closure, ControlFlow, ErrorStack, EvalCtx, FrameEnv, GetArgsOutput,
  SourceLoc, Sym, TexStorage, TextureHandle, Value, Vec2, Vec3, Vec4, EMPTY_KWARGS,
};

const MIN_TEXELS: usize = 64;
const MAX_CACHED_PLANS: usize = 512;
const REG_BYTE_BUDGET: usize = 512 << 20;

pub struct VectorizeState {
  plans: RefCell<FxHashMap<PlanKey, Option<Rc<Plan>>>>,
  /// Shared per-(w,h) uv planes: `u = (x+0.5)/w`, `v = (y+0.5)/h` — every generator and
  /// uv-referencing map body at one size reads the same two read-only planes.
  uv_planes: RefCell<FxHashMap<(u32, u32), [Rc<Vec<f32>>; 2]>>,
  pub no_vectorize: Cell<bool>,
  pub verify: Cell<bool>,
  /// Per-body outcome for diagnostics, keyed by `ResolvedBody::id`; last invocation wins.
  pub reports: RefCell<FxHashMap<u64, VectorizeReport>>,
}

impl Default for VectorizeState {
  fn default() -> Self {
    #[cfg(not(target_arch = "wasm32"))]
    let env_on = |name: &str| std::env::var_os(name).is_some_and(|v| v != "0");
    #[cfg(target_arch = "wasm32")]
    let env_on = |_name: &str| false;
    VectorizeState {
      plans: RefCell::new(FxHashMap::default()),
      uv_planes: RefCell::new(FxHashMap::default()),
      no_vectorize: Cell::new(env_on("GEOSCRIPT_NO_VECTORIZE")),
      verify: Cell::new(env_on("GEOSCRIPT_VECTORIZE_VERIFY")),
      reports: RefCell::new(FxHashMap::default()),
    }
  }
}

#[derive(Clone, Debug)]
pub struct VectorizeReport {
  pub vectorized: bool,
  /// Bail reason naming the offending construct.
  pub reason: Option<String>,
  /// (line, col) of the offending node (or the body on success).
  pub loc: (u32, u32),
}

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
struct PlanKey {
  body_id: u64,
  input_arity: u8,
  generator: bool,
  capture_sig: u64,
}

#[derive(Clone, Copy, PartialEq)]
enum Src {
  Reg(u16),
  /// Input texture plane (the texel param's channel).
  In(u8),
  /// Channel of the ctx-cached per-(w,h) uv planes (0 = u, 1 = v); read-only, shared.
  Uv(u8),
  /// Channel of a runtime uniform value.
  Uni(u16, u8),
  Const(f32),
}

#[derive(Clone, Copy, PartialEq, Debug)]
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
  /// Vec-arm lerp (nalgebra axpy): `t*b + (1-t)*a`, srcs (a, b, t).
  LerpV,
}

enum UniSrc {
  Expr(Expr),
  Const(Value),
  Capture(u16),
  /// Read of a uniform local from the frame's mirror (call targets resolved via slots).
  Slot(u16),
  /// Copy of an earlier uniform-table value (binds uniform args to inlinee param slots).
  UniRef(u16),
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum UniShape {
  /// Extracted as per-channel f32s with this arity.
  Num(u8),
  /// A builtin callable with this `fn_entry_ix` (validated, never extracted).
  Builtin(usize),
  /// A closure whose body was inlined; validated by `ResolvedBody` id.
  ClosureBody(u64),
  /// A pure dynamic callable with this return arity (validated, invoked per texel).
  Dynamic(u8),
  /// Used as a raw `Value` (fbm params); validated at the use site.
  Any,
}

struct UniStep {
  src: UniSrc,
  shape: UniShape,
  frame: u16,
  slot: Option<u16>,
  hint: Option<ArgType>,
}

/// One interpreter frame the plan evaluates uniform expressions in: frame 0 is the texel
/// closure itself; each closure inline adds one, with its captures/self taken from the
/// callee value at `callee_uix` per run.
struct FrameSpec {
  n_slots: u16,
  callee_uix: Option<u16>,
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

enum Step {
  Op { kind: OpKind, dst: u16, a: Src, b: Src, c: Src },
  Fbm(FbmStep),
  Dyn(DynStep),
}

enum PlanOut {
  Chans(ArrayVec<Src, 4>),
  Uniform(u16),
}

pub(crate) struct Plan {
  frames: Vec<FrameSpec>,
  n_regs: u16,
  steps: Vec<Step>,
  unis: Vec<UniStep>,
  n_fbm: u16,
  /// Step index of the last read per register (`u32::MAX` = output, never freed).
  reg_last: Vec<u32>,
  out: PlanOut,
  /// Peak simultaneously-live registers, for the memory gate.
  peak_regs: u16,
  /// Whether any step or output reads the shared uv planes.
  uses_uv: bool,
}

/// Resolved+validated per-run uniform state; `exec` is infallible given one of these.
struct UniRun {
  vals: Vec<Value>,
  chans: Vec<[f32; 4]>,
  fbm: Vec<FbmResolved>,
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
}

#[derive(Clone)]
enum AbsVal {
  U(u16),
  V(VV),
}

#[derive(Clone)]
enum SlotState {
  Unset,
  Uniform,
  Varying(VV),
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
    ctx.rendered_textures.inner.borrow_mut().truncate(self.textures);
    ctx.rendered_gizmos.inner.borrow_mut().truncate(self.gizmos);
    ctx.rendered_controls.inner.borrow_mut().truncate(self.controls);
    ctx.next_render_id.set(self.next_render_id);
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

fn uv_planes_for(ctx: &EvalCtx, w: usize, h: usize) -> [Rc<Vec<f32>>; 2] {
  let key = (w as u32, h as u32);
  if let Some(p) = ctx.tex_vectorize.uv_planes.borrow().get(&key) {
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
  if cache.len() >= MAX_CACHED_UV_SIZES {
    cache.clear();
  }
  cache.insert(key, planes.clone());
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
  reg_last: Vec<u32>,
  /// Frame stack; last = the frame currently being compiled.
  frames: Vec<CFrame>,
  plan_frames: Vec<FrameSpec>,
  /// `ResolvedBody` ids of closures currently being inlined (recursion guard).
  inline_stack: Vec<u64>,
  uses_uv: bool,
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
    captures: Rc<[Value]>,
    self_ref: Rc<Callable>,
  ) {
    let plan_frame = self.plan_frames.len() as u16;
    self.plan_frames.push(FrameSpec { n_slots, callee_uix });
    self.frames.push(CFrame {
      plan_frame,
      slot_abs: vec![SlotState::Unset; n_slots as usize],
      mirror: Rc::new(RefCell::new(vec![Value::Nil; n_slots as usize])),
      captures,
      self_ref,
    });
  }

  fn alloc_reg(&mut self) -> u16 {
    let r = self.n_regs;
    self.n_regs += 1;
    self.reg_last.push(0);
    r
  }

  fn touch(&mut self, s: Src) {
    if let Src::Reg(r) = s {
      self.reg_last[r as usize] = self.steps.len() as u32;
    }
  }

  fn push_op(&mut self, kind: OpKind, a: Src, b: Src, c: Src) -> Src {
    self.touch(a);
    self.touch(b);
    self.touch(c);
    let dst = self.alloc_reg();
    self.steps.push(Step::Op { kind, dst, a, b, c });
    Src::Reg(dst)
  }

  fn push_uni(&mut self, src: UniSrc, val: Value, slot: Option<u16>, hint: Option<ArgType>) -> u16 {
    let uix = self.unis.len() as u16;
    self.unis.push(UniStep {
      src,
      shape: UniShape::Any,
      frame: self.cur().plan_frame,
      slot,
      hint,
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
  /// right arity for varyings.
  fn typed_value(&self, v: &AbsVal) -> Value {
    match v {
      AbsVal::U(uix) => self.uni_val(*uix).clone(),
      AbsVal::V(vv) => phantom(vv.chans.len() as u8),
    }
  }

  fn arity(&self, v: &AbsVal) -> Result<u8, CErr> {
    match v {
      AbsVal::U(uix) => num_arity(self.uni_val(*uix))
        .ok_or_else(|| CErr::Bail("non-numeric uniform operand in varying expression".into(), SourceLoc::default())),
      AbsVal::V(vv) => Ok(vv.chans.len() as u8),
    }
  }

  /// Channel `c` of a value as a kernel source. For uniforms this pins the extracted-numeric
  /// shape so cache-hit runs validate against it.
  fn chan(&mut self, v: &AbsVal, c: u8) -> Src {
    match v {
      AbsVal::U(uix) => {
        let ar = num_arity(self.uni_val(*uix)).expect("chan() on non-numeric uniform");
        self.unis[*uix as usize].shape = UniShape::Num(ar);
        Src::Uni(*uix, c.min(ar - 1))
      }
      AbsVal::V(vv) => vv.chans[c as usize],
    }
  }

  // -------------------------------------------------------------------------------------
  // Uniform-subtree classification
  // -------------------------------------------------------------------------------------

  fn slot_is_varying(&self, slot: u16) -> bool {
    matches!(self.cur().slot_abs[slot as usize], SlotState::Varying(_))
  }

  fn expr_is_uniform(&self, expr: &Expr) -> bool {
    match expr {
      Expr::Ident { res, .. } => match res {
        VarRes::Local(slot) => !self.slot_is_varying(*slot),
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
          VarRes::Local(slot) => !self.slot_is_varying(slot),
          _ => true,
        };
        target_uniform
          && call.args.iter().all(|a| self.expr_is_uniform(a))
          && call.kwargs.values().all(|a| self.expr_is_uniform(a))
      }
      Expr::Closure { resolved, .. } => resolved.as_ref().is_some_and(|meta| {
        meta.captures.iter().all(|(_, from)| match from {
          CaptureFrom::Local(slot) => !self.slot_is_varying(*slot),
          _ => true,
        })
      }),
      Expr::ArrayLiteral { elements, .. } => elements.iter().all(|e| self.expr_is_uniform(e)),
      Expr::MapLiteral { entries, .. } => {
        entries.iter().all(|e| self.expr_is_uniform(e.expr()))
      }
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
          && else_expr.as_deref().map_or(true, |e| self.expr_is_uniform(e))
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
    match self.ctx.eval_expr_env(expr, &frame) {
      Ok(ControlFlow::Continue(v)) => Ok(v),
      Ok(_) => bail("return/break escaped a uniform subtree", expr.loc()),
      Err(e) => Err(CErr::Err(e)),
    }
  }

  fn emit_uniform(&mut self, expr: &Expr) -> Result<AbsVal, CErr> {
    let val = self.eval_uniform_now(expr)?;
    Ok(AbsVal::U(self.push_uni(
      UniSrc::Expr(expr.clone()),
      val,
      None,
      None,
    )))
  }

  // -------------------------------------------------------------------------------------
  // Varying-spine compilation
  // -------------------------------------------------------------------------------------

  fn compile_expr(&mut self, expr: &Expr) -> Result<AbsVal, CErr> {
    if self.expr_is_uniform(expr) {
      return self.emit_uniform(expr);
    }
    let loc = expr.loc();
    match expr {
      Expr::Ident { res, .. } => match res {
        VarRes::Local(slot) => match &self.cur().slot_abs[*slot as usize] {
          SlotState::Varying(vv) => Ok(AbsVal::V(vv.clone())),
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
            return bail("varying comparison (masks land in Phase 2)", loc)
          }
          BinOp::And | BinOp::Or => return bail("varying boolean op (masks land in Phase 2)", loc),
          other => return bail(format!("unsupported operator `{other:?}` on varying values"), loc),
        };
        let l = self.compile_expr(lhs)?;
        let r = self.compile_expr(rhs)?;
        self.lower_arith(*op, kind, l, r, loc)
      }
      Expr::PrefixOp { op, expr: inner, .. } => match op {
        PrefixOp::Neg => {
          let v = self.compile_expr(inner)?;
          self.elementwise(OpKind::Neg, &v)
        }
        PrefixOp::Pos => self.compile_expr(inner),
        PrefixOp::Not => bail("varying `!` (masks land in Phase 2)", loc),
      },
      Expr::StaticFieldAccess { lhs, field, .. } => {
        let v = self.compile_expr(lhs)?;
        self.lower_swizzle(&v, field, loc)
      }
      Expr::Call { call, .. } => self.lower_call(call, loc),
      Expr::Block { statements, .. } => self.compile_statements(statements, loc),
      Expr::Conditional { .. } => bail("varying conditional (Phase 2)", loc),
      Expr::FieldAccess { .. } => bail("indexing a varying value", loc),
      Expr::Range { .. } => bail("range over a varying value", loc),
      Expr::ArrayLiteral { .. } => bail("array literal containing varying values", loc),
      Expr::MapLiteral { .. } => bail("map literal containing varying values", loc),
      Expr::Closure { .. } => bail("closure capturing a varying value", loc),
      Expr::Literal { .. } => unreachable!("literals are uniform"),
    }
  }

  fn elementwise(&mut self, kind: OpKind, v: &AbsVal) -> Result<AbsVal, CErr> {
    let ar = self.arity(v)?;
    let mut chans = ArrayVec::new();
    for c in 0..ar {
      let s = self.chan(v, c);
      chans.push(self.push_op(kind, s, Src::Const(0.), Src::Const(0.)));
    }
    Ok(AbsVal::V(VV { chans }))
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
          return bail("non-numeric uniform operand of a varying arithmetic op", loc);
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
    Ok(AbsVal::V(VV { chans }))
  }

  fn lower_swizzle(&mut self, v: &AbsVal, field: &str, loc: SourceLoc) -> Result<AbsVal, CErr> {
    let AbsVal::V(vv) = v else { unreachable!() };
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
    Ok(AbsVal::V(VV { chans }))
  }

  fn lower_pipeline(
    &mut self,
    lhs: AbsVal,
    rhs: &Expr,
    loc: SourceLoc,
  ) -> Result<AbsVal, CErr> {
    if !self.expr_is_uniform(rhs) {
      return bail("varying callee in pipeline", loc);
    }
    // A literal callable is baked into the AST — no per-run identity to validate. Anything
    // else re-resolves per run through the uniform table.
    if let Expr::Literal {
      value: Value::Callable(c),
      ..
    } = rhs
    {
      return self.lower_callable_call(&c.clone(), None, vec![lhs], FxHashMap::default(), loc);
    }
    let val = self.eval_uniform_now(rhs)?;
    let Value::Callable(c) = &val else {
      return bail("pipeline into a non-callable varying combination", loc);
    };
    let c = c.clone();
    let uix = self.push_uni(UniSrc::Expr(rhs.clone()), val, None, None);
    self.lower_callable_call(&c, Some(uix), vec![lhs], FxHashMap::default(), loc)
  }

  fn lower_call(&mut self, call: &FunctionCall, loc: SourceLoc) -> Result<AbsVal, CErr> {
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
          if self.slot_is_varying(slot) {
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
        self.lower_builtin(*fn_entry_ix, pre_resolved_signature.as_ref(), args, kwargs, loc)
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
          return bail(format!("dynamic callable `{name}` without a usable return type hint"), loc);
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
            let s = self.chan(a, c);
            self.touch(s);
            chans.push(s);
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
        self.steps.push(Step::Dyn(DynStep {
          callee,
          args: arg_srcs,
          dst: dst.clone(),
        }));
        Ok(AbsVal::V(VV {
          chans: dst.iter().map(|r| Src::Reg(*r)).collect(),
        }))
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
      Rc::clone(&inner.captures),
      Rc::clone(callable),
    );

    let result = self.bind_and_compile_inline(inner, args, kwargs, loc);

    self.frames.pop();
    self.inline_stack.pop();
    let result = result?;

    if let Some(hint) = inner.return_type_hint {
      match &result {
        AbsVal::V(vv) => {
          let ok = matches!(
            (hint, vv.chans.len()),
            (ArgType::Float | ArgType::Numeric, 1)
              | (ArgType::Vec2, 2)
              | (ArgType::Vec3, 3)
              | (ArgType::Vec4, 4)
          );
          if !ok {
            return bail("return type hint mismatch on inlined closure", loc);
          }
        }
        AbsVal::U(uix) => {
          if hint.validate_val(self.uni_val(*uix)).is_err() {
            return bail("return type hint violation on inlined closure", loc);
          }
        }
      }
    }
    Ok(result)
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
      if let Some(hint) = param.type_hint {
        match &bound {
          AbsVal::U(uix) => {
            if hint.validate_val(self.uni_val(*uix)).is_err() {
              return bail("param type hint violation on inlined closure", loc);
            }
          }
          AbsVal::V(vv) => {
            let ok = matches!(
              (hint, vv.chans.len()),
              (ArgType::Float | ArgType::Numeric, 1)
                | (ArgType::Vec2, 2)
                | (ArgType::Vec3, 3)
                | (ArgType::Vec4, 4)
            );
            if !ok {
              return bail("param type hint mismatch on inlined closure", loc);
            }
          }
        }
      }
      match bound {
        AbsVal::U(src_uix) => {
          let val = self.uni_val(src_uix).clone();
          self.cur().mirror.borrow_mut()[slot as usize] = val.clone();
          self.push_uni(UniSrc::UniRef(src_uix), val, Some(slot), None);
          self.cur_mut().slot_abs[slot as usize] = SlotState::Uniform;
        }
        AbsVal::V(vv) => {
          self.cur_mut().slot_abs[slot as usize] = SlotState::Varying(vv);
        }
      }
    }
    self.compile_statements(&inner.body.0, loc)
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
      "sin", "cos", "tan", "asin", "acos", "atan", "sqrt", "exp", "log2", "floor", "ceil",
      "round", "fract", "trunc", "sigmoid", "abs", "pow", "atan2", "min", "max", "clamp",
      "smoothstep", "lerp", "len", "dot", "distance", "normalize", "vec2", "vec3", "vec4", "fbm",
      "linearstep", "remap",
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
            return bail(format!("partial application of `{name}` on varying values"), loc)
          }
          Err(e) => return Err(CErr::Err(e)),
        }
      }
    };

    let mut arg = |compiler: &mut Self, i: usize| -> AbsVal {
      match &arg_refs[i] {
        ArgRef::Positional(ix) => args[*ix].clone(),
        ArgRef::Keyword(sym) => kwargs[sym].clone(),
        ArgRef::Default(v) => AbsVal::U(compiler.push_uni(UniSrc::Const(v.clone()), v.clone(), None, None)),
      }
    };

    match (*name, def_ix) {
      ("sin", _) => { let x = arg(self, 0); self.elementwise(OpKind::Sin, &x) }
      ("cos", _) => { let x = arg(self, 0); self.elementwise(OpKind::Cos, &x) }
      ("tan", _) => { let x = arg(self, 0); self.elementwise(OpKind::Tan, &x) }
      ("asin", _) => { let x = arg(self, 0); self.elementwise(OpKind::Asin, &x) }
      ("acos", _) => { let x = arg(self, 0); self.elementwise(OpKind::Acos, &x) }
      ("atan", _) => { let x = arg(self, 0); self.elementwise(OpKind::Atan, &x) }
      ("sqrt", _) => { let x = arg(self, 0); self.elementwise(OpKind::Sqrt, &x) }
      ("exp", _) => { let x = arg(self, 0); self.elementwise(OpKind::Exp, &x) }
      ("log2", _) => { let x = arg(self, 0); self.elementwise(OpKind::Log2, &x) }
      ("floor", _) => { let x = arg(self, 0); self.elementwise(OpKind::Floor, &x) }
      ("ceil", _) => { let x = arg(self, 0); self.elementwise(OpKind::Ceil, &x) }
      ("round", _) => { let x = arg(self, 0); self.elementwise(OpKind::Round, &x) }
      ("fract", _) => { let x = arg(self, 0); self.elementwise(OpKind::Fract, &x) }
      ("trunc", _) => { let x = arg(self, 0); self.elementwise(OpKind::Trunc, &x) }
      ("sigmoid", _) => { let x = arg(self, 0); self.elementwise(OpKind::Sigmoid, &x) }
      ("abs", 1..=3) => { let x = arg(self, 0); self.elementwise(OpKind::Abs, &x) }
      ("pow", 0..=2) => {
        let base = arg(self, 0);
        let expo = arg(self, 1);
        let ar = self.arity(&base)?;
        let e = self.chan(&expo, 0);
        let mut chans = ArrayVec::new();
        for c in 0..ar {
          let b = self.chan(&base, c);
          chans.push(self.push_op(OpKind::Pow, b, e, Src::Const(0.)));
        }
        Ok(AbsVal::V(VV { chans }))
      }
      ("atan2", 0) => {
        let y = arg(self, 0);
        let x = arg(self, 1);
        let (ys, xs) = (self.chan(&y, 0), self.chan(&x, 0));
        Ok(AbsVal::V(VV { chans: [self.push_op(OpKind::Atan2, ys, xs, Src::Const(0.))].into_iter().collect() }))
      }
      ("atan2", 1) => {
        let v = arg(self, 0);
        let (ys, xs) = (self.chan(&v, 1), self.chan(&v, 0));
        Ok(AbsVal::V(VV { chans: [self.push_op(OpKind::Atan2, ys, xs, Src::Const(0.))].into_iter().collect() }))
      }
      ("min", 1..=3) | ("max", 1..=3) => {
        let kind = if *name == "min" { OpKind::Min } else { OpKind::Max };
        let a = arg(self, 0);
        let b = arg(self, 1);
        let ar = self.arity(&a)?;
        let mut chans = ArrayVec::new();
        for c in 0..ar {
          let (x, y) = (self.chan(&a, c), self.chan(&b, c.min(self.arity(&b)? - 1)));
          chans.push(self.push_op(kind, x, y, Src::Const(0.)));
        }
        Ok(AbsVal::V(VV { chans }))
      }
      ("clamp", 1..=3) => {
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
        Ok(AbsVal::V(VV { chans }))
      }
      ("smoothstep", 0) => {
        let e0 = arg(self, 0);
        let e1 = arg(self, 1);
        let x = arg(self, 2);
        let (e0s, e1s, xs) = (self.chan(&e0, 0), self.chan(&e1, 0), self.chan(&x, 0));
        Ok(AbsVal::V(VV { chans: [self.push_op(OpKind::SmoothStep, xs, e0s, e1s)].into_iter().collect() }))
      }
      ("lerp", 0..=3) => {
        let t = arg(self, 0);
        let a = arg(self, 1);
        let b = arg(self, 2);
        let kind = if def_ix == 1 { OpKind::LerpF } else { OpKind::LerpV };
        let ar = self.arity(&a)?;
        let ts = self.chan(&t, 0);
        let mut chans = ArrayVec::new();
        for c in 0..ar {
          let (x, y) = (self.chan(&a, c), self.chan(&b, c));
          chans.push(self.push_op(kind, x, y, ts));
        }
        Ok(AbsVal::V(VV { chans }))
      }
      ("len", 0..=1) => {
        let v = arg(self, 0);
        let s = self.sum_of_products(&v, &v)?;
        Ok(AbsVal::V(VV { chans: [self.push_op(OpKind::Sqrt, s, Src::Const(0.), Src::Const(0.))].into_iter().collect() }))
      }
      ("dot", 0..=1) => {
        let a = arg(self, 0);
        let b = arg(self, 1);
        let s = self.sum_of_products(&a, &b)?;
        Ok(AbsVal::V(VV { chans: [s].into_iter().collect() }))
      }
      ("distance", 0..=1) => {
        let a = arg(self, 0);
        let b = arg(self, 1);
        let ar = self.arity(&a)?;
        let mut acc = None;
        for c in 0..ar {
          let (x, y) = (self.chan(&a, c), self.chan(&b, c));
          let d = self.push_op(OpKind::Sub, x, y, Src::Const(0.));
          let sq = self.push_op(OpKind::Mul, d, d, Src::Const(0.));
          acc = Some(match acc {
            None => sq,
            Some(prev) => self.push_op(OpKind::Add, prev, sq, Src::Const(0.)),
          });
        }
        let s = acc.unwrap();
        Ok(AbsVal::V(VV { chans: [self.push_op(OpKind::Sqrt, s, Src::Const(0.), Src::Const(0.))].into_iter().collect() }))
      }
      ("normalize", 0..=1) => {
        let v = arg(self, 0);
        let ar = self.arity(&v)?;
        let s = self.sum_of_products(&v, &v)?;
        let norm = self.push_op(OpKind::Sqrt, s, Src::Const(0.), Src::Const(0.));
        let mut chans = ArrayVec::new();
        for c in 0..ar {
          let x = self.chan(&v, c);
          chans.push(self.push_op(OpKind::Div, x, norm, Src::Const(0.)));
        }
        Ok(AbsVal::V(VV { chans }))
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
        Ok(AbsVal::V(VV { chans: [self.push_op(OpKind::LinearStep, xs, e0s, e1s)].into_iter().collect() }))
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
        Ok(AbsVal::V(VV { chans }))
      }
      ("fbm", _) => self.lower_fbm(def_ix, &mut arg, loc),
      _ => bail(format!("`{name}` def {def_ix} is not vectorizable"), loc),
    }
  }

  fn sum_of_products(&mut self, a: &AbsVal, b: &AbsVal) -> Result<Src, CErr> {
    let ar = self.arity(a)?;
    let mut acc = None;
    for c in 0..ar {
      let (x, y) = (self.chan(a, c), self.chan(b, c));
      let m = self.push_op(OpKind::Mul, x, y, Src::Const(0.));
      acc = Some(match acc {
        None => m,
        Some(prev) => self.push_op(OpKind::Add, prev, m, Src::Const(0.)),
      });
    }
    Ok(acc.unwrap())
  }

  fn construct(&mut self, comps: &[(AbsVal, u8)]) -> Result<AbsVal, CErr> {
    let mut chans = ArrayVec::new();
    for (v, c) in comps {
      chans.push(self.chan(v, *c));
    }
    Ok(AbsVal::V(VV { chans }))
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
        AbsVal::V(_) => bail(format!("varying fbm `{what}` parameter"), loc),
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

    let dst = self.alloc_reg();
    let rix = self.n_fbm;
    self.n_fbm += 1;
    for s in pos_srcs.iter().take(dim as usize) {
      self.touch(*s);
    }
    self.steps.push(Step::Fbm(FbmStep {
      dim,
      dst,
      pos: pos_srcs,
      params,
      tileable,
      rix,
    }));
    Ok(AbsVal::V(VV { chans: [Src::Reg(dst)].into_iter().collect() }))
  }

  // -------------------------------------------------------------------------------------
  // Statements
  // -------------------------------------------------------------------------------------

  fn compile_statements(&mut self, stmts: &[Statement], loc: SourceLoc) -> Result<AbsVal, CErr> {
    let mut last: Option<AbsVal> = None;
    for (i, stmt) in stmts.iter().enumerate() {
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
          } else {
            let vv = match self.compile_expr(expr)? {
              AbsVal::V(vv) => vv,
              AbsVal::U(_) => unreachable!(),
            };
            if let Some(hint) = type_hint {
              let hint_ar = match hint {
                ArgType::Float | ArgType::Numeric => 1,
                ArgType::Vec2 => 2,
                ArgType::Vec3 => 3,
                ArgType::Vec4 => 4,
                _ => return bail("unsupported type hint on varying assignment", expr.loc()),
              };
              if hint_ar != vv.chans.len() {
                return bail("type hint arity mismatch on varying assignment", expr.loc());
              }
            }
            self.cur_mut().slot_abs[*slot as usize] = SlotState::Varying(vv);
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
          unreachable!("pre-filter rejects return/break")
        }
      }
    }
    last.ok_or_else(|| CErr::Bail("empty closure body".into(), loc))
  }
}

// ---------------------------------------------------------------------------------------
// Pre-filter
// ---------------------------------------------------------------------------------------

/// Syntactic whole-body bails: effectful/rng literal builtin calls anywhere (incl. nested
/// closure bodies — the effect fence backstops non-literal callees at run time), `return`/
/// `break` in the body proper, and — for the texel closure itself (`xy_params`) — any
/// reference to the `x_ix`/`y_ix` params.
fn prefilter(closure: &Closure, xy_from: Option<usize>) -> Result<(), CErr> {
  let mut xy_slots: Vec<u16> = Vec::new();
  if let Some(xy_from) = xy_from {
    for (i, param) in closure.params.iter().enumerate().skip(xy_from) {
      let n_idents = param.ident.iter_idents().count();
      let start = closure.resolved.param_slots[i];
      xy_slots.extend((0..n_idents as u16).map(|k| start + k));
    }
  }

  let mut bad: Option<(String, SourceLoc)> = None;
  for stmt in &closure.body.0 {
    check_stmt_control_flow(stmt, &mut bad);
    stmt.traverse_exprs(&mut |e: &Expr| {
      if bad.is_some() {
        return;
      }
      match e {
        Expr::Call {
          call:
            FunctionCall {
              target: FunctionCallTarget::Literal(c),
              ..
            },
          loc,
          ..
        } => {
          if c.is_side_effectful() || c.is_rng_dependent() {
            bad = Some((format!("side-effectful or rng-dependent call: {c:?}"), *loc));
          }
        }
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
          if meta.captures.iter().any(
            |(_, from)| matches!(from, CaptureFrom::Local(s) if xy_slots.contains(s)),
          ) {
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

/// Return/break scan over the body's own statements; does not descend into nested closure
/// bodies (those run through the interpreter or get their own walk when inlined).
fn check_stmt_control_flow(stmt: &Statement, bad: &mut Option<(String, SourceLoc)>) {
  if bad.is_some() {
    return;
  }
  match stmt {
    Statement::Return { value } => {
      *bad = Some((
        "early `return` in texel closure".into(),
        value.as_ref().map(|e| e.loc()).unwrap_or_default(),
      ))
    }
    Statement::Break { value } => {
      *bad = Some((
        "`break` in texel closure".into(),
        value.as_ref().map(|e| e.loc()).unwrap_or_default(),
      ))
    }
    _ => {
      for e in stmt.exprs() {
        check_expr_control_flow(e, bad);
      }
    }
  }
}

fn check_expr_control_flow(expr: &Expr, bad: &mut Option<(String, SourceLoc)>) {
  if bad.is_some() {
    return;
  }
  match expr {
    Expr::Block { statements, .. } => {
      for s in statements {
        check_stmt_control_flow(s, bad);
      }
    }
    Expr::BinOp { lhs, rhs, .. } => {
      check_expr_control_flow(lhs, bad);
      check_expr_control_flow(rhs, bad);
    }
    Expr::PrefixOp { expr, .. } => check_expr_control_flow(expr, bad),
    Expr::Range { start, end, .. } => {
      check_expr_control_flow(start, bad);
      if let Some(e) = end {
        check_expr_control_flow(e, bad);
      }
    }
    Expr::StaticFieldAccess { lhs, .. } => check_expr_control_flow(lhs, bad),
    Expr::FieldAccess {
      lhs, field, field2, ..
    } => {
      check_expr_control_flow(lhs, bad);
      check_expr_control_flow(field, bad);
      if let Some(f) = field2 {
        check_expr_control_flow(f, bad);
      }
    }
    Expr::Call { call, .. } => {
      for a in &call.args {
        check_expr_control_flow(a, bad);
      }
      for a in call.kwargs.values() {
        check_expr_control_flow(a, bad);
      }
    }
    Expr::Conditional {
      cond,
      then,
      else_if_exprs,
      else_expr,
      ..
    } => {
      check_expr_control_flow(cond, bad);
      check_expr_control_flow(then, bad);
      for (c, e) in else_if_exprs {
        check_expr_control_flow(c, bad);
        check_expr_control_flow(e, bad);
      }
      if let Some(e) = else_expr {
        check_expr_control_flow(e, bad);
      }
    }
    Expr::ArrayLiteral { elements, .. } => {
      for e in elements {
        check_expr_control_flow(e, bad);
      }
    }
    // Closure bodies deliberately skipped.
    Expr::MapLiteral { .. } | Expr::Closure { .. } | Expr::Ident { .. } | Expr::Literal { .. } => {}
  }
}

// ---------------------------------------------------------------------------------------
// Uniform evaluation + validation (per run)
// ---------------------------------------------------------------------------------------

enum UniErr {
  /// Fall back to the scalar loop (validation surprise or observable effect).
  Abort,
  Err(ErrorStack),
}

fn eval_uniforms(
  ctx: &EvalCtx,
  plan: &Plan,
  closure: &Closure,
  callable: &Rc<Callable>,
) -> Result<Vec<Value>, UniErr> {
  let fence = EffectFence::snapshot(ctx);
  let res = eval_uniforms_inner(ctx, plan, closure, callable);
  if !fence.verify_or_restore(ctx) {
    return Err(UniErr::Abort);
  }
  res
}

fn eval_uniforms_inner(
  ctx: &EvalCtx,
  plan: &Plan,
  closure: &Closure,
  callable: &Rc<Callable>,
) -> Result<Vec<Value>, UniErr> {
  // Frame 0 = the texel closure; inline frames resolve their captures/self from the callee
  // value once its uniform entry has been evaluated (always earlier in the table).
  let mirrors: Vec<RefCell<Vec<Value>>> = plan
    .frames
    .iter()
    .map(|f| RefCell::new(vec![Value::Nil; f.n_slots as usize]))
    .collect();
  let mut frame_callees: Vec<Option<Rc<Callable>>> = vec![None; plan.frames.len()];
  let mut vals: Vec<Value> = Vec::with_capacity(plan.unis.len());

  for step in &plan.unis {
    let fi = step.frame as usize;
    let (captures, self_ref): (Rc<[Value]>, Rc<Callable>) = if fi == 0 {
      (Rc::clone(&closure.captures), Rc::clone(callable))
    } else {
      let callee = match &frame_callees[fi] {
        Some(c) => Rc::clone(c),
        None => {
          let uix = plan.frames[fi].callee_uix.unwrap() as usize;
          let Value::Callable(c) = &vals[uix] else {
            return Err(UniErr::Abort);
          };
          frame_callees[fi] = Some(Rc::clone(c));
          Rc::clone(c)
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
          Ok(ControlFlow::Continue(v)) => v,
          Ok(_) => return Err(UniErr::Abort),
          Err(e) => return Err(UniErr::Err(e)),
        }
      }
      UniSrc::Const(v) => v.clone(),
      UniSrc::Capture(ix) => captures[*ix as usize].clone(),
      UniSrc::Slot(slot) => mirrors[fi].borrow()[*slot as usize].clone(),
      UniSrc::UniRef(uix) => vals[*uix as usize].clone(),
    };
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
  Ok(vals)
}

/// Shape-checks the uniform table against what compile observed and pre-resolves everything
/// `exec` needs, so `exec` is infallible.
fn validate_uniforms(plan: &Plan, vals: Vec<Value>) -> Option<UniRun> {
  let mut chans = vec![[0f32; 4]; vals.len()];
  for (i, (step, val)) in plan.unis.iter().zip(&vals).enumerate() {
    match step.shape {
      UniShape::Num(ar) => {
        let (c, got_ar) = value_chans(val)?;
        if got_ar != ar {
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
      UniShape::Any => {}
    }
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
  for step in &plan.steps {
    let Step::Fbm(f) = step else { continue };
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
  Some(UniRun { vals, chans, fbm })
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
        let n_uses = srcs.iter().filter(|o| matches!(o, Src::Reg(x) if x == r)).count();
        if n_uses == 1
          && plan.reg_last[*r as usize] == step_ix
          && self.regs[*r as usize].is_some()
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

fn write_zip3(buf: &mut Vec<f32>, a: &[f32], b: &[f32], c: &[f32], f: impl Fn(f32, f32, f32) -> f32) {
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

fn exec(
  ctx: &EvalCtx,
  plan: &Plan,
  uni: &UniRun,
  input: &[Rc<Vec<f32>>],
  uv: Option<&[Rc<Vec<f32>>; 2]>,
  n: usize,
) -> Result<Vec<Rc<Vec<f32>>>, ErrorStack> {
  let mut ex = Exec {
    regs: vec![None; plan.n_regs as usize],
    pool: Vec::new(),
    uni,
    input,
    uv,
  };

  for (step_ix, step) in plan.steps.iter().enumerate() {
    let step_ix = step_ix as u32;
    match step {
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
        buf.reserve(n);
        let get = |s: Src, i: usize| -> f32 {
          match s {
            Src::Reg(r) => ex.regs[r as usize].as_ref().unwrap()[i],
            Src::In(c) => ex.input[c as usize][i],
            Src::Uv(c) => ex.uv.unwrap()[c as usize][i],
            Src::Uni(uix, c) => ex.uni.chans[uix as usize][c as usize],
            Src::Const(k) => k,
          }
        };
        match f.dim {
          1 => {
            for i in 0..n {
              buf.push(fbm_1d(p.seed, p.octaves, p.frequency, p.persistence, p.lacunarity, get(f.pos[0], i)));
            }
          }
          2 => match p.tileable {
            Some(period) => {
              for i in 0..n {
                let pos = Vec2::new(get(f.pos[0], i), get(f.pos[1], i));
                buf.push(fbm_2d_tileable(p.seed, p.octaves, p.frequency, p.persistence, p.lacunarity, period, pos));
              }
            }
            None => {
              for i in 0..n {
                let pos = Vec2::new(get(f.pos[0], i), get(f.pos[1], i));
                buf.push(fbm_2d(p.seed, p.octaves, p.frequency, p.persistence, p.lacunarity, pos));
              }
            }
          },
          _ => {
            for i in 0..n {
              let pos = Vec3::new(get(f.pos[0], i), get(f.pos[1], i), get(f.pos[2], i));
              buf.push(fbm_3d(p.seed, p.octaves, p.frequency, p.persistence, p.lacunarity, pos));
            }
          }
        }
        ex.regs[f.dst as usize] = Some(buf);
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
          let res = inner
            .invoke(&argv, EMPTY_KWARGS, ctx)
            .map_err(|e| e.wrap(format!("Error invoking dynamic callable `{name}` per texel")))?;
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
      if stolen.is_some() {
        // In-place for ternaries: only when the stolen reg is src a.
        if stolen_is(a) {
          match (ex.resolve(b), ex.resolve(c)) {
            (RSrc::K(y), RSrc::K(z)) => kern::map_in(buf, |x| f(x, y, z)),
            (RSrc::S(s), RSrc::K(z)) => kern::zip_in_a(buf, s, |x, y| f(x, y, z)),
            (RSrc::K(y), RSrc::S(s)) => kern::zip_in_a(buf, s, |x, z| f(x, y, z)),
            (RSrc::S(sb), RSrc::S(sc)) => {
              let n_ = buf.len();
              for i in 0..n_ {
                buf[i] = f(buf[i], sb[i], sc[i]);
              }
            }
          }
        } else {
          // Stolen b or c: give the buffer back conceptually — recompute out-of-place into
          // a temp, then move. Rare shape; correctness over cleverness.
          let saved = std::mem::take(buf);
          let rs = |s: Src| -> RSrc<'_> {
            match (s, stolen) {
              (Src::Reg(r), Some(st)) if r == st => RSrc::S(&saved),
              (s, _) => ex.resolve(s),
            }
          };
          let mut out = Vec::new();
          run_ternary(&mut out, rs(a), rs(b), rs(c), n, f);
          *buf = out;
          ex.pool.push(saved);
        }
      } else {
        run_ternary(buf, ex.resolve(a), ex.resolve(b), ex.resolve(c), n, f);
      }
    }};
  }

  match kind {
    OpKind::Neg => unary!(|x: f32| -x),
    OpKind::Abs => unary!(|x: f32| x.abs()),
    OpKind::Sqrt => unary!(|x: f32| x.sqrt()),
    OpKind::Sin => unary!(|x: f32| x.sin()),
    OpKind::Cos => unary!(|x: f32| x.cos()),
    OpKind::Tan => unary!(|x: f32| x.tan()),
    OpKind::Asin => unary!(|x: f32| x.asin()),
    OpKind::Acos => unary!(|x: f32| x.acos()),
    OpKind::Atan => unary!(|x: f32| x.atan()),
    OpKind::Exp => unary!(|x: f32| x.exp()),
    OpKind::Log2 => unary!(|x: f32| x.log2()),
    OpKind::Floor => unary!(|x: f32| x.floor()),
    OpKind::Ceil => unary!(|x: f32| x.ceil()),
    OpKind::Round => unary!(|x: f32| x.round()),
    OpKind::Fract => unary!(|x: f32| x.fract()),
    OpKind::Trunc => unary!(|x: f32| x.trunc()),
    OpKind::Sigmoid => unary!(|x: f32| 1.0 / (1.0 + (-x).exp())),
    OpKind::Add => binary!(|x: f32, y: f32| x + y),
    OpKind::Sub => binary!(|x: f32, y: f32| x - y),
    OpKind::Mul => binary!(|x: f32, y: f32| x * y),
    OpKind::Div => binary!(|x: f32, y: f32| x / y),
    OpKind::Mod => binary!(|x: f32, y: f32| x % y),
    OpKind::Pow => binary!(|x: f32, y: f32| x.powf(y)),
    OpKind::Atan2 => binary!(|y: f32, x: f32| y.atan2(x)),
    OpKind::Min => binary!(|x: f32, y: f32| x.min(y)),
    OpKind::Max => binary!(|x: f32, y: f32| x.max(y)),
    OpKind::Clamp => ternary!(crate::builtins::clampf),
    OpKind::SmoothStep => ternary!(|x: f32, e0: f32, e1: f32| {
      let t = ((x - e0) / (e1 - e0)).clamp(0., 1.);
      t * t * (3. - 2. * t)
    }),
    OpKind::LinearStep => ternary!(|x: f32, e0: f32, e1: f32| ((x - e0) / (e1 - e0)).clamp(0., 1.)),
    OpKind::LerpF => ternary!(|a: f32, b: f32, t: f32| a + (b - a) * t),
    OpKind::LerpV => ternary!(|a: f32, b: f32, t: f32| t * b + (1. - t) * a),
  }
}

// ---------------------------------------------------------------------------------------
// Entry
// ---------------------------------------------------------------------------------------

fn param_slot_referenced(closure: &Closure, slot: usize) -> bool {
  let mut referenced = false;
  for stmt in &closure.body.0 {
    stmt.traverse_exprs(&mut |e: &Expr| {
      match e {
        Expr::Ident {
          res: VarRes::Local(s),
          ..
        } if *s as usize == slot => referenced = true,
        Expr::Closure { resolved: Some(m), .. } => {
          if m
            .captures
            .iter()
            .any(|(_, f)| matches!(f, CaptureFrom::Local(s) if *s as usize == slot))
          {
            referenced = true;
          }
        }
        _ => {}
      }
    });
  }
  referenced
}

#[derive(Clone, Copy)]
enum CompileKind {
  /// `t -> |val, uv, x_ix, y_ix| …`
  Map { input_arity: u8 },
  /// `texture(w, h, |uv, x_ix, y_ix| …)`
  Generator,
}

fn compile(
  ctx: &EvalCtx,
  callable: &Rc<Callable>,
  closure: &Closure,
  kind: CompileKind,
) -> Result<(Plan, Vec<Value>), CErr> {
  let (xy_from, max_params) = match kind {
    CompileKind::Map { .. } => (2, 4),
    CompileKind::Generator => (1, 3),
  };
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
    reg_last: Vec::new(),
    frames: Vec::new(),
    plan_frames: Vec::new(),
    inline_stack: vec![meta.id],
    uses_uv: false,
  };
  compiler.push_frame(
    meta.n_slots,
    None,
    Rc::clone(&closure.captures),
    Rc::clone(callable),
  );

  // Map: param 0 = texel value (input planes), param 1 = uv. Generator: param 0 = uv.
  // uv binds to the shared ctx-cached planes — zero ops, zero copies — and only when the
  // body actually references it, so uniform bodies never build the planes.
  if let CompileKind::Map { input_arity } = kind {
    if !closure.params.is_empty() {
      let mut chans = ArrayVec::new();
      for c in 0..input_arity {
        chans.push(Src::In(c));
      }
      compiler.cur_mut().slot_abs[meta.param_slots[0] as usize] = SlotState::Varying(VV { chans });
    }
  }
  let uv_param_ix = xy_from - 1;
  if closure.params.len() > uv_param_ix {
    let uv_slot = meta.param_slots[uv_param_ix] as usize;
    if param_slot_referenced(closure, uv_slot) {
      let mut chans = ArrayVec::new();
      chans.push(Src::Uv(0));
      chans.push(Src::Uv(1));
      compiler.cur_mut().slot_abs[uv_slot] = SlotState::Varying(VV { chans });
      compiler.uses_uv = true;
    }
  }

  let body_loc = closure.body.0.first().and_then(|s| s.exprs().next()).map(|e| e.loc()).unwrap_or_default();
  let result = compiler.compile_statements(&closure.body.0, body_loc);
  if !fence.verify_or_restore(ctx) {
    return bail("uniform subtree performed an observable effect", body_loc);
  }
  let result = result?;

  let out = match result {
    AbsVal::V(vv) => {
      for s in &vv.chans {
        if let Src::Reg(r) = s {
          compiler.reg_last[*r as usize] = u32::MAX;
        }
      }
      PlanOut::Chans(vv.chans)
    }
    AbsVal::U(uix) => PlanOut::Uniform(uix),
  };

  // Peak live registers via step-order simulation, for the run-time memory gate.
  let mut live = vec![false; compiler.n_regs as usize];
  let mut peak = 0u16;
  let mut cur = 0u16;
  for (step_ix, step) in compiler.steps.iter().enumerate() {
    let dsts: &[u16] = match step {
      Step::Op { dst, .. } => std::slice::from_ref(dst),
      Step::Fbm(f) => std::slice::from_ref(&f.dst),
      Step::Dyn(d) => &d.dst,
    };
    for &d in dsts {
      if !live[d as usize] {
        live[d as usize] = true;
        cur += 1;
        peak = peak.max(cur);
      }
    }
    for (r, l) in live.iter_mut().enumerate() {
      if *l && compiler.reg_last[r] == step_ix as u32 {
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
      unis: compiler.unis,
      n_fbm: compiler.n_fbm,
      reg_last: compiler.reg_last,
      out,
      peak_regs: peak.max(1),
      uses_uv: compiler.uses_uv,
    },
    compiler.uni_vals,
  ))
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
) -> Result<Option<Vec<Rc<Vec<f32>>>>, ErrorStack> {
  match &plan.out {
    PlanOut::Uniform(uix) => {
      let Some((chans, ar)) = value_chans(&uni.vals[*uix as usize]) else {
        return Ok(None);
      };
      Ok(Some(
        (0..ar as usize).map(|c| Rc::new(vec![chans[c]; n])).collect(),
      ))
    }
    PlanOut::Chans(_) => exec(ctx, plan, uni, input, uv, n).map(Some),
  }
}

/// The vectorized fast path for `map` over a texture. `None` ⇒ run the scalar loop.
pub(crate) fn try_vectorized_map(
  ctx: &EvalCtx,
  cb: &Rc<Callable>,
  tex: &TextureHandle,
) -> Option<Result<Value, ErrorStack>> {
  let state = &ctx.tex_vectorize;
  if state.no_vectorize.get() {
    return None;
  }
  let Callable::Closure(closure) = &**cb else {
    return None;
  };
  let (w, h) = (tex.width, tex.height);
  if w * h < MIN_TEXELS {
    return None;
  }

  let key = PlanKey {
    body_id: closure.resolved.id,
    input_arity: tex.channels as u8,
    generator: false,
    capture_sig: capture_sig(closure),
  };

  let cached = state.plans.borrow().get(&key).map(|p| p.clone());
  let (plan, uni_vals) = match cached {
    Some(None) => return None,
    Some(Some(plan)) => {
      let vals = match eval_uniforms(ctx, &plan, closure, cb) {
        Ok(v) => v,
        Err(UniErr::Abort) => return None,
        Err(UniErr::Err(e)) => return Some(Err(e)),
      };
      (plan, vals)
    }
    None => {
      match compile(ctx, cb, closure, CompileKind::Map { input_arity: tex.channels as u8 }) {
        Ok((plan, vals)) => {
          let plan = Rc::new(plan);
          if state.plans.borrow().len() >= MAX_CACHED_PLANS {
            state.plans.borrow_mut().clear();
          }
          state.plans.borrow_mut().insert(key, Some(plan.clone()));
          state.reports.borrow_mut().insert(
            key.body_id,
            VectorizeReport {
              vectorized: true,
              reason: None,
              loc: (0, 0),
            },
          );
          (plan, vals)
        }
        Err(CErr::Bail(reason, loc)) => {
          if state.plans.borrow().len() >= MAX_CACHED_PLANS {
            state.plans.borrow_mut().clear();
          }
          state.plans.borrow_mut().insert(key, None);
          let (line, col) = ctx.resolve_loc(loc);
          state.reports.borrow_mut().insert(
            key.body_id,
            VectorizeReport {
              vectorized: false,
              reason: Some(reason),
              loc: (line, col),
            },
          );
          return None;
        }
        Err(CErr::Err(e)) => return Some(Err(e)),
      }
    }
  };

  if plan.peak_regs as usize * w * h * 4 > REG_BYTE_BUDGET {
    return None;
  }
  let uni = validate_uniforms(&plan, uni_vals)?;
  let input = tex.as_planes();
  let uv = plan.uses_uv.then(|| uv_planes_for(ctx, w, h));
  match plan_output_planes(ctx, &plan, &uni, &input, uv.as_ref(), w * h) {
    Ok(Some(planes)) => {
      let channels = planes.len();
      Some(Ok(Value::Texture(Rc::new(TextureHandle {
        channels,
        storage: TexStorage::planes(planes),
        mips: Default::default(),
        ..tex.clone()
      }))))
    }
    Ok(None) => None,
    Err(e) => Some(Err(e)),
  }
}

/// The vectorized fast path for `texture(w, h, generator)`. `None` ⇒ run the scalar loop.
pub(crate) fn try_vectorized_texture(
  ctx: &EvalCtx,
  cb: &Rc<Callable>,
  w: usize,
  h: usize,
  wrap: crate::TextureWrap,
) -> Option<Result<Value, ErrorStack>> {
  let state = &ctx.tex_vectorize;
  if state.no_vectorize.get() {
    return None;
  }
  let Callable::Closure(closure) = &**cb else {
    return None;
  };
  if w * h < MIN_TEXELS {
    return None;
  }

  let key = PlanKey {
    body_id: closure.resolved.id,
    input_arity: 0,
    generator: true,
    capture_sig: capture_sig(closure),
  };

  let cached = state.plans.borrow().get(&key).map(|p| p.clone());
  let (plan, uni_vals) = match cached {
    Some(None) => return None,
    Some(Some(plan)) => {
      let vals = match eval_uniforms(ctx, &plan, closure, cb) {
        Ok(v) => v,
        Err(UniErr::Abort) => return None,
        Err(UniErr::Err(e)) => return Some(Err(e)),
      };
      (plan, vals)
    }
    None => match compile(ctx, cb, closure, CompileKind::Generator) {
      Ok((plan, vals)) => {
        let plan = Rc::new(plan);
        if state.plans.borrow().len() >= MAX_CACHED_PLANS {
          state.plans.borrow_mut().clear();
        }
        state.plans.borrow_mut().insert(key, Some(plan.clone()));
        state.reports.borrow_mut().insert(
          key.body_id,
          VectorizeReport {
            vectorized: true,
            reason: None,
            loc: (0, 0),
          },
        );
        (plan, vals)
      }
      Err(CErr::Bail(reason, loc)) => {
        if state.plans.borrow().len() >= MAX_CACHED_PLANS {
          state.plans.borrow_mut().clear();
        }
        state.plans.borrow_mut().insert(key, None);
        let (line, col) = ctx.resolve_loc(loc);
        state.reports.borrow_mut().insert(
          key.body_id,
          VectorizeReport {
            vectorized: false,
            reason: Some(reason),
            loc: (line, col),
          },
        );
        return None;
      }
      Err(CErr::Err(e)) => return Some(Err(e)),
    },
  };

  if plan.peak_regs as usize * w * h * 4 > REG_BYTE_BUDGET {
    return None;
  }
  let uni = validate_uniforms(&plan, uni_vals)?;
  let uv = plan.uses_uv.then(|| uv_planes_for(ctx, w, h));
  match plan_output_planes(ctx, &plan, &uni, &[], uv.as_ref(), w * h) {
    Ok(Some(planes)) => {
      let channels = planes.len();
      Some(Ok(Value::Texture(Rc::new(TextureHandle {
        storage: TexStorage::planes(planes),
        width: w,
        height: h,
        channels,
        wrap,
        min_filter: None,
        mag_filter: None,
        format: None,
        transform: crate::Mat4::identity(),
        mips: Default::default(),
      }))))
    }
    Ok(None) => None,
    Err(e) => Some(Err(e)),
  }
}

/// Bit-exact comparison for `GEOSCRIPT_VECTORIZE_VERIFY`.
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
      if va.to_bits() != vb.to_bits() {
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

  fn eval_both(src: &str) -> (crate::EvalCtx, crate::EvalCtx) {
    let vec_ctx = parse_and_eval_program(src).unwrap();
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
    ctx.tex_vectorize.reports.borrow().values().cloned().collect()
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
    assert!(n_vec >= 8, "expected all bodies to vectorize; reports: {reps:?}");
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
(t -> |v| if v > 0.5 { 1. } else { 0. }) | render_texture(name="o")"#,
        "conditional",
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
(t -> |v| (0..3 -> |i| v * float(i)) | last) | render_texture(name="o")"#,
        "",
      ),
      (
        r#"t = texture(16, 16, |uv| uv.x)
(t -> |v| { c = || v * 2.
c() }) | render_texture(name="o")"#,
        "closure capturing a varying",
      ),
      (
        r#"t = texture(16, 16, |uv| uv.x)
(t -> |v| { [a, b] = [v, v * 2.]
a + b }) | render_texture(name="o")"#,
        "destructuring",
      ),
      (
        r#"t = texture(16, 16, |uv| uv.x)
(t -> |v| { return v * 2. }) | render_texture(name="o")"#,
        "return",
      ),
      (
        r#"t = texture(16, 16, |uv| uv.x)
(t -> |v| { print(v)
v }) | render_texture(name="o")"#,
        "side-effectful",
      ),
      (
        r#"t = texture(16, 16, |uv| uv.x)
(t -> |v| v + randf()) | render_texture(name="o")"#,
        "rng",
      ),
      (
        r#"t = texture(16, 16, |uv| uv.x)
(t -> |v| [v, v * 2.] | first) | render_texture(name="o")"#,
        "",
      ),
      (
        r#"t = texture(16, 16, |uv| uv.x)
(t -> |v| v > 0.5) | render_texture(name="o")"#,
        "!err",
      ),
    ] {
      let ctx = EvalCtx::default();
      let res = crate::parse_and_eval_program_with_ctx(src.to_string(), &ctx, false);
      if needle == "!err" {
        // body returns a bool: both paths must error
        assert!(res.is_err());
        continue;
      }
      res.unwrap_or_else(|e| panic!("scalar fallback must succeed: {e}\n{src}"));
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
        && r.reason.as_deref().is_some_and(|s| s.contains("observable effect"))),
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
    let cached = vec_ctx.tex_vectorize.uv_planes.borrow()[&(8, 8)].clone();
    assert!(Rc::ptr_eq(&c.as_planes()[0], &cached[0]));
    assert!(Rc::ptr_eq(&c.as_planes()[1], &cached[1]));
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
    next_local: usize,
  }

  impl Gen {
    fn lit(&mut self) -> String {
      let v = (self.rng.random::<f32>() - 0.4) * 4.;
      format!("{v:?}")
    }

    /// A random expression of the requested arity.
    fn expr(&mut self, arity: u8, depth: u8) -> String {
      if depth == 0 {
        return self.leaf(arity);
      }
      let d = depth - 1;
      match self.rng.random_range(0..11u32) {
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
          let f = ["sin", "cos", "sqrt", "abs", "exp", "floor", "fract", "sigmoid", "round"]
            [self.rng.random_range(0..9usize)];
          format!("{f}({})", self.expr(arity, d))
        }
        4 => format!("-({})", self.expr(arity, d)),
        5 => {
          // clamp / smoothstep-ish scalar shapes
          if arity == 1 {
            match self.rng.random_range(0..3u32) {
              0 => format!(
                "smoothstep({}, {}, {})",
                self.lit(),
                self.lit(),
                self.expr(1, d)
              ),
              1 => format!("atan2({}, {})", self.expr(1, d), self.expr(1, d)),
              _ => format!("({} % {})", self.expr(1, d), self.lit()),
            }
          } else {
            format!("clamp({}, {}, {})", self.lit(), self.lit(), self.expr(arity, d))
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
              let src_ar = self.rng.random_range(2..=3u8);
              match self.rng.random_range(0..3u32) {
                0 => format!("len({})", self.expr(src_ar, d)),
                1 => format!("dot({}, {})", self.expr(src_ar, d), self.expr(src_ar, d)),
                _ => format!("distance({}, {})", self.expr(src_ar, d), self.expr(src_ar, d)),
              }
            }
            2 => format!("v2({}, {})", self.expr(1, d), self.expr(1, d)),
            3 => match self.rng.random_range(0..3u32) {
              0 => format!("v3({}, {}, {})", self.expr(1, d), self.expr(1, d), self.expr(1, d)),
              1 => format!("v3({}, {})", self.expr(2, d), self.expr(1, d)),
              _ => format!("normalize({})", self.expr(3, d)),
            },
            _ => format!("v4({}, {})", self.expr(2, d), self.expr(2, d)),
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
        _ => match arity {
          // captured helper closures (inlined) and ramps (per-texel dynamic kernel)
          1 => match self.rng.random_range(0..3u32) {
            0 => format!("helper1({})", self.expr(1, d)),
            1 => format!("helper2({}, {})", self.expr(1, d), self.expr(1, d)),
            _ => format!("shade1({})", self.expr(1, d)),
          },
          3 => format!("shade3({})", self.expr(1, d)),
          _ => self.leaf(arity),
        },
      }
    }

    fn leaf(&mut self, arity: u8) -> String {
      let candidates: Vec<&(String, u8)> =
        self.vars.iter().filter(|(_, a)| *a == arity).collect();
      if !candidates.is_empty() && self.rng.random_range(0..3u32) > 0 {
        return candidates[self.rng.random_range(0..candidates.len())].0.clone();
      }
      match arity {
        1 => {
          if self.rng.random_range(0..4u32) == 0 {
            format!("{}", self.rng.random_range(-3..7i64))
          } else {
            self.lit()
          }
        }
        2 => format!("v2({}, {})", self.lit(), self.lit()),
        3 => format!("v3({}, {}, {})", self.lit(), self.lit(), self.lit()),
        _ => format!("v4({}, {}, {}, {})", self.lit(), self.lit(), self.lit(), self.lit()),
      }
    }

    fn body(&mut self, out_arity: u8) -> String {
      let mut stmts = Vec::new();
      for _ in 0..self.rng.random_range(0..3u32) {
        let ar = self.rng.random_range(1..=4u8);
        let name = format!("l{}", self.next_local);
        self.next_local += 1;
        let e = self.expr(ar, 2);
        stmts.push(format!("{name} = {e}"));
        self.vars.push((name, ar));
      }
      stmts.push(self.expr(out_arity, 3));
      stmts.join("\n  ")
    }
  }

  #[test]
  fn differential_fuzz() {
    for seed in 0..120u64 {
      // Every third seed fuzzes the generator entry (`texture(w, h, |uv| …)`, no texel
      // param) instead of the map entry.
      let generator_shape = seed % 3 == 0;
      let mut vars = vec![
        ("uv".to_string(), 2),
        ("cap".to_string(), 1),
        ("cap3".to_string(), 3),
      ];
      if !generator_shape {
        vars.push(("v".to_string(), 1));
      }
      let mut g = Gen {
        rng: Pcg32::new(0xcafef00dd15ea5e5 ^ seed, 0xa02bdbf7bb3c0a7 ^ (seed << 17)),
        vars,
        next_local: 0,
      };
      let out_arity = g.rng.random_range(1..=4u8);
      let body = g.body(out_arity);
      let entry = if generator_shape {
        format!("out = texture(12, 9, |uv| {{\n  {body}\n}})")
      } else {
        format!(
          "t = texture(12, 9, |uv| fbm(pos=uv * 3.) * 2.)\nout = t -> |v, uv| {{\n  {body}\n}}"
        )
      };
      let src = format!(
        r#"
cap = 1.37
cap3 = v3(0.2, -1.1, 0.6)
helper1 = |x| sigmoid(x * 2.) - 0.25
helper2 = |a, b| a * b + abs(a)
shade1 = ramp(stops=[[0., 0.], [0.5, 0.8], [1., 1.]])
shade3 = color_ramp(stops=[srgb(0x102030), srgb(0xf0e0d0)])
{entry}
out | render_texture(name="o")
"#
      );

      let vec_ctx = EvalCtx::default();
      let vec_res = crate::parse_and_eval_program_with_ctx(src.clone(), &vec_ctx, false);
      let scalar_ctx = EvalCtx::default();
      scalar_ctx.tex_vectorize.no_vectorize.set(true);
      let scalar_res = crate::parse_and_eval_program_with_ctx(src.clone(), &scalar_ctx, false);
      match (vec_res, scalar_res) {
        (Ok(_), Ok(_)) => {}
        (Err(a), Err(_b)) => {
          let _ = a;
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
      assert_bit_identical(
        &Value::Texture(Rc::clone(&a[0].texture)),
        &Value::Texture(Rc::clone(&b[0].texture)),
      )
      .unwrap_or_else(|e| panic!("seed {seed}: {e}\nprogram:\n{src}"));

      let reps: Vec<_> = vec_ctx.tex_vectorize.reports.borrow().values().cloned().collect();
      assert!(
        !reps.is_empty() && reps.iter().all(|r| r.vectorized),
        "seed {seed}: generated whitelist-only body failed to vectorize: {reps:?}\nprogram:\n{src}"
      );
    }
  }
}
