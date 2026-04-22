import * as THREE from 'three';

import type { GradientStop } from './gradient';

type RGB = readonly [number, number, number];

/**
 * Linear-RGB → Oklab. Mirror of the GLSL `rgbToOklab` in the old prelude.
 * Input is linear (non-gamma-corrected) RGB, which matches THREE.Color's
 * internal representation.
 *
 * Ref: https://bottosson.github.io/posts/oklab/
 */
const rgbToOklab = (r: number, g: number, b: number): RGB => {
  const l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
  const m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
  const s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;
  const l_ = Math.cbrt(Math.max(l, 0));
  const m_ = Math.cbrt(Math.max(m, 0));
  const s_ = Math.cbrt(Math.max(s, 0));
  return [
    0.2104542553 * l_ + 0.793617785 * m_ - 0.0040720468 * s_,
    1.9779984951 * l_ - 2.428592205 * m_ + 0.4505937099 * s_,
    0.0259040371 * l_ + 0.7827717662 * m_ - 0.808675766 * s_,
  ];
};

const oklabToRgb = (L: number, a: number, b: number): RGB => {
  const l_ = L + 0.3963377774 * a + 0.2158037573 * b;
  const m_ = L - 0.1055613458 * a - 0.0638541728 * b;
  const s_ = L - 0.0894841775 * a - 1.291485548 * b;
  const l = l_ * l_ * l_;
  const m = m_ * m_ * m_;
  const s = s_ * s_ * s_;
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.695608256 * s,
  ];
};

const oklabMix = (a: RGB, b: RGB, t: number): RGB => {
  const [la0, la1, la2] = rgbToOklab(a[0], a[1], a[2]);
  const [lb0, lb1, lb2] = rgbToOklab(b[0], b[1], b[2]);
  return oklabToRgb(la0 + t * (lb0 - la0), la1 + t * (lb1 - la1), la2 + t * (lb2 - la2));
};

/**
 * Precompute a 1D LUT of `size` samples across elevation [0, 1] by walking
 * the authored stops and Oklab-interpolating between adjacent pairs. The
 * shader-side lookup does RGB linear lerp between adjacent LUT entries —
 * fine because adjacent entries are Oklab-close by construction.
 *
 * Colors are linear RGB (THREE.Color's `.r/.g/.b` fields).
 */
export const computeGradientLut = (stops: GradientStop[], size: number): RGB[] => {
  if (stops.length < 1) {
    throw new Error('computeGradientLut: at least one stop required');
  }
  if (size < 2) {
    throw new Error(`computeGradientLut: size must be >= 2, got ${size}`);
  }

  const rgbStops: { position: number; rgb: RGB }[] = stops.map(s => {
    const c = new THREE.Color(s.color);
    return { position: s.position, rgb: [c.r, c.g, c.b] };
  });

  const out: RGB[] = new Array(size);
  for (let i = 0; i < size; i++) {
    const t = i / (size - 1);

    if (t <= rgbStops[0].position) {
      out[i] = rgbStops[0].rgb;
      continue;
    }

    let placed = false;
    for (let k = 1; k < rgbStops.length; k++) {
      const p1 = rgbStops[k].position;
      if (t <= p1) {
        const p0 = rgbStops[k - 1].position;
        const f = Math.min(1, Math.max(0, (t - p0) / Math.max(p1 - p0, 1e-6)));
        out[i] = oklabMix(rgbStops[k - 1].rgb, rgbStops[k].rgb, f);
        placed = true;
        break;
      }
    }
    if (!placed) {
      out[i] = rgbStops[rgbStops.length - 1].rgb;
    }
  }
  return out;
};
