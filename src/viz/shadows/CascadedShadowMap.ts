import * as THREE from 'three';

import { renderToTarget } from 'src/viz/util/renderToTarget';

const _invProj = new THREE.Matrix4();

/**
 * Camera view-frustum corners, split into depth cascades. Ported from three's pure `CSMFrustum`
 * (the addon's type decls are broken, and inlining keeps this self-contained). Verts are in whatever
 * space the source projection/`toSpace` matrix implies (view space from `setFromProjectionMatrix`).
 */
class SplitFrustum {
  readonly vertices = {
    near: [new THREE.Vector3(), new THREE.Vector3(), new THREE.Vector3(), new THREE.Vector3()],
    far: [new THREE.Vector3(), new THREE.Vector3(), new THREE.Vector3(), new THREE.Vector3()],
  };

  setFromProjectionMatrix(projectionMatrix: THREE.Matrix4, maxFar: number) {
    _invProj.copy(projectionMatrix).invert();
    const { near, far } = this.vertices;
    // NDC cube corners (order: 0=+x+y, 1=+x-y, 2=-x-y, 3=-x+y) unprojected to view space.
    near[0].set(1, 1, -1);
    near[1].set(1, -1, -1);
    near[2].set(-1, -1, -1);
    near[3].set(-1, 1, -1);
    far[0].set(1, 1, 1);
    far[1].set(1, -1, 1);
    far[2].set(-1, -1, 1);
    far[3].set(-1, 1, 1);
    for (let i = 0; i < 4; i += 1) {
      near[i].applyMatrix4(_invProj);
      const v = far[i].applyMatrix4(_invProj);
      v.multiplyScalar(Math.min(maxFar / Math.abs(v.z), 1));
    }
  }

  split(breaks: number[], target: SplitFrustum[]) {
    while (breaks.length > target.length) {
      target.push(new SplitFrustum());
    }
    target.length = breaks.length;
    for (let i = 0; i < breaks.length; i += 1) {
      const c = target[i];
      for (let j = 0; j < 4; j += 1) {
        if (i === 0) {
          c.vertices.near[j].copy(this.vertices.near[j]);
        } else {
          c.vertices.near[j].lerpVectors(this.vertices.near[j], this.vertices.far[j], breaks[i - 1]);
        }
        if (i === breaks.length - 1) {
          c.vertices.far[j].copy(this.vertices.far[j]);
        } else {
          c.vertices.far[j].lerpVectors(this.vertices.near[j], this.vertices.far[j], breaks[i]);
        }
      }
    }
  }

  toSpace(matrix: THREE.Matrix4, target: SplitFrustum) {
    for (let i = 0; i < 4; i += 1) {
      target.vertices.near[i].copy(this.vertices.near[i]).applyMatrix4(matrix);
      target.vertices.far[i].copy(this.vertices.far[i]).applyMatrix4(matrix);
    }
  }
}

export interface CascadedShadowMapParams {
  camera: THREE.PerspectiveCamera;
  /** Sun light; only its direction (position→target) is read, refreshed every update. */
  light: THREE.DirectionalLight;
  cascades?: number;
  /** Cap on cascade coverage distance; defaults to `camera.far`. */
  maxDistance?: number;
  /** Split blend: 0 = uniform, 1 = logarithmic. */
  lambda?: number;
  /** Per-cascade shadow map resolution (drives texel-snap granularity). */
  mapSize?: number;
  /** Depth behind the frustum (toward the sun) kept for off-screen casters. */
  lightMargin?: number;
  /** PCF kernel radius in texels. */
  pcfRadius?: number;
  /** Normal-offset bias as a multiple of a cascade's world texel size (acne suppression). */
  normalBias?: number;
}

const _origin = new THREE.Vector3();
const _up = new THREE.Vector3(0, 1, 0);
const _altUp = new THREE.Vector3(0, 0, 1);
const _lightDir = new THREE.Vector3();
const _lightOrient = new THREE.Matrix4();
const _lightOrientInv = new THREE.Matrix4();
const _camToLight = new THREE.Matrix4();
const _lightSpaceFrustum = new SplitFrustum();
const _bbox = new THREE.Box3();
const _center = new THREE.Vector3();
const _casterBox = new THREE.Box3();
const _casterMeshBox = new THREE.Box3();
const _corner = new THREE.Vector3();

/**
 * Custom cascaded shadow map manager: slices the camera view frustum into N depth cascades and fits a
 * rotation-invariant, texel-snapped orthographic camera to each so shadow texel density stays roughly
 * constant in screen space at any scene scale. Owns only the cascade cameras + split/matrix state;
 * depth rendering and shader sampling are layered on in later phases. Cascade math is ported from
 * three's `CSM`/`CSMFrustum` (pure, no WebGL) but with our own cameras instead of N real lights.
 */
