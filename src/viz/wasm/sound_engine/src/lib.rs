#![allow(clippy::missing_safety_doc)]

use std::collections::HashMap;

const FRAME_SIZE: usize = 128;
const MAX_VOICES: usize = 64;
const MAX_EVENTS_PER_TICK: usize = 64;
const LIMITER_LOOKAHEAD: usize = 40;

#[inline(always)]
fn one_pole(state: &mut f32, target: f32, coeff: f32) -> f32 {
  *state += (target - *state) * coeff;
  *state
}

#[inline(always)]
fn db_to_gain(db: f32) -> f32 { 10f32.powf(db / 20.0) }

#[inline(always)]
fn gain_to_db(gain: f32) -> f32 {
  if gain <= 0.0 {
    -200.0
  } else {
    20.0 * gain.log10()
  }
}

#[inline(always)]
fn clampf(x: f32, lo: f32, hi: f32) -> f32 { x.max(lo).min(hi) }

/// Linear interpolation read from a sample buffer at fractional position.
#[inline(always)]
fn read_interp(buf: &[f32], pos: f32) -> f32 {
  let i0 = pos as usize;
  let i1 = i0 + 1;
  if i1 >= buf.len() {
    return *buf.last().unwrap_or(&0.0);
  }
  let frac = pos - i0 as f32;
  buf[i0] * (1.0 - frac) + buf[i1] * frac
}

#[derive(Clone, Copy, Default)]
struct BiquadCoeffs {
  b0: f32,
  b1: f32,
  b2: f32,
  a1: f32,
  a2: f32,
}

#[derive(Clone, Copy, Default)]
struct BiquadState {
  z1: f32,
  z2: f32,
}

impl BiquadState {
  // Transposed direct form II
  #[inline(always)]
  fn process(&mut self, c: &BiquadCoeffs, x: f32) -> f32 {
    let y = c.b0 * x + self.z1;
    self.z1 = c.b1 * x - c.a1 * y + self.z2;
    self.z2 = c.b2 * x - c.a2 * y;
    y
  }
}

const FILTER_NONE: u32 = 0;
const FILTER_LP: u32 = 1;
const FILTER_HP: u32 = 2;
const FILTER_BP: u32 = 3;
const FILTER_NOTCH: u32 = 4;

fn make_biquad(kind: u32, freq: f32, q: f32, sample_rate: f32) -> Option<BiquadCoeffs> {
  if kind == FILTER_NONE {
    return None;
  }
  let q = q.max(0.0001);
  let omega = 2.0 * std::f32::consts::PI * freq.max(1.0) / sample_rate;
  let cos_w = omega.cos();
  let sin_w = omega.sin();
  let alpha = sin_w / (2.0 * q);

  let (b0, b1, b2, a0, a1, a2) = match kind {
    FILTER_LP => {
      let b1 = 1.0 - cos_w;
      let b0 = b1 * 0.5;
      let b2 = b0;
      (b0, b1, b2, 1.0 + alpha, -2.0 * cos_w, 1.0 - alpha)
    },
    FILTER_HP => {
      let b1 = -(1.0 + cos_w);
      let b0 = (1.0 + cos_w) * 0.5;
      let b2 = b0;
      (b0, b1, b2, 1.0 + alpha, -2.0 * cos_w, 1.0 - alpha)
    },
    FILTER_BP => {
      let b0 = alpha;
      let b1 = 0.0;
      let b2 = -alpha;
      (b0, b1, b2, 1.0 + alpha, -2.0 * cos_w, 1.0 - alpha)
    },
    FILTER_NOTCH => {
      let b0 = 1.0;
      let b1 = -2.0 * cos_w;
      let b2 = 1.0;
      (b0, b1, b2, 1.0 + alpha, -2.0 * cos_w, 1.0 - alpha)
    },
    _ => return None,
  };

  let inv_a0 = 1.0 / a0;
  Some(BiquadCoeffs {
    b0: b0 * inv_a0,
    b1: b1 * inv_a0,
    b2: b2 * inv_a0,
    a1: a1 * inv_a0,
    a2: a2 * inv_a0,
  })
}

// ---- limiter (linked stereo, ported from web-synth safety_limiter) --------

const LIM_THRESHOLD_DB: f32 = 6.0;
const LIM_RATIO: f32 = 200.0;
const LIM_ATTACK: f32 = 0.08;
const LIM_RELEASE: f32 = 0.003;

