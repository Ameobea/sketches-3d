import * as THREE from 'three';
import type { Viz } from 'src/viz';

import CheckpointVertexShader from './checkpoint.vert?raw';
import CheckpointFragmentShader from './checkpoint.frag?raw';
import depthExactVertexBody from 'src/viz/shaders/depthExactVertex.glsl?raw';
import commonShaderCode from 'src/viz/shaders/common.frag?raw';
import noiseShaderCode from 'src/viz/shaders/noise.frag?raw';

export const DEFAULT_CHECKPOINT_COLOR: [number, number, number] = [0.8, 0.1, 0.645 * 2];

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

  // ── Cap (normal-aware) noise ───────────────────────────────────────────────
  /**
   * When true, fragments whose world-space normal points mostly up or down
   * blend toward an alternate noise sampling that looks good on horizontal
   * surfaces (e.g. the tops of flame-column portals). Pass-through on vertical
   * faces — existing side tuning is not disturbed. Default: true.
   */
  capEnabled?: boolean;
  /**
   * Extra Euler rotation (XYZ, radians) applied *after* noiseRotation when
   * sampling the cap noise field. Default [PI/2, 0, 0] — a 90° rotation about
   * X, which swaps the noise Y and Z axes so the "flame flow up" direction of
   * the noise field projects onto the world XZ plane. The result looks like
   * cross-sections through the same flame column instead of a single static
   * slice.
   */
  capNoiseRotation?: [number, number, number];
  /**
   * |worldNormal.y| below which the cap branch is a full pass-through (pure
   * side noise). Default 0.5 — keeps side walls and up to ~60°-tilted faces
   * untouched.
   */
  capBlendLo?: number;
  /**
   * |worldNormal.y| at which the cap branch is fully active. Default 0.85.
   */
  capBlendHi?: number;

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

export interface CheckpointMaterialExtras {
  /** Optional material name (forwarded to THREE.Material.name). */
  name?: string;
  /**
   * Final multiplier on output RGB. Use this to push the material brighter
   * so it crosses the emissive bloom threshold. Replaces the old
   * `ambientLightScale` knob, which is meaningless now that the material is
   * unlit. Default 1.
   */
  intensity?: number;
  /** Discard threshold on computed alpha. Default 0.05. */
  alphaTest?: number;
  /**
   * When true, only front faces are rendered (`THREE.FrontSide`). Use this for
   * portal meshes whose geometry exactly touches a surrounding frame — rendering
   * back faces at the same depth as the frame surface causes z-fighting.
   * Default false (DoubleSide, so the interior is visible when looking in from any angle).
   */
  frontFaceOnly?: boolean;
}

/**
 * A plain THREE.ShaderMaterial with a `setMesh` helper that computes the
 * world-space bounding box of a mesh and updates the fade uniforms.
 */
