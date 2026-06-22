// grate_trench (projectedField, intersect: analytic w/ safeStep fallback). Mirrors
// src/levels/boost_nova/grateTrench.{common,height,normal}.glsl.
// Section 1 (smoothstep `height`): the historical grazing serration + the refine 3->8 fix,
//   which still governs the safeStep fallback path. Section 2 (linearstep `heightLin`): the
//   shipped Tier-A field — validates the closed-form `analyticHit` against dense truth and
//   reports the smoothstep->linearstep visual delta.
import { fract, clamp, sign, smoothstep, sweep } from './marchers.mjs';

const ALONG_X = true;
const PITCH = 6,
  HALF_W = 0.9,
  SLAT_PITCH = 0.5,
  GAP_HW = 0.13,
  WALL = 0.018,
  END = 0.1,
  END_WALL = 0.05;
const END_OUT = HALF_W - END; // 0.8
const CARVE = 0.8,
  DEPTH = 0.12;

const trenchOff = uv => (fract((ALONG_X ? uv[1] : uv[0]) / PITCH + 0.5) - 0.5) * PITCH;
const gapOff = uv => (fract((ALONG_X ? uv[0] : uv[1]) / SLAT_PITCH + 0.5) - 0.5) * SLAT_PITCH;

const height = uv => {
  const dv = Math.abs(trenchOff(uv));
  if (dv >= END_OUT) return 0;
  const g = Math.abs(gapOff(uv));
  return (
    CARVE * (1 - smoothstep(GAP_HW, GAP_HW + WALL, g)) * (1 - smoothstep(END_OUT - END_WALL, END_OUT, dv))
  );
};
const lat = uv => {
  const dv = Math.abs(trenchOff(uv)),
    g = Math.abs(gapOff(uv));
  const dGap = Math.max(GAP_HW - g, g - (GAP_HW + WALL));
  const dEnd = Math.abs(dv - (END_OUT - 0.5 * END_WALL)) - 0.5 * END_WALL;
  return Math.max(0, Math.min(dGap, dEnd));
};
const classify = uv => {
  const h = height(uv);
  return h < 0.05 ? 'T' : h > 0.7 ? 'F' : 'W';
};
// analytic relief-normal gradient magnitude (mirrors grateTrench.normal.glsl), serration proxy
const normGrad = uv => {
  const vOff = trenchOff(uv),
    dv = Math.abs(vOff);
  if (dv >= END_OUT) return 0;
  const gOff = gapOff(uv),
    g = Math.abs(gOff);
  const tg = clamp((g - GAP_HW) / WALL, 0, 1),
    gap = 1 - tg * tg * (3 - 2 * tg),
    dGap = ((-6 * tg * (1 - tg)) / WALL) * sign(gOff);
  const te = clamp((dv - END_OUT + END_WALL) / END_WALL, 0, 1),
    endMask = 1 - te * te * (3 - 2 * te),
    dEnd = ((-6 * te * (1 - te)) / END_WALL) * sign(vOff);
  return DEPTH * Math.hypot(CARVE * endMask * dGap, CARVE * gap * dEnd);
};

// --- Phase 4 analytic (Tier-A) pilot ---------------------------------------
// smoothstep -> linearstep walls (the canonical conversion): cubic*cubic sextic
// becomes linear*linear = quadratic along a ray, so the first crossing of
// s*NdotV = DEPTH*carve(line(s)) has a closed form. Fast-path only (single slat
// cell + single trench); else -1 -> safeStep fallback.
const gapLin = g => clamp((GAP_HW + WALL - g) / WALL, 0, 1);
const endLin = dv => clamp((END_OUT - dv) / END_WALL, 0, 1);
const heightLin = uv => {
  const dv = Math.abs(trenchOff(uv));
  if (dv >= END_OUT) return 0;
  return CARVE * gapLin(Math.abs(gapOff(uv))) * endLin(dv);
};
const classifyLin = uv => {
  const h = heightLin(uv);
  return h < 0.05 ? 'T' : h > 0.7 ? 'F' : 'W';
};

