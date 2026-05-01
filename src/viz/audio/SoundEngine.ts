import * as THREE from 'three';
import type { Readable } from 'svelte/store';

import type { VizConfig } from '../conf';
import { MaterialClass } from '../shaders/customShader';
import soundEngineWasmURL from '../wasmComp/sound_engine.wasm?url';
import soundEngineAWPURL from './SoundEngineAWP.js?url';

// Shared with the AWP / wasm. Kept in sync with src/viz/wasm/sound_engine/src/lib.rs.
const EV_PLAY_ONESHOT = 1;
const EV_START_SPATIAL_LOOP = 2;
const EV_UPDATE_SPATIAL_LOOP = 3;
const EV_STOP_VOICE = 4;
const EV_SET_MASTER_GAIN = 5;

const FILTER_NONE = 0;
const FILTER_LP = 1;
const FILTER_HP = 2;
const FILTER_BP = 3;
const FILTER_NOTCH = 4;

export type FilterType = 'lp' | 'hp' | 'bp' | 'notch';

const filterTypeCode = (t: FilterType | undefined): number => {
  switch (t) {
    case 'lp':
      return FILTER_LP;
    case 'hp':
      return FILTER_HP;
    case 'bp':
      return FILTER_BP;
    case 'notch':
      return FILTER_NOTCH;
    default:
      return FILTER_NONE;
  }
};

const LISTENER_F32_COUNT = 10;

// -- Sample defs ---------------------------------------------------------------
//
// `BUILTIN_SFX_DEFS` is the registry of always-available core SFX (used
// across many scenes). Per-scene one-offs should be added at runtime via
// `registerSfxDefs` — this avoids polluting the global registry with
// scene-specific entries.

export interface SfxDef {
  url: string;
  playbackRate?: number;
}

const BUILTIN_SFX_DEFS: Record<string, SfxDef> = {
  dash: { url: 'https://i.ameo.link/cta.ogg' },
  dash_pickup: { url: 'https://i.ameo.link/ctb.ogg', playbackRate: 1.4 },
  land_default: { url: 'https://i.ameo.link/cvk.ogg' },
  player_die: { url: 'https://i.ameo.link/cx8.ogg' },
  metal_plate_land_0: { url: 'https://i.ameo.link/dmi.ogg' },
  metal_plate_land_1: { url: 'https://i.ameo.link/dmj.ogg' },
  jump_pad_trigger: { url: 'https://i.ameo.link/dml.ogg' },
};

const METAL_PLATE_LAND_SFX = ['metal_plate_land_0', 'metal_plate_land_1'] as const;

// -- Config (preserved from old SfxManager) -------------------------------------

export interface SfxWalkConfig {
  playWalkSound?: (mat: MaterialClass) => void;
  timeBetweenStepsSeconds: number;
  timeBetweenStepsJitterSeconds: number;
}

export interface SfxLandConfig {
  materialLandSounds: Partial<Record<MaterialClass, (() => void) | string[]>>;
}

export interface SfxConfig {
  walk: SfxWalkConfig;
  land?: Partial<SfxLandConfig>;
  neededSfx?: string[];
}

export const buildDefaultSfxConfig = (): SfxConfig => ({
  walk: {
    timeBetweenStepsSeconds: 0.35,
    timeBetweenStepsJitterSeconds: 0.04,
  },
});

// -- Public types --------------------------------------------------------------

export interface PlaySfxOpts {
  gain?: number; // linear, default 1
  playbackRate?: number; // default 1 (or sfx-def override)
  pan?: number; // -1..1, default 0
}

export interface SpatialLoopOpts {
  pos: [number, number, number];
  gain?: number; // linear, default 1
  playbackRate?: number; // default 1
  /** 0..1 fraction of the sample for crossfade ramp on each end. Default 0.1. */
  xfade?: number;
  filter?: { type: FilterType; freq: number; q?: number };
  /** Distance at which attenuation begins. Default 1. */
  refDistance?: number;
  /** Attenuation curve exponent: gain = 1 / max(1, dist/ref)^rolloff. Default 1. */
  rolloff?: number;
  /**
   * If the per-channel post-attenuation gain falls below this linear threshold,
   * mixing is skipped (playhead still advances). 0 disables. Default 0.001
   * (~-60 dB).
   */
  cullThreshold?: number;
}

