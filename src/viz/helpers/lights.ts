import * as THREE from 'three';

import { GraphicsQuality } from 'src/viz/conf';

interface ConfigureShadowMapParams {
  light: THREE.DirectionalLight;
  renderer: THREE.WebGLRenderer;
  quality: GraphicsQuality;
  /**
   * Shadow map resolution per quality tier. Default: `{ low: 1024, medium: 2048, high: 4096 }`.
   */
  mapSize?: { low: number; medium: number; high: number };
  /**
   * If true, use VSMShadowMap at medium/high quality (radius 4, blurSamples 16) and fall back
   * to PCFShadowMap at low quality (radius 2, no blur samples).
   * Leave false/omitted for scenes that already use PCF — only the map size will be adjusted.
   */
  useVsm?: boolean;
}

/**
 * Applies quality-scaled shadow map settings to a directional light.
 * Handles map size, renderer shadow type (when useVsm is true), and blur params.
 * Scene-specific values like `bias` should still be set by the caller.
 */
export const configureShadowMap = ({
  light,
  renderer,
  quality,
  mapSize = { low: 1024, medium: 2048, high: 4096 },
  useVsm = false,
}: ConfigureShadowMapParams): void => {
  const size = {
    [GraphicsQuality.Low]: mapSize.low,
    [GraphicsQuality.Medium]: mapSize.medium,
    [GraphicsQuality.High]: mapSize.high,
  }[quality];
  light.shadow.mapSize.width = size;
  light.shadow.mapSize.height = size;

  if (useVsm) {
    if (quality > GraphicsQuality.Low) {
      light.shadow.radius = 4;
      light.shadow.blurSamples = 16;
      renderer.shadowMap.type = THREE.VSMShadowMap;
    } else {
      light.shadow.radius = 2;
      renderer.shadowMap.type = THREE.PCFShadowMap;
    }
  }
};
