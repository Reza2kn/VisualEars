# VisualEars Web App — visualears.com

**چیزی که نمیشنوی رو ببین** — fully on-device Persian transcription in the browser.
React + Vite + TypeScript, `onnxruntime-web` running the in-house
**FastConformer-FA 115M** fp16 ONNX CTC core. No server inference, no account,
no tracking; after the one-time model download everything works offline.

Two functions:

- **زیرنویس زنده** — live transcription of the microphone or system/tab audio
- **زیرنویس رسانه** — drop a video/audio file → synced subtitles + timestamped
  transcript, exportable as SRT/VTT

## The silent provider ladder

The engine picks the fastest path available, with no user-facing choices:

1. **WebGPU** (`navigator.gpu` adapter) — external-data ONNX pair
2. **WASM + SIMD** (+threads when `crossOriginIsolated`) — same pair
3. **WASM no-SIMD compat** — pinned ORT 1.18 build (`onnxruntime-web-compat`
   npm alias) + the single-file embedded model, so older devices still work

All ORT runtime assets are self-hosted under `/ort/` (zero CDN), copied from
`node_modules` by `scripts/copy-ort-assets.mjs` (runs automatically on
`dev`/`build`). Model files come from Hugging Face in dev and from same-origin
`/models/` in production (`VITE_MODEL_BASE=/models`), cached in Cache Storage.

The feature pipeline (80-bin log-mel → fp16 `[1,80,2005]`) and CTC vocabulary
are the validated browser contract published as sidecars next to the models —
see `src/engine/preprocessor.json` and `src/engine/tokens.json`
(`tools/upload_sidecars.py` pushes them to the HF repos).

## Commands

```bash
npm install
npm run dev        # dev server with COOP/COEP headers (required for threads)
npm test           # vitest: golden log-mel parity, CTC, fp16, SRT/VTT
npm run build      # production build (set VITE_MODEL_BASE=/models for deploy)
node tests/e2e.mjs # real-model E2E in headless Chrome (needs `npm run dev` running)
```

`tests/e2e.mjs` drives the full flow — loader → model download → Media mode
with benchmark clips — and compares transcripts against the published parity
expectations. `?engine=wasm` / `?engine=compat` force the lower tiers for
testing (dev-only override).

## Deploy (Mac Studio behind Cloudflare DNS-only)

See `deploy/Caddyfile.example` — Caddy terminates TLS, sets the COOP/COEP
headers, and serves the app, the ORT runtime, and the model files from one
origin. Build with `VITE_MODEL_BASE=/models` so the loader never leaves the
origin.

## Design source of truth

The UI is a pixel-faithful build of the design bundle (tokens, components,
copy) — Persian-first, RTL-native (CSS logical properties only), Lalezar +
Vazirmatn (self-hosted), pill controls, warm espresso shadows, saffron focus
glow, Persian digits via `ss01`. All Persian copy uses the informal تو register
and lives in `src/fa.ts`.
