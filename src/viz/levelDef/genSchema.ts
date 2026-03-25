/**
 * Generates src/levels/schema.json from the Zod LevelDef schema.
 * Run with: yarn gen:level-schema
 */
import { writeFileSync } from 'fs';
import { join } from 'path';
import { z } from 'zod';

import { LevelDefRawSchema } from './types';

const schema = z.toJSONSchema(LevelDefRawSchema, {
  target: 'draft-7',
});

// Add a title so the schema is self-describing
(schema as Record<string, unknown>).title = 'LevelDef';
(schema as Record<string, unknown>).$id = 'https://ameo.design/schemas/level-def.json';

const outPath = join(import.meta.dirname, '../../../src/levels/schema.json');
writeFileSync(outPath, JSON.stringify(schema, null, 2) + '\n', 'utf-8');
console.log('Wrote', outPath);
