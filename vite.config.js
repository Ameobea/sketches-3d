import wasm from 'vite-plugin-wasm';

import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import devtoolsJson from 'vite-plugin-devtools-json';
import crossOriginIsolation from 'vite-plugin-cross-origin-isolation';
import { behaviorsPlugin } from './src/viz/sceneRuntime/viteBehaviorsPlugin';
import { generatorsPlugin } from './src/viz/levelDef/viteGeneratorsPlugin';
import { generatedScenesPlugin } from './src/viz/scenes/viteGeneratedScenesPlugin';

const config = defineConfig({
  plugins: [
    // Must run before `sveltekit()` so its `config()` hook writes the
    // `src/routes/(generated)/` tree before SvelteKit walks the routes dir.
    generatedScenesPlugin(),
    wasm(),
    sveltekit(),
    devtoolsJson(),
    crossOriginIsolation(),
    behaviorsPlugin(),
    generatorsPlugin(),
  ],
  server: {
    port: 4800,
    proxy: {
      '/api': {
        target: 'https://3d.ameo.design/api',
        changeOrigin: true,
        rewrite: path => path.replace(/^\/api/, ''),
      },
    },
    watch: {
      ignored: p =>
        /[\\/](?:node_modules|\.git|\.svelte-kit|dist|build)(?:[\\/]|$)/.test(p) ||
        /[\\/](?:backend|geoscript_backend)(?:[\\/]|$)/.test(p) ||
        /[\\/]src[\\/]viz[\\/]wasm(?:[\\/]|$)/.test(p),
    },
  },
  optimizeDeps: {
    exclude: ['codemirror'],
  },
  ssr: {},
  build: {
    sourcemap: true,
    target: 'esnext',
  },
  worker: {
    format: 'es',
  },
});

export default config;
