import type * as THREE from 'three';

import { buildMaterial, FallbackMat, HiddenMat, type MaterialDef } from 'src/geoscript/materials';
import { CustomBasicShaderMaterial } from 'src/viz/shaders/customBasicShader';
import { CustomShaderMaterial } from 'src/viz/shaders/customShader';
import { Textures } from 'src/viz/scenes/geoscriptPlayground/materialEditor/state.svelte';
import { referencedTextureIDsForDef } from 'src/viz/scenes/geoscriptPlayground/materialLoading.svelte';
import type { Viz } from 'src/viz';

export interface MaterialRuntimeEntry {
  name: string;
  /** Latest landed build; null while the first build is in flight (render as `HiddenMat`). */
  material: THREE.Material | null;
  loading: boolean;
}

const isSharedMat = (m: THREE.Material) => m === FallbackMat || m === HiddenMat;

/**
 * Owns def → `THREE.Material` builds and their lifetime: per-id content hashing (incl.
 * referenced-texture metadata availability — a build that ran before metadata arrived
 * resolved its maps to nothing and must re-run once it lands), generation guards on
 * async landings, disposal of replaced builds, and per-material animation callbacks.
 */
export class MaterialRuntime {
  /** By def id. Reassigned wholesale so consumers can depend on it cheaply. */
  entries: Record<string, MaterialRuntimeEntry> = $state.raw({});
  err: string | null = $state(null);
  readonly allLoaded = $derived(Object.values(this.entries).every(e => !e.loading));
  /** Geoscript-visible material name → entry. */
  readonly byName = $derived(Object.fromEntries(Object.values(this.entries).map(e => [e.name, e])));

  private builds = new Map<string, { hash: string; gen: number; cb?: (t: number) => void }>();
  private errors: Record<string, string> = {};
  private loadWaiters: (() => void)[] = [];

  constructor(
    private readonly viz: Viz,
    private readonly loader: THREE.ImageBitmapLoader
  ) {}

  /** Rebuilds changed/new defs, removes deleted ones. Unchanged defs (by hash) are no-ops. */
  sync(defs: Record<string, MaterialDef>) {
    for (const [id, def] of Object.entries(defs)) {
      const texKey = referencedTextureIDsForDef(def)
        .map(tid => Textures.textures[tid]?.url ?? '')
        .join('|');
      const hash = `${JSON.stringify(def)}|${texKey}`;
      const prev = this.builds.get(id);
      if (prev?.hash === hash) continue;

      const gen = (prev?.gen ?? 0) + 1;
      if (prev?.cb) this.viz.unregisterBeforeRenderCb(prev.cb);
      this.builds.set(id, { hash, gen });
      delete this.errors[id];

      let result: THREE.Material | Promise<THREE.Material>;
      try {
        result = buildMaterial(this.loader, def, id);
      } catch (e) {
        this.recordError(id, def, e);
        result = FallbackMat;
      }

      if (result instanceof Promise) {
        // Keep showing the previous build (if any) until the new one lands.
        this.setEntry(id, { name: def.name, material: this.entries[id]?.material ?? null, loading: true });
        result
          .catch(e => {
            if (this.builds.get(id)?.gen === gen) this.recordError(id, def, e);
            return FallbackMat as THREE.Material;
          })
          .then(mat => this.land(id, def, gen, mat));
      } else {
        this.land(id, def, gen, result);
      }
    }

    for (const id of Object.keys(this.entries)) {
      if (id in defs) continue;
      const build = this.builds.get(id);
      if (build?.cb) this.viz.unregisterBeforeRenderCb(build.cb);
      this.builds.delete(id);
      delete this.errors[id];
      const mat = this.entries[id].material;
      if (mat && !isSharedMat(mat)) queueMicrotask(() => mat.dispose());
      const next = { ...this.entries };
      delete next[id];
      this.entries = next;
    }
    this.reportErrors();
    this.flushLoadWaiters();
  }

  /** Resolves once no build is in flight. Waiters are resolved at build-landing points
   *  rather than reactively, so callers outside the effect graph (render harness) can await. */
  untilAllLoaded(): Promise<void> {
    if (this.allLoaded) {
      return Promise.resolve();
    }
    return new Promise(r => this.loadWaiters.push(r));
  }

  private flushLoadWaiters() {
    if (this.loadWaiters.length === 0 || !this.allLoaded) {
      return;
    }
    const waiters = this.loadWaiters;
    this.loadWaiters = [];
    for (const w of waiters) w();
  }

  private land(id: string, def: MaterialDef, gen: number, mat: THREE.Material) {
    const build = this.builds.get(id);
    if (build?.gen !== gen) {
      // Superseded (or removed) while in flight: this build never gets used.
      if (!isSharedMat(mat)) mat.dispose();
      return;
    }
    if (
      (mat instanceof CustomShaderMaterial || mat instanceof CustomBasicShaderMaterial) &&
      def.type === 'customShader' &&
      (def.shaders?.colorShader ||
        def.shaders?.iridescenceShader ||
        def.shaders?.metalnessShader ||
        def.shaders?.roughnessShader)
    ) {
      build.cb = curTimeSeconds => mat.setCurTimeSeconds(curTimeSeconds);
      this.viz.registerBeforeRenderCb(build.cb);
    }
    const prevMat = this.entries[id]?.material;
    this.setEntry(id, { name: def.name, material: mat, loading: false });
    if (prevMat && prevMat !== mat && !isSharedMat(prevMat)) {
      // Deferred so the mesh-assignment effect flushes the swap first.
      queueMicrotask(() => prevMat.dispose());
    }
    this.reportErrors();
    this.flushLoadWaiters();
  }

  private setEntry(id: string, entry: MaterialRuntimeEntry) {
    this.entries = { ...this.entries, [id]: entry };
  }

  private recordError(id: string, def: MaterialDef, e: unknown) {
    this.errors[id] = `Material "${def.name ?? 'material'}": ${e instanceof Error ? e.message : String(e)}`;
  }

  private reportErrors() {
    const msgs = Object.values(this.errors);
    this.err = msgs.length ? msgs.join('\n\n') : null;
  }
}
