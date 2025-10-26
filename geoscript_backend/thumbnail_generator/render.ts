// yes this is 100% vibecoded

import express, { type Request, type Response } from 'express';
import puppeteer, { type Page, type Browser } from 'puppeteer';
import sharp from 'sharp';

const app = express();
const port = 5812;

const BROWSER_ARGS = [
  '--no-sandbox',
  '--disable-setuid-sandbox',
  '--disable-dev-shm-usage',
  '--use-gl=swiftshader',
  '--enable-webgl',
  '--ignore-gpu-blacklist',
];

interface CompositionRender {
  versionId: number;
  abortController: AbortController;
}

interface MaterialRender {
  abortController: AbortController;
}

const activeCompositionRenders = new Map<string, CompositionRender>();
const activeMaterialRenders = new Map<string, MaterialRender>();

async function setupPage(
  page: Page,
  url: string,
  abortController: AbortController
): Promise<{ promise: Promise<void> }> {
  await page.setViewport({ width: 600, height: 600 });

  const renderReadyPromise = new Promise<void>((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error('Render timed out after 30 minutes')), 30 * 60 * 1000);

    const onAbort = () => {
      clearTimeout(timeout);
      reject(new Error('Render cancelled by newer request'));
    };

    if (abortController.signal.aborted) {
      clearTimeout(timeout);
      reject(new Error('Render cancelled by newer request'));
      return;
    }

    abortController.signal.addEventListener('abort', onAbort);

    page.exposeFunction('onRenderReady', () => {
      clearTimeout(timeout);
      abortController.signal.removeEventListener('abort', onAbort);
      resolve();
    });
  });

  await page.setRequestInterception(true);

  page.on('request', request => {
    const requestUrlString = request.url();
    let requestUrl: URL;

    try {
      requestUrl = new URL(requestUrlString);
    } catch (_err) {
      console.log(`Blocking request to invalid URL: ${requestUrlString}`);
      request.abort();
      return;
    }

    const ipv4Regex = /^\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}$/;
    // Block if the hostname is an IP address. This covers IPv4 and IPv6 (which contain colons).
    const isIpAddress = ipv4Regex.test(requestUrl.hostname) || requestUrl.hostname.includes(':');

    if (isIpAddress) {
      console.log(`Blocking request to IP address: ${requestUrlString}`);
      request.abort();
      return;
    }

    // Block requests to the file protocol, or localhost (which might not be an IP if not resolved yet)
    if (requestUrl.protocol === 'file:' || requestUrl.hostname === 'localhost') {
      console.log(`Blocking request to local resource: ${requestUrlString}`);
      request.abort();
      return;
    }

    // Block navigations away from the target page
    if (request.resourceType() === 'document' && requestUrlString !== url) {
      console.log(`Blocking navigation to: ${requestUrlString}`);
      request.abort();
      return;
    }

    // Allow all other requests to continue
    request.continue();
  });

  return { promise: renderReadyPromise };
}

async function render(url: string, abortController: AbortController): Promise<Buffer> {
  console.log('Launching browser...');
  const browser: Browser = await puppeteer.launch({ args: BROWSER_ARGS });

  try {
    const page = await browser.newPage();
    const { promise: renderReadyPromise } = await setupPage(page, url, abortController);

    console.log(`Navigating to ${url}...`);
    await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 60000 });

    console.log('Waiting for visualization to signal readiness...');
    await renderReadyPromise;

    console.log('Taking screenshot...');
    const screenshotBuffer = await page.screenshot();

    console.log('Encoding screenshot to AVIF...');
    const avifBuffer = await sharp(screenshotBuffer).avif({ quality: 70, effort: 7 }).toBuffer();

    console.log('Successfully captured + encoded screenshot.');

    return avifBuffer;
  } finally {
    console.log('Closing browser...');
    await browser.close();
  }
}

