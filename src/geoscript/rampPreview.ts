import type { RampSpecJson, RampStopJson } from './geotoyAPIClient';

/**
 * Client-side sampling of a ramp spec for editor previews (gradient bars). Mirrors the
 * wasm implementation's math: named easings, all four mix spaces, shorter-arc OKLCH hue.
 * Preview-grade — the wasm side remains the source of truth for rendered output.
 */

type V3 = [number, number, number];

const srgbC2Lin = (c: number) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const linC2Srgb = (c: number) => (c <= 0.0031308 ? c * 12.92 : 1.055 * c ** (1 / 2.4) - 0.055);

export const linearToSrgb = (c: V3): V3 => [linC2Srgb(c[0]), linC2Srgb(c[1]), linC2Srgb(c[2])];
export const srgbToLinear = (c: V3): V3 => [srgbC2Lin(c[0]), srgbC2Lin(c[1]), srgbC2Lin(c[2])];

const linearToOklab = (c: V3): V3 => {
  const l = Math.cbrt(0.4122214708 * c[0] + 0.5363325363 * c[1] + 0.0514459929 * c[2]);
  const m = Math.cbrt(0.2119034982 * c[0] + 0.6806995451 * c[1] + 0.1073969566 * c[2]);
  const s = Math.cbrt(0.0883024619 * c[0] + 0.2817188376 * c[1] + 0.6299787005 * c[2]);
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
};

const oklabToLinear = (c: V3): V3 => {
  const l = (c[0] + 0.3963377774 * c[1] + 0.2158037573 * c[2]) ** 3;
  const m = (c[0] - 0.1055613458 * c[1] - 0.0638541728 * c[2]) ** 3;
  const s = (c[0] - 0.0894841775 * c[1] - 1.291485548 * c[2]) ** 3;
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ];
};

const ease = (name: RampStopJson['ease'], t: number): number => {
  switch (name) {
    case 'linear':
      return t;
    case 'smooth':
      return t * t * (3 - 2 * t);
    case 'smoother':
      return t * t * t * (t * (t * 6 - 15) + 10);
    case 'step':
      return 0;
  }
};

const toSpace = (space: RampSpecJson['space'], v: V3): V3 => {
  switch (space) {
    case 'linear':
      return v;
    case 'srgb':
      return linearToSrgb(v);
    case 'oklab':
      return linearToOklab(v);
    case 'oklch': {
      const [L, a, b] = linearToOklab(v);
      return [L, Math.hypot(a, b), Math.atan2(b, a)];
    }
  }
};

const fromSpace = (space: RampSpecJson['space'], v: V3): V3 => {
  const clamp01 = (c: V3): V3 => [
    Math.min(1, Math.max(0, c[0])),
    Math.min(1, Math.max(0, c[1])),
    Math.min(1, Math.max(0, c[2])),
  ];
  switch (space) {
    case 'linear':
      return v;
    case 'srgb':
      return clamp01(srgbToLinear(v));
    case 'oklab':
      return clamp01(oklabToLinear(v));
    case 'oklch':
      return clamp01(oklabToLinear([v[0], v[1] * Math.cos(v[2]), v[1] * Math.sin(v[2])]));
  }
};

const mix = (space: RampSpecJson['space'], a: V3, b: V3, t: number): V3 => {
  if (space === 'oklch') {
    let [ha, hb] = [a[2], b[2]];
    if (a[1] < 1e-4) ha = hb;
    if (b[1] < 1e-4) hb = ha;
    const dh = ((((hb - ha + Math.PI) % (2 * Math.PI)) + 2 * Math.PI) % (2 * Math.PI)) - Math.PI;
    return [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, ha + dh * t];
  }
  return [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
};

const stopV3 = (s: RampStopJson): V3 =>
  s.value.length >= 3 ? [s.value[0], s.value[1], s.value[2]] : [s.value[0], s.value[0], s.value[0]];

/** Sample the ramp at `x` (raw input units, clamped to the stop extent) → linear RGB
 *  (scalar ramps replicate the value across channels). */
export const sampleRampSpec = (spec: RampSpecJson, x: number): V3 => {
  const stops = spec.stops;
  if (stops.length === 0) return [0, 0, 0];
  if (stops.length === 1) return stopV3(stops[0]);
  const space = spec.scalar ? 'linear' : spec.space;
  const u = Math.min(stops[stops.length - 1].pos, Math.max(stops[0].pos, x));
  let idx = stops.length;
  for (let i = 0; i < stops.length; i += 1) {
    if (stops[i].pos > u) {
      idx = i;
      break;
    }
  }
  if (idx === 0) return stopV3(stops[0]);
  if (idx >= stops.length) return stopV3(stops[stops.length - 1]);
  const [s0, s1] = [stops[idx - 1], stops[idx]];
  const t = s1.pos > s0.pos ? (u - s0.pos) / (s1.pos - s0.pos) : 1;
  return fromSpace(
    space,
    mix(space, toSpace(space, stopV3(s0)), toSpace(space, stopV3(s1)), ease(s0.ease, t))
  );
};

/** Paint the full gradient (sRGB-encoded for display) across a canvas. */
export const drawRampPreview = (canvas: HTMLCanvasElement, spec: RampSpecJson) => {
  const ctx2d = canvas.getContext('2d');
  if (!ctx2d || spec.stops.length === 0) return;
  const { width: w, height: h } = canvas;
  const img = ctx2d.createImageData(w, 1);
  const lo = spec.stops[0].pos;
  const hi = spec.stops[spec.stops.length - 1].pos;
  for (let i = 0; i < w; i += 1) {
    const s = linearToSrgb(sampleRampSpec(spec, lo + ((hi - lo) * i) / (w - 1)));
    img.data[i * 4] = Math.round(Math.min(1, Math.max(0, s[0])) * 255);
    img.data[i * 4 + 1] = Math.round(Math.min(1, Math.max(0, s[1])) * 255);
    img.data[i * 4 + 2] = Math.round(Math.min(1, Math.max(0, s[2])) * 255);
    img.data[i * 4 + 3] = 255;
  }
  for (let y = 0; y < h; y += 1) ctx2d.putImageData(img, 0, y);
};
