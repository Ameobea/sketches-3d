// yes this is 100% vibecoded

import express from 'express';
import puppeteer from 'puppeteer';
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

async function setupPage(page, url) {
  await page.setViewport({ width: 600, height: 600 });

  const renderReadyPromise = new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error('Render timed out after 30 minutes')), 30 * 60 * 1000);
    page.exposeFunction('onRenderReady', () => {
      clearTimeout(timeout);
      resolve();
    });
  });

  await page.setRequestInterception(true);

  page.on('request', request => {
    const requestUrlString = request.url();
    let requestUrl;

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

async function render(url) {
  console.log('Launching browser...');
  const browser = await puppeteer.launch({ args: BROWSER_ARGS });

  try {
    const page = await browser.newPage();
    const { promise: renderReadyPromise } = await setupPage(page, url);

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

app.get('/render/:id', async (req, res) => {
  const sceneId = req.params.id;
  const adminToken = req.query.admin_token;
  const versionID = req.query.version_id;

  if (!/^[0-9]+$/.test(sceneId)) {
    return res.status(400).send('Invalid scene ID');
  }

  try {
    const baseUrl = 'https://3d.ameo.design/geotoy/edit/';
    const url = `${baseUrl}${sceneId}?render=true&admin_token=${adminToken}&version_id=${versionID}`;
    const screenshot = await render(url);
    res.set('Content-Type', 'image/avif');
    res.send(screenshot);
  } catch (error) {
    console.error(error);
    res.status(500).send('Error generating screenshot');
  }
});

app.get('/render_material/:id', async (req, res) => {
  const materialId = req.params.id;
  const adminToken = req.query.admin_token;

  if (!/^[0-9]+$/.test(materialId)) {
    return res.status(400).send('Invalid material ID');
  }

  try {
    const baseUrl = 'https://3d.ameo.design/geotoy/material/preview/';
    const url = `${baseUrl}${materialId}?render=true&admin_token=${adminToken}`;
    const screenshot = await render(url);
    res.set('Content-Type', 'image/avif');
    res.send(screenshot);
  } catch (error) {
    console.error(error);
    res.status(500).send('Error generating screenshot');
  }
});

const imageBodyParser = express.raw({ type: 'image/*', limit: '100mb' });

app.post('/thumbnail', imageBodyParser, async (req, res) => {
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

app.post('/convert-to-avif', imageBodyParser, async (req, res) => {
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
    console.error('Error converting image to AVIF:', error);
    res.status(500).send(`Error converting image to AVIF: ${error.message}`);
  }
});

app.listen(port, () => {
  console.log(`Screenshot service listening at http://localhost:${port}`);
});