struct Limiter {
  envelope: f32,
  lookahead_l: [f32; LIMITER_LOOKAHEAD],
  lookahead_r: [f32; LIMITER_LOOKAHEAD],
}

impl Limiter {
  const fn new() -> Self {
    Self {
      envelope: 0.0,
      lookahead_l: [0.0; LIMITER_LOOKAHEAD],
      lookahead_r: [0.0; LIMITER_LOOKAHEAD],
    }
  }

  #[inline(always)]
  fn detect(&mut self, lookahead_sample: f32) -> f32 {
    let abs_sample = if lookahead_sample.is_normal() {
      lookahead_sample.abs()
    } else {
      0.0
    };
    let coeff = if abs_sample > self.envelope {
      LIM_ATTACK
    } else {
      LIM_RELEASE
    };
    one_pole(&mut self.envelope, abs_sample, coeff)
  }

  #[inline(always)]
  fn gain_for(&self, detected_linear: f32) -> f32 {
    let detected_db = gain_to_db(detected_linear);
    if detected_db < LIM_THRESHOLD_DB {
      1.0
    } else {
      let output_db = LIM_THRESHOLD_DB + (detected_db - LIM_THRESHOLD_DB) / LIM_RATIO;
      db_to_gain(-(detected_db - output_db))
    }
  }

  /// Process one stereo frame in-place. Linked: envelope tracks max(|L|, |R|).
  fn process(&mut self, l: &mut [f32; FRAME_SIZE], r: &mut [f32; FRAME_SIZE]) {
    // Phase 1: process the first LOOKAHEAD samples using the lookahead buffer
    // contents from the previous frame as the delayed signal, and the start of
    // the current frame as the lookahead signal.
    let mut tmp_l = [0.0f32; FRAME_SIZE];
    let mut tmp_r = [0.0f32; FRAME_SIZE];

    for i in 0..LIMITER_LOOKAHEAD {
      let la = l[i].abs().max(r[i].abs());
      let env = self.detect(la);
      let g = self.gain_for(env);
      let out_l = self.lookahead_l[i] * g;
      let out_r = self.lookahead_r[i] * g;
      tmp_l[i] = clampf(out_l, -4.0, 4.0);
      tmp_r[i] = clampf(out_r, -4.0, 4.0);
    }

    // Phase 2: process the rest using current-frame samples directly (their
    // own lookahead is in the same frame).
    for i in LIMITER_LOOKAHEAD..FRAME_SIZE {
      let delayed_l = l[i - LIMITER_LOOKAHEAD];
      let delayed_r = r[i - LIMITER_LOOKAHEAD];
      let la = l[i].abs().max(r[i].abs());
      let env = self.detect(la);
      let g = self.gain_for(env);
      let out_l = delayed_l * g;
      let out_r = delayed_r * g;
      tmp_l[i] = clampf(out_l, -4.0, 4.0);
      tmp_r[i] = clampf(out_r, -4.0, 4.0);
    }

    // Stash the tail of the current frame as the next frame's "delayed" signal.
    for i in 0..LIMITER_LOOKAHEAD {
      self.lookahead_l[i] = l[FRAME_SIZE - LIMITER_LOOKAHEAD + i];
      self.lookahead_r[i] = r[FRAME_SIZE - LIMITER_LOOKAHEAD + i];
    }

    *l = tmp_l;
    *r = tmp_r;
  }
}

// ---- samples + voices ------------------------------------------------------

struct Sample {
  channels: u8, // 1 or 2
  /// Per-channel length in samples. Total floats = channels * len.
  len: u32,
  /// Planar: [ch0..., ch1...]
  data: Vec<f32>,
  /// Pre-baked crossfade copy, same shape. Empty if no crossfade requested.
  xfade_data: Vec<f32>,
}

impl Sample {
  fn read_at(&self, channel: u8, pos: f32, use_xfade: bool) -> f32 {
    let buf = if use_xfade && !self.xfade_data.is_empty() {
      &self.xfade_data
    } else {
      &self.data
    };
    if buf.is_empty() {
      return 0.0;
    }
    let ch = (channel as usize).min(self.channels as usize - 1);
    let start = ch * self.len as usize;
    let end = start + self.len as usize;
    read_interp(&buf[start..end], pos)
  }
}

