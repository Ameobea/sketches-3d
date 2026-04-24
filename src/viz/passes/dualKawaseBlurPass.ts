import * as THREE from 'three';

import DOWN_FRAG from './shaders/dualKawaseDown.frag?raw';
import UP_FRAG from './shaders/dualKawaseUp.frag?raw';

const FULLSCREEN_VERT = `
varying vec2 vUv;
void main() {
  vUv = position.xy * 0.5 + 0.5;
  gl_Position = vec4(position.xy, 1.0, 1.0);
}
`;

/**
 * Dual Kawase blur pass — downsample + upsample mip pyramid with half-texel
 * bilinear sampling for smooth, non-square bloom kernels.
 *
 * Ref: https://blog.frost.kiwi/dual-kawase/
 * Based on Masaki Kawase's blur technique and the dual-filter formulation
 * from Marius Bjørge's "Bandwidth-Efficient Rendering" (SIGGRAPH 2015).
 *
 * Pipeline:
 *   input → down[0] (½) → down[1] (¼) → … → down[N-1] (1/2^N)
 *         → up[N-2] (1/2^(N-1)) → … → up[0] (½) → output (full res)
 *
 * Each upsample level blends with the corresponding downsample level:
 *   result = mix(downsample[i], upsample(deeper), radius)
 *
 * This gives multi-scale bloom: tight bright core from the upper levels,
 * wide soft halo from the deeper levels.  `radius` controls how much of
 * the deeper (wider) blur propagates up at each step.
 */
export class DualKawaseBlurPass {
  private levels: number;
  private radius: number;

  private downTargets: THREE.WebGLRenderTarget[] = [];
  private upTargets: THREE.WebGLRenderTarget[] = [];
  /** Full-res output target — the final upsample writes here. */
  private outputTarget: THREE.WebGLRenderTarget | null = null;

  private readonly downMaterial: THREE.ShaderMaterial;
  private readonly upMaterial: THREE.ShaderMaterial;
  private readonly fsScene: THREE.Scene;
  private readonly fsCamera: THREE.OrthographicCamera;
  private readonly fsMesh: THREE.Mesh;

  private width = 0;
  private height = 0;

  /** The final bloom texture after render(). */
  get texture(): THREE.Texture {
    return this.outputTarget!.texture;
  }

