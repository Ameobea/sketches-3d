#![allow(clippy::missing_safety_doc, private_interfaces, dead_code)]

use std::mem;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SubtickSnapshot {
  // Physics state from C++ packState (10 f32s)
  pos: [f32; 3],
  external_vel: [f32; 3],
  vertical_vel: f32,
  vertical_offset: f32,
  flags: u32, // bit 0: on_ground, bit 1: is_jumping
  floor_user_index: i32,

  // Input state from JS
  walk_dir: [f32; 3], // walkDirection set this subtick
  look_dir: [f32; 2], // phi, theta from camera controller
  key_flags: u32,     // bit 0: W, 1: S, 2: A, 3: D, 4: Space, 5: Shift
}

#[repr(u32)]
#[derive(Clone, Copy)]
enum EventType {
  Jump = 0,
  Dash = 1,
  ZoneEvent = 2,
  Teleport = 3,
  RunStart = 4,
  RunEnd = 5,
  OOBRespawn = 6,
}

#[derive(Clone)]
struct RecorderEvent {
  event_type: u32,
  subtick: u32,
  data: [f32; 4], // up to 4 floats of event-specific data
  data_len: u32,
}

/// Ordered key-value metadata store. Keys are UTF-8 strings, values are raw bytes.
#[derive(Clone, Default)]
struct MetadataMap {
  entries: Vec<(Vec<u8>, Vec<u8>)>,
}

impl MetadataMap {
  fn set(&mut self, key: &[u8], value: Vec<u8>) {
    for entry in &mut self.entries {
      if entry.0 == key {
        entry.1 = value;
        return;
      }
    }
    self.entries.push((key.to_vec(), value));
  }

  fn get(&self, key: &[u8]) -> Option<&[u8]> {
    self
      .entries
      .iter()
      .find(|(k, _)| k == key)
      .map(|(_, v)| v.as_slice())
  }

  fn set_u32(&mut self, key: &[u8], v: u32) {
    self.set(key, v.to_le_bytes().to_vec());
  }

  fn get_u32(&self, key: &[u8]) -> Option<u32> {
    let v = self.get(key)?;
    if v.len() < 4 {
      return None;
    }
    Some(u32::from_le_bytes(v[..4].try_into().ok()?))
  }

  fn set_u64(&mut self, key: &[u8], v: u64) {
    self.set(key, v.to_le_bytes().to_vec());
  }

  fn set_f32(&mut self, key: &[u8], v: f32) {
    self.set(key, v.to_le_bytes().to_vec());
  }

  fn set_f32x3(&mut self, key: &[u8], v: [f32; 3]) {
    let mut buf = Vec::with_capacity(12);
    for f in v {
      buf.extend_from_slice(&f.to_le_bytes());
    }
    self.set(key, buf);
  }

  fn set_string(&mut self, key: &[u8], v: &str) {
    self.set(key, v.as_bytes().to_vec());
  }

  fn get_string(&self, key: &[u8]) -> Option<&str> {
    let v = self.get(key)?;
    std::str::from_utf8(v).ok()
  }

  fn clear(&mut self) {
    self.entries.clear();
  }
}

struct RecorderCtx {
  metadata: MetadataMap,
  snapshots: Vec<SubtickSnapshot>,
  events: Vec<RecorderEvent>,
  serialized: Option<Vec<u8>>,
}

#[no_mangle]
pub extern "C" fn create_recorder() -> *mut RecorderCtx {
  let ctx = Box::new(RecorderCtx {
    metadata: MetadataMap::default(),
    snapshots: Vec::with_capacity(16384),
    events: Vec::with_capacity(256),
    serialized: None,
  });
  Box::into_raw(ctx)
}

#[no_mangle]
pub unsafe extern "C" fn destroy_recorder(ctx: *mut RecorderCtx) {
  if !ctx.is_null() {
    drop(Box::from_raw(ctx));
  }
}

#[no_mangle]
pub extern "C" fn fr_malloc(size: usize) -> *mut u8 {
  let mut v = Vec::<u8>::with_capacity(size);
  let ptr = v.as_mut_ptr();
  mem::forget(v);
  ptr
}

#[no_mangle]
pub unsafe extern "C" fn fr_free(ptr: *mut u8, size: usize) {
  if !ptr.is_null() {
    drop(Vec::from_raw_parts(ptr, 0, size));
  }
}

