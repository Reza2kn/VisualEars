/** Model variant manifest — the loader renders whatever is listed here, so new
 * artifacts (onnx-w4, onnx-fp, w2…) ship as config entries, not code changes.
 *
 * Dev fetches straight from Hugging Face (CORS-safe). Production builds set
 * VITE_MODEL_BASE (e.g. "/models") so everything stays same-origin per the
 * zero-third-party-requests rule. */

import { lang, type Lang } from '../lang';

export interface ModelFile {
  /** File name — also the path under VITE_MODEL_BASE in production. */
  name: string;
  hfUrl: string;
  bytes: number;
}

export interface ModelSidecars {
  tokens: ModelFile;
  preprocessor: ModelFile;
  melFilters?: ModelFile;
}

export interface ModelVariant {
  id: string;
  /** Which UI language this model serves (visualears.com → en, shenava.app → fa). */
  lang: Lang;
  /** Row label, in the model's own language. */
  label: string;
  /** LTR meta line under the label. */
  metaLine: string;
  recommended: boolean;
  tensorType: 'float16' | 'float32';
  hasLengthInput: boolean;
  /** Model emits punctuation (and speaker tokens) in its own token stream, so the
   *  pipeline must NOT run the Persian prosody punctuator over its output. */
  nativeFormatting?: boolean;
  /** Model transcribes numbers as spoken Persian words (not digits) — so the
   *  Persian ITN MUST still run to digitize them for display, even when
   *  nativeFormatting handles the punctuation. (v4+: spoken-number labels.) */
  spokenNumbers?: boolean;
  /** Graph + external-data pair for the modern (webgpu / wasm-simd) tiers. */
  graph: ModelFile;
  data: ModelFile;
  /** Single-file export for the no-SIMD compat tier. */
  embedded: ModelFile;
  sidecars: ModelSidecars;
}

const HF_SHENAVA_RIZEH_09_FP16 =
  'https://huggingface.co/Reza2kn/Shenava-Rizeh-0.9-onnx-fp16/resolve/main';
const HF_SHENAVA_KOOCHIK_09_FP16 =
  'https://huggingface.co/Reza2kn/Shenava-Koochik-0.9-onnx-fp16/resolve/main';
// Koochik v3 — ve_tok_v3 tokenizer (digits + punctuation + <spk> diarization).
// Not on HF yet; production serves it from same-origin /models.
const HF_SHENAVA_KOOCHIK_09_V3_FP16 =
  'https://huggingface.co/Reza2kn/Shenava-Koochik-0.9-onnx-fp16/resolve/main';
// English model — not yet on HF; production serves it from same-origin /models.
// (Upload here to make dev/HF-fallback work too.)
const HF_VISUALEARS_EN_01_FP16 =
  'https://huggingface.co/Reza2kn/VisualEars-EN-0.1-onnx-fp16/resolve/main';
const MODEL_ASSET_VERSION = '20260625-koochik-v4-numfix';

