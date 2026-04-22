import * as THREE from 'three';

import type { Viz } from 'src/viz';

import { composeSkyShader } from './compose';
import { SkyStackPass } from './SkyStackPass';
import skyStackVert from './shaders/skyStack.vert?raw';
import type { BackgroundLayer, Layer } from './types';
import { createSharedUniforms, type SkyStackSharedUniforms } from './uniforms';

export interface SkyStackParams {
  /** Elevation offset of the horizon, in [-1, 1]. Default 0. */
  horizonOffset?: number;
  /** Half-width of the horizon smoothstep (in elevation units). Default 0.02. */
  horizonBlend?: number;
  /** Front-to-back layers. Any order — sorted by zIndex at compose time. */
  layers?: Layer[];
  /** Optional backmost fill. If omitted, sky is black where no layer writes. */
  background?: BackgroundLayer;
}

/**
 * Unified sub-pipeline for background sky content. Owns a single `SkyStackPass`
 * that runs one MRT draw producing (color, emissive) simultaneously.
 *
 *   attachment 0 (color)    — blitted into inputBuffer BEFORE MainRenderPass.
 *                             Tone-mapped in `FinalPass`.
 *   attachment 1 (emissive) — blitted into `emissiveRT`. Bypasses tone mapping,
 *                             drives bloom, shared with EmissiveBypassPass.
 *
 * Layers are supplied by factory functions — see `layers/*`, `backgrounds/*`,
 * and the custom-layer escape hatches (`customLayer`, `customBackground`).
 * Per-layer uniforms live on the layer objects' `.uniforms` records; hold a
 * reference to the layer if you need to mutate them at runtime.
 *
 * Shared uniforms (time, scene depth, horizon offset/blend) stay here:
 *   viz.registerBeforeRenderCb(t => skyStack.setTime(t));
 */
export class SkyStack {
  public readonly pass: SkyStackPass;
  private readonly uniforms: SkyStackSharedUniforms;

  constructor(viz: Viz, params: SkyStackParams, width: number, height: number) {
    this.uniforms = createSharedUniforms();
    this.uniforms.uHorizonOffset.value = params.horizonOffset ?? 0;
    this.uniforms.uHorizonBlend.value = params.horizonBlend ?? 0.02;
    this.uniforms.uProjectionMatrixInverse.value = viz.camera.projectionMatrixInverse;
    this.uniforms.uCameraWorldMatrix.value = viz.camera.matrixWorld;

    const { fragmentShader, uniforms } = composeSkyShader(
      this.uniforms,
      params.layers ?? [],
      params.background ?? null
    );

    const material = new THREE.ShaderMaterial({
      name: 'SkyStack.Unified',
      vertexShader: skyStackVert,
      fragmentShader,
      uniforms,
      glslVersion: THREE.GLSL3,
      transparent: false,
      depthTest: false,
      depthWrite: false,
    });

    this.pass = new SkyStackPass(material, width, height);
  }

  public get emissiveRT(): THREE.WebGLRenderTarget {
    return this.pass.emissiveRT;
  }

  public setTime(timeSeconds: number): void {
    this.uniforms.uTime.value = timeSeconds;
  }

  public setSceneDepth(depthTexture: THREE.Texture | null): void {
    this.uniforms.uSceneDepth.value = depthTexture;
  }

  public setHorizonOffset(offset: number): void {
    this.uniforms.uHorizonOffset.value = offset;
  }

  public setHorizonBlend(blend: number): void {
    this.uniforms.uHorizonBlend.value = blend;
  }
}
