import * as THREE from 'three';

import type { SceneConfig } from '.';
import type { VizState } from '..';
import { buildCustomShader } from '../shaders/customShader';
import { initBaseScene } from '../util';

const locations = {
  spawn: { pos: new THREE.Vector3(52.7, 1.35, -5.515), rot: new THREE.Vector3(0.51, 1.65, 0) },
};

const PillarVertexShaderFragment = `
  float displacement = sin(curTimeSeconds * 4.) * 0.5 + 0.5;
  displacement = displacement * 4.;
  vec3 newPosition = position + normal * displacement;
  gl_Position = projectionMatrix * modelViewMatrix * vec4( newPosition, 1.0 );
`;

export const processLoadedScene = (viz: VizState, loadedWorld: THREE.Group): SceneConfig => {
  initBaseScene(viz);

  const pillar = loadedWorld.getObjectByName('pillar')! as THREE.Mesh;
  const pillarMaterial = new THREE.ShaderMaterial(
    buildCustomShader({}, { customVertexFragment: PillarVertexShaderFragment }, {})
  );
  pillar.material = pillarMaterial;

  viz.registerBeforeRenderCb(curtimeSeconds => {
    pillarMaterial.uniforms.curTimeSeconds.value = curtimeSeconds;
  });

  return { locations, spawnLocation: 'spawn' };
};
