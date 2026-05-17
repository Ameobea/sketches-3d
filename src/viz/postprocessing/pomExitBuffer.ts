import * as THREE from 'three';

import type { Viz } from 'src/viz';
import { POM_BOUNDED_SILHOUETTE_FLAG } from 'src/viz/shaders/pom';

/**
 * Runtime support for the back-face-depth-bounded POM silhouette mode (the
 * shader side is in `src/viz/shaders/pom.ts`; full rationale + VRAM analysis in
 * `pom-known-limitations-and-authoring-guide.md` §2/§4).
 *
 * Each `boundedSilhouette` POM mesh needs, per pixel, the Euclidean distance
 * from the camera to its own nearest back face so the raymarch can clamp itself
 * to the convex exit. We capture that into a small RT pool each frame, just
 * before the main render:
 *
 *  - Non-overlapping meshes share one **combined** buffer (correct: nothing
 *    nearer contributes a back face at their pixels).
 *  - A mesh that overlaps another POM mesh on screen gets a **dedicated** buffer
 *    from a capped, lazily-allocated pool, rendered alone, so a rear mesh seen
 *    through a front mesh's carved notch bounds its march by its *own* exit.
 *
 * Overlap is detected conservatively (projected world-AABB screen rects);
 * pool overflow / mis-assignment degrades safely to Phase-1 via the shader's
 * non-positive-chord guard. VRAM is bounded at `poolCap + 1` RTs regardless of
 * scene POM count; the common (no-overlap) case keeps only the combined buffer.
 */

export interface PomExitBufferOptions {
  /** Max dedicated buffers. Typical on-screen overlap depth is 2-3; 4 = headroom. */
  poolCap?: number;
  /** Idle frames (~4s @60fps) the pool stays oversized before releasing extras. */
  poolShrinkAfter?: number;
}

type PomMaterial = THREE.ShaderMaterial & { uniforms: Record<string, THREE.IUniform> };

const isPomBoundedMaterial = (m: THREE.Material | null | undefined): m is PomMaterial =>
  !!m && !!(m as THREE.Material).userData?.[POM_BOUNDED_SILHOUETTE_FLAG];

/**
 * Front-face-culled "distance to camera" material. For a convex mesh the single
 * surviving (nearest) back-face fragment per pixel is the ray's exit point; its
 * Euclidean distance from the camera is what the POM marcher needs to bound
 * itself. R channel only; the RT is cleared to 0 so uncovered texels read as
 * the shader's "unbounded" sentinel. Generic — one shared instance.
 */
const buildBackFaceDistMaterial = () =>
  new THREE.ShaderMaterial({
    side: THREE.BackSide,
    depthTest: true,
    depthWrite: true,
    uniforms: { uCamPos: { value: new THREE.Vector3() } },
    vertexShader: /* glsl */ `
      varying vec3 vWP;
      void main() {
        vec4 wp = modelMatrix * vec4(position, 1.0);
        vWP = wp.xyz;
        gl_Position = projectionMatrix * viewMatrix * wp;
      }
    `,
    fragmentShader: /* glsl */ `
      uniform vec3 uCamPos;
      varying vec3 vWP;
      void main() {
        gl_FragColor = vec4(distance(vWP, uCamPos), 0.0, 0.0, 1.0);
      }
    `,
  });

interface Rect {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

const rectsOverlap = (a: Rect, b: Rect): boolean =>
  a.minX <= b.maxX && a.maxX >= b.minX && a.minY <= b.maxY && a.maxY >= b.minY;

export class PomExitBufferManager {
  private readonly viz: Viz;
  private readonly poolCap: number;
  private readonly poolShrinkAfter: number;

  private readonly meshes: THREE.Mesh[];
  /** Each mesh's bounded-POM material (multiple meshes may share one). */
  private readonly matOf = new Map<THREE.Mesh, PomMaterial>();
  private readonly pomMaterials = new Set<PomMaterial>();

  private readonly bfMat = buildBackFaceDistMaterial();
  private readonly dbSize = new THREE.Vector2();

