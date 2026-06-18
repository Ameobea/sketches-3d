import * as THREE from 'three';

import { deriveDirectionalShadowNormalBias } from 'src/viz/helpers/lights';
import type { RenderedObject } from 'src/geoscript/runner/types';

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
  /** When set, the frustum + light distance are fit to the scene bbox in `fitAutoShadowFrusta`. */
  auto: boolean;
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
      directionalLight.shadow.mapSize.width = dirLight.shadow_map_size.width;
      directionalLight.shadow.mapSize.height = dirLight.shadow_map_size.height;
      if (dirLight.shadow_camera.auto) {
        // Frustum + light distance are fit to the scene bbox post-render in `fitAutoShadowFrusta`,
        // which also recomputes the texel-scaled normalBias once the extents are known.
        directionalLight.userData.autoShadowFrustum = true;
      } else {
        directionalLight.shadow.camera.near = dirLight.shadow_camera.near;
        directionalLight.shadow.camera.far = dirLight.shadow_camera.far;
        directionalLight.shadow.camera.left = dirLight.shadow_camera.left;
        directionalLight.shadow.camera.right = dirLight.shadow_camera.right;
        directionalLight.shadow.camera.top = dirLight.shadow_camera.top;
        directionalLight.shadow.camera.bottom = dirLight.shadow_camera.bottom;
        // Texel-scaled normalBias so the DoubleSide shadow casting the playground sets on geoscript
        // materials doesn't reintroduce self-shadow acne. Preserves the user-supplied bias.
        deriveDirectionalShadowNormalBias(directionalLight, { bias: dirLight.shadow_map_bias });
      }
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

const AUTO_MIN_EXTENT = 0.05;
const AUTO_MAX_EXTENT = 5000;

const _box = new THREE.Box3();
const _sphere = new THREE.Sphere();
const _meshBox = new THREE.Box3();
const _corner = new THREE.Vector3();
const _view = new THREE.Matrix4();
const _dir = new THREE.Vector3();
const _up = new THREE.Vector3(0, 1, 0);

/**
 * Fits the shadow frustum and light distance of every directional light flagged with
 * `userData.autoShadowFrustum` to the scene's shadow-casting geometry.
 */
export const fitAutoShadowFrusta = (scene: THREE.Scene, renderedObjects: RenderedObject[]): void => {
  const autoLights = renderedObjects.filter(
    (o): o is THREE.DirectionalLight =>
      o instanceof THREE.DirectionalLight && o.userData.autoShadowFrustum === true
  );
  if (autoLights.length === 0) {
    return;
  }

  scene.updateMatrixWorld(true);
  _box.makeEmpty();
  for (const obj of renderedObjects) {
    if (!(obj instanceof THREE.Mesh) || !obj.castShadow) {
      continue;
    }
    const geom = obj.geometry as THREE.BufferGeometry;
    if (!geom.boundingBox) {
      geom.computeBoundingBox();
    }
    if (geom.boundingBox) {
      _box.union(_meshBox.copy(geom.boundingBox).applyMatrix4(obj.matrixWorld));
    }
  }
  if (_box.isEmpty()) {
    return;
  }

  _box.getBoundingSphere(_sphere);
  const center = _sphere.center;
  const radius = _sphere.radius;
  if (!Number.isFinite(radius) || radius <= 0) {
    return;
  }
  const margin = Math.max(radius * 0.05, AUTO_MIN_EXTENT);
  const nearFloor = Math.max(radius * 1e-3, 1e-4);
  const { min, max } = _box;

  for (const light of autoLights) {
    _dir.copy(light.target.position).sub(light.position);
    if (_dir.lengthSq() < 1e-12) {
      _dir.set(0, -1, 0);
    }
    _dir.normalize();
    light.target.position.copy(center);
    light.position.copy(center).addScaledVector(_dir, -(radius + margin));

    // Matches THREE's `LightShadow.updateMatrices`: shadow cam at the light, looking at the target.
    _view.lookAt(light.position, light.target.position, _up).setPosition(light.position).invert();

    let minX = Infinity,
      minY = Infinity,
      minZ = Infinity;
    let maxX = -Infinity,
      maxY = -Infinity,
      maxZ = -Infinity;
    for (let i = 0; i < 8; i += 1) {
      _corner.set(i & 1 ? max.x : min.x, i & 2 ? max.y : min.y, i & 4 ? max.z : min.z).applyMatrix4(_view);
      minX = Math.min(minX, _corner.x);
      maxX = Math.max(maxX, _corner.x);
      minY = Math.min(minY, _corner.y);
      maxY = Math.max(maxY, _corner.y);
      minZ = Math.min(minZ, _corner.z);
      maxZ = Math.max(maxZ, _corner.z);
    }

    const halfW = THREE.MathUtils.clamp((maxX - minX) / 2, AUTO_MIN_EXTENT, AUTO_MAX_EXTENT);
    const halfH = THREE.MathUtils.clamp((maxY - minY) / 2, AUTO_MIN_EXTENT, AUTO_MAX_EXTENT);
    const cx = (minX + maxX) / 2;
    const cy = (minY + maxY) / 2;
    const cam = light.shadow.camera;
    cam.left = cx - halfW;
    cam.right = cx + halfW;
    cam.bottom = cy - halfH;
    cam.top = cy + halfH;
    // Camera looks down -Z, so geometry in front has negative view-space z.
    cam.near = Math.max(-maxZ, nearFloor);
    cam.far = Math.max(-minZ, cam.near + nearFloor);
    cam.updateProjectionMatrix();

    deriveDirectionalShadowNormalBias(light, { bias: light.shadow.bias });
  }
};
