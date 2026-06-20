import { Pass } from 'postprocessing';
import * as THREE from 'three';

export const EMISSIVE_BYPASS_LAYER = 31;

/**
 * Composites registered bypass meshes onto a dedicated RGBA HalfFloat render target
 * (`emissiveRT`), which `EmissiveClearPass` clears ahead of it each frame.
 * `FinalPass` composites the result after tone mapping with only sRGB encoding,
 * preserving vivid saturated colors that AgX would desaturate.
 *
 * Bypass meshes are excluded from the main render via layer mask (layer 0
 * disabled, layer 31 enabled) (the main camera renders only layer 0 by default).
 * The `bypassCamera` here renders only layer 31.
 *
 * The depth buffer is shared with the main render, so depth writes here will properly
 * cause emissive bypass meshes to occlude and be occluded by main-scene meshes.
 */
export class EmissiveBypassPass extends Pass {
  private readonly bypassCamera: THREE.PerspectiveCamera;
  readonly emissiveRT: THREE.WebGLRenderTarget;
  /**
   * When true, another subsystem (e.g. `SkyStackPass`) owns `emissiveRT` and is
   * responsible for allocation, resize, and depth wiring. This pass only composites
   * bypass meshes on top; clearing is `EmissiveClearPass`'s job either way.
   */
  private readonly rtIsExternal: boolean;
  private readonly _mainCamera: THREE.PerspectiveCamera;
  public readonly scene: THREE.Scene;
  private readonly _registeredMeshes = new Set<THREE.Mesh>();

  private readonly _frustum = new THREE.Frustum();
  private readonly _projScreenMatrix = new THREE.Matrix4();
  private readonly _sphere = new THREE.Sphere();

  constructor(
    scene: THREE.Scene,
    mainCamera: THREE.PerspectiveCamera,
    width: number,
    height: number,
    externalEmissiveRT?: THREE.WebGLRenderTarget
  ) {
    super('EmissiveBypassPass');
    this.scene = scene;
    this._mainCamera = mainCamera;
    this.needsSwap = false;

    this.bypassCamera = mainCamera.clone() as THREE.PerspectiveCamera;
    this.bypassCamera.layers.disableAll();
    this.bypassCamera.layers.enable(EMISSIVE_BYPASS_LAYER);
    this.bypassCamera.matrixAutoUpdate = false;
    this.bypassCamera.matrixWorldAutoUpdate = false;
    this.bypassCamera.matrixWorld = mainCamera.matrixWorld;
    this.bypassCamera.matrixWorldInverse = mainCamera.matrixWorldInverse;
    this.bypassCamera.projectionMatrix = mainCamera.projectionMatrix;
    this.bypassCamera.projectionMatrixInverse = mainCamera.projectionMatrixInverse;

    if (externalEmissiveRT) {
      this.emissiveRT = externalEmissiveRT;
      this.rtIsExternal = true;
    } else {
      this.emissiveRT = new THREE.WebGLRenderTarget(width, height, {
        type: THREE.HalfFloatType,
        format: THREE.RGBAFormat,
        depthBuffer: false,
      });
      this.rtIsExternal = false;
    }
  }

  addBypassMesh(mesh: THREE.Mesh): void {
    if (this._registeredMeshes.has(mesh)) return;
    this._registeredMeshes.add(mesh);
    mesh.layers.disable(0);
    mesh.layers.enable(EMISSIVE_BYPASS_LAYER);
  }

  /**
   * Attach an externally-owned depth texture (the composer's stable depth)
   * as `emissiveRT`'s depth attachment. MUST be called before the first
   * render — three.js sets up the FBO lazily on the first
   * `setRenderTarget(emissiveRT)`. No-op when an external RT is in use
   * (SkyStack already wired its own depth).
   */
  setStableDepthTexture(depthTexture: THREE.DepthTexture): void {
    if (this.rtIsExternal) return;
    this.emissiveRT.depthTexture = depthTexture;
    this.emissiveRT.depthBuffer = true;
  }

  override setSize(width: number, height: number): void {
    if (!this.rtIsExternal) {
      this.emissiveRT.setSize(width, height);
    }
  }

  override render(
    renderer: THREE.WebGLRenderer,
    _inputBuffer: THREE.WebGLRenderTarget,
    _outputBuffer: THREE.WebGLRenderTarget
  ): void {
    if (this._registeredMeshes.size === 0) return;

    // skip the render if no bypass mesh is within the camera frustum
    this._projScreenMatrix.multiplyMatrices(
      this._mainCamera.projectionMatrix,
      this._mainCamera.matrixWorldInverse
    );
    this._frustum.setFromProjectionMatrix(this._projScreenMatrix);
    let anyVisible = false;
    for (const mesh of this._registeredMeshes) {
      if (!mesh.visible) continue;
      if (!mesh.geometry.boundingSphere) mesh.geometry.computeBoundingSphere();
      this._sphere.copy(mesh.geometry.boundingSphere!);
      this._sphere.applyMatrix4(mesh.matrixWorld);
      if (this._frustum.intersectsSphere(this._sphere)) {
        anyVisible = true;
        break;
      }
    }
    if (!anyVisible) return;

    renderer.setRenderTarget(this.emissiveRT);
    const savedBackground = this.scene.background;
    this.scene.background = null;
    renderer.render(this.scene, this.bypassCamera);
    this.scene.background = savedBackground;
  }

  override dispose(): void {
    if (!this.rtIsExternal) {
      this.emissiveRT.dispose();
    }
    super.dispose();
  }
}
