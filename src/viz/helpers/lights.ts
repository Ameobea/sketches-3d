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

const AUTO_MIN_EXTENT = 0.05;
const AUTO_MAX_EXTENT = 5000;

const _fitSphere = new THREE.Sphere();
const _fitCorner = new THREE.Vector3();
const _fitView = new THREE.Matrix4();
const _fitDir = new THREE.Vector3();
const _fitUp = new THREE.Vector3(0, 1, 0);
const _fitBox = new THREE.Box3();
const _fitMeshBox = new THREE.Box3();

/**
 * Positions a directional light and fits its orthographic shadow frustum, near/far, and
 * texel-scaled normalBias to `box` (world-space bounds of the shadow casters), preserving the
 * light's incoming direction. The light's position is moved to hug the bounds along that
 * direction, so in auto mode only the light's *direction* (position→target) is meaningful.
 */
export const fitDirectionalShadowFrustumToBox = (light: THREE.DirectionalLight, box: THREE.Box3): void => {
  box.getBoundingSphere(_fitSphere);
  const { center } = _fitSphere;
  const radius = _fitSphere.radius;
  if (!Number.isFinite(radius) || radius <= 0) {
    return;
  }
  const margin = Math.max(radius * 0.05, AUTO_MIN_EXTENT);
  const nearFloor = Math.max(radius * 1e-3, 1e-4);
  const { min, max } = box;

  _fitDir.copy(light.target.position).sub(light.position);
  if (_fitDir.lengthSq() < 1e-12) {
    _fitDir.set(0, -1, 0);
  }
  _fitDir.normalize();
  light.target.position.copy(center);
  light.position.copy(center).addScaledVector(_fitDir, -(radius + margin));

  // Matches THREE's `LightShadow.updateMatrices`: shadow cam at the light, looking at the target.
  _fitView.lookAt(light.position, light.target.position, _fitUp).setPosition(light.position).invert();

  let minX = Infinity,
    minY = Infinity,
    minZ = Infinity;
  let maxX = -Infinity,
    maxY = -Infinity,
    maxZ = -Infinity;
  for (let i = 0; i < 8; i += 1) {
    _fitCorner
      .set(i & 1 ? max.x : min.x, i & 2 ? max.y : min.y, i & 4 ? max.z : min.z)
      .applyMatrix4(_fitView);
    minX = Math.min(minX, _fitCorner.x);
    maxX = Math.max(maxX, _fitCorner.x);
    minY = Math.min(minY, _fitCorner.y);
    maxY = Math.max(maxY, _fitCorner.y);
    minZ = Math.min(minZ, _fitCorner.z);
    maxZ = Math.max(maxZ, _fitCorner.z);
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
};

/**
 * Fits every directional light flagged with `userData.autoShadowFrustum` to the union bounds of
 * the scene's shadow-casting meshes. Run once after geometry is placed and before the shadow map
 * is baked.
 */
export const fitAutoShadowFrustaFromScene = (scene: THREE.Scene, lights: THREE.Light[]): void => {
  const autoLights = lights.filter(
    (l): l is THREE.DirectionalLight =>
      l instanceof THREE.DirectionalLight && l.userData.autoShadowFrustum === true
  );
  if (autoLights.length === 0) {
    return;
  }

  scene.updateMatrixWorld(true);
  _fitBox.makeEmpty();
  scene.traverse(obj => {
    if (!(obj instanceof THREE.Mesh) || !obj.castShadow) {
      return;
    }
    const geom = obj.geometry as THREE.BufferGeometry;
    if (!geom.boundingBox) {
      geom.computeBoundingBox();
    }
    if (geom.boundingBox) {
      _fitBox.union(_fitMeshBox.copy(geom.boundingBox).applyMatrix4(obj.matrixWorld));
    }
  });
  if (_fitBox.isEmpty()) {
    return;
  }

  for (const light of autoLights) {
    fitDirectionalShadowFrustumToBox(light, _fitBox);
  }
};
