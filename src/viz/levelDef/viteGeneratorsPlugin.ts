import { type Plugin } from 'vite';

/**
 * Vite plugin that stores the dev server on `globalThis` so that SSR-loaded
 * code (which gets its own module instances) can access `ssrLoadModule`.
 */
export function generatorsPlugin(): Plugin {
  return {
    name: 'level-generators',
    configureServer(server) {
      (globalThis as Record<string, unknown>).__viteDevServer = server;
    },
  };
}
