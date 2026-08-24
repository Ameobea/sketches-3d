import * as THREE from 'three';

import type { Viz } from 'src/viz';

/** Ceiling on the delta handed to render callbacks; a governed loop can idle for seconds. */
export const MAX_FRAME_DELTA_SECONDS = 1 / 15;

// Presents when the scene hash moves. The hash is an optimization, not the correctness
// boundary: HEARTBEAT_MS presents regardless, so anything it misses is stale for a second
// rather than forever, which is what keeps its coverage from having to be exhaustive.

const SETTLE_MS = 250;
const WATCH_TO_SLEEP_MS = 3_000;
// SLEEP_POLL_MS is a floor, stretched by a costly hash; past HASH_BUDGET_MS the watch tier is
// skipped entirely.
const SLEEP_POLL_MS = 250;
const HASH_BUDGET_MS = 0.75;
const HEARTBEAT_MS = 1_000;

export type FrameGovernorTier = 'render' | 'watch' | 'sleep' | 'suspended';

const TEX_SLOTS = [
  'map',
  'normalMap',
  'roughnessMap',
  'metalnessMap',
  'emissiveMap',
  'aoMap',
  'alphaMap',
  'bumpMap',
  'displacementMap',
  'envMap',
  'lightMap',
  'iridescenceMap',
  'clearcoatMap',
  'specularMap',
] as const;

// Count folded in because `setAttribute` resets `version` to 0; equal-length swaps still alias.
const hashAttr = (
  h: Hasher,
  a: THREE.BufferAttribute | THREE.InterleavedBufferAttribute | null | undefined
) => {
  if (!a) {
    h.u32(0);
    return;
  }
  h.u32('data' in a ? a.data.version : a.version);
  h.u32(a.count);
};

// One mix per float instead of two; a change too small for f32 is too small to see.
const f32 = new Float32Array(1);
const f32Bits = new Uint32Array(f32.buffer);

class Hasher {
  private h = 0;

  reset() {
    this.h = 0x811c9dc5;
  }

  u32(v: number) {
    this.h = Math.imul(this.h ^ (v >>> 0), 0x01000193);
  }

  f(v: number) {
    f32[0] = v;
    this.u32(f32Bits[0]);
  }

  get value(): number {
    return this.h >>> 0;
  }
}

export class FrameGovernor {
  private readonly viz: Viz;
  private readonly hasher = new Hasher();
  private readonly leases = new Set<symbol>();
  /** Leases that also override suspension. */
  private readonly captureLeases = new Set<symbol>();
  private lastHash = -1;
  private tier: FrameGovernorTier = 'render';
  private renderUntilMs = 0;
  private stillSinceMs = 0;
  private epoch = 0;
  private sleepTimer = 0;
  private hashMs = 0;
  private lastPresentMs = 0;
  private disposed = false;
  private suspendRequested = false;

  constructor(viz: Viz) {
    this.viz = viz;
    // Hover highlights live in shader uniforms no hash can see, so viewport pointer
    // activity renders unconditionally.
    const canvas = viz.renderer.domElement;
    canvas.addEventListener('pointermove', this.onCanvasInput);
    canvas.addEventListener('pointerdown', this.onCanvasInput);
    canvas.addEventListener('wheel', this.onCanvasInput, { passive: true });
    // Anywhere else, input only wakes the loop back up; the hash decides from there.
    for (const evt of ['pointerdown', 'keydown', 'wheel'] as const) {
      document.addEventListener(evt, this.onDocumentInput, { capture: true, passive: true });
    }
  }

  /** Presents every frame until released. `forCapture` also overrides suspension, for readers
   *  of the canvas itself; a plain lease yields to a covered canvas. */
  acquireContinuous = (forCapture = false): (() => void) => {
    const token = Symbol();
    this.leases.add(token);
    if (forCapture) {
      this.captureLeases.add(token);
    }
    this.applySuspension();
    this.wake();
    return () => {
      if (!this.leases.delete(token)) {
        return;
      }
      this.captureLeases.delete(token);
      this.applySuspension();
    };
  };

  /** Forces a render on the next tick, for state no scene hash can see. */
  invalidate = () => {
    this.epoch += 1;
    this.wake();
  };

  /** Hard off, for a canvas nothing can see. */
  setSuspended = (suspended: boolean) => {
    this.suspendRequested = suspended;
    this.applySuspension();
  };

