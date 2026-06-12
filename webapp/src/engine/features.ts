/** Browser-style 80-bin log-mel features for the fixed-frame CTC core.
 *  This is the exact pipeline validated to 269/269 transcript parity
 *  (see preprocessor.json) — ported from the Space, with preallocated
 *  scratch buffers since live mode calls it every couple of seconds. */

import preprocessor from './preprocessor.json';

export const SAMPLE_RATE = preprocessor.sample_rate; // 16000
export const N_FFT = preprocessor.n_fft; // 512
export const WIN_LENGTH = preprocessor.win_length; // 400
export const HOP_LENGTH = preprocessor.hop_length; // 160
export const N_MELS = preprocessor.n_mels; // 80
export const FIXED_FRAMES = preprocessor.fixed_frames; // 2005
export const OUTPUT_STRIDE = preprocessor.output_stride; // 8
export const MAX_SAMPLES = (FIXED_FRAMES - 1) * HOP_LENGTH + WIN_LENGTH; // 321,040 ≈ 20.07 s

export interface FeatureResult {
  /** [N_MELS × FIXED_FRAMES], mel-major. Reused between calls — consume before the next compute(). */
  features: Float32Array;
  frameCount: number;
}

function hzToMel(hz: number): number {
  return 2595 * Math.log10(1 + hz / 700);
}

function melToHz(mel: number): number {
  return 700 * (10 ** (mel / 2595) - 1);
}

function createMelFilters(): Float32Array[] {
  const minMel = hzToMel(0);
  const maxMel = hzToMel(SAMPLE_RATE / 2);
  const melPoints = new Array(N_MELS + 2)
    .fill(0)
    .map((_, i) => minMel + ((maxMel - minMel) * i) / (N_MELS + 1));
  const bins = melPoints.map((m) => Math.floor(((N_FFT + 1) * melToHz(m)) / SAMPLE_RATE));
  const filters: Float32Array[] = [];
  for (let m = 1; m <= N_MELS; m++) {
    const filter = new Float32Array(N_FFT / 2 + 1);
    const left = bins[m - 1];
    const center = bins[m];
    const right = bins[m + 1];
    for (let k = left; k < center; k++) filter[k] = (k - left) / Math.max(1, center - left);
    for (let k = center; k < right; k++) filter[k] = (right - k) / Math.max(1, right - center);
    filters.push(filter);
  }
  return filters;
}

export class FeatureExtractor {
  private readonly cos = new Float32Array(N_FFT / 2);
  private readonly sin = new Float32Array(N_FFT / 2);
  private readonly hann = new Float32Array(WIN_LENGTH);
  private readonly melFilters = createMelFilters();
  private readonly re = new Float32Array(N_FFT);
  private readonly im = new Float32Array(N_FFT);
  private readonly power = new Float32Array(N_FFT / 2 + 1);
  private readonly features = new Float32Array(N_MELS * FIXED_FRAMES);

  constructor() {
    for (let i = 0; i < N_FFT / 2; i++) {
      this.cos[i] = Math.cos((-2 * Math.PI * i) / N_FFT);
      this.sin[i] = Math.sin((-2 * Math.PI * i) / N_FFT);
    }
    for (let i = 0; i < WIN_LENGTH; i++) {
      this.hann[i] = 0.5 - 0.5 * Math.cos((2 * Math.PI * i) / (WIN_LENGTH - 1));
    }
  }

  /** In-place iterative radix-2 FFT over this.re/this.im → power spectrum. */
  private fftRealPower(): Float32Array {
    const n = N_FFT;
    const { re, im, cos, sin, power } = this;
    im.fill(0);
    let j = 0;
    for (let i = 1; i < n; i++) {
      let bit = n >> 1;
      for (; j & bit; bit >>= 1) j ^= bit;
      j ^= bit;
      if (i < j) {
        const tr = re[i];
        re[i] = re[j];
        re[j] = tr;
        const ti = im[i];
        im[i] = im[j];
        im[j] = ti;
      }
    }
    for (let len = 2; len <= n; len <<= 1) {
      const half = len >> 1;
      const step = n / len;
      for (let i = 0; i < n; i += len) {
        for (let k = 0; k < half; k++) {
          const idx = k * step;
          const wr = cos[idx];
          const wi = sin[idx];
          const ur = re[i + k];
          const ui = im[i + k];
          const vr = re[i + k + half] * wr - im[i + k + half] * wi;
          const vi = re[i + k + half] * wi + im[i + k + half] * wr;
          re[i + k] = ur + vr;
          im[i + k] = ui + vi;
          re[i + k + half] = ur - vr;
          im[i + k + half] = ui - vi;
        }
      }
    }
    for (let i = 0; i < power.length; i++) power[i] = (re[i] * re[i] + im[i] * im[i]) / n;
    return power;
  }

  compute(pcm: Float32Array): FeatureResult {
    const frameCount = Math.max(
      1,
      Math.min(FIXED_FRAMES, Math.floor((pcm.length - WIN_LENGTH) / HOP_LENGTH) + 1),
    );
    const features = this.features;
    features.fill(0);

    for (let t = 0; t < frameCount; t++) {
      const offset = t * HOP_LENGTH;
      const { re } = this;
      re.fill(0);
      for (let i = 0; i < WIN_LENGTH; i++) re[i] = (pcm[offset + i] || 0) * this.hann[i];
      const power = this.fftRealPower();
      for (let m = 0; m < N_MELS; m++) {
        const filter = this.melFilters[m];
        let energy = 0;
        for (let k = 0; k < filter.length; k++) energy += power[k] * filter[k];
        features[m * FIXED_FRAMES + t] = Math.log(Math.max(energy, 1e-20));
      }
    }

    // Per-mel-bin mean/variance normalization over the valid frames only.
    for (let m = 0; m < N_MELS; m++) {
      const base = m * FIXED_FRAMES;
      let mean = 0;
      for (let t = 0; t < frameCount; t++) mean += features[base + t];
      mean /= frameCount;
      let variance = 0;
      for (let t = 0; t < frameCount; t++) {
        const d = features[base + t] - mean;
        variance += d * d;
      }
      const invStd = 1 / Math.sqrt(variance / frameCount + 1e-5);
      for (let t = 0; t < frameCount; t++) features[base + t] = (features[base + t] - mean) * invStd;
    }

    return { features, frameCount };
  }
}
