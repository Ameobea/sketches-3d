import * as THREE from 'three';

import noiseShaderCode from 'src/viz/shaders/noise.frag?raw';

import fragmentShaderPrelude from './shaders/groundPlane.prelude.frag?raw';
import fragmentShader from './shaders/groundPlane.frag?raw';
import vertexShader from './shaders/groundPlane.vert?raw';

export interface GroundPlaneParams {
  /**
   * UV-space scale knob for the virtual ground plane. The plane is mathematical, not
   * literal geometry — larger `height` makes features appear smaller (coarser UV units),
   * smaller makes them larger. Start around 50–200 and tune by eye.
   */
  height?: number;
  /**
   * Below-horizon elevation (in |dir.y| units) where alpha fade begins. 0 means the fade
   * starts exactly at the horizon line. Default 0.
   */
  horizonFadeStart?: number;
  /**
   * Below-horizon elevation where alpha fade completes. Default 0.08 — roughly the bottom
   * 8% of the view direction range below horizon. Increase for a softer transition.
   */
  horizonFadeEnd?: number;
  /**
   * GLSL source that defines the paint function:
   *
   *   vec4 paintGround(vec2 uv, vec2 uvDeriv, vec3 dir, float invDist)
   *
   * Returns (rgb, alpha). Alpha is multiplied by the horizon fade. `uTime` and helpers
   * from `noise.frag` (hash, noise, fbm) are in scope.
   */
  paintShader: string;
  /**
   * Fake atmospheric tint — mixes the paint color toward `color` as -dir.y approaches 0,
   * approximating distance reddening/darkening without a real optical-depth calc. At
   * strength 0 (the default) it's a no-op. Match `color` to the sky's horizon hue for
   * a seamless blend.
   */
  atmosphericTint?: {
    color: THREE.ColorRepresentation;
    /** |dir.y| range over which the tint fades in. At -dir.y=range, tint ≈ 0; at horizon, full strength. Default 0.2. */
    range?: number;
    /** Max mix factor at the horizon. 0 = off, 1 = full replacement. Default 0. */
    strength?: number;
  };
  /** Additional uniforms exposed to the paint shader. */
  uniforms?: Record<string, THREE.IUniform>;
}

export class GroundPlane extends THREE.Mesh {
  private readonly uniforms: Record<string, THREE.IUniform>;

  constructor(params: GroundPlaneParams) {
    const uniforms: Record<string, THREE.IUniform> = {
      uTime: { value: 0 },
      uHeight: { value: params.height ?? 100 },
      uHorizonFadeStart: { value: params.horizonFadeStart ?? 0.0 },
      uHorizonFadeEnd: { value: params.horizonFadeEnd ?? 0.08 },
      uAtmoTintColor: { value: new THREE.Color(params.atmosphericTint?.color ?? 0x000000) },
      uAtmoTintRange: { value: params.atmosphericTint?.range ?? 0.2 },
      uAtmoTintStrength: { value: params.atmosphericTint?.strength ?? 0 },
      ...(params.uniforms ?? {}),
    };

    const material = new THREE.ShaderMaterial({
      name: 'GroundPlane',
      vertexShader,
      fragmentShader: `${noiseShaderCode}\n${fragmentShaderPrelude}\n${params.paintShader}\n${fragmentShader}`,
      uniforms,
      side: THREE.BackSide,
      transparent: true,
      depthWrite: false,
      glslVersion: THREE.GLSL3,
    });
    material.toneMapped = false;

    super(new THREE.BoxGeometry(1, 1, 1), material);
    this.uniforms = uniforms;
  }

  public setTime(timeSeconds: number): void {
    this.uniforms.uTime.value = timeSeconds;
  }

  public getUniforms(): Record<string, THREE.IUniform> {
    return this.uniforms;
  }
}