export interface SpatialVoiceHandle {
  setPosition(x: number, y: number, z: number): void;
  setGain(g: number): void;
  stop(): void;
}

interface PendingSpatialLoop {
  name: string;
  opts: SpatialLoopOpts;
  handle: number;
}

// -- Implementation ------------------------------------------------------------

export interface SoundEngineOpts {
  config?: SfxConfig;
  /**
   * When false, the engine is constructed in a no-op state: no `AudioContext`,
   * no `SharedArrayBuffer`, no AudioWorklet, no wasm fetch, no sample fetches.
   * All public methods become no-ops.  Used for scenes that have no audio at
   * all (e.g. the geoscript playground), avoiding the COEP/SAB requirements
   * and unnecessary network traffic.
   */
  enabled?: boolean;
}

export class SoundEngine {
  private readonly enabled: boolean;
  private vizConfig: Readable<VizConfig> | null = null;
  private config: SfxConfig;
  private ctx: AudioContext | null = null;
  private node: AudioWorkletNode | null = null;
  private nodeReady = false;

  /** Runtime registry: builtins seeded at ctor time, extended by scenes via
   * `registerSfxDefs`. */
  private sfxDefs: Record<string, SfxDef> = { ...BUILTIN_SFX_DEFS };
  private samplesByName = new Map<string, number>();
  private requestedSamples = new Set<string>();
  private nextSampleId = 1;
  private nextHandle = 1;

  /** SFX queued before the wasm finished loading. */
  private pendingPlaySfx: { name: string; opts?: PlaySfxOpts }[] = [];
  private pendingSpatial: PendingSpatialLoop[] = [];

  private listenerSab: SharedArrayBuffer | null = null;
  private listenerView: Float32Array | null = null;

  private camera: THREE.Camera | null = null;
  private cameraDir = new THREE.Vector3();
  private cameraRight = new THREE.Vector3();

  private masterGain = 1;
  private vizConfigUnsubscribe: (() => void) | null = null;

  // Walk timing (carried over from old SfxManager).
  private curWalkMatClass: MaterialClass | null = null;
  private timeSinceLastStepSound = 0;
  private nextStepSoundTime = 0;

  private get isWalking(): boolean {
    return this.curWalkMatClass !== null;
  }

  constructor(opts?: SoundEngineOpts) {
    this.enabled = opts?.enabled ?? true;
    this.config = opts?.config ?? buildDefaultSfxConfig();
    this.nextStepSoundTime = this.getNextStepSoundTime();

    if (!this.enabled) return;

    this.ctx = new AudioContext();
    this.listenerSab = new SharedArrayBuffer(LISTENER_F32_COUNT * Float32Array.BYTES_PER_ELEMENT);
    this.listenerView = new Float32Array(this.listenerSab);
    // Sensible default forward / right so a sound at the origin doesn't pan
    // randomly before the first tick.
    this.listenerView[5] = -1; // forward = -Z
    this.listenerView[6] = 1; // right = +X

    const ctx = this.ctx;
    const tryResume = () =>
      ctx
        .resume()
        .then(() => {
          window.removeEventListener('pointerdown', tryResume);
          window.removeEventListener('keydown', tryResume);
        })
        .catch(err => console.warn('[SoundEngine] ctx.resume failed:', err));

    window.addEventListener('pointerdown', tryResume);
    window.addEventListener('keydown', tryResume);
    tryResume();

    void this.init().catch(err => console.error('[SoundEngine] init failed:', err));
  }

  private async init() {
    const ctx = this.ctx!;
    const [, wasmBytes] = await Promise.all([
      ctx.audioWorklet.addModule(soundEngineAWPURL),
      fetch(soundEngineWasmURL).then(r => r.arrayBuffer()),
    ]);

    this.node = new AudioWorkletNode(ctx, 'sound-engine-awp', {
      numberOfInputs: 0,
      numberOfOutputs: 1,
      outputChannelCount: [2],
    });

    const globalVolumeNode = (ctx as any).globalVolume as GainNode | undefined;
    if (globalVolumeNode) {
      this.node.connect(globalVolumeNode);
    } else {
      this.node.connect(ctx.destination);
    }

    this.node.port.onmessage = evt => {
      if (evt.data?.type === 'ready') {
        this.nodeReady = true;
        this.flushPending();
      }
    };

    this.node.onprocessorerror = evt => console.error('[SoundEngine] AWP processor error:', evt);

    this.node.port.postMessage({ type: 'setWasmBytes', wasmBytes });
    this.node.port.postMessage({ type: 'listenerSab', sab: this.listenerSab });

    this.loadNeededSfx();
  }

