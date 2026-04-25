import * as THREE from 'three';

import type { Viz } from 'src/viz';
import type { BtDashToken } from 'src/ammojs/ammoTypes';
import { rwritable } from '../util/TransparentWritable';
import { clearPhysicsBindings, withPhysicsContext } from '../util/physics';
import type { BulletPhysics } from '../collision';

export class DashToken extends THREE.Object3D {
  private viz: Viz;
  private base: THREE.Object3D;
  /** Tracks whether physics bindings have been cleared from the shared base object. */
  private static clearedBases = new WeakSet<THREE.Object3D>();

  constructor(viz: Viz, base: THREE.Object3D) {
    super();
    this.viz = viz;
    this.base = base;

    this.scale.setScalar(0.9);

    // Only clear physics on the base once — clones share the same base object.
    if (!DashToken.clearedBases.has(base)) {
      DashToken.clearedBases.add(base);
      withPhysicsContext(viz, fpCtx => clearPhysicsBindings(base, fpCtx));
    }

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
): THREE.Object3D | undefined => {
  const base = loadedWorld.getObjectByName('dash_token')!;
  if (!base) {
    return;
  }

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
  ringMaterial: THREE.Material,
  dashCharges = rwritable(0)
) => {
  let base = initDashTokenGraphics(loadedWorld, coreMaterial, ringMaterial);
  if (!base) {
    base = new THREE.Group();
    const core = new THREE.Mesh(new THREE.SphereGeometry(0.5), coreMaterial);
    core.name = 'core';
    base.add(core);

    const ring = new THREE.Mesh(new THREE.TorusGeometry(0.75, 0.1, 8, 24), ringMaterial);
    ring.name = 'ring';
    base.add(ring);
  }

  const dashTokenBase = new DashToken(viz, base);
  const entries: { token: BtDashToken | null; visual: THREE.Object3D; halfExtents: THREE.Vector3 }[] = [];

  const computeHalfExtents = (obj: THREE.Object3D): THREE.Vector3 => {
    const halfExtents = new THREE.Vector3();
    obj.traverse(child => {
      if (!(child instanceof THREE.Mesh)) {
        return;
      }
      const box = new THREE.Box3().setFromObject(child);
      const size = new THREE.Vector3();
      box.getSize(size);
      halfExtents.max(size.divideScalar(2));
    });
    return halfExtents;
  };

  loadedWorld.traverse(obj => {
    if (!obj.name.includes('dash_token_loc')) {
      return;
    }

    const visual = dashTokenBase.clone();
    visual.name = `clone_${obj.name}`;
    visual.position.copy(obj.position);
    viz.scene.add(visual);
    entries.push({
      token: null,
      visual,
      halfExtents: computeHalfExtents(visual),
    });
  });

  const syncFromController = () => {
    const fpCtx = viz.fpCtx;
    if (!fpCtx) {
      return;
    }

    const charges = fpCtx.getDashCharges();
    if (dashCharges.current !== charges) {
      dashCharges.set(charges);
    }
    for (const entry of entries) {
      if (!entry.token) {
        continue;
      }
      const isActive = entry.token.isActive();
      if (entry.visual.visible !== isActive) {
        entry.visual.visible = isActive;
      }
    }
  };

  const registerTokens = (fpCtx: BulletPhysics) => {
    fpCtx.playerController.setDashCharges(dashCharges.current);
    for (const entry of entries) {
      const tokenEntry = fpCtx.addDashToken(
        {
          type: 'box',
          halfExtents: entry.halfExtents,
          pos: entry.visual.position,
        },
        { chargesGranted: 1 },
        () => {
          entry.visual.visible = false;
          viz.sfxManager.playSfx('dash_pickup');
        }
      );
      entry.token = tokenEntry.token;
    }

    fpCtx.captureInitialDashState();
    fpCtx.saveDashCheckpointState();
    syncFromController();
  };
  withPhysicsContext(viz, registerTokens);
  viz.registerBeforeRenderCb(() => {
    syncFromController();
  });

  return {
    syncFromController,
  };
};
