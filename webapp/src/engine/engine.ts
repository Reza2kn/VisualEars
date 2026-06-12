/** Engine facade — single instance shared by every screen. Owns the worker
 *  (modern tiers), the silent compat fallback, download progress, and decode
 *  serialization. React reads it through useEngine() (useSyncExternalStore). */

import { DEFAULT_VARIANT, fileUrl, type ModelVariant } from './manifest';
import { detectCapabilities, type Capabilities } from './capabilities';
import { isCached } from './modelCache';
import { createCompatEngine, type CompatEngine } from './compatBackend';
import type { DecodeOutcome, DecodeStats, FromWorker, Provider, ToWorker } from './protocol';

export type EngineStatus = 'idle' | 'downloading' | 'initializing' | 'ready' | 'error';

export interface EngineState {
  status: EngineStatus;
  variant: ModelVariant;
  loadedBytes: number;
  totalBytes: number;
  provider: Provider | null;
  threads: number;
  caps: Capabilities | null;
  lastStats: DecodeStats | null;
  error: string | null;
}

type Listener = () => void;

class Engine {
  private state: EngineState = {
    status: 'idle',
    variant: DEFAULT_VARIANT,
    loadedBytes: 0,
    totalBytes: DEFAULT_VARIANT.graph.bytes + DEFAULT_VARIANT.data.bytes,
    provider: null,
    threads: 1,
    caps: null,
    lastStats: null,
    error: null,
  };
  private listeners = new Set<Listener>();
  private worker: Worker | null = null;
  private compat: CompatEngine | null = null;
  private loadPromise: Promise<void> | null = null;
  private decodeChain: Promise<unknown> = Promise.resolve();
  private nextDecodeId = 1;
  private pendingDecodes = new Map<
    number,
    { resolve: (o: DecodeOutcome) => void; reject: (e: Error) => void }
  >();

  getState = (): EngineState => this.state;

  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  private update(partial: Partial<EngineState>): void {
    this.state = { ...this.state, ...partial };
    for (const l of this.listeners) l();
  }

  providerLabel(): string {
    return this.state.provider === 'webgpu' ? 'WebGPU' : 'WASM · CPU';
  }

  /** Probe capabilities for the loader checklist (no side effects). */
  async probeCaps(): Promise<Capabilities> {
    const caps = await detectCapabilities();
    this.update({ caps });
    return caps;
  }

  async isModelCached(): Promise<boolean> {
    const v = this.state.variant;
    const pair = await isCached([fileUrl(v.graph), fileUrl(v.data)]);
    if (pair) return true;
    return isCached([fileUrl(v.embedded)]);
  }

  /** Idempotent: kicks off (or joins) the silent load ladder. */
  ensureLoaded(): Promise<void> {
    if (!this.loadPromise) {
      this.loadPromise = this.load().catch((err: Error) => {
        this.loadPromise = null;
        this.update({ status: 'error', error: err.message });
        throw err;
      });
    }
    return this.loadPromise;
  }

  /** Dev/testing override via ?engine=wasm|compat — never set in normal use. */
  private forcedTier(): 'wasm' | 'compat' | undefined {
    if (typeof location === 'undefined') return undefined;
    const v = new URLSearchParams(location.search).get('engine');
    return v === 'wasm' || v === 'compat' ? v : undefined;
  }

  private load(): Promise<void> {
    const v = this.state.variant;
    const forced = this.forcedTier();
    this.update({
      status: 'downloading',
      loadedBytes: 0,
      totalBytes: v.graph.bytes + v.data.bytes,
      error: null,
    });

    return new Promise<void>((resolve, reject) => {
      const fallbackToCompat = (reason: string) => {
        console.warn('[engine] falling back to compat tier:', reason);
        this.worker?.terminate();
        this.worker = null;
        this.update({ status: 'downloading', loadedBytes: 0, totalBytes: v.embedded.bytes });
        createCompatEngine(
          fileUrl(v.embedded),
          v.embedded.bytes,
          (bytes) => this.update({ loadedBytes: this.state.loadedBytes + bytes }),
          (phase) =>
            this.update({ status: phase === 'download' ? 'downloading' : 'initializing' }),
        )
          .then((compat) => {
            this.compat = compat;
            this.update({ status: 'ready', provider: 'wasm-nosimd', threads: 1 });
            resolve();
          })
          .catch(reject);
      };

      if (forced === 'compat') {
        fallbackToCompat('forced via ?engine=compat');
        return;
      }

      const worker = new Worker(new URL('./engine.worker.ts', import.meta.url), {
        type: 'module',
      });
      this.worker = worker;

      worker.onerror = (event) => fallbackToCompat(event.message || 'worker error');
      worker.onmessage = (event: MessageEvent<FromWorker>) => {
        const msg = event.data;
        switch (msg.type) {
          case 'caps':
            this.update({ caps: msg.caps });
            break;
          case 'progress':
            this.update({ loadedBytes: this.state.loadedBytes + msg.receivedBytes });
            break;
          case 'phase':
            this.update({ status: msg.phase === 'download' ? 'downloading' : 'initializing' });
            break;
          case 'ready':
            console.info('[engine] ready —', msg.provider, `(${msg.threads} threads)`);
            this.update({ status: 'ready', provider: msg.provider, threads: msg.threads });
            resolve();
            break;
          case 'need-compat':
            fallbackToCompat(msg.reason);
            break;
          case 'result': {
            const pending = this.pendingDecodes.get(msg.id);
            if (pending) {
              this.pendingDecodes.delete(msg.id);
              this.update({ lastStats: msg.outcome.stats });
              pending.resolve(msg.outcome);
            }
            break;
          }
          case 'decode-error': {
            const pending = this.pendingDecodes.get(msg.id);
            if (pending) {
              this.pendingDecodes.delete(msg.id);
              pending.reject(new Error(msg.message));
            }
            break;
          }
        }
      };

      const loadMsg: ToWorker = {
        type: 'load',
        graphUrl: fileUrl(v.graph),
        dataUrl: fileUrl(v.data),
        graphBytes: v.graph.bytes,
        dataBytes: v.data.bytes,
        dataPath: v.data.name,
        forceTier: forced === 'wasm' ? 'wasm' : undefined,
      };
      worker.postMessage(loadMsg);
    });
  }

  /** Serialized decode of ≤ ~20 s of 16 kHz mono PCM. The buffer is transferred —
   *  pass a copy you will not reuse. */
  decode(pcm: Float32Array, opts?: { fullSteps?: boolean }): Promise<DecodeOutcome> {
    const run = async (): Promise<DecodeOutcome> => {
      await this.ensureLoaded();
      if (this.compat) {
        const outcome = await this.compat.decode(pcm);
        this.update({ lastStats: outcome.stats });
        return outcome;
      }
      const worker = this.worker;
      if (!worker) throw new Error('engine worker missing');
      const id = this.nextDecodeId++;
      return new Promise<DecodeOutcome>((resolve, reject) => {
        this.pendingDecodes.set(id, { resolve, reject });
        const msg: ToWorker = { type: 'decode', id, pcm, fullSteps: opts?.fullSteps };
        worker.postMessage(msg, [pcm.buffer]);
      });
    };
    const result = this.decodeChain.then(run, run);
    this.decodeChain = result.catch(() => undefined);
    return result;
  }
}

export const engine = new Engine();
