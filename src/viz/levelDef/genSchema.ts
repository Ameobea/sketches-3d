/**
 * Generates JSON Schema files from the Zod level def schemas.
 * Run with: yarn gen:level-schema
 *
 * `--check` renders without writing and exits 1 if any committed file is stale
 * (wired into `yarn run check` so schema edits can't silently skip regeneration).
 */
import { readFileSync, writeFileSync } from 'fs';
import { join } from 'path';
import { z } from 'zod';

import {
  AudioFileSchema,
  LevelDefRawSchema,
  LibraryMaterialFileSchema,
  LocationsFileSchema,
  MaterialsFileSchema,
  ObjectsFileSchema,
} from './types';

const schemasDir = join(import.meta.dirname, '../../../src/levels');
const assetsSchemasDir = join(import.meta.dirname, '../../../src/assets');

const checkMode = process.argv.includes('--check');
const stale: string[] = [];

const write = (
  filename: string,
  schema: Record<string, unknown>,
  title: string,
  id: string,
  dir: string = schemasDir
) => {
  schema.title = title;
  schema.$id = id;
  const outPath = join(dir, filename);
  const rendered = JSON.stringify(schema, null, 2) + '\n';
  if (checkMode) {
    let current: string | null = null;
    try {
      current = readFileSync(outPath, 'utf-8');
    } catch {
      // missing counts as stale
    }
    if (current !== rendered) stale.push(outPath);
    return;
  }
  writeFileSync(outPath, rendered, 'utf-8');
  console.log('Wrote', outPath);
};

write(
  'schema.json',
  z.toJSONSchema(LevelDefRawSchema, { target: 'draft-7' }) as Record<string, unknown>,
  'LevelDef',
  'https://ameo.design/schemas/level-def.json'
);

write(
  'materials-schema.json',
  z.toJSONSchema(MaterialsFileSchema, { target: 'draft-7' }) as Record<string, unknown>,
  'MaterialsFile',
  'https://ameo.design/schemas/level-materials.json'
);

write(
  'objects-schema.json',
  z.toJSONSchema(ObjectsFileSchema, { target: 'draft-7' }) as Record<string, unknown>,
  'ObjectsFile',
  'https://ameo.design/schemas/level-objects.json'
);

write(
  'locations-schema.json',
  z.toJSONSchema(LocationsFileSchema, { target: 'draft-7' }) as Record<string, unknown>,
  'LocationsFile',
  'https://ameo.design/schemas/level-locations.json'
);

write(
  'audio-schema.json',
  z.toJSONSchema(AudioFileSchema, { target: 'draft-7' }) as Record<string, unknown>,
  'AudioFile',
  'https://ameo.design/schemas/level-audio.json'
);

write(
  'library-material-schema.json',
  z.toJSONSchema(LibraryMaterialFileSchema, { target: 'draft-7' }) as Record<string, unknown>,
  'LibraryMaterialFile',
  'https://ameo.design/schemas/library-material.json',
  assetsSchemasDir
);

if (checkMode && stale.length) {
  console.error(`Stale generated schema(s) — run \`yarn gen:level-schema\` and commit:`);
  for (const p of stale) console.error(`  ${p}`);
  process.exit(1);
}
