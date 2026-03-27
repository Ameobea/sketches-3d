import { readFileSync, writeFileSync } from 'fs';
import { join } from 'path';

import { LevelDefSchema, LevelDefRawSchema } from './types';
import type { LevelDef } from './types';

/**
 * Reads a level definition from `src/levels/<name>/def.json`, resolves any
 * `file` references in geoscript assets (inlining the code from disk), validates
 * the result, and returns it.
 *
 * Also fixes the `$schema` field in the file on disk when it's missing or stale,
 * so IDEs get autocomplete and inline validation automatically.
 *
 * Intended for use in SvelteKit `+page.server.ts` load functions so the level
 * definition is baked into the page response rather than fetched separately.
 *
 * In development, reads fresh from disk on every request — supporting the
 * level editor workflow of edit → reload → see changes.
 */
export const loadLevelData = (name: string): LevelDef => {
  const levelDir = join(process.cwd(), 'src', 'levels', name);
  const filePath = join(levelDir, 'def.json');
  const raw = readFileSync(filePath, 'utf-8');
  const json = JSON.parse(raw);

  // Fix $schema if missing or pointing at the wrong path.
  if (json.$schema !== '../schema.json') {
    json.$schema = '../schema.json';
    writeFileSync(filePath, JSON.stringify(json, null, 2) + '\n', 'utf-8');
  }

  // Parse with the raw schema, which accepts both `code` and `file` geoscript assets.
  const rawResult = LevelDefRawSchema.safeParse(json);
  if (!rawResult.success) {
    const msg = rawResult.error.issues.map(i => `  ${i.path.join('.')}: ${i.message}`).join('\n');
    throw new Error(`[loadLevelData] Invalid level def "${name}":\n${msg}`);
  }

  // Inline any `file` geoscript assets: read the file and substitute `code`.
  const resolvedAssets = Object.fromEntries(
    Object.entries(rawResult.data.assets).map(([assetId, assetDef]) => {
      if (assetDef.type === 'geoscript' && 'file' in assetDef) {
        const codePath = join(levelDir, assetDef.file);
        const code = readFileSync(codePath, 'utf-8');
        return [assetId, { type: 'geoscript' as const, code, includePrelude: assetDef.includePrelude }];
      }
      return [assetId, assetDef];
    })
  );
  const inlinedDef = { ...rawResult.data, assets: resolvedAssets };

  // Validate the fully-inlined def (includes cross-reference checks).
  const result = LevelDefSchema.safeParse(inlinedDef);
  if (!result.success) {
    const msg = result.error.issues.map(i => `  ${i.path.join('.')}: ${i.message}`).join('\n');
    throw new Error(`[loadLevelData] Invalid level def "${name}" (after inlining):\n${msg}`);
  }

  return result.data;
};
