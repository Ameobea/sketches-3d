import * as THREE from 'three';

import { buildLayerRenderCamera } from 'src/viz/passes/emissiveBypassPass';
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
  private renderCamera: THREE.PerspectiveCamera | THREE.OrthographicCamera;
  private _mainCamera: THREE.PerspectiveCamera | THREE.OrthographicCamera;
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

    this.renderCamera = buildLayerRenderCamera(mainCamera, INLINE_EMISSIVE_LAYER);
  }

  /** Rebind after the scene's camera object is swapped (e.g. ortho/perspective toggle). */
  setMainCamera(camera: THREE.PerspectiveCamera | THREE.OrthographicCamera): void {
    this._mainCamera = camera;
    this.renderCamera = buildLayerRenderCamera(camera, INLINE_EMISSIVE_LAYER);
  }

  addMesh(mesh: THREE.Mesh): void {
    // Re-arm even for already-registered meshes: callers re-add on every scene sync,
    // and lights created since the last sync (live geoscript runs) need layer 30.
    this._lightsSynced = false;
    if (this._registeredMeshes.has(mesh)) return;
    this._registeredMeshes.add(mesh);
    mesh.layers.disable(0);
    mesh.layers.enable(INLINE_EMISSIVE_LAYER);
  }

  /** Return the mesh to the main pass (layer 0). No-op if not registered. */
  removeMesh(mesh: THREE.Mesh): void {
    if (!this._registeredMeshes.delete(mesh)) return;
    mesh.layers.disable(INLINE_EMISSIVE_LAYER);
    mesh.layers.enable(0);
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
    // This render must never re-bake shadow maps — the layer-30 camera would filter
    // the caster set down to inline meshes only, clobbering the scene's maps.
    const savedShadowAutoUpdate = renderer.shadowMap.autoUpdate;
    renderer.shadowMap.autoUpdate = false;
    renderer.render(this.scene, this.renderCamera);
    renderer.shadowMap.autoUpdate = savedShadowAutoUpdate;
    this.scene.background = savedBackground;
  }
}
