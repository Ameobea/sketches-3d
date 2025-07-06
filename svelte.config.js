import adapter from '@sveltejs/adapter-node';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess({ script: true }),

  kit: {
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
      inspector: {
        holdMode: true,
      },
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