  tick = (deltaTime: number, curTimeSeconds: number): boolean => {
    if (this.tier === 'suspended') {
      return false;
    }

    const now = performance.now();
    this.viz.stageFrame(deltaTime, curTimeSeconds);

    if (this.leases.size > 0) {
      // Stillness tracked here too, or releasing a long-held lease reads as minutes of quiet
      // and drops straight past the watch tier.
      this.stillSinceMs = now;
      this.setTier('render');
      this.present(deltaTime, curTimeSeconds, now);
      return true;
    }

    const inSettleWindow = now < this.renderUntilMs;
    const changed = inSettleWindow || this.hashChanged();
    if (changed) {
      // Stillness tracks change, not presentation — a heartbeat frame must not look like
      // activity, or the loop could never wind down past it.
      this.stillSinceMs = now;
      this.setTier('render');
    }
    if (changed || this.heartbeatDue(now)) {
      this.present(deltaTime, curTimeSeconds, now);
      // `hashChanged` stores the hash itself; the settle window short-circuits it, so force a
      // fresh one on the next tick.
      if (inSettleWindow) {
        this.lastHash = -1;
      }
      return true;
    }

    // Watching at frame rate only pays off while the hash is cheap; past that, polling is
    // both the throttle and the fallback.
    if (this.hashMs > HASH_BUDGET_MS || now - this.stillSinceMs >= WATCH_TO_SLEEP_MS) {
      this.setTier('sleep');
      this.scheduleSleepPoll();
      return false;
    }
    this.setTier('watch');
    return true;
  };

  private present(deltaTime: number, curTimeSeconds: number, now: number) {
    this.viz.presentFrame(deltaTime, curTimeSeconds);
    this.lastPresentMs = now;
  }

  private heartbeatDue(now: number): boolean {
    return now - this.lastPresentMs >= HEARTBEAT_MS;
  }

  private applySuspension() {
    const suspend = this.suspendRequested && this.captureLeases.size === 0;
    if (suspend === (this.tier === 'suspended')) {
      return;
    }
    if (suspend) {
      this.setTier('suspended');
      this.clearSleepTimer();
      this.viz.stopAnimationLoop();
    } else {
      this.setTier('render');
      this.renderUntilMs = performance.now() + SETTLE_MS;
      this.lastHash = -1;
      this.viz.startAnimationLoop();
    }
  }

  dispose = () => {
    this.disposed = true;
    this.clearSleepTimer();
    this.leases.clear();
    this.captureLeases.clear();
    const canvas = this.viz.renderer.domElement;
    canvas.removeEventListener('pointermove', this.onCanvasInput);
    canvas.removeEventListener('pointerdown', this.onCanvasInput);
    canvas.removeEventListener('wheel', this.onCanvasInput);
    for (const evt of ['pointerdown', 'keydown', 'wheel'] as const) {
      document.removeEventListener(evt, this.onDocumentInput, { capture: true });
    }
  };

  private onCanvasInput = () => {
    if (this.tier === 'suspended') {
      return;
    }
    this.renderUntilMs = performance.now() + SETTLE_MS;
    this.wake();
  };

  private onDocumentInput = () => {
    if (this.tier === 'sleep') {
      this.wake();
    }
  };

  get currentTier(): FrameGovernorTier {
    return this.tier;
  }

  private setTier(tier: FrameGovernorTier) {
    this.tier = tier;
    // Not deduped: the stats also receive tiers this class never sees.
    this.viz.stats?.setTier(tier);
  }

  private wake() {
    if (this.disposed || this.tier === 'suspended') {
      return;
    }
    this.stillSinceMs = performance.now();
    if (this.tier === 'sleep') {
      this.setTier('watch');
      this.clearSleepTimer();
    }
    // Unconditional: the loop can have been stopped without the tier changing.
    this.viz.startAnimationLoop();
  }

  private scheduleSleepPoll() {
    this.clearSleepTimer();
    const delay = Math.max(SLEEP_POLL_MS, this.hashMs * 100);
    this.sleepTimer = window.setTimeout(this.pollWhileAsleep, delay);
  }

  private clearSleepTimer() {
    if (this.sleepTimer) {
      window.clearTimeout(this.sleepTimer);
      this.sleepTimer = 0;
    }
  }

  private pollWhileAsleep = () => {
    this.sleepTimer = 0;
    if (this.disposed || this.tier !== 'sleep') {
      return;
    }
    if (this.viz.loopBlocked) {
      // Parked rather than re-armed: nothing may be presented, and `maybeResumeViz` restarts
      // the loop on the way back.
      return;
    }
    // Staged first for the same reason `tick` does it: the before-render callbacks write the
    // state being hashed.
    const curTimeSeconds = this.viz.clock.getElapsedTime();
    const now = performance.now();
    // One delta for both halves of the frame, so before- and after-render callbacks agree.
    const deltaTime = Math.min((now - this.lastPresentMs) / 1000, MAX_FRAME_DELTA_SECONDS);
    this.viz.stageFrame(deltaTime, curTimeSeconds);
    if (this.hashChanged()) {
      // The check above consumed the change, so only the settle window draws it.
      this.renderUntilMs = now + SETTLE_MS;
      this.wake();
      return;
    }
    if (this.heartbeatDue(now)) {
      // Presented from the timer rather than by waking: a heartbeat is no reason to resume
      // frame-rate hashing, and staying asleep keeps the rAF loop stopped.
      this.present(deltaTime, curTimeSeconds, now);
    }
    this.scheduleSleepPoll();
  };