/// Set the replay header from a flat f32 buffer.
/// Layout (21 floats):
///   [0]: tick_rate_hz (as f32, cast to u32)
///   [1]: gravity
///   [2]: jump_speed
///   [3]: move_speed_ground
///   [4]: move_speed_air
///   [5]: collider_height
///   [6]: collider_radius
///   [7..10]: ext_vel_air_damping[3]
///   [10..13]: ext_vel_ground_damping[3]
///   [13]: gravity_shape_rise_mult
///   [14]: gravity_shape_apex_mult
///   [15]: gravity_shape_fall_mult
///   [16]: gravity_shape_apex_threshold
///   [17]: gravity_shape_knee_width
///   [18]: gravity_shape_only_jumps (0.0 or 1.0)
///   [19]: map_id_hash low bits (f32 bitcast from u32)
///   [20]: map_id_hash high bits (f32 bitcast from u32)
#[no_mangle]
pub unsafe extern "C" fn set_header(ctx: *mut RecorderCtx, header_ptr: *const f32, len: u32) {
  let ctx = &mut *ctx;
  let data = std::slice::from_raw_parts(header_ptr, len as usize);
  let md = &mut ctx.metadata;

  md.set_u32(b"tick_rate_hz", data[0] as u32);
  md.set_f32(b"gravity", data[1]);
  md.set_f32(b"jump_speed", data[2]);
  md.set_f32(b"move_speed_ground", data[3]);
  md.set_f32(b"move_speed_air", data[4]);
  md.set_f32(b"collider_height", data[5]);
  md.set_f32(b"collider_radius", data[6]);
  md.set_f32x3(b"ext_vel_air_damp", [data[7], data[8], data[9]]);
  md.set_f32x3(b"ext_vel_gnd_damp", [data[10], data[11], data[12]]);
  md.set_f32(b"grav_rise_mult", data[13]);
  md.set_f32(b"grav_apex_mult", data[14]);
  md.set_f32(b"grav_fall_mult", data[15]);
  md.set_f32(b"grav_apex_thresh", data[16]);
  md.set_f32(b"grav_knee_width", data[17]);
  md.set_u32(b"grav_only_jumps", if data[18] > 0.5 { 1 } else { 0 });

  if len >= 21 {
    let lo = data[19].to_bits();
    let hi = data[20].to_bits();
    let hash = (hi as u64) << 32 | lo as u64;
    md.set_u64(b"map_id_hash", hash);
  }
}

/// Set a metadata string entry from JS.
#[no_mangle]
pub unsafe extern "C" fn set_metadata_string(
  ctx: *mut RecorderCtx,
  key_ptr: *const u8,
  key_len: u32,
  val_ptr: *const u8,
  val_len: u32,
) {
  let ctx = &mut *ctx;
  ctx.serialized = None;
  let key = std::slice::from_raw_parts(key_ptr, key_len as usize);
  let val = std::slice::from_raw_parts(val_ptr, val_len as usize);
  ctx.metadata.set(key, val.to_vec());
}

/// Record a single subtick snapshot.
///
/// physics_state_ptr: 10 floats from C++ packState
///   [0..3]: pos, [3..6]: external_vel, [6]: vertical_vel, [7]: vertical_offset,
///   [8]: flags (bitcast u32), [9]: floor_user_index (bitcast i32)
///
/// input_state_ptr: 5 floats
///   [0..3]: walk_dir, [3]: phi, [4]: theta
///
/// key_flags: packed key bitmask
#[no_mangle]
pub unsafe extern "C" fn record_subtick(
  ctx: *mut RecorderCtx,
  physics_state_ptr: *const f32,
  input_state_ptr: *const f32,
  key_flags: u32,
) {
  let ctx = &mut *ctx;
  let ps = std::slice::from_raw_parts(physics_state_ptr, 10);
  let is = std::slice::from_raw_parts(input_state_ptr, 5);

  ctx.serialized = None; // invalidate cached serialization

  let snapshot = SubtickSnapshot {
    pos: [ps[0], ps[1], ps[2]],
    external_vel: [ps[3], ps[4], ps[5]],
    vertical_vel: ps[6],
    vertical_offset: ps[7],
    flags: ps[8].to_bits(),
    floor_user_index: ps[9].to_bits() as i32,
    walk_dir: [is[0], is[1], is[2]],
    look_dir: [is[3], is[4]],
    key_flags,
  };
  ctx.snapshots.push(snapshot);
}

