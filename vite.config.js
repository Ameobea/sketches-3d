import { resolve } from 'path';
import wasm from 'vite-plugin-wasm';

import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

const config = defineConfig({
  plugins: [wasm(), sveltekit()],
  resolve: {
    alias: {
      src: resolve('./src'),
      '@codemirror/state': resolve(__dirname, './node_modules/@codemirror/state/dist/index.cjs'),
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
  },
});

export default config;
