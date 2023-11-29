import adapter from '@sveltejs/adapter-static';
import preprocess from 'svelte-preprocess';

/** @type {import('@sveltejs/kit').Config} */
const config = {
  // Consult https://github.com/sveltejs/svelte-preprocess
  // for more information about preprocessors
  preprocess: preprocess(),

  kit: {
    inlineStyleThreshold: 2048 * 2,
    prerender: {
      concurrency: 6,
    },
    adapter: adapter({
      // default options are shown
      pages: 'build',
      assets: 'build',
      fallback: null,
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
  compilerOptions: {
    format: 'esm',
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
