import adapter from '@sveltejs/adapter-node';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess({ script: true }),

  kit: {
    alias: {
      src: fileURLToPath(new URL('./src', import.meta.url)),
    },
    inlineStyleThreshold: 2048 * 2,
    prerender: {
      concurrency: 6,
    },
    adapter: adapter({
      out: 'build',
      // Emit brotli-q11 + gzip-9 sidecars; nginx serves them via `brotli_static`/`gzip_static`
      // instead of its low-quality dynamic brotli (~30% larger on the big wasm/JS blobs).
      precompress: true,
    }),
  },
  viteOptions: {
    experimental: {
      prebundleSvelteLibraries: true,
    },
  },
  build: {
    rollupOptions: {
      output: {
        format: 'esm',
      },
    },
  },
};

export default config;
