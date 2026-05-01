// AudioWorklet processor for the custom sound engine.
//
// Loads a wasm module that owns sample storage, voice mixing, biquad filters,
// crossfade-looping, simple stereo panning + distance attenuation for spatial
// sources, and a final stereo-linked safety limiter.
//
// Communication with the main thread:
//   - postMessage events: setWasmBytes, listenerSab, uploadSample, event,
//     events, shutdown
//   - SharedArrayBuffer for listener pose (continuous state, written every
//     frame from the main thread, read on every process())
//   - postMessage event ring buffer flushed each tick into wasm memory
//
// Event struct layout in wasm memory (64 bytes, repr(C)):
//   u32 kind
//   u32 handle
//   u32 sample_id
//   u32 flags
//   f32[12] params

const FRAME_SIZE = 128;
const EVENT_SIZE_U32 = 16; // 4 header u32 + 12 params f32 = 16 * 4 bytes = 64 bytes
const LISTENER_SIZE_F32 = 10;

class SoundEngineAWP extends AudioWorkletProcessor {
  constructor() {
    super();

    this.isShutdown = false;
    this.paused = false;
    this.wasm = null;
    this.ctxPtr = 0;
    this.outLPtr = 0;
    this.outRPtr = 0;
    this.eventBufPtr = 0;
    this.eventBufCapacity = 0;
    this.listenerPtr = 0;
    this.eventStructSize = 0;

    /** @type {Float32Array | null} */
    this.listenerSabView = null;

    this.pendingEvents = [];
    /** @type {Array<{type:string, [k:string]: any}>} */
    this.pendingMessages = [];

    this.port.onmessage = evt => this.handleMessage(evt.data);
  }

  // -- wasm memory accessors -------------------------------------------------

  f32mem() {
    if (this.f32 == null || this.f32.buffer !== this.wasm.exports.memory.buffer) {
      this.f32 = new Float32Array(this.wasm.exports.memory.buffer);
      this.u32 = new Uint32Array(this.wasm.exports.memory.buffer);
    }
    return this.f32;
  }

  u32mem() {
    this.f32mem();
    return this.u32;
  }

  // -- message dispatch ------------------------------------------------------

  handleMessage(data) {
    if (!this.ctxPtr && data.type !== 'setWasmBytes') {
      this.pendingMessages.push(data);
      return;
    }

    switch (data.type) {
      case 'setWasmBytes':
        this.initWasm(data.wasmBytes);
        break;
      case 'listenerSab':
        this.listenerSabView = new Float32Array(data.sab);
        break;
      case 'uploadSample':
        this.uploadSample(data.id, data.channels, data.data, data.xfadeThreshold ?? 0);
        break;
      case 'freeSample':
        this.queueEvent({ kind: 6, sampleId: data.id });
        break;
      case 'event':
        this.queueEvent(data.event);
        break;
      case 'events':
        for (let i = 0; i < data.events.length; i++) this.queueEvent(data.events[i]);
        break;
      case 'setPaused':
        this.paused = !!data.paused;
        break;
      case 'shutdown':
        this.isShutdown = true;
        break;
      default:
        console.error('Unhandled SoundEngineAWP message:', data.type);
    }
  }

  async initWasm(wasmBytes) {
    try {
      const compiled = await WebAssembly.compile(wasmBytes);
      this.wasm = await WebAssembly.instantiate(compiled, { env: {} });
    } catch (err) {
      console.error('[AWP] wasm compile/instantiate failed:', err);
      this.port.postMessage({ type: 'error', stage: 'instantiate', err: String(err) });
      return;
    }

    this.ctxPtr = this.wasm.exports.sound_engine_init(sampleRate);
    this.outLPtr = this.wasm.exports.sound_engine_get_output_ptr(this.ctxPtr);
    this.outRPtr = this.wasm.exports.sound_engine_get_output_r_ptr(this.ctxPtr);
    this.eventBufPtr = this.wasm.exports.sound_engine_get_event_buffer_ptr(this.ctxPtr);
    this.eventBufCapacity = this.wasm.exports.sound_engine_get_event_buffer_capacity();
    this.eventStructSize = this.wasm.exports.sound_engine_get_event_struct_size();
    this.listenerPtr = this.wasm.exports.sound_engine_get_listener_ptr(this.ctxPtr);

    if (this.eventStructSize !== EVENT_SIZE_U32 * 4) {
      console.error('[AWP] event struct size mismatch', this.eventStructSize);
    }

    this.port.postMessage({ type: 'ready' });

    const pending = this.pendingMessages;
    this.pendingMessages = [];
    for (let i = 0; i < pending.length; i++) this.handleMessage(pending[i]);
  }

