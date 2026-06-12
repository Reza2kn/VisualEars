// Screenshot pass for the fidelity review + live-mode smoke test.
//   - loader (fresh profile, no cached model)
//   - mode picker (cached profile)
//   - live transcription fed by a fake mic playing a benchmark wav
//   - media done view for an audio file
// Usage: node tests/shots.mjs   (dev server must be running)
import { mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import puppeteer from 'puppeteer-core';

const here = dirname(fileURLToPath(import.meta.url));
const CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const base = 'http://localhost:5199';
const outDir = join(here, 'shots');
mkdirSync(outDir, { recursive: true });

const WAV = join(here, 'fixtures/e2e/002_visualears_worst269_004.wav');
const textIncludes = (needle) => `document.body.innerText.includes(${JSON.stringify(needle)})`;

async function clickByText(page, selector, needle) {
  await page.evaluate(
    (sel, text) => {
      const el = [...document.querySelectorAll(sel)].find((e) => e.textContent.includes(text));
      if (el) el.click();
    },
    selector,
    needle,
  );
}

async function withBrowser(opts, fn) {
  const browser = await puppeteer.launch({
    executablePath: CHROME,
    headless: true,
    protocolTimeout: 10 * 60 * 1000,
    ...opts,
  });
  try {
    const page = await browser.newPage();
    await page.setViewport({ width: 1280, height: 800 });
    page.setDefaultTimeout(10 * 60 * 1000);
    await fn(page);
  } finally {
    await browser.close();
  }
}

// 1. Loader on a fresh profile (no cache → loader shows).
await withBrowser({ userDataDir: '/tmp/ve-shot-fresh-' + Date.now() }, async (page) => {
  await page.goto(base, { waitUntil: 'networkidle2' });
  await page.waitForFunction(textIncludes('دانلود و بارگذاری مدل'));
  await new Promise((r) => setTimeout(r, 600)); // fonts settle
  await page.screenshot({ path: join(outDir, '1-loader.png') });
  console.log('[shots] 1-loader.png');
});

// 2–4 on the cached profile.
await withBrowser(
  {
    userDataDir: '/tmp/ve-e2e-profile',
    args: [
      '--no-first-run',
      '--enable-unsafe-webgpu',
      '--use-angle=metal',
      '--enable-gpu',
      '--use-fake-ui-for-media-stream',
      '--use-fake-device-for-media-stream',
      `--use-file-for-fake-audio-capture=${WAV}`,
      '--autoplay-policy=no-user-gesture-required',
    ],
  },
  async (page) => {
    await page.evaluateOnNewDocument(() => localStorage.removeItem('ve-web-stage'));
    await page.goto(base, { waitUntil: 'domcontentloaded' });
    await page.waitForFunction(textIncludes('چی‌کار کنیم؟'));
    await new Promise((r) => setTimeout(r, 700));
    await page.screenshot({ path: join(outDir, '2-mode.png') });
    console.log('[shots] 2-mode.png');

    // Live: fake mic loops the benchmark wav — wait for transcript lines.
    await clickByText(page, 'h3', 'زیرنویس زنده');
    await page.waitForFunction(textIncludes('زیرنویس زنده'));
    try {
      await page.waitForFunction(
        `document.body.innerText.includes('گوینده')`,
        { timeout: 4 * 60 * 1000 },
      );
    } catch {
      console.log('[shots] live: no utterance appeared (timeout) — capturing anyway');
    }
    await new Promise((r) => setTimeout(r, 1200));
    await page.screenshot({ path: join(outDir, '3-live.png') });
    const liveText = await page.evaluate(() => document.body.innerText);
    console.log('[shots] 3-live.png — live screen text sample:');
    console.log(
      liveText
        .split('\n')
        .filter((l) => l.trim())
        .slice(0, 14)
        .map((l) => '    ' + l)
        .join('\n'),
    );

    // Media done view (audio file → transcript-only layout).
    await clickByText(page, 'button[aria-label="بازگشت"]', '');
    await page.waitForFunction(textIncludes('چی‌کار کنیم؟'));
    await clickByText(page, 'h3', 'زیرنویس رسانه');
    await page.waitForFunction(textIncludes('فایل رو بنداز اینجا'));
    const input = await page.$('input[type=file]');
    await input.uploadFile(WAV);
    await page.waitForFunction(
      `document.querySelectorAll('[data-testid="segment-text"]').length > 0`,
    );
    await new Promise((r) => setTimeout(r, 500));
    await page.screenshot({ path: join(outDir, '4-media-done.png') });
    console.log('[shots] 4-media-done.png');
  },
);
console.log('[shots] complete');
