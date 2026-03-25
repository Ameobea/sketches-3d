import * as THREE from 'three';
import type { Viz } from 'src/viz';
import {
  buildCustomShader,
  type CustomShaderMaterial,
  type CustomShaderProps,
} from 'src/viz/shaders/customShader';
import BridgeMistColorShader from 'src/viz/shaders/bridge2/bridge_top_mist/color.frag?raw';

export const DEFAULT_CHECKPOINT_COLOR: [number, number, number] = [0.8, 0.1, 0.645 * 2];

/**
 * The bridge-top-mist color shader with all sentinels filled in with their default values,
 * including the default checkpoint color. Import this instead of the raw .frag file when using
 * the shader outside of buildCheckpointMaterial (e.g. passed directly to buildCustomShader).
 */
export const BridgeMistColorShaderDefault = BridgeMistColorShader.replace(
  'vec4 outColor = vec4(0.8, 0.5, 0.6, 0.0);',
  `vec4 outColor = vec4(${DEFAULT_CHECKPOINT_COLOR[0].toFixed(8)}, ${DEFAULT_CHECKPOINT_COLOR[1].toFixed(8)}, ${DEFAULT_CHECKPOINT_COLOR[2].toFixed(8)}, 0.0);`
)
  .replace('__NOISE_ROT__', 'mat3(1.0)')
  .replace('__NOISE_DIR__', 'vec3(0.0000, 1.0000, -3.0000)')
  .replace('__NOISE_FREQ__', 'vec3(3.6000, 0.3000, 0.6000)')
  .replace('__NOISE_POS_QUANT__', '0.02')
  .replaceAll('__NOISE_BIAS__', '-0.2')
  .replaceAll('__NOISE_POW__', '0.62')
  .replaceAll('__NOISE_QUANT__', '0.042')
  .replaceAll('__NOISE_MULTIPLIER__', '1.0')
  .replace('__FADE_DEFS__', '')
  .replace('__EDGE_WARP_DEFS__', '');
// Note: __BREEZE_*__ sentinels are inside #ifdef EDGE_WARP_ACTIVE which is inactive here,
// so they remain unresolved but are safely skipped by the GLSL preprocessor.

export interface CheckpointMaterialOptions {
  /** Direction (and speed) of noise animation. Default vec3(0, 1, -3). */
  noiseDir?: [number, number, number];
  /**
   * Per-axis noise sampling frequency applied just before the FBM call.
   * Default vec3(3.6, 0.3, 0.6).
   */
  noiseFreq?: [number, number, number];
  /**
   * Euler angles [x, y, z] in radians applied to the noise sampling coordinates.
   * Baked in as a compile-time mat3 constant. Default: identity.
   */
  noiseRotation?: [number, number, number];
  /** Step size for quantizing the noise sampling position. Default 0.02. */
  noisePosQuantize?: number;
  /** Added to raw FBM output before the power curve (negative = threshold cutoff). Default -0.2. */
  noiseBias?: number;
  /** Exponent after bias subtraction. Default 0.62. */
  noisePow?: number;
  /** Step size for quantizing the post-power noise value. Default 0.042. */
  noiseQuantize?: number;
  /** Final scalar multiplier on noise before clamping. Default 1.0. */
  noiseMultiplier?: number;
  /**
   * Normalized vertical position [0,1] where the vertical bias taper begins. Default 0.3.
   * Only meaningful when noiseVertBiasAmtLo/Hi are set.
   */
  noiseVertBiasLo?: number;
  /**
   * Normalized vertical position [0,1] where the taper reaches full effect. Default 1.0.
   * Must be > noiseVertBiasLo. Only meaningful when noiseVertBiasAmtLo/Hi are set.
   */
  noiseVertBiasHi?: number;
  /**
   * Bias value added at noiseVertBiasLo (bottom of taper range). Default 0.
   * Setting this (or noiseVertBiasAmtHi) activates the vertical bias block.
   */
  noiseVertBiasAmtLo?: number;
  /**
   * Bias value added at noiseVertBiasHi (top of taper range). Default 0.
   * To thin flame tips, use a negative value here (e.g. -0.8 with noiseBias=-0.2
   * gives noise + (-0.2) + (-0.8) = noise - 1.0 at the very top).
   */
  noiseVertBiasAmtHi?: number;