  // -- sample upload ---------------------------------------------------------

  uploadSample(id, channels, data, xfadeThreshold) {
    if (!this.ctxPtr) return;
    const lenPerChannel = (data.length / channels) | 0;
    const ptr = this.wasm.exports.sound_engine_alloc_sample(this.ctxPtr, id, channels, lenPerChannel);
    const f32 = this.f32mem();
    const offset = ptr / Float32Array.BYTES_PER_ELEMENT;
    f32.set(data, offset);
    this.wasm.exports.sound_engine_finalize_sample(this.ctxPtr, id, xfadeThreshold);
  }

  // -- event queueing --------------------------------------------------------

  queueEvent(ev) {
    if (this.pendingEvents.length >= this.eventBufCapacity) {
      // Drop oldest if we'd overflow before next process(). Shouldn't happen
      // in practice (~64 events per ~3ms @ 44.1kHz/128).
      this.pendingEvents.shift();
    }
    this.pendingEvents.push(ev);
  }

  flushEvents() {
    const n = this.pendingEvents.length;
    if (n === 0) {
      this.wasm.exports.sound_engine_set_event_count(this.ctxPtr, 0);
      return;
    }
    const u32 = this.u32mem();
    const f32 = this.f32mem();
    const baseU32 = this.eventBufPtr / Uint32Array.BYTES_PER_ELEMENT;
    const baseF32 = this.eventBufPtr / Float32Array.BYTES_PER_ELEMENT;

    for (let i = 0; i < n; i++) {
      const ev = this.pendingEvents[i];
      const o = i * EVENT_SIZE_U32;
      u32[baseU32 + o + 0] = ev.kind | 0;
      u32[baseU32 + o + 1] = (ev.handle ?? 0) | 0;
      u32[baseU32 + o + 2] = (ev.sampleId ?? 0) | 0;
      u32[baseU32 + o + 3] = (ev.flags ?? 0) | 0;
      const params = ev.params;
      const pBase = baseF32 + o + 4;
      if (params) {
        for (let j = 0; j < 12; j++) f32[pBase + j] = params[j] ?? 0;
      } else {
        for (let j = 0; j < 12; j++) f32[pBase + j] = 0;
      }
    }
    this.pendingEvents.length = 0;
    this.wasm.exports.sound_engine_set_event_count(this.ctxPtr, n);
  }

  // -- listener sync ---------------------------------------------------------

  syncListener() {
    if (!this.listenerSabView) return;
    const f32 = this.f32mem();
    const offset = this.listenerPtr / Float32Array.BYTES_PER_ELEMENT;
    // Direct copy: layout matches wasm-side ListenerPose exactly.
    for (let i = 0; i < LISTENER_SIZE_F32; i++) f32[offset + i] = this.listenerSabView[i];
  }

  // -- process ---------------------------------------------------------------

  process(_inputs, outputs, _params) {
    if (this.isShutdown) return false;
    if (!this.ctxPtr) return true;

    if (this.paused) {
      // Drain queued events into wasm state (so e.g. master-gain or
      // newly-started voices land at the right point) but do NOT call
      // sound_engine_process — that's how phase is preserved across pause.
      // Voices created during pause sit at pos=0 and start playing on resume.
      this.flushEvents();
      const out = outputs[0];
      if (out && out.length >= 1) {
        out[0].fill(0);
        if (out[1] && out[1] !== out[0]) out[1].fill(0);
      }
      return true;
    }

    this.syncListener();
    this.flushEvents();
    this.wasm.exports.sound_engine_process(this.ctxPtr);

    const f32 = this.f32mem();
    const lOff = this.outLPtr / Float32Array.BYTES_PER_ELEMENT;
    const rOff = this.outRPtr / Float32Array.BYTES_PER_ELEMENT;
    const out = outputs[0];
    if (out && out.length >= 1) {
      const outL = out[0];
      const outR = out[1] ?? out[0];
      outL.set(f32.subarray(lOff, lOff + FRAME_SIZE));
      if (outR !== outL) outR.set(f32.subarray(rOff, rOff + FRAME_SIZE));
    }
    return true;
  }
}

registerProcessor('sound-engine-awp', SoundEngineAWP);
