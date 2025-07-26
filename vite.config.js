import { resolve } from 'path';
import wasm from 'vite-plugin-wasm';

import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import devtoolsJson from 'vite-plugin-devtools-json';
import crossOriginIsolation from 'vite-plugin-cross-origin-isolation';

const config = defineConfig({
  plugins: [wasm(), sveltekit(), devtoolsJson(), crossOriginIsolation()],
  resolve: {
    alias: {
      src: resolve('./src'),
    },
  },
  server: {
    port: 4800,
    proxy: {
      '/api': {
        target: 'https://3d.ameo.design/api',
        changeOrigin: true,
        rewrite: path => path.replace(/^\/api/, ''),
      },
    },
  },
  optimizeDeps: {
    exclude: ['codemirror'],
  },
  build: {
    sourcemap: true,
    target: 'esnext',
  },
  worker: {
    format: 'es',
  },
});

export default config;