#[no_mangle]
pub unsafe extern "C" fn record_event(
  ctx: *mut RecorderCtx,
  event_type: u32,
  data_ptr: *const f32,
  data_len: u32,
) {
  let ctx = &mut *ctx;
  ctx.serialized = None;

  let mut data = [0f32; 4];
  let len = (data_len as usize).min(4);
  if !data_ptr.is_null() && len > 0 {
    let src = std::slice::from_raw_parts(data_ptr, len);
    data[..len].copy_from_slice(src);
  }

  ctx.events.push(RecorderEvent {
    event_type,
    subtick: ctx.snapshots.len() as u32,
    data,
    data_len: len as u32,
  });
}

// ─── Serialization ────────────────────────────────────────────────────────

fn write_u32(buf: &mut Vec<u8>, v: u32) {
  buf.extend_from_slice(&v.to_le_bytes());
}

fn write_f32(buf: &mut Vec<u8>, v: f32) {
  buf.extend_from_slice(&v.to_le_bytes());
}

/// Extract a single f32 channel from all snapshots and compress it with pcodec.
fn compress_f32_channel(values: &[f32]) -> Vec<u8> {
  pco::standalone::simple_compress(values, &pco::ChunkConfig::default()).unwrap()
}

/// Extract a single u32 channel from all snapshots and compress it with pcodec.
fn compress_u32_channel(values: &[u32]) -> Vec<u8> {
  pco::standalone::simple_compress(values, &pco::ChunkConfig::default()).unwrap()
}

/// Extract a single i32 channel from all snapshots and compress it with pcodec.
fn compress_i32_channel(values: &[i32]) -> Vec<u8> {
  pco::standalone::simple_compress(values, &pco::ChunkConfig::default()).unwrap()
}

fn write_compressed_block(buf: &mut Vec<u8>, data: &[u8]) {
  write_u32(buf, data.len() as u32);
  buf.extend_from_slice(data);
}

/// Serialize metadata section into buffer.
/// Layout:
///   u32: metadata_section_byte_length (everything after this u32 up to num_snapshots)
///   u32: num_entries
///   per entry:
///     u8:  key_len
///     [u8]: key (UTF-8)
///     u32: value_len
///     [u8]: value (raw LE bytes)
fn write_metadata(buf: &mut Vec<u8>, md: &MetadataMap) {
  // Reserve space for section byte length
  let len_pos = buf.len();
  write_u32(buf, 0); // placeholder

  write_u32(buf, md.entries.len() as u32);

  for (key, value) in &md.entries {
    buf.push(key.len() as u8);
    buf.extend_from_slice(key);
    write_u32(buf, value.len() as u32);
    buf.extend_from_slice(value);
  }

  // Patch section byte length (does not include the length field itself)
  let section_len = (buf.len() - len_pos - 4) as u32;
  buf[len_pos..len_pos + 4].copy_from_slice(&section_len.to_le_bytes());
}