fn build_xfade(data: &[f32], len: usize, channels: usize, threshold: f32) -> Vec<f32> {
  let mut out = vec![0.0f32; data.len()];
  let half = (threshold * 0.5 * len as f32) as usize;
  if half == 0 {
    out.copy_from_slice(data);
    return out;
  }
  for ch in 0..channels {
    let src = &data[ch * len..(ch + 1) * len];
    let dst = &mut out[ch * len..(ch + 1) * len];
    for i in 0..len {
      let s = if i < half {
        let f = i as f32 / half as f32;
        let other = src[len - i - 1];
        src[i] * f + other * (1.0 - f)
      } else if i >= len - half {
        let f = (len - i) as f32 / half as f32;
        let other = src[len - i - 1];
        src[i] * f + other * (1.0 - f)
      } else {
        src[i]
      };
      dst[i] = s;
    }
  }
  out
}

#[derive(Clone, Copy, PartialEq)]
enum VoiceMode {
  /// Non-spatial oneshot, fixed pan.
  OneshotPan { pan: f32 },
  /// Spatial source, looped with crossfade.
  SpatialLoop {
    pos: [f32; 3],
    ref_dist: f32,
    rolloff: f32,
    cull_threshold: f32,
  },
}

struct Voice {
  active: bool,
  sample_id: u32,
  pos: f32,
  rate: f32,
  gain: f32,
  mode: VoiceMode,
  is_loop: bool,
  biquad_coeffs: Option<BiquadCoeffs>,
  bq_l: BiquadState,
  bq_r: BiquadState,
  age: u64,
  /// External handle (0 = none / oneshot).
  handle: u32,
}

impl Voice {
  const fn empty() -> Self {
    Self {
      active: false,
      sample_id: 0,
      pos: 0.0,
      rate: 1.0,
      gain: 0.0,
      mode: VoiceMode::OneshotPan { pan: 0.0 },
      is_loop: false,
      biquad_coeffs: None,
      bq_l: BiquadState { z1: 0.0, z2: 0.0 },
      bq_r: BiquadState { z1: 0.0, z2: 0.0 },
      age: 0,
      handle: 0,
    }
  }
}

// ---- events ----------------------------------------------------------------

const EV_PLAY_ONESHOT: u32 = 1;
const EV_START_SPATIAL_LOOP: u32 = 2;
const EV_UPDATE_SPATIAL_LOOP: u32 = 3;
const EV_STOP_VOICE: u32 = 4;
const EV_SET_MASTER_GAIN: u32 = 5;
const EV_FREE_SAMPLE: u32 = 6;

#[repr(C)]
#[derive(Clone, Copy)]
struct Event {
  kind: u32,
  handle: u32,
  sample_id: u32,
  flags: u32,
  params: [f32; 12],
}

impl Event {
  const fn zero() -> Self {
    Self {
      kind: 0,
      handle: 0,
      sample_id: 0,
      flags: 0,
      params: [0.0; 12],
    }
  }
}

// ---- listener --------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
struct ListenerPose {
  pos: [f32; 3],
  forward: [f32; 3],
  right: [f32; 3],
  _seq: f32,
}

impl ListenerPose {
  const fn zero() -> Self {
    Self {
      pos: [0.0; 3],
      forward: [0.0, 0.0, -1.0],
      right: [1.0, 0.0, 0.0],
      _seq: 0.0,
    }
  }
}

// ---- ctx -------------------------------------------------------------------

pub struct Ctx {
  sample_rate: f32,
  master_gain: f32,
  samples: HashMap<u32, Sample>,
  voices: [Voice; MAX_VOICES],
  spatial_handles: HashMap<u32, usize>,
  age_counter: u64,
  out_l: [f32; FRAME_SIZE],
  out_r: [f32; FRAME_SIZE],
  events: [Event; MAX_EVENTS_PER_TICK],
  event_count: u32,
  listener: ListenerPose,
  limiter: Limiter,
}

impl Ctx {
  fn new(sample_rate: f32) -> Self {
    Self {
      sample_rate,
      master_gain: 1.0,
      samples: HashMap::new(),
      voices: [const { Voice::empty() }; MAX_VOICES],
      spatial_handles: HashMap::new(),
      age_counter: 0,
      out_l: [0.0; FRAME_SIZE],
      out_r: [0.0; FRAME_SIZE],
      events: [Event::zero(); MAX_EVENTS_PER_TICK],
      event_count: 0,
      listener: ListenerPose::zero(),
      limiter: Limiter::new(),
    }
  }

