import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterAll, expect, test } from 'vitest';

const root = mkdtempSync(join(tmpdir(), 'levels-'));
process.env.LEVELS_DIR = root;
afterAll(() => rmSync(root, { recursive: true, force: true }));

const { openLevel } = await import('src/routes/level_editor/levelEditorUtils.server');
const { loadLevelData } = await import('src/viz/levelDef/loadLevelData.server');

test('openLevel().save() keeps audio, discovered .geo assets and `_meta` out of def.json', async () => {
  const dir = join(root, 'lvl');
  const meta = (count: number) => ({ runtimeMs: 1, count, codeHash: 'h' });
  mkdirSync(join(dir, 'geo'), { recursive: true });
  const write = (f: string, v: unknown) =>
    writeFileSync(join(dir, f), typeof v === 'string' ? v : JSON.stringify(v));
  write('def.json', {
    version: 1,
    assets: { box: { type: 'geoscript', code: 'export mesh = box(1)', _meta: meta(1) } },
    objects: [],
  });
  write('objects.json', { objects: [{ id: 'a', asset: 'box', position: [0, 0, 0] }] });
  write('audio.json', { sfxDefs: { ping: { url: 'ping.ogg' } } });
  write('geo/thing.geo', 'export mesh = box(2)');

  const level = openLevel('lvl');
  level.def.objects[0].position = [1, 2, 3];
  level.save();

  const def = JSON.parse(readFileSync(join(dir, 'def.json'), 'utf8'));
  expect(def.audio).toBeUndefined();
  expect(def.assets).toEqual({ box: { type: 'geoscript', code: 'export mesh = box(1)' } });
  expect(JSON.parse(readFileSync(join(dir, 'objects.json'), 'utf8')).objects[0].position).toEqual([1, 2, 3]);
  const defJson = readFileSync(join(dir, 'def.json'), 'utf8');

  const metaLevel = openLevel('lvl');
  expect((metaLevel.def.assets.box as { _meta?: unknown })._meta).toEqual(meta(1));
  (metaLevel.def.assets.thing as { _meta?: unknown })._meta = meta(2);
  metaLevel.saveMeta();
  expect(readFileSync(join(dir, 'def.json'), 'utf8')).toBe(defJson);
  expect(JSON.parse(readFileSync(join(dir, '.meta.json'), 'utf8'))).toEqual({ box: meta(1), thing: meta(2) });

  rmSync(join(dir, 'geo/thing.geo'));
  expect((await loadLevelData('lvl')).objects.map(o => o.id)).toEqual(['a']);
});