fn do_serialize(ctx: &RecorderCtx) -> Vec<u8> {
  let mut buf = Vec::with_capacity(4096);

  // Magic
  buf.extend_from_slice(b"FREC");
  // Version
  write_u32(&mut buf, 1);

  // Metadata section
  write_metadata(&mut buf, &ctx.metadata);

  // Counts
  let num_snapshots = ctx.snapshots.len();
  let num_events = ctx.events.len();
  write_u32(&mut buf, num_snapshots as u32);
  write_u32(&mut buf, num_events as u32);

  // Columnar snapshot data - extract each channel
  if num_snapshots > 0 {
    // pos_x, pos_y, pos_z
    for axis in 0..3 {
      let col: Vec<f32> = ctx.snapshots.iter().map(|s| s.pos[axis]).collect();
      write_compressed_block(&mut buf, &compress_f32_channel(&col));
    }
    // external_vel x, y, z
    for axis in 0..3 {
      let col: Vec<f32> = ctx.snapshots.iter().map(|s| s.external_vel[axis]).collect();
      write_compressed_block(&mut buf, &compress_f32_channel(&col));
    }
    // vertical_vel
    {
      let col: Vec<f32> = ctx.snapshots.iter().map(|s| s.vertical_vel).collect();
      write_compressed_block(&mut buf, &compress_f32_channel(&col));
    }
    // vertical_offset
    {
      let col: Vec<f32> = ctx.snapshots.iter().map(|s| s.vertical_offset).collect();
      write_compressed_block(&mut buf, &compress_f32_channel(&col));
    }
    // flags
    {
      let col: Vec<u32> = ctx.snapshots.iter().map(|s| s.flags).collect();
      write_compressed_block(&mut buf, &compress_u32_channel(&col));
    }
    // floor_user_index
    {
      let col: Vec<i32> = ctx.snapshots.iter().map(|s| s.floor_user_index).collect();
      write_compressed_block(&mut buf, &compress_i32_channel(&col));
    }
    // walk_dir x, y, z
    for axis in 0..3 {
      let col: Vec<f32> = ctx.snapshots.iter().map(|s| s.walk_dir[axis]).collect();
      write_compressed_block(&mut buf, &compress_f32_channel(&col));
    }
    // look_dir phi, theta
    for axis in 0..2 {
      let col: Vec<f32> = ctx.snapshots.iter().map(|s| s.look_dir[axis]).collect();
      write_compressed_block(&mut buf, &compress_f32_channel(&col));
    }
    // key_flags
    {
      let col: Vec<u32> = ctx.snapshots.iter().map(|s| s.key_flags).collect();
      write_compressed_block(&mut buf, &compress_u32_channel(&col));
    }
  }

  // Event data
  if num_events > 0 {
    let event_types: Vec<u32> = ctx.events.iter().map(|e| e.event_type).collect();
    write_compressed_block(&mut buf, &compress_u32_channel(&event_types));
    let subticks: Vec<u32> = ctx.events.iter().map(|e| e.subtick).collect();
    write_compressed_block(&mut buf, &compress_u32_channel(&subticks));
    let data_lens: Vec<u32> = ctx.events.iter().map(|e| e.data_len).collect();
    write_compressed_block(&mut buf, &compress_u32_channel(&data_lens));
    // Flatten event data
    for i in 0..4 {
      let col: Vec<f32> = ctx.events.iter().map(|e| e.data[i]).collect();
      write_compressed_block(&mut buf, &compress_f32_channel(&col));
    }
  }

  buf
}

#[no_mangle]
pub unsafe extern "C" fn serialize(ctx: *mut RecorderCtx) -> *mut u8 {
  let ctx = &mut *ctx;
  if ctx.serialized.is_none() {
    ctx.serialized = Some(do_serialize(ctx));
  }
  ctx.serialized.as_mut().unwrap().as_mut_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn get_serialized_len(ctx: *mut RecorderCtx) -> u32 {
  let ctx = &*ctx;
  ctx.serialized.as_ref().map_or(0, |v| v.len() as u32)
}

#[no_mangle]
pub unsafe extern "C" fn reset_recorder(ctx: *mut RecorderCtx) {
  let ctx = &mut *ctx;
  ctx.snapshots.clear();
  ctx.events.clear();
  ctx.metadata.clear();
  ctx.serialized = None;
}

#[no_mangle]
pub unsafe extern "C" fn get_subtick_count(ctx: *mut RecorderCtx) -> u32 {
  let ctx = &*ctx;
  ctx.snapshots.len() as u32
}

// ─── Player (deserialization / playback) ──────────────────────────────────

struct PlayerCtx {
  metadata: MetadataMap,
  snapshots: Vec<SubtickSnapshot>,
  events: Vec<RecorderEvent>,
}

#[no_mangle]
pub extern "C" fn create_player() -> *mut PlayerCtx {
  let ctx = Box::new(PlayerCtx {
    metadata: MetadataMap::default(),
    snapshots: Vec::new(),
    events: Vec::new(),
  });
  Box::into_raw(ctx)
}

#[no_mangle]
pub unsafe extern "C" fn destroy_player(ctx: *mut PlayerCtx) {
  if !ctx.is_null() {
    drop(Box::from_raw(ctx));
  }
}

fn read_u32(data: &[u8], offset: &mut usize) -> Option<u32> {
  if *offset + 4 > data.len() {
    return None;
  }
  let v = u32::from_le_bytes(data[*offset..*offset + 4].try_into().ok()?);
  *offset += 4;
  Some(v)
}

fn read_u8(data: &[u8], offset: &mut usize) -> Option<u8> {
  if *offset >= data.len() {
    return None;
  }
  let v = data[*offset];
  *offset += 1;
  Some(v)
}

fn read_bytes<'a>(data: &'a [u8], offset: &mut usize, len: usize) -> Option<&'a [u8]> {
  if *offset + len > data.len() {
    return None;
  }
  let slice = &data[*offset..*offset + len];
  *offset += len;
  Some(slice)
}