export const MODEL_VARIANTS: ModelVariant[] = [
  {
    // Koochik v4 — same arch + ve_tok_v3, retrained on spoken-form number labels
    // (CTC transcribes numbers as words, ITN digitizes at display) so multi-digit
    // numbers are no longer mangled. golden-6669 [70,1]: NUMERAL 11.67% ≈ NON-num 12.40%.
    id: 'shenava-koochik-0-9-v4-onnx-fp16',
    lang: 'fa',
    label: 'شنوا کوچیک ۴',
    metaLine: 'Koochik v4 · اعداد اصلاح‌شده + نگارش و گوینده · ONNX FP16 · 230 MB',
    recommended: true,
    tensorType: 'float16',
    hasLengthInput: true,
    nativeFormatting: true,
    spokenNumbers: true,
    graph: {
      name: 'shenava_koochik_0_9_v4_ctc_fixed2005_len_att70_1_fp16_full_io.onnx',
      hfUrl: `${HF_SHENAVA_KOOCHIK_09_V3_FP16}/shenava_koochik_0_9_v4_ctc_fixed2005_len_att70_1_fp16_full_io.onnx`,
      bytes: 11_316_554,
    },
    data: {
      name: 'shenava_koochik_0_9_v4_ctc_fixed2005_len_att70_1_fp16_full_io.onnx.data',
      hfUrl: `${HF_SHENAVA_KOOCHIK_09_V3_FP16}/shenava_koochik_0_9_v4_ctc_fixed2005_len_att70_1_fp16_full_io.onnx.data`,
      bytes: 218_839_562,
    },
    embedded: {
      name: 'shenava_koochik_0_9_v4_ctc_fixed2005_len_att70_1_fp16_full_io_embedded.onnx',
      hfUrl: `${HF_SHENAVA_KOOCHIK_09_V3_FP16}/shenava_koochik_0_9_v4_ctc_fixed2005_len_att70_1_fp16_full_io_embedded.onnx`,
      bytes: 230_079_714,
    },
    sidecars: {
      tokens: {
        name: 'shenava_koochik_0_9_v4-tokens.json',
        hfUrl: `${HF_SHENAVA_KOOCHIK_09_V3_FP16}/tokens.json`,
        bytes: 14_673,
      },
      preprocessor: {
        name: 'shenava_koochik_0_9_v4-preprocessor.json',
        hfUrl: `${HF_SHENAVA_KOOCHIK_09_V3_FP16}/preprocessor.json`,
        bytes: 1_800,
      },
      melFilters: {
        name: 'shenava_koochik_0_9_v4-mel_filters_slaney_80x257.json',
        hfUrl: `${HF_SHENAVA_KOOCHIK_09_V3_FP16}/mel_filters_slaney_80x257.json`,
        bytes: 91_115,
      },
    },
  },
  {
    id: 'shenava-koochik-0-9-v3-onnx-fp16',
    lang: 'fa',
    label: 'شنوا کوچیک ۳',
    metaLine: 'Koochik v3 · نگارش و اعداد و گوینده · ONNX FP16 · 230 MB',
    recommended: false,
    tensorType: 'float16',
    hasLengthInput: true,
    nativeFormatting: true,
    graph: {
      name: 'shenava_koochik_0_9_v3_ctc_fixed2005_len_att70_1_fp16_full_io.onnx',
      hfUrl: `${HF_SHENAVA_KOOCHIK_09_V3_FP16}/shenava_koochik_0_9_v3_ctc_fixed2005_len_att70_1_fp16_full_io.onnx`,
      bytes: 11_316_554,
    },
    data: {
      name: 'shenava_koochik_0_9_v3_ctc_fixed2005_len_att70_1_fp16_full_io.onnx.data',
      hfUrl: `${HF_SHENAVA_KOOCHIK_09_V3_FP16}/shenava_koochik_0_9_v3_ctc_fixed2005_len_att70_1_fp16_full_io.onnx.data`,
      bytes: 218_839_562,
    },
    embedded: {
      name: 'shenava_koochik_0_9_v3_ctc_fixed2005_len_att70_1_fp16_full_io_embedded.onnx',
      hfUrl: `${HF_SHENAVA_KOOCHIK_09_V3_FP16}/shenava_koochik_0_9_v3_ctc_fixed2005_len_att70_1_fp16_full_io_embedded.onnx`,
      bytes: 230_079_714,
    },
    sidecars: {
      tokens: {
        name: 'shenava_koochik_0_9_v3-tokens.json',
        hfUrl: `${HF_SHENAVA_KOOCHIK_09_V3_FP16}/tokens.json`,
        bytes: 14_673,
      },
      preprocessor: {
        name: 'shenava_koochik_0_9_v3-preprocessor.json',
        hfUrl: `${HF_SHENAVA_KOOCHIK_09_V3_FP16}/preprocessor.json`,
        bytes: 1_800,
      },
      melFilters: {
        name: 'shenava_koochik_0_9_v3-mel_filters_slaney_80x257.json',
        hfUrl: `${HF_SHENAVA_KOOCHIK_09_V3_FP16}/mel_filters_slaney_80x257.json`,
        bytes: 91_115,
      },
    },
  },
  {
    id: 'shenava-rizeh-0-9-onnx-fp16',
    lang: 'fa',
    label: 'شنوا ریزه',
    metaLine: 'Shenava 0.9 · ONNX FP16 · 59 MB',
    recommended: false,
    tensorType: 'float16',
    hasLengthInput: true,
    graph: {
      name: 'shenava_rizeh_0_9_ctc_fixed2005_len_att70_1_fp16_full_io.onnx',
      hfUrl: `${HF_SHENAVA_RIZEH_09_FP16}/shenava_rizeh_0_9_ctc_fixed2005_len_att70_1_fp16_full_io.onnx`,
      bytes: 6_171_937,
    },
    data: {
      name: 'shenava_rizeh_0_9_ctc_fixed2005_len_att70_1_fp16_full_io.onnx.data',
      hfUrl: `${HF_SHENAVA_RIZEH_09_FP16}/shenava_rizeh_0_9_ctc_fixed2005_len_att70_1_fp16_full_io.onnx.data`,
      bytes: 52_731_906,
    },
    embedded: {
      name: 'shenava_rizeh_0_9_ctc_fixed2005_len_att70_1_fp16_full_io_embedded.onnx',
      hfUrl: `${HF_SHENAVA_RIZEH_09_FP16}/shenava_rizeh_0_9_ctc_fixed2005_len_att70_1_fp16_full_io_embedded.onnx`,
      bytes: 58_875_455,
    },
    sidecars: {
      tokens: {
        name: 'shenava_rizeh_0_9-tokens.json',
        hfUrl: `${HF_SHENAVA_RIZEH_09_FP16}/tokens.json`,
        bytes: 15_115,
      },
      preprocessor: {
        name: 'shenava_rizeh_0_9-preprocessor.json',
        hfUrl: `${HF_SHENAVA_RIZEH_09_FP16}/preprocessor.json`,
        bytes: 1_795,
      },
      melFilters: {
        name: 'shenava_rizeh_0_9-mel_filters_slaney_80x257.json',
        hfUrl: `${HF_SHENAVA_RIZEH_09_FP16}/mel_filters_slaney_80x257.json`,
        bytes: 91_115,
      },
    },
  },
  {
    id: 'shenava-koochik-0-9-onnx-fp16',
    lang: 'fa',
    label: 'شنوا کوچیک',
    metaLine: 'Shenava 0.9 · ONNX FP16 · 230 MB',
    recommended: false,
    tensorType: 'float16',
    hasLengthInput: true,
    graph: {
      name: 'shenava_koochik_0_9_ctc_fixed2005_len_att70_1_fp16_full_io.onnx',
      hfUrl: `${HF_SHENAVA_KOOCHIK_09_FP16}/shenava_koochik_0_9_ctc_fixed2005_len_att70_1_fp16_full_io.onnx`,
      bytes: 11_207_297,
    },
    data: {
      name: 'shenava_koochik_0_9_ctc_fixed2005_len_att70_1_fp16_full_io.onnx.data',
      hfUrl: `${HF_SHENAVA_KOOCHIK_09_FP16}/shenava_koochik_0_9_ctc_fixed2005_len_att70_1_fp16_full_io.onnx.data`,
      bytes: 218_835_458,
    },
    embedded: {
      name: 'shenava_koochik_0_9_ctc_fixed2005_len_att70_1_fp16_full_io_embedded.onnx',
      hfUrl: `${HF_SHENAVA_KOOCHIK_09_FP16}/shenava_koochik_0_9_ctc_fixed2005_len_att70_1_fp16_full_io_embedded.onnx`,
      bytes: 229_968_267,
    },
    sidecars: {
      tokens: {
        name: 'shenava_koochik_0_9-tokens.json',
        hfUrl: `${HF_SHENAVA_KOOCHIK_09_FP16}/tokens.json`,
        bytes: 15_115,
      },
      preprocessor: {
        name: 'shenava_koochik_0_9-preprocessor.json',
        hfUrl: `${HF_SHENAVA_KOOCHIK_09_FP16}/preprocessor.json`,
        bytes: 1_797,
      },
      melFilters: {
        name: 'shenava_koochik_0_9-mel_filters_slaney_80x257.json',
        hfUrl: `${HF_SHENAVA_KOOCHIK_09_FP16}/mel_filters_slaney_80x257.json`,
        bytes: 91_115,
      },
    },
  },
  // English — NVIDIA FastConformer-Hybrid medium (CTC head), exported to ONNX FP16
  // via the same pipeline as the Persian models. Punctuation + capitalization built in.
  {
    id: 'visualears-en-0-1-onnx-fp16',
    lang: 'en',
    label: 'VisualEars English',
    metaLine: 'FastConformer EN · ONNX FP16 · 59 MB',
    recommended: true,
    tensorType: 'float16',
    hasLengthInput: true,
    graph: {
      name: 'visualears_en_0_1_ctc_fixed2005_len_att70_1_fp16_full_io.onnx',
      hfUrl: `${HF_VISUALEARS_EN_01_FP16}/visualears_en_0_1_ctc_fixed2005_len_att70_1_fp16_full_io.onnx`,
      bytes: 6_279_156,
    },
    data: {
      name: 'visualears_en_0_1_ctc_fixed2005_len_att70_1_fp16_full_io.onnx.data',
      hfUrl: `${HF_VISUALEARS_EN_01_FP16}/visualears_en_0_1_ctc_fixed2005_len_att70_1_fp16_full_io.onnx.data`,
      bytes: 52_731_906,
    },
    embedded: {
      name: 'visualears_en_0_1_ctc_fixed2005_len_att70_1_fp16_full_io_embedded.onnx',
      hfUrl: `${HF_VISUALEARS_EN_01_FP16}/visualears_en_0_1_ctc_fixed2005_len_att70_1_fp16_full_io_embedded.onnx`,
      bytes: 58_982_674,
    },
    sidecars: {
      tokens: {
        name: 'visualears_en_0_1-tokens.json',
        hfUrl: `${HF_VISUALEARS_EN_01_FP16}/tokens.json`,
        bytes: 14_686,
      },
      preprocessor: {
        name: 'visualears_en_0_1-preprocessor.json',
        hfUrl: `${HF_VISUALEARS_EN_01_FP16}/preprocessor.json`,
        bytes: 1_795,
      },
      melFilters: {
        name: 'visualears_en_0_1-mel_filters_slaney_80x257.json',
        hfUrl: `${HF_VISUALEARS_EN_01_FP16}/mel_filters_slaney_80x257.json`,
        bytes: 91_115,
      },
    },
  },
];

/** Variants for the active UI language (visualears.com → en, shenava.app → fa). */
export const LANG_VARIANTS = MODEL_VARIANTS.filter((v) => v.lang === lang);

/** Default model for the active language (falls back to the first listed). */
export const DEFAULT_VARIANT = LANG_VARIANTS[0] ?? MODEL_VARIANTS[0];

export function fileUrl(file: ModelFile): string {
  const base = import.meta.env.VITE_MODEL_BASE as string | undefined;
  return base
    ? `${base.replace(/\/$/, '')}/${file.name}?v=${MODEL_ASSET_VERSION}`
    : file.hfUrl;
}
