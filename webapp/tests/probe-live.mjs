// Live-mode probe: getUserMedia is patched in-page to return a synthetic mic
// stream built from a benchmark wav (clip + 2.5 s of silence, looped), which
// drives the full real path — AudioWorklet capture, VAD, partial decodes,
// finalize-on-silence. Chrome's --use-file-for-fake-audio-capture is silent
// in new headless, so we bring our own fake mic.
// Usage: node tests/probe-live.mjs [--gain 0.06] [wav]
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import puppeteer from 'puppeteer-core';

const here = dirname(fileURLToPath(import.meta.url));
const CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const args = process.argv.slice(2);
const gainIdx = args.indexOf('--gain');
// gain 1 ≈ a normal mic; ~0.06 simulates a quiet/distant mic (RMS ≈ 0.008).
const GAIN = gainIdx >= 0 ? Number(args[gainIdx + 1]) : 1;
const positional = args.filter((a, i) => a !== '--gain' && (gainIdx < 0 || i !== gainIdx + 1));
const WAV = positional[0] ?? join(here, 'fixtures/e2e/002_visualears_worst269_004.wav');
const wavB64 = readFileSync(WAV).toString('base64');
console.log(`[probe-live] wav=${WAV.split('/').pop()} gain=${GAIN}`);

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
  await page.evaluateOnNewDocument((b64, gain) => {
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
      const samples = rendered.getChannelData(0);
      if (gain !== 1) for (let i = 0; i < samples.length; i++) samples[i] *= gain;
      looped.getChannelData(0).set(samples);
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
  }, wavB64, GAIN);
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
    // Let partials, the 18 s force-commit, and silence finalization play out.
    const settleMs = WAV.includes('golha') ? 40_000 : 6_000;
    await new Promise((r) => setTimeout(r, settleMs));
    const text = await page.evaluate(() => document.body.innerText);
    const lines = text.split('\n').filter((l) => /[؀-ۿ]/.test(l) && l.trim().length > 3);
    console.log('[probe-live] transcript lines:');
    for (const l of lines.slice(-8)) console.log('   ', l);
    await page.screenshot({ path: join(here, 'shots/3-live.png') });

    const chrome = ['مدل', 'زیرنویس', 'میکروفون', 'سیستم', 'گوینده', 'ساکته'];
    const spoken = lines.filter((l) => !chrome.some((c) => l.includes(c))).join(' ');
    if (WAV.includes('golha')) {
      const gold = readFileSync(join(here, 'fixtures/e2e/golha_clear.txt'), 'utf8');
      const goldWords = [...new Set(gold.split(/\s+/).filter((w) => w.length > 2))];
      const hit = goldWords.filter((w) => spoken.includes(w));
      const overlap = hit.length / goldWords.length;
      console.log(
        `[probe-live] committed text: "${spoken.slice(0, 160)}${spoken.length > 160 ? '…' : ''}"`,
      );
      console.log(
        `[probe-live] gold-word overlap: ${hit.length}/${goldWords.length} (${(overlap * 100).toFixed(0)}%)`,
      );
      if (overlap < 0.3) {
        console.error('[probe-live] FAIL — words are not forming from live audio');
        process.exitCode = 1;
      } else {
        console.log('[probe-live] PASS — live captions carry the spoken words');
      }
    } else if (!/[ا-ی]/.test(spoken)) {
      // Persian LETTERS required — timestamps alone (Persian digits) don't count.
      console.error('[probe-live] FAIL — utterance committed but no Persian words visible');
      process.exitCode = 1;
    } else {
      console.log('[probe-live] PASS — live mode committed Persian text (shots/3-live.png updated)');
    }
  } catch {
    const text = await page.evaluate(() => document.body.innerText);
    console.error('[probe-live] FAIL — no utterance in 90 s. Screen text:');
    console.error(text.split('\n').filter(Boolean).slice(0, 16).join(' | '));
    process.exitCode = 1;
  }
} finally {
  await browser.close();
}
