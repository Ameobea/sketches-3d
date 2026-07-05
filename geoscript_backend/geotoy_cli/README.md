# geotoy CLI — headless Geoscript rendering for agents

This is the LLM/agent companion to Geotoy. It bundles a Geoscript composition
(code + optional metadata files) into a JSON payload, posts it to the
geoscript_backend `/render/transient` endpoint, and writes back either a PNG (or
AVIF / JPEG) of the rendered scene (`render`) or a JSON envelope of the program's
outputs (`eval`). Nothing touches the database — each run is ephemeral.

Two commands:

- **`geotoy render <path>`** → a rendered image. Look at the scene.
- **`geotoy eval <path>`** → JSON of the composition's outputs: computed/exported
  values, `render()`ed meshes, `render_path()`ed paths, `print()` output, and an
  optional `--expr`. Assert on real numbers/points instead of eyeballing a PNG.

It exists so an agent can do this loop:

1. write/edit Geoscript files locally
2. `geotoy render my_composition/` → look at the resulting PNG, or
   `geotoy eval my_composition/ --expr '…'` → read the actual values
3. iterate

## Install

There's nothing to install per se — it ships as a small Node script that uses
`--experimental-strip-types`. From inside the repo:

```sh
cd geoscript_backend/geotoy_cli
yarn install   # only needed once, just pulls @types/node
```

The `bin/geotoy` shim is executable and can be symlinked anywhere on `$PATH`:

```sh
ln -s "$(pwd)/bin/geotoy" ~/.local/bin/geotoy
```

The script requires Node ≥ 22 (for native `--experimental-strip-types`).

## Quick start

The simplest possible composition is just a `.geo` file:

```sh
echo 'box(8) | render' > scene.geo
GEOTOY_CLI_TOKEN=... geotoy render scene.geo
# writes scene.png
```

A directory with extras:

```
my_scene/
  main.geo          # required — _root source
  view.json         # camera + target
```

```sh
geotoy render my_scene/ -o preview.png
```

## Auth

Every request needs a CLI token, distinct from the admin token. Get the
current value from `cli_token` in the backend's `geoscript-backend.yml`.

Either pass `--token <value>` or set the env var:

```sh
export GEOTOY_CLI_TOKEN='...'
```

## Directory layout

Everything but `main.geo` is optional. Defaults are filled in by the
frontend at render time (camera, materials, environment, prelude) and match
what a fresh Geotoy session shows.

```
my_scene/
  main.geo             # _root node source — the entry point
  globals.geo          # optional — shared definitions (the "Globals" tab)
  view.json            # optional — camera position/target/fov/zoom
  materials.json       # optional — full MaterialDefinitions
  environment.json     # optional — scene-wide IBL config
  nodes/<name>.geo     # optional — extra child nodes hanging off _root
  tree.json            # optional — full TreeDef, overrides everything above
  .prelude_ejected     # optional — marker file; if present, prelude is skipped
```

Single-file shortcut: pass a path to a `.geo` file directly and it's treated as
`_root`'s source. Everything else defaults.

### `view.json`

```json
{
  "cameraPosition": [12, 12, 12],
  "target": [0, 0, 0],
  "fov": 60,
  "zoom": 1
}
```

`fov` is for perspective cameras (default); `zoom` is for orthographic. Omit
either as needed.

**If `view.json` is missing**, the camera auto-frames the rendered geometry —
the same operation the `.` keybind performs in the editor. The framing
preserves the default view direction (looking at the origin from a
front-top-right diagonal) and fits the bounding sphere of all rendered
meshes with a small margin. Provide `view.json` if you want a specific
angle, distance, or framing.

### `materials.json`

