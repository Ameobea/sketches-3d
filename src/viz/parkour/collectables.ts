import * as THREE from 'three';

import type { VizState } from 'src/viz';
import type { ContactRegion } from 'src/viz/collision';

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
  viz: VizState;
  loadedWorld: THREE.Group;
  collectableName: string;
  replacementObject?: THREE.Object3D;
  onCollect: (obj: THREE.Object3D) => void;
  material?: THREE.Material;
  collisionRegionScale?: THREE.Vector3;
  type?: 'mesh' | 'convexHull' | 'aabb';
}

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
          if (clone instanceof THREE.Mesh) {
            clone.material = material;
          }
          clone.traverse(obj => {
            if (obj instanceof THREE.Mesh) {
              obj.material = material;
            }
          });
        }

        viz.scene.add(clone);
        collectables.push(clone);
      }
    } else {
      if (obj instanceof THREE.Mesh && obj.name.includes(collectableName)) {
        obj.userData.nocollide = true;
        collectables.push(obj);
        if (material) {
          obj.material = material;
        }
      }
    }
  });

  const ctx = new CollectablesCtx();
  viz.collisionWorldLoadedCbs.push(fpCtx => {
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
  });

  return ctx;
};