app.get('/render/:id', async (req: Request, res: Response) => {
  const sceneId = req.params.id;
  const adminToken = req.query.admin_token as string | undefined;
  const versionID = req.query.version_id as string | undefined;

  if (!/^[0-9]+$/.test(sceneId)) {
    return res.status(400).send('Invalid scene ID');
  }

  const versionNum = parseInt(versionID!, 10);
  if (isNaN(versionNum)) {
    return res.status(400).send('Invalid version ID');
  }

  // Check if there's already an active render for this composition
  const existingRender = activeCompositionRenders.get(sceneId);
  if (existingRender) {
    if (versionNum <= existingRender.versionId) {
      // New request is for an older or same version, reject it
      console.log(
        `Rejecting render request for composition ${sceneId} version ${versionNum} (current: ${existingRender.versionId})`
      );
      return res.status(409).send('A render for a newer or equal version is already in progress');
    } else {
      // New request is for a newer version, cancel the old one
      console.log(
        `Cancelling render for composition ${sceneId} version ${existingRender.versionId} (new: ${versionNum})`
      );
      existingRender.abortController.abort();
    }
  }

  const abortController = new AbortController();
  activeCompositionRenders.set(sceneId, { versionId: versionNum, abortController });

  try {
    const baseUrl = 'https://3d.ameo.design/geotoy/edit/';
    const url = `${baseUrl}${sceneId}?render=true&admin_token=${adminToken}&version_id=${versionID}`;
    const screenshot = await render(url, abortController);
    res.set('Content-Type', 'image/avif');
    res.send(screenshot);
  } catch (error) {
    if (error instanceof Error && error.message === 'Render cancelled by newer request') {
      console.log(`Render for composition ${sceneId} version ${versionNum} was cancelled`);
      return res.status(409).send('Render cancelled due to newer request');
    }
    console.error(error);
    res.status(500).send('Error generating screenshot');
  } finally {
    // Clean up tracking - only if this is still the active render
    const currentRender = activeCompositionRenders.get(sceneId);
    if (currentRender && currentRender.versionId === versionNum) {
      activeCompositionRenders.delete(sceneId);
    }
  }
});

app.get('/render_material/:id', async (req: Request, res: Response) => {
  const materialId = req.params.id;
  const adminToken = req.query.admin_token as string | undefined;

  if (!/^[0-9]+$/.test(materialId)) {
    return res.status(400).send('Invalid material ID');
  }

  // Check if there's already an active render for this material
  const existingRender = activeMaterialRenders.get(materialId);
  if (existingRender) {
    console.log(`Cancelling existing render for material ${materialId}`);
    existingRender.abortController.abort();
  }

  const abortController = new AbortController();
  activeMaterialRenders.set(materialId, { abortController });

  try {
    const baseUrl = 'https://3d.ameo.design/geotoy/material/preview/';
    const url = `${baseUrl}${materialId}?render=true&admin_token=${adminToken}`;
    const screenshot = await render(url, abortController);
    res.set('Content-Type', 'image/avif');
    res.send(screenshot);
  } catch (error) {
    if (error instanceof Error && error.message === 'Render cancelled by newer request') {
      console.log(`Render for material ${materialId} was cancelled`);
      return res.status(409).send('Render cancelled due to newer request');
    }
    console.error(error);
    res.status(500).send('Error generating screenshot');
  } finally {
    activeMaterialRenders.delete(materialId);
  }
});

const imageBodyParser = express.raw({ type: 'image/*', limit: '100mb' });

app.post('/thumbnail', imageBodyParser, async (req: Request, res: Response) => {
  if (!req.body || !Buffer.isBuffer(req.body)) {
    return res
      .status(400)
      .send('Invalid image payload. Make sure to set Content-Type to an image type (e.g. image/png).');
  }

  if (Buffer.byteLength(req.body) === 0) {
    return res
      .status(400)
      .send(
        'Invalid or empty image payload. Make sure to set Content-Type to an image type (e.g. image/png).'
      );
  }

  try {
    const imageBuffer = await sharp(req.body)
      .resize(256, 256, {
        fit: 'inside',
        withoutEnlargement: true,
      })
      .avif({ quality: 70, effort: 5 })
      .toBuffer();

    res.set('Content-Type', 'image/avif');
    res.send(imageBuffer);
  } catch (error) {
    console.error('Error processing image for thumbnail:', error);
    res.status(500).send('Error generating thumbnail');
  }
});

app.post('/convert-to-avif', imageBodyParser, async (req: Request, res: Response) => {
  if (Buffer.byteLength(req.body) === 0) {
    return res
      .status(400)
      .send(
        'Invalid or empty image payload. Make sure to set Content-Type to an image type (e.g. image/png).'
      );
  }

  try {
    const avifBuffer = await sharp(req.body).avif({ quality: 85, effort: 7 }).toBuffer();

    res.set('Content-Type', 'image/avif');
    res.send(avifBuffer);
  } catch (error) {
    if (error instanceof Error) {
      console.error('Error converting image to AVIF:', error);
      res.status(500).send(`Error converting image to AVIF: ${error.message}`);
    } else {
      console.error('Error converting image to AVIF:', error);
      res.status(500).send('Error converting image to AVIF');
    }
  }
});

app.listen(port, () => {
  console.log(`Screenshot service listening at http://localhost:${port}`);
});
