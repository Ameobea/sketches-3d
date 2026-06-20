import * as THREE from 'three';

import type { Viz } from 'src/viz';
import { POM_BOUNDED_SILHOUETTE_FLAG } from 'src/viz/shaders/pom';
import { INLINE_EMISSIVE_LAYER } from 'src/viz/passes/inlineEmissivePass';

/**
 * Runtime support for the back-face-depth-bounded POM silhouette mode (the
 * shader side is in `pom.ts`
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
 * Overlap is detected conservatively based on projected world-AABB screen rects.
 */

export interface PomExitBufferOptions {
  poolCap?: number;
  poolShrinkAfter?: number;
}

type PomMaterial = THREE.ShaderMaterial & { uniforms: Record<string, THREE.IUniform> };

const isPomBoundedMaterial = (m: THREE.Material | null | undefined): m is PomMaterial =>
  !!m && !!(m as THREE.Material).userData?.[POM_BOUNDED_SILHOUETTE_FLAG];

/**
 * Front-face-culled "distance to camera" material. For a convex mesh the single
 * surviving (nearest) back-face fragment per pixel is the ray's exit point; its
 * Euclidean distance from the camera is what the POM marcher needs to bound
 * itself.
 */
const buildBackFaceDistMaterial = () =>
  new THREE.ShaderMaterial({
    side: THREE.BackSide,
    depthTest: true,
    depthWrite: true,
    uniforms: { uCamPos: { value: new THREE.Vector3() } },
    // `gl_Position` must match Three.js's `project_vertex` association order
    // exactly; different orderings drift at ULP scale and z-fight the main pass.
    vertexShader: /* glsl */ `
      varying vec3 vWP;
      void main() {
        vec4 localPos = vec4(position, 1.0);
        #ifdef USE_INSTANCING
          localPos = instanceMatrix * localPos;
        #endif
        vWP = (modelMatrix * localPos).xyz;
        gl_Position = projectionMatrix * modelViewMatrix * localPos;
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

const collectPomBoundedMeshes = (scene: THREE.Scene): THREE.Mesh[] => {
  const out: THREE.Mesh[] = [];
  scene.traverse(o => {
    if (!(o instanceof THREE.Mesh)) {
      return;
    }
    const mats = Array.isArray(o.material) ? o.material : [o.material];
    if (mats.some(isPomBoundedMaterial)) {
      out.push(o);
    }
  });
  return out;
};

export class PomExitBufferManager {
  private readonly viz: Viz;
  private readonly poolCap: number;
  private readonly poolShrinkAfter: number;

  private meshes: THREE.Mesh[];
  /** Each mesh's bounded-POM material (multiple meshes may share one). */
  private readonly matOf = new Map<THREE.Mesh, PomMaterial>();
  private readonly pomMaterials = new Set<PomMaterial>();
  /** Pre-install `onBeforeRender` per mesh, so `unregisterMesh` can restore it. */
  private readonly prevHook = new Map<THREE.Mesh, THREE.Object3D['onBeforeRender'] | undefined>();

  private readonly bfMat = buildBackFaceDistMaterial();
  private readonly dbSize = new THREE.Vector2();

  private pool: THREE.WebGLRenderTarget[] = [];
  private poolIdleFrames = 0;
  private combinedRT: THREE.WebGLRenderTarget;
  private readonly assignedRT = new Map<THREE.Mesh, THREE.WebGLRenderTarget>();

  private readonly camMatInv = new THREE.Matrix4();
  private readonly viewProj = new THREE.Matrix4();
  private readonly frustum = new THREE.Frustum();
  private readonly worldSphere = new THREE.Sphere();
  private readonly corner = new THREE.Vector3();
  private readonly scratchColor = new THREE.Color();

  private readonly resizeCb: () => void;
  private disposed = false;

  /**
   * One-shot deferred scene scan. Returns a manager iff at least one mesh in
   * the scene uses a `boundedSilhouette` POM material; otherwise `null` and
   * nothing is allocated.
   */
  static tryCreate(viz: Viz, opts?: PomExitBufferOptions): PomExitBufferManager | null {
    const meshes = collectPomBoundedMeshes(viz.scene);
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
      this.registerMesh(mesh);
    }
    this.syncResolutionUniform();

    this.resizeCb = () => this.onResize();
    viz.registerResizeCb(this.resizeCb);
    viz.registerDestroyedCb(() => this.dispose());
  }

  /** Idempotent; the hook reads `matOf`/`assignedRT` live each draw. */
  private registerMesh(mesh: THREE.Mesh): void {
    const mats = Array.isArray(mesh.material) ? mesh.material : [mesh.material];
    const pomMat = mats.find(isPomBoundedMaterial)!;
    this.matOf.set(mesh, pomMat);
    this.pomMaterials.add(pomMat);
    if (!this.assignedRT.has(mesh)) {
      this.assignedRT.set(mesh, this.combinedRT);
    }

    if (this.prevHook.has(mesh)) {
      return;
    }
    const prev = mesh.onBeforeRender;
    this.prevHook.set(mesh, prev);
    mesh.onBeforeRender = (renderer, scene, camera, geometry, material, group) => {
      if (typeof prev === 'function') {
        prev.call(mesh, renderer, scene, camera, geometry, material, group);
      }
      const m = this.matOf.get(mesh);
      if (!m) {
        return;
      }
      m.uniforms.pomBackDepth.value = (this.assignedRT.get(mesh) ?? this.combinedRT).texture;
      m.uniformsNeedUpdate = true;
    };
  }

  private unregisterMesh(mesh: THREE.Mesh): void {
    this.matOf.delete(mesh);
    this.assignedRT.delete(mesh);
    if (this.prevHook.has(mesh)) {
      mesh.onBeforeRender = this.prevHook.get(mesh) ?? (() => {});
      this.prevHook.delete(mesh);
    }
  }

  rescan(): void {
    if (this.disposed) {
      return;
    }
    const current = new Set(collectPomBoundedMeshes(this.viz.scene));

    for (const mesh of this.meshes) {
      if (!current.has(mesh)) {
        this.unregisterMesh(mesh);
      }
    }
    for (const mesh of current) {
      this.registerMesh(mesh);
    }
    this.meshes = Array.from(current);

    this.pomMaterials.clear();
    for (const m of this.matOf.values()) {
      this.pomMaterials.add(m);
    }
    this.syncResolutionUniform();
  }

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
    for (const rt of this.pool) {
      rt.dispose();
    }
    this.combinedRT.dispose();
    this.pool = [];
    this.poolIdleFrames = 0;
    this.combinedRT = this.makeRT();
  }

  /** Project a mesh's world AABB to an NDC-xy screen rect. Returns null if the
   * box straddles/behind the camera -> caller treats that conservatively as
   * "overlaps" so the mesh gets a dedicated RT.
   */
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

  /**
   * Render the back-face exit buffers. Must be called immediately before
   * `effectComposer.render` so `viz.camera` matches the matrix the main pass
   * uses — a before-render cb would see last-frame's camera and shimmer the
   * silhouettes by one frame.
   */
  update(): void {
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
    // A bounded mesh that also opts into `inlineEmissiveBypass` lives off layer 0,
    // so it would be layer-culled from these single-mesh renders (the main camera
    // tests only layer 0). Enable its layer for the exit pass; harmless for the
    // common layer-0 bounded meshes.
    const prevCamLayerMask = viz.camera.layers.mask;
    viz.camera.layers.enable(INLINE_EMISSIVE_LAYER);

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

    viz.camera.layers.mask = prevCamLayerMask;
    viz.renderer.setRenderTarget(prevRT);
    viz.renderer.setClearColor(prevClear, prevClearAlpha);
    viz.renderer.autoClear = prevAutoClear;
  }

  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    for (const mesh of this.meshes) {
      this.unregisterMesh(mesh);
    }
    this.meshes = [];
    for (const rt of this.pool) {
      rt.dispose();
    }
    this.pool = [];
    this.combinedRT.dispose();
    this.bfMat.dispose();
  }
}
