import type * as Comlink from 'comlink';
import * as THREE from 'three';

import type { VizState } from 'src/viz';
import type { VizConfig } from 'src/viz/conf';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import type { SceneConfig } from '../..';
import { getRuneGenWorker } from '../../stone/runeGen/runeGen';
import type { RuneGenCtx } from '../../stone/runeGen/runeGenWorker.worker';

async function renderAABBDebug(runeGenWorker: Comlink.Remote<RuneGenCtx>, scale: number, viz: VizState) {
  const debugAABB = await runeGenWorker.debugAABB();
  if (debugAABB.length % 5 !== 0) {
    throw new Error('debugAABB.length % 5 !== 0');
  }

  // Should handle a depth range -1-20. -1 is a special leaf node.  Anything higher is clamped to 20.
  const colorByDepth = (depth: number) => {
    if (depth < 0) {
      return new THREE.Color(0xff00ff);
    }

    const clampedDepth = Math.min(depth, 20);
    const color = new THREE.Color();
    color.setHSL((1 - clampedDepth / 20) * 0.3, 1, 0.5);
    return color;
  };

  for (let aabbIx = 0; aabbIx < debugAABB.length / 5; aabbIx += 1) {
    const [depth, minx, miny, maxx, maxy] = debugAABB.subarray(aabbIx * 5, (aabbIx + 1) * 5);
    const mat = new THREE.LineBasicMaterial({ color: colorByDepth(depth) });
    const geo = new THREE.BufferGeometry();
    geo.setFromPoints([
      new THREE.Vector3(minx * scale, 0, miny * scale),
      new THREE.Vector3(maxx * scale, 0, miny * scale),
      new THREE.Vector3(maxx * scale, 0, maxy * scale),
      new THREE.Vector3(minx * scale, 0, maxy * scale),
      new THREE.Vector3(minx * scale, 0, miny * scale),
    ]);

    const segs = new THREE.Line(geo, mat);
    viz.scene.add(segs);
  }
}

const initAsync = async (viz: VizState) => {
  const runeGenWorker = await getRuneGenWorker();
  await runeGenWorker.awaitInit();
  const { indices, vertices } = await runeGenWorker.generate();
  if (indices.length % 3 !== 0) {
    throw new Error('indices.length % 3 !== 0');
  }

  const geometry = new THREE.BufferGeometry();
  geometry.setIndex(new THREE.BufferAttribute(indices, 1));
  geometry.setAttribute('position', new THREE.BufferAttribute(vertices, 3));
  geometry.computeVertexNormals();

  const material = buildCustomShader({ side: THREE.DoubleSide, color: new THREE.Color(0xffffff) }, {}, {});
  const mesh = new THREE.Mesh(geometry, material);
  viz.scene.add(mesh);

  // await renderAABBDebug(runeGenWorker, 1, viz);
};

export const processLoadedScene = async (
  viz: VizState,
  loadedWorld: THREE.Group,
  vizConfig: VizConfig
): Promise<SceneConfig> => {
  const dirLight = new THREE.DirectionalLight(0xffffff, 10);
  dirLight.position.set(1000, 1000, 1000);
  dirLight.updateMatrixWorld();
  dirLight.target.position.set(0, 0, 0);
  dirLight.target.updateMatrixWorld();
  viz.scene.add(dirLight);
  viz.scene.add(dirLight.target);
  viz.scene.add(new THREE.AmbientLight(0xffffff, 0.5));

  viz.camera.far = 20_000;
  viz.camera.near = 1;
  initAsync(viz);

  // configureDefaultPostprocessingPipeline(viz, vizConfig.graphics.quality);

  return {
    viewMode: {
      type: 'orbit',
      pos: new THREE.Vector3(181.22753849171932, 138.02448586567655, 27.040261164079567),
      target: new THREE.Vector3(150.4576135164917, -2.576488624641985, -69.31625231265262),
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
