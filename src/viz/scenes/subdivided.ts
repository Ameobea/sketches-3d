import * as THREE from 'three';

import type { SceneConfig } from '.';
import type { VizState } from '..';
import { buildCustomShader } from '../shaders/customShader';
import { initBaseScene } from '../util';
import pillarColorShader from '../shaders/subdivided/pillar/color.frag?raw';
import pillarRoghnessShader from '../shaders/subdivided/pillar/roughness.frag?raw';
import PillarVertexShaderFragment from '../shaders/subdivided/pillar/displacement.vert?raw';
import groundRoughnessShader from '../shaders/subdivided/ground/roughness.frag?raw';

const locations = {
  spawn: { pos: new THREE.Vector3(52.7, 1.35, -5.515), rot: new THREE.Vector3(0.51, 1.65, 0) },
};

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  const baseScene = initBaseScene(viz);
  baseScene.light.color = new THREE.Color(0xffffff);
  baseScene.light.intensity = 0.6;
  baseScene.ambientlight.intensity = 5.4;

  const pointLight = new THREE.PointLight(0x228888, 8, 100);
  pointLight.position.set(-40, 18, 15);
  viz.scene.add(pointLight);
  const pointLightCube = new THREE.Mesh(
    new THREE.BoxGeometry(1, 1, 1),
    new THREE.MeshBasicMaterial({ color: 0x228888 })
  );
  pointLightCube.position.copy(pointLight.position);
  viz.scene.add(pointLightCube);

  const ground = loadedWorld.getObjectByName('ground001')! as THREE.Mesh;
  const groundMaterial = new THREE.ShaderMaterial(
    buildCustomShader(
      { color: new THREE.Color(0x121266), metalness: 0.95, roughness: 0.2 },
      { roughnessShader: groundRoughnessShader },
      { antialiasRoughnessShader: true }
    )
  );
  ground.material = groundMaterial;

  const loader = new THREE.ImageBitmapLoader();
  const texture = await new Promise<THREE.Texture>((resolve, reject) =>
    loader.load(
      'https://ameo.link/u/aa8.png',
      imageBitmap => {
        console.log('loaded');
        const texture = new THREE.CanvasTexture(
          imageBitmap,
          THREE.UVMapping,
          THREE.RepeatWrapping,
          THREE.RepeatWrapping,
          THREE.LinearFilter,
          THREE.LinearFilter
        );
        texture.needsUpdate = true;
        resolve(texture);
      },
      undefined,
      reject
    )
  );
  loader.manager = new THREE.LoadingManager();
  const normalTexture = await new Promise<THREE.Texture>((resolve, reject) =>
    loader.load(
      'https://ameo.link/u/aaa.png',
      imageBitmap => {
        console.log('loaded normal map');
        const texture = new THREE.CanvasTexture(
          imageBitmap,
          THREE.UVMapping,
          THREE.RepeatWrapping,
          THREE.RepeatWrapping,
          THREE.LinearFilter,
          THREE.LinearFilter,
          THREE.RGBAFormat
        );
        texture.needsUpdate = true;
        resolve(texture);
      },
      undefined,
      reject
    )
  );

  const pillar = loadedWorld.getObjectByName('pillar')! as THREE.Mesh;
  const pillarMaterial = new THREE.ShaderMaterial(
    buildCustomShader(
      { metalness: 0.96, normalScale: 2, color: new THREE.Color(0xffffff) },
      {
        customVertexFragment: PillarVertexShaderFragment,
        colorShader: pillarColorShader,
        roughnessShader: pillarRoghnessShader,
      },
      {}
    )
  );
  pillarMaterial.map = texture;
  pillarMaterial.normalMap = normalTexture;
  // pillarMaterial.normalScale = new THREE.Vector2(8, 8);
  pillarMaterial.normalMapType = THREE.TangentSpaceNormalMap;
  pillarMaterial.uniforms.map.value = texture;
  pillarMaterial.uniforms.normalMap.value = normalTexture;
  const uvTransformMat = new THREE.Matrix3().scale(3, 4);
  pillarMaterial.uniforms.uvTransform.value = uvTransformMat;
  // pillarMaterial.uniforms.normalScale.value = new THREE.Vector2(1, 1);
  pillarMaterial.needsUpdate = true;
  pillar.material = pillarMaterial;
  pillar.scale.set(0.8, 0.6, 0.8);
  // pillar.geometry = new THREE.SphereGeometry(40, 64, 64);
  pillar.geometry = new THREE.CylinderGeometry(20, 20, 220, 64, 64);
  pillar.rotation.x = 0.2;

  viz.registerBeforeRenderCb(curTimeSeconds => {
    pillarMaterial.uniforms.curTimeSeconds.value = curTimeSeconds;
    groundMaterial.uniforms.curTimeSeconds.value = curTimeSeconds;
    pillar.rotation.y = curTimeSeconds * 0.4;

    // rOTATE point light about the pillar
    pointLight.position.x = Math.sin(curTimeSeconds * -0.3) * 20;
    pointLight.position.z = Math.cos(curTimeSeconds * -0.3) * 20;
    pointLightCube.position.copy(pointLight.position);
  });

  return { locations, spawnLocation: 'spawn' };
};
