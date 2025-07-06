// yes this is 100% vibecoded

import express from 'express';
import puppeteer from 'puppeteer';

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

  const page = await browser.newPage();

  const baseUrl = 'https://3d.ameo.design/geotoy/edit/';
  const url = `${baseUrl}${sceneId}?render=true&admin_token=${adminToken}&version_id=${versionID}`;

  console.log(`Navigating to ${url}...`);
  await page.goto(url, { waitUntil: 'networkidle0', timeout: 60000 });

  console.log('Taking screenshot...');
  const screenshotBuffer = await page.screenshot();

  console.log('Closing browser...');
  await browser.close();

  return screenshotBuffer;
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
    res.set('Content-Type', 'image/png');
    res.send(screenshot);
  } catch (error) {
    console.error(error);
    res.status(500).send('Error generating screenshot');
  }
});

app.listen(port, () => {
  console.log(`Screenshot service listening at http://localhost:${port}`);
});