  private flushPending() {
    // Master gain (in case it was set before ready).
    this.postEvent({ kind: EV_SET_MASTER_GAIN, params: [this.masterGain] });

    const sfx = this.pendingPlaySfx;
    this.pendingPlaySfx = [];
    for (const p of sfx) this.playSfx(p.name, p.opts);

    const spatial = this.pendingSpatial;
    this.pendingSpatial = [];
    for (const p of spatial) this.startSpatialLoopByHandle(p.handle, p.name, p.opts);
  }

  private postEvent(ev: {
    kind: number;
    handle?: number;
    sampleId?: number;
    flags?: number;
    params?: number[];
  }) {
    if (!this.node) return;
    this.node.port.postMessage({ type: 'event', event: ev });
  }

  // -- sample loading --------------------------------------------------------

  private getOrAllocSampleId(name: string): number {
    let id = this.samplesByName.get(name);
    if (id === undefined) {
      id = this.nextSampleId++;
      this.samplesByName.set(name, id);
    }
    return id;
  }

  public loadSfx(name: string) {
    if (!this.enabled) return;
    if (this.requestedSamples.has(name)) return;
    const def = this.sfxDefs[name];
    if (!def) {
      console.error('SFX def not found:', name);
      return;
    }
    this.requestedSamples.add(name);
    const id = this.getOrAllocSampleId(name);

    fetch(def.url)
      .then(r => r.arrayBuffer())
      .then(buf => this.ctx!.decodeAudioData(buf))
      .then(audioBuf => this.uploadDecoded(id, audioBuf))
      .catch(err => console.error(`Failed to load sfx "${name}":`, err));
  }

  private uploadDecoded(id: number, audioBuf: AudioBuffer) {
    if (!this.node) {
      // Shouldn't happen — node is ready well before any decode finishes — but
      // be defensive: queue and retry once node exists.
      setTimeout(() => this.uploadDecoded(id, audioBuf), 50);
      return;
    }
    const channels = Math.min(2, audioBuf.numberOfChannels);
    const len = audioBuf.length;
    const planar = new Float32Array(channels * len);
    for (let ch = 0; ch < channels; ch++) {
      const src = audioBuf.getChannelData(ch);
      planar.set(src, ch * len);
    }
    this.node.port.postMessage({ type: 'uploadSample', id, channels, data: planar, xfadeThreshold: 0 }, [
      planar.buffer,
    ]);
  }

  /** Re-upload a sample with crossfade-loop pre-baking enabled. */
  private uploadDecodedForLoop(id: number, audioBuf: AudioBuffer, xfade: number) {
    if (!this.node) return;
    const channels = Math.min(2, audioBuf.numberOfChannels);
    const len = audioBuf.length;
    const planar = new Float32Array(channels * len);
    for (let ch = 0; ch < channels; ch++) {
      planar.set(audioBuf.getChannelData(ch), ch * len);
    }
    this.node.port.postMessage({ type: 'uploadSample', id, channels, data: planar, xfadeThreshold: xfade }, [
      planar.buffer,
    ]);
  }

  private loadNeededSfx() {
    this.loadSfx('land_default');
    if (!this.config.neededSfx) return;
    for (const name of this.config.neededSfx) this.loadSfx(name);
  }

  // -- public API matching old SfxManager ------------------------------------

  public setConfig(config: SfxConfig) {
    this.config = config;
    if (!this.enabled) return;
    this.loadNeededSfx();
  }