  // Pool grows lazily up to `poolCap` (most frames need 0 dedicated RTs) and
  // shrinks after a sustained idle. The combined buffer is always live.
  private pool: THREE.WebGLRenderTarget[] = [];
  private poolIdleFrames = 0;
  private combinedRT: THREE.WebGLRenderTarget;
  private readonly assignedRT = new Map<THREE.Mesh, THREE.WebGLRenderTarget>();

  // Scratch reused each frame (no per-frame allocation).
  private readonly camMatInv = new THREE.Matrix4();
  private readonly viewProj = new THREE.Matrix4();
  private readonly frustum = new THREE.Frustum();
  private readonly worldSphere = new THREE.Sphere();
  private readonly corner = new THREE.Vector3();
  private readonly scratchColor = new THREE.Color();

  private readonly beforeRenderCb: () => void;
  private readonly resizeCb: () => void;
  private disposed = false;

  /**
   * One-shot deferred scene scan. Returns a manager iff at least one mesh in
   * the scene uses a `boundedSilhouette` POM material; otherwise `null` and
   * nothing is allocated.
   */
  static tryCreate(viz: Viz, opts?: PomExitBufferOptions): PomExitBufferManager | null {
    const meshes: THREE.Mesh[] = [];
    viz.scene.traverse(o => {
      if (!(o instanceof THREE.Mesh)) {
        return;
      }
      const mats = Array.isArray(o.material) ? o.material : [o.material];
      if (mats.some(isPomBoundedMaterial)) {
        meshes.push(o);
      }
    });
    if (meshes.length === 0) {
      return null;
    }
    return new PomExitBufferManager(viz, meshes, opts);
  }

  private constructor(viz: Viz, meshes: THREE.Mesh[], opts?: PomExitBufferOptions) {
    this.viz = viz;
    this.poolCap = opts?.poolCap ?? 4;
    this.poolShrinkAfter = opts?.poolShrinkAfter ?? 240;
    this.meshes = meshes;

    viz.renderer.getDrawingBufferSize(this.dbSize);
    this.combinedRT = this.makeRT();

    for (const mesh of meshes) {
      const mats = Array.isArray(mesh.material) ? mesh.material : [mesh.material];
      const pomMat = mats.find(isPomBoundedMaterial)!;
      this.matOf.set(mesh, pomMat);
      this.pomMaterials.add(pomMat);
      this.assignedRT.set(mesh, this.combinedRT);

      // The shared material samples one buffer per draw; rebind it to this
      // mesh's assigned RT right before the mesh draws (forced re-upload via
      // `uniformsNeedUpdate`, the same pattern `randomizeUVOffset` relies on).
      // Chain any pre-existing hook rather than clobbering it.
      const prev = mesh.onBeforeRender;
      mesh.onBeforeRender = (renderer, scene, camera, geometry, material, group) => {
        if (typeof prev === 'function') {
          prev.call(mesh, renderer, scene, camera, geometry, material, group);
        }
        const m = this.matOf.get(mesh)!;
        m.uniforms.pomBackDepth.value = (this.assignedRT.get(mesh) ?? this.combinedRT).texture;
        m.uniformsNeedUpdate = true;
      };
    }
    this.syncResolutionUniform();

    this.beforeRenderCb = () => this.update();
    this.resizeCb = () => this.onResize();
    // Priority 2: after scene before-render cbs (matrix/animation updates) so
    // the exit prerender sees the final transforms, still before the composite.
    viz.registerBeforeRenderCb(this.beforeRenderCb, 2);
    viz.registerResizeCb(this.resizeCb);
    viz.registerDestroyedCb(() => this.dispose());

    // Created from the one-shot detector during the first frame's before-render
    // phase (before the composite). Run one prerender now so that very first
    // main pass already samples correct exit buffers — no Phase-1 transient.
    this.update();
  }

