import * as THREE from 'three';

import noiseShaderCode from 'src/viz/shaders/noise.frag?raw';

import fragmentShader from './shaders/gradientSky.frag?raw';
import vertexShader from './shaders/gradientSky.vert?raw';

const MAX_STOPS = 8;
const MAX_BANDS = 4;
const MAX_HAZES = 2;

export const HorizonMode = {
  SolidBelow: 0,
  Mirror: 1,
  Extend: 2,
} as const;
export type HorizonMode = (typeof HorizonMode)[keyof typeof HorizonMode];

export interface GradientStop {
  /** 0 = horizon, 1 = zenith. Stops must be sorted ascending. */
  position: number;
  color: THREE.ColorRepresentation;
}

export interface CloudBand {
  /** Elevation center in [-1, 1]. 0 = horizon, 1 = zenith, -1 = nadir. */
  center: number;
  /** Falloff half-width in the same units as `center`. */
  width: number;
  color: THREE.ColorRepresentation;
  intensity: number;
  /** Fade rate in rad/s. 0 disables animation. */
  fadeRate?: number;
  fadePhase?: number;
}

export interface HazeLayer {
  /**
   * Cloud color for thin / low-density regions (wispy edges). When `highColor`
   * is not set, this is the only color used.
   */
  color: THREE.ColorRepresentation;
  /**
   * Cloud color for dense / high-density regions (cores). If set, the layer
   * color is Oklab-mixed between `color` and `highColor` by the shaped fBm
   * value — wisps read as `color`, cores read as `highColor`. Defaults to
   * `color` (no multi-coloration).
   */
  highColor?: THREE.ColorRepresentation;
  /** Peak opacity of the layer in [0, 1]. */
  intensity: number;
  /** Elevation center in [-1, 1]. */
  center: number;
  /** Half-width in elevation units. */
  width: number;
  /**
   * Edge sharpness of the noise threshold, in (0, 0.5]. Smaller = crisper,
   * wispier features; larger = softer, more diffuse. 0.15 is a reasonable
   * starting point.
   */
  sharpness?: number;
  /**
   * Anisotropic scale applied to the direction vector before sampling 3D
   * noise. Large y with small x/z produces horizontal streaking; uniform
   * values produce isotropic puffs.
   */
  scale?: [number, number, number];
  /** Per-axis drift speed applied to the noise input (units/second). */
  speed?: [number, number, number];
  /**
   * Amplitude (in elevation units) of a low-frequency warp applied to the
   * band's center elevation. Breaks up the circular contours that would
   * otherwise appear when a band is sampled far from the horizon. 0 disables.
   */
  warp?: number;
  /** Spatial frequency of the warp around the azimuth loop. */
  warpScale?: number;
  /** Time-drift of the warp pattern, rad/s. 0 keeps it static. */
  warpSpeed?: number;
  /**
   * fBm octave count. More octaves add finer detail at increasing cost. Capped
   * at 6 by the shader. Default 4.
   */
  octaves?: number;
  /** fBm frequency multiplier per octave. 2.0 is standard; 2.1–2.3 reduces axial alignment. Default 2.0. */
  lacunarity?: number;
  /** fBm amplitude multiplier per octave. 0.5 is standard; higher = wispier. Default 0.5. */
  gain?: number;
  /**
   * Offset added to the fBm output before thresholding. Positive values make
   * coverage denser, negative values sparser. Typical range [-0.3, 0.3].
   * Default 0.
   */
  bias?: number;
  /**
   * Exponent applied to the (biased) fBm output. >1 crushes low values (crisper
   * towering features), <1 lifts them (softer haze). Default 1 (no shaping).
   */
  pow?: number;
}

export interface StarField {
  color?: THREE.ColorRepresentation;
  /** Overall brightness multiplier. 0 disables. */
  intensity: number;
  /**
   * Cells across one full azimuth turn (roughly "stars across the horizon"
   * at full sky coverage). 64–256 is a reasonable range.
   */
  density: number;
  /** Fraction of cells that actually contain a star, in [0, 1]. */
  threshold: number;
  /** Star point size in local cell units. 0.02–0.1 works well. */
  size: number;
  /** Twinkle speed in rad/s. 0 disables twinkle. */
  twinkleSpeed: number;
  /** Elevation below which stars fade out. Use slightly above the horizon. */
  minElev: number;
}

export interface GradientSkyParams {
  stops: GradientStop[];
  horizonOffset?: number;
  horizonMode?: HorizonMode;
  belowColor?: THREE.ColorRepresentation;
  horizonBlend?: number;
  bands?: CloudBand[];
  haze?: HazeLayer[];
  stars?: StarField;
}

