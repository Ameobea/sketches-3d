import { Pass } from 'postprocessing';
import * as THREE from 'three';

import { buildLayerRenderCamera, EMISSIVE_BYPASS_LAYER } from './emissiveBypassPass';
import { INLINE_EMISSIVE_LAYER } from './inlineEmissivePass';

export const TRANSPARENT_PASS_LAYER = 29;

/** `transparent` (depth testing on) and not routed through the emissive systems. */
const materialsQualify = (mesh: THREE.Mesh): boolean => {
  // Material arrays are excluded: DepthPass's transparency skip tests `material.transparent`
  // directly, so an adopted multi-material mesh would still write prepass depth and block
  // the sky behind itself.
  if (Array.isArray(mesh.material)) {
    return false;
  }
  const m = mesh.material;
  return (
    m?.transparent === true &&
    m.depthTest !== false &&
    !m.userData?.emissiveBypass &&
    !m.userData?.inlineEmissiveBypass
  );
};

/** True whether a layer-0 mesh qualifies for automatic adoption into the transparent pass. */
export const isAutoTransparentMesh = (obj: THREE.Object3D): obj is THREE.Mesh =>
  obj instanceof THREE.Mesh && !!(obj.layers.mask & 1) && materialsQualify(obj);

/**
 * Renders transparent meshes after the sky composite (and before the scene's middle
 * passes, so volumetric fog composites over them) into a backing FBO whose color
 * attachment is re-pointed at the composer's CURRENT input buffer each frame, with
 * the stable depth attached — depth-tested against real scene depth regardless of
 * ping-pong parity. Adopted meshes move off layer 0 onto TRANSPARENT_PASS_LAYER.
 * Authored `depthWrite` is kept: writes here land after the stable-depth blit and
 * sky composite (harmless downstream) and keep opaque-alpha regions occluding each
 * other per-fragment instead of depending on three's per-object transparent sort;
 * pure-blend materials can author `depthWrite: false` themselves.
 */
export class TransparentPass extends Pass {
  private readonly sceneToRender: THREE.Scene;
  private transparentCamera: THREE.PerspectiveCamera | THREE.OrthographicCamera;
  private readonly registeredMeshes = new Set<THREE.Mesh>();
  private readonly backingRT: THREE.WebGLRenderTarget;
  private boundAttachment: WebGLTexture | null = null;

  private readonly frustum = new THREE.Frustum();
  private readonly projScreenMatrix = new THREE.Matrix4();
  private readonly sphere = new THREE.Sphere();

  constructor(scene: THREE.Scene, mainCamera: THREE.PerspectiveCamera | THREE.OrthographicCamera) {
    super('TransparentPass');
    this.sceneToRender = scene;
    this.needsSwap = false;
    // Disabled until a mesh is adopted so empty scenes pay nothing.
    this.enabled = false;
    this.transparentCamera = buildLayerRenderCamera(mainCamera, TRANSPARENT_PASS_LAYER);
    this.backingRT = new THREE.WebGLRenderTarget(1, 1, {
      type: THREE.HalfFloatType,
      format: THREE.RGBAFormat,
      depthBuffer: false,
    });
  }

  /**
   * Attach the composer's stable depth as the backing FBO's depth. MUST be called
   * before the first render (three creates the FBO lazily on first setRenderTarget).
   */
  setStableDepthTexture(depthTexture: THREE.DepthTexture): void {
    this.backingRT.depthTexture = depthTexture;
    this.backingRT.depthBuffer = true;
  }

  addTransparentMesh(mesh: THREE.Mesh): void {
    if (this.registeredMeshes.has(mesh)) {
      return;
    }
    this.registeredMeshes.add(mesh);
    mesh.layers.disable(0);
    mesh.layers.enable(TRANSPARENT_PASS_LAYER);
    this.enabled = true;
  }

  private releaseMesh(mesh: THREE.Mesh): void {
    this.registeredMeshes.delete(mesh);
    mesh.layers.disable(TRANSPARENT_PASS_LAYER);
    // Hand back to the main pass only if no emissive pass has claimed the mesh in the
    // meantime (e.g. inlineEmissiveBypass toggled onto it) — re-enabling layer 0 then
    // would double-render it.
    if (!mesh.layers.isEnabled(INLINE_EMISSIVE_LAYER) && !mesh.layers.isEnabled(EMISSIVE_BYPASS_LAYER)) {
      mesh.layers.enable(0);
    }
  }