  /**
   * Color = R32F: must match the shader's full-float `frontDist` precision, or
   * `exitDist - frontDist` quantizes at the silhouette and the discard edge
   * shimmers. Depth = 16-bit DepthTexture (front-culled convex => ~1 back face
   * per pixel, so depth precision is irrelevant; pure VRAM win, never sampled).
   */
  private makeRT(): THREE.WebGLRenderTarget {
    const w = Math.max(1, this.dbSize.x);
    const h = Math.max(1, this.dbSize.y);
    const rt = new THREE.WebGLRenderTarget(w, h, {
      type: THREE.FloatType,
      format: THREE.RedFormat,
      minFilter: THREE.NearestFilter,
      magFilter: THREE.NearestFilter,
      depthBuffer: true,
    });
    const dt = new THREE.DepthTexture(w, h);
    dt.type = THREE.UnsignedShortType;
    rt.depthTexture = dt;
    return rt;
  }

  private syncResolutionUniform(): void {
    for (const m of this.pomMaterials) {
      (m.uniforms.pomResolution.value as THREE.Vector2).set(this.dbSize.x, this.dbSize.y);
      m.uniformsNeedUpdate = true;
    }
  }

  private onResize(): void {
    this.viz.renderer.getDrawingBufferSize(this.dbSize);
    this.syncResolutionUniform();
    // Recreate (not setSize): reliably resizes the attached DepthTexture too.
    for (const rt of this.pool) {
      rt.dispose();
    }
    this.combinedRT.dispose();
    this.pool = [];
    this.poolIdleFrames = 0;
    this.combinedRT = this.makeRT();
  }

  /** Project a mesh's world AABB to an NDC-xy screen rect. Returns null if the
   * box straddles/behind the camera (perspective divide unreliable) -> caller
   * treats that conservatively as "overlaps" so the mesh gets a dedicated RT. */
  private projectedRect(mesh: THREE.Mesh): Rect | null {
    const box = mesh.geometry.boundingBox!;
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    const e = this.viewProj.elements;
    for (let i = 0; i < 8; i++) {
      this.corner.set(
        i & 1 ? box.max.x : box.min.x,
        i & 2 ? box.max.y : box.min.y,
        i & 4 ? box.max.z : box.min.z
      );
      this.corner.applyMatrix4(mesh.matrixWorld);
      const x = e[0] * this.corner.x + e[4] * this.corner.y + e[8] * this.corner.z + e[12];
      const y = e[1] * this.corner.x + e[5] * this.corner.y + e[9] * this.corner.z + e[13];
      const w = e[3] * this.corner.x + e[7] * this.corner.y + e[11] * this.corner.z + e[15];
      if (w <= 1e-6) {
        return null;
      }
      const nx = x / w;
      const ny = y / w;
      minX = Math.min(minX, nx);
      minY = Math.min(minY, ny);
      maxX = Math.max(maxX, nx);
      maxY = Math.max(maxY, ny);
    }
    return { minX, minY, maxX, maxY };
  }

