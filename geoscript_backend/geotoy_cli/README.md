# geotoy CLI — headless Geoscript rendering for agents

This is the LLM/agent companion to Geotoy. It bundles a Geoscript composition
(code + optional metadata files) into a JSON payload, posts it to the
geoscript_backend `/render/transient` endpoint, and writes back a PNG (or AVIF
/ JPEG) of the rendered scene. Nothing touches the database — each render is
ephemeral.

It exists so an agent can do this loop:

1. write/edit Geoscript files locally
2. `geotoy render my_composition/` → look at the resulting PNG
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

Mirrors the in-app `MaterialDefinitions` shape:

```json
{
  "defaultMaterialID": "default",
  "materials": {
    "default": {
      "type": "physical",
      "name": "default",
      "color": { "r": 0.8, "g": 0.4, "b": 0.2 },
      "roughness": 0.6,
      "metalness": 0.1,
      "clearcoat": 0,
      "clearcoatRoughness": 0,
      "iridescence": 0,
      "normalScale": 1,
      "uvScale": { "x": 0.13, "y": 0.13 }
    }
  }
}
```

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
  "rootId": "...",
  "globalsSource": "",
  "nodes": { "...": { "id": "...", "name": "_root", "source": "...", "transform": {"pos":[0,0,0],"rot":[0,0,0],"scale":[1,1,1]}, "children": [] } }
}
```

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
  --stdout             Write image to stdout (suppresses progress)
```

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
