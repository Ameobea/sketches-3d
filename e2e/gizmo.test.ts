import type { Browser, Page } from 'playwright-core';
import { afterAll, beforeAll, expect, test } from 'vitest';

import { launch, newPage, openEditor, run, sceneDigest, setCode } from './lib';

let browser: Browser;
let page: Page;

beforeAll(async () => {
  browser = await launch();
  page = await newPage(await browser.newContext());
  await openEditor(page, '/geotoy/edit');
});

afterAll(() => browser?.close());

const ORIGIN = [0, 2, 0];

/** Page coords of `ORIGIN` and of a point `px` pixels along the on-screen direction of world +Y. */
const alongY = (px: number) =>
  page.evaluate(
    ([origin, px]) => {
      const { camera, renderer } = (window as any).viz;
      const r = renderer.domElement.getBoundingClientRect();
      const project = (y: number) => {
        const v = camera.position
          .clone()
          .set(origin[0], origin[1] + y, origin[2])
          .project(camera);
        return [r.left + ((v.x + 1) / 2) * r.width, r.top + ((1 - v.y) / 2) * r.height];
      };
      const [o, up] = [project(0), project(1)];
      const len = Math.hypot(up[0] - o[0], up[1] - o[1]);
      return o.map((c, i) => c + ((up[i] - o[i]) / len) * px) as [number, number];
    },
    [ORIGIN, px] as const
  );

/** Lit pixels in the just-presented frame within `radius` px of a page coord. The scene background
 *  is black and the probed spots are clear of the box, so anything lit came from the overlay pass. */
const litPixels = ([x, y]: [number, number], radius = 6) =>
  page.evaluate(
    ([x, y, radius]) =>
      new Promise<number>(resolve => {
        const viz = (window as any).viz;
        const cb = () => {
          viz.unregisterAfterRenderCb(cb);
          const gl = viz.renderer.domElement as HTMLCanvasElement;
          const r = gl.getBoundingClientRect();
          const dpr = gl.width / r.width;
          const side = Math.ceil(2 * radius * dpr);
          const ctx = new OffscreenCanvas(side, side).getContext('2d')!;
          ctx.drawImage(
            gl,
            (x - r.left - radius) * dpr,
            (y - r.top - radius) * dpr,
            side,
            side,
            0,
            0,
            side,
            side
          );
          const px = ctx.getImageData(0, 0, side, side).data;
          let lit = 0;
          for (let i = 0; i < px.length; i += 4) {
            if (px[i] + px[i + 1] + px[i + 2] > 96) lit++;
          }
          resolve(lit);
        };
        viz.registerAfterRenderCb(cb);
        viz.invalidate();
      }),
    [x, y, radius] as const
  );

const boxCenter = async () => {
  const [, , min, max] = (await sceneDigest(page)).find(d => d.startsWith('Mesh:'))!.split(':');
  const [a, b] = [min, max].map(v => v.split(',').map(Number));
  return a.map((v, i) => (v + b[i]) / 2);
};

test('gizmo ghost + armed handle draw in the viewport, and an axis drag feeds back into the program', async () => {
  await setCode(page, 'box(1) | render');
  await run(page);
  const origin = await alongY(0);
  const shaft = await alongY(40);
  expect(await litPixels(origin)).toBe(0);

  await setCode(page, `p = gizmo("a", origin=vec3(${ORIGIN}))\nbox(1) | translate(p) | render`);
  await run(page);
  expect(await litPixels(origin)).toBeGreaterThan(0);
  expect(await litPixels(shaft)).toBe(0);

  await page.click('.cm-gizmo-chip');
  await expect.poll(() => litPixels(shaft)).toBeGreaterThan(0);

  await page.mouse.move(...shaft);
  await page.mouse.down();
  await page.mouse.move(...(await alongY(140)), { steps: 10 });
  await page.mouse.up();
  await page.waitForTimeout(150);
  await page.waitForSelector('button.run:not([disabled])');

  const [x, y, z] = await boxCenter();
  expect(y).toBeGreaterThan(0.2);
  expect(x).toBeCloseTo(0);
  expect(z).toBeCloseTo(0);
  const readout = (await page.textContent('.cm-gizmo-readout'))!.match(/-?[\d.]+/g)!.map(Number);
  expect(readout[0]).toBe(0);
  expect(readout[1]).toBeCloseTo(y, 1);
  expect(readout[2]).toBe(0);

  // the armed gizmo follows the handle to its dragged position
  const moved = await page.evaluate(y => {
    (window as any).viz.overlayScene.updateMatrixWorld(true);
    return (window as any).viz.overlayScene.children.some(
      (o: any) => o.visible && Math.abs(o.getWorldPosition(o.position.clone()).y - y) < 1e-3
    );
  }, ORIGIN[1] + y);
  expect(moved).toBe(true);
});