  public setVizConfig(vizConfig: Readable<VizConfig>) {
    if (!this.enabled) return;
    this.vizConfig = vizConfig;
    if (this.vizConfigUnsubscribe) this.vizConfigUnsubscribe();
    this.vizConfigUnsubscribe = this.vizConfig.subscribe(cfg => {
      this.masterGain = cfg.audio.sfxVolume;
      if (this.nodeReady) {
        this.postEvent({ kind: EV_SET_MASTER_GAIN, params: [this.masterGain] });
      }
    });
  }

  public setCamera(camera: THREE.Camera) {
    if (!this.enabled) return;
    this.camera = camera;
  }

  /**
   * Register additional sfx defs into the runtime registry. Useful for
   * scene-specific one-offs (ambient loops, level-specific stingers) that
   * don't belong in the global builtins. Names override existing entries.
   */
  public registerSfxDefs(defs: Record<string, SfxDef>) {
    for (const name of Object.keys(defs)) {
      this.sfxDefs[name] = defs[name];
    }
  }

  /**
   * Pause / resume audio rendering without disturbing internal state. While
   * paused, all sample playheads, biquad filters, and the limiter envelope
   * stay frozen; output is silence. Queued events (master-gain changes,
   * new voice starts) still apply, so on resume the next render reflects
   * any state changes that happened during pause.
   */
  public setPaused(paused: boolean) {
    this.node?.port.postMessage({ type: 'setPaused', paused });
  }

  public onPlayerLand(materialClass: MaterialClass) {
    const override = this.config.land?.materialLandSounds?.[materialClass];
    if (override !== undefined) {
      if (typeof override === 'function') override();
      else this.playSfxRandom(override);
      return;
    }
    if (materialClass === MaterialClass.MetalPlate) {
      this.playSfxRandom(METAL_PLATE_LAND_SFX);
      return;
    }
    this.playSfx('land_default');
  }

  public onMaterialClassPresent(materialClass: MaterialClass) {
    if (materialClass === MaterialClass.MetalPlate) {
      const hasOverride = this.config.land?.materialLandSounds?.[materialClass] !== undefined;
      if (!hasOverride) {
        for (const name of METAL_PLATE_LAND_SFX) this.loadSfx(name);
      }
    }
  }

  public onJumpPadPresent() {
    this.loadSfx('jump_pad_trigger');
  }

  public onWalkStart(materialClass: MaterialClass) {
    this.curWalkMatClass = materialClass;
  }

  public onWalkStop() {
    this.curWalkMatClass = null;
  }

  public playSfxRandom(names: readonly string[], opts?: PlaySfxOpts) {
    this.playSfx(names[Math.floor(Math.random() * names.length)], opts);
  }

  public playSfx(name: string, opts?: PlaySfxOpts) {
    if (!this.enabled) return;
    if (!this.nodeReady) {
      this.pendingPlaySfx.push({ name, opts });
      return;
    }
    const id = this.samplesByName.get(name);
    if (id === undefined) {
      // Sample wasn't preloaded; kick off a load and queue the play.
      this.loadSfx(name);
      this.pendingPlaySfx.push({ name, opts });
      return;
    }
    const def = this.sfxDefs[name];
    const rate = opts?.playbackRate ?? def?.playbackRate ?? 1;
    const gain = opts?.gain ?? 1;
    const pan = opts?.pan ?? 0;
    this.postEvent({
      kind: EV_PLAY_ONESHOT,
      sampleId: id,
      params: [gain, rate, pan],
    });
  }

  // -- new: spatial loops ----------------------------------------------------

  public playSpatialLoop(name: string, opts: SpatialLoopOpts): SpatialVoiceHandle {
    const handle = this.nextHandle++;

    if (!this.enabled || !this.nodeReady) {
      if (this.enabled) this.pendingSpatial.push({ name, opts, handle });
      return this.makeHandle(handle);
    }

    const id = this.samplesByName.get(name);
    if (id === undefined) {
      // Need to (re)load with crossfade pre-baking.
      this.loadSfxForLoop(name, opts.xfade ?? 0.1);
      this.pendingSpatial.push({ name, opts, handle });
      return this.makeHandle(handle);
    }

    this.startSpatialLoopByHandle(handle, name, opts);
    return this.makeHandle(handle);
  }

