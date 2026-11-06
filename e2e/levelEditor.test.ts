import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import type { Browser, Page } from 'playwright-core';
import { afterAll, beforeAll, expect, test } from 'vitest';

import { BASE, launch, newPage } from './lib';

const NAME = 'e2e_scratch';
const levelDir = join('src/levels', NAME);
const routeDir = join('src/routes/(generated)', NAME);
const defPath = join(levelDir, 'def.json');
const readObjects = () => JSON.parse(readFileSync(defPath, 'utf8')).objects;

let browser: Browser;
let page: Page;

const openLevel = async () => {
  await page.goto(`${BASE}/${NAME}`, { waitUntil: 'load' });
  await page.waitForFunction(() => (window as any).levelEditor?.rootNodes.length >= 2);
};

/** Copies node `id` via the clipboard and pastes it; resolves to the pasted node's id. */
const pasteCopyOf = (id: string): Promise<string> =>
  page.evaluate(async id => {
    const ed = (window as any).levelEditor;
    const src = ed.nodeById.get(id);
    src.object.updateMatrixWorld(true);
    const p = new src.object.position.constructor(),
      q = src.object.quaternion.clone(),
      s = p.clone();
    src.object.matrixWorld.decompose(p, q, s);
    const e = new src.object.rotation.constructor().setFromQuaternion(q, 'YXZ');
    const world = { position: p.toArray(), rotation: [e.x, e.y, e.z], scale: s.toArray() };
    ed.clipboard = [ed.mutationController.captureClipboardEntry(src, world)];
    await ed.pasteObject();
    return ed.selectedNode.id;
  }, id);

/** `fn` runs in-page on the editor and returns node pairs; resolves to the angle between each pair's world rotations. */
const worldAngles = (fn: string): Promise<number[]> =>
  page.evaluate(async fn => {
    const ed = (window as any).levelEditor;
    const pairs: [any, any][] = await (0, eval)(fn)(ed);
    return pairs.map(([a, b]) => {
      const q = (o: any) => (o.updateMatrixWorld(true), o.getWorldQuaternion(o.quaternion.clone()));
      return q(a.object).angleTo(q(b.object));
    });
  }, fn);

beforeAll(async () => {
  mkdirSync(levelDir);
  writeFileSync(
    defPath,
    JSON.stringify({
      $schema: '../schema.json',
      version: 1,
      assets: { box: { type: 'geoscript', code: 'export mesh = box(1, 2, 3)' } },
      objects: [
        { id: 'a', asset: 'box', position: [0, 0, 0], rotation: [0.5, 0.7, 0] },
        {
          id: 'grp',
          position: [6, 0, 0],
          rotation: [0.3, 0.9, 0.2],
          scale: [1, 1, 1],
          children: [
            { id: 'b', asset: 'box', position: [1, 0, 0], rotation: [0.4, -0.6, 0.1] },
            { id: 'c', asset: 'box', position: [-1, 0, 0] },
            {
              id: 'tok',
              position: [0, 2, 0],
              parkour: { entities: [{ kind: 'dashToken', checkpointIx: 0 }] },
            },
          ],
        },
      ],
    })
  );
  mkdirSync(routeDir);
  for (const f of ['+page.svelte', '+page.server.ts']) {
    const src = readFileSync(join('src/routes/(generated)/scene_def_test', f), 'utf8');
    writeFileSync(join(routeDir, f), src.replaceAll("'scene_def_test'", `'${NAME}'`));
  }
  browser = await launch();
  page = await newPage(await browser.newContext());
  await openLevel();
});

afterAll(async () => {
  await browser?.close();
  rmSync(routeDir, { recursive: true, force: true });
  rmSync(levelDir, { recursive: true, force: true });
});

test('structural delete then undo of loaded and pasted groups leaves the disk unchanged', async () => {
  const pasted = await pasteCopyOf('grp');
  const before = readObjects();
  expect(before.find((o: any) => o.id === pasted).children.filter((c: any) => c.parkour)).toHaveLength(1);
  await page.evaluate(async pasted => {
    const ed = (window as any).levelEditor;
    const mc = ed.mutationController;
    mc.deleteNodes([ed.nodeById.get('grp'), ed.nodeById.get(pasted)]);
    await mc.structuralSyncQueue;
  }, pasted);
  expect(readObjects().map((o: any) => o.id)).toEqual(['a']);
  await page.evaluate(async () => {
    const ed = (window as any).levelEditor;
    ed.undoSystem.undo(ed.applyUndoEntry);
    await new Promise(r => setTimeout(r, 50));
    await ed.mutationController.structuralSyncQueue;
  });
  expect(readObjects()).toEqual(before);
});

test('a standalone dash-token marker can be copied and pasted', async () => {
  const id = await pasteCopyOf('tok');
  const children = readObjects().find((o: any) => o.id === 'grp').children;
  const [src, copy] = ['tok', id].map(id => children.find((c: any) => c.id === id));
  expect(copy.parkour).toEqual(src.parkour);
});

test('copy/paste preserves world orientation of rotated leaves, nested leaves and groups', async () => {
  const pairs: [string, string][] = [];
  for (const id of ['a', 'b', 'grp']) pairs.push([id, await pasteCopyOf(id)]);
  const angles = await worldAngles(
    `ed => ${JSON.stringify(pairs)}.map(([a, b]) => [ed.nodeById.get(a), ed.nodeById.get(b)])`
  );
  for (const a of angles) expect(a).toBeLessThan(1e-3);
});

test('an editor-created group keeps its rotation across reload', async () => {
  const id = await page.evaluate(async () => {
    const ed = (window as any).levelEditor;
    const g = await ed.mutationController.spawnGroup([0, 5, 0]);
    const E = g.object.rotation.constructor;
    g.object.quaternion.setFromEuler(new E(0.6, 0.8, 0.3, 'XYZ'));
    await ed.api.saveTransform(g);
    return g.id as string;
  });
  const quat = () =>
    page.evaluate(id => (window as any).levelEditor.nodeById.get(id).object.quaternion.toArray(), id);
  const before: number[] = await quat();
  await openLevel();
  const after: number[] = await quat();
  const dot = Math.abs(before.reduce((s, v, i) => s + v * after[i], 0));
  expect(2 * Math.acos(Math.min(1, dot))).toBeLessThan(1e-3);
});