  fn alloc_voice(&mut self) -> usize {
    self.age_counter += 1;
    // First, try to find an inactive slot.
    for i in 0..MAX_VOICES {
      if !self.voices[i].active {
        return i;
      }
    }
    // All slots active: steal the oldest non-spatial-loop voice if possible,
    // else the oldest voice overall.
    let mut steal_ix = 0usize;
    let mut oldest_nonloop_age = u64::MAX;
    let mut oldest_age = u64::MAX;
    let mut oldest_ix = 0usize;
    let mut found_nonloop = false;
    for i in 0..MAX_VOICES {
      let v = &self.voices[i];
      if v.age < oldest_age {
        oldest_age = v.age;
        oldest_ix = i;
      }
      if !v.is_loop && v.age < oldest_nonloop_age {
        oldest_nonloop_age = v.age;
        steal_ix = i;
        found_nonloop = true;
      }
    }
    let ix = if found_nonloop { steal_ix } else { oldest_ix };
    self.deactivate_voice(ix);
    ix
  }

  fn deactivate_voice(&mut self, ix: usize) {
    let v = &mut self.voices[ix];
    if v.handle != 0 {
      self.spatial_handles.remove(&v.handle);
    }
    v.active = false;
    v.handle = 0;
  }

  fn handle_event(&mut self, ev: &Event) {
    match ev.kind {
      EV_PLAY_ONESHOT => {
        if !self.samples.contains_key(&ev.sample_id) {
          return;
        }
        let ix = self.alloc_voice();
        self.age_counter += 1;
        let v = &mut self.voices[ix];
        v.active = true;
        v.sample_id = ev.sample_id;
        v.pos = 0.0;
        v.gain = ev.params[0];
        v.rate = if ev.params[1] > 0.0 { ev.params[1] } else { 1.0 };
        v.mode = VoiceMode::OneshotPan {
          pan: clampf(ev.params[2], -1.0, 1.0),
        };
        v.is_loop = false;
        v.biquad_coeffs = None;
        v.bq_l = BiquadState::default();
        v.bq_r = BiquadState::default();
        v.age = self.age_counter;
        v.handle = 0;
      },
      EV_START_SPATIAL_LOOP => {
        if !self.samples.contains_key(&ev.sample_id) {
          return;
        }
        // If a voice already exists for this handle, stop it first.
        if let Some(&old_ix) = self.spatial_handles.get(&ev.handle) {
          self.deactivate_voice(old_ix);
        }
        let ix = self.alloc_voice();
        self.age_counter += 1;
        let coeffs = make_biquad(
          ev.params[6] as u32,
          ev.params[7],
          ev.params[8],
          self.sample_rate,
        );
        let v = &mut self.voices[ix];
        v.active = true;
        v.sample_id = ev.sample_id;
        v.pos = 0.0;
        v.gain = ev.params[3];
        v.rate = if ev.params[4] > 0.0 { ev.params[4] } else { 1.0 };
        v.mode = VoiceMode::SpatialLoop {
          pos: [ev.params[0], ev.params[1], ev.params[2]],
          ref_dist: ev.params[9].max(0.01),
          rolloff: ev.params[10].max(0.0),
          cull_threshold: ev.params[11].max(0.0),
        };
        v.is_loop = true;
        v.biquad_coeffs = coeffs;
        v.bq_l = BiquadState::default();
        v.bq_r = BiquadState::default();
        v.age = self.age_counter;
        v.handle = ev.handle;
        if ev.handle != 0 {
          self.spatial_handles.insert(ev.handle, ix);
        }
      },
      EV_UPDATE_SPATIAL_LOOP => {
        if let Some(&ix) = self.spatial_handles.get(&ev.handle) {
          let v = &mut self.voices[ix];
          if let VoiceMode::SpatialLoop {
            ref mut pos,
            ref mut ref_dist,
            ref mut rolloff,
            ref mut cull_threshold,
          } = v.mode
          {
            pos[0] = ev.params[0];
            pos[1] = ev.params[1];
            pos[2] = ev.params[2];
            v.gain = ev.params[3];
            // ref_dist / rolloff / cull retained from start; flag bit 0 = also
            // update them.
            if ev.flags & 0x1 != 0 {
              *ref_dist = ev.params[9].max(0.01);
              *rolloff = ev.params[10].max(0.0);
              *cull_threshold = ev.params[11].max(0.0);
            }
          }
        }
      },
      EV_STOP_VOICE => {
        if let Some(&ix) = self.spatial_handles.get(&ev.handle) {
          self.deactivate_voice(ix);
        }
      },
      EV_SET_MASTER_GAIN => {
        self.master_gain = ev.params[0].max(0.0);
      },
      EV_FREE_SAMPLE => {
        // Stop any voices using this sample.
        for ix in 0..MAX_VOICES {
          if self.voices[ix].active && self.voices[ix].sample_id == ev.sample_id {
            self.deactivate_voice(ix);
          }
        }
        self.samples.remove(&ev.sample_id);
      },
      _ => {},
    }
  }

}

