import type * as Comlink from 'comlink';
import * as THREE from 'three';

import type { Viz } from 'src/viz';
import type { VizConfig } from 'src/viz/conf';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { loadNamedTextures } from 'src/viz/textureLoading';
import type { SceneConfig } from '../..';
import { getRuneGenerator } from '../../stone/runeGen/runeGen';

// import type { RuneGenCtx } from '../../stone/runeGen/RuneGenCtx';

// async function renderAABBDebug(runeGenWorker: Comlink.Remote<RuneGenCtx>, scale: number, viz: VizState) {
//   const debugAABB = await runeGenWorker.debugAABB();
//   if (debugAABB.length % 5 !== 0) {
//     throw new Error('debugAABB.length % 5 !== 0');
//   }

//   // Should handle a depth range -1-20. -1 is a special leaf node.  Anything higher is clamped to 20.
//   const colorByDepth = (depth: number) => {
//     if (depth < 0) {
//       return new THREE.Color(0xff00ff);
//     }

//     const clampedDepth = Math.min(depth, 20);
//     const color = new THREE.Color();
//     color.setHSL((1 - clampedDepth / 20) * 0.3, 1, 0.5);
//     return color;
//   };

//   for (let aabbIx = 0; aabbIx < debugAABB.length / 5; aabbIx += 1) {
//     const [depth, minx, miny, maxx, maxy] = debugAABB.subarray(aabbIx * 5, (aabbIx + 1) * 5);
//     const mat = new THREE.LineBasicMaterial({ color: colorByDepth(depth) });
//     const geo = new THREE.BufferGeometry();
//     geo.setFromPoints([
//       new THREE.Vector3(minx * scale, 0, miny * scale),
//       new THREE.Vector3(maxx * scale, 0, miny * scale),
//       new THREE.Vector3(maxx * scale, 0, maxy * scale),
//       new THREE.Vector3(minx * scale, 0, maxy * scale),
//       new THREE.Vector3(minx * scale, 0, miny * scale),
//     ]);

//     const segs = new THREE.Line(geo, mat);
//     viz.scene.add(segs);
//   }
// }

const initAsync = async (viz: Viz, loadedWorld: THREE.Group) => {
  const runeGenWorker = await getRuneGenerator();

  const loader = new THREE.ImageBitmapLoader();
  const {
    goldTextureAlbedo,
    goldTextureNormal,
    goldTextureRoughness,
    cubesTexture,
    cubesTextureNormal,
    cubesTextureRoughness,
  } = await loadNamedTextures(loader, {
    goldTextureAlbedo: 'https://i.ameo.link/be0.jpg',
    goldTextureNormal: 'https://i.ameo.link/be2.jpg',
    goldTextureRoughness: 'https://i.ameo.link/bdz.jpg',
    cubesTextureRoughness: 'https://i.ameo.link/bew.jpg',
    cubesTextureNormal: 'https://i.ameo.link/bex.jpg',
    cubesTexture: 'https://i.ameo.link/bey.jpg',
  });

  const targetMesh = loadedWorld.getObjectByName('Torus') as THREE.Mesh;
  const targetMat = buildCustomShader(
    {
      map: cubesTexture,
      roughnessMap: cubesTextureRoughness,
      normalMap: cubesTextureNormal,
      color: new THREE.Color(0xaaaaaa),
      uvTransform: new THREE.Matrix3().scale(0.01, 0.01),
      mapDisableDistance: null,
      roughness: 0.9,
    },
    {},
    {
      useTriplanarMapping: true,
    }
  );
  targetMesh.material = targetMat;

  const material = buildCustomShader(
    {
      map: goldTextureAlbedo,
      roughnessMap: goldTextureRoughness,
      normalMap: goldTextureNormal,
      color: new THREE.Color(0xaaaaaa),
      uvTransform: new THREE.Matrix3().scale(0.1, 0.1),
      mapDisableDistance: null,
      roughness: 0.2,
    },
    {},
    {
      useTriplanarMapping: true,
    }
  );

  const mesh = await runeGenWorker.generateMesh(targetMesh, material);
  viz.scene.add(mesh);

  // await renderAABBDebug(runeGenWorker, 1, viz);
};

export const processLoadedScene = async (
  viz: Viz,
  loadedWorld: THREE.Group,
  vizConfig: VizConfig
): Promise<SceneConfig> => {
  const dirLight = new THREE.DirectionalLight(0xbb4242, 1);
  dirLight.position.set(1000, 1000, 1000);
  dirLight.updateMatrixWorld();
  dirLight.target.position.set(0, 0, 0);
  dirLight.target.updateMatrixWorld();
  viz.scene.add(dirLight);
  viz.scene.add(dirLight.target);
  viz.scene.add(new THREE.AmbientLight(0xffffff, 0.5));

  viz.camera.far = 40_000;
  viz.camera.near = 10;
  initAsync(viz, loadedWorld);

  configureDefaultPostprocessingPipeline(viz, vizConfig.graphics.quality);

  return {
    viewMode: {
      type: 'orbit',
      pos: new THREE.Vector3(-225, -19.02, -4.53),
      target: new THREE.Vector3(-52.44, -11.4, 8.3551),
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
