import { defineConfig, type Plugin } from 'vite';
import react from '@vitejs/plugin-react';

// SharedArrayBuffer (WASM threads) requires cross-origin isolation.
// Production must mirror these headers (see deploy/Caddyfile.example).
function crossOriginIsolation(): Plugin {
  const setHeaders = (res: import('http').ServerResponse) => {
    res.setHeader('Cross-Origin-Opener-Policy', 'same-origin');
    res.setHeader('Cross-Origin-Embedder-Policy', 'require-corp');
  };
  return {
    name: 'cross-origin-isolation',
    configureServer(server) {
      server.middlewares.use((_req, res, next) => {
        setHeaders(res);
        next();
      });
    },
    configurePreviewServer(server) {
      server.middlewares.use((_req, res, next) => {
        setHeaders(res);
        next();
      });
    },
  };
}

export default defineConfig({
  plugins: [react(), crossOriginIsolation()],
  worker: {
    format: 'es',
  },
  build: {
    target: 'es2022',
  },
  // onnxruntime-web resolves its .wasm/.mjs runtime from ort.env.wasm.wasmPaths
  // (self-hosted under /ort/, copied by scripts/copy-ort-assets.mjs) — keep the
  // npm packages out of the dependency pre-bundle so the worker import stays lazy.
  optimizeDeps: {
    exclude: ['onnxruntime-web', 'onnxruntime-web-compat'],
  },
});
