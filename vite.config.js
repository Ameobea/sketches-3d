import { sveltekit } from '@sveltejs/kit/vite';

/** @type {import('vite').UserConfig} */
const config = {
  plugins: [sveltekit()],
  server: {
    port: 4800,
  },
  build: {
    sourcemap: true,
  },
};

export default config;
