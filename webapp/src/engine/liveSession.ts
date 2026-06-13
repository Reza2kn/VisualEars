/** Live transcription session: AudioWorklet capture → 16 kHz mono → RMS speech
 *  gate → utterance state machine (rolling ≤20 s window, partial decodes on an
 *  adaptive cadence, finalize after 900 ms of silence). Gate and segmentation
 *  thresholds mirror the validated Space demo (speech gate 0.006), with the
 *  gate only adapting *upward* when ambient noise clearly exceeds it.
 *
 *  The capture context runs at the device's native rate (forcing 16 kHz makes
 *  Chrome's tab/display capture deliver silence on some platforms); chunks are
 *  resampled to 16 kHz here. */

import { engine } from './engine';
import { MAX_SAMPLES, SAMPLE_RATE } from './features';
import { punctuate } from './punctuate';
import { fa } from '../fa';

export type LiveSource = 'mic' | 'sys';

export interface Utterance {
  tStart: number;
  speakerId: number;
  text: string;
}

export interface LiveSnapshot {
  utterances: Utterance[];
  /** In-progress utterance text (updates in place until it commits). */
  partial: Utterance | null;
  running: boolean;
  /** 0–1 recent levels for the visualizer (oldest → newest). */
  levels: Float32Array;
  /** True when capture is live but nothing audible has arrived for a while —
   *  the UI shows a warm "I can't hear anything" hint. */
  noSignal: boolean;
  error: string | null;
}

const LEVEL_BARS = 26;
// Space demo defaults: speech gate 0.006, ≤350 ms of trailing silence kept,
// finalize after 900 ms of silence, ≥0.4 s before the first partial decode.
const BASE_GATE = 0.006;
const MAX_GATE = 0.04;
const TRAILING_SILENCE_MS = 350;
const FINALIZE_SILENCE_MS = 900;
const MIN_PARTIAL_SECONDS = 0.4;
const MIN_FINAL_SECONDS = 0.25;
/** Continuous audio (e.g. tab playback) may never pause 900 ms — commit the
 *  utterance anyway before the rolling window starts dropping its start. */
const FORCE_COMMIT_SECONDS = 18;
const NO_SIGNAL_AFTER_MS = 5000;
const NO_SIGNAL_PEAK = 0.0015;

function resampleLinear(input: Float32Array, fromRate: number, toRate: number): Float32Array {
  if (fromRate === toRate) return input;
  const outLen = Math.max(1, Math.round((input.length * toRate) / fromRate));
  const output = new Float32Array(outLen);
  const ratio = (input.length - 1) / Math.max(1, outLen - 1);
  for (let i = 0; i < outLen; i++) {
    const x = i * ratio;
    const j = Math.floor(x);
    const frac = x - j;
    output[i] = input[j] * (1 - frac) + input[Math.min(j + 1, input.length - 1)] * frac;
  }
  return output;
}

export class LiveSession {
  private snapshot: LiveSnapshot = {
    utterances: [],
    partial: null,
    running: false,
    levels: new Float32Array(LEVEL_BARS),
    noSignal: false,
    error: null,
  };
  private readonly onChange: () => void;

  private ctx: AudioContext | null = null;
  private mediaStream: MediaStream | null = null;
  private node: AudioWorkletNode | null = null;

  private utterancePcm = new Float32Array(0);
  private utteranceStartSec = 0;
  private sessionSec = 0;
  private speechActive = false;
  private silenceMs = 0;
  private finalizing = false;
  private decoding = false;
  private lastDecodeMs = 1500;
  private decodeTimer: ReturnType<typeof setTimeout> | null = null;
  private finalizeTimer: ReturnType<typeof setTimeout> | null = null;
  private noiseFloor = 0;
  private quietMs = 0;
  private peakSinceStart = 0;
  private levelRing = new Float32Array(LEVEL_BARS);
  private lastLevelEmit = 0;
  /** Bumped on every start/stop; async steps bail out when it changes
   *  (guards against StrictMode double-mounts and rapid source switches). */
  private generation = 0;

