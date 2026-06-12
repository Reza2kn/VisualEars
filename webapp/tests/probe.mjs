// Decode-latency probe: loads the app (model should already be cached from the
// E2E run), then times engine.decode() on synthetic audio of a few lengths.
// Also reports WebGPU adapter visibility on the main thread vs in a worker.
// Usage: node tests/probe.mjs [--engine wasm|compat] [--base http://localhost:5199]
import puppeteer from 'puppeteer-core';

const CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const args = process.argv.slice(2);
const flag = (name) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : undefined;
};
const engineTier = flag('engine');
const base = flag('base') ?? 'http://localhost:5199';
const url = engineTier ? `${base}/?engine=${engineTier}` : base;

const browser = await puppeteer.launch({
  executablePath: CHROME,
  headless: true,
  userDataDir: '/tmp/ve-e2e-profile',
  protocolTimeout: 30 * 60 * 1000,
  args: ['--no-first-run', '--enable-unsafe-webgpu', '--use-angle=metal', '--enable-gpu'],
});

try {
  const page = await browser.newPage();
  page.setDefaultTimeout(30 * 60 * 1000);
  page.on('console', (msg) => {
    const t = msg.text();
    if (t.includes('[engine]')) console.log(`[page] ${t}`);
  });
  await page.goto(url, { waitUntil: 'domcontentloaded' });

  const gpuMain = await page.evaluate(async () => {
    if (!navigator.gpu) return 'no navigator.gpu';
    try {
      const a = await navigator.gpu.requestAdapter();
      return a ? 'adapter ok' : 'no adapter';
    } catch (e) {
      return `error: ${e.message}`;
    }
  });
  const gpuWorker = await page.evaluate(async () => {
    const code = `
      (async () => {
        if (!navigator.gpu) { postMessage('no navigator.gpu'); return; }
        try { const a = await navigator.gpu.requestAdapter(); postMessage(a ? 'adapter ok' : 'no adapter'); }
        catch (e) { postMessage('error: ' + e.message); }
      })();`;
    const blob = new Blob([code], { type: 'text/javascript' });
    const w = new Worker(URL.createObjectURL(blob));
    return new Promise((res) => {
      w.onmessage = (e) => res(e.data);
      setTimeout(() => res('timeout'), 5000);
    });
  });
  console.log(`[probe] WebGPU main: ${gpuMain} | worker: ${gpuWorker}`);

  console.log('[probe] loading engine (cached model)…');
  const report = await page.evaluate(async () => {
    const engine = window.__veEngine;
    if (!engine) throw new Error('__veEngine hook missing (dev build only)');
    const t0 = performance.now();
    await engine.ensureLoaded();
    const loadMs = performance.now() - t0;
    const out = { loadMs, provider: engine.getState().provider, runs: [] };
    for (const seconds of [1, 5, 20]) {
      const n = Math.round(seconds * 16000);
      const pcm = new Float32Array(n);
      for (let i = 0; i < n; i++) {
        pcm[i] = 0.25 * Math.sin((2 * Math.PI * 220 * i) / 16000) + 0.001 * Math.sin(i * 1.7);
      }
      const t1 = performance.now();
      const r = await engine.decode(pcm);
      out.runs.push({
        seconds,
        totalMs: Math.round(performance.now() - t1),
        inferMs: Math.round(r.stats.inferMs),
        preprocessMs: Math.round(r.stats.preprocessMs),
        rtf: Number(r.stats.rtf.toFixed(3)),
        textLen: r.text.length,
      });
    }
    return out;
  });
  console.log('[probe]', JSON.stringify(report, null, 1));
} finally {
  await browser.close();
}
