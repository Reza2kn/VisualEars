/** Media subtitling pipeline: decode any audio/video file the browser can
 *  read → 16 kHz mono PCM → silence-aware ≤20 s chunks → sequential decode →
 *  segments with CTC-step-refined timestamps. */

import { engine } from './engine';
import { HOP_LENGTH, MAX_SAMPLES, SAMPLE_RATE } from './features';
import { itn } from './itn';
import { punctuate, nativeText } from './punctuate';
import type { DecodeStats } from './protocol';

export interface Segment {
  tStart: number;
  tEnd: number;
  speakerId: number;
  text: string;
}

export interface MediaProgress {
  pct: number;
  stats: DecodeStats | null;
}

const STEP_SECONDS = 0.08; // one CTC output step = output_stride × hop = 80 ms
const VALLEY_SEARCH_FROM_SECONDS = 14; // prefer a silence valley in the last ~6 s of a window

export async function decodeFileToPcm(file: File): Promise<Float32Array> {
  const bytes = await file.arrayBuffer();
  const probe = new OfflineAudioContext(1, 1, SAMPLE_RATE);
  const decoded = await probe.decodeAudioData(bytes);
  const length = Math.max(1, Math.ceil(decoded.duration * SAMPLE_RATE));
  const resampler = new OfflineAudioContext(1, length, SAMPLE_RATE);
  const source = resampler.createBufferSource();
  source.buffer = decoded;
  source.connect(resampler.destination);
  source.start();
  const rendered = await resampler.startRendering();
  return rendered.getChannelData(0);
}

function hopEnergies(pcm: Float32Array): Float32Array {
  const hops = Math.ceil(pcm.length / HOP_LENGTH);
  const energies = new Float32Array(hops);
  for (let h = 0; h < hops; h++) {
    const start = h * HOP_LENGTH;
    const end = Math.min(pcm.length, start + HOP_LENGTH);
    let sum = 0;
    for (let i = start; i < end; i++) sum += pcm[i] * pcm[i];
    energies[h] = sum / Math.max(1, end - start);
  }
  return energies;
}

/** Pick the chunk end: the quietest hop inside the last stretch of the window,
 *  so cuts land in pauses instead of mid-word. */
function pickCut(energies: Float32Array, cursor: number, hardEnd: number): number {
  const fromHop = Math.floor((cursor + VALLEY_SEARCH_FROM_SECONDS * SAMPLE_RATE) / HOP_LENGTH);
  const toHop = Math.floor(hardEnd / HOP_LENGTH) - 1;
  if (toHop <= fromHop) return hardEnd;
  let bestHop = toHop;
  let bestEnergy = Infinity;
  for (let h = fromHop; h <= toHop; h++) {
    if (energies[h] < bestEnergy) {
      bestEnergy = energies[h];
      bestHop = h;
    }
  }
  return Math.min(hardEnd, bestHop * HOP_LENGTH);
}

export async function transcribePcm(
  pcm: Float32Array,
  onProgress: (progress: MediaProgress) => void,
): Promise<Segment[]> {
  await engine.ensureLoaded();
  const energies = hopEnergies(pcm);
  const segments: Segment[] = [];
  let cursor = 0;

  while (cursor < pcm.length) {
    const hardEnd = Math.min(cursor + MAX_SAMPLES, pcm.length);
    const end = hardEnd < pcm.length ? pickCut(energies, cursor, hardEnd) : hardEnd;
    const chunk = pcm.subarray(cursor, end).slice();

    // Skip stretches that are pure silence — no point burning a full decode.
    let peak = 0;
    for (let i = 0; i < chunk.length; i++) {
      const a = Math.abs(chunk[i]);
      if (a > peak) peak = a;
    }
    if (peak > 1e-4 && chunk.length >= SAMPLE_RATE * 0.2) {
      const outcome = await engine.decode(chunk);
      // Chunk boundaries land on silence valleys, so the end is sentence-final.
      const mv = engine.getState().variant;
      const text = mv.nativeFormatting === true
        ? (mv.spokenNumbers === true ? itn(nativeText(outcome.text)) : nativeText(outcome.text))
        : itn(punctuate(outcome.words, { isFinal: true }));
      if (text) {
        const base = cursor / SAMPLE_RATE;
        const tStart = base + Math.max(0, outcome.firstStep) * STEP_SECONDS;
        const tEnd = Math.min(
          end / SAMPLE_RATE,
          base + (outcome.lastStep + 1) * STEP_SECONDS,
        );
        segments.push({ tStart, tEnd: Math.max(tEnd, tStart + 0.4), speakerId: 0, text });
      }
      onProgress({ pct: (end / pcm.length) * 100, stats: outcome.stats });
    } else {
      onProgress({ pct: (end / pcm.length) * 100, stats: null });
    }
    cursor = end;
  }

  return segments;
}
