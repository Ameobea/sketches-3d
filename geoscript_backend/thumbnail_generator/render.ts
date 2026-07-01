// yes this is 100% vibecoded

import express, { type Request, type Response } from 'express';
import puppeteer, { type Page, type Browser } from 'puppeteer';
import sharp from 'sharp';

const app = express();
const port = 5812;

// macOS (dev) can't create a WebGL2 context under SwiftShader, so use ANGLE's
// Metal backend (real GPU); Linux servers keep software SwiftShader.
const GL_ARGS =
  process.platform === 'darwin' ? ['--use-gl=angle', '--use-angle=metal'] : ['--use-gl=swiftshader'];

const BROWSER_ARGS = [
  '--no-sandbox',
  '--disable-setuid-sandbox',
  '--disable-dev-shm-usage',
  ...GL_ARGS,
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

const PROD_TRANSIENT_URL = 'https://3d.ameo.design/geotoy/render';
const DEV_TRANSIENT_URL = 'http://localhost:4800/geotoy/render';

interface TransientRenderOptions {
  /** 'png' (default) | 'avif' | 'jpeg' */
  format?: 'png' | 'avif' | 'jpeg';
  /** Viewport width in pixels. Defaults to 800. */
  width?: number;
  /** Viewport height in pixels. Defaults to 800. */
  height?: number;
  /** Quality for lossy formats (avif/jpeg). 0-100. */
  quality?: number;
  /** If true, navigate to the local dev frontend instead of the prod URL. */
  dev?: boolean;
  /** Readiness timeout in ms before failing with captured diagnostics. */
  timeoutMs?: number;
  /** Debug material applied to all meshes before capture: 'normal' | 'wireframe' | 'wireframe-xray'. */
  materialOverride?: 'normal' | 'wireframe' | 'wireframe-xray';
}

interface TransientRenderBody {
  tree?: unknown;
  metadata?: unknown;
  options?: TransientRenderOptions;
}

async function setupPage(
  page: Page,
  url: string,
  abortController: AbortController,
  opts: { width?: number; height?: number; allowLocalhost?: boolean; timeoutMs?: number } = {}
): Promise<{ promise: Promise<void>; getDiagnostics: () => string[] }> {
  await page.setViewport({ width: opts.width ?? 600, height: opts.height ?? 600 });
  const allowLocalhost = !!opts.allowLocalhost;
  const timeoutMs = opts.timeoutMs ?? 30 * 60 * 1000;

  // Capture in-page failures (console errors/warnings, uncaught exceptions, failed
  // requests) so a hung or broken render can report *why* instead of a bare timeout.
  const diagnostics: string[] = [];
  const pushDiag = (line: string) => {
    if (diagnostics.length < 100) diagnostics.push(line);
  };
  page.on('console', msg => {
    const type = msg.type();
    if (type === 'error' || type === 'warning') {
      pushDiag(`[console.${type}] ${msg.text()}`);
    }
  });
  page.on('pageerror', err => pushDiag(`[pageerror] ${err.stack || err.message || String(err)}`));
  page.on('requestfailed', req => {
    const failure = req.failure();
    if (failure && failure.errorText !== 'net::ERR_ABORTED') {
      pushDiag(`[requestfailed] ${req.url()} — ${failure.errorText}`);
    }
  });
  await page.evaluateOnNewDocument(() => {
    window.addEventListener('unhandledrejection', e => {
      const reason = (e as PromiseRejectionEvent).reason;
      console.error('[unhandledrejection]', reason && reason.stack ? reason.stack : String(reason));
    });
  });

  const renderReadyPromise = new Promise<void>((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error(`Render timed out after ${timeoutMs}ms`)), timeoutMs);

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

    // Block file: always; block localhost unless explicitly allowed (dev mode).
    if (requestUrl.protocol === 'file:') {
      console.log(`Blocking request to local resource: ${requestUrlString}`);
      request.abort();
      return;
    }
    if (requestUrl.hostname === 'localhost' && !allowLocalhost) {
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

  return { promise: renderReadyPromise, getDiagnostics: () => diagnostics };
}

interface RenderOpts {
  /** Output image post-processing. If unset, AVIF q70 (legacy thumbnail behavior). */
  encode?: (raw: Buffer) => Promise<Buffer>;
  /** Viewport dimensions. */
  width?: number;
  height?: number;
  /** Allow puppeteer to load localhost resources (dev mode). */
  allowLocalhost?: boolean;
  /** Setup hook called before navigation (e.g. `evaluateOnNewDocument` for payload injection). */
  beforeNavigate?: (page: Page) => Promise<void>;
  /** Readiness timeout in ms before the render fails (with captured diagnostics). */
  timeoutMs?: number;
}

async function render(
  url: string,
  abortController: AbortController,
  opts: RenderOpts = {}
): Promise<Buffer> {
  console.log('Launching browser...');
  const browser: Browser = await puppeteer.launch({ args: BROWSER_ARGS });

  try {
    const page = await browser.newPage();
    const { promise: renderReadyPromise, getDiagnostics } = await setupPage(page, url, abortController, {
      width: opts.width,
      height: opts.height,
      allowLocalhost: opts.allowLocalhost,
      timeoutMs: opts.timeoutMs,
    });

    if (opts.beforeNavigate) {
      await opts.beforeNavigate(page);
    }

    try {
      console.log(`Navigating to ${url}...`);
      await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 60000 });

      console.log('Waiting for visualization to signal readiness...');
      await renderReadyPromise;

      console.log('Taking screenshot...');
      const screenshotBuffer = await page.screenshot();

      const encode = opts.encode ?? (raw => sharp(raw).avif({ quality: 70, effort: 7 }).toBuffer());
      const encoded = await encode(screenshotBuffer);

      console.log('Successfully captured + encoded screenshot.');

      return encoded;
    } catch (err) {
      const diags = getDiagnostics();
      const base = err instanceof Error ? err.message : String(err);
      const enriched = new Error(diags.length ? `${base}\n\nBrowser diagnostics:\n${diags.join('\n')}` : base);
      (enriched as Error & { diagnostics?: string[] }).diagnostics = diags;
      throw enriched;
    }
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

const jsonBodyParser = express.json({ limit: '50mb' });

app.post('/render_transient', jsonBodyParser, async (req: Request, res: Response) => {
  const body = req.body as TransientRenderBody | undefined;
  if (!body || typeof body !== 'object') {
    return res.status(400).send('Invalid JSON body');
  }
  const tree = body.tree;
  const metadata = body.metadata ?? {};
  if (tree !== undefined && (tree === null || typeof tree !== 'object')) {
    return res.status(400).send('`tree` must be an object');
  }
  const options = body.options ?? {};
  const format: 'png' | 'avif' | 'jpeg' = options.format ?? 'png';
  const width = options.width ?? 800;
  const height = options.height ?? 800;
  const quality = options.quality;
  const dev = !!options.dev;
  const timeoutMs =
    typeof options.timeoutMs === 'number' && options.timeoutMs > 0
      ? Math.min(options.timeoutMs, 10 * 60 * 1000)
      : undefined;

  if (width < 16 || width > 4096 || height < 16 || height > 4096) {
    return res.status(400).send('width/height must be in [16, 4096]');
  }

  const encode = (raw: Buffer): Promise<Buffer> => {
    switch (format) {
      case 'png':
        return sharp(raw).png({ compressionLevel: 8 }).toBuffer();
      case 'avif':
        return sharp(raw).avif({ quality: quality ?? 70, effort: 6 }).toBuffer();
      case 'jpeg':
        return sharp(raw).jpeg({ quality: quality ?? 90, mozjpeg: true }).toBuffer();
    }
  };
  const contentType = format === 'jpeg' ? 'image/jpeg' : `image/${format}`;

  const url = dev ? DEV_TRANSIENT_URL + '?render=true' : PROD_TRANSIENT_URL + '?render=true';
  const abortController = new AbortController();
  // Only abort on premature client disconnect — `req.on('close')` fires for normal completion too
  // once the body parser drains the stream, which would cancel before the render even starts.
  res.on('close', () => {
    if (!res.writableEnded) abortController.abort();
  });

  try {
    const payload = { tree, metadata, materialOverride: options.materialOverride };
    const image = await render(url, abortController, {
      encode,
      width,
      height,
      allowLocalhost: dev,
      timeoutMs,
      beforeNavigate: async page => {
        await page.evaluateOnNewDocument((p: unknown) => {
          (window as any).__transientCompositionPayload = p;
        }, payload);
      },
    });
    res.set('Content-Type', contentType);
    res.send(image);
  } catch (error) {
    console.error('Transient render failed:', error);
    res.status(500).type('text/plain').send(error instanceof Error ? error.message : 'Render failed');
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
