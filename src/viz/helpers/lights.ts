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

interface DeriveShadowNormalBiasParams {
  /**
   * Multiplier on the shadow texel's world size. ~1.5 is a good default; raise toward 2 for a
   * safety margin, lower if the residual offset reopens a visible contact gap.
   */
  texelMultiplier?: number;
  /**
   * Constant depth bias to set alongside. If omitted, the light's existing `bias` is left
   * untouched (used by the auto-default path so it never stomps a scene's deliberate bias).
   * With `DoubleSide` casting there is no contact gap left for a positive `bias` to fight.
   */
  bias?: number;
}

/**
 * Derives `shadow.normalBias` from the directional light's shadow-map texel world size
 * (`orthoFrustumExtent / mapSize`) — the scale at which front/double-side self-shadow acne
 * appears — so one multiplier stays robust across scenes and surface slopes.  Pair with
 * `DoubleSide` shadow casting (`setShadowCastSide` in customShader).  Returns the computed
 * `normalBias`.
 */
export const deriveDirectionalShadowNormalBias = (
  light: THREE.DirectionalLight,
  { texelMultiplier = 1.65, bias }: DeriveShadowNormalBiasParams = {}
): number => {
  const cam = light.shadow.camera;
  const texelWorld = Math.max(
    Math.abs(cam.right - cam.left) / light.shadow.mapSize.width,
    Math.abs(cam.top - cam.bottom) / light.shadow.mapSize.height
  );
  const normalBias = texelMultiplier * texelWorld;
  light.shadow.normalBias = normalBias;
  if (bias !== undefined) {
    light.shadow.bias = bias;
  }
  return normalBias;
};
