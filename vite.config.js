import wasm from 'vite-plugin-wasm';

import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import devtoolsJson from 'vite-plugin-devtools-json';
import crossOriginIsolation from 'vite-plugin-cross-origin-isolation';
import { behaviorsPlugin } from './src/viz/sceneRuntime/viteBehaviorsPlugin';
import { generatorsPlugin } from './src/viz/levelDef/viteGeneratorsPlugin';

const config = defineConfig({
  plugins: [
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
      ignored: [
        '**/node_modules/**',
        '**/dist/**',
        '**/build/**',
        'src/viz/wasm/**',
        'backend/**',
        '**/.svelte-kit/**',
        '**/.git/**',
        'geoscript_backend/**',
      ],
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