fn read_f32(data: &[u8], offset: &mut usize) -> Option<f32> {
  if *offset + 4 > data.len() {
    return None;
  }
  let v = f32::from_le_bytes(data[*offset..*offset + 4].try_into().ok()?);
  *offset += 4;
  Some(v)
}

fn read_compressed_f32_block(data: &[u8], offset: &mut usize) -> Option<Vec<f32>> {
  let block_len = read_u32(data, offset)? as usize;
  if *offset + block_len > data.len() {
    return None;
  }
  let block = &data[*offset..*offset + block_len];
  *offset += block_len;
  pco::standalone::simple_decompress::<f32>(block).ok()
}

fn read_compressed_u32_block(data: &[u8], offset: &mut usize) -> Option<Vec<u32>> {
  let block_len = read_u32(data, offset)? as usize;
  if *offset + block_len > data.len() {
    return None;
  }
  let block = &data[*offset..*offset + block_len];
  *offset += block_len;
  pco::standalone::simple_decompress::<u32>(block).ok()
}

fn read_compressed_i32_block(data: &[u8], offset: &mut usize) -> Option<Vec<i32>> {
  let block_len = read_u32(data, offset)? as usize;
  if *offset + block_len > data.len() {
    return None;
  }
  let block = &data[*offset..*offset + block_len];
  *offset += block_len;
  pco::standalone::simple_decompress::<i32>(block).ok()
}

/// Read metadata section from buffer.
fn read_metadata(data: &[u8], offset: &mut usize) -> Option<MetadataMap> {
  let section_len = read_u32(data, offset)? as usize;
  let section_end = *offset + section_len;
  if section_end > data.len() {
    return None;
  }

  let num_entries = read_u32(data, offset)? as usize;
  let mut md = MetadataMap {
    entries: Vec::with_capacity(num_entries),
  };

  for _ in 0..num_entries {
    let key_len = read_u8(data, offset)? as usize;
    let key = read_bytes(data, offset, key_len)?.to_vec();
    let val_len = read_u32(data, offset)? as usize;
    let val = read_bytes(data, offset, val_len)?.to_vec();
    md.entries.push((key, val));
  }

  // Ensure we consumed exactly the section
  if *offset != section_end {
    return None;
  }

  Some(md)
}

fn do_deserialize(data: &[u8]) -> Option<PlayerCtx> {
  // Magic
  if data.len() < 4 || &data[0..4] != b"FREC" {
    return None;
  }
  let mut offset = 4;

  // Version
  let _version = read_u32(data, &mut offset)?;

  // Metadata section
  let metadata = read_metadata(data, &mut offset)?;

  // Counts
  let num_snapshots = read_u32(data, &mut offset)? as usize;
  let num_events = read_u32(data, &mut offset)? as usize;

  // Decompress snapshot channels
  let mut snapshots = Vec::with_capacity(num_snapshots);
  if num_snapshots > 0 {
    let pos_x = read_compressed_f32_block(data, &mut offset)?;
    let pos_y = read_compressed_f32_block(data, &mut offset)?;
    let pos_z = read_compressed_f32_block(data, &mut offset)?;

    let ext_vel_x = read_compressed_f32_block(data, &mut offset)?;
    let ext_vel_y = read_compressed_f32_block(data, &mut offset)?;
    let ext_vel_z = read_compressed_f32_block(data, &mut offset)?;
    let vertical_vel = read_compressed_f32_block(data, &mut offset)?;
    let vertical_offset = read_compressed_f32_block(data, &mut offset)?;
    let flags = read_compressed_u32_block(data, &mut offset)?;
    let floor_user_index = read_compressed_i32_block(data, &mut offset)?;
    let walk_dir_x = read_compressed_f32_block(data, &mut offset)?;
    let walk_dir_y = read_compressed_f32_block(data, &mut offset)?;
    let walk_dir_z = read_compressed_f32_block(data, &mut offset)?;
    let look_dir_phi = read_compressed_f32_block(data, &mut offset)?;
    let look_dir_theta = read_compressed_f32_block(data, &mut offset)?;
    let key_flags = read_compressed_u32_block(data, &mut offset)?;

    for i in 0..num_snapshots {
      snapshots.push(SubtickSnapshot {
        pos: [pos_x[i], pos_y[i], pos_z[i]],
        external_vel: [ext_vel_x[i], ext_vel_y[i], ext_vel_z[i]],
        vertical_vel: vertical_vel[i],
        vertical_offset: vertical_offset[i],
        flags: flags[i],
        floor_user_index: floor_user_index[i],
        walk_dir: [walk_dir_x[i], walk_dir_y[i], walk_dir_z[i]],
        look_dir: [look_dir_phi[i], look_dir_theta[i]],
        key_flags: key_flags[i],
      });
    }
  }

  // Decompress events
  let mut events = Vec::with_capacity(num_events);
  if num_events > 0 {
    let event_types = read_compressed_u32_block(data, &mut offset)?;
    let subticks = read_compressed_u32_block(data, &mut offset)?;
    let data_lens = read_compressed_u32_block(data, &mut offset)?;
    let mut event_data = [[0f32; 0]; 4].map(|_| Vec::new());
    for i in 0..4 {
      event_data[i] = read_compressed_f32_block(data, &mut offset)?;
    }

    for i in 0..num_events {
      events.push(RecorderEvent {
        event_type: event_types[i],
        subtick: subticks[i],
        data: [
          event_data[0][i],
          event_data[1][i],
          event_data[2][i],
          event_data[3][i],
        ],
        data_len: data_lens[i],
      });
    }
  }

  Some(PlayerCtx {
    metadata,
    snapshots,
    events,
  })
}

