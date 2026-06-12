/** Model variant manifest — the loader renders whatever is listed here, so new
 *  artifacts (onnx-w4, onnx-fp, w2…) ship as config entries, not code changes.
 *
 *  Dev fetches straight from Hugging Face (CORS-safe). Production builds set
 *  VITE_MODEL_BASE (e.g. "/models") so everything stays same-origin per the
 *  zero-third-party-requests rule. */

export interface ModelFile {
  /** File name — also the path under VITE_MODEL_BASE in production. */
  name: string;
  hfUrl: string;
  bytes: number;
}

export interface ModelVariant {
  id: string;
  /** Persian row label (final copy register). */
  label: string;
  /** LTR meta line under the label. */
  metaLine: string;
  recommended: boolean;
  /** Graph + external-data pair for the modern (webgpu / wasm-simd) tiers. */
  graph: ModelFile;
  data: ModelFile;
  /** Single-file export for the no-SIMD compat tier. */
  embedded: ModelFile;
}

const HF_FP16 =
  'https://huggingface.co/Reza2kn/visualears-fastconformer-fa-full-ab-onnx-fp16/resolve/main';

export const MODEL_VARIANTS: ModelVariant[] = [
  {
    id: 'onnx-fp16',
    label: 'دقت کامل',
    metaLine: 'FastConformer-FA · 115M · 232 MB',
    recommended: true,
    graph: {
      name: 'fastconformer_ctc_fixed2005_fp16_full_io.onnx',
      hfUrl: `${HF_FP16}/fastconformer_ctc_fixed2005_fp16_full_io.onnx`,
      bytes: 2_815_131,
    },
    data: {
      name: 'fastconformer_ctc_fixed2005_fp16_full_io.onnx.data',
      hfUrl: `${HF_FP16}/fastconformer_ctc_fixed2005_fp16_full_io.onnx.data`,
      bytes: 228_870_563,
    },
    embedded: {
      name: 'fastconformer_ctc_fixed2005_fp16_full_io_embedded.onnx',
      hfUrl: `${HF_FP16}/fastconformer_ctc_fixed2005_fp16_full_io_embedded.onnx`,
      bytes: 231_626_820,
    },
  },
];

export const DEFAULT_VARIANT = MODEL_VARIANTS[0];

export function fileUrl(file: ModelFile): string {
  const base = import.meta.env.VITE_MODEL_BASE as string | undefined;
  return base ? `${base.replace(/\/$/, '')}/${file.name}` : file.hfUrl;
}