  /**
   * Adopt qualifying meshes and release adopted ones that no longer qualify (material
   * swapped back to opaque) or left the scene. For dynamic-content contexts like the
   * geotoy editor where meshes and materials churn at runtime.
   */
  syncAdoptedMeshes(scene: THREE.Scene): void {
    const seen = new Set<THREE.Mesh>();
    scene.traverse(obj => {
      if (!(obj instanceof THREE.Mesh)) {
        return;
      }
      if (this.registeredMeshes.has(obj)) {
        seen.add(obj);
        if (!materialsQualify(obj)) {
          this.releaseMesh(obj);
        }
      } else if (isAutoTransparentMesh(obj)) {
        this.addTransparentMesh(obj);
        seen.add(obj);
      }
    });
    for (const mesh of [...this.registeredMeshes]) {
      if (!seen.has(mesh)) {
        this.releaseMesh(mesh);
      }
    }
    this.enabled = this.registeredMeshes.size > 0;
  }

  /** Rebind after the scene's camera object is swapped (e.g. ortho/perspective toggle). */
  setMainCamera(camera: THREE.PerspectiveCamera | THREE.OrthographicCamera): void {
    this.transparentCamera = buildLayerRenderCamera(camera, TRANSPARENT_PASS_LAYER);
  }

  override setSize(width: number, height: number): void {
    this.backingRT.setSize(width, height);
    this.boundAttachment = null;
  }

  override render(
    renderer: THREE.WebGLRenderer,
    inputBuffer: THREE.WebGLRenderTarget,
    _outputBuffer: THREE.WebGLRenderTarget
  ): void {
    if (!this.backingRT.depthTexture) {
      throw new Error('TransparentPass: setStableDepthTexture() must be called before the first render.');
    }

    this.projScreenMatrix.multiplyMatrices(
      this.transparentCamera.projectionMatrix,
      this.transparentCamera.matrixWorldInverse
    );
    this.frustum.setFromProjectionMatrix(this.projScreenMatrix);
    let anyVisible = false;
    for (const mesh of this.registeredMeshes) {
      // Self-heal material swaps in scenes with no rescan hook (e.g. an async
      // placeholder material replaced by the real opaque one).
      if (!materialsQualify(mesh)) {
        this.releaseMesh(mesh);
        continue;
      }
      if (!mesh.visible || anyVisible) {
        continue;
      }
      // Base-geometry bounds are wrong for instanced/skinned meshes — treat as visible.
      if (
        !mesh.frustumCulled ||
        (mesh as Partial<THREE.InstancedMesh>).isInstancedMesh ||
        (mesh as Partial<THREE.SkinnedMesh>).isSkinnedMesh
      ) {
        anyVisible = true;
        continue;
      }
      if (mesh.geometry.boundingSphere === null) {
        mesh.geometry.computeBoundingSphere();
      }
      this.sphere.copy(mesh.geometry.boundingSphere!).applyMatrix4(mesh.matrixWorld);
      if (this.frustum.intersectsSphere(this.sphere)) {
        anyVisible = true;
      }
    }
    if (this.registeredMeshes.size === 0) {
      this.enabled = false;
      return;
    }
    if (!anyVisible) {
      return;
    }

    // Re-point the backing FBO's color attachment at the composer's current input
    // buffer texture (rebinding only when it changes: first run, resize, realloc).
    const gl = renderer.getContext() as WebGL2RenderingContext;
    const props = (renderer as any).properties;
    renderer.setRenderTarget(inputBuffer);
    renderer.setRenderTarget(this.backingRT);
    const fbo = props.get(this.backingRT)?.__webglFramebuffer as WebGLFramebuffer | undefined;
    const tex = props.get(inputBuffer.texture)?.__webglTexture as WebGLTexture | undefined;
    if (!fbo || !tex) {
      return;
    }
    if (this.boundAttachment !== tex) {
      // three memoizes FBO bindings, so stay on the already-bound FBO — a raw unbind
      // to null here would desync the cache and the render would land on the canvas.
      gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0);
      this.boundAttachment = tex;
    }

    // Lights are layer-culled like meshes, so main-scene (layer 0) lights must also
    // be on the transparent layer or adopted meshes render unlit. Stamped every
    // render so dynamically-added lights are picked up; isolated special-layer
    // lights (e.g. the emissive bypass ambient) are left alone.
    this.sceneToRender.traverse(obj => {
      if ((obj as THREE.Light).isLight && obj.layers.mask & 1) {
        obj.layers.enable(TRANSPARENT_PASS_LAYER);
      }
    });

    // three repaints scene.background at the start of every render call, which would
    // overwrite the main render in the hijacked buffer.
    const savedBackground = this.sceneToRender.background;
    this.sceneToRender.background = null;
    const shadowAutoUpdate = renderer.shadowMap.autoUpdate;
    renderer.shadowMap.autoUpdate = false;
    renderer.render(this.sceneToRender, this.transparentCamera);
    renderer.shadowMap.autoUpdate = shadowAutoUpdate;
    this.sceneToRender.background = savedBackground;
  }

  override dispose(): void {
    this.backingRT.dispose();
    super.dispose();
  }
}
