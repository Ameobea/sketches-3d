import { Pass } from 'postprocessing';
import * as THREE from 'three';

/**
 * Layer bit reserved for objects that should bypass AgX/filmic tone mapping.
 */
export const EMISSIVE_BYPASS_LAYER = 31;

interface BypassEntry {
  mesh: THREE.Mesh;
  realMat: THREE.Material | THREE.Material[];
  proxyMat: THREE.MeshBasicMaterial;
}

/**
 * Renders registered bypass meshes into a dedicated RGBA HalfFloat render target
 * (emissiveRT). The FinalPass composites this after tone mapping with only sRGB
 * encoding, preserving vivid saturated colors that AgX would desaturate.
 *
 * Each registered mesh gets a depth-proxy material (`colorWrite: false,
 * depthWrite: true`) for the main render pass. This keeps the mesh invisible to
 * the main pipeline (no AgX, no double-counting in bloom) while still writing
 * depth so volumetric fog and other depth-dependent effects terminate correctly
 * at the mesh boundary.
 *
 * During render(), materials are temporarily swapped to the real materials so the
 * bypass camera captures the correct emissive appearance.
 *
 * needsSwap = false — writes to its own RT, not the ping-pong pair.
 */
export class EmissiveBypassPass extends Pass {
  private readonly bypassCamera: THREE.PerspectiveCamera;
  readonly emissiveRT: THREE.WebGLRenderTarget;
  /**
   * When true, another subsystem (e.g. SkyStackPass) owns emissiveRT and is
   * responsible for allocation, resize, depth blit, and per-frame clear. This
   * pass then only renders bypass meshes on top of the existing contents.
   */
  private readonly rtIsExternal: boolean;
  private stableDepthTarget: THREE.WebGLRenderTarget | null = null;
  private readonly _mainCamera: THREE.PerspectiveCamera;
  public readonly scene: THREE.Scene;
  private readonly bypassEntries: BypassEntry[] = [];
  private readonly _registeredMeshes = new Set<THREE.Mesh>();

  // Cached objects to avoid per-frame allocation in the frustum visibility check.
  private readonly _frustum = new THREE.Frustum();
  private readonly _projScreenMatrix = new THREE.Matrix4();
  private readonly _sphere = new THREE.Sphere();
  private _emissiveRTHasContent = false;

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
    // Prevent Three.js from recomputing matrices from position/rotation each frame.
    // We manually sync matrixWorld from the main camera in render().
    this.bypassCamera.matrixAutoUpdate = false;