  constructor(onChange: () => void) {
    this.onChange = onChange;
  }

  getSnapshot = (): LiveSnapshot => this.snapshot;

  private update(partial: Partial<LiveSnapshot>): void {
    this.snapshot = { ...this.snapshot, ...partial };
    this.onChange();
  }

  async start(source: LiveSource): Promise<void> {
    await this.stop();
    const gen = ++this.generation;
    this.update({ error: null, noSignal: false });
    try {
      await engine.ensureLoaded();
      if (gen !== this.generation) return;
      const stream = await this.captureStream(source);
      if (gen !== this.generation) {
        stream.getTracks().forEach((t) => t.stop());
        return;
      }
      this.mediaStream = stream;

      // Native-rate context — see header comment. Resampling happens per chunk.
      const ctx = new AudioContext();
      this.ctx = ctx;
      if (ctx.state === 'suspended') await ctx.resume().catch(() => undefined);
      await ctx.audioWorklet.addModule('/capture-worklet.js');
      if (gen !== this.generation) {
        await ctx.close().catch(() => undefined);
        stream.getTracks().forEach((t) => t.stop());
        return;
      }
      const sourceNode = ctx.createMediaStreamSource(stream);
      const node = new AudioWorkletNode(ctx, 've-capture', {
        numberOfInputs: 1,
        numberOfOutputs: 1,
        outputChannelCount: [1],
      });
      this.node = node;
      sourceNode.connect(node);
      node.connect(ctx.destination); // worklet outputs silence; keeps the graph pulled
      node.port.onmessage = (event: MessageEvent<Float32Array>) => {
        if (gen === this.generation) this.handleChunk(event.data, ctx.sampleRate);
      };

      const track = stream.getAudioTracks()[0];
      track?.addEventListener('ended', () => {
        if (gen === this.generation) void this.stop();
      });
      console.info(
        '[live] capture started —',
        source,
        '| track:',
        track?.label || '(unnamed)',
        '| track rate:',
        track?.getSettings?.().sampleRate ?? '?',
        '| context rate:',
        ctx.sampleRate,
        '| context state:',
        ctx.state,
      );

      this.peakSinceStart = 0;
      this.quietMs = 0;
      this.update({ running: true });
      this.scheduleDecode();
    } catch (err) {
      const message = this.describeError(err, source);
      console.warn('[live] capture failed:', err);
      await this.stop();
      this.update({ error: message });
    }
  }

  private async captureStream(source: LiveSource): Promise<MediaStream> {
    if (source === 'mic') {
      return navigator.mediaDevices.getUserMedia({
        audio: {
          channelCount: 1,
          echoCancellation: true,
          noiseSuppression: true,
          autoGainControl: true,
        },
      });
    }
    if (!navigator.mediaDevices.getDisplayMedia) {
      throw new Error('display-media-unsupported');
    }
    const stream = await navigator.mediaDevices.getDisplayMedia({ audio: true, video: true });
    if (stream.getAudioTracks().length === 0) {
      stream.getTracks().forEach((t) => t.stop());
      throw new Error('display-media-no-audio');
    }
    return stream;
  }

  private describeError(err: unknown, source: LiveSource): string {
    const message = err instanceof Error ? err.message : String(err);
    if (message === 'display-media-unsupported') return fa.live.systemUnsupported;
    if (message === 'display-media-no-audio') return fa.live.noSystemAudio;
    if (source === 'mic') return fa.live.micDenied;
    return fa.live.noSystemAudio;
  }

  /** Pause capture without tearing the session down. */
  async pause(): Promise<void> {
    if (this.ctx && this.ctx.state === 'running') await this.ctx.suspend();
    if (this.decodeTimer) clearTimeout(this.decodeTimer);
    this.decodeTimer = null;
    this.update({ running: false });
  }

  async resume(): Promise<void> {
    if (this.ctx && this.ctx.state === 'suspended') await this.ctx.resume();
    this.update({ running: true });
    this.scheduleDecode();
  }

