/** Runtime capability probes — work in both window and worker scopes.
 *  The loader checklist shows these for real; the provider ladder consumes
 *  them silently (webgpu → wasm-simd → wasm-nosimd compat). */

export interface Capabilities {
  simd: boolean;
  threads: boolean;
  maxThreads: number;
  webgpu: boolean;
}

// Minimal wasm module using a v128 op — validates only where SIMD exists.
const SIMD_PROBE = new Uint8Array([
  0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
  0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7b,
  0x03, 0x02, 0x01, 0x00,
  0x0a, 0x0a, 0x01, 0x08, 0x00, 0x41, 0x00, 0xfd, 0x0f, 0xfd, 0x62, 0x0b,
]);

export function detectSimd(): boolean {
  try {
    return typeof WebAssembly !== 'undefined' && WebAssembly.validate(SIMD_PROBE);
  } catch {
    return false;
  }
}

export function detectThreads(): boolean {
  return (
    typeof Atomics !== 'undefined' &&
    typeof SharedArrayBuffer !== 'undefined' &&
    globalThis.crossOriginIsolated === true
  );
}

export function maxThreads(): number {
  const hc = (globalThis.navigator && navigator.hardwareConcurrency) || 1;
  return Math.max(1, Math.min(8, hc));
}

export async function detectWebGpu(): Promise<boolean> {
  const gpu = (globalThis.navigator as Navigator & { gpu?: { requestAdapter(): Promise<unknown> } })
    ?.gpu;
  if (!gpu) return false;
  try {
    return (await gpu.requestAdapter()) != null;
  } catch {
    return false;
  }
}

export async function detectCapabilities(): Promise<Capabilities> {
  return {
    simd: detectSimd(),
    threads: detectThreads(),
    maxThreads: maxThreads(),
    webgpu: await detectWebGpu(),
  };
}
