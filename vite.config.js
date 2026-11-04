import adapter from '@sveltejs/adapter-node';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { execSync } from 'child_process';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import devtoolsJson from 'vite-plugin-devtools-json';
import crossOriginIsolation from 'vite-plugin-cross-origin-isolation';
import { behaviorsPlugin } from './src/viz/sceneRuntime/viteBehaviorsPlugin.ts';
import { generatorsPlugin } from './src/viz/levelDef/viteGeneratorsPlugin.ts';
import { generatedScenesPlugin } from './src/viz/scenes/viteGeneratedScenesPlugin.ts';

// Docker builds have no .git; the Justfile passes GIT_HEAD as a build arg instead.
const gitHead = () => {
  try {
    return execSync('git describe --always --dirty', { stdio: ['ignore', 'pipe', 'ignore'] })
      .toString()
      .trim();
  } catch {
    return 'unknown';
  }
};

const config = defineConfig({
  define: { __GIT_HEAD__: JSON.stringify(process.env.GIT_HEAD || gitHead()) },
  plugins: [
    // Must run before `sveltekit()` so its `config()` hook writes the
    // `src/routes/(generated)/` tree before SvelteKit walks the routes dir.
    generatedScenesPlugin(),

    sveltekit({
      preprocess: vitePreprocess({ script: true }),
      alias: { src: fileURLToPath(new URL('./src', import.meta.url)) },
      inlineStyleThreshold: 2048 * 2,
      prerender: { concurrency: 6 },
      adapter: adapter({
        out: 'build',
        // Emit brotli-q11 + gzip-9 sidecars; nginx serves them via `brotli_static`/`gzip_static`
        // instead of its low-quality dynamic brotli (~30% larger on the big wasm/JS blobs).
        precompress: true,
      }),
    }),
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
    allowedHosts: ['3d.p.ameo.design'],
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
