import * as THREE from 'three';

import type { Viz } from 'src/viz';
import type { VizConfig } from 'src/viz/conf';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { loadNamedTextures } from 'src/viz/textureLoading';
import type { SceneConfig } from '..';

// Minimal "grooves" field: periodic horizontal channels along world Y
const GROOVES_HEIGHT_SHADER = /* glsl */ `
float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) {
  float cell = fract(pos.y * 3.0);
  return 1.0 - smoothstep(0.08, 0.42, abs(cell - 0.5));
}
`;

export const processLoadedScene = async (
  viz: Viz,
  _loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  viz.camera.near = 0.1;
  viz.camera.far = 2000;
  viz.camera.updateProjectionMatrix();

  viz.scene.background = new THREE.Color(0x223044);
  viz.scene.add(new THREE.AmbientLight(0xffffff, 0.35));
  const sun = new THREE.DirectionalLight(0xffffff, 2.2);
  sun.position.set(8, 6, 10);
  sun.castShadow = false;
  viz.scene.add(sun);

  const loader = new THREE.ImageBitmapLoader();
  const { diffuse } = await loadNamedTextures(loader, {
    diffuse: 'https://i.ameo.link/amf.png',
  });

  const pomMat = buildCustomShader(
    {
      map: diffuse,
      color: 0x9b8f7e,
      roughness: 0.85,
      metalness: 0,
      uvTransform: new THREE.Matrix3().scale(0.2, 0.2),
      mapDisableDistance: null,
    },
    { pomHeightShader: GROOVES_HEIGHT_SHADER },
    {
      useTriplanarMapping: true,
      pom: {
        depth: 0.15,
        steps: 24,
        lodFadeStart: 400,
        lodFadeRange: 100,
        boundedSilhouette: true,
      },
    }
  );
  viz.registerBeforeRenderCb(curTimeSeconds => pomMat.setCurTimeSeconds(curTimeSeconds));

  const ground = new THREE.Mesh(
    new THREE.BoxGeometry(60, 1, 60),
    new THREE.MeshStandardMaterial({ color: 0x3a3a3a, roughness: 0.9 })
  );
  ground.position.set(0, -0.5, 0);
  viz.scene.add(ground);

  const wall = new THREE.Mesh(new THREE.BoxGeometry(10, 7, 1.5), pomMat);
  wall.position.set(-4, 3.5, 0);
  viz.scene.add(wall);

  const sphere = new THREE.Mesh(new THREE.SphereGeometry(2.5, 64, 48), pomMat);
  sphere.position.set(6, 3.5, 0);
  viz.scene.add(sphere);

  const sphere2 = new THREE.Mesh(new THREE.SphereGeometry(2.0, 64, 48), pomMat);
  sphere2.position.set(6.5, 4.4, -2.4);
  viz.scene.add(sphere2);

  configureDefaultPostprocessingPipeline({
    viz,
    quality: vizConf.graphics.quality,
    pomExitBuffers: true,
  });

  return {
    locations: {
      spawn: {
        pos: new THREE.Vector3(0, 4, 12),
        rot: new THREE.Vector3(0, 0, 0),
      },
    },
    spawnLocation: 'spawn',
    viewMode: {
      type: 'orbit',
      pos: new THREE.Vector3(2, 5, 14),
      target: new THREE.Vector3(0, 3.5, 0),
    },
  };
};
