import type { Browser, Page } from 'playwright-core';
import { afterAll, beforeAll, expect, test } from 'vitest';

import {
  api,
  BASE,
  formState,
  getCode,
  launch,
  newPage,
  openEditor,
  register,
  run,
  save,
  sceneDigest,
  setCode,
} from './lib';

let browser: Browser;
let owner: Awaited<ReturnType<typeof register>>;
let page: Page;
let compId: number;

const server = () => api(page, `/compositions/${compId}`);
const latestSource = async (p: Page, id: number): Promise<string> => {
  const { tree } = (await api(p, `/compositions/${id}/latest`)).tree.trees[0];
  return tree.nodes[tree.rootId].source;
};

beforeAll(async () => {
  browser = await launch();
  owner = await register(browser);
  page = owner.page;
  page.on('dialog', d => d.accept());
  await openEditor(page, '/geotoy/edit');
  await setCode(page, 'box(2) | render // v1');
  await run(page);
  await page.click('button.save');
  await page.fill('#title-input', 'e2e comp');
  await page.locator('.popover button', { hasText: /^\s*save\s*$/ }).click();
  await page.waitForURL(/\/geotoy\/edit\/\d+/);
  compId = +page.url().match(/edit\/(\d+)/)![1];
  await page.waitForSelector('button.run:not([disabled])');
});

afterAll(async () => {
  if (compId) await api(page, `/compositions/${compId}`, { method: 'DELETE' });
  await browser?.close();
});

test('save round-trip keeps metadata, post-save drafts, and clear-local-changes target', async () => {
  await setCode(page, 'box(2) | render // v2');
  await save(page, { title: 'renamed', isShared: true });
  expect(await server()).toMatchObject({ title: 'renamed', is_shared: true });

  await setCode(page, 'box(2) | render // v3');
  await save(page);
  expect(await server()).toMatchObject({ title: 'renamed', is_shared: true });
  expect(await formState(page)).toEqual({ title: 'renamed', isShared: true });
  expect(await latestSource(page, compId)).toBe('box(2) | render // v3');

  await setCode(page, 'box(2) | render // v4 unsaved');
  await run(page);
  await page.reload({ waitUntil: 'networkidle' });
  await page.waitForSelector('.cm-content');
  expect(await getCode(page)).toBe('box(2) | render // v4 unsaved');

  await page.locator('.menubar .title', { hasText: 'comp' }).click();
  await page.locator('.dropdown .item', { hasText: 'clear local changes' }).click();
  expect(await getCode(page)).toBe('box(2) | render // v3');
});

test('fork with local edits persists the edits as a single-version private "(fork)" comp', async () => {
  await api(page, `/compositions/${compId}`, {
    method: 'PATCH',
    data: { field_mask: ['is_shared'], patch: { is_shared: true } },
  });
  const b = await register(browser);
  await openEditor(b.page, `/geotoy/edit/${compId}`);
  await setCode(b.page, 'box(5) | render // forked edit');
  await b.page.click('button.save');
  await b.page.locator('.popover button', { hasText: /fork/ }).click();
  await b.page.waitForURL(u => /edit\/\d+/.test(u.pathname) && !u.pathname.endsWith(`/${compId}`));
  const forkId = +b.page.url().match(/edit\/(\d+)/)![1];
  await expect.poll(() => latestSource(b.page, forkId)).toBe('box(5) | render // forked edit');
  await b.ctx.close();

  const fresh = await browser.newContext({ storageState: b.storage });
  const p = await newPage(fresh);
  await openEditor(p, `/geotoy/edit/${forkId}`);
  expect(await getCode(p)).toBe('box(5) | render // forked edit');
  expect(await api(p, `/compositions/${forkId}/history`)).toHaveLength(1);
  expect(await api(p, `/compositions/${forkId}`)).toMatchObject({
    title: `${(await server()).title} (fork)`,
    is_shared: false,
  });
  await api(p, `/compositions/${forkId}`, { method: 'DELETE' });
  await fresh.close();
});

