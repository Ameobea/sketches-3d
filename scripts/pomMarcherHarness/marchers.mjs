// Shared CPU port of the POM marchers in `src/viz/shaders/pom.ts`, for validating a
// material's height field against a dense ground-truth first-hit without the browser.
// Keep in sync with pom.ts: fixed march = pomMarchProjected; safe march =
// pomMarchProjectedSafe / pomMarchGridSafe (both reduce to "evaluate height at uv" on
// the CPU, so one port covers projectedField + grid); refines mirror their _pom* twins
// including the adaptive POM_REFINE_TOL early-out.

export const fract = x => x - Math.floor(x);
export const clamp = (x, a, b) => Math.min(Math.max(x, a), b);
export const sign = x => (x > 0 ? 1 : x < 0 ? -1 : 0);
export const mix = (a, b, w) => a * (1 - w) + b * w;
export const smoothstep = (e0, e1, x) => {
  const t = clamp((x - e0) / (e1 - e0), 0, 1);
  return t * t * (3 - 2 * t);
};

// A material field: { height(uv)->[0,1] carve, lat(uv)->gridLateralDist (optional),
//   depth, steps, minFeature, refine (binary steps), refineTol (uv units, =0.01*minFeature
//   in the shader; 0 disables the adaptive early-out) }.
// surf() mirrors _pomSurfUv/_pomSurfGrid: clamp(height,0,0.8)*depth.
const at = (uv0, duv, s) => [uv0[0] + duv[0] * s, uv0[1] + duv[1] * s];

// Dense first-hit of rayDepth=s*NdotV with surf(uv); the reference every marcher is scored against.
export const truth = (uv0, duv, NdotV, marchLen, F) => {
  const N = 8000;
  const surf = uv => clamp(F.height(uv), 0, 0.8) * F.depth;
  let sPrev = 0;
  if (0 >= surf(uv0)) return 0;
  for (let i = 1; i <= N; i++) {
    const s = (marchLen * i) / N;
    if (s * NdotV >= surf(at(uv0, duv, s))) {
      let lo = sPrev,
        hi = s;
      for (let b = 0; b < 40; b++) {
        const m = 0.5 * (lo + hi);
        if (m * NdotV >= surf(at(uv0, duv, m))) hi = m;
        else lo = m;
      }
      return hi;
    }
    sPrev = s;
  }
  return sPrev;
};

// pomMarchProjected: uniform dStep, seeds hPrev=0 (the pre-safeStep baseline).
export const fixedMarch = (uv0, duv, NdotV, marchLen, F) => {
  let evals = 0;
  const surf = uv => {
    evals++;
    return clamp(F.height(uv), 0, 0.8) * F.depth;
  };
  const dStep = marchLen / F.steps;
  let sPrev = 0,
    hPrev = 0,
    dPrev = 0;
  for (let i = 1; i <= F.steps; i++) {
    const s = dStep * i,
      rd = s * NdotV,
      h = surf(at(uv0, duv, s));
    if (rd >= h) {
      const overshoot = rd - h,
        prevGap = hPrev - dPrev,
        span = overshoot + prevGap;
      const w = span > 1e-6 ? overshoot / span : 0;
      if (prevGap <= 1e-6) return { s: mix(s, sPrev, w), evals };
      let lo = sPrev,
        hi = s;
      for (let b = 0; b < F.refine; b++) {
        const m = 0.5 * (lo + hi);
        if (m * NdotV >= surf(at(uv0, duv, m))) hi = m;
        else lo = m;
      }
      return { s: hi, evals };
    }
    sPrev = s;
    hPrev = h;
    dPrev = rd;
  }
  return { s: sPrev, evals };
};

// pomMarchProjectedSafe / pomMarchGridSafe: s=0 priming sample, adaptive stride
// max(featStride, minStride, deadline-cover), bracket + adaptive-refine.
export const safeMarch = (uv0, duv, NdotV, marchLen, F) => {
  let evals = 0;
  const surf = uv => {
    evals++;
    return clamp(F.height(uv), 0, 0.8) * F.depth;
  };
  const latSpeed = Math.max(Math.hypot(duv[0], duv[1]), 1e-4);
  const minStride = F.minFeature / latSpeed;
  let s = 0,
    sPrev = 0,
    hPrev = 0,
    dPrev = 0;
  for (let i = 0; i < F.steps; i++) {
    const uv = at(uv0, duv, s),
      h = surf(uv),
      rd = s * NdotV;
    if (rd >= h) {
      if (i === 0) return { s: 0, evals };
      const overshoot = rd - h,
        prevGap = hPrev - dPrev,
        span = overshoot + prevGap;
      const w = span > 1e-6 ? overshoot / span : 0;
      if (prevGap <= 1e-6 || Math.abs(h - hPrev) <= 1e-3 * F.depth) return { s: mix(s, sPrev, w), evals };
      let lo = sPrev,
        hi = s;
      for (let b = 0; b < F.refine; b++) {
        if (F.refineTol && (hi - lo) * latSpeed <= F.refineTol) break;
        const m = 0.5 * (lo + hi);
        if (m * NdotV >= surf(at(uv0, duv, m))) hi = m;
        else lo = m;
      }
      return { s: hi, evals };
    }
    if (s >= marchLen) break;
    sPrev = s;
    hPrev = h;
    dPrev = rd;
    const featStride = F.lat ? F.lat(uv) / latSpeed : minStride;
    const cover = (marchLen - s) / Math.max(F.steps - 1 - i, 1);
    s = Math.min(s + Math.max(Math.max(featStride, minStride), cover), marchLen);
  }
  return { s: sPrev, evals };
};