export type CheckpointMaterial = THREE.ShaderMaterial & {
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

const buildCheckpointVertexShader = () =>
  CheckpointVertexShader.replace('__DEPTH_EXACT_VERTEX_BODY__', depthExactVertexBody);

const buildCheckpointFragmentShader = (
  color: [number, number, number],
  options: CheckpointMaterialOptions,
  alphaTest: number
): string => {
  const [nx, ny, nz] = options.noiseDir ?? [0, 1, -3];
  const [fx, fy, fz] = options.noiseFreq ?? [3.6, 0.3, 0.6];
  const noiseRotGLSL = options.noiseRotation ? buildNoiseRotGLSL(options.noiseRotation) : 'mat3(1.0)';
  const f = (n: number) => n.toFixed(6);

  const capEnabled = options.capEnabled ?? true;
  const capNoiseRotGLSL = buildNoiseRotGLSL(options.capNoiseRotation ?? [Math.PI / 2, 0, 0]);

  const helpers = `${commonShaderCode}\n${noiseShaderCode}\n`;

  return (
    helpers +
    CheckpointFragmentShader.replaceAll('__BASE_COLOR__', v3(color))
      .replaceAll('__NOISE_ROT__', noiseRotGLSL)
      .replaceAll('__NOISE_DIR__', `vec3(${nx.toFixed(4)}, ${ny.toFixed(4)}, ${nz.toFixed(4)})`)
      .replaceAll('__NOISE_FREQ__', `vec3(${fx.toFixed(4)}, ${fy.toFixed(4)}, ${fz.toFixed(4)})`)
      .replaceAll('__NOISE_POS_QUANT__', f(options.noisePosQuantize ?? 0.02))
      .replaceAll('__NOISE_BIAS__', f(options.noiseBias ?? -0.2))
      .replaceAll('__NOISE_POW__', f(options.noisePow ?? 0.62))
      .replaceAll('__NOISE_QUANT__', f(options.noiseQuantize ?? 0.042))
      .replaceAll('__NOISE_MULTIPLIER__', f(options.noiseMultiplier ?? 1.0))
      .replaceAll(
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
      .replaceAll('__EDGE_WARP_DEFS__', options.fadeEdgeAmp ? '#define EDGE_WARP_ACTIVE' : '')
      .replaceAll('__CAP_DEFS__', capEnabled ? '#define CAP_ACTIVE' : '')
      .replaceAll('__CAP_NOISE_ROT__', capNoiseRotGLSL)
      .replaceAll('__CAP_BLEND_LO__', f(options.capBlendLo ?? 0.5))
      .replaceAll('__CAP_BLEND_HI__', f(options.capBlendHi ?? 0.85))
      .replaceAll('__ALPHA_TEST__', f(alphaTest))
      // Breeze sentinels — only live inside #ifdef EDGE_WARP_ACTIVE so safe to always replace
      .replaceAll('__BREEZE_TIME_FREQ__', f(options.breezeTimeFreq ?? 0.75))
      .replaceAll('__VERT_BIAS_LO__', f(options.noiseVertBiasLo ?? 0.3))
      .replaceAll('__VERT_BIAS_HI__', f(options.noiseVertBiasHi ?? 1.0))
      .replaceAll('__VERT_BIAS_AMT_LO__', f(options.noiseVertBiasAmtLo ?? 0.0))
      .replaceAll('__VERT_BIAS_AMT_HI__', f(options.noiseVertBiasAmtHi ?? 0.0))
      .replaceAll('__X_FADE_LO__', f(options.xFadeLo ?? 0.0))
      .replaceAll('__X_FADE_HI__', f(options.xFadeHi ?? 0.0))
      .replaceAll('__BREEZE_THRESHOLD_HI__', f(options.breezeThresholdHi ?? 1.0))
      .replaceAll('__BREEZE_THRESHOLD__', f(options.breezeThreshold ?? 0.5))
      .replaceAll('__BREEZE_BIAS_DELTA__', f(options.breezeBiasDelta ?? 0.0))
      .replaceAll('__BREEZE_NOISE_AMP_MULT__', f(options.breezeNoiseAmpMult ?? 0.0))
      .replaceAll('__BREEZE_HOT_COLOR__', options.breezeHotColor ? v3(options.breezeHotColor) : v3(color))
      .replaceAll('__BREEZE_COLOR_MIX__', f(options.breezeColorMix ?? 0.0))
      .replaceAll('__BREEZE_MOD_SCALE__', f(options.breezeModScale ?? 0.5))
      .replaceAll('__BREEZE_PM_DEPTH__', f(options.breezePmDepth ?? 1.8))
      .replaceAll('__BREEZE_AMP_MULT__', f(options.breezeAmpMult ?? 2.5))
  );
};

export const buildCheckpointMaterial = (
  viz: Viz,
  color: [number, number, number] = DEFAULT_CHECKPOINT_COLOR,
  extras: CheckpointMaterialExtras = {},
  options: CheckpointMaterialOptions = {}
): CheckpointMaterial => {
  const alphaTest = extras.alphaTest ?? 0.05;

  // Fade/edge-warp uniforms use `fraction of bbox height` as their authored
  // unit, but the shader consumes world-space distance. setMesh() rewrites
  // these when the mesh is known; until then we use a stand-in bbox height so
  // the material at least looks reasonable.
  const DEFAULT_BBOX_HEIGHT = 4;
  const [ex, ez] = options.fadeEdgeSpeed ?? [0.15, 0.1];

  const mat = new THREE.ShaderMaterial({
    vertexShader: buildCheckpointVertexShader(),
    fragmentShader: buildCheckpointFragmentShader(color, options, alphaTest),
    glslVersion: THREE.GLSL3,
    transparent: true,
    side: extras.frontFaceOnly ? THREE.FrontSide : THREE.DoubleSide,
    depthWrite: false,
    uniforms: {
      curTimeSeconds: { value: 0 },
      intensity: { value: extras.intensity ?? 1 },
      bboxYMin: { value: 0 },
      bboxYMax: { value: DEFAULT_BBOX_HEIGHT },
      fadeTopDist: { value: (options.fadeTopDist ?? 0) * DEFAULT_BBOX_HEIGHT },
      fadeTopSteepness: { value: options.fadeTopSteepness ?? 1 },
      fadeBottomDist: { value: (options.fadeBottomDist ?? 0) * DEFAULT_BBOX_HEIGHT },
      fadeBottomSteepness: { value: options.fadeBottomSteepness ?? 1 },
      fadeEdgeAmp: { value: (options.fadeEdgeAmp ?? 0) * DEFAULT_BBOX_HEIGHT },
      fadeEdgeFreq: { value: options.fadeEdgeFreq ?? 0.08 },
      fadeEdgeSpeed: { value: new THREE.Vector2(ex, ez) },
    },
  });

  mat.toneMapped = false;
  if (extras.name) {
    mat.name = extras.name;
  }

  mat.userData.emissiveBypass = true;
  mat.userData.occlusionExclude = true;

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

  viz.registerBeforeRenderCb(curTimeSeconds => {
    mat.uniforms.curTimeSeconds.value = curTimeSeconds;
  });
  return checkpointMat;
};
