/**
 * Generates JSON Schema files from the Zod level def schemas.
 * Run with: yarn gen:level-schema
 */
import { writeFileSync } from 'fs';
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
  writeFileSync(outPath, JSON.stringify(schema, null, 2) + '\n', 'utf-8');
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
