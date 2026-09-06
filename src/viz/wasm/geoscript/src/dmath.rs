//! Transcendentals routed through `libm` on native targets so results are platform-independent
//! (tests, goldens) and match wasm, whose std already lowers these to libm.

#[cfg(not(target_arch = "wasm32"))]
macro_rules! unary {
  ($($f:ident => $l:ident),* $(,)?) => { $(
    #[inline(always)]
    pub fn $f(x: f32) -> f32 { libm::$l(x) }
  )* };
}
#[cfg(target_arch = "wasm32")]
macro_rules! unary {
  ($($f:ident => $l:ident),* $(,)?) => { $(
    #[inline(always)]
    pub fn $f(x: f32) -> f32 { x.$f() }
  )* };
}

unary!(
  sin => sinf, cos => cosf, tan => tanf, asin => asinf, acos => acosf, atan => atanf,
  exp => expf, ln => logf, log2 => log2f, log10 => log10f,
  sinh => sinhf, cosh => coshf, tanh => tanhf, cbrt => cbrtf,
);

#[inline(always)]
pub fn powf(x: f32, y: f32) -> f32 {
  #[cfg(not(target_arch = "wasm32"))]
  {
    libm::powf(x, y)
  }
  #[cfg(target_arch = "wasm32")]
  {
    x.powf(y)
  }
}

#[inline(always)]
pub fn atan2(y: f32, x: f32) -> f32 {
  #[cfg(not(target_arch = "wasm32"))]
  {
    libm::atan2f(y, x)
  }
  #[cfg(target_arch = "wasm32")]
  {
    y.atan2(x)
  }
}

#[inline(always)]
pub fn sin_cos(x: f32) -> (f32, f32) {
  (sin(x), cos(x))
}

#[inline(always)]
pub fn sigmoid(x: f32) -> f32 {
  1.0 / (1.0 + exp(-x))
}

#[inline(always)]
pub fn pow64(x: f64, y: f64) -> f64 {
  #[cfg(not(target_arch = "wasm32"))]
  {
    libm::pow(x, y)
  }
  #[cfg(target_arch = "wasm32")]
  {
    x.powf(y)
  }
}
