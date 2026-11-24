// Loads the built web game (web/dist) in Chrome with WebGPU and checks that every
// shader/pipeline validates and the first game frame is presented.
// Usage: node test.mjs
// Env: BACKEND=webgpu|webgl, BROWSER=chromium|webkit, MOBILE=1,
// WEBGPU_FAILURE=missing|null|rejected (WebGL), PORT, CHROME_PATH, TIMEOUT_MS, HEADED=1.
import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium, webkit } from 'playwright';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../..');
const port = Number(process.env.PORT ?? 8879);
const timeout = Number(process.env.TIMEOUT_MS ?? 240_000);
const backend = process.env.BACKEND ?? 'webgpu';
const browserName = process.env.BROWSER ?? 'chromium';
const gpuFailure = process.env.WEBGPU_FAILURE ?? 'missing';
if (!['missing', 'null', 'rejected'].includes(gpuFailure)) throw new Error('Unknown WEBGPU_FAILURE');
if (!['chromium', 'webkit'].includes(browserName)) throw new Error('BROWSER must be chromium or webkit');
if (!['webgpu', 'webgl'].includes(backend)) throw new Error('BACKEND must be webgpu or webgl');
const url = `http://127.0.0.1:${port}/game/`;

if (!existsSync(resolve(repo, 'web/dist/pkg/mtl_jobines.js'))) {
  console.error('web/dist is missing: run `make build` first (or use `make test-web`).');
  process.exit(2);
}

const server = spawn('node', [resolve(repo, 'scripts/serve-web.mjs'), String(port)], {
  stdio: ['ignore', 'ignore', 'inherit'],
});

// Chrome reports WebGPU validation problems (bad WGSL, invalid pipelines, lost
// devices, ...) as console errors/warnings and uncaught page errors.
const problems = [];
const interesting = /error|invalid|wgsl|shader|validation|device.*lost|panick/i;
const ignored = [/favicon/i, /GPU stall due to ReadPixels/i, /GroupMarkerNotSet/i];
const progress = text => { if (process.env.VERBOSE) console.log(text); };

let browser;
let page;
let exitCode = 1;
try {
  const executablePath = process.env.CHROME_PATH
    ?? (process.platform === 'darwin' ? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' : undefined);
  browser = browserName === 'webkit' ? await webkit.launch({ headless: !process.env.HEADED }) : await chromium.launch({
    executablePath,
    channel: executablePath ? undefined : 'chrome',
    headless: !process.env.HEADED,
    args: [
      '--enable-unsafe-webgpu',
      '--enable-features=WebGPU',
      '--ignore-gpu-blocklist',
      '--use-angle=metal',
    ],
  });
  progress('Browser launched');
  page = await browser.newPage(process.env.MOBILE
    ? { viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, deviceScaleFactor: 3 }
    : { viewport: { width: 1280, height: 720 } });
  if (backend === 'webgl') {
    // Exercise automatic selection, exactly as when Safari exposes no WebGPU.
    await page.addInitScript(failure => Object.defineProperty(Navigator.prototype, 'gpu', {
      get: () => failure === 'missing' ? undefined : {
        requestAdapter: () => failure === 'null' ? Promise.resolve(null) : Promise.reject(new Error('GPU unavailable')),
      },
    }), gpuFailure);
  }
  page.on('console', message => {
    const text = message.text();
    const type = message.type();
    if (process.env.VERBOSE) console.log(`[console.${type}] ${text}`);
    if (ignored.some(re => re.test(text))) return;
    if (type === 'error' || (type === 'warning' && interesting.test(text))) problems.push(`[console.${type}] ${text}`);
  });
  page.on('pageerror', error => problems.push(`[pageerror] ${error.message}`));
  page.on('response', response => {
    if (response.status() >= 400) problems.push(`[http ${response.status()}] ${response.url()}`);
  });

  await page.goto(url, { waitUntil: 'load' });
  progress('Page loaded');
  if ((await page.evaluate(() => !!navigator.gpu)) !== (backend === 'webgpu' || gpuFailure !== 'missing')) throw new Error(`Unexpected GPU availability for ${backend}`);

  // web_status() sets data-state="ready" once the first frame is presented,
  // or "error" if startup failed.
  const handle = await page.waitForFunction(
    () => {
      const state = document.getElementById('loading')?.dataset.state;
      return state === 'ready' || state === 'error' ? state : false;
    },
    undefined,
    { timeout, polling: 250 },
  );
  const state = await handle.jsonValue();
  progress(`Startup state: ${state}`);
  if (state === 'ready' && await page.locator('#game').getAttribute('data-renderer') !== backend) {
    problems.push(`[backend] expected ${backend}`);
  }
  if (state === 'error') {
    const message = await page.locator('#loading-message').textContent();
    problems.push(`[startup] ${message}`);
  }
  // Let a few more frames run so late pipeline/validation errors surface.
  await page.waitForTimeout(2000);
  if (state === 'ready') {
    await page.locator('#game').focus();
    // Enter play, skip the first-run intro, and dismiss the help screen.
    for (let phase = 0; phase < 3; phase++) {
      await page.keyboard.press('Enter');
      await page.waitForTimeout(500);
    }
  }
  if (process.env.MOBILE && state === 'ready') {
    await page.locator('#mobile-controls').waitFor({ state: 'visible' });
    progress('Mobile controls visible');
    await page.locator('#mobile-primary').tap();
    progress('Portrait touch complete');
    await page.setViewportSize({ width: 844, height: 390 });
    progress('Viewport rotated');
    await page.locator('#mobile-primary').tap();
    await page.waitForTimeout(1000);
  }

  const canvasSize = await page.evaluate(() => {
    const c = document.getElementById('game');
    return [c.width, c.height];
  });
  if (!(canvasSize[0] > 0 && canvasSize[1] > 0)) problems.push(`[canvas] bad size ${canvasSize}`);

  const screenshot = `first-frame-${browserName}-${backend}${process.env.MOBILE ? '-mobile' : ''}.png`;
  await page.screenshot({ path: resolve(here, screenshot) });
  if (problems.length) {
    console.error(`FAIL: ${problems.length} problem(s)`);
    for (const p of [...new Set(problems)].slice(0, 30)) console.error('  ' + p);
  } else {
    console.log(`OK: ${browserName}/${backend} shaders compiled and first frame rendered (tests/web/${screenshot})`);
    exitCode = 0;
  }
} catch (error) {
  console.error('FAIL:', error.message);
  if (page) {
    console.error('[page state]', await page.evaluate(() => ({
      loading: document.getElementById('loading')?.dataset.state,
      mobileMode: document.getElementById('game')?.dataset.mobileMode,
      activeElement: document.activeElement?.id,
      viewport: [innerWidth, innerHeight],
    })).catch(() => 'unavailable'));
    await page.screenshot({ path: resolve(here, `first-frame-${browserName}-${backend}-failure.png`), timeout: 3000 }).catch(() => {});
  }
  for (const p of [...new Set(problems)].slice(0, 30)) console.error('  ' + p);
} finally {
  await browser?.close();
  server.kill();
}
process.exit(exitCode);
