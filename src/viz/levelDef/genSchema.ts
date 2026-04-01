/**
 * Generates JSON Schema files from the Zod level def schemas.
 * Run with: yarn gen:level-schema
 */
import { writeFileSync } from 'fs';
import { join } from 'path';
import { z } from 'zod';

import { LevelDefRawSchema, MaterialsFileSchema, ObjectsFileSchema } from './types';

const schemasDir = join(import.meta.dirname, '../../../src/levels');

const write = (filename: string, schema: Record<string, unknown>, title: string, id: string) => {
  schema.title = title;
  schema.$id = id;
  const outPath = join(schemasDir, filename);
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
