/** Cache Storage wrapper for the model binaries: streamed download with real
 *  progress, cached after first fetch so reloads are instant and offline.
 *  Available in both window and worker scopes. */

const CACHE_NAME = 've-models-v1';

export type ProgressFn = (loadedBytes: number) => void;

export async function isCached(urls: string[]): Promise<boolean> {
  if (!('caches' in globalThis)) return false;
  try {
    const cache = await caches.open(CACHE_NAME);
    for (const url of urls) {
      if (!(await cache.match(url))) return false;
    }
    return true;
  } catch {
    return false;
  }
}

/** Fetch a (possibly cached) binary, reporting cumulative bytes via onProgress. */
export async function fetchWithCache(
  url: string,
  expectedBytes: number,
  onProgress: ProgressFn,
): Promise<Uint8Array> {
  let cache: Cache | null = null;
  try {
    cache = await caches.open(CACHE_NAME);
    const hit = await cache.match(url);
    if (hit) {
      const buf = new Uint8Array(await hit.arrayBuffer());
      onProgress(buf.byteLength);
      return buf;
    }
  } catch {
    cache = null; // private mode / quota issues — stream without caching
  }

  const res = await fetch(url);
  if (!res.ok) throw new Error(`HTTP ${res.status} for ${url}`);

  const total = Number(res.headers.get('Content-Length')) || expectedBytes;
  if (!res.body) {
    const buf = new Uint8Array(await res.arrayBuffer());
    onProgress(buf.byteLength);
    if (cache) await putSafely(cache, url, buf);
    return buf;
  }

  const out = new Uint8Array(total > 0 ? total : 0);
  let received = 0;
  let chunks: Uint8Array[] | null = total > 0 ? null : [];
  const reader = res.body.getReader();
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    if (chunks) {
      chunks.push(value);
    } else if (received + value.byteLength <= out.byteLength) {
      out.set(value, received);
    } else {
      // server lied about Content-Length — fall back to chunk list
      chunks = [out.subarray(0, received).slice(), value];
    }
    received += value.byteLength;
    onProgress(value.byteLength);
  }

  let buf: Uint8Array;
  if (chunks) {
    buf = new Uint8Array(received);
    let off = 0;
    for (const c of chunks) {
      buf.set(c, off);
      off += c.byteLength;
    }
  } else {
    buf = received === out.byteLength ? out : out.subarray(0, received).slice();
  }

  if (cache) await putSafely(cache, url, buf);
  return buf;
}

async function putSafely(cache: Cache, url: string, buf: Uint8Array): Promise<void> {
  try {
    await cache.put(
      url,
      new Response(buf.slice().buffer, {
        headers: { 'Content-Type': 'application/octet-stream', 'Content-Length': String(buf.byteLength) },
      }),
    );
  } catch {
    // quota exceeded — keep going without persistence
  }
}