  async stop(): Promise<void> {
    this.generation++;
    if (this.decodeTimer) clearTimeout(this.decodeTimer);
    if (this.finalizeTimer) clearTimeout(this.finalizeTimer);
    this.decodeTimer = null;
    this.finalizeTimer = null;
    this.node?.port.close();
    this.node?.disconnect();
    this.node = null;
    this.mediaStream?.getTracks().forEach((t) => t.stop());
    this.mediaStream = null;
    if (this.ctx) {
      await this.ctx.close().catch(() => undefined);
      this.ctx = null;
    }
    this.speechActive = false;
    this.silenceMs = 0;
    this.finalizing = false;
    this.utterancePcm = new Float32Array(0);
    this.noiseFloor = 0;
    this.quietMs = 0;
    this.levelRing.fill(0);
    this.update({ running: false, noSignal: false, levels: new Float32Array(LEVEL_BARS) });
  }

  /** Speech gate: the proven default, raised only when ambient noise clearly
   *  sits above it (capped so loud system audio can't gate itself out). */
  private currentGate(): number {
    return Math.min(MAX_GATE, Math.max(BASE_GATE, this.noiseFloor * 2.5));
  }

  private handleChunk(raw: Float32Array, fromRate: number): void {
    if (!this.snapshot.running) return;
    const chunk = resampleLinear(raw, fromRate, SAMPLE_RATE);
    const chunkMs = (chunk.length / SAMPLE_RATE) * 1000;
    this.sessionSec += chunkMs / 1000;

    let rms = 0;
    for (let i = 0; i < chunk.length; i++) rms += chunk[i] * chunk[i];
    rms = Math.sqrt(rms / chunk.length);

    this.pushLevel(rms);
    this.trackSignal(rms, chunkMs);

    const speechy = rms >= this.currentGate();
    if (!speechy) {
      // Slow EMA of the ambient floor; rises gently so speech pauses in noisy
      // rooms lift the gate, falls faster when things quiet down.
      this.noiseFloor =
        rms > this.noiseFloor
          ? this.noiseFloor * 0.98 + rms * 0.02
          : this.noiseFloor * 0.95 + rms * 0.05;
    }

    if (speechy) {
      if (!this.speechActive) {
        this.utterancePcm = new Float32Array(0);
        this.utteranceStartSec = Math.max(0, this.sessionSec - chunkMs / 1000);
        this.speechActive = true;
        this.silenceMs = 0;
        this.finalizing = false;
        this.update({ partial: { tStart: this.utteranceStartSec, speakerId: 0, text: '' } });
      }
      this.silenceMs = 0;
      this.appendToUtterance(chunk);
      if (
        this.utterancePcm.length >= SAMPLE_RATE * FORCE_COMMIT_SECONDS &&
        !this.finalizing
      ) {
        this.finalizing = true;
        this.finalizeUtterance();
      }
      return;
    }

    if (!this.speechActive) return;
    this.silenceMs += chunkMs;
    if (this.silenceMs <= TRAILING_SILENCE_MS) this.appendToUtterance(chunk);
    if (this.silenceMs >= FINALIZE_SILENCE_MS && !this.finalizing) {
      this.finalizing = true;
      this.finalizeUtterance();
    }
  }

  /** Detect dead capture (live but everything ~zero) for the UI hint. */
  private trackSignal(rms: number, chunkMs: number): void {
    this.peakSinceStart = Math.max(this.peakSinceStart, rms);
    if (rms < NO_SIGNAL_PEAK) {
      this.quietMs += chunkMs;
    } else {
      this.quietMs = 0;
      if (this.snapshot.noSignal) this.update({ noSignal: false });
    }
    if (this.quietMs >= NO_SIGNAL_AFTER_MS && !this.snapshot.noSignal) {
      console.warn(
        `[live] no audible signal for ${Math.round(this.quietMs / 1000)} s ` +
          `(peak RMS so far ${this.peakSinceStart.toFixed(5)})`,
      );
      this.update({ noSignal: true });
    }
  }

