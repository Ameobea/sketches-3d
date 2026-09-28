import GeoscriptWorker from 'src/geoscript/geoscriptWorker.worker?worker';
import { getGeoscriptWorkerWasmURLs } from 'src/viz/wasmComp/wasmAssetURLs';

/** SSR'd by scene pages; its content is the comma-joined eager async dep names. */
export const GEOSCRIPT_EAGER_DEPS_META = 'geoscript-eager-deps';

let prespawned: Worker | null = null;

/**
 * Spawns the scene's geoscript worker from the client `init` hook, well before the scene page
 * component mounts, so the worker's wasm fetch + compile overlap the app bundle's evaluation.
 * The worker self-inits from its `name` (see the worker's startup code).
 */
export const prespawnGeoscriptWorker = () => {
  const content = document.querySelector<HTMLMetaElement>(
    `meta[name="${GEOSCRIPT_EAGER_DEPS_META}"]`
  )?.content;
  if (content === undefined) {
    return;
  }
  const eagerDeps = Object.fromEntries(
    content
      .split(',')
      .filter(Boolean)
      .map(d => [d, true])
  );
  prespawned = new GeoscriptWorker({
    name: JSON.stringify({ urls: getGeoscriptWorkerWasmURLs(), eagerDeps }),
  });
};

export const takePrespawnedGeoscriptWorker = (): Worker | null => {
  const worker = prespawned;
  prespawned = null;
  return worker;
};
