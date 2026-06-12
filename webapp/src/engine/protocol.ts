/** Message protocol between the engine facade (main thread) and engine.worker. */

import type { Capabilities } from './capabilities';
import type { LogitsType } from './fp16';

export type Provider = 'webgpu' | 'wasm-simd-threaded' | 'wasm-simd' | 'wasm-nosimd';

export interface DecodeStats {
  preprocessMs: number;
  inferMs: number;
  totalMs: number;
  audioSeconds: number;
  /** Real-time factor: processing time ÷ audio duration (lower is faster). */
  rtf: number;
  frames: number;
  steps: number;
}

export interface DecodeOutcome {
  text: string;
  firstStep: number;
  lastStep: number;
  stats: DecodeStats;
}

export type ToWorker =
  | {
      type: 'load';
      graphUrl: string;
      dataUrl: string;
      graphBytes: number;
      dataBytes: number;
      dataPath: string;
      /** Dev/testing override: skip the WebGPU tier. */
      forceTier?: 'wasm';
    }
  | { type: 'decode'; id: number; pcm: Float32Array };

export type FromWorker =
  | { type: 'caps'; caps: Capabilities }
  | { type: 'progress'; receivedBytes: number }
  | { type: 'phase'; phase: 'download' | 'init' }
  | { type: 'ready'; provider: Provider; threads: number }
  /** Worker tiers exhausted (no SIMD, or session creation failed) — main thread
   *  should silently fall back to the ORT 1.18 compat path + embedded model. */
  | { type: 'need-compat'; reason: string }
  | { type: 'result'; id: number; outcome: DecodeOutcome }
  | { type: 'decode-error'; id: number; message: string };

export interface LogitsResult {
  data: ArrayLike<number>;
  dims: readonly number[];
  type: LogitsType;
}
