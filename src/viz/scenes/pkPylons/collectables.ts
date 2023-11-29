import * as THREE from 'three';

import type { VizState } from 'src/viz';

export class CollectablesCtx {
  public hiddenCollectables: Set<THREE.Mesh> = new Set();

  constructor() {}

  public restore(objs: THREE.Mesh[]) {
    for (const obj of objs) {
      this.hiddenCollectables.delete(obj);
      obj.visible = true;
    }
  }
}

interface InitCollectablesArgs {
  viz: VizState;
  loadedWorld: THREE.Group;
  collectableName: string;
  onCollect: (obj: THREE.Mesh) => void;
  material: THREE.Material;
  collisionRegionScale?: THREE.Vector3;
  type?: 'mesh' | 'convexHull';
}

export const initCollectables = ({
  viz,
  loadedWorld,
  collectableName,
  onCollect,
  material,
  collisionRegionScale,
  type = 'mesh',
}: InitCollectablesArgs): CollectablesCtx => {
  const collectables: THREE.Mesh[] = [];
  loadedWorld.traverse(obj => {
    if (obj instanceof THREE.Mesh && obj.name.includes(collectableName)) {
      obj.userData.nocollide = true;
      collectables.push(obj);
      obj.material = material;
    }
  });

  const ctx = new CollectablesCtx();
  viz.collisionWorldLoadedCbs.push(fpCtx => {
    for (const collectable of collectables) {
      fpCtx.addPlayerRegionContactCb(
        {
          type,
          mesh: collectable,
          scale: collisionRegionScale,
        },
        () => {
          if (!ctx.hiddenCollectables.has(collectable)) {
            ctx.hiddenCollectables.add(collectable);
            collectable.visible = false;
            onCollect(collectable);
          }
        }
      );
    }
  });

  return ctx;
};
