import { expect, test } from 'bun:test';
import { readdirSync, readFileSync } from 'fs';
import { join } from 'path';

import type { CompositionDoc, NodeDef } from '../src/geoscript/geotoyAPIClient';
import { applyToDoc, parseGeoFile, parsedFromDoc, renderGeo } from './geo-composition-doc';

const dir = join(import.meta.dir, '..', 'geo_compositions');

test('every synced .geo file round-trips byte-for-byte', () => {
  const names = readdirSync(dir).filter(n => /^\d+_.*\.geo$/.test(n));
  expect(names.length).toBeGreaterThan(0);
  for (const name of names) {
    const text = readFileSync(join(dir, name), 'utf8');
    expect(renderGeo(parseGeoFile(text))).toBe(text);
  }
});

const node = (id: string, name: string, source: string, children: string[] = []): NodeDef => ({
  id,
  name,
  source,
  instances: [{ pos: [1, 2, 3], rot: [0, 0, 0], scale: [1, 1, 1], id: `i${id}` }],
  children,
});

const doc: CompositionDoc = {
  version: 2,
  trees: [
    {
      id: 'main',
      kind: 'mesh',
      name: 'main',
      tree: {
        version: 1,
        rootId: 'r',
        globalsSource: 'g = 1\n',
        nodes: {
          r: node('r', '_root', 'a | render', ['b', 'a']),
          a: node('a', 'leaf', ''),
          b: node('b', 'mid', '// === Cube 3-Compound ===\nbox(1)\n', ['c']),
          c: node('c', 'leaf', 'sphere(2)'),
          z: node('z', 'orphan', 'unreachable'),
        },
      },
    },
    {
      id: 'tex',
      kind: 'texture',
      name: 'tex',
      tree: {
        version: 1,
        rootId: 'r',
        globalsSource: '  \n',
        nodes: { r: node('r', '_root', 'texture(8, 8, |uv| uv.x) | render_texture') },
      },
    },
  ],
};

const blanked = (d: CompositionDoc): CompositionDoc => {
  const out = structuredClone(d);
  for (const { tree } of out.trees) {
    if (tree.globalsSource.trim()) {
      tree.globalsSource = 'STALE';
    }
    for (const n of Object.values(tree.nodes)) {
      n.source = 'STALE';
    }
  }
  return out;
};

test('applyToDoc inverts the sync flattening, including duplicate names and orphans', () => {
  const text = renderGeo(parsedFromDoc(7, 'demo: (x)', doc));
  const parsed = parseGeoFile(text);
  expect(parsed.id).toBe(7);
  expect(parsed.title).toBe('demo: (x)');
  const applied = applyToDoc(blanked(doc), parsed);
  expect(applied).toEqual(doc);
  expect(renderGeo(parsedFromDoc(7, 'demo: (x)', applied))).toBe(text);

  const noGlobals = { ...parsed, trees: parsed.trees.map(t => ({ ...t, globalsSource: null })) };
  expect(applyToDoc(doc, noGlobals).trees.map(t => t.tree.globalsSource)).toEqual(['', '  \n']);
});

test('single-tree file targets the only tree regardless of name; unknown nodes throw', () => {
  const single: CompositionDoc = { version: 2, trees: [{ ...doc.trees[1], name: 'oddly_named' }] };
  const parsed = parseGeoFile(renderGeo(parsedFromDoc(3, 't', single)));
  expect(parsed.trees[0].name).toBe('main');
  expect(applyToDoc(blanked(single), parsed)).toEqual(single);
  expect(() => applyToDoc(single, parseGeoFile('// Composition 3: t\n// === node: nope ===\nx\n'))).toThrow(
    /node `nope`/
  );
});