/// Load a compressed replay blob into the player context.
/// Returns 0 on success, 1 on error.
#[no_mangle]
pub unsafe extern "C" fn player_load(ctx: *mut PlayerCtx, data_ptr: *const u8, len: u32) -> u32 {
  let ctx = &mut *ctx;
  let data = std::slice::from_raw_parts(data_ptr, len as usize);
  match do_deserialize(data) {
    Some(loaded) => {
      ctx.metadata = loaded.metadata;
      ctx.snapshots = loaded.snapshots;
      ctx.events = loaded.events;
      0
    }
    None => 1,
  }
}

/// Write header data to the output buffer (21 f32s, same layout as set_header input).
/// Reads from metadata map entries.
#[no_mangle]
pub unsafe extern "C" fn player_get_header(ctx: *mut PlayerCtx, out_ptr: *mut f32) {
  let ctx = &*ctx;
  let out = std::slice::from_raw_parts_mut(out_ptr, 21);
  let md = &ctx.metadata;

  let get_f32 = |key: &[u8]| -> f32 {
    md.get(key)
      .and_then(|v| {
        if v.len() >= 4 {
          Some(f32::from_le_bytes(v[..4].try_into().unwrap()))
        } else {
          None
        }
      })
      .unwrap_or(0.0)
  };
  let get_u32 = |key: &[u8]| -> u32 { md.get_u32(key).unwrap_or(0) };
  let get_f32x3 = |key: &[u8]| -> [f32; 3] {
    md.get(key)
      .and_then(|v| {
        if v.len() >= 12 {
          Some([
            f32::from_le_bytes(v[0..4].try_into().unwrap()),
            f32::from_le_bytes(v[4..8].try_into().unwrap()),
            f32::from_le_bytes(v[8..12].try_into().unwrap()),
          ])
        } else {
          None
        }
      })
      .unwrap_or([0.0; 3])
  };
  let get_u64 = |key: &[u8]| -> u64 {
    md.get(key)
      .and_then(|v| {
        if v.len() >= 8 {
          Some(u64::from_le_bytes(v[..8].try_into().unwrap()))
        } else {
          None
        }
      })
      .unwrap_or(0)
  };

  out[0] = get_u32(b"tick_rate_hz") as f32;
  out[1] = get_f32(b"gravity");
  out[2] = get_f32(b"jump_speed");
  out[3] = get_f32(b"move_speed_ground");
  out[4] = get_f32(b"move_speed_air");
  out[5] = get_f32(b"collider_height");
  out[6] = get_f32(b"collider_radius");
  let air = get_f32x3(b"ext_vel_air_damp");
  out[7] = air[0];
  out[8] = air[1];
  out[9] = air[2];
  let gnd = get_f32x3(b"ext_vel_gnd_damp");
  out[10] = gnd[0];
  out[11] = gnd[1];
  out[12] = gnd[2];
  out[13] = get_f32(b"grav_rise_mult");
  out[14] = get_f32(b"grav_apex_mult");
  out[15] = get_f32(b"grav_fall_mult");
  out[16] = get_f32(b"grav_apex_thresh");
  out[17] = get_f32(b"grav_knee_width");
  out[18] = if get_u32(b"grav_only_jumps") != 0 {
    1.0
  } else {
    0.0
  };
  let hash = get_u64(b"map_id_hash");
  let lo = (hash & 0xffffffff) as u32;
  let hi = ((hash >> 32) & 0xffffffff) as u32;
  out[19] = f32::from_bits(lo);
  out[20] = f32::from_bits(hi);
}