export class CascadedShadowMap {
  readonly cascades: number;
  readonly cameras: THREE.OrthographicCamera[] = [];
  /** Per-cascade world→light-clip matrix (`proj * viewInverse`); for shader sampling in P3. */
  readonly lightMatrices: THREE.Matrix4[] = [];
  /** Per-cascade far edge as a positive view-space distance; cascade i spans (splitDistances[i-1], splitDistances[i]]. */
  readonly splitDistances: Float32Array;
  /** Per-cascade world size of one shadow texel (ortho side ÷ mapSize); drives normal-offset bias. */
  readonly texelWorld: Float32Array;
  /** Packed-depth cascade layers (sampler2DArray). Object identity stays stable for uniform sharing. */
  readonly depthRT: THREE.WebGLArrayRenderTarget;

  maxDistance: number;
  lambda: number;
  mapSize: number;
  lightMargin: number;
  pcfRadius: number;
  normalBias: number;

  private readonly camera: THREE.PerspectiveCamera;
  private readonly light: THREE.DirectionalLight;
  private readonly mainFrustum = new SplitFrustum();
  private readonly frustums: SplitFrustum[] = [];
  private readonly breaks: number[] = [];
  private readonly depthMaterial = new THREE.MeshDepthMaterial({ depthPacking: THREE.RGBADepthPacking });
  private readonly hidden: THREE.Object3D[] = [];
  /** World-space corners of the scene's shadow-caster bounds; extends cascade near planes so tall
   *  casters are never clipped out of the depth render (which would pop shadows as cascades re-fit). */
  private casterCorners: THREE.Vector3[] | null = null;

