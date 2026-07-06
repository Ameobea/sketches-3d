import * as THREE from 'three';

const _prevClearColor = new THREE.Color();

export interface RenderToTargetOptions {
  /** Array/3D-RT layer to render into (`setRenderTarget(target, layer)`). */
  layer?: number;
  /** Set as `scene.overrideMaterial` for this render, then restored. */
  overrideMaterial?: THREE.Material;
  clearColor?: THREE.ColorRepresentation;
  clearAlpha?: number;
  /** Clear color+depth before rendering (default true). */
  clear?: boolean;
  /** Temporary camera layer mask for this render. */
  cameraLayersMask?: number;
}

/**
 * Renders `scene`/`camera` into `target` (optionally a specific array-RT `layer`), saving and
 * restoring the renderer's active render target + layer, clear color/alpha, `autoClear`, the camera
 * layer mask, and `scene.overrideMaterial`. Pass `renderFn` to replace the default
 * `renderer.render(scene, camera)` body (e.g. per-mesh material swaps). Consolidates the
 * hand-rolled RT-hijack save/restore dance duplicated across the pass/gizmo code.
 */
export const renderToTarget = (
  renderer: THREE.WebGLRenderer,
  target: THREE.WebGLRenderTarget | null,
  scene: THREE.Scene,
  camera: THREE.Camera,
  {
    layer = 0,
    overrideMaterial,
    clearColor,
    clearAlpha = 1,
    clear = true,
    cameraLayersMask,
  }: RenderToTargetOptions = {},
  renderFn?: () => void
): void => {
  const prevTarget = renderer.getRenderTarget();
  const prevCubeFace = renderer.getActiveCubeFace();
  const prevMip = renderer.getActiveMipmapLevel();
  const prevAutoClear = renderer.autoClear;
  const prevCamLayerMask = camera.layers.mask;
  const prevOverride = scene.overrideMaterial;
  const setsClearColor = clearColor !== undefined;
  const prevClearAlpha = renderer.getClearAlpha();
  if (setsClearColor) {
    renderer.getClearColor(_prevClearColor);
    renderer.setClearColor(clearColor, clearAlpha);
  }
  if (cameraLayersMask !== undefined) {
    camera.layers.mask = cameraLayersMask;
  }
  if (overrideMaterial !== undefined) {
    scene.overrideMaterial = overrideMaterial;
  }

  renderer.autoClear = false;
  renderer.setRenderTarget(target, layer, 0);
  if (clear) {
    renderer.clear(true, true, false);
  }
  if (renderFn) {
    renderFn();
  } else {
    renderer.render(scene, camera);
  }

  renderer.setRenderTarget(prevTarget, prevCubeFace, prevMip);
  renderer.autoClear = prevAutoClear;
  camera.layers.mask = prevCamLayerMask;
  scene.overrideMaterial = prevOverride;
  if (setsClearColor) {
    renderer.setClearColor(_prevClearColor, prevClearAlpha);
  }
};