Mirrors the in-app `MaterialDefinitions` shape. Materials must use the modern
`customShader` shape — the legacy `physical`/`basic` shape (with `{r,g,b}`
color objects) is **not** accepted at render time and fails silently: the mesh
falls back to a flat unlit gray. If a render comes back flat gray with no
shading, suspect this file first (the prelude's lights are present either way).

```json
{
  "defaultMaterialID": "uvdbg",
  "materials": {
    "uvdbg": {
      "type": "customShader",
      "name": "uvdbg",
      "props": {
        "color": 16777215,
        "roughness": 0.9,
        "metalness": 0,
        "map": "30",
        "uvScale": [1, 1]
      },
      "shaders": {},
      "options": {
        "useTriplanarMapping": false,
        "useGeneratedUVs": false
      }
    }
  }
}
```

This example is the standard recipe for inspecting mesh UVs: texture `"30"` is
the `uv_debug` checker/label texture, and `useTriplanarMapping: false` makes it
sample the mesh's `uv` attribute (the default material is triplanar and ignores
UVs entirely). `props.color` is an sRGB hex int, `props.map` and friends are
texture IDs as strings, `uvScale` is an array.

The `name` is what Geoscript references via `set_default_material` /
`set_material`. Texture and shader fields work the same as in the in-app
material editor (textures still come from the geoscript_backend by ID).

If `materials.json` is missing, the renderer uses the same defaults as a fresh
Geotoy session.

### `environment.json`

```json
{ "kind": "gradient", "skyColor": 16777215, "horizonColor": 12303291, "groundColor": 4473924, "intensity": 0.8, "setBackground": true }
```

or:

```json
{ "kind": "equirect", "textureId": 42, "intensity": 1.0, "setBackground": true }
```

Omit the file for no scene environment.

### `nodes/`

Each `<name>.geo` in `nodes/` becomes a child of `_root` with that name and an
identity transform. Useful for splitting big scenes the way the Geotoy
hierarchy panel does.

For nested trees or non-identity transforms, write a full `tree.json` instead.

### `tree.json`

Full override — same shape the backend stores. Useful when you've exported a
composition from the editor and want a faithful local copy. Documented by the
`TreeDef` interface in `src/geoscript/geotoyAPIClient.ts`.

```json
{
  "version": 1,
  "rootId": "...",
  "globalsSource": "",
  "nodes": { "...": { "id": "...", "name": "_root", "source": "...", "instances": [{ "pos": [0,0,0], "rot": [0,0,0], "scale": [1,1,1], "id": "00000000" }], "children": [] } }
}
```

Each node carries `instances` — a list of placements (each a transform plus a
short `id`); `instances.length === 1` is the common single-copy case.

## CLI reference

```
geotoy render <path> [options]

  <path>     A directory or a single .geo file.

Options:
  -o, --out <file>     Output image path (default: <basename>.<ext>)
  --dev                Use localhost services instead of prod
  --backend <url>      Override the backend URL
  --token <token>      CLI token; falls back to $GEOTOY_CLI_TOKEN
  --width <n>          Render width in px (default 800)
  --height <n>         Render height in px (default 800)
  --format <fmt>       png (default) | avif | jpeg
  --quality <n>        Quality 0-100 for avif/jpeg
  --material <mode>    Debug material for all meshes: normal | wireframe | wireframe-xray
  --no-prelude         Skip the standard geoscript prelude (default: included)
  --stdout             Write image to stdout (suppresses progress)
```

The standard prelude (default lights, camera, helpers) is included on every
render unless you pass `--no-prelude` or drop a `.prelude_ejected` marker in the
composition directory. Without it, a bare `box(8) | render` has no lights and
renders black. There is no need to add `ambient_light`/`dir_light` to
`main.geo` yourself — if a scene looks unlit (flat gray) despite the prelude,
the cause is almost certainly an invalid `materials.json` falling back to an
unlit material (see the `materials.json` section), not missing lights.

`--material` swaps every rendered mesh to a debug material right before capture —
the headless equivalent of the app's `n` (normal material), `w` (wireframe), and
`shift+w` (wireframe x-ray) keybinds. Great for inspecting shading normals (e.g.
whether a bevel is smooth) without lighting/material noise. `materials.json` is
ignored for the overridden meshes while this is set.

## `geotoy eval` — extract program outputs as JSON

`geotoy eval <path>` runs a composition exactly like `render` (same prelude,
globals, tree, async deps, error handling), but instead of a PNG it returns a
JSON envelope of the program's *outputs*. Use it to assert on real values,
inspect rendered geometry, or find out *why* a scene came back empty.