const cellAt = (c, p) => Math.floor(c / p + 0.5);
// Smallest root of A s^2 + B s + C = 0 in [a,b] (>=0), else -1.
const solveSeg = (A, B, C, a, b) => {
  const ok = s => s >= Math.max(a, 0) - 1e-7 && s <= b + 1e-7;
  if (Math.abs(A) < 1e-12) {
    if (Math.abs(B) < 1e-15) return -1;
    const s = -C / B;
    return ok(s) ? Math.max(s, 0) : -1;
  }
  const disc = B * B - 4 * A * C;
  if (disc < 0) return -1;
  const sq = Math.sqrt(disc);
  const lo = (-B - sq) / (2 * A),
    hi = (-B + sq) / (2 * A);
  const r1 = Math.min(lo, hi),
    r2 = Math.max(lo, hi);
  if (ok(r1)) return Math.max(r1, 0);
  if (ok(r2)) return Math.max(r2, 0);
  return -1;
};
const analyticHit = (uv0, duv, NdotV, marchLen) => {
  const u0 = ALONG_X ? uv0[0] : uv0[1],
    du = ALONG_X ? duv[0] : duv[1]; // along-axis (gaps)
  const v0 = ALONG_X ? uv0[1] : uv0[0],
    dvel = ALONG_X ? duv[1] : duv[0]; // across-axis (trenches)
  if (cellAt(u0, SLAT_PITCH) !== cellAt(u0 + du * marchLen, SLAT_PITCH)) return -1;
  if (cellAt(v0, PITCH) !== cellAt(v0 + dvel * marchLen, PITCH)) return -1;
  if (heightLin(uv0) <= 0) return 0; // entry on flat top

  const gOff0 = gapOff(uv0),
    vOff0 = trenchOff(uv0);
  const bps = [0, marchLen];
  const push = s => {
    if (s > 1e-9 && s < marchLen) bps.push(s);
  };
  if (Math.abs(du) > 1e-12) {
    for (const tgt of [0, GAP_HW, -GAP_HW, GAP_HW + WALL, -(GAP_HW + WALL)]) push((tgt - gOff0) / du);
  }
  if (Math.abs(dvel) > 1e-12) {
    for (const tgt of [0, END_OUT - END_WALL, -(END_OUT - END_WALL), END_OUT, -END_OUT])
      push((tgt - vOff0) / dvel);
  }
  bps.sort((x, y) => x - y);

  const k = DEPTH * CARVE;
  for (let i = 0; i + 1 < bps.length; i++) {
    const a = bps[i],
      b = bps[i + 1];
    if (b - a < 1e-9) continue;
    const m = 0.5 * (a + b);
    const gm = Math.abs(gOff0 + du * m);
    let ag = 1,
      bg = 0;
    if (gm >= GAP_HW + WALL) ag = 0;
    else if (gm > GAP_HW) {
      const sg = Math.sign(gOff0 + du * m);
      ag = (GAP_HW + WALL - sg * gOff0) / WALL;
      bg = (-sg * du) / WALL;
    }
    const dm = Math.abs(vOff0 + dvel * m);
    let ae = 1,
      be = 0;
    if (dm >= END_OUT) ae = 0;
    else if (dm > END_OUT - END_WALL) {
      const sv = Math.sign(vOff0 + dvel * m);
      ae = (END_OUT - sv * vOff0) / END_WALL;
      be = (-sv * dvel) / END_WALL;
    }
    const root = solveSeg(k * bg * be, k * (ag * be + bg * ae) - NdotV, k * ag * ae, a, b);
    if (root >= 0) return root;
  }
  return marchLen;
};

const F = (refine, minFeature = 0.1) => ({
  height,
  lat,
  depth: DEPTH,
  steps: 10,
  minFeature,
  refine,
  refineTol: 0.01 * minFeature,
});
// Analytic config: linearized field as ground truth, closed-form hit, safeStep fallback.
const FA = (minFeature = 0.1) => ({
  height: heightLin,
  lat,
  depth: DEPTH,
  steps: 10,
  minFeature,
  refine: 8,
  refineTol: 0.01 * minFeature,
  analyticHit,
});
const dir = (nx, ny) => {
  const L = Math.hypot(nx, ny);
  return [nx / L, ny / L];
};
const ROW = t => [0, t * 18]; // scan across trenches at a gap column

const ANGLES = [
  ['head-on', 0.7, dir(0, 1)],
  ['grazing', 0.06, dir(0, 1)],
  ['graze-diag', 0.06, dir(0.45, 0.9)],
  ['steep-graze', 0.03, dir(0, 1)],
];
console.log('# grate_trench — classMiss (hit lands on wrong surface = serration) + normGrad jaggedness\n');
for (const refine of [3, 8]) {
  console.log(`-- refinementSteps=${refine} ${refine === 3 ? '(pre-fix)' : '(SHIP)'} --`);
  for (const [name, nv, d] of ANGLES) {
    const r = sweep(nv, d, ROW, 700, F(refine), { classify, normGrad });
    console.log(
      `  ${name.padEnd(11)} miss safe=${r.classMiss.safe.padStart(6)} (fixed=${r.classMiss.fixed})  jagNorm safe=${r.jagNorm.safe} (truth=${r.jagNorm.truth} fixed=${r.jagNorm.fixed})  evals safe=${r.evals.safe}`
    );
  }
}

// --- Phase 4: smoothstep -> linearstep visual delta + analytic ToI validation ---
let maxd = 0,
  argmax = null;
for (let i = 0; i <= 400; i++) {
  for (let j = 0; j <= 400; j++) {
    const uv = [(i / 400) * SLAT_PITCH, ((j / 400) * 2 - 1) * END_OUT * 1.05];
    const d = Math.abs(height(uv) - heightLin(uv));
    if (d > maxd) {
      maxd = d;
      argmax = uv;
    }
  }
}
console.log(
  `\n# smoothstep -> linearstep carve delta: max |Δcarve|=${maxd.toFixed(4)} (carve in [0,${CARVE}], = ${((100 * maxd) / CARVE).toFixed(1)}% of full) at uv≈[${argmax[0].toFixed(3)},${argmax[1].toFixed(3)}]`
);

console.log(
  '\n# analytic (Tier-A) vs dense truth on the LINEARIZED field — hitErr (frac of marchLen), classMiss, fast-path rate\n'
);
for (const [name, nv, d] of ANGLES) {
  const r = sweep(nv, d, ROW, 700, FA(), { classify: classifyLin, normGrad });
  const ml = r.marchLen;
  console.log(
    `  ${name.padEnd(11)} hitErr ana=${(+r.hitErr.analytic / ml).toExponential(1)} (safe=${(+r.hitErr.safe / ml).toFixed(4)})  classMiss ana=${r.classMiss.analytic.padStart(6)} (safe=${r.classMiss.safe})  jagNorm ana=${r.jagNorm.analytic} (truth=${r.jagNorm.truth})  fast=${r.fastRate}`
  );
}
