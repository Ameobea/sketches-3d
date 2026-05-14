import * as THREE from 'three';

/**
 * Renders the overlay scene through a multisample target and composites it
 * onto the canvas.  Exists because the EffectComposer's SMAA pass runs before
 * the overlay is drawn, so without this helper gizmo edges are aliased.
 *
 * Overlay materials must output premultiplied alpha (`vec4(rgb * a, a)`) with
 * `premultipliedAlpha: true` — required for correct MSAA edge resolution: the
 * alpha-weighted partial-coverage samples sum consistently and the final over-
 * blend reads `out = src + dst * (1 - src.a)` without re-scaling.
 */
export class OverlayMSAARenderer {
  private renderer: THREE.WebGLRenderer;
  private target: THREE.WebGLRenderTarget;
  private blitMaterial: THREE.ShaderMaterial;
  private blitScene: THREE.Scene;
  private blitCamera: THREE.OrthographicCamera;
  private blitMesh: THREE.Mesh;
  private width = 0;
  private height = 0;

  constructor(renderer: THREE.WebGLRenderer, samples = 4) {
    this.renderer = renderer;
    // `samples > 0` is silently a no-op on WebGL1 (single-sample render target).
    this.target = new THREE.WebGLRenderTarget(1, 1, {
      samples,
      depthBuffer: true,
      stencilBuffer: false,
      type: THREE.UnsignedByteType,
      format: THREE.RGBAFormat,
    });

    this.blitMaterial = new THREE.ShaderMaterial({
      name: 'OverlayMSAABlit',
      uniforms: { tInput: { value: this.target.texture } },
      vertexShader: /* glsl */ `
        varying vec2 vUv;
        void main() {
          vUv = position.xy * 0.5 + 0.5;
          gl_Position = vec4(position.xy, 1.0, 1.0);
        }
      `,
      fragmentShader: /* glsl */ `
        precision highp float;
        uniform sampler2D tInput;
        varying vec2 vUv;
        void main() {
          gl_FragColor = texture2D(tInput, vUv);
        }
      `,
      transparent: true,
      premultipliedAlpha: true,
      depthTest: false,
      depthWrite: false,
    });

    this.blitScene = new THREE.Scene();
    this.blitCamera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
    this.blitMesh = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), this.blitMaterial);
    this.blitMesh.frustumCulled = false;
    this.blitScene.add(this.blitMesh);
  }

  setSize(width: number, height: number) {
    if (this.width === width && this.height === height) return;
    this.width = width;
    this.height = height;
    const pr = this.renderer.getPixelRatio();
    this.target.setSize(Math.floor(width * pr), Math.floor(height * pr));
  }

  /** Composites onto the existing canvas contents — does not clear them. */
  render(scene: THREE.Scene, camera: THREE.Camera) {
    const size = this.renderer.getSize(_tmpSize);
    this.setSize(size.x, size.y);

    const renderer = this.renderer;
    const prevTarget = renderer.getRenderTarget();
    const prevAutoClear = renderer.autoClear;
    const prevAutoClearColor = renderer.autoClearColor;
    const prevAutoClearDepth = renderer.autoClearDepth;
    const prevClearColor = new THREE.Color();
    renderer.getClearColor(prevClearColor);
    const prevClearAlpha = renderer.getClearAlpha();

    renderer.setRenderTarget(this.target);
    renderer.autoClear = false;
    renderer.setClearColor(0x000000, 0);
    renderer.clear(true, true, false);
    renderer.render(scene, camera);

    renderer.setRenderTarget(prevTarget);
    renderer.autoClear = false;
    renderer.autoClearColor = false;
    renderer.autoClearDepth = false;
    renderer.render(this.blitScene, this.blitCamera);

    renderer.autoClear = prevAutoClear;
    renderer.autoClearColor = prevAutoClearColor;
    renderer.autoClearDepth = prevAutoClearDepth;
    renderer.setClearColor(prevClearColor, prevClearAlpha);
  }

  dispose() {
    this.target.dispose();
    this.blitMaterial.dispose();
    this.blitMesh.geometry.dispose();
  }
}

const _tmpSize = new THREE.Vector2();