```sh
geotoy eval my_scene/                       # → JSON on stdout
geotoy eval my_scene/ --expr 'my_radius'    # + the value of an expression
geotoy eval my_scene/ --meshes glb -o out.json   # + full geometry as out.glb
```

### Envelope

```jsonc
{
  "ok": true,
  "error": null,
  "stats":   { "meshes": 1, "paths": 1, "lights": 2, "vertices": 88, "faces": 75, "runtimeMs": 3.1 },
  "exports": { "<name>": <value>, ... },       // the composition's own top-level bindings
  "expr":    <value>,                          // only with --expr
  "prints":  ["...", ...],                      // print() output, in call order
  "meshes":  [{ "id", "sourceModule", "material", "vertices", "faces", "bbox": { "min", "max" } }],
  "paths":   [{ "id", "sourceModule", "points": [[x,y,z], ...] }],
  "lights":  [{ "type", "color", "intensity", "position" }],
  "meshData": { ... }                          // only with --meshes glb|gltf|obj|json
}
```

### Tagged values

Every value (`exports`, `expr`, sequence items, map entries) is tagged:
`{ "t": <type>, "v": <payload> }`.

| `t`        | shape                                                                  |
| ---------- | ---------------------------------------------------------------------- |
| `nil`      | `{}`                                                                    |
| `int`      | `v`: number                                                            |
| `float`    | `v`: number — or the string `"NaN"` / `"Infinity"` / `"-Infinity"`     |
| `bool`     | `v`: bool                                                              |
| `string`   | `v`: string                                                            |
| `vec2`     | `v`: `[x, y]`                                                          |
| `vec3`     | `v`: `[x, y, z]`                                                       |
| `mat4`     | `v`: 16 numbers (column-major)                                         |
| `seq`      | `v`: items, `len`: number, optional `truncated: true` (capped at 4096) |
| `map`      | `v`: object of tagged values                                          |
| `mesh`     | `vertices`, `faces` (counts; full geometry is in `meshes`/`meshData`) |
| `material` | `name`                                                                |
| `light`    | `v`: the light's JSON                                                 |
| `callable` | with `--samples N`: `samples: [{ t_in, out | error }]` over t∈[0,1]   |

Non-finite floats are emitted as strings on purpose, so a NaN leaking into a
value is visible rather than crashing the JSON.

### What gets captured

- **`exports`** — every top-level `name = …` binding in `_root`. Prelude/globals
  names are excluded, so this is just the composition's own definitions.
- **`meshes`** — one entry per `render()`ed mesh (counts + world-space bbox).
  Full geometry with `--meshes` (below).
- **`paths`** — one entry per `render_path()`ed path, as a world-space polyline.
- **`lights`** — one entry per rendered light.
- **`prints`** — everything `print(...)` emitted, in order.
- **`expr`** — `--expr '<geoscript>'` is appended to `_root` as a trailing
  expression, so it can reference the composition's definitions and is evaluated
  as part of the run. A malformed expr fails the whole run (see errors below).

### Mesh geometry — `--meshes <fmt>`

| `--meshes`          | Effect                                                            |
| ------------------- | ---------------------------------------------------------------- |
| `summary` (default) | counts + bbox only, in `meshes[]`                                |
| `glb`               | binary glTF — recommended for loading into other apps            |
| `gltf`              | text glTF                                                        |
| `obj`               | Wavefront OBJ                                                    |
| `json`              | raw `positions`/`normals`/`uvs`/`indices` arrays + `matrixWorld` |

