/// <reference lib="webworker" />
/** Engine worker — owns the modern ORT tiers of the silent provider ladder:
 *    1. WebGPU (jsep)            — external-data pair
 *    2. WASM + SIMD (+threads)   — external-data pair
 *  When neither tier is possible (no SIMD on this device, or session creation
 *  failed), it reports `need-compat` and the main thread silently falls back
 *  to the ORT 1.18 + embedded-model path. Decoding (mel → fp16 → CTC) happens
 *  here so the UI thread never blocks. */

import * as ort from 'onnxruntime-web/webgpu';
import { detectCapabilities, type Capabilities } from './capabilities';
import { fetchWithCache } from './modelCache';
import { FeatureExtractor, FIXED_FRAMES, N_MELS, OUTPUT_STRIDE, SAMPLE_RATE } from './features';
import { float32ArrayToFloat16Bits, fp16TensorData, logitsNumericView } from './fp16';
import { decodeCtcGreedy, TOKENS } from './ctc';
import type { FromWorker, ToWorker, Provider } from './protocol';

const post = (msg: FromWorker) => (self as unknown as Worker).postMessage(msg);

let session: ort.InferenceSession | null = null;
let provider: Provider | null = null;
const extractor = new FeatureExtractor();

async function load(msg: Extract<ToWorker, { type: 'load' }>): Promise<void> {
  const caps: Capabilities = await detectCapabilities();
  if (msg.forceTier === 'wasm') caps.webgpu = false;
  post({ type: 'caps', caps });

  if (!caps.webgpu && !caps.simd) {
    // Modern ORT ships SIMD-only WASM binaries — this device needs the compat tier.
    post({ type: 'need-compat', reason: 'no WebGPU and no WASM SIMD' });
    return;
  }

  post({ type: 'phase', phase: 'download' });
  const onProgress = (bytes: number) => post({ type: 'progress', receivedBytes: bytes });
  const graph = await fetchWithCache(msg.graphUrl, msg.graphBytes, onProgress);
  const data = await fetchWithCache(msg.dataUrl, msg.dataBytes, onProgress);

  post({ type: 'phase', phase: 'init' });
  // No wasmPaths override: the bundle build ships its JS glue inline and
  // resolves its .wasm next to itself (node_modules in dev, hashed asset in
  // prod) — same-origin either way, and nothing for Vite to mis-import.
  const threads = caps.threads ? caps.maxThreads : 1;
  ort.env.wasm.numThreads = threads;

  const baseOptions: ort.InferenceSession.SessionOptions = {
    graphOptimizationLevel: 'all',
    enableMemPattern: false,
    enableCpuMemArena: true,
    externalData: [{ data, path: msg.dataPath }],
  };

  if (caps.webgpu) {
    try {
      session = await ort.InferenceSession.create(graph, {
        ...baseOptions,
        executionProviders: ['webgpu'],
      });
      provider = 'webgpu';
      post({ type: 'ready', provider, threads });
      return;
    } catch (err) {
      console.warn('[engine] WebGPU session failed, falling back to WASM:', err);
    }
  }

  if (caps.simd) {
    try {
      session = await ort.InferenceSession.create(graph, {
        ...baseOptions,
        executionProviders: ['wasm'],
      });
      provider = caps.threads ? 'wasm-simd-threaded' : 'wasm-simd';
      post({ type: 'ready', provider, threads });
      return;
    } catch (err) {
      console.warn('[engine] WASM SIMD session failed:', err);
      post({ type: 'need-compat', reason: String((err as Error)?.message ?? err) });
      return;
    }
  }

  post({ type: 'need-compat', reason: 'WebGPU session failed and SIMD is unavailable' });
}

async function decode(msg: Extract<ToWorker, { type: 'decode' }>): Promise<void> {
  if (!session) {
    post({ type: 'decode-error', id: msg.id, message: 'engine not loaded' });
    return;
  }
  try {
    const started = performance.now();
    const { features, frameCount } = extractor.compute(msg.pcm);
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
    const dims = logits.dims as readonly number[];
    const vocabSize = dims[2] || TOKENS.length;
    const usableSteps = msg.fullSteps
      ? dims[1]
      : Math.max(1, Math.min(dims[1], Math.ceil(frameCount / OUTPUT_STRIDE)));
    const view = logitsNumericView(logits.data, logits.type);
    const { text, firstStep, lastStep } = decodeCtcGreedy(
      view.values,
      usableSteps,
      vocabSize,
      view.type,
    );
    const totalMs = performance.now() - started;
    const audioSeconds = msg.pcm.length / SAMPLE_RATE;
    post({
      type: 'result',
      id: msg.id,
      outcome: {
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
      },
    });
  } catch (err) {
    post({ type: 'decode-error', id: msg.id, message: String((err as Error)?.message ?? err) });
  }
}

self.onmessage = (event: MessageEvent<ToWorker>) => {
  const msg = event.data;
  if (msg.type === 'load') {
    void load(msg).catch((err) =>
      post({ type: 'need-compat', reason: String((err as Error)?.message ?? err) }),
    );
  } else if (msg.type === 'decode') {
    void decode(msg);
  }
};