test('owner fork takes the title/description from the save form', async () => {
  await page.click('button.save');
  await page.fill('#title-input', 'forked title');
  await page.fill('#description-input', 'forked desc');
  await page.locator('.popover button', { hasText: /fork/ }).click();
  await page.waitForURL(u => /edit\/\d+/.test(u.pathname) && !u.pathname.endsWith(`/${compId}`));
  const forkId = +page.url().match(/edit\/(\d+)/)![1];
  expect(await api(page, `/compositions/${forkId}`)).toMatchObject({
    title: 'forked title',
    description: 'forked desc',
    forked_from_id: compId,
  });
  expect(await api(page, `/compositions/${forkId}/history`)).toHaveLength(1);
  expect(await server()).toMatchObject({ title: 'renamed', description: '' });
  await api(page, `/compositions/${forkId}`, { method: 'DELETE' });
  await openEditor(page, `/geotoy/edit/${compId}`);
});

test('leaving the editor tears it down; shadow-light reruns keep GPU textures flat', async () => {
  const ctx = await browser.newContext({ storageState: owner.storage });
  await ctx.addInitScript(() => {
    const w = window as any;
    w.__keydown = 0;
    const { addEventListener: add, removeEventListener: rm } = EventTarget.prototype;
    EventTarget.prototype.addEventListener = function (type: string, ...rest: any[]) {
      if (this === window && type === 'keydown') w.__keydown++;
      return add.call(this, type, ...(rest as [any]));
    };
    EventTarget.prototype.removeEventListener = function (type: string, ...rest: any[]) {
      if (this === window && type === 'keydown') w.__keydown--;
      return rm.call(this, type, ...(rest as [any]));
    };
  });
  const p = await newPage(ctx);
  const me = await api(p, '/users/me');
  await p.goto(BASE + `/geotoy/user/${me.id}`, { waitUntil: 'networkidle' });
  const keydown = () => p.evaluate(() => (window as any).__keydown as number);
  const workers = () => p.workers().filter(w => w.url().includes('geoscriptWorker')).length;
  const baseKeydown = await keydown();
  const live: number[] = [];
  for (let i = 0; i < 3; i++) {
    await p.locator(`a[href$="/geotoy/edit/${compId}"]`).first().click();
    await p.waitForSelector('button.run:not([disabled])');
    await p.goBack({ waitUntil: 'networkidle' });
    await p.waitForTimeout(500);
    expect(await keydown()).toBe(baseKeydown);
    live.push(workers());
  }
  expect(live[2]).toBe(live[0]);

  await p.locator(`a[href$="/geotoy/edit/${compId}"]`).first().click();
  await p.waitForSelector('button.run:not([disabled])');
  // an edited run replaces its lights, whose shadow maps (color + depth) only allocate on the next render
  const texturesAfterFrame = () =>
    new Promise<number>(r =>
      requestAnimationFrame(() =>
        requestAnimationFrame(() => r((window as any).viz.renderer.info.memory.textures))
      )
    );
  const textures: number[] = [];
  for (let i = 0; i < 10; i++) {
    await setCode(p, `box(${1 + i}) | render\ndir_light(intensity=3, cast_shadow=true) | render`);
    await run(p);
    textures.push(await p.evaluate(texturesAfterFrame));
  }
  // a leak adds a shadow map per run
  expect(Math.max(...textures)).toBeLessThan(textures[0] + 3);
  await ctx.close();
});

const rerunDigests = async (p: Page) => {
  await run(p);
  const a = await sceneDigest(p);
  await run(p);
  return [a, await sceneDigest(p)];
};

test('re-running a render_path + offset_path program renders the same output', async () => {
  await openEditor(page, `/geotoy/edit/${compId}`);
  await setCode(
    page,
    [
      'p = circle(center=v2(0), radius=3)',
      'p | render_path',
      'p | offset_path(delta=0.5) | render_path',
      'box(1) | render',
    ].join('\n')
  );
  const [a, b] = await rerunDigests(page);
  expect(a.length).toBeGreaterThan(2);
  expect(b).toEqual(a);
});

test('changing an ambient _globals value feeding a mesh re-renders with the new value', async () => {
  const extentX = async () => {
    const box = (await sceneDigest(page)).find(d => d.startsWith('Mesh:'))!.split(':');
    return +box[3].split(',')[0] - +box[2].split(',')[0];
  };
  const setGlobals = async (n: number) => {
    await page.click('.globals-row');
    await setCode(page, `n = { print("x"); ${n} }\nm = box(n)`);
    await page.locator('.hierarchy .row .node-id', { hasText: 'Root' }).click();
  };
  await page.click('button.add-node-btn');
  await setGlobals(1);
  await setCode(page, 'render(m | flip_normals)');
  await run(page);
  expect(await extentX()).toBeCloseTo(1);
  await setGlobals(2);
  await run(page);
  expect(await extentX()).toBeCloseTo(2);
});