  // ── X-axis fade ────────────────────────────────────────────────────────────
  /**
   * World-space X position where the alpha fade begins (smoothstep lo edge).
   * When set alongside xFadeHi, multiplies alpha by `1 - smoothstep(lo, hi, pos.x)`
   * so the material fades out as X increases toward hi.
   */
  xFadeLo?: number;
  /** World-space X position where the alpha reaches zero (smoothstep hi edge). */
  xFadeHi?: number;

  // ── Vertical fade ──────────────────────────────────────────────────────────
  /**
   * Distance from the top of the bbox over which alpha fades 1→0, as a fraction of bbox height.
   * 0 = disabled. E.g. 0.125 fades the top 12.5% of the mesh.
   */
  fadeTopDist?: number;
  /** Exponent on the top fade ramp. 1 = linear, >1 = steeper. Default 1. */
  fadeTopSteepness?: number;
  /**
   * Distance from the bottom of the bbox over which alpha fades 1→0, as a fraction of bbox height.
   * 0 = disabled.
   */
  fadeBottomDist?: number;
  /** Exponent on the bottom fade ramp. Default 1. */
  fadeBottomSteepness?: number;

  // ── Edge warp + breeze ─────────────────────────────────────────────────────
  /**
   * Amplitude of the noise that perturbs the fade edges, as a fraction of bbox height.
   * 0 = disabled (default). Enables the entire EDGE_WARP_ACTIVE block including breeze.
   */
  fadeEdgeAmp?: number;
  /** XZ sampling frequency for edge-perturbation noise. Default 0.08. */
  fadeEdgeFreq?: number;
  /** Per-axis drift speed (units/second) for edge noise in X and Z. Default [0.15, 0.1]. */
  fadeEdgeSpeed?: [number, number];

  // ── Breeze ─────────────────────────────────────────────────────────────────
  /** Rate at which the 1D breeze envelope noise oscillates. Default 0.75. */
  breezeTimeFreq?: number;
  /**
   * Smoothstep lo edge for the breeze envelope. [0,1].
   * Higher = rarer breezes. Default 0.5.
   */
  breezeThreshold?: number;
  /**
   * Smoothstep hi edge for the breeze envelope. [0,1], must be > breezeThreshold.
   * Narrower gap = snappier onset. Default 1.0.
   */
  breezeThresholdHi?: number;
  /**
   * Scale factor applied to the PM modulator noise relative to the main edge sample.
   * Lower = broader displacement blobs. Default 0.5.
   */
  breezeModScale?: number;
  /**
   * PM depth: how far (in noise space) the sample point is displaced at full breeze.
   * Default 1.8.
   */
  breezePmDepth?: number;
  /** Peak edge-warp amplitude multiplier at full breeze (additive factor). Default 2.5. */
  breezeAmpMult?: number;
  /**
   * Optional color mixed toward at full breeze. When omitted, no color shift occurs
   * (equivalent to mixing with the base color at strength 0).
   */
  breezeHotColor?: [number, number, number];
  /**
   * Blend strength toward breezeHotColor at full breeze. Default 0 (disabled).
   * Has no effect when breezeHotColor is not set.
   */
  breezeColorMix?: number;
  /**
   * Amount added to noiseBias at full breeze (positive = more noise visible / denser flame).
   * Default 0.
   */
  breezeBiasDelta?: number;
  /**
   * Additional noise amplitude multiplier at full breeze (1.0 + breezeT * this).
   * Default 0.
   */
  breezeNoiseAmpMult?: number;
}

/**
 * A CustomShaderMaterial produced by buildCheckpointMaterial, extended with a `setMesh`
 * helper that computes the world-space bounding box and updates the fade uniforms.
 */
