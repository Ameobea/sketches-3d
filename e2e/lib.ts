import { chromium, type Browser, type BrowserContext, type Page } from 'playwright-core';

export const BASE = process.env.E2E_BASE ?? 'http://localhost:4810';

export const launch = () =>
  chromium.launch({
    executablePath: '/usr/bin/google-chrome-unstable',
    args: ['--enable-unsafe-swiftshader', '--use-angle=swiftshader'],
  });

export const newPage = async (ctx: BrowserContext) => {
  const page = await ctx.newPage();
  await page.setViewportSize({ width: 1600, height: 1000 });
  page.setDefaultTimeout(120_000);
  return page;
};

export const api = async <T = any>(
  page: Page,
  path: string,
  init: { method?: string; data?: unknown } = {}
) => {
  const r = await page.request.fetch(`${BASE}/geotoy_api${path}`, init);
  if (!r.ok()) throw new Error(`${init.method ?? 'GET'} ${path}: ${r.status()} ${await r.text()}`);
  return (r.headers()['content-type']?.includes('json') ? r.json() : r.text()) as Promise<T>;
};

export const register = async (browser: Browser) => {
  const ctx = await browser.newContext();
  const page = await newPage(ctx);
  const user = { username: `e2e_${Date.now().toString(36)}`, password: 'e2e_pass_1234' };
  await api(page, '/users/register', { method: 'POST', data: user });
  await api(page, '/users/login', { method: 'POST', data: user });
  return { ctx, page, storage: await ctx.storageState() };
};

const editorView = `(() => { const c = document.querySelector('.cm-content'); return c?.cmTile?.view ?? c?.cmView?.rootView?.view; })()`;

export const setCode = (page: Page, code: string) =>
  page.evaluate(
    ([code, ev]) => {
      const v = (0, eval)(ev);
      v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: code } });
    },
    [code, editorView]
  );

export const getCode = (page: Page): Promise<string> =>
  page.evaluate(ev => (0, eval)(ev).state.doc.toString(), editorView);

export const openEditor = async (page: Page, path: string) => {
  await page.goto(BASE + path, { waitUntil: 'networkidle' });
  await page.waitForSelector('.cm-content');
  await page.waitForSelector('button.run:not([disabled])');
};

export const run = async (page: Page) => {
  await page.click('button.run:not([disabled])');
  await page.waitForTimeout(150);
  await page.waitForSelector('button.run:not([disabled])');
  const err = await page.evaluate(() => document.querySelector('.run-output .error')?.textContent ?? null);
  if (err) throw new Error('run failed: ' + err);
};

// Escape would open the pause menu.
const closePopover = async (page: Page) => {
  await page.click('button.save');
  await page.waitForSelector('#title-input', { state: 'detached' });
};

export const save = async (page: Page, { title, isShared }: { title?: string; isShared?: boolean } = {}) => {
  await page.click('button.save');
  await page.waitForSelector('#title-input');
  if (title !== undefined) await page.fill('#title-input', title);
  if (isShared !== undefined) await page.setChecked('#is-shared-checkbox', isShared);
  await page.locator('.popover button', { hasText: /^\s*save\s*$/ }).click();
  await page.waitForSelector('.popover .status.ok');
  await closePopover(page);
};

export const formState = async (page: Page) => {
  await page.click('button.save');
  await page.waitForSelector('#title-input');
  const res = {
    title: await page.inputValue('#title-input'),
    isShared: await page.isChecked('#is-shared-checkbox'),
  };
  await closePopover(page);
  return res;
};

/** Order-independent digest of the run's visible meshes/lines: vertex count + world bbox. */
export const sceneDigest = (page: Page): Promise<string[]> =>
  page.evaluate(() => {
    const viz = (window as any).viz;
    const out: string[] = [];
    viz.scene.updateMatrixWorld(true);
    viz.scene.traverse((o: any) => {
      if (!(o.isMesh || o.isLine) || o.userData.reuseKey === undefined) return;
      for (let q = o; q; q = q.parent) if (!q.visible) return;
      o.geometry.computeBoundingBox();
      const b = o.geometry.boundingBox.clone().applyMatrix4(o.matrixWorld);
      const f = (v: any) => [v.x, v.y, v.z].map((x: number) => x.toFixed(3)).join(',');
      out.push(`${o.type}:${o.geometry.attributes.position.count}:${f(b.min)}:${f(b.max)}`);
    });
    return out.sort();
  });
