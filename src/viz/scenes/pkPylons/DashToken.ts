import type { VizState } from 'src/viz';
import * as THREE from 'three';

export class DashToken extends THREE.Object3D {
  private viz: VizState;
  private base: THREE.Object3D;

  constructor(viz: VizState, base: THREE.Object3D) {
    super();
    this.viz = viz;
    this.base = base;

    const core = base.children.find(c => c.name.includes('core'))!.clone();
    const rings = base.children.filter(c => c.name.includes('ring')).map(obj => obj.clone()) as THREE.Mesh[];
    const rotationAxes: THREE.Vector3[] = [];

    // Randomize rotation axis for each ring
    rings.forEach((ring, index) => {
      const axis = new THREE.Vector3(
        Math.random() * 2 - 1,
        Math.random() * 2 - 1,
        Math.random() * 2 - 1
      ).normalize();

      rotationAxes[index] = axis;

      ring.quaternion.setFromAxisAngle(axis, Math.random() * 2 * Math.PI);
    });

    this.add(core, ...rings);

    viz.registerBeforeRenderCb((curTimeSeconds, tDiffSeconds) => {
      const rotationSpeed = 2.5;

      rings.forEach((ring, index) => {
        // TODO: this is broken
        const axis = rotationAxes[index];
        const quaternion = new THREE.Quaternion();
        quaternion.setFromAxisAngle(axis, rotationSpeed * tDiffSeconds);
        ring.quaternion.multiplyQuaternions(quaternion, ring.quaternion);
      });
    });
  }

  public override clone(recursive?: boolean): DashToken {
    if (recursive === false) {
      throw new Error('DashToken.clone: clone() without recursive support is not implemented.');
    }

    const clone = new DashToken(this.viz, this.base);
    // clone.copy(this, true);
    return clone;
  }
}
