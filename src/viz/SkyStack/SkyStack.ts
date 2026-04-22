import * as THREE from 'three';

import type { Viz } from 'src/viz';

import type { BuildingsLayerConfig } from './layers/BuildingsLayer';
import type { CloudsLayerConfig } from './layers/CloudsLayer';
import type { GradientLayerConfig } from './layers/GradientLayer';
import type { GroundLayerConfig } from './layers/GroundLayer';
import type { StarsLayerConfig } from './layers/StarsLayer';
import { SkyStackPass } from './SkyStackPass';
import skyStackVert from './shaders/skyStack.vert?raw';
import { buildUnifiedSkyShader, type UnifiedLayerUniforms } from './skyUnifiedShader';
import {
  createSkyStackUniforms,
  HorizonMode,
  type CloudBand,
  type GradientStop,
  type SkyStackUniforms,
} from './uniforms';

export interface SkyStackParams {
  /**
   * Elevation offset of the horizon, in [-1, 1]. Applied inside the shader
   * before every layer does its elev-based math.
   */
  horizonOffset?: number;
  gradient: GradientLayerConfig;
  stars?: StarsLayerConfig;
  buildings?: BuildingsLayerConfig;
  /**
   * Cloud band rendered *behind* the buildings — sits above the gradient but
   * gets occluded by silhouettes + windows. Its attenuator dims stars that
   * sit behind the cloud.
   */
  cloudsBack?: CloudsLayerConfig;
  /**
   * Cloud band rendered *in front* of the buildings — covers silhouettes and
   * windows. Its attenuator dims both stars and windows behind the cloud.
   */
  cloudsFront?: CloudsLayerConfig;
  /**
   * Virtual ground plane — infinite y=0 plane rendered below the horizon.
   * Paint is a user-provided GLSL function; its output goes to the emissive
   * path only (bypasses tone mapping, drives bloom), matching the old
   * standalone GroundPlane-as-emissive-bypass behavior.
   */
  ground?: GroundLayerConfig;
}

/**
 * Unified sub-pipeline for background sky content. Owns a single `SkyStackPass`
 * that runs one MRT draw producing (color, emissive) simultaneously.
 *
 *   attachment 0 (color)    — blitted into the composer's inputBuffer BEFORE
 *                             MainRenderPass. Tone-mapped in `FinalPass`.
 *   attachment 1 (emissive) — blitted into `emissiveRT` (owned by this pass).
 *                             Bypasses tone mapping, drives bloom, shared with
 *                             EmissiveBypassPass (bypass meshes composite on
 *                             top without clearing).
 *
 * Runtime-mutable state (gradient stops, bands, time, depth, horizon offset)
 * lives in the shared `SkyStackUniforms` record. Per-layer uniforms (stars
 * intensity, building counts, cloud shape, ground atmospheric tint, etc.) are
 * allocated by the unified shader builder; if you need to mutate them at
 * runtime, reach into `layerUniforms` and update the `.value` on the right
 * slot.
 *
 * Usage:
 *   const skyStack = new SkyStack(viz, { gradient: { ... } }, width, height);
 *   configureDefaultPostprocessingPipeline({ ..., skyStack });
 *   viz.registerBeforeRenderCb(t => skyStack.setTime(t));
 */
export class SkyStack {
  public readonly pass: SkyStackPass;
  public readonly layerUniforms: UnifiedLayerUniforms;
  private readonly uniforms: SkyStackUniforms;
  /** Stop count baked into the shader at construction. setStops requires exactly this many entries. */
  private readonly stopCount: number;
  /** Band count baked into the shader at construction. setBands requires exactly this many entries. */
  private readonly bandCount: number;

  constructor(viz: Viz, params: SkyStackParams, width: number, height: number) {
    if (params.gradient.stops.length < 1) {
      throw new Error('SkyStack: gradient must have at least one stop');
    }

    const stops = params.gradient.stops;
    const bands = params.gradient.bands ?? [];
    this.stopCount = stops.length;
    this.bandCount = bands.length;

    this.uniforms = createSkyStackUniforms({
      stopCount: this.stopCount,
      bandCount: this.bandCount,
    });
    this.uniforms.uHorizonOffset.value = params.horizonOffset ?? 0;

    this.uniforms.uProjectionMatrixInverse.value = viz.camera.projectionMatrixInverse;
    this.uniforms.uCameraWorldMatrix.value = viz.camera.matrixWorld;

    this.setStops(stops);
    this.uniforms.uHorizonMode.value = params.gradient.horizonMode ?? HorizonMode.Mirror;
    this.uniforms.uBelowColor.value.set(params.gradient.belowColor ?? 0x000000);
    this.uniforms.uHorizonBlend.value = params.gradient.horizonBlend ?? 0.02;
    this.setBands(bands);

    const { fragmentShader, uniforms, ownUniforms } = buildUnifiedSkyShader(
      this.uniforms,
      {
        stars: params.stars,
        buildings: params.buildings,
        cloudsBack: params.cloudsBack,
        cloudsFront: params.cloudsFront,
        ground: params.ground,
      },
      params.ground?.paintShader
    );

    this.layerUniforms = ownUniforms;

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

  /**
   * Update the stop values. Length must match the count baked at construction
   * — the shader's loop bound and uniform-array size are compile-time constants
   * derived from that count, so the count itself can't change at runtime.
   */
  public setStops(stops: GradientStop[]): void {
    if (stops.length !== this.stopCount) {
      throw new Error(
        `SkyStack.setStops: expected ${this.stopCount} stops (baked at construction), got ${stops.length}`
      );
    }
    const positions = this.uniforms.uStopPositions.value;
    const colors = this.uniforms.uStopColors.value;
    for (let i = 0; i < this.stopCount; i++) {
      positions[i] = stops[i].position;
      colors[i].set(stops[i].color);
    }
  }

  /** Same contract as setStops. */
  public setBands(bands: CloudBand[]): void {
    if (bands.length !== this.bandCount) {
      throw new Error(
        `SkyStack.setBands: expected ${this.bandCount} bands (baked at construction), got ${bands.length}`
      );
    }
    const u = this.uniforms;
    const centers = u.uBandCenters.value;
    const widths = u.uBandWidths.value;
    const intensities = u.uBandIntensities.value;
    const fadeRates = u.uBandFadeRates.value;
    const fadePhases = u.uBandFadePhases.value;
    const colors = u.uBandColors.value;
    for (let i = 0; i < this.bandCount; i++) {
      const b = bands[i];
      centers[i] = b.center;
      widths[i] = b.width;
      intensities[i] = b.intensity;
      fadeRates[i] = b.fadeRate ?? 0;
      fadePhases[i] = b.fadePhase ?? 0;
      colors[i].set(b.color);
    }
  }
}
