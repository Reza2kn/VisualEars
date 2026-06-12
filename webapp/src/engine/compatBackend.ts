/** No-SIMD compat tier — the last rung of the silent provider ladder.
 *  Loads the ORT 1.18 UMD build (the final release shipping non-SIMD WASM
 *  binaries, self-hosted under /ort/compat/) on the main thread and runs the
 *  single-file embedded fp16 model so older devices still work. */

import { fetchWithCache, type ProgressFn } from './modelCache';
import { FeatureExtractor, FIXED_FRAMES, N_MELS, OUTPUT_STRIDE, SAMPLE_RATE } from './features';
import { float32ArrayToFloat16Bits, fp16TensorData, logitsNumericView } from './fp16';
import { decodeCtcGreedy, TOKENS } from './ctc';
import type { DecodeOutcome } from './protocol';

// Minimal surface of the ORT 1.18 UMD global we rely on.
interface OrtCompat {
  env: { wasm: { wasmPaths: string; simd: boolean; numThreads: number } };
  Tensor: new (type: string, data: Uint16Array, dims: number[]) => unknown;
  InferenceSession: {
    create(
      model: Uint8Array,
      options: Record<string, unknown>,
    ): Promise<{
      run(feeds: Record<string, unknown>): Promise<
        Record<string, { data: ArrayLike<number>; dims: readonly number[]; type: string }>
      >;
    }>;
  };
}

declare global {
  interface Window {
    ort?: OrtCompat;
  }
}

function loadCompatScript(): Promise<OrtCompat> {
  return new Promise((resolve, reject) => {
    if (window.ort) {
      resolve(window.ort);
      return;
    }
    const script = document.createElement('script');
    script.id = 've-ort-compat';
    script.src = '/ort/compat/ort.wasm.min.js';
    script.async = true;
    script.onload = () => {
      if (window.ort) resolve(window.ort);
      else reject(new Error('compat ONNX Runtime did not initialize'));
    };
    script.onerror = () => reject(new Error('compat ONNX Runtime script failed to load'));
    document.head.appendChild(script);
  });
}

export interface CompatEngine {
  decode(pcm: Float32Array): Promise<DecodeOutcome>;
}

export async function createCompatEngine(
  embeddedUrl: string,
  embeddedBytes: number,
  onProgress: ProgressFn,
  onPhase: (phase: 'download' | 'init') => void,
): Promise<CompatEngine> {
  onPhase('download');
  const model = await fetchWithCache(embeddedUrl, embeddedBytes, onProgress);

  onPhase('init');
  const ort = await loadCompatScript();
  ort.env.wasm.wasmPaths = '/ort/compat/';
  ort.env.wasm.simd = false;
  ort.env.wasm.numThreads = 1;
  const session = await ort.InferenceSession.create(model, {
    executionProviders: ['wasm'],
    graphOptimizationLevel: 'all',
    enableMemPattern: false,
    enableCpuMemArena: true,
  });

  const extractor = new FeatureExtractor();

  return {
    async decode(pcm: Float32Array): Promise<DecodeOutcome> {
      const started = performance.now();
      const { features, frameCount } = extractor.compute(pcm);
      const packed = float32ArrayToFloat16Bits(features);
      const tensor = new ort.Tensor(
        'float16',
        fp16TensorData(packed) as unknown as Uint16Array,
        [1, N_MELS, FIXED_FRAMES],
      );
      const inferStarted = performance.now();
      const output = await session.run({ processed_signal: tensor });
      const inferMs = performance.now() - inferStarted;
      const logits = output['logits'];
      const dims = logits.dims;
      const vocabSize = dims[2] || TOKENS.length;
      const usableSteps = Math.max(1, Math.min(dims[1], Math.ceil(frameCount / OUTPUT_STRIDE)));
      const view = logitsNumericView(logits.data, logits.type);
      const { text, firstStep, lastStep } = decodeCtcGreedy(
        view.values,
        usableSteps,
        vocabSize,
        view.type,
      );
      const totalMs = performance.now() - started;
      const audioSeconds = pcm.length / SAMPLE_RATE;
      return {
        text,
        firstStep,
        lastStep,
        stats: {
          preprocessMs: inferStarted - started,
          inferMs,
          totalMs,
          audioSeconds,
          rtf: totalMs / 1000 / Math.max(0.001, audioSeconds),
          frames: frameCount,
          steps: usableSteps,
        },
      };
    },
  };
}
