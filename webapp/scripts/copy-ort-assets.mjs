// Copies the ORT 1.18 compat runtime into public/ort/compat/ so the no-SIMD
// fallback tier is fully same-origin (zero CDN). The modern tiers use the
// onnxruntime-web bundle build, which carries its JS glue inline and resolves
// its .wasm next to itself — no copies needed.
import { copyFileSync, mkdirSync, readdirSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
const webapp = join(root, '..');

const sets = [
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
