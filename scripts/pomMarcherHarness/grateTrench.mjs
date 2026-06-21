// grate_trench (projectedField + safeStep). Mirrors src/levels/boost_nova/grateTrench.{common,height,normal}.glsl.
// Demonstrates the grazing serration on slat/rail edges and the refinement fix (refine 3 -> 8).
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

const F = (refine, minFeature = 0.1) => ({
  height,
  lat,
  depth: DEPTH,
  steps: 10,
  minFeature,
  refine,
  refineTol: 0.01 * minFeature,
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
