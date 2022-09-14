import * as THREE from 'three';

import SkyVertexShader from './shaders/vertex.vert?raw';
import SkyFragmentShader from './shaders/fragment.frag?raw';

/**
 * Based on "A Practical Analytic Model for Daylight"
 * aka The Preetham Model, the de facto standard analytic skydome model
 * https://www.researchgate.net/publication/220720443_A_Practical_Analytic_Model_for_Daylight
 *
 * First implemented by Simon Wallner
 * http://simonwallner.at/project/atmospheric-scattering/
 *
 * Improved by Martin Upitis
 * http://blenderartists.org/forum/showthread.php?245954-preethams-sky-impementation-HDR
 *
 * Three.js integration by zz85 http://twitter.com/blurspline
 *
 * With modifications by me
 */

const SkyShader = {
  uniforms: {
    turbidity: {
      value: 2,
    },
    rayleigh: {
      value: 1,
    },
    mieCoefficient: {
      value: 0.005,
    },
    mieDirectionalG: {
      value: 0.8,
    },
    sunPosition: {
      value: new THREE.Vector3(),
    },
    up: {
      // We tilt the sky a bit to allow us to position the sun higher to line up with the directional light without
      // causing the whole sky to brigten a huge amount due to the way the sky simulation shader is implemented.
      value: new THREE.Vector3(0, 1, 0.12).normalize(),
    },
  },
  vertexShader: SkyVertexShader,
  fragmentShader: SkyFragmentShader,
};

export class CustomSky extends THREE.Mesh {
  public static isSky = true;

  constructor() {
    const shader = SkyShader;
    const material = new THREE.ShaderMaterial({
      name: 'SkyShader',
      fragmentShader: shader.fragmentShader,
      vertexShader: shader.vertexShader,
      uniforms: THREE.UniformsUtils.clone(shader.uniforms),
      side: THREE.BackSide,
      depthWrite: false,
    });
    super(new THREE.BoxGeometry(1, 1, 1), material);
  }
}
