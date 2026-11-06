# Visual regression harness

Renders curated scene poses and geotoy comps via the app's `?capture=1` hooks (`src/viz/capture.ts`) in headless
`google-chrome-unstable` on the real GPU (ANGLE-GL, throwaway profile), then diffs two capture dirs with FLIP + SSIM.

## Setup (once)

```sh
python3 -m venv ~/three-upgrade-baselines/venv
~/three-upgrade-baselines/venv/bin/pip install -r scripts/visual-regression/requirements.txt
```

`puppeteer-core` is a root devDependency (`yarn install`); it drives the system Chrome, no bundled browser.

## Servers

```sh
yarn build && PORT=5897 node build/index.js     # prod build; or: yarn preview --port 5897 --strictPort
cd geoscript_backend && RUSTFLAGS="--cfg tokio_unstable --cfg foundations_unstable" cargo run -- -c config.yml  # :5810, needed for geotoy comps + sunrise/boost_nova
```

## Capture

```sh
node scripts/visual-regression/capture.mjs --out ~/three-upgrade-baselines/r165/A [--quality low|med|high] [--only stone,geotoy_62] [--no-geotoy] [--all-locations]
```

Writes `<scene>__<pose>.png` / `geotoy_<id>.png`, a `.json` sidecar per image (camera pose, `renderer.info` counts,
`readyError`, console warnings/errors, failed requests) and `report.json` (GPU string, failures, identical-image groups).
Exits nonzero if any capture failed outright. Poses live in `scenes.json`: a name from the scene's `locations`, or
`{name, pos, rot}` (camera position + YXZ euler), `feet: true` to treat `pos` as a player feet position, or
`{name, pos, target}` for orbit scenes. Per-scene `frames` (default 8) / `time` (default 10s) override the freeze.

## Compare

```sh
P=~/three-upgrade-baselines/venv/bin/python
$P scripts/visual-regression/compare.py base/A base/B --out base/floor          # noise floor
$P scripts/visual-regression/compare.py base/A stage-N --floor base/floor/summary.json
```

Flags an image when FLIP mean > 0.003, FLIP worst 32×32 tile > 0.02 or SSIM worst tile < 0.98 (each loosened to 3× the
`--floor` value for that image), when it's missing, or when its sidecar differs (camera, draw calls / triangles /
programs / geometries / textures, readyError, console messages with digits normalized). Writes `summary.json` and
A | B | FLIP-heatmap triptychs for flagged images (`--all-heatmaps` for all). Exits 1 if anything is flagged.

## App hooks (`?capture=1`, implies `render=true`)

- `window.vizReady`: level assets, textures (`DefaultLoadingManager` idle), physics startup barriers,
  `compileAsync`, `loadingComplete`. Rejects with the scene-init error.
- `vizCapture.pose(name | pose)`: places the camera directly, detaches camera control, removes the player mesh.
- `vizCapture.freeze({time, frames})`: pins render + physics clocks to `time`, renders `frames` fixed-dt frames, returns
  `{camera, info}` for the last one.
- Seeded `Math.random`, quality from `quality=`, FPS stats / audio / xray off, web-synth skipped, no dev
  `asset-metadata` writes.