  constructor(levels = 8, radius = 0.85) {
    this.levels = levels;
    this.radius = radius;

    this.downMaterial = new THREE.ShaderMaterial({
      name: 'DualKawaseDown',
      uniforms: {
        tInput: { value: null },
        uHalfTexel: { value: new THREE.Vector2() },
      },
      vertexShader: FULLSCREEN_VERT,
      fragmentShader: DOWN_FRAG,
      depthWrite: false,
      depthTest: false,
    });

    this.upMaterial = new THREE.ShaderMaterial({
      name: 'DualKawaseUp',
      uniforms: {
        tInput: { value: null },
        tDownsample: { value: null },
        uHalfTexel: { value: new THREE.Vector2() },
        uRadius: { value: radius },
      },
      vertexShader: FULLSCREEN_VERT,
      fragmentShader: UP_FRAG,
      depthWrite: false,
      depthTest: false,
    });

    this.fsScene = new THREE.Scene();
    this.fsCamera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
    this.fsMesh = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), this.downMaterial);
    this.fsMesh.frustumCulled = false;
    this.fsScene.add(this.fsMesh);
  }

  setLevels(levels: number): void {
    if (levels !== this.levels) {
      this.levels = levels;
      this.rebuildTargets();
    }
  }

  setRadius(radius: number): void {
    this.radius = radius;
    this.upMaterial.uniforms.uRadius.value = radius;
  }

  setSize(width: number, height: number): void {
    if (width === this.width && height === this.height) return;
    this.width = width;
    this.height = height;
    this.rebuildTargets();
  }

  private rebuildTargets(): void {
    this.disposeTargets();
    if (this.width === 0 || this.height === 0) return;

    const makeRT = (w: number, h: number): THREE.WebGLRenderTarget =>
      new THREE.WebGLRenderTarget(Math.max(w, 1), Math.max(h, 1), {
        type: THREE.HalfFloatType,
        format: THREE.RGBAFormat,
        minFilter: THREE.LinearFilter,
        magFilter: THREE.LinearFilter,
        depthBuffer: false,
      });

    // Downsample chain: each level is half the previous.
    let w = this.width;
    let h = this.height;
    for (let i = 0; i < this.levels; i++) {
      w = Math.max(Math.floor(w / 2), 1);
      h = Math.max(Math.floor(h / 2), 1);
      this.downTargets.push(makeRT(w, h));
    }

    // Upsample chain: mirrors the downsample chain (excluding the deepest level).
    // up[0] matches down[levels-2] size, up[levels-2] matches down[0] size.
    for (let i = this.levels - 2; i >= 0; i--) {
      this.upTargets.push(makeRT(this.downTargets[i].width, this.downTargets[i].height));
    }

    // Full-res output target.
    this.outputTarget = makeRT(this.width, this.height);
  }

  /**
   * Run the dual Kawase blur.
   * @param renderer  WebGL renderer
   * @param inputRT   Source render target (e.g. filtered emissive)
   */
  render(renderer: THREE.WebGLRenderer, inputRT: THREE.WebGLRenderTarget): void {
    if (this.downTargets.length === 0 || !this.outputTarget) return;

    // --- Downsample ---
    this.fsMesh.material = this.downMaterial;
    let src: THREE.WebGLRenderTarget = inputRT;
    for (let i = 0; i < this.levels; i++) {
      const dst = this.downTargets[i];
      this.downMaterial.uniforms.tInput.value = src.texture;
      this.downMaterial.uniforms.uHalfTexel.value.set(0.5 / src.width, 0.5 / src.height);
      renderer.setRenderTarget(dst);
      renderer.render(this.fsScene, this.fsCamera);
      src = dst;
    }

    // --- Upsample ---
    this.fsMesh.material = this.upMaterial;
    this.upMaterial.uniforms.uRadius.value = this.radius;

    // First upsample: from deepest down level → second-deepest size.
    // `tInput` is the deepest level, `tDownsample` is the level we're upsampling TO.
    let upSrc = this.downTargets[this.levels - 1];
    for (let i = 0; i < this.upTargets.length; i++) {
      const dst = this.upTargets[i];
      // The downsample level at this output resolution (for the blend).
      const downAtThisLevel = this.downTargets[this.levels - 2 - i];
      this.upMaterial.uniforms.tInput.value = upSrc.texture;
      this.upMaterial.uniforms.tDownsample.value = downAtThisLevel.texture;
      this.upMaterial.uniforms.uHalfTexel.value.set(0.5 / upSrc.width, 0.5 / upSrc.height);
      renderer.setRenderTarget(dst);
      renderer.render(this.fsScene, this.fsCamera);
      upSrc = dst;
    }

    // Final upsample to full resolution.
    this.upMaterial.uniforms.tInput.value = upSrc.texture;
    this.upMaterial.uniforms.tDownsample.value = inputRT.texture;
    this.upMaterial.uniforms.uHalfTexel.value.set(0.5 / upSrc.width, 0.5 / upSrc.height);
    renderer.setRenderTarget(this.outputTarget);
    renderer.render(this.fsScene, this.fsCamera);
  }

  private disposeTargets(): void {
    for (const rt of this.downTargets) rt.dispose();
    for (const rt of this.upTargets) rt.dispose();
    this.outputTarget?.dispose();
    this.downTargets = [];
    this.upTargets = [];
    this.outputTarget = null;
  }

  dispose(): void {
    this.disposeTargets();
    this.fsMesh.geometry.dispose();
    this.downMaterial.dispose();
    this.upMaterial.dispose();
  }
}
