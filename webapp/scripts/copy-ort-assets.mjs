// Copies the onnxruntime-web runtime binaries into public/ort/ so the app is
// fully same-origin (zero CDN). Two sets:
//   public/ort/         — modern ORT (wasm SIMD+threads, WebGPU/JSEP)
//   public/ort/compat/  — ORT 1.18 (last release with non-SIMD wasm binaries),
//                         used by the silent no-SIMD fallback tier
import { copyFileSync, mkdirSync, readdirSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
const webapp = join(root, '..');

const sets = [
  {
    from: join(webapp, 'node_modules/onnxruntime-web/dist'),
    to: join(webapp, 'public/ort'),
    // every loader/binary variant the extern-wasm build may request at runtime
    // (plain CPU, jsep/webgpu, asyncify, jspi)
    match: (f) => /^ort-wasm-simd-threaded.*\.(wasm|mjs)$/.test(f),
  },
  {
    from: join(webapp, 'node_modules/onnxruntime-web-compat/dist'),
    to: join(webapp, 'public/ort/compat'),
    // UMD script consumed via <script> on the compat path + its wasm binaries
    match: (f) =>
      /^ort-wasm(-simd|-threaded|-simd-threaded)?\.wasm$/.test(f) || f === 'ort.wasm.min.js',
  },
];

for (const { from, to, match } of sets) {
  if (!existsSync(from)) {
    console.warn(`[copy-ort-assets] missing ${from} — run npm install first`);
    process.exitCode = 1;
    continue;
  }
  mkdirSync(to, { recursive: true });
  let n = 0;
  for (const f of readdirSync(from)) {
    if (match(f)) {
      copyFileSync(join(from, f), join(to, f));
      n++;
    }
  }
  console.log(`[copy-ort-assets] ${n} files → ${to}`);
}