#[no_mangle]
pub extern "C" fn sound_engine_init(sample_rate: f32) -> *mut Ctx {
  Box::into_raw(Box::new(Ctx::new(sample_rate)))
}

#[no_mangle]
pub extern "C" fn sound_engine_get_output_ptr(ctx: *mut Ctx) -> *mut f32 {
  unsafe { (*ctx).out_l.as_mut_ptr() }
}

#[no_mangle]
pub extern "C" fn sound_engine_get_output_r_ptr(ctx: *mut Ctx) -> *mut f32 {
  unsafe { (*ctx).out_r.as_mut_ptr() }
}

#[no_mangle]
pub extern "C" fn sound_engine_get_event_buffer_ptr(ctx: *mut Ctx) -> *mut u8 {
  unsafe { (*ctx).events.as_mut_ptr() as *mut u8 }
}

#[no_mangle]
pub extern "C" fn sound_engine_get_event_buffer_capacity() -> u32 { MAX_EVENTS_PER_TICK as u32 }

#[no_mangle]
pub extern "C" fn sound_engine_get_event_struct_size() -> u32 { std::mem::size_of::<Event>() as u32 }

#[no_mangle]
pub extern "C" fn sound_engine_get_listener_ptr(ctx: *mut Ctx) -> *mut u8 {
  unsafe { &mut (*ctx).listener as *mut _ as *mut u8 }
}

#[no_mangle]
pub extern "C" fn sound_engine_get_listener_size() -> u32 {
  std::mem::size_of::<ListenerPose>() as u32
}

/// Allocate (or replace) a sample slot. Returns a pointer the caller writes
/// `channels * len` planar f32s into. After writing, call
/// `sound_engine_finalize_sample` with the same id and the desired xfade.
#[no_mangle]
pub extern "C" fn sound_engine_alloc_sample(
  ctx: *mut Ctx,
  id: u32,
  channels: u32,
  len: u32,
) -> *mut f32 {
  let ctx = unsafe { &mut *ctx };
  // Stop any voices using a previous version of this sample.
  for ix in 0..MAX_VOICES {
    if ctx.voices[ix].active && ctx.voices[ix].sample_id == id {
      ctx.deactivate_voice(ix);
    }
  }
  let total = (channels * len) as usize;
  ctx.samples.insert(
    id,
    Sample {
      channels: channels as u8,
      len,
      data: vec![0.0; total],
      xfade_data: Vec::new(),
    },
  );
  ctx.samples.get_mut(&id).unwrap().data.as_mut_ptr()
}

#[no_mangle]
pub extern "C" fn sound_engine_finalize_sample(ctx: *mut Ctx, id: u32, xfade_threshold: f32) {
  let ctx = unsafe { &mut *ctx };
  if let Some(s) = ctx.samples.get_mut(&id) {
    if xfade_threshold > 0.0 && s.len > 4 {
      s.xfade_data = build_xfade(
        &s.data,
        s.len as usize,
        s.channels as usize,
        xfade_threshold,
      );
    } else {
      s.xfade_data.clear();
    }
  }
}

/// Set the count of valid events in the event buffer. The caller writes events
/// to `sound_engine_get_event_buffer_ptr` first, then calls this, then
/// `sound_engine_process`.
#[no_mangle]
pub extern "C" fn sound_engine_set_event_count(ctx: *mut Ctx, n: u32) {
  unsafe {
    (*ctx).event_count = n.min(MAX_EVENTS_PER_TICK as u32);
  }
}

