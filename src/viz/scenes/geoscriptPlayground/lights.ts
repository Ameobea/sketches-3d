import * as THREE from 'three';

import { deriveDirectionalShadowNormalBias } from 'src/viz/helpers/lights';

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

// prettier-ignore
type Mat4 = [[number,number,number,number,number,number,number,number,number,number,number,number,number,number,number,number]];

export type Light =
  | { Ambient: [{ color: number; intensity: number }] }
  | {
      Directional: [
        {
          target: [[number, number, number]];
          color: number;
          intensity: number;
          transform: Mat4;
          cast_shadow: boolean;
          shadow_map_size: ShadowMapSize;
          shadow_map_radius: number;
          shadow_map_blur_samples: number;
          shadow_map_type: string;
          shadow_map_bias: number;
          shadow_camera: ShadowCamera;
        },
      ];
    }
  | { Hemisphere: [{ sky_color: number; ground_color: number; intensity: number; transform: Mat4 }] }
  | {
      RectArea: [{ color: number; intensity: number; width: number; height: number; transform: Mat4 }];
    };

enum LightType {
  Ambient,
  Directional,
  Hemisphere,
  RectArea,
}

export const buildLight = (light: Light, renderMode: boolean): THREE.Light => {
  let lightType: LightType;
  if ('Ambient' in light) {
    lightType = LightType.Ambient;
  } else if ('Directional' in light) {
    lightType = LightType.Directional;
  } else if ('Hemisphere' in light) {
    lightType = LightType.Hemisphere;
  } else if ('RectArea' in light) {
    lightType = LightType.RectArea;
  } else {
    throw new Error(`Unknown light type: ${Object.keys(light)[0]}`);
  }

  switch (lightType) {
    case LightType.Ambient: {
      const ambientLight = (light as Extract<Light, { Ambient: any }>).Ambient[0];
      const builtLight = new THREE.AmbientLight(ambientLight.color, ambientLight.intensity);
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
      // Texel-scaled normalBias so the DoubleSide shadow casting the playground sets on geoscript
      // materials doesn't reintroduce self-shadow acne. Preserves the user-supplied bias.
      deriveDirectionalShadowNormalBias(directionalLight, { bias: dirLight.shadow_map_bias });
      directionalLight.applyMatrix4(new THREE.Matrix4().fromArray(dirLight.transform[0]));
      directionalLight.target.position.set(
        dirLight.target[0][0],
        dirLight.target[0][1],
        dirLight.target[0][2]
      );
      return directionalLight;
    }
    case LightType.Hemisphere: {
      const hemiLight = (light as Extract<Light, { Hemisphere: any }>).Hemisphere[0];
      // Non-positional: applying the transform would collapse the sky/ground gradient.
      return new THREE.HemisphereLight(hemiLight.sky_color, hemiLight.ground_color, hemiLight.intensity);
    }
    case LightType.RectArea: {
      const rectLight = (light as Extract<Light, { RectArea: any }>).RectArea[0];
      const builtLight = new THREE.RectAreaLight(
        rectLight.color,
        rectLight.intensity,
        rectLight.width,
        rectLight.height
      );
      // Position + orientation come from the transform; emits from local -Z.
      builtLight.applyMatrix4(new THREE.Matrix4().fromArray(rectLight.transform[0]));
      return builtLight;
    }
  }
};