/// Get a metadata value by key. Writes value bytes into out_ptr (up to out_capacity).
/// Returns the actual value length, or -1 if not found.
#[no_mangle]
pub unsafe extern "C" fn player_get_metadata(
  ctx: *mut PlayerCtx,
  key_ptr: *const u8,
  key_len: u32,
  out_ptr: *mut u8,
  out_capacity: u32,
) -> i32 {
  let ctx = &*ctx;
  let key = std::slice::from_raw_parts(key_ptr, key_len as usize);
  match ctx.metadata.get(key) {
    Some(val) => {
      let copy_len = (val.len() as u32).min(out_capacity) as usize;
      if copy_len > 0 && !out_ptr.is_null() {
        std::ptr::copy_nonoverlapping(val.as_ptr(), out_ptr, copy_len);
      }
      val.len() as i32
    }
    None => -1,
  }
}

/// Write subtick data for a given index.
/// Layout: 16 f32s - walk_dir[3], look_dir[2], key_flags (bitcast), pos[3],
///   ext_vel[3], vert_vel, vert_offset, flags (bitcast), floor_idx (bitcast)
/// Returns 0 on success, 1 if index out of range.
#[no_mangle]
pub unsafe extern "C" fn player_get_subtick(
  ctx: *mut PlayerCtx,
  index: u32,
  out_ptr: *mut f32,
) -> u32 {
  let ctx = &*ctx;
  let idx = index as usize;
  if idx >= ctx.snapshots.len() {
    return 1;
  }
  let s = &ctx.snapshots[idx];
  let out = std::slice::from_raw_parts_mut(out_ptr, 16);
  out[0] = s.walk_dir[0];
  out[1] = s.walk_dir[1];
  out[2] = s.walk_dir[2];
  out[3] = s.look_dir[0]; // phi
  out[4] = s.look_dir[1]; // theta
  out[5] = f32::from_bits(s.key_flags);
  out[6] = s.pos[0];
  out[7] = s.pos[1];
  out[8] = s.pos[2];
  out[9] = s.external_vel[0];
  out[10] = s.external_vel[1];
  out[11] = s.external_vel[2];
  out[12] = s.vertical_vel;
  out[13] = s.vertical_offset;
  out[14] = f32::from_bits(s.flags);
  out[15] = f32::from_bits(s.floor_user_index as u32);
  0
}

#[no_mangle]
pub unsafe extern "C" fn player_get_subtick_count(ctx: *mut PlayerCtx) -> u32 {
  let ctx = &*ctx;
  ctx.snapshots.len() as u32
}

#[no_mangle]
pub unsafe extern "C" fn player_get_event_count(ctx: *mut PlayerCtx) -> u32 {
  let ctx = &*ctx;
  ctx.events.len() as u32
}

/// Write event data for a given index.
/// Layout: 6 f32s - event_type (u32 bitcast), subtick (u32 bitcast), data[4]
/// Returns 0 on success, 1 if index out of range.
#[no_mangle]
pub unsafe extern "C" fn player_get_event(
  ctx: *mut PlayerCtx,
  index: u32,
  out_ptr: *mut f32,
) -> u32 {
  let ctx = &*ctx;
  let idx = index as usize;
  if idx >= ctx.events.len() {
    return 1;
  }
  let e = &ctx.events[idx];
  let out = std::slice::from_raw_parts_mut(out_ptr, 6);
  out[0] = f32::from_bits(e.event_type);
  out[1] = f32::from_bits(e.subtick);
  out[2] = e.data[0];
  out[3] = e.data[1];
  out[4] = e.data[2];
  out[5] = e.data[3];
  0
}
