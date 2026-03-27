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
      precompress: false,
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