const makeColorArray = (n: number): THREE.Color[] => {
  const out: THREE.Color[] = [];
  for (let i = 0; i < n; i++) {
    out.push(new THREE.Color());
  }
  return out;
};

const makeVec3Array = (n: number): THREE.Vector3[] => {
  const out: THREE.Vector3[] = [];
  for (let i = 0; i < n; i++) {
    out.push(new THREE.Vector3());
  }
  return out;
};

export class GradientSky extends THREE.Mesh {
  public static isSky = true;

  private readonly uniforms: Record<string, THREE.IUniform>;

  constructor(params: GradientSkyParams) {
    const uniforms: Record<string, THREE.IUniform> = {
      uTime: { value: 0 },

      uStopCount: { value: 0 },
      uStopPositions: { value: new Float32Array(MAX_STOPS) },
      uStopColors: { value: makeColorArray(MAX_STOPS) },

      uHorizonOffset: { value: params.horizonOffset ?? 0 },
      uHorizonMode: { value: params.horizonMode ?? HorizonMode.Mirror },
      uBelowColor: { value: new THREE.Color(params.belowColor ?? 0x000000) },
      uHorizonBlend: { value: params.horizonBlend ?? 0.02 },

      uBandCount: { value: 0 },
      uBandCenters: { value: new Float32Array(MAX_BANDS) },
      uBandWidths: { value: new Float32Array(MAX_BANDS) },
      uBandColors: { value: makeColorArray(MAX_BANDS) },
      uBandIntensities: { value: new Float32Array(MAX_BANDS) },
      uBandFadeRates: { value: new Float32Array(MAX_BANDS) },
      uBandFadePhases: { value: new Float32Array(MAX_BANDS) },

      uHazeCount: { value: 0 },
      uHazeColors: { value: makeColorArray(MAX_HAZES) },
      uHazeHighColors: { value: makeColorArray(MAX_HAZES) },
      uHazeIntensities: { value: new Float32Array(MAX_HAZES) },
      uHazeCenters: { value: new Float32Array(MAX_HAZES) },
      uHazeWidths: { value: new Float32Array(MAX_HAZES) },
      uHazeSharpness: { value: new Float32Array(MAX_HAZES) },
      uHazeScales: { value: makeVec3Array(MAX_HAZES) },
      uHazeSpeeds: { value: makeVec3Array(MAX_HAZES) },
      uHazeWarp: { value: new Float32Array(MAX_HAZES) },
      uHazeWarpScale: { value: new Float32Array(MAX_HAZES) },
      uHazeWarpSpeed: { value: new Float32Array(MAX_HAZES) },
      uHazeOctaves: { value: new Int32Array(MAX_HAZES) },
      uHazeLacunarity: { value: new Float32Array(MAX_HAZES) },
      uHazeGain: { value: new Float32Array(MAX_HAZES) },
      uHazeBias: { value: new Float32Array(MAX_HAZES) },
      uHazePow: { value: new Float32Array(MAX_HAZES) },

      uStarIntensity: { value: 0 },
      uStarColor: { value: new THREE.Color(0xffffff) },
      uStarDensity: { value: 128 },
      uStarThreshold: { value: 0.02 },
      uStarSize: { value: 0.05 },
      uStarTwinkleSpeed: { value: 1.5 },
      uStarMinElev: { value: 0.02 },
    };

    const material = new THREE.ShaderMaterial({
      name: 'GradientSky',
      vertexShader,
      fragmentShader: `${noiseShaderCode}\n${fragmentShader}`,
      uniforms,
      side: THREE.BackSide,
      depthWrite: false,
      glslVersion: THREE.GLSL3,
    });
    material.toneMapped = false;

    super(new THREE.BoxGeometry(1, 1, 1), material);

    this.uniforms = uniforms;
    this.setStops(params.stops);
    this.setBands(params.bands ?? []);
    this.setHaze(params.haze ?? []);
    if (params.stars) {
      this.setStars(params.stars);
    }
  }

  public setTime(timeSeconds: number): void {
    this.uniforms.uTime.value = timeSeconds;
  }

  public setStops(stops: GradientStop[]): void {
    const n = Math.min(stops.length, MAX_STOPS);
    const positions = this.uniforms.uStopPositions.value as Float32Array;
    const colors = this.uniforms.uStopColors.value as THREE.Color[];
    for (let i = 0; i < n; i++) {
      positions[i] = stops[i].position;
      colors[i].set(stops[i].color);
    }
    this.uniforms.uStopCount.value = n;
  }

