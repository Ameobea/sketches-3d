import * as THREE from 'three';

import { GraphicsQuality } from 'src/viz/conf';

import type { LevelLight } from './levelSceneTypes';
import type { LightDef, ShadowConfigDef, ShadowMapSize } from './types';

const resolveShadowMapSize = (size: ShadowMapSize, quality: GraphicsQuality): number => {
  if (typeof size === 'number') return size;
  switch (quality) {
    case GraphicsQuality.Low:
      return size.low;
    case GraphicsQuality.Medium:
      return size.medium;
    case GraphicsQuality.High:
      return size.high;
  }
};

/**
 * Applies persisted shadow settings before the shadow map is first rendered.
 * This is also safe to call when toggling `castShadow` back on for a light
 * that already has shadow config in its def.
 */
const applyShadowConfig = (
  light: THREE.DirectionalLight | THREE.PointLight | THREE.SpotLight,
  cfg: ShadowConfigDef,
  quality: GraphicsQuality
): void => {
  if (cfg.mapSize !== undefined) {
    const size = resolveShadowMapSize(cfg.mapSize, quality);
    light.shadow.mapSize.set(size, size);
  }
  if (cfg.bias !== undefined) light.shadow.bias = cfg.bias;
  if (cfg.normalBias !== undefined) light.shadow.normalBias = cfg.normalBias;
  if (cfg.radius !== undefined) light.shadow.radius = cfg.radius;
  if (cfg.blurSamples !== undefined) light.shadow.blurSamples = cfg.blurSamples;

  const cam = light.shadow.camera;
  if (cfg.near !== undefined) cam.near = cfg.near;
  if (cfg.far !== undefined) cam.far = cfg.far;
  if (light instanceof THREE.DirectionalLight && cam instanceof THREE.OrthographicCamera) {
    if (cfg.left !== undefined) cam.left = cfg.left;
    if (cfg.right !== undefined) cam.right = cfg.right;
    if (cfg.top !== undefined) cam.top = cfg.top;
    if (cfg.bottom !== undefined) cam.bottom = cfg.bottom;
  }
  cam.updateProjectionMatrix();
};

const instantiateThreeLightFromDef = (
  def: LightDef,
  quality: GraphicsQuality
): { light: THREE.Light; target?: THREE.Object3D } => {
  switch (def.type) {
    case 'ambient':
      return { light: new THREE.AmbientLight(def.color ?? 0xffffff, def.intensity ?? 1) };
    case 'directional': {
      const light = new THREE.DirectionalLight(def.color ?? 0xffffff, def.intensity ?? 1);
      if (def.position) light.position.fromArray(def.position);
      if (def.target) light.target.position.fromArray(def.target);
      if (def.castShadow) {
        light.castShadow = true;
        if (def.shadow) applyShadowConfig(light, def.shadow, quality);
      }
      return { light, target: light.target };
    }
    case 'point': {
      const light = new THREE.PointLight(
        def.color ?? 0xffffff,
        def.intensity ?? 1,
        def.distance ?? 0,
        def.decay ?? 2
      );
      if (def.position) light.position.fromArray(def.position);
      if (def.castShadow) {
        light.castShadow = true;
        if (def.shadow) applyShadowConfig(light, def.shadow, quality);
      }
      return { light };
    }
    case 'spot': {
      const light = new THREE.SpotLight(
        def.color ?? 0xffffff,
        def.intensity ?? 1,
        def.distance ?? 0,
        def.angle ?? Math.PI / 4,
        def.penumbra ?? 0,
        def.decay ?? 2
      );
      if (def.position) light.position.fromArray(def.position);
      if (def.target) light.target.position.fromArray(def.target);
      if (def.castShadow) {
        light.castShadow = true;
        if (def.shadow) applyShadowConfig(light, def.shadow, quality);
      }
      return { light, target: light.target };
    }
  }
};

export const createLevelLight = (def: LightDef, quality: GraphicsQuality): LevelLight => ({
  id: def.id,
  def,
  ...instantiateThreeLightFromDef(def, quality),
});

export const addLevelLightToScene = (scene: THREE.Scene, levelLight: LevelLight): void => {
  scene.add(levelLight.light);
  if (levelLight.target) scene.add(levelLight.target);
};

export const removeLevelLightFromScene = (scene: THREE.Scene, levelLight: LevelLight): void => {
  scene.remove(levelLight.light);
  if (levelLight.target) scene.remove(levelLight.target);
};

export const applyLightDefToLevelLight = (levelLight: LevelLight, quality: GraphicsQuality): void => {
  const { def, light } = levelLight;
  light.name = def.id;

  switch (def.type) {
    case 'ambient': {
      if (!(light instanceof THREE.AmbientLight)) return;
      light.color.setHex(def.color ?? 0xffffff);
      light.intensity = def.intensity ?? 1;
      return;
    }
    case 'directional': {
      if (!(light instanceof THREE.DirectionalLight)) return;
      levelLight.target ??= light.target;
      light.color.setHex(def.color ?? 0xffffff);
      light.intensity = def.intensity ?? 1;
      if (def.position) light.position.fromArray(def.position);
      if (def.target) levelLight.target.position.fromArray(def.target);
      light.castShadow = def.castShadow ?? false;
      if (light.castShadow && def.shadow) applyShadowConfig(light, def.shadow, quality);
      return;
    }
    case 'point': {
      if (!(light instanceof THREE.PointLight)) return;
      light.color.setHex(def.color ?? 0xffffff);
      light.intensity = def.intensity ?? 1;
      if (def.position) light.position.fromArray(def.position);
      light.distance = def.distance ?? 0;
      light.decay = def.decay ?? 2;
      light.castShadow = def.castShadow ?? false;
      if (light.castShadow && def.shadow) applyShadowConfig(light, def.shadow, quality);
      return;
    }
    case 'spot': {
      if (!(light instanceof THREE.SpotLight)) return;
      levelLight.target ??= light.target;
      light.color.setHex(def.color ?? 0xffffff);
      light.intensity = def.intensity ?? 1;
      if (def.position) light.position.fromArray(def.position);
      if (def.target) levelLight.target.position.fromArray(def.target);
      light.distance = def.distance ?? 0;
      light.decay = def.decay ?? 2;
      light.angle = def.angle ?? Math.PI / 4;
      light.penumbra = def.penumbra ?? 0;
      light.castShadow = def.castShadow ?? false;
      if (light.castShadow && def.shadow) applyShadowConfig(light, def.shadow, quality);
      return;
    }
  }
};