  private update(): void {
    if (this.disposed) {
      return;
    }
    const { viz } = this;
    viz.camera.updateMatrixWorld();
    (this.bfMat.uniforms.uCamPos.value as THREE.Vector3).setFromMatrixPosition(viz.camera.matrixWorld);
    this.camMatInv.copy(viz.camera.matrixWorld).invert();
    this.viewProj.multiplyMatrices(viz.camera.projectionMatrix, this.camMatInv);
    this.frustum.setFromProjectionMatrix(this.viewProj);

    // 1. Cull to frustum + compute each visible mesh's screen rect.
    const visible: THREE.Mesh[] = [];
    const rectOf = new Map<THREE.Mesh, Rect | null>();
    for (const mesh of this.meshes) {
      mesh.updateWorldMatrix(true, false);
      if (!mesh.geometry.boundingBox) {
        mesh.geometry.computeBoundingBox();
      }
      if (!mesh.geometry.boundingSphere) {
        mesh.geometry.computeBoundingSphere();
      }
      this.worldSphere.copy(mesh.geometry.boundingSphere!).applyMatrix4(mesh.matrixWorld);
      if (!this.frustum.intersectsSphere(this.worldSphere)) {
        this.assignedRT.set(mesh, this.combinedRT);
        continue;
      }
      visible.push(mesh);
      rectOf.set(mesh, this.projectedRect(mesh));
    }

    // No POM mesh on screen this frame: nothing samples an exit buffer, so
    // skip all renderer work (state save/restore, clears, the prerender).
    if (visible.length === 0) {
      if (this.pool.length > 0 && ++this.poolIdleFrames >= this.poolShrinkAfter) {
        for (const rt of this.pool) {
          rt.dispose();
        }
        this.pool = [];
        this.poolIdleFrames = 0;
      }
      return;
    }

    // 2. A mesh needs a dedicated RT if its rect overlaps another visible POM
    //    mesh's rect (or its rect is null = straddles the eye -> treat
    //    conservatively as overlapping). `needsDedicated` only ever receives
    //    members of `visible`.
    const needsDedicated = new Set<THREE.Mesh>();
    for (let i = 0; i < visible.length; i++) {
      const ra = rectOf.get(visible[i]) ?? null;
      for (let j = i + 1; j < visible.length; j++) {
        const rb = rectOf.get(visible[j]) ?? null;
        if (ra === null || rb === null || rectsOverlap(ra, rb)) {
          needsDedicated.add(visible[i]);
          needsDedicated.add(visible[j]);
        }
      }
    }

    // 3. Size the pool to demand: grow lazily up to `poolCap`, release extras
    //    after a sustained idle so the common (no-overlap) case keeps only the
    //    combined buffer live.
    const wantDedicated = Math.min(needsDedicated.size, this.poolCap);
    while (this.pool.length < wantDedicated) {
      this.pool.push(this.makeRT());
    }
    if (this.pool.length > wantDedicated) {
      if (++this.poolIdleFrames >= this.poolShrinkAfter) {
        while (this.pool.length > wantDedicated) {
          this.pool.pop()!.dispose();
        }
        this.poolIdleFrames = 0;
      }
    } else {
      this.poolIdleFrames = 0;
    }

    // Assign: dedicated meshes pull from the pool until it is exhausted;
    // everything else (and pool overflow) shares the combined buffer.
    const dedicated: THREE.Mesh[] = [];
    let poolIdx = 0;
    for (const mesh of visible) {
      if (needsDedicated.has(mesh) && poolIdx < this.pool.length) {
        const rt = this.pool[poolIdx++];
        this.assignedRT.set(mesh, rt);
        dedicated.push(mesh);
      } else {
        this.assignedRT.set(mesh, this.combinedRT);
      }
    }

    // 4. Render the exit buffers.
    const prevRT = viz.renderer.getRenderTarget();
    const prevClear = viz.renderer.getClearColor(this.scratchColor);
    const prevClearAlpha = viz.renderer.getClearAlpha();
    const prevAutoClear = viz.renderer.autoClear;
    viz.renderer.setClearColor(0x000000, 0);
    viz.renderer.autoClear = false;

    // 4a. Dedicated meshes: each alone into its own pool RT.
    for (const mesh of dedicated) {
      const rt = this.assignedRT.get(mesh)!;
      const savedMat = mesh.material;
      mesh.material = this.bfMat;
      viz.renderer.setRenderTarget(rt);
      viz.renderer.clear(true, true, false);
      viz.renderer.render(mesh, viz.camera);
      mesh.material = savedMat;
    }

    // 4b. Combined buffer: all combined-assigned visible meshes together,
    //     cleared once, depth test keeping the nearest back face per pixel.
    //     Correct for non-overlapping meshes; overflow degrades via the
    //     shader's non-positive-chord guard.
    viz.renderer.setRenderTarget(this.combinedRT);
    viz.renderer.clear(true, true, false);
    for (const mesh of visible) {
      if (this.assignedRT.get(mesh) !== this.combinedRT) {
        continue;
      }
      const savedMat = mesh.material;
      mesh.material = this.bfMat;
      viz.renderer.render(mesh, viz.camera);
      mesh.material = savedMat;
    }

    viz.renderer.setRenderTarget(prevRT);
    viz.renderer.setClearColor(prevClear, prevClearAlpha);
    viz.renderer.autoClear = prevAutoClear;
  }

  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    this.viz.unregisterBeforeRenderCb(this.beforeRenderCb);
    for (const rt of this.pool) {
      rt.dispose();
    }
    this.pool = [];
    this.combinedRT.dispose();
    this.bfMat.dispose();
  }
}