  constructor(params: CascadedShadowMapParams) {
    this.camera = params.camera;
    this.light = params.light;
    this.cascades = params.cascades ?? 3;
    this.maxDistance = params.maxDistance ?? params.camera.far;
    this.lambda = params.lambda ?? 0.5;
    this.mapSize = params.mapSize ?? 2048;
    this.lightMargin = params.lightMargin ?? 150;
    this.pcfRadius = params.pcfRadius ?? 2;
    this.normalBias = params.normalBias ?? 2;
    this.splitDistances = new Float32Array(this.cascades);
    this.texelWorld = new Float32Array(this.cascades);
    this.depthRT = new THREE.WebGLArrayRenderTarget(this.mapSize, this.mapSize, this.cascades);
    this.depthRT.texture.name = 'CSMDepthArray';
    this.depthMaterial.side = THREE.DoubleSide;

    for (let i = 0; i < this.cascades; i += 1) {
      this.cameras.push(new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1));
      this.lightMatrices.push(new THREE.Matrix4());
    }
    this.update();
  }

  private computeBreaks() {
    const near = this.camera.near;
    const far = Math.min(this.camera.far, this.maxDistance);
    const n = this.cascades;
    this.breaks.length = 0;
    for (let i = 1; i < n; i += 1) {
      const uniform = (near + (far - near) * (i / n)) / far;
      const log = (near * (far / near) ** (i / n)) / far;
      this.breaks.push(THREE.MathUtils.lerp(uniform, log, this.lambda));
    }
    this.breaks.push(1);
    for (let i = 0; i < n; i += 1) {
      this.splitDistances[i] = this.breaks[i] * far;
    }
  }

  update() {
    const camera = this.camera;
    camera.updateMatrixWorld();
    camera.updateProjectionMatrix();

    this.computeBreaks();
    this.mainFrustum.setFromProjectionMatrix(camera.projectionMatrix, this.maxDistance);
    this.mainFrustum.split(this.breaks, this.frustums);

    _lightDir.copy(this.light.target.position).sub(this.light.position);
    if (_lightDir.lengthSq() < 1e-12) {
      _lightDir.set(0, -1, 0);
    }
    _lightDir.normalize();
    const up = Math.abs(_lightDir.dot(_up)) > 0.99 ? _altUp : _up;
    _lightOrient.lookAt(_origin, _lightDir, up);
    _lightOrientInv.copy(_lightOrient).invert();
    _camToLight.multiplyMatrices(_lightOrientInv, camera.matrixWorld);

    // Highest scene caster in light space (+z is toward the sun); cascade near planes extend up to
    // this so a caster taller than its cascade slab isn't clipped/culled out of the depth render.
    let casterMaxZ = -Infinity;
    if (this.casterCorners) {
      for (const c of this.casterCorners) {
        casterMaxZ = Math.max(casterMaxZ, _corner.copy(c).applyMatrix4(_lightOrientInv).z);
      }
    }

    for (let i = 0; i < this.cascades; i += 1) {
      const frustum = this.frustums[i];
      const cam = this.cameras[i];
      const nearV = frustum.vertices.near;
      const farV = frustum.vertices.far;

      // Square side = longest sub-frustum diagonal → rotation-invariant, so texel-snap can hold the
      // map steady under small camera moves. Projection onto any plane can't exceed this extent.
      const p1 = farV[0];
      const p2 = p1.distanceTo(farV[2]) > p1.distanceTo(nearV[2]) ? farV[2] : nearV[2];
      const side = p1.distanceTo(p2);
      const texel = side / this.mapSize;
      this.texelWorld[i] = texel;

      frustum.toSpace(_camToLight, _lightSpaceFrustum);
      _bbox.makeEmpty();
      for (let j = 0; j < 4; j += 1) {
        _bbox.expandByPoint(_lightSpaceFrustum.vertices.near[j]);
        _bbox.expandByPoint(_lightSpaceFrustum.vertices.far[j]);
      }
      _bbox.getCenter(_center);
      const nearZ = Math.max(_bbox.max.z, casterMaxZ) + this.lightMargin;
      _center.z = nearZ;
      _center.x = Math.floor(_center.x / texel) * texel;
      _center.y = Math.floor(_center.y / texel) * texel;
      _center.applyMatrix4(_lightOrient);

      cam.up.copy(up);
      cam.position.copy(_center);
      cam.lookAt(_center.x + _lightDir.x, _center.y + _lightDir.y, _center.z + _lightDir.z);
      cam.left = -side / 2;
      cam.right = side / 2;
      cam.top = side / 2;
      cam.bottom = -side / 2;
      cam.near = 0;
      cam.far = nearZ - _bbox.min.z + this.lightMargin;
      cam.updateMatrixWorld(true);
      cam.updateProjectionMatrix();
      this.lightMatrices[i].multiplyMatrices(cam.projectionMatrix, cam.matrixWorldInverse);
    }
  }

  /**
   * Renders scene shadow-caster depth into each cascade layer (packed RGBA). Non-casters are hidden
   * for the pass so they don't self-occlude. Clears each layer to white (= far plane). Call after
   * `update()` and before the main color render.
   */
  renderCascades(renderer: THREE.WebGLRenderer, scene: THREE.Scene) {
    // `overrideMaterial` draws every renderable, so hide anything that isn't a shadow-casting mesh
    // (non-casters, plus lines/points like debug helpers) to keep the depth layers clean.
    scene.traverse(o => {
      const r = o as THREE.Mesh & { isLine?: boolean; isPoints?: boolean };
      if (!o.visible) {
        return;
      }
      const renderable = r.isMesh || r.isLine || r.isPoints;
      if (renderable && !(r.isMesh && r.castShadow)) {
        o.visible = false;
        this.hidden.push(o);
      }
    });
    for (let i = 0; i < this.cascades; i += 1) {
      renderToTarget(renderer, this.depthRT, scene, this.cameras[i], {
        layer: i,
        overrideMaterial: this.depthMaterial,
        clearColor: 0xffffff,
        clearAlpha: 1,
      });
    }
    for (const o of this.hidden) {
      o.visible = true;
    }
    this.hidden.length = 0;
  }

  /**
   * Unions the world-space bounds of the scene's shadow casters (used to extend cascade near planes).
   * Call once after scene geometry is placed; recall if casters move significantly.
   */
  computeCasterBounds(scene: THREE.Scene) {
    scene.updateMatrixWorld();
    _casterBox.makeEmpty();
    scene.traverse(o => {
      const m = o as THREE.Mesh;
      if (!m.isMesh || !m.castShadow) {
        return;
      }
      const g = m.geometry as THREE.BufferGeometry;
      if (!g.boundingBox) {
        g.computeBoundingBox();
      }
      if (g.boundingBox) {
        _casterBox.union(_casterMeshBox.copy(g.boundingBox).applyMatrix4(m.matrixWorld));
      }
    });
    if (_casterBox.isEmpty()) {
      this.casterCorners = null;
      return;
    }
    const { min, max } = _casterBox;
    this.casterCorners ??= Array.from({ length: 8 }, () => new THREE.Vector3());
    for (let i = 0; i < 8; i += 1) {
      this.casterCorners[i].set(i & 1 ? max.x : min.x, i & 2 ? max.y : min.y, i & 4 ? max.z : min.z);
    }
  }

  dispose() {
    this.depthRT.dispose();
    this.depthMaterial.dispose();
  }
}
