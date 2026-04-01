import { createRequire } from 'node:module';
import vm from 'node:vm';

import { buildSync } from 'esbuild';
import { existsSync, readdirSync, readFileSync, writeFileSync } from 'fs';
import { join } from 'path';

import { formatLevelJson } from './formatLevelJson';
import type { GeneratorFn } from './generatorTypes';
import { GENERATED_NODE_USERDATA_KEY, isObjectGroup } from './levelDefTreeUtils';
import { LevelDefSchema, LevelDefRawSchema } from './types';
import type { LevelDef } from './types';

/**
 * Compiles and loads a generator TypeScript file using esbuild + vm, bypassing
 * the Node ESM module cache so that dev edits are always reflected.
 */
const loadGeneratorFn = (filePath: string): GeneratorFn => {
  const { outputFiles } = buildSync({
    entryPoints: [filePath],
    bundle: true,
    format: 'cjs',
    platform: 'node',
    write: false,
    external: ['three'],
    define: { 'import.meta.url': `"file://${filePath}"` },
    alias: { src: join(process.cwd(), 'src') },
  });
  const req = createRequire(filePath);
  const mod = { exports: {} as Record<string, unknown> };
  vm.runInNewContext(outputFiles[0].text, {
    module: mod,
    exports: mod.exports,
    require: req,
    console,
    process,
    Buffer,
  });
  return (mod.exports.default ?? mod.exports) as GeneratorFn;
};

const markGeneratedNode = (node: import('./types').ObjectDef | import('./types').ObjectGroupDef) => {
  const nextUserData = { ...(node.userData ?? {}), [GENERATED_NODE_USERDATA_KEY]: true };
  if (isObjectGroup(node)) {
    return {
      ...node,
      userData: nextUserData,
      children: node.children.map(markGeneratedNode),
    };
  }

  return {
    ...node,
    userData: nextUserData,
  };
};

/**
 * Reads a level definition from `src/levels/<name>/def.json`, merges any
 * optional `materials.json` and `objects.json` sidecar files, auto-discovers
 * `.geo` files from the level's `geo/` subdirectory, resolves any `file`
 * references in geoscript assets (inlining the code from disk), validates
 * the result, and returns it.
 *
 * Also fixes the `$schema` field in each file on disk when it's missing or
 * stale, so IDEs get autocomplete and inline validation automatically.
 *
 * Intended for use in SvelteKit `+page.server.ts` load functions so the level
 * definition is baked into the page response rather than fetched separately.
 *
 * In development, reads fresh from disk on every request — supporting the
 * level editor workflow of edit → reload → see changes.
 */
export const loadLevelData = async (name: string): Promise<LevelDef> => {
  const levelDir = join(process.cwd(), 'src', 'levels', name);
  const filePath = join(levelDir, 'def.json');
  const raw = readFileSync(filePath, 'utf-8');
  const json = JSON.parse(raw);

  // Fix $schema if missing or pointing at the wrong path.
  if (json.$schema !== '../schema.json') {
    json.$schema = '../schema.json';
    writeFileSync(filePath, formatLevelJson(json), 'utf-8');
  }

  // Merge materials.json if present (external values win on conflict).
  const materialsPath = join(levelDir, 'materials.json');
  if (existsSync(materialsPath)) {
    const matsJson = JSON.parse(readFileSync(materialsPath, 'utf-8'));
    if (matsJson.$schema !== '../materials-schema.json') {
      matsJson.$schema = '../materials-schema.json';
      writeFileSync(materialsPath, formatLevelJson(matsJson), 'utf-8');
    }
    if (matsJson.textures) json.textures = { ...json.textures, ...matsJson.textures };
    if (matsJson.materials) json.materials = { ...json.materials, ...matsJson.materials };
  }

  // Replace objects from objects.json if present.
  const objectsPath = join(levelDir, 'objects.json');
  if (existsSync(objectsPath)) {
    const objsJson = JSON.parse(readFileSync(objectsPath, 'utf-8'));
    if (objsJson.$schema !== '../objects-schema.json') {
      objsJson.$schema = '../objects-schema.json';
      writeFileSync(objectsPath, formatLevelJson(objsJson), 'utf-8');
    }
    json.objects = objsJson.objects ?? [];
  }

  // Auto-discover *.geo files from the geo/ subdirectory.
  const geoDir = join(levelDir, 'geo');
  if (existsSync(geoDir)) {
    if (!json.assets) json.assets = {};
    for (const file of readdirSync(geoDir).filter((f: string) => f.endsWith('.geo'))) {
      const id = file.slice(0, -4);
      if (!(id in json.assets)) {
        json.assets[id] = { type: 'geoscript', file: `geo/${file}` };
      } else {
        console.warn(
          `[loadLevelData] "${name}": geo/${file} ignored — asset "${id}" already defined in def.json`
        );
      }
    }
  }

  // Run generator modules if any are declared. Generators push additional objects
  // into the def before validation so their output is fully validated alongside
  // the static objects.
  if (Array.isArray(json.generators) && json.generators.length > 0) {
    for (const genDef of json.generators) {
      const genPath = join(levelDir, genDef.file as string);
      if (!existsSync(genPath)) {
        throw new Error(`[loadLevelData] Generator "${genDef.file}" not found in level "${name}"`);
      }
      const fn = loadGeneratorFn(genPath);
      const result = await fn({
        def: json as import('./types').LevelDefRaw,
        physics: (json.physics as import('./types').ScenePhysicsDef) ?? {},
        params: (genDef.params as Record<string, unknown>) ?? {},
      });
      json.objects = [...(json.objects ?? []), ...result.objects.map(markGeneratedNode)];
    }
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
