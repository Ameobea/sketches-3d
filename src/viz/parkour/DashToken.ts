import * as THREE from 'three';

import type { Viz } from 'src/viz';
import { initCollectables } from './collectables';
import { rwritable } from '../util/TransparentWritable';

export class DashToken extends THREE.Object3D {
  private viz: Viz;
  private base: THREE.Object3D;

  constructor(viz: Viz, base: THREE.Object3D) {
    super();
    this.viz = viz;
    this.base = base;

    this.scale.setScalar(0.9);

    const core = base.children.find(c => c.name.includes('core'))!.clone();
    const rings = base.children.filter(c => c.name.includes('ring')).map(obj => obj.clone()) as THREE.Mesh[];

    // the planes within which the rings are oriented
    const ringInitialPlanes: THREE.Plane[] = [];

    // rads/sec
    const baseRotationSpeed = 2.5;
    const rotationSpeeds: number[] = [];

    // Randomize rotation axis for each ring
    rings.forEach(ring => {
      // rings start out sitting in the XZ plane
      const plane = new THREE.Plane().setFromCoplanarPoints(
        new THREE.Vector3(0, 0, 0),
        new THREE.Vector3(0, 0, 1),
        new THREE.Vector3(1, 0, 0)
      );

      // randomize the plane's normal
      const rotationAxis = new THREE.Vector3(
        Math.random() * 2 - 1,
        Math.random() * 2 - 1,
        Math.random() * 2 - 1
      ).normalize();
      plane.normal.copy(rotationAxis);

      // rotate the ring to match the plane
      const rotationMatrix = new THREE.Matrix4().lookAt(
        new THREE.Vector3(),
        plane.normal,
        new THREE.Vector3(0, 1, 0)
      );
      ring.applyMatrix4(rotationMatrix);

      ringInitialPlanes.push(plane);
      rotationSpeeds.push(0.5 * baseRotationSpeed + baseRotationSpeed * Math.random());
    });

    this.add(core, ...rings);

    let lastBobPhase = 0;
    const bobHeight = 0.2;
    const bobRate = 1.5;

    viz.registerBeforeRenderCb((curTimeSeconds, tDiffSeconds) => {
      const bobPhase = Math.sin(curTimeSeconds * bobRate);
      const bobDelta = bobPhase - lastBobPhase;
      lastBobPhase = bobPhase;

      // bob up and down
      this.position.y += bobDelta * bobHeight;

      rings.forEach((ring, index) => {
        // spin the ring around its plane's normal
        const plane = ringInitialPlanes[index];
        const rotationAxis = plane.normal.clone();

        const rotationSpeed = rotationSpeeds[index];
        ring.rotateOnWorldAxis(rotationAxis, rotationSpeed * tDiffSeconds);
      });
    });
  }

  public override clone(recursive?: boolean): this {
    if (recursive === false) {
      throw new Error('DashToken.clone: clone() without recursive support is not implemented.');
    }

    const clone = new DashToken(this.viz, this.base);
    return clone as this;
  }
}

export const initDashTokenGraphics = (
  loadedWorld: THREE.Group,
  coreMaterial: THREE.Material,
  ringMaterial: THREE.Material
): THREE.Object3D => {
  const base = loadedWorld.getObjectByName('dash_token')!;
  base.visible = false;
  base.traverse(obj => {
    if (!(obj instanceof THREE.Mesh)) {
      return;
    }

    if (obj.name.includes('ring')) {
      obj.material = ringMaterial;
    } else if (obj.name.includes('core')) {
      obj.material = coreMaterial;
    }
  });
  return base;
};

export const initDashTokens = (
  viz: Viz,
  loadedWorld: THREE.Group,
  coreMaterial: THREE.Material,
  ringMaterial: THREE.Material
) => {
  const dashCharges = rwritable(0);
  const base = initDashTokenGraphics(loadedWorld, coreMaterial, ringMaterial);
  const dashTokenBase = new DashToken(viz, base);
  const ctx = initCollectables({
    viz,
    loadedWorld,
    collectableName: 'dash_token_loc',
    replacementObject: dashTokenBase,
    onCollect: () => {
      dashCharges.update(n => n + 1);
      viz.sfxManager.playSfx('dash_pickup');
    },
    type: 'aabb',
  });
  return {
    dashCharges,
    ctx,
    reset: () => {
      ctx.reset();
      dashCharges.set(0);
    },
  };
};
