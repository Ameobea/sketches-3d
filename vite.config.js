import { resolve } from 'path';

import { sveltekit } from '@sveltejs/kit/vite';

/** @type {import('vite').UserConfig} */
const config = {
  plugins: [sveltekit()],
  resolve: {
    alias: {
      src: resolve('./src'),
    },
  },
  server: {
    port: 4800,
  },
  build: {
    sourcemap: true,
  },
};

export default config;