For `glb`/`gltf`/`obj` the geometry goes to a **sidecar file** next to `--out`
(or to `--meshes-out <file>`), and `meshData` points at it:
`{ "format": "glb", "path": "out.glb", "bytes": 2392 }`. With no `--out` and no
`--meshes-out` (pure stdout), the bytes are base64-embedded in `meshData`
instead. This reuses the editor's "Export Scene" serializer: geometry is baked
to world space (the same `render()`-composed transforms the export button uses),
lights are included, paths are not (they're already in `paths[]`). It is the
canonical `render()` output — material-driven UV unwrap is a render-time
transform and is not applied here.

### eval options

```
  -o, --out <file>     Write the JSON envelope to a file (default: stdout)
  --expr <geoscript>   Evaluate an expression against the composition's root scope
  --samples <n>        Sample callable values at N points over t in [0,1] (default 0)
  --meshes <fmt>       summary (default) | glb | gltf | obj | json
  --meshes-out <file>  Where to write full mesh geometry (default: beside --out)
```

Plus the common `--dev` / `--backend` / `--token` / `--no-prelude` / `--timeout`
(eval defaults to a 30s timeout). All the composition-directory inputs
(`main.geo`, `globals.geo`, `nodes/`, `tree.json`, …) work identically to
`render`.

## Errors & Wasm panics — no more silent blank output

A geoscript error (bad argument, a NaN reaching a path sampler, …) or a Wasm
panic used to produce a valid-but-blank PNG (or an empty eval) with no
diagnostic. Both commands now **fail loudly**: the message comes back as a
non-zero exit with the error on stderr, e.g.

```
Server returned 500 Internal Server Error
at line 8, column 1: No valid function signature found for `render_path` ...
```

Wasm panics are reported with their real message and location (not the opaque
`RuntimeError: unreachable` the trap otherwise surfaces as), and are still
logged to the browser console and captured by Sentry.

**GLSL shader compile errors fail loudly too.** A `customShader` material with a
snippet that doesn't compile used to yield a clean-looking PNG with the mesh
silently missing (three.js only logs shader errors; nothing throws). `render`
now fails with three's full report — material name, the GLSL error, and a
numbered source excerpt around the offending line:

```
Server returned 500 Internal Server Error
THREE.WebGLProgram: Shader Error 0 - VALIDATE_STATUS false
Material Name: pool_tiles
...
ERROR: 0:1931: 'this' : Illegal use of reserved word
> 1931: this is not valid glsl;
```

So: a mesh that's missing from an otherwise-fine render is *not* a shader error
(those abort the render) — suspect the legacy-shape `materials.json` fallback
(see the `materials.json` section) or scene/camera issues instead.

### Defaults & overrides

| Field             | Default                                                |
| ----------------- | ------------------------------------------------------ |
| Backend (prod)    | `https://3d.ameo.design/geotoy_api/render/transient`   |
| Backend (`--dev`) | `http://localhost:5810/render/transient`               |
| Frontend (prod)   | `https://3d.ameo.design/geotoy/render`                 |
| Frontend (`--dev`)| `http://localhost:4800/geotoy/render`                  |
| Format            | `png`                                                  |
| Size              | 800×800                                                |

The `--dev` flag affects two things:
1. The CLI hits the local backend instead of prod.
2. The backend tells the renderer service to navigate to the local frontend
   instead of `3d.ameo.design`.

For dev mode to work end-to-end you need three services running locally:

- geoscript_backend (port 5810)
- thumbnail_generator service (port 5812)
- the SvelteKit frontend (port 4800)

In prod, none of that matters — the CLI just talks to the public backend URL.

## How it works (one paragraph)

The CLI bundles the directory into `{tree, metadata, options}` and POSTs to
the backend's `/render/transient` route. The backend authenticates the
`X-CLI-Token` header, forwards the JSON body to the local
`thumbnail_generator` service at `/render_transient`. That service spawns a
headless browser, injects the payload via Puppeteer's `evaluateOnNewDocument`
(landing it on `window.__transientCompositionPayload`), and navigates to a
new `/geotoy/render` SvelteKit route. The frontend reads the payload, mounts
the standard playground in render mode, runs the script, waits for materials
to load, then calls `window.onRenderReady()`. Puppeteer screenshots, sharp
encodes, and the bytes stream back through the chain to the CLI's output file.

## Limitations

- Texture references in `materials.json` still need to live in the
  geoscript_backend texture library (by ID). There's no way to upload new
  textures inline yet.
- Materials and environment defaults are filled in by the frontend at render
  time, not by the CLI itself — passing a partial `materials.json` may
  collide with defaults in unexpected ways.
- A render holds a browser tab open for the full duration of the script. Heavy
  compositions take a while; the upstream timeout is 15 minutes.
- Only one render per browser tab — the service launches a fresh browser per
  request.
