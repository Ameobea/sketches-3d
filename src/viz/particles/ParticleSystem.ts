import * as THREE from 'three';

import type { Viz } from 'src/viz';
import type { CustomUniformDef } from 'src/viz/shaders/customShader.types';
import { acquireHeightField } from './HeightField';
import { isExposureGate, ParticleMaterial, type ParticlePipelineCtx } from './particleMaterial';
import type { ParticleSystemDef } from './schema';

export interface ParticleFrame {
  time: number;
  playerPos: THREE.Vector3;
  viewportHeight: number;
}

const buildQuadGeometry = (count: number): THREE.InstancedBufferGeometry => {
  const geo = new THREE.InstancedBufferGeometry();
  geo.setAttribute(
    'position',
    new THREE.Float32BufferAttribute([-0.5, -0.5, 0, 0.5, -0.5, 0, 0.5, 0.5, 0, -0.5, 0.5, 0], 3)
  );
  geo.setAttribute('uv', new THREE.Float32BufferAttribute([0, 0, 1, 0, 1, 1, 0, 1], 2));
  geo.setIndex([0, 1, 2, 0, 2, 3]);
  geo.setAttribute(
    'aSeed',
    new THREE.InstancedBufferAttribute(
      Float32Array.from({ length: count }, (_, i) => i),
      1
    )
  );
  geo.instanceCount = count;
  geo.boundingSphere = new THREE.Sphere(new THREE.Vector3(), Infinity);
  return geo;
};

const shadowMatrix = new THREE.Matrix4();

export class ParticleSystem {
  public readonly def: ParticleSystemDef;
  public readonly mesh: THREE.Mesh;
  private readonly viz: Viz;
  private readonly customUniforms: Record<string, CustomUniformDef>;
  private material: ParticleMaterial | null = null;
  private lights: {
    ambient: (THREE.AmbientLight | THREE.HemisphereLight)[];
    sun: THREE.DirectionalLight | null;
  } | null = null;
  private releaseLease: (() => void) | null;
  private disposed = false;

  constructor(
    viz: Viz,
    def: ParticleSystemDef,
    count: number,
    customUniforms: Record<string, CustomUniformDef>
  ) {
    this.viz = viz;
    this.def = def;
    this.customUniforms = customUniforms;
    this.mesh = new THREE.Mesh(buildQuadGeometry(count));
    this.mesh.frustumCulled = false;
    this.mesh.visible = false;
    this.releaseLease = viz.frameGovernor?.acquireContinuous() ?? null;
  }

  public ensureMaterial(ctx: ParticlePipelineCtx): boolean {
    if (this.material || this.disposed) return !!this.material;
    try {
      this.material = new ParticleMaterial(this.def, this.customUniforms, ctx);
    } catch (err) {
      console.error(err);
      this.disposed = true;
      return false;
    }
    this.mesh.material = this.material;
    this.mesh.visible = true;
    const [sx, , sz] = this.def.volume.size;
    this.def.gates?.forEach((g, i) => {
      if (!isExposureGate(g)) return;
      const field = acquireHeightField(this.viz, {
        direction: g.gate === 'floorExposed' ? 'up' : 'down',
        planeY: g.planeY,
        depth: g.depth ?? 1024,
        extent: g.extent ?? Math.max(sx, sz) * 1.25,
        resolution: g.resolution ?? 1024,
      });
      const u = this.material!.uniforms;
      u[`pFieldMap${i}`] = { value: field.texture };
      u[`pFieldMatrix${i}`] = { value: field.matrix };
      u[`pFieldDepth${i}`] = { value: field.depth };
    });
    if (this.def.light === 'sunGated') {
      const ambient: (THREE.AmbientLight | THREE.HemisphereLight)[] = [];
      let sun: THREE.DirectionalLight | null = null;
      this.viz.scene.traverse(o => {
        if (!(o instanceof THREE.Light) || !o.layers.isEnabled(0)) return;
        if (o instanceof THREE.AmbientLight || o instanceof THREE.HemisphereLight) ambient.push(o);
        else if (o instanceof THREE.DirectionalLight && !sun) sun = o;
      });
      this.lights = { ambient, sun };
    }
    return true;
  }

  public update({ time, playerPos, viewportHeight }: ParticleFrame): void {
    const u = this.material!.uniforms;
    u.pTime.value = time;
    u.pPlayerPos.value.copy(playerPos);
    u.pViewportHeight.value = viewportHeight;
    const cam = this.viz.camera as THREE.PerspectiveCamera;
    (u.pNearFar.value as THREE.Vector2).set(cam.near, cam.far);
    const center = (u.pBoxCenter.value as THREE.Vector3).setFromMatrixPosition(cam.matrixWorld);
    const { volume } = this.def;
    if (volume.kind === 'cameraSlab') {
      center.set(center.x + (volume.offset?.[0] ?? 0), volume.centerY, center.z + (volume.offset?.[1] ?? 0));
    } else if (volume.offset) {
      center.x += volume.offset[0];
      center.y += volume.offset[1];
      center.z += volume.offset[2];
    }
    if (!this.lights) return;

    const ambient = u.pAmbient.value as THREE.Color;
    ambient.setScalar(0);
    for (const l of this.lights.ambient) {
      if (l instanceof THREE.HemisphereLight) {
        ambient.r += (l.color.r + l.groundColor.r) * 0.5 * l.intensity;
        ambient.g += (l.color.g + l.groundColor.g) * 0.5 * l.intensity;
        ambient.b += (l.color.b + l.groundColor.b) * 0.5 * l.intensity;
      } else {
        ambient.r += l.color.r * l.intensity;
        ambient.g += l.color.g * l.intensity;
        ambient.b += l.color.b * l.intensity;
      }
    }
    const { sun } = this.lights;
    (u.pSunColor.value as THREE.Color).setScalar(0);
    if (!sun) return;
    (u.pSunColor.value as THREE.Color).copy(sun.color).multiplyScalar(sun.intensity);
    const map = sun.castShadow ? sun.shadow.map?.texture : null;
    u.pSunShadowOn.value = map ? 1 : 0;
    if (map) {
      u.pSunShadowMap.value = map;
      const cam = sun.shadow.camera;
      (u.pSunShadowMatrix.value as THREE.Matrix4).copy(
        shadowMatrix.multiplyMatrices(cam.projectionMatrix, cam.matrixWorldInverse)
      );
    }
  }

  public dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.mesh.geometry.dispose();
    this.material?.dispose();
    this.releaseLease?.();
    this.releaseLease = null;
  }
}