export type CheckpointMaterial = CustomShaderMaterial & {
  setMesh(mesh: THREE.Mesh): void;
};

const buildNoiseRotGLSL = (rotation: [number, number, number]): string => {
  const m = new THREE.Matrix4();
  m.makeRotationFromEuler(new THREE.Euler(rotation[0], rotation[1], rotation[2], 'XYZ'));
  const e = m.elements;
  // Three.js Matrix4 elements are column-major; extract top-left 3x3 into GLSL mat3(col0,col1,col2)
  const f = (n: number) => n.toFixed(6);
  return `mat3(${f(e[0])},${f(e[1])},${f(e[2])},${f(e[4])},${f(e[5])},${f(e[6])},${f(e[8])},${f(e[9])},${f(e[10])})`;
};

const v3 = ([r, g, b]: [number, number, number]) => `vec3(${r.toFixed(6)}, ${g.toFixed(6)}, ${b.toFixed(6)})`;

export const buildCheckpointMaterial = (
  viz: Viz,
  color: [number, number, number] = DEFAULT_CHECKPOINT_COLOR,
  extraProps: Partial<CustomShaderProps> = {},
  options: CheckpointMaterialOptions = {}
): CheckpointMaterial => {
  const [nx, ny, nz] = options.noiseDir ?? [0, 1, -3];
  const [fx, fy, fz] = options.noiseFreq ?? [3.6, 0.3, 0.6];
  const noiseRotGLSL = options.noiseRotation ? buildNoiseRotGLSL(options.noiseRotation) : 'mat3(1.0)';
  const f = (n: number) => n.toFixed(6);

  const mat = buildCustomShader(
    { metalness: 0, alphaTest: 0.05, transparent: true, ambientLightScale: 2, ...extraProps },
    {
      colorShader: BridgeMistColorShader.replace(
        'vec4 outColor = vec4(0.8, 0.5, 0.6, 0.0);',
        `vec4 outColor = vec4(${color[0].toFixed(8)}, ${color[1].toFixed(8)}, ${color[2].toFixed(8)}, 0.0);`
      )
        .replace('__NOISE_ROT__', noiseRotGLSL)
        .replace('__NOISE_DIR__', `vec3(${nx.toFixed(4)}, ${ny.toFixed(4)}, ${nz.toFixed(4)})`)
        .replace('__NOISE_FREQ__', `vec3(${fx.toFixed(4)}, ${fy.toFixed(4)}, ${fz.toFixed(4)})`)
        .replace('__NOISE_POS_QUANT__', f(options.noisePosQuantize ?? 0.02))
        .replaceAll('__NOISE_BIAS__', f(options.noiseBias ?? -0.2))
        .replaceAll('__NOISE_POW__', f(options.noisePow ?? 0.62))
        .replaceAll('__NOISE_QUANT__', f(options.noiseQuantize ?? 0.042))
        .replaceAll('__NOISE_MULTIPLIER__', f(options.noiseMultiplier ?? 1.0))
        .replace(
          '__FADE_DEFS__',
          [
            options.fadeTopDist || options.fadeBottomDist ? '#define FADE_ACTIVE' : '',
            options.noiseVertBiasAmtLo !== undefined || options.noiseVertBiasAmtHi !== undefined
              ? '#define VERT_BIAS_ACTIVE'
              : '',
            options.xFadeLo !== undefined && options.xFadeHi !== undefined ? '#define X_FADE_ACTIVE' : '',
          ]
            .filter(Boolean)
            .join('\n')
        )
        .replace('__EDGE_WARP_DEFS__', options.fadeEdgeAmp ? '#define EDGE_WARP_ACTIVE' : '')
        // Breeze sentinels — only live inside #ifdef EDGE_WARP_ACTIVE so safe to always replace
        .replace('__BREEZE_TIME_FREQ__', f(options.breezeTimeFreq ?? 0.75))
        .replace('__VERT_BIAS_LO__', f(options.noiseVertBiasLo ?? 0.3))
        .replace('__VERT_BIAS_HI__', f(options.noiseVertBiasHi ?? 1.0))
        .replace('__VERT_BIAS_AMT_LO__', f(options.noiseVertBiasAmtLo ?? 0.0))
        .replace('__VERT_BIAS_AMT_HI__', f(options.noiseVertBiasAmtHi ?? 0.0))
        .replace('__X_FADE_LO__', f(options.xFadeLo ?? 0.0))
        .replace('__X_FADE_HI__', f(options.xFadeHi ?? 0.0))
        .replace('__BREEZE_THRESHOLD__', f(options.breezeThreshold ?? 0.5))
        .replace('__BREEZE_THRESHOLD_HI__', f(options.breezeThresholdHi ?? 1.0))
        .replace('__BREEZE_BIAS_DELTA__', f(options.breezeBiasDelta ?? 0.0))
        .replace('__BREEZE_NOISE_AMP_MULT__', f(options.breezeNoiseAmpMult ?? 0.0))
        .replace('__BREEZE_HOT_COLOR__', options.breezeHotColor ? v3(options.breezeHotColor) : 'outColor.rgb')
        .replace('__BREEZE_COLOR_MIX__', f(options.breezeColorMix ?? 0.0))
        .replaceAll('__BREEZE_MOD_SCALE__', f(options.breezeModScale ?? 0.5))
        .replace('__BREEZE_PM_DEPTH__', f(options.breezePmDepth ?? 1.8))
        .replace('__BREEZE_AMP_MULT__', f(options.breezeAmpMult ?? 2.5)),
    },
    { disableToneMapping: true }
  );

  // Vertical fade + edge-warp uniforms — always present so the shader compiles.
  // fadeTopDist, fadeBottomDist, fadeEdgeAmp are stored as fractions of bbox height and
  // converted to world-space units in setMesh(). Initial values use DEFAULT_BBOX_HEIGHT
  // as a stand-in so the material looks reasonable before setMesh is called.
  const DEFAULT_BBOX_HEIGHT = 4;
  const [ex, ez] = options.fadeEdgeSpeed ?? [0.15, 0.1];
  mat.uniforms.bboxYMin = { value: 0 };
  mat.uniforms.bboxYMax = { value: DEFAULT_BBOX_HEIGHT };
  mat.uniforms.fadeTopDist = { value: (options.fadeTopDist ?? 0) * DEFAULT_BBOX_HEIGHT };
  mat.uniforms.fadeTopSteepness = { value: options.fadeTopSteepness ?? 1 };
  mat.uniforms.fadeBottomDist = { value: (options.fadeBottomDist ?? 0) * DEFAULT_BBOX_HEIGHT };
  mat.uniforms.fadeBottomSteepness = { value: options.fadeBottomSteepness ?? 1 };
  mat.uniforms.fadeEdgeAmp = { value: (options.fadeEdgeAmp ?? 0) * DEFAULT_BBOX_HEIGHT };
  mat.uniforms.fadeEdgeFreq = { value: options.fadeEdgeFreq ?? 0.08 };
  mat.uniforms.fadeEdgeSpeed = { value: new THREE.Vector2(ex, ez) };

  const checkpointMat = mat as CheckpointMaterial;
  checkpointMat.setMesh = (mesh: THREE.Mesh) => {
    const bbox = new THREE.Box3().setFromObject(mesh);
    const height = bbox.max.y - bbox.min.y;
    mat.uniforms.bboxYMin.value = bbox.min.y;
    mat.uniforms.bboxYMax.value = bbox.max.y;
    mat.uniforms.fadeTopDist.value = (options.fadeTopDist ?? 0) * height;
    mat.uniforms.fadeBottomDist.value = (options.fadeBottomDist ?? 0) * height;
    mat.uniforms.fadeEdgeAmp.value = (options.fadeEdgeAmp ?? 0) * height;
  };

  viz.registerBeforeRenderCb(curTimeSeconds => mat.setCurTimeSeconds(curTimeSeconds));
  return checkpointMat;
};
