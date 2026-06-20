import * as THREE from 'three';

import { HijackedMRTPass } from 'src/viz/passes/hijackedMRTPass';

export const INLINE_EMISSIVE_LAYER = 30;

/**
 * Renders `inlineEmissiveBypass` meshes once into a two-output MRT: attachment 0
 * is the live scene-color buffer (`inputBuffer`) and attachment 1 is the shared
 * `emissiveRT`. The material's base surface lands in the main color (tone-mapped,
 * lit, fogged) and its emissive lands in the bypass buffer (skips tone mapping,
 * blooms). POM is marched exactly once because this is the only pass that draws
 * these meshes for color.
 *
 * The MRT attachment hijack + shared stable depth live in `HijackedMRTPass`. On
 * top of that:
 *  - Meshes live on `INLINE_EMISSIVE_LAYER` (off layer 0), so they're skipped by
 *    the depth prepass and main pass. A clone camera renders only that layer.
 *  - The backing MRT shares the composer's stable depth (`setStableDepthTexture`)
 *    so the meshes depth-test/-write against the scene (occlude, self-sort, drive
 *    fog/sky correctly downstream).
 *  - Runs after `EmissiveClearPass` (which zeroes `emissiveRT` each frame) and
 *    before the bloom pass; `needsSwap=false` so FinalPass reads our writes.
 *
 * Lights live on layer 0, so the clone camera would otherwise collect none of them
 * and the meshes would render unlit; `_syncLights` enables `INLINE_EMISSIVE_LAYER`
 * on every layer-0 light once meshes are present.
 */
export class InlineEmissivePass extends HijackedMRTPass {
  public readonly scene: THREE.Scene;
  public readonly emissiveRT: THREE.WebGLRenderTarget;
  private readonly renderCamera: THREE.PerspectiveCamera;
  private readonly _mainCamera: THREE.PerspectiveCamera;
  private readonly _registeredMeshes = new Set<THREE.Mesh>();

  private readonly _frustum = new THREE.Frustum();
  private readonly _projScreenMatrix = new THREE.Matrix4();
  private readonly _sphere = new THREE.Sphere();
  private _lightsSynced = false;

  constructor(
    scene: THREE.Scene,
    mainCamera: THREE.PerspectiveCamera,
    width: number,
    height: number,
    emissiveRT: THREE.WebGLRenderTarget
  ) {
    super('InlineEmissivePass', width, height);
    this.scene = scene;
    this._mainCamera = mainCamera;
    this.emissiveRT = emissiveRT;

    this.renderCamera = mainCamera.clone() as THREE.PerspectiveCamera;
    this.renderCamera.layers.disableAll();
    this.renderCamera.layers.enable(INLINE_EMISSIVE_LAYER);
    this.renderCamera.matrixAutoUpdate = false;
    this.renderCamera.matrixWorldAutoUpdate = false;
    this.renderCamera.matrixWorld = mainCamera.matrixWorld;
    this.renderCamera.matrixWorldInverse = mainCamera.matrixWorldInverse;
    this.renderCamera.projectionMatrix = mainCamera.projectionMatrix;
    this.renderCamera.projectionMatrixInverse = mainCamera.projectionMatrixInverse;
  }

  addMesh(mesh: THREE.Mesh): void {
    if (this._registeredMeshes.has(mesh)) return;
    this._registeredMeshes.add(mesh);
    mesh.layers.disable(0);
    mesh.layers.enable(INLINE_EMISSIVE_LAYER);
    this._lightsSynced = false;
  }

  /** Share the composer's stable depth so meshes depth-test/-write against the scene. */
  setStableDepthTexture(depthTexture: THREE.DepthTexture): void {
    this.attachDepth(depthTexture);
  }

  private _syncLights(): void {
    this.scene.traverse(obj => {
      if (obj instanceof THREE.Light && obj.layers.isEnabled(0)) {
        obj.layers.enable(INLINE_EMISSIVE_LAYER);
      }
    });
  }

  override render(renderer: THREE.WebGLRenderer, inputBuffer: THREE.WebGLRenderTarget): void {
    if (this._registeredMeshes.size === 0) return;

    this._projScreenMatrix.multiplyMatrices(
      this._mainCamera.projectionMatrix,
      this._mainCamera.matrixWorldInverse
    );
    this._frustum.setFromProjectionMatrix(this._projScreenMatrix);
    let anyVisible = false;
    for (const mesh of this._registeredMeshes) {
      if (!mesh.visible) continue;
      if (!mesh.geometry.boundingSphere) mesh.geometry.computeBoundingSphere();
      this._sphere.copy(mesh.geometry.boundingSphere!).applyMatrix4(mesh.matrixWorld);
      if (this._frustum.intersectsSphere(this._sphere)) {
        anyVisible = true;
        break;
      }
    }
    if (!anyVisible) return;

    if (!this._lightsSynced) {
      this._syncLights();
      this._lightsSynced = true;
    }

    if (!this.bindAttachments(renderer, inputBuffer, this.emissiveRT)) return;

    // No clears: EmissiveClearPass already zeroed emissiveRT (att 1), and att 0 holds
    // the live scene color we composite onto. Opaque meshes overwrite both at their
    // pixels via the depth test.
    const savedBackground = this.scene.background;
    this.scene.background = null;
    renderer.render(this.scene, this.renderCamera);
    this.scene.background = savedBackground;
  }
}