// analytic (Tier-A): the material's closed-form first-hit when the ray stays in one cell;
// -1 sentinel -> safeStep fallback (cf. the planned pomMarchProjectedAnalytic). F.analyticHit
// returns hit s in [0, marchLen] or -1. Returns `fast` = whether the closed form applied.
export const analyticMarch = (uv0, duv, NdotV, marchLen, F) => {
  const s = F.analyticHit(uv0, duv, NdotV, marchLen);
  if (s >= 0) {
    return { s, evals: 0, fast: true };
  }
  const r = safeMarch(uv0, duv, NdotV, marchLen, F);
  return { s: r.s, evals: r.evals, fast: false };
};

// Score fixed + safe against truth over a "scanline" of parallel pixels (uv0 swept along
// `axis`, fixed ray dir + NdotV). classify(uv)->tag flags surface mis-hits; normGrad(uv)
// proxies relief-normal serration (jaggedness of the per-pixel series).
export const jag = arr => {
  let s = 0,
    n = 0;
  for (let i = 1; i < arr.length - 1; i++) {
    s += Math.abs(arr[i - 1] - 2 * arr[i] + arr[i + 1]);
    n++;
  }
  return s / n;
};

export const sweep = (NdotV, dir, uv0Fn, NP, F, { classify, normGrad } = {}) => {
  const marchLen = F.depth / Math.max(NdotV, 1e-3);
  const an = !!F.analyticHit;
  const r = {
    hitErr: { fixed: 0, safe: 0, analytic: 0 },
    classMiss: { fixed: 0, safe: 0, analytic: 0 },
    evals: { fixed: 0, safe: 0, analytic: 0 },
    fast: 0,
    jagNorm: {},
  };
  const tS = [],
    fS = [],
    sS = [],
    aS = [],
    tN = [],
    fN = [],
    sN = [],
    aN = [];
  for (let p = 0; p < NP; p++) {
    const uv0 = uv0Fn(p / NP);
    const t = truth(uv0, dir, NdotV, marchLen, F);
    const f = fixedMarch(uv0, dir, NdotV, marchLen, F);
    const s = safeMarch(uv0, dir, NdotV, marchLen, F);
    const a = an ? analyticMarch(uv0, dir, NdotV, marchLen, F) : null;
    r.hitErr.fixed += Math.abs(f.s - t);
    r.hitErr.safe += Math.abs(s.s - t);
    r.evals.fixed += f.evals;
    r.evals.safe += s.evals;
    if (classify) {
      if (classify(at(uv0, dir, f.s)) !== classify(at(uv0, dir, t))) r.classMiss.fixed++;
      if (classify(at(uv0, dir, s.s)) !== classify(at(uv0, dir, t))) r.classMiss.safe++;
    }
    if (a) {
      r.hitErr.analytic += Math.abs(a.s - t);
      r.evals.analytic += a.evals;
      if (a.fast) r.fast++;
      if (classify && classify(at(uv0, dir, a.s)) !== classify(at(uv0, dir, t))) r.classMiss.analytic++;
      aS.push(a.s);
      if (normGrad) aN.push(normGrad(at(uv0, dir, a.s)));
    }
    tS.push(t);
    fS.push(f.s);
    sS.push(s.s);
    if (normGrad) {
      tN.push(normGrad(at(uv0, dir, t)));
      fN.push(normGrad(at(uv0, dir, f.s)));
      sN.push(normGrad(at(uv0, dir, s.s)));
    }
  }
  const pct = x => ((100 * x) / NP).toFixed(1) + '%';
  const num = x => (x / NP).toFixed(4);
  return {
    NdotV,
    marchLen: +marchLen.toFixed(3),
    hitErr: {
      fixed: num(r.hitErr.fixed),
      safe: num(r.hitErr.safe),
      analytic: an ? num(r.hitErr.analytic) : undefined,
    },
    classMiss: classify
      ? {
          fixed: pct(r.classMiss.fixed),
          safe: pct(r.classMiss.safe),
          analytic: an ? pct(r.classMiss.analytic) : undefined,
        }
      : undefined,
    fastRate: an ? pct(r.fast) : undefined,
    jagNorm: normGrad
      ? {
          truth: jag(tN).toFixed(4),
          fixed: jag(fN).toFixed(4),
          safe: jag(sN).toFixed(4),
          analytic: an ? jag(aN).toFixed(4) : undefined,
        }
      : undefined,
    evals: { fixed: (r.evals.fixed / NP).toFixed(1), safe: (r.evals.safe / NP).toFixed(1) },
  };
};
