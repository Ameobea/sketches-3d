# POM marcher CPU harness

A browser-free way to validate a procedural POM material's height field against the
exact marchers in `src/viz/shaders/pom.ts`. GLSL only compiles in-browser, so a bad
marcher/material interaction normally only shows up as a visual artifact you have to
hunt for in-scene. This ports the height field + both marchers to Node and scores them
against a **dense ground-truth first-hit** (8000 uniform samples + 40-step bisect), so
correctness is a number, not a screenshot.

It has already caught two real bugs the GPU bench missed:
- raised_tiles **step-budget exhaustion** at grazing (→ the deadline-floor fix).
- grate_trench / grooved_plastic **grazing serration** on feature edges (→ refinementSteps 3→8).

## Run

```sh
node scripts/pomMarcherHarness/grateTrench.mjs
node scripts/pomMarcherHarness/groovedPlastic.mjs
node scripts/pomMarcherHarness/raisedTiles.mjs
```

No deps, no build. Each prints a small table comparing the fixed march, the safeStep
march, and (implicitly) dense truth across a range of view angles (head-on → steep grazing).

## What's shared vs per-material

- **`marchers.mjs`** — the reusable core, kept in sync with `pom.ts`:
  `fixedMarch` (= `pomMarchProjected`, the pre-safeStep baseline), `safeMarch`
  (= `pomMarchProjectedSafe`/`pomMarchGridSafe` — on the CPU both reduce to "evaluate
  height at uv", so one port covers projectedField **and** grid), the refines (incl. the
  adaptive `POM_REFINE_TOL` early-out), the dense `truth`, and a `sweep()` scorer.
- **`<material>.mjs`** — just the field: `height(uv)→[0,1]` carve, optional `lat(uv)`
  (= `gridLateralDist`), and a `classify(uv)` and/or `normGrad(uv)` for the metrics.
  Mirror the material's `*.common.glsl` / `*.height.glsl` constants exactly.

## Interpreting the output

- **classMiss** — fraction of pixels whose hit lands on a different surface *class*
  (Top / Wall / Floor) than dense truth. This is the **serration proxy**: a hit that
  lands on a steep wall instead of the flat top flips the analytic normal → visible
  sawtooth. `safe` should be ≤ `fixed` and ideally near 0; double-digit `safe` at
  grazing = the serration bug.
- **jagNorm** — mean |2nd difference| of the analytic relief-normal gradient along the
  scanline (needs `normGrad`). The truest serration measure: compare **safe to `truth`**,
  not to 0 (a material legitimately has normal variation). `safe` far above `truth` =
  the marcher is inventing high-frequency normal noise truth doesn't have.
- **hitErr/marchLen** — mean |hit_s − truth_s| as a fraction of the march length. The
  position-accuracy metric (use it when a material's height is continuous so class
  buckets don't apply, e.g. raised_tiles). Lower is better; `safe` ≫ `fixed` flags a
  capping/budget bug.
- **evals** — mean height-field evaluations per pixel (march + refine). This is the cost
  knob; it's what the GPU bench's `evalProxy` measures. Watch that a quality fix doesn't
  blow up evals at head-on (the common case) — the adaptive refine keeps head-on cheap.

Good result: `safe` classMiss/jagNorm at-or-below truth across all angles, with evals
that only rise at grazing.

## Adding a material

Copy `groovedPlastic.mjs`, replace the `height`/`lat`/`classify` bodies with ports of
the material's GLSL (match `clamp(height,0,0.8)*depth` semantics — `marchers.mjs` applies
that wrapper), set the `F` config (`depth`, `steps`, `minFeature`, `refine`,
`refineTol = 0.01*minFeature`), and run. If the material declares no `lateralDist`, omit
`F.lat` (uniform `minFeature` striding).

## Companions

- **GPU bench** `src/viz/scenes/pomBench/pomBench.ts` (`window.pomBench.run(n)`) — real
  GPU-timer ms + `evalProxy`; the cross-material cost truth. Baselines in
  `pom-bench-baseline.md`.
- **Visual diff** `window.pomBench.shot(name, preset, 'march'|'safe', refine?)` drives a
  headless before/after capture (see the puppeteer driver used during the safeStep work).