  private loadSfxForLoop(name: string, xfade: number) {
    if (!this.enabled) return;
    const def = this.sfxDefs[name];
    if (!def) {
      console.error('SFX def not found:', name);
      return;
    }
    const id = this.getOrAllocSampleId(name);
    this.requestedSamples.add(name);
    fetch(def.url)
      .then(r => r.arrayBuffer())
      .then(buf => this.ctx!.decodeAudioData(buf))
      .then(audioBuf => {
        this.uploadDecodedForLoop(id, audioBuf, xfade);
        // Flush any pending spatial loops for this sample.
        const pending = this.pendingSpatial.filter(p => p.name === name);
        this.pendingSpatial = this.pendingSpatial.filter(p => p.name !== name);
        for (const p of pending) this.startSpatialLoopByHandle(p.handle, p.name, p.opts);
      })
      .catch(err => console.error(`Failed to load looping sfx "${name}":`, err));
  }

  private startSpatialLoopByHandle(handle: number, name: string, opts: SpatialLoopOpts) {
    const id = this.samplesByName.get(name);
    if (id === undefined) return;
    const filter = opts.filter;
    const filterCode = filterTypeCode(filter?.type);
    this.postEvent({
      kind: EV_START_SPATIAL_LOOP,
      handle,
      sampleId: id,
      params: [
        opts.pos[0],
        opts.pos[1],
        opts.pos[2],
        opts.gain ?? 1,
        opts.playbackRate ?? 1,
        opts.xfade ?? 0.1,
        filterCode,
        filter?.freq ?? 1000,
        filter?.q ?? 0.707,
        opts.refDistance ?? 1,
        opts.rolloff ?? 1,
        opts.cullThreshold ?? 0.001,
      ],
    });
  }

  private makeHandle(handle: number): SpatialVoiceHandle {
    const post = (params: number[], flags = 0) =>
      this.postEvent({ kind: EV_UPDATE_SPATIAL_LOOP, handle, flags, params });

    let pos: [number, number, number] = [0, 0, 0];
    let gain = 1;
    let stopped = false;

    return {
      setPosition: (x, y, z) => {
        if (stopped) return;
        pos = [x, y, z];
        post([x, y, z, gain]);
      },
      setGain: g => {
        if (stopped) return;
        gain = g;
        post([pos[0], pos[1], pos[2], gain]);
      },
      stop: () => {
        if (stopped) return;
        stopped = true;
        this.postEvent({ kind: EV_STOP_VOICE, handle });
      },
    };
  }

  // -- per-frame tick --------------------------------------------------------

  public tick(timeDiffSeconds: number, _curTimeSeconds: number) {
    if (!this.enabled) return;

    // Walk-step timing (preserved from old SfxManager).
    if (this.config.walk.playWalkSound && this.isWalking) {
      this.timeSinceLastStepSound += timeDiffSeconds;
      if (this.timeSinceLastStepSound > this.nextStepSoundTime) {
        this.config.walk.playWalkSound(this.curWalkMatClass!);
        const remainder = Math.abs(this.timeSinceLastStepSound - this.nextStepSoundTime);
        this.timeSinceLastStepSound = remainder;
        this.nextStepSoundTime = this.getNextStepSoundTime();
      }
    }

    // Listener pose sync (only writer; AWP racy-reads on each process()).
    if (this.camera && this.listenerView) {
      const cam = this.camera;
      cam.updateMatrixWorld();
      this.cameraDir.set(0, 0, -1).applyQuaternion(cam.quaternion);
      this.cameraRight.set(1, 0, 0).applyQuaternion(cam.quaternion);
      const v = this.listenerView;
      v[0] = cam.position.x;
      v[1] = cam.position.y;
      v[2] = cam.position.z;
      v[3] = this.cameraDir.x;
      v[4] = this.cameraDir.y;
      v[5] = this.cameraDir.z;
      v[6] = this.cameraRight.x;
      v[7] = this.cameraRight.y;
      v[8] = this.cameraRight.z;
      // v[9] reserved
    }
  }

  private getNextStepSoundTime() {
    let t = this.config.walk.timeBetweenStepsSeconds;
    t += Math.random() * this.config.walk.timeBetweenStepsJitterSeconds;
    return Math.max(0, t);
  }
}
