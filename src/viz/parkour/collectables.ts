import * as THREE from 'three';

import type { Viz } from 'src/viz';
import type { BulletPhysics, ContactRegion } from 'src/viz/collision';
import { clearPhysicsBindings, withPhysicsContext } from 'src/viz/util/physics';

export class CollectablesCtx {
  public hiddenCollectables: Set<THREE.Object3D> = new Set();

  constructor() {}

  public restore(objs: THREE.Object3D[]) {
    for (const obj of objs) {
      this.hiddenCollectables.delete(obj);
      obj.visible = true;
    }
  }

  public reset() {
    for (const obj of this.hiddenCollectables) {
      obj.visible = true;
    }
    this.hiddenCollectables.clear();
  }
}

interface InitCollectablesArgs {
  viz: Viz;
  loadedWorld: THREE.Group;
  collectableName: string;
  replacementObject?: THREE.Object3D;
  onCollect: (obj: THREE.Object3D) => void;
  material?: THREE.Material | (() => THREE.Material);
  collisionRegionScale?: THREE.Vector3;
  type?: 'mesh' | 'convexHull' | 'aabb';
}

const applyMaterial = (mesh: THREE.Mesh, material: THREE.Material | (() => THREE.Material)) => {
  const mat = typeof material === 'function' ? material() : material;
  mesh.material = mat;
  if (typeof (mat as any).setMesh === 'function') {
    (mat as any).setMesh(mesh);
  }
};

export const initCollectables = ({
  viz,
  loadedWorld,
  collectableName,
  replacementObject,
  onCollect,
  material,
  collisionRegionScale,
  type = 'mesh',
}: InitCollectablesArgs): CollectablesCtx => {
  const collectables: THREE.Object3D[] = [];

  loadedWorld.traverse(obj => {
    if (replacementObject) {
      if (obj.name.includes(collectableName)) {
        const clone = replacementObject.clone();
        clone.name = `clone_${obj.name}`;
        clone.position.copy(obj.position);

        if (material) {
          clone.traverse(obj => {
            if (obj instanceof THREE.Mesh) {
              applyMaterial(obj, material);
            }
          });
        }

        viz.scene.add(clone);
        collectables.push(clone);
      }
    } else {
      if (obj instanceof THREE.Mesh && obj.name.includes(collectableName)) {
        obj.userData.nocollide = true;
        withPhysicsContext(viz, fpCtx => clearPhysicsBindings(obj, fpCtx));

        collectables.push(obj);
        if (material) {
          applyMaterial(obj, material);
        }
      }
    }
  });

  const ctx = new CollectablesCtx();
  const cb = (fpCtx: BulletPhysics) => {
    for (const collectable of collectables) {
      const region: ContactRegion = ((): ContactRegion => {
        if (replacementObject && !(collectable instanceof THREE.Mesh)) {
          if (type !== 'aabb') {
            throw new Error('replacementObject must be a mesh if type is not aabb');
          }

          const halfExtents = new THREE.Vector3();
          collectable.traverse(obj => {
            if (obj instanceof THREE.Mesh) {
              const box = new THREE.Box3().setFromObject(obj);
              const size = new THREE.Vector3();
              box.getSize(size);
              halfExtents.max(size.divideScalar(2));
            }
          });

          return { type: 'box', halfExtents, pos: collectable.position };
        }

        if (!(collectable instanceof THREE.Mesh)) {
          throw new Error('collectable must be a mesh');
        }

        return {
          type,
          mesh: collectable,
          scale: collisionRegionScale,
        };
      })();

      fpCtx.addPlayerRegionContactCb(region, () => {
        if (!ctx.hiddenCollectables.has(collectable)) {
          ctx.hiddenCollectables.add(collectable);
          collectable.visible = false;
          onCollect(collectable);
        }
      });
    }
  };
  if (viz.fpCtx) {
    cb(viz.fpCtx);
  } else {
    viz.collisionWorldLoadedCbs.push(cb);
  }

  return ctx;
};
