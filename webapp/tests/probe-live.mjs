// Live-mode probe: getUserMedia is patched in-page to return a synthetic mic
// stream built from a benchmark wav (clip + 2.5 s of silence, looped), which
// drives the full real path — AudioWorklet capture, VAD, partial decodes,
// finalize-on-silence. Chrome's --use-file-for-fake-audio-capture is silent
// in new headless, so we bring our own fake mic.
// Usage: node tests/probe-live.mjs [wav]
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import puppeteer from 'puppeteer-core';

const here = dirname(fileURLToPath(import.meta.url));
const CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const WAV = process.argv[2] ?? join(here, 'fixtures/e2e/002_visualears_worst269_004.wav');
const wavB64 = readFileSync(WAV).toString('base64');

const browser = await puppeteer.launch({
  executablePath: CHROME,
  headless: true,
  userDataDir: '/tmp/ve-e2e-profile',
  protocolTimeout: 10 * 60 * 1000,
  args: ['--no-first-run', '--enable-unsafe-webgpu', '--use-angle=metal', '--enable-gpu'],
});

try {
  const page = await browser.newPage();
  page.setDefaultTimeout(10 * 60 * 1000);
  page.on('console', (msg) => {
    const t = msg.text();
    if (t.includes('[engine]') || t.includes('[live]')) console.log(`[page] ${t}`);
  });
  await page.evaluateOnNewDocument((b64) => {
    localStorage.removeItem('ve-web-stage');
    // Synthetic mic: decode the wav, append 2.5 s of silence, loop it into a
    // MediaStreamDestination, and hand that stream to anyone calling
    // getUserMedia({audio}).
    const makeStream = async () => {
      const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
      const ctx = new AudioContext();
      await ctx.resume();
      const clip = await ctx.decodeAudioData(bytes.buffer.slice(0));
      const total = ctx.sampleRate * (clip.duration + 2.5);
      const looped = ctx.createBuffer(1, Math.ceil(total), ctx.sampleRate);
      // resample-copy the clip into the front of the loop buffer
      const off = new OfflineAudioContext(1, Math.ceil(clip.duration * ctx.sampleRate), ctx.sampleRate);
      const s0 = off.createBufferSource();
      s0.buffer = clip;
      s0.connect(off.destination);
      s0.start();
      const rendered = await off.startRendering();
      looped.getChannelData(0).set(rendered.getChannelData(0));
      const dest = ctx.createMediaStreamDestination();
      const src = ctx.createBufferSource();
      src.buffer = looped;
      src.loop = true;
      src.connect(dest);
      src.start();
      return dest.stream;
    };
    navigator.mediaDevices.getUserMedia = (constraints) => {
      if (constraints && constraints.audio) return makeStream();
      return Promise.reject(new Error('fake getUserMedia: audio only'));
    };
  }, wavB64);
  await page.goto('http://localhost:5199', { waitUntil: 'domcontentloaded' });

  await page.waitForFunction(`document.body.innerText.includes('چی‌کار کنیم؟')`, { polling: 300 });
  await page.evaluate(() => {
    [...document.querySelectorAll('h3')].find((e) => e.textContent.includes('زیرنویس زنده'))?.click();
  });
  try {
    await page.waitForFunction(`document.body.innerText.includes('گوینده')`, {
      polling: 500,
      timeout: 90_000,
    });
    await new Promise((r) => setTimeout(r, 4000));
    const text = await page.evaluate(() => document.body.innerText);
    const lines = text.split('\n').filter((l) => /[؀-ۿ]/.test(l) && l.length > 3);
    console.log('[probe-live] transcript lines:');
    for (const l of lines.slice(-6)) console.log('   ', l);
    await page.screenshot({ path: join(here, 'shots/3-live.png') });
    console.log('[probe-live] PASS — live mode committed an utterance (shots/3-live.png updated)');
  } catch {
    const text = await page.evaluate(() => document.body.innerText);
    console.error('[probe-live] FAIL — no utterance in 90 s. Screen text:');
    console.error(text.split('\n').filter(Boolean).slice(0, 16).join(' | '));
    process.exitCode = 1;
  }
} finally {
  await browser.close();
}
