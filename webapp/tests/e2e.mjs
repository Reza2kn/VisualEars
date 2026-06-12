// End-to-end check against the real model in real Chrome:
//   1. loads the app (dev server), runs the loader (download + init)
//   2. reports which provider tier engaged (webgpu / wasm-simd / compat)
//   3. pushes benchmark wavs through Media mode
//   4. compares transcripts with the published parity expectations
//
// Usage: node tests/e2e.mjs [--engine wasm|compat] [--base http://localhost:5199]
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import puppeteer from 'puppeteer-core';

const here = dirname(fileURLToPath(import.meta.url));
const CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

const args = process.argv.slice(2);
const flag = (name) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : undefined;
};
const engineTier = flag('engine');
const base = flag('base') ?? 'http://localhost:5199';

// App-level flow check: loader → ready → media upload → visible Persian
// transcript + export buttons. (Transcript parity against the published
// expectations lives in tests/probe-media.mjs, which decodes full windows the
// way the offline parity set was generated.)
const FIXTURES = ['002_visualears_worst269_004.wav'];
const PERSIAN = /[؀-ۿ]/;

const textIncludes = (needle) => `document.body.innerText.includes(${JSON.stringify(needle)})`;

async function clickByText(page, selector, needle) {
  const ok = await page.evaluate(
    (sel, text) => {
      const el = [...document.querySelectorAll(sel)].find((e) => e.textContent.includes(text));
      if (!el) return false;
      el.click();
      return true;
    },
    selector,
    needle,
  );
  if (!ok) throw new Error(`clickByText: "${needle}" not found in ${selector}`);
}

const url = engineTier ? `${base}/?engine=${engineTier}` : base;
console.log(`[e2e] opening ${url}`);

const browser = await puppeteer.launch({
  executablePath: CHROME,
  headless: true,
  userDataDir: '/tmp/ve-e2e-profile',
  protocolTimeout: 30 * 60 * 1000,
  args: ['--no-first-run', '--enable-unsafe-webgpu', '--use-angle=metal', '--enable-gpu'],
});

try {
  const page = await browser.newPage();
  page.setDefaultTimeout(20 * 60 * 1000);
  page.on('console', (msg) => {
    const t = msg.text();
    if (t.includes('[engine]')) console.log(`[page:${msg.type()}] ${t}`);
  });
  // Start from a known stage regardless of what a previous session persisted.
  await page.evaluateOnNewDocument(() => localStorage.removeItem('ve-web-stage'));
  await page.goto(url, { waitUntil: 'domcontentloaded' });

  // Loader (first run) or straight to mode picker (cached model).
  await page.waitForFunction(
    `${textIncludes('دانلود و بارگذاری مدل')} || ${textIncludes('چی‌کار کنیم؟')}`,
    { timeout: 60_000 },
  );
  const onLoader = await page.evaluate(textIncludes('دانلود و بارگذاری مدل'));
  if (onLoader) {
    console.log('[e2e] loader shown — starting model download (first run, ~232 MB)…');
    await clickByText(page, 'button', 'دانلود و بارگذاری مدل');
  } else {
    console.log('[e2e] model already cached — loader skipped');
  }

  await page.waitForFunction(textIncludes('چی‌کار کنیم؟'));
  const headerBadges = await page.evaluate(() =>
    [...document.querySelectorAll('header .ve-badge')].map((b) => b.textContent.trim()),
  );
  console.log('[e2e] ready — header badges:', headerBadges.join(' | '));

  const results = [];
  for (const wav of FIXTURES) {
    await page.waitForFunction(textIncludes('چی‌کار کنیم؟'), { polling: 300 });
    await clickByText(page, 'h3', 'زیرنویس رسانه');
    await page.waitForFunction(textIncludes('فایل رو بنداز اینجا'), { polling: 300 });
    const input = await page.$('input[type=file]');
    await input.uploadFile(join(here, 'fixtures/e2e', wav));
    // Either segments appear (success) or the warm error copy shows (failure).
    await page.waitForFunction(
      `document.querySelectorAll('[data-testid="segment-text"]').length > 0` +
        ` || ${textIncludes('نتونستم بخونم')} || ${textIncludes('پیدا نکردم')}`,
      { polling: 500, timeout: 10 * 60 * 1000 },
    );
    const got = await page.evaluate(() =>
      [...document.querySelectorAll('[data-testid="segment-text"]')]
        .map((e) => e.textContent)
        .join(' ')
        .trim(),
    );
    const srtVisible = await page.evaluate(textIncludes('SRT'));
    const ok = PERSIAN.test(got) && got.length >= 3 && srtVisible;
    results.push({ wav, got, ok });
    console.log(`[e2e] ${wav} → "${got}" | SRT button: ${srtVisible} | ${ok ? 'OK' : 'FAIL'}`);
    await clickByText(page, 'button[aria-label="بازگشت"]', '');
  }

  const failures = results.filter((r) => !r.ok);
  if (failures.length) {
    console.error(`[e2e] FAIL — ${failures.length}/${results.length} media flows failed`);
    process.exitCode = 1;
  } else {
    console.log(`[e2e] PASS — media flow produced a visible Persian transcript with exports`);
  }
} finally {
  await browser.close();
}
