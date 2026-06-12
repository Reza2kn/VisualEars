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

const FIXTURES = [
  '000_visualears_worst269_001.wav',
  '002_visualears_worst269_004.wav',
];
const expected = JSON.parse(readFileSync(join(here, 'fixtures/e2e/expected.json'), 'utf8'));

const normalize = (s) =>
  s
    .replaceAll('⁇', ' ')
    .replace(/\s+/g, ' ')
    .trim();

function similarity(a, b) {
  // normalized Levenshtein over chars
  const m = a.length;
  const n = b.length;
  if (!m && !n) return 1;
  let prev = Array.from({ length: n + 1 }, (_, j) => j);
  for (let i = 1; i <= m; i++) {
    const cur = [i];
    for (let j = 1; j <= n; j++) {
      cur[j] = Math.min(
        prev[j] + 1,
        cur[j - 1] + 1,
        prev[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1),
      );
    }
    prev = cur;
  }
  return 1 - prev[n] / Math.max(m, n);
}

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
  args: ['--no-first-run', '--enable-unsafe-webgpu', '--use-angle=metal', '--enable-gpu'],
});

try {
  const page = await browser.newPage();
  page.setDefaultTimeout(20 * 60 * 1000);
  page.on('console', (msg) => {
    const t = msg.text();
    if (t.includes('[engine]') || msg.type() === 'error') console.log(`[page:${msg.type()}] ${t}`);
  });
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
    await page.waitForFunction(textIncludes('چی‌کار کنیم؟'));
    await clickByText(page, 'h3', 'زیرنویس رسانه');
    await page.waitForFunction(textIncludes('فایل رو بنداز اینجا'));
    const input = await page.$('input[type=file]');
    await input.uploadFile(join(here, 'fixtures/e2e', wav));
    await page.waitForFunction(
      `document.querySelectorAll('[data-testid="segment-text"]').length > 0`,
    );
    const got = normalize(
      await page.evaluate(() =>
        [...document.querySelectorAll('[data-testid="segment-text"]')]
          .map((e) => e.textContent)
          .join(' '),
      ),
    );
    const want = normalize(expected[wav].baseline_norm);
    const sim = similarity(got, want);
    results.push({ wav, want, got, sim });
    console.log(`[e2e] ${wav}\n  expected: ${want}\n  got:      ${got}\n  similarity: ${sim.toFixed(3)}`);
    await clickByText(page, 'button[aria-label="بازگشت"]', '');
  }

  const failures = results.filter((r) => r.sim < 0.85);
  if (failures.length) {
    console.error(`[e2e] FAIL — ${failures.length}/${results.length} below 0.85 similarity`);
    process.exitCode = 1;
  } else {
    console.log(`[e2e] PASS — ${results.length}/${results.length} transcripts match (≥0.85)`);
  }
} finally {
  await browser.close();
}
