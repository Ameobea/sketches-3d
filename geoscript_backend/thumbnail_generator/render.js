// yes this is 100% vibecoded

import express from 'express';
import puppeteer from 'puppeteer';
import sharp from 'sharp';

const app = express();
const port = 5812;

// This is the main function that renders a screenshot of a webpage
async function renderScreenshot(sceneId, adminToken, versionID) {
  console.log('Launching browser...');
  const browser = await puppeteer.launch({
    // These flags are essential for running in a GPU-less Docker container
    args: [
      '--no-sandbox',
      '--disable-setuid-sandbox',
      '--disable-dev-shm-usage',
      '--use-gl=swiftshader',
      '--enable-webgl',
      '--ignore-gpu-blacklist',
    ],
  });

  try {
    const page = await browser.newPage();
    await page.setViewport({ width: 600, height: 600 });

    const baseUrl = 'https://3d.ameo.design/geotoy/edit/';
    // const baseUrl = 'http://localhost:4800/geotoy/edit/';
    const url = `${baseUrl}${sceneId}?render=true&admin_token=${adminToken}&version_id=${versionID}`;

    const renderReadyPromise = new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error('Render timed out after 60 seconds')), 60000);
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

  // Basic validation to ensure the ID is numeric
  if (!/^[0-9]+$/.test(sceneId)) {
    return res.status(400).send('Invalid scene ID');
  }

  try {
    const screenshot = await renderScreenshot(sceneId, adminToken, versionID);
    res.set('Content-Type', 'image/avif');
    res.send(screenshot);
  } catch (error) {
    console.error(error);
    res.status(500).send('Error generating screenshot');
  }
});

app.listen(port, () => {
  console.log(`Screenshot service listening at http://localhost:${port}`);
});