#[no_mangle]
pub extern "C" fn sound_engine_process(ctx: *mut Ctx) {
  let ctx = unsafe { &mut *ctx };

  // Drain events.
  let n = ctx.event_count as usize;
  ctx.event_count = 0;
  for i in 0..n {
    let ev = ctx.events[i];
    ctx.handle_event(&ev);
  }

  // Clear output.
  ctx.out_l = [0.0; FRAME_SIZE];
  ctx.out_r = [0.0; FRAME_SIZE];

  // Mix voices.
  let listener = ctx.listener;
  let master_gain = ctx.master_gain;
  for ix in 0..MAX_VOICES {
    if !ctx.voices[ix].active {
      continue;
    }
    mix_voice(
      &mut ctx.voices[ix],
      &ctx.samples,
      &listener,
      &mut ctx.out_l,
      &mut ctx.out_r,
    );
  }

  // Master gain.
  if (master_gain - 1.0).abs() > 1e-6 {
    for i in 0..FRAME_SIZE {
      ctx.out_l[i] *= master_gain;
      ctx.out_r[i] *= master_gain;
    }
  }

  // Limiter.
  ctx.limiter.process(&mut ctx.out_l, &mut ctx.out_r);
}

fn mix_voice(
  v: &mut Voice,
  samples: &HashMap<u32, Sample>,
  listener: &ListenerPose,
  out_l: &mut [f32; FRAME_SIZE],
  out_r: &mut [f32; FRAME_SIZE],
) {
  let Some(sample) = samples.get(&v.sample_id) else {
    v.active = false;
    return;
  };
  let len = sample.len as f32;
  if len < 2.0 {
    return;
  }

  let (left_gain, right_gain) = match v.mode {
    VoiceMode::OneshotPan { pan } => {
      let p = clampf(pan, -1.0, 1.0);
      let l = ((1.0 - p) * 0.5).sqrt();
      let r = ((1.0 + p) * 0.5).sqrt();
      (v.gain * l, v.gain * r)
    },
    VoiceMode::SpatialLoop {
      pos,
      ref_dist,
      rolloff,
      cull_threshold,
    } => {
      let lp = listener.pos;
      let rel = [pos[0] - lp[0], pos[1] - lp[1], pos[2] - lp[2]];
      let dist = (rel[0] * rel[0] + rel[1] * rel[1] + rel[2] * rel[2]).sqrt();
      let atten = if rolloff <= 0.0 {
        1.0
      } else {
        1.0 / (dist / ref_dist).max(1.0).powf(rolloff)
      };
      let pan = if dist > 0.0001 {
        let r = listener.right;
        clampf(
          (rel[0] * r[0] + rel[1] * r[1] + rel[2] * r[2]) / dist,
          -1.0,
          1.0,
        )
      } else {
        0.0
      };
      let lg = ((1.0 - pan) * 0.5).sqrt();
      let rg = ((1.0 + pan) * 0.5).sqrt();
      let voice_gain = v.gain * atten;
      let l = voice_gain * lg;
      let r = voice_gain * rg;
      if cull_threshold > 0.0 && l.abs().max(r.abs()) < cull_threshold {
        // Skip mixing, but advance the playhead so the loop stays in phase.
        let span = len - 2.0;
        if span > 0.0 {
          let advance = v.rate * FRAME_SIZE as f32;
          let mut p = v.pos + advance;
          if v.is_loop {
            p -= span * (p / span).floor();
          }
          v.pos = p;
        }
        return;
      }
      (l, r)
    },
  };

  let stereo_source = sample.channels >= 2;
  let use_xfade = v.is_loop && !sample.xfade_data.is_empty();
  let coeffs = v.biquad_coeffs;

  for i in 0..FRAME_SIZE {
    if v.pos >= len - 2.0 {
      if v.is_loop {
        let span = len - 2.0;
        if span > 0.0 {
          v.pos -= span * (v.pos / span).floor();
        } else {
          v.pos = 0.0;
        }
      } else {
        v.active = false;
        return;
      }
    }

    let s_l = sample.read_at(0, v.pos, use_xfade);
    let s_r = if stereo_source {
      sample.read_at(1, v.pos, use_xfade)
    } else {
      s_l
    };

    let (s_l, s_r) = if let Some(c) = coeffs {
      let yl = v.bq_l.process(&c, s_l);
      let yr = v.bq_r.process(&c, s_r);
      (yl, yr)
    } else {
      (s_l, s_r)
    };

    out_l[i] += s_l * left_gain;
    out_r[i] += s_r * right_gain;
    v.pos += v.rate;
  }
}
