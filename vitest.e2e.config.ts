// Needs the geoscript backend (VITE_GEOSCRIPT_API_URL in .env.local) and `yarn dev` serving this
// checkout on :4810 (override with E2E_BASE). Drives /usr/bin/google-chrome-unstable via playwright-core.
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  resolve: {
    alias: {
      src: fileURLToPath(new URL('./src', import.meta.url)),
      '$app/env': fileURLToPath(new URL('./e2e/appEnv.ts', import.meta.url)),
    },
  },
  test: {
    include: ['e2e/**/*.test.ts'],
    fileParallelism: false,
    testTimeout: 300_000,
    hookTimeout: 300_000,
  },
});
