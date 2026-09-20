import * as THREE from 'three';

import type { Viz } from 'src/viz';
import { TRANSPARENT_PASS_LAYER } from 'src/viz/passes/transparentPass';
import { renderToTarget } from 'src/viz/util/renderToTarget';

export interface HeightFieldConf {
  /** Which way the map looks from `planeY`. */
  direction: 'up' | 'down';
  planeY: number;
  depth: number;
  /** World size of the square map; must exceed the consuming volume's XZ size by at least `SnapFrac` of itself. */
  extent: number;
  resolution: number;
}

/** Map re-centres in steps of this fraction of its extent, so it re-renders only on a cell crossing. */
const SnapFrac = 0.1;
const Unoccluded = new THREE.Color(1e9, 0, 0);

const DistanceMaterial = new THREE.ShaderMaterial({
  side: THREE.DoubleSide,
  vertexShader: `
    varying float vDist;
    void main() {
      #include <begin_vertex>
      #include <project_vertex>
      vDist = -mvPosition.z;
    }`,
  fragmentShader: `
    varying float vDist;
    void main() { gl_FragColor = vec4(vDist, 0.0, 0.0, 1.0); }`,
});

/**
 * Camera-following 2.5D map of the distance from a horizontal plane to the first static surface
 * along ±Y; a particle is exposed to the plane while it is nearer than that. `matrix` projects world
 * positions into the map: xy → uv, z → distance as a fraction of `depth`.
 */
export class HeightField {
  public readonly matrix = new THREE.Matrix4();
  public readonly depth: number;
  private readonly viz: Viz;
  private readonly rt: THREE.WebGLRenderTarget;
  private readonly camera: THREE.OrthographicCamera;
  private readonly snap: number;
  private readonly center = new THREE.Vector2(NaN, NaN);
  private readonly hidden: THREE.Object3D[] = [];
  private dirty = true;

  constructor(viz: Viz, { direction, planeY, depth, extent, resolution }: HeightFieldConf) {
    this.viz = viz;
    this.depth = depth;
    this.snap = Math.max(1, Math.round(SnapFrac * resolution)) * (extent / resolution);
    this.rt = new THREE.WebGLRenderTarget(resolution, resolution, {
      type: THREE.FloatType,
      format: THREE.RedFormat,
      minFilter: THREE.NearestFilter,
      magFilter: THREE.NearestFilter,
    });
    const h = extent / 2;
    this.camera = new THREE.OrthographicCamera(-h, h, h, -h, 0, depth);
    this.camera.position.y = planeY;
    this.camera.up.set(0, 0, -1);
    this.camera.lookAt(0, planeY + (direction === 'up' ? 1 : -1), 0);
    this.camera.layers.enable(TRANSPARENT_PASS_LAYER);
    viz.registerBeforeRenderCb(this.update);
  }

  get texture() {
    return this.rt.texture;
  }

  /** Re-render on the next frame (static geometry changed). */
  invalidate() {
    this.dirty = true;
  }

  private update = () => {
    const { x, z } = this.viz.camera.position;
    const cx = Math.round(x / this.snap) * this.snap;
    const cz = Math.round(z / this.snap) * this.snap;
    if (!this.dirty && cx === this.center.x && cz === this.center.y) return;
    this.dirty = false;
    this.center.set(cx, cz);
    this.camera.position.setX(cx).setZ(cz);
    this.camera.updateMatrixWorld();
    this.matrix.multiplyMatrices(this.camera.projectionMatrix, this.camera.matrixWorldInverse);

    const { scene, renderer } = this.viz;
    const player = this.viz.sceneConf.player?.mesh;
    scene.traverse(o => {
      const r = o as THREE.Object3D & {
        isLine?: boolean;
        isPoints?: boolean;
        isSprite?: boolean;
        isSkinnedMesh?: boolean;
      };
      if (o.visible && (o === player || r.isLine || r.isPoints || r.isSprite || r.isSkinnedMesh)) {
        o.visible = false;
        this.hidden.push(o);
      }
    });
    // three repaints `scene.background` on every render, which would land in the map
    const background = scene.background;
    scene.background = null;
    renderToTarget(renderer, this.rt, scene, this.camera, {
      overrideMaterial: DistanceMaterial,
      clearColor: Unoccluded,
    });
    scene.background = background;
    for (const o of this.hidden) o.visible = true;
    this.hidden.length = 0;
  };

  dispose() {
    this.viz.unregisterBeforeRenderCb(this.update);
    this.rt.dispose();
  }
}

const fields = new WeakMap<Viz, Map<string, HeightField>>();

export const invalidateHeightFields = (viz: Viz) => fields.get(viz)?.forEach(f => f.invalidate());

/** Systems declaring identical maps share one. */
export const acquireHeightField = (viz: Viz, conf: HeightFieldConf): HeightField => {
  let byKey = fields.get(viz);
  if (!byKey) {
    byKey = new Map();
    fields.set(viz, byKey);
    const owned = byKey;
    viz.registerDestroyedCb(() => owned.forEach(f => f.dispose()));
  }
  const key = JSON.stringify(conf);
  let field = byKey.get(key);
  if (!field) {
    field = new HeightField(viz, conf);
    byKey.set(key, field);
  }
  return field;
};
