import * as THREE from 'three';

import type { CascadedShadowMap } from './CascadedShadowMap';

const _size = new THREE.Vector2();
const _vp = new THREE.Vector4();
const _sc = new THREE.Vector4();

/**
 * Draws each cascade's packed-depth layer as a grayscale thumbnail strip along the bottom-left of the
 * screen (near = dark, far/empty = white). Proves the array-RT render-to-layer path works. Debug only.
 */
export class CsmDebugBlit {
  private readonly scene = new THREE.Scene();
  private readonly cam = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
  private readonly material: THREE.ShaderMaterial;

  constructor(csm: CascadedShadowMap) {
    this.material = new THREE.ShaderMaterial({
      uniforms: { uDepth: { value: csm.depthRT.texture }, uLayer: { value: 0 } },
      depthTest: false,
      depthWrite: false,
      vertexShader: /* glsl */ `
        varying vec2 vUv;
        void main() {
          vUv = uv;
          gl_Position = vec4(position.xy, 0.0, 1.0);
        }`,
      fragmentShader: /* glsl */ `
        precision highp float;
        precision highp sampler2DArray;
        uniform sampler2DArray uDepth;
        uniform float uLayer;
        varying vec2 vUv;
        const vec4 UnpackFactors = (255.0 / 256.0) / vec4(256.0 * 256.0 * 256.0, 256.0 * 256.0, 256.0, 1.0);
        void main() {
          float d = dot(texture(uDepth, vec3(vUv, uLayer)), UnpackFactors);
          gl_FragColor = vec4(vec3(d), 1.0);
        }`,
    });
    this.scene.add(new THREE.Mesh(new THREE.PlaneGeometry(2, 2), this.material));
  }

  render(renderer: THREE.WebGLRenderer, cascades: number) {
    renderer.getSize(_size);
    const side = Math.min(200, Math.floor((_size.x - 16) / cascades) - 8);
    const pad = 8;
    const prevScissorTest = renderer.getScissorTest();
    const prevAutoClear = renderer.autoClear;
    renderer.getViewport(_vp);
    renderer.getScissor(_sc);
    renderer.setRenderTarget(null);
    renderer.autoClear = false;
    renderer.setScissorTest(true);
    for (let i = 0; i < cascades; i += 1) {
      const x = pad + i * (side + pad);
      this.material.uniforms.uLayer.value = i;
      renderer.setViewport(x, pad, side, side);
      renderer.setScissor(x, pad, side, side);
      renderer.render(this.scene, this.cam);
    }
    renderer.setScissorTest(prevScissorTest);
    renderer.setViewport(_vp);
    renderer.setScissor(_sc);
    renderer.autoClear = prevAutoClear;
  }
}