  private pushLevel(rms: number): void {
    this.levelRing.copyWithin(0, 1);
    this.levelRing[LEVEL_BARS - 1] = Math.max(0.06, Math.min(1, rms * 14));
    const now = performance.now();
    if (now - this.lastLevelEmit > 90) {
      this.lastLevelEmit = now;
      this.update({ levels: this.levelRing.slice() });
    }
  }

  private appendToUtterance(chunk: Float32Array): void {
    const merged = new Float32Array(Math.min(MAX_SAMPLES, this.utterancePcm.length + chunk.length));
    const keep = Math.max(0, merged.length - chunk.length);
    if (keep > 0) merged.set(this.utterancePcm.subarray(this.utterancePcm.length - keep), 0);
    merged.set(chunk.subarray(Math.max(0, chunk.length - merged.length)), keep);
    this.utterancePcm = merged;
  }

  private scheduleDecode(): void {
    if (this.decodeTimer) clearTimeout(this.decodeTimer);
    if (!this.snapshot.running) return;
    const delay = Math.max(1200, this.lastDecodeMs * 1.3);
    this.decodeTimer = setTimeout(() => {
      void this.runPartialDecode().finally(() => this.scheduleDecode());
    }, delay);
  }

  private async runPartialDecode(): Promise<void> {
    if (
      this.decoding ||
      this.finalizing ||
      !this.speechActive ||
      this.utterancePcm.length < SAMPLE_RATE * MIN_PARTIAL_SECONDS
    ) {
      return;
    }
    await this.decodeUtterance(false);
  }

  private finalizeUtterance(): void {
    if (this.decoding) {
      this.finalizeTimer = setTimeout(() => this.finalizeUtterance(), 150);
      return;
    }
    if (this.utterancePcm.length < SAMPLE_RATE * MIN_FINAL_SECONDS) {
      this.speechActive = false;
      this.finalizing = false;
      this.update({ partial: null });
      return;
    }
    void this.decodeUtterance(true);
  }

  private async decodeUtterance(final: boolean): Promise<void> {
    this.decoding = true;
    try {
      const pcm = this.utterancePcm.slice();
      const capturedLen = pcm.length;
      const outcome = await engine.decode(pcm);
      this.lastDecodeMs = outcome.stats.totalMs;
      if (final) {
        const text = punctuate(outcome.words, { isFinal: true });
        const committed = text
          ? [...this.snapshot.utterances, { tStart: this.utteranceStartSec, speakerId: 0, text }]
          : this.snapshot.utterances;

        // Speech that arrived while the final decode ran (forced commits on
        // continuous audio) seeds the next utterance instead of being lost.
        const tail =
          this.snapshot.running &&
          this.speechActive &&
          this.silenceMs < FINALIZE_SILENCE_MS &&
          this.utterancePcm.length > capturedLen
            ? this.utterancePcm.slice(capturedLen)
            : null;

        this.finalizing = false;
        if (tail && tail.length > 0) {
          this.utterancePcm = tail;
          this.utteranceStartSec = Math.max(0, this.sessionSec - tail.length / SAMPLE_RATE);
          this.silenceMs = 0;
          this.update({
            utterances: committed,
            partial: { tStart: this.utteranceStartSec, speakerId: 0, text: '' },
          });
        } else {
          this.speechActive = false;
          this.silenceMs = 0;
          this.utterancePcm = new Float32Array(0);
          this.update({ utterances: committed, partial: null });
        }
      } else if (this.speechActive) {
        this.update({
          partial: {
            tStart: this.utteranceStartSec,
            speakerId: 0,
            text: punctuate(outcome.words, { isFinal: false }),
          },
        });
      }
    } catch (err) {
      console.warn('[live] decode failed:', err);
      if (final) {
        this.speechActive = false;
        this.finalizing = false;
        this.update({ partial: null });
      }
    } finally {
      this.decoding = false;
    }
  }
}
