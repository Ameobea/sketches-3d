import type { Viz } from 'src/viz';
import * as THREE from 'three';

export interface ShadowMapSize {
  width: number;
  height: number;
}

export interface ShadowCamera {
  near: number;
  far: number;
  left: number;
  right: number;
  top: number;
  bottom: number;
}

export type Light =
  | { Ambient: [{ color: number; intensity: number }] }
  | {
      Directional: [
        {
          target: [[number, number, number]];
          color: number;
          intensity: number;
          // prettier-ignore
          transform: [[number,number,number,number,number,number,number,number,number,number,number,number,number,number,number,number]];
          cast_shadow: boolean;
          shadow_map_size: ShadowMapSize;
          shadow_map_radius: number;
          shadow_map_blur_samples: number;
          shadow_map_type: string;
          shadow_map_bias: number;
          shadow_camera: ShadowCamera;
        },
      ];
    };

enum LightType {
  Ambient,
  Directional,
}

export const buildAndAddLight = (viz: Viz, light: Light, renderMode: boolean): THREE.Light => {
  let lightType: LightType;
  if ('Ambient' in light) {
    lightType = LightType.Ambient;
  } else if ('Directional' in light) {
    lightType = LightType.Directional;
  } else {
    throw new Error(`Unknown light type: ${Object.keys(light)[0]}`);
  }

  switch (lightType) {
    case LightType.Ambient: {
      const ambientLight = (light as Extract<Light, { Ambient: any }>).Ambient[0];
      const builtLight = new THREE.AmbientLight(ambientLight.color, ambientLight.intensity);
      viz.scene.add(builtLight);
      return builtLight;
    }
    case LightType.Directional: {
      const dirLight = (light as Extract<Light, { Directional: any }>).Directional[0];
      const directionalLight = new THREE.DirectionalLight(dirLight.color, dirLight.intensity);
      directionalLight.castShadow = dirLight.cast_shadow;
      let mapWidth = dirLight.shadow_map_size.width;
      let mapHeight = dirLight.shadow_map_size.height;
      if (renderMode) {
        mapWidth = Math.min(512, mapWidth);
        mapHeight = Math.min(512, mapHeight);
      }
      directionalLight.shadow.mapSize.set(mapWidth, mapHeight);
      directionalLight.shadow.radius = dirLight.shadow_map_radius;
      directionalLight.shadow.blurSamples = dirLight.shadow_map_blur_samples;
      directionalLight.shadow.bias = dirLight.shadow_map_bias;
      directionalLight.shadow.camera.near = dirLight.shadow_camera.near;
      directionalLight.shadow.camera.far = dirLight.shadow_camera.far;
      directionalLight.shadow.camera.left = dirLight.shadow_camera.left;
      directionalLight.shadow.camera.right = dirLight.shadow_camera.right;
      directionalLight.shadow.camera.top = dirLight.shadow_camera.top;
      directionalLight.shadow.camera.bottom = dirLight.shadow_camera.bottom;
      directionalLight.shadow.mapSize.width = dirLight.shadow_map_size.width;
      directionalLight.shadow.mapSize.height = dirLight.shadow_map_size.height;
      directionalLight.applyMatrix4(new THREE.Matrix4().fromArray(dirLight.transform[0]));
      directionalLight.target.position.set(
        dirLight.target[0][0],
        dirLight.target[0][1],
        dirLight.target[0][2]
      );
      viz.scene.add(directionalLight);
      viz.scene.add(directionalLight.target);
      return directionalLight;
    }
  }
};
