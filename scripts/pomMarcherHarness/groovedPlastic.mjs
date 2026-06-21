// grooved_plastic (grid + safeStep). Mirrors src/levels/boost_nova/groovedPlastic.{common,height}.glsl.
// Same grazing serration + refinement fix (refine 3 -> 8) as grate_trench, on the grid tier.
import { fract, smoothstep, sweep } from './marchers.mjs';

const CELL = 4,
  SQ = 1.7,
  PITCH = 0.34,
  HW = 0.042,
  WALL = 0.018,
  END_OUT = 1.7,
  END_WALL = 0.06,
  CARVE = 0.8;
const SEAM_W = 0.12,
  HAIR_HW = 0.012,
  HAIR_WALL = 0.025,
  SEAM_D = 0.16,
  HAIR_D = 0.14;
const DEPTH = 0.07;

const slotOff = l => (fract((l + SQ) / PITCH) - 0.5) * PITCH;
const alongX = (cx, cy) => {
  const s = cx + cy;
  return s - 2 * Math.floor(s / 2) < 0.5;
};
const seamCarve = b =>
  SEAM_D * (1 - smoothstep(0, SEAM_W, b)) + HAIR_D * (1 - smoothstep(HAIR_HW, HAIR_HW + HAIR_WALL, b));
const height = uv => {
  const ux = uv[0],
    uy = uv[1];
  const cx = Math.floor(ux / CELL),
    cy = Math.floor(uy / CELL);
  const clx = (fract(ux / CELL) - 0.5) * CELL,
    cly = (fract(uy / CELL) - 0.5) * CELL;
  const d = Math.max(Math.abs(clx), Math.abs(cly));
  if (d < SQ) {
    const aX = alongX(cx, cy),
      g = Math.abs(slotOff(aX ? cly : clx)),
      a = Math.abs(aX ? clx : cly);
    return CARVE * (1 - smoothstep(HW, HW + WALL, g)) * (1 - smoothstep(END_OUT - END_WALL, END_OUT, a));
  }
  const b = 0.5 * CELL - d;
  return b < SEAM_W ? seamCarve(b) : 0;
};
const classify = uv => {
  const h = height(uv);
  return h < 0.05 ? 'T' : h > 0.7 ? 'F' : 'W';
};

// grooved ships uniform (no gridLateralDist), so F.lat is omitted -> minStride striding.
const F = (refine, minFeature = 0.06) => ({
  height,
  depth: DEPTH,
  steps: 8,
  minFeature,
  refine,
  refineTol: 0.01 * minFeature,
});
const dir = (nx, ny) => {
  const L = Math.hypot(nx, ny);
  return [nx / L, ny / L];
};
const ROW = t => [t * 8, t * 3.2]; // diagonal scan across cells/slots

const ANGLES = [
  ['head-on', 0.7],
  ['grazing', 0.11],
  ['steep-graze', 0.069],
  ['v-steep', 0.045],
];
console.log('# grooved_plastic — classMiss (hit lands on wrong surface = serration)\n');
for (const refine of [3, 8]) {
  console.log(`-- refinementSteps=${refine} ${refine === 3 ? '(pre-fix)' : '(SHIP)'} --`);
  for (const [name, nv] of ANGLES) {
    const r = sweep(nv, dir(1, 0.4), ROW, 700, F(refine), { classify });
    console.log(
      `  ${name.padEnd(12)} miss safe=${r.classMiss.safe.padStart(6)} (fixed=${r.classMiss.fixed})  evals safe=${r.evals.safe}`
    );
  }
}
