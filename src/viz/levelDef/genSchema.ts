/**
 * Generates JSON Schema files from the Zod level def schemas.
 * Run with: yarn gen:level-schema
 *
 * `--check` renders without writing and exits 1 if any committed file is stale
 * (wired into `yarn run check` so schema edits can't silently skip regeneration).
 */
import { readFileSync, rmSync, writeFileSync } from 'fs';
import { join } from 'path';
import { pathToFileURL } from 'url';
import ts from 'typescript';
import { z } from 'zod';

const PROJECT_ROOT = join(import.meta.dirname, '../../..');
/** Schema modules whose JSDoc feeds the generated descriptions, by import alias. */
const DOC_MODULES = ['src/viz/levelDef/types', 'src/viz/materials/schema', 'src/viz/particles/schema'];
const documentedPath = (alias: string) => join(PROJECT_ROOT, `${alias}.__withDocs__.ts`);

/**
 * A copy of a schema module with every JSDoc'd property / `const` initializer wrapped in `__doc(expr, text)`,
 * which attaches the comment as zod metadata.  `z.toJSONSchema` emits it as `description` plus VS Code's
 * `markdownDescription`, so comments show on hover / completion in level JSON without being duplicated
 * into `.describe()` calls.  Imports of other doc modules are redirected to their documented copies.
 */
const buildDocumentedSource = (alias: string): string => {
  const path = join(PROJECT_ROOT, `${alias}.ts`);
  const src = readFileSync(path, 'utf-8');
  const sf = ts.createSourceFile(path, src, ts.ScriptTarget.Latest, true);
  const inserts: { pos: number; text: string }[] = [];
  const docOf = (node: ts.Node): string | undefined => {
    const comment = ts.getJSDocCommentsAndTags(node).find(ts.isJSDoc)?.comment;
    const text = comment === undefined ? '' : ts.getTextOfJSDocComment(comment)?.trim();
    return text || undefined;
  };
  const wrap = (init: ts.Expression, doc: string) => {
    inserts.push({ pos: init.getStart(sf), text: '__doc(' });
    inserts.push({ pos: init.getEnd(), text: `, ${JSON.stringify(doc)})` });
  };
  const visit = (node: ts.Node) => {
    if (ts.isPropertyAssignment(node)) {
      const doc = docOf(node);
      if (doc) {
        wrap(node.initializer, doc);
      }
    } else if (ts.isVariableStatement(node) && node.declarationList.declarations.length === 1) {
      const doc = docOf(node);
      const init = node.declarationList.declarations[0].initializer;
      if (doc && init) {
        wrap(init, doc);
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(sf);
  // Descending position; ties keep visit (outer-first) order so an inner suffix lands before the outer one.
  let out = src;
  for (const { pos, text } of inserts.sort((a, b) => b.pos - a.pos)) {
    out = out.slice(0, pos) + text + out.slice(pos);
  }
  for (const other of DOC_MODULES) {
    out = out.replaceAll(`'${other}'`, `'${other}.__withDocs__'`);
  }
  return `${out}
function __doc<T>(schema: T, doc: string): T {
  return schema instanceof z.ZodType ? (schema.meta({ description: doc, markdownDescription: doc }) as T) : schema;
}
`;
};

let types: typeof import('./types');
try {
  for (const alias of DOC_MODULES) {
    writeFileSync(documentedPath(alias), buildDocumentedSource(alias), 'utf-8');
  }
  types = await import(pathToFileURL(documentedPath('src/viz/levelDef/types')).href);
} finally {
  for (const alias of DOC_MODULES) {
    rmSync(documentedPath(alias), { force: true });
  }
}
const {
  AudioFileSchema,
  LevelDefRawSchema,
  LibraryMaterialFileSchema,
  LocationsFileSchema,
  MaterialsFileSchema,
  ObjectsFileSchema,
} = types;

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
