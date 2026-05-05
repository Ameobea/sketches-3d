import ammoWasmURL from 'src/ammojs/ammo.wasm.wasm?url';
import geodesicsWasmURL from 'src/geodesics/geodesics.wasm?url';
import cgalWasmURL from 'src/viz/wasm/cgal/index.wasm?url';
import clipper2WasmURL from 'src/viz/wasm/clipper2/clipper2z.wasm?url';
import flightRecorderWasmURL from 'src/viz/wasmComp/flight_recorder.wasm?url';
import geoscriptReplWasmURL from 'src/viz/wasmComp/geoscript_repl_bg.wasm?url';
import manifoldWasmURL from 'manifold-3d/manifold.wasm?url';

/**
 * Centralised registry of hashed wasm asset URLs, resolved by Vite from `?url`
 * imports.  Importing this module pulls every listed wasm into the **main**
 * bundle's asset graph; do **not** import it from a `?worker` graph or Vite
 * will emit a second copy under `workers/assets/` and `<link rel=preload>`s
 * for the main URLs will miss the worker's actual fetches.
 *
 * Workers receive the URLs they need through their `init()` message instead.
 */
export const WASM_ASSET_URLS = {
  ammo: ammoWasmURL,
  geodesics: geodesicsWasmURL,
  cgal: cgalWasmURL,
  clipper2: clipper2WasmURL,
  flightRecorder: flightRecorderWasmURL,
  geoscriptRepl: geoscriptReplWasmURL,
  manifold: manifoldWasmURL,
} as const;

/** Subset of URLs the geoscript worker needs in its `init()` call. */
export interface GeoscriptWorkerWasmURLs {
  manifold: string;
  geoscriptRepl: string;
  cgal: string;
  clipper2: string;
  geodesics: string;
}

export const getGeoscriptWorkerWasmURLs = (): GeoscriptWorkerWasmURLs => ({
  manifold: WASM_ASSET_URLS.manifold,
  geoscriptRepl: WASM_ASSET_URLS.geoscriptRepl,
  cgal: WASM_ASSET_URLS.cgal,
  clipper2: WASM_ASSET_URLS.clipper2,
  geodesics: WASM_ASSET_URLS.geodesics,
});