    if (externalEmissiveRT) {
      this.emissiveRT = externalEmissiveRT;
      this.rtIsExternal = true;
    } else {
      this.emissiveRT = new THREE.WebGLRenderTarget(width, height, {
        type: THREE.HalfFloatType,
        format: THREE.RGBAFormat,
        depthBuffer: true,
        depthTexture: new THREE.DepthTexture(width, height, THREE.UnsignedIntType),
      });
      this.rtIsExternal = false;
    }
  }

  /**
   * Register a mesh for emissive bypass. Its current material becomes the
   * "real" material rendered into emissiveRT; in the main scene it is replaced
   * with a depth-only proxy so it writes depth (blocking fog) but is invisible
   * to the main render and tone mapping.
   */
  addBypassMesh(mesh: THREE.Mesh): void {
    if (this._registeredMeshes.has(mesh)) return;
    this._registeredMeshes.add(mesh);
    const realMat = mesh.material as THREE.Material | THREE.Material[];
    const proxyMat = new THREE.MeshBasicMaterial({
      colorWrite: false,
      depthWrite: false,
      // transparent: true puts this in the back-to-front pass so it renders *after*
      // all opaque geometry — opaque objects behind the portal are already drawn and
      // won't be depth-tested against the proxy. depthWrite: false lets fog and
      // other depth-dependent effects pass through the portal surface.
      transparent: true,
      side: Array.isArray(realMat) ? THREE.FrontSide : (realMat as THREE.Material).side,
    });
    mesh.material = proxyMat;
    mesh.layers.enable(EMISSIVE_BYPASS_LAYER);
    this.bypassEntries.push({ mesh, realMat, proxyMat });
  }

  setStableDepthTarget(target: THREE.WebGLRenderTarget): void {
    this.stableDepthTarget = target;
  }

  /**
   * True iff the emissive RT contains meaningful pixels from the most recent frame
   * (i.e. at least one registered bypass mesh was in the camera frustum and got rendered).
   * Downstream passes that consume emissiveRT can short-circuit when false to avoid
   * redundant work on frames where no bypass geometry is visible.
   */
  get hasContent(): boolean {
    return this._emissiveRTHasContent;
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
    if (this.bypassEntries.length === 0) return;

    // Frustum cull: skip the entire pass if no bypass mesh is within the camera frustum.
    // This avoids the depth blit, clear, and scene render when all portals are off-screen.
    this._projScreenMatrix.multiplyMatrices(
      this._mainCamera.projectionMatrix,
      this._mainCamera.matrixWorldInverse
    );
    this._frustum.setFromProjectionMatrix(this._projScreenMatrix);
    const anyVisible = this.bypassEntries.some(({ mesh }) => {
      if (!mesh.visible) return false;
      if (!mesh.geometry.boundingSphere) mesh.geometry.computeBoundingSphere();
      this._sphere.copy(mesh.geometry.boundingSphere!);
      this._sphere.applyMatrix4(mesh.matrixWorld);
      return this._frustum.intersectsSphere(this._sphere);
    });
    if (!anyVisible) {
      // When an external owner (SkyStack) manages the RT, it cleared this frame
      // already — do not touch anything here. Only the internal-ownership path
      // needs the "stale pixels from last frame" cleanup.
      if (this._emissiveRTHasContent && !this.rtIsExternal) {
        renderer.setRenderTarget(this.emissiveRT);
        renderer.setClearColor(new THREE.Color(0, 0, 0), 0);
        renderer.clear(true, false, false);
        renderer.setRenderTarget(null);
        this._emissiveRTHasContent = false;
      }
      return;
    }

    // Sync bypass camera to current main camera pose.
    // matrixWorldNeedsUpdate = false prevents Three.js from overwriting matrixWorld
    // with a recomputation from position/rotation during renderer.render().
    this.bypassCamera.matrixWorld.copy(this._mainCamera.matrixWorld);
    this.bypassCamera.matrixWorldInverse.copy(this._mainCamera.matrixWorldInverse);
    this.bypassCamera.projectionMatrix.copy(this._mainCamera.projectionMatrix);
    this.bypassCamera.projectionMatrixInverse.copy(this._mainCamera.projectionMatrixInverse);
    this.bypassCamera.matrixWorldNeedsUpdate = false;

    // Ensure emissiveRT's WebGL FBO exists.
    renderer.setRenderTarget(this.emissiveRT);

    if (this.rtIsExternal) {
      // External-ownership path (SkyStack): emissiveRT's depth attachment
      // is wired directly to stableDepth's depth texture at setup time,
      // so no per-frame depth work is needed. Color was painted + cleared
      // by SkyStackPass.
    } else if (this.stableDepthTarget) {
      // Internal-ownership path: emissiveRT owns its own depth texture, so
      // we must blit scene depth in each frame.
      const gl = renderer.getContext() as WebGL2RenderingContext;
      const props = (renderer as any).properties;

      const srcProps = props.get(this.stableDepthTarget);
      const dstProps = props.get(this.emissiveRT);
      if (srcProps?.__webglFramebuffer && dstProps?.__webglFramebuffer) {
        const srcFBO = srcProps.__webglFramebuffer as WebGLFramebuffer;
        const dstFBO = dstProps.__webglFramebuffer as WebGLFramebuffer;
        const { width, height } = this.emissiveRT;

        gl.bindFramebuffer(gl.READ_FRAMEBUFFER, srcFBO);
        gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, dstFBO);
        gl.blitFramebuffer(0, 0, width, height, 0, 0, width, height, gl.DEPTH_BUFFER_BIT, gl.NEAREST);
        gl.bindFramebuffer(gl.READ_FRAMEBUFFER, null);
        gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, null);
      }
      // Restore Three.js render target tracking after raw GL calls.
      renderer.setRenderTarget(this.emissiveRT);

      // Clear color to transparent, keeping depth from the blit.
      renderer.setClearColor(new THREE.Color(0, 0, 0), 0);
      renderer.clearColor();
    }

    // Swap to real materials for the bypass render
    for (const entry of this.bypassEntries) {
      entry.mesh.material = entry.realMat;
    }

    const savedBackground = this.scene.background;
    this.scene.background = null;
    renderer.render(this.scene, this.bypassCamera);
    this.scene.background = savedBackground;

    // Restore depth-proxy materials for the main render
    for (const entry of this.bypassEntries) {
      entry.mesh.material = entry.proxyMat;
    }

    this._emissiveRTHasContent = true;
  }

  override dispose(): void {
    for (const { proxyMat } of this.bypassEntries) {
      proxyMat.dispose();
    }
    if (!this.rtIsExternal) {
      this.emissiveRT.dispose();
    }
    super.dispose();
  }
}