  public setBands(bands: CloudBand[]): void {
    const n = Math.min(bands.length, MAX_BANDS);
    const centers = this.uniforms.uBandCenters.value as Float32Array;
    const widths = this.uniforms.uBandWidths.value as Float32Array;
    const intensities = this.uniforms.uBandIntensities.value as Float32Array;
    const fadeRates = this.uniforms.uBandFadeRates.value as Float32Array;
    const fadePhases = this.uniforms.uBandFadePhases.value as Float32Array;
    const colors = this.uniforms.uBandColors.value as THREE.Color[];
    for (let i = 0; i < n; i++) {
      const b = bands[i];
      centers[i] = b.center;
      widths[i] = b.width;
      intensities[i] = b.intensity;
      fadeRates[i] = b.fadeRate ?? 0;
      fadePhases[i] = b.fadePhase ?? 0;
      colors[i].set(b.color);
    }
    this.uniforms.uBandCount.value = n;
  }

  public setHaze(layers: HazeLayer[]): void {
    const n = Math.min(layers.length, MAX_HAZES);
    const colors = this.uniforms.uHazeColors.value as THREE.Color[];
    const highColors = this.uniforms.uHazeHighColors.value as THREE.Color[];
    const intensities = this.uniforms.uHazeIntensities.value as Float32Array;
    const centers = this.uniforms.uHazeCenters.value as Float32Array;
    const widths = this.uniforms.uHazeWidths.value as Float32Array;
    const sharpness = this.uniforms.uHazeSharpness.value as Float32Array;
    const scales = this.uniforms.uHazeScales.value as THREE.Vector3[];
    const speeds = this.uniforms.uHazeSpeeds.value as THREE.Vector3[];
    const warp = this.uniforms.uHazeWarp.value as Float32Array;
    const warpScale = this.uniforms.uHazeWarpScale.value as Float32Array;
    const warpSpeed = this.uniforms.uHazeWarpSpeed.value as Float32Array;
    const octaves = this.uniforms.uHazeOctaves.value as Int32Array;
    const lacunarity = this.uniforms.uHazeLacunarity.value as Float32Array;
    const gain = this.uniforms.uHazeGain.value as Float32Array;
    const bias = this.uniforms.uHazeBias.value as Float32Array;
    const powExp = this.uniforms.uHazePow.value as Float32Array;
    for (let i = 0; i < n; i++) {
      const h = layers[i];
      colors[i].set(h.color);
      highColors[i].set(h.highColor ?? h.color);
      intensities[i] = h.intensity;
      centers[i] = h.center;
      widths[i] = h.width;
      sharpness[i] = h.sharpness ?? 0.15;
      const s = h.scale ?? [1.2, 8, 1.2];
      scales[i].set(s[0], s[1], s[2]);
      const sp = h.speed ?? [0.02, 0, 0.02];
      speeds[i].set(sp[0], sp[1], sp[2]);
      warp[i] = h.warp ?? 0;
      warpScale[i] = h.warpScale ?? 1.5;
      warpSpeed[i] = h.warpSpeed ?? 0;
      octaves[i] = Math.max(1, Math.min(6, h.octaves ?? 4));
      lacunarity[i] = h.lacunarity ?? 2.0;
      gain[i] = h.gain ?? 0.5;
      bias[i] = h.bias ?? 0;
      powExp[i] = h.pow ?? 1;
    }
    this.uniforms.uHazeCount.value = n;
  }

  public setStars(stars: StarField | null): void {
    if (!stars) {
      this.uniforms.uStarIntensity.value = 0;
      return;
    }
    this.uniforms.uStarIntensity.value = stars.intensity;
    (this.uniforms.uStarColor.value as THREE.Color).set(stars.color ?? 0xffffff);
    this.uniforms.uStarDensity.value = stars.density;
    this.uniforms.uStarThreshold.value = stars.threshold;
    this.uniforms.uStarSize.value = stars.size;
    this.uniforms.uStarTwinkleSpeed.value = stars.twinkleSpeed;
    this.uniforms.uStarMinElev.value = stars.minElev;
  }

  public setHorizon(opts: {
    offset?: number;
    mode?: HorizonMode;
    belowColor?: THREE.ColorRepresentation;
    blend?: number;
  }): void {
    if (opts.offset !== undefined) {
      this.uniforms.uHorizonOffset.value = opts.offset;
    }
    if (opts.mode !== undefined) {
      this.uniforms.uHorizonMode.value = opts.mode;
    }
    if (opts.belowColor !== undefined) {
      (this.uniforms.uBelowColor.value as THREE.Color).set(opts.belowColor);
    }
    if (opts.blend !== undefined) {
      this.uniforms.uHorizonBlend.value = opts.blend;
    }
  }
}
