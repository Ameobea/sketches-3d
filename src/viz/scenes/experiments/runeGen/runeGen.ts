import * as THREE from 'three';

import type { VizState } from 'src/viz';
import type { SceneConfig } from '../..';
import { getRuneGenWorker } from '../../stone/runeGen/runeGen';

const initAsync = async (viz: VizState) => {
  const runeGenWorker = await getRuneGenWorker();
  await runeGenWorker.awaitInit();
  const { indices, vertices } = await runeGenWorker.generate();
  console.log(indices, vertices);
  if (indices.length % 3 !== 0) {
    throw new Error('indices.length % 3 !== 0');
  }

  // These vertices are 2D.  Create a 3D mesh from them, setting y=0.
  const scale = 1;
  const geometry = new THREE.BufferGeometry();
  geometry.setIndex(new THREE.BufferAttribute(indices, 1));
  const Vertices3D = new Float32Array((vertices.length / 2) * 3);
  for (let i = 0; i < vertices.length; i += 1) {
    Vertices3D[i * 3 + 0] = vertices[i * 2 + 0] * scale;
    Vertices3D[i * 3 + 1] = 0; //i;
    Vertices3D[i * 3 + 2] = vertices[i * 2 + 1] * scale;
  }
  geometry.setAttribute('position', new THREE.BufferAttribute(Vertices3D, 3));

  const material = new THREE.MeshBasicMaterial({ color: 0x00ff00, wireframe: true });
  const mesh = new THREE.Mesh(geometry, material);
  viz.scene.add(mesh);
};

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  initAsync(viz);

  return {
    viewMode: {
      type: 'orbit',
      pos: new THREE.Vector3(100, 100, 100),
      target: new THREE.Vector3(0, 0, 0),
    },
    locations: {
      spawn: {
        pos: new THREE.Vector3(0, 2, 0),
        rot: new THREE.Vector3(),
      },
    },
    spawnLocation: 'spawn',
  };
};
