import { Pass, RenderPass, type Resizable, Selection } from 'postprocessing';
import * as THREE from 'three';

export class ClearDepthPass extends Pass {
  constructor() {
    super();
    this.needsSwap = false;
  }

  render(renderer: THREE.WebGLRenderer) {
    renderer.clearDepth();
  }
}

export class DepthPass extends RenderPass implements Resizable {
  public renderTarget: THREE.WebGLRenderTarget | null = null;
  /**
   * Optional non-dithering depth-only material used in a second prepass step for meshes flagged
   * `occlusionExclude` (the player, `nonPermeable` walls, etc). Without this second step those
   * meshes leave depth at the far plane, which makes downstream consumers (SkyStack's
   * `discardIfOccluded`, FinalPass's sky-bypass tone-map gate, FinalPass's emissive composite)
   * mistake their pixels for sky and bleed sky color through them.
   */
  private plainDepthMaterial: THREE.Material | null;

  constructor(
    scene: THREE.Scene,
    camera: THREE.Camera,
    overrideMaterial: THREE.Material,
    useExternalRenderTarget?: boolean,
    plainDepthMaterial?: THREE.Material
  ) {
    super(scene, camera, overrideMaterial);
    this.clearPass.setClearFlags(false, true, false);
    overrideMaterial.colorWrite = false;
    if (plainDepthMaterial) plainDepthMaterial.colorWrite = false;
    this.plainDepthMaterial = plainDepthMaterial ?? null;
    if (useExternalRenderTarget) {
      this.renderTarget = new THREE.WebGLRenderTarget(1, 1, {
        format: THREE.RGBAFormat,
        type: THREE.UnsignedByteType,
        depthBuffer: true,
      });
    }
  }

  render(
    renderer: THREE.WebGLRenderer,
    inputBuffer: THREE.WebGLRenderTarget,
    outputBuffer: THREE.WebGLRenderTarget,
    deltaTime?: number | undefined,
    stencilTest?: boolean | undefined
  ): void {
    // Categorize scene meshes: `selection` gets the dithering override material; `occlusionExcluded`
    // gets the plain depth material in a second step so its depth still lands in the buffer without
    // picking up the camera-occlusion dither pattern.
    const selection: THREE.Object3D[] = [];
    const occlusionExcluded: THREE.Object3D[] = [];
    this.scene.traverse(c => {
      if (c instanceof THREE.Mesh && c.material.transparent) {
        return;
      }
      const mat = (c as any).material;
      if (mat?.depthTest === false) {
        return;
      }
      // The flag may be set on the mesh's userData or on the material's userData
      // (`buildCustomShader` with `noOcclusion: true` sets it on `material.userData`).
      const matUserData = !Array.isArray(mat) ? mat?.userData : undefined;
      if ((c as THREE.Mesh).userData?.occlusionExclude || matUserData?.occlusionExclude) {
        occlusionExcluded.push(c);
        return;
      }
      selection.push(c);
    });
    if (!this.selection) {
      this.selection = new Selection(selection);
    }
    this.selection.clear();
    for (const child of selection) {
      this.selection.add(child);
    }

    // Step 1: dithering prepass for occlusion-eligible meshes. Hide excluded ones so the
    // dithering override material doesn't punch holes into their depth.
    for (const obj of occlusionExcluded) {
      obj.visible = false;
    }

    const shadowMapWasEnabled = renderer.shadowMap.enabled;
    renderer.shadowMap.enabled = false;
    renderer.getContext().depthFunc(renderer.getContext().LEQUAL);
    if (this.renderTarget) {
      // I have no idea why, but `RenderPass` seems to render to `inputBuffer` instead of `outputBuffer`
      this.renderTarget.depthTexture = inputBuffer.depthTexture;
      super.render(renderer, this.renderTarget, outputBuffer, deltaTime, stencilTest);
    } else {
      super.render(renderer, inputBuffer, outputBuffer, deltaTime, stencilTest);
    }

    for (const obj of occlusionExcluded) {
      obj.visible = true;
    }

    // Step 2: plain (non-dithering) depth prepass for occlusion-excluded meshes. Toggles
    // visibility on every other Mesh so `scene.overrideMaterial` doesn't accidentally apply to
    // them. Map captures prior visibility so user-hidden meshes don't get force-shown after.
    if (occlusionExcluded.length > 0 && this.plainDepthMaterial) {
      const occlusionExcludedSet = new Set(occlusionExcluded);
      const restoreVisibility = new Map<THREE.Object3D, boolean>();
      this.scene.traverse(c => {
        if (!(c instanceof THREE.Mesh)) return;
        if (occlusionExcludedSet.has(c)) return;
        if (!c.visible) return;
        restoreVisibility.set(c, true);
        c.visible = false;
      });

      const savedOverride = this.scene.overrideMaterial;
      this.scene.overrideMaterial = this.plainDepthMaterial;
      const target = this.renderTarget ?? inputBuffer;
      renderer.setRenderTarget(target);
      renderer.render(this.scene, this.camera);
      this.scene.overrideMaterial = savedOverride;

      for (const obj of restoreVisibility.keys()) obj.visible = true;
    }

    renderer.shadowMap.enabled = shadowMapWasEnabled;
  }

  setSize(width: number, height: number): void {
    this.renderTarget?.setSize(width, height);
    super.setSize(width, height);
  }
}

export class MainRenderPass extends RenderPass {
  constructor(scene: THREE.Scene, camera: THREE.Camera) {
    super(scene, camera);
    this.clear = false;
  }
}
