// Loads the built web game (web/dist) in Chrome with WebGPU and checks that every
// shader/pipeline validates and the first game frame is presented.
// Usage: node test.mjs   (env: PORT, CHROME_PATH, TIMEOUT_MS, HEADED=1)
import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../..');
const port = Number(process.env.PORT ?? 8879);
const timeout = Number(process.env.TIMEOUT_MS ?? 240_000);
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

let browser;
let exitCode = 1;
try {
  const executablePath = process.env.CHROME_PATH
    ?? (process.platform === 'darwin' ? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' : undefined);
  browser = await chromium.launch({
    executablePath,
    channel: executablePath ? undefined : 'chrome',
    headless: !process.env.HEADED,
    args: [
      '--enable-unsafe-webgpu',
      '--enable-features=WebGPU,Vulkan',
      '--ignore-gpu-blocklist',
      '--use-angle=metal',
    ],
  });
  const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
  page.on('console', message => {
    const text = message.text();
    const type = message.type();
    if (process.env.VERBOSE) console.log(`[console.${type}] ${text}`);
    if (ignored.some(re => re.test(text))) return;
    if (type === 'error' || (type === 'warning' && interesting.test(text))) problems.push(`[console.${type}] ${text}`);
  });
  page.on('pageerror', error => problems.push(`[pageerror] ${error.message}`));

  await page.goto(url, { waitUntil: 'load' });
  if (!(await page.evaluate(() => !!navigator.gpu))) throw new Error('navigator.gpu is unavailable in this Chrome');

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
  if (state === 'error') {
    const message = await page.locator('#loading-message').textContent();
    problems.push(`[startup] ${message}`);
  }
  // Let a few more frames run so late pipeline/validation errors surface.
  await page.waitForTimeout(2000);

  const canvasSize = await page.evaluate(() => {
    const c = document.getElementById('game');
    return [c.width, c.height];
  });
  if (!(canvasSize[0] > 0 && canvasSize[1] > 0)) problems.push(`[canvas] bad size ${canvasSize}`);

  await page.screenshot({ path: resolve(here, 'first-frame.png') });
  if (problems.length) {
    console.error(`FAIL: ${problems.length} problem(s)`);
    for (const p of [...new Set(problems)].slice(0, 30)) console.error('  ' + p);
  } else {
    console.log('OK: shaders compiled and first frame rendered (tests/web/first-frame.png)');
    exitCode = 0;
  }
} catch (error) {
  console.error('FAIL:', error.message);
  for (const p of [...new Set(problems)].slice(0, 30)) console.error('  ' + p);
} finally {
  await browser?.close();
  server.kill();
}
process.exit(exitCode);
