// raised_tiles (grid + safeStep + bounded). Mirrors src/assets/materials/procedural/raised_tiles/raisedTiles.common.glsl.
// Regression guard for the deadline-floor fix: deep (0.38) + dense (cell 1) + thin (0.02) walls
// once exhausted the 16-step budget at grazing and clamped short. The cover floor (in safeMarch)
// must keep safe's hit error near truth at all angles.
import { fract, clamp, mix, sweep } from './marchers.mjs';

const CELL = 1.0,
  WALL_W = 0.02,
  SPAN = 0.5,
  TOP = 0.0,
  DEPTH = 0.38;
const hash01 = (x, y) => {
  const h = Math.sin(x * 127.1 + y * 311.7) * 43758.5453;
  return h - Math.floor(h);
};
const carve = (cx, cy) => TOP + (1 - hash01(cx + 0.5, cy + 0.5)) * SPAN;
const height = uv => {
  const ux = uv[0],
    uy = uv[1];
  const cx = Math.floor(ux / CELL),
    cy = Math.floor(uy / CELL);
  const clx = (fract(ux / CELL) - 0.5) * CELL,
    cly = (fract(uy / CELL) - 0.5) * CELL;
  const b = 0.5 * CELL - Math.max(Math.abs(clx), Math.abs(cly));
  if (b >= WALL_W) return carve(cx, cy);
  let edx, edy;
  if (Math.abs(clx) >= Math.abs(cly)) {
    edx = Math.sign(clx);
    edy = 0;
  } else {
    edx = 0;
    edy = Math.sign(cly);
  }
  return mix(
    0.5 * (carve(cx, cy) + carve(cx + edx, cy + edy)),
    carve(cx, cy),
    clamp(b / WALL_W, 0, 1) ** 2 * (3 - 2 * clamp(b / WALL_W, 0, 1))
  );
};
const lat = uv => {
  const clx = (fract(uv[0] / CELL) - 0.5) * CELL,
    cly = (fract(uv[1] / CELL) - 0.5) * CELL;
  return Math.max(0, 0.5 * CELL - Math.max(Math.abs(clx), Math.abs(cly)) - WALL_W);
};

const F = (minFeature = 0.06) => ({
  height,
  lat,
  depth: DEPTH,
  steps: 16,
  minFeature,
  refine: 4,
  refineTol: 0.01 * minFeature,
});
const dir = (nx, ny) => {
  const L = Math.hypot(nx, ny);
  return [nx / L, ny / L];
};
const ROW = t => [t * 4, t * 1.4];

console.log(
  '# raised_tiles — hit error vs truth (fraction of marchLen); deadline floor must keep safe low at grazing\n'
);
for (const [name, nv] of [
  ['head-on', 0.7],
  ['grazing', 0.12],
  ['steep-graze', 0.075],
  ['v-steep', 0.047],
]) {
  const r = sweep(nv, dir(1, 0.35), ROW, 700, F(), {});
  const ml = r.marchLen;
  console.log(
    `  ${name.padEnd(12)} hitErr/marchLen safe=${(+r.hitErr.safe / ml).toFixed(3)} fixed=${(+r.hitErr.fixed / ml).toFixed(3)}  evals safe=${r.evals.safe} fixed=${r.evals.fixed}`
  );
}
console.log(
  '\n# minFeatureWidth sweep at steep grazing (0.02 = the value that exhausted the budget pre-fix):'
);
for (const mf of [0.02, 0.06, 0.1]) {
  const r = sweep(0.075, dir(1, 0.35), ROW, 700, F(mf), {});
  console.log(
    `  minFeature=${mf}  hitErr/marchLen safe=${(+r.hitErr.safe / r.marchLen).toFixed(3)}  evals safe=${r.evals.safe}`
  );
}