  private hashChanged(): boolean {
    const start = performance.now();
    const hash = this.computeHash();
    // Fast attack, slow decay: a scene that becomes expensive to hash should leave the watch
    // tier on the next frame, not twenty frames later.
    const sample = performance.now() - start;
    this.hashMs = Math.max(sample, this.hashMs * 0.9 + sample * 0.1);
    if (hash === this.lastHash) {
      return false;
    }
    this.lastHash = hash;
    return true;
  }

  private computeHash(): number {
    const { viz } = this;
    const h = this.hasher;
    h.reset();
    h.u32(this.epoch);

    const cam = viz.camera;
    cam.updateMatrixWorld();
    const camEls = cam.matrixWorld.elements;
    const projEls = cam.projectionMatrix.elements;
    for (let i = 0; i < 16; i++) {
      h.f(camEls[i]);
      h.f(projEls[i]);
    }

    const size = viz.renderer.getDrawingBufferSize(_size);
    h.f(size.x);
    h.f(size.y);

    this.hashScene(viz.scene);
    this.hashScene(viz.overlayScene);
    return h.value;
  }

  private hashScene(scene: THREE.Scene) {
    const h = this.hasher;
    const bg = scene.background;
    h.u32(bg instanceof THREE.Color ? bg.getHex() : bg ? bg.id : 0);
    h.u32(scene.environment ? scene.environment.id : 0);
    h.f(scene.environmentIntensity ?? 1);
    h.u32(scene.overrideMaterial ? scene.overrideMaterial.id : 0);
    const fog = scene.fog;
    if (fog) {
      h.u32(fog.color.getHex());
      h.f((fog as THREE.Fog).near ?? (fog as THREE.FogExp2).density);
      h.f((fog as THREE.Fog).far ?? 0);
    }
    scene.traverse(this.hashObject);
  }

  private hashObject = (o: THREE.Object3D) => {
    const h = this.hasher;
    h.u32(o.id);
    h.u32(o.visible ? 1 : 0);
    h.u32(o.castShadow ? 1 : 0);
    h.u32(o.receiveShadow ? 1 : 0);
    h.u32(o.layers.mask);
    h.f(o.renderOrder);
    h.f(o.position.x);
    h.f(o.position.y);
    h.f(o.position.z);
    h.f(o.quaternion.x);
    h.f(o.quaternion.y);
    h.f(o.quaternion.z);
    h.f(o.quaternion.w);
    h.f(o.scale.x);
    h.f(o.scale.y);
    h.f(o.scale.z);
    // Directly-written matrices (gizmo internals) never touch the TRS above.
    if (!o.matrixAutoUpdate) {
      const els = o.matrix.elements;
      for (let i = 0; i < 16; i++) {
        h.f(els[i]);
      }
    }

    const mesh = o as THREE.Mesh;
    if (mesh.isMesh || (o as THREE.Line).isLine || (o as THREE.Points).isPoints) {
      const geom = mesh.geometry;
      h.u32(geom.id);
      hashAttr(h, geom.attributes.position);
      hashAttr(h, geom.index);
      h.f(geom.drawRange.start);
      h.f(geom.drawRange.count);
      const mat = mesh.material;
      if (Array.isArray(mat)) {
        for (const m of mat) {
          this.hashMaterial(m);
        }
      } else if (mat) {
        this.hashMaterial(mat);
      }
      const inst = o as THREE.InstancedMesh;
      if (inst.isInstancedMesh) {
        h.u32(inst.count);
        hashAttr(h, inst.instanceMatrix);
        hashAttr(h, inst.instanceColor);
      }
    }

    const light = o as THREE.Light;
    if (light.isLight) {
      h.f(light.intensity);
      h.u32(light.color.getHex());
      const target = (o as THREE.DirectionalLight).target;
      if (target) {
        h.f(target.position.x);
        h.f(target.position.y);
        h.f(target.position.z);
      }
      const shadow = (light as THREE.DirectionalLight).shadow as THREE.LightShadow | undefined;
      if (shadow) {
        h.f(shadow.bias);
        h.f(shadow.normalBias);
        h.f(shadow.radius);
        h.u32(shadow.mapSize.x);
        h.u32(shadow.mapSize.y);
      }
    }
  };

  private hashMaterial(m: THREE.Material) {
    const h = this.hasher;
    h.u32(m.id);
    h.u32(m.version);
    h.u32(m.visible ? 1 : 0);
    h.f(m.opacity);
    for (const slot of TEX_SLOTS) {
      const tex = (m as any)[slot] as THREE.Texture | null | undefined;
      h.u32(tex ? tex.id : 0);
      h.u32(tex ? tex.version : 0);
    }
    // Neither sets `needsUpdate`, so `m.version` never moves for them. `curTimeSeconds`
    // advancing is also what keeps a time-reading material rendering, and one bound to nothing
    // is never traversed.
    const uniforms = (m as THREE.ShaderMaterial).uniforms;
    if (uniforms?.envMapIntensity) {
      h.f(uniforms.envMapIntensity.value as number);
    }
    if (uniforms?.curTimeSeconds) {
      h.f(uniforms.curTimeSeconds.value as number);
    }
  }
}

const _size = new THREE.Vector2();
