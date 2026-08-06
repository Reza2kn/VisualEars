//! Un-normalized slaney log-mel, byte-for-byte match to the macOS `NativeFeatureExtractor`
//! and the NeMo `normalize=NA` fbank contract: preemphasis 0.97 → reflect-padded framing
//! (hop 160, center) → Hann-400-in-512 → power spectrum → 80 slaney mel bins → ln(x + 2^-24).

use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;

pub const SAMPLE_RATE: usize = 16_000;
const N_FFT: usize = 512;
const WIN_LEN: usize = 400;
pub const HOP: usize = 160;
pub const N_MELS: usize = 80;
const CENTER_PAD: i64 = 256;
const PREEMPH: f32 = 0.97;

pub struct FeatureExtractor {
    mel_filters: Vec<Vec<f32>>, // [80][257]
    hann: Vec<f32>,             // [400]
    fft: Arc<dyn Fft<f32>>,
}

impl FeatureExtractor {
    pub fn new(mel_filters: Vec<Vec<f32>>) -> Self {
        assert_eq!(mel_filters.len(), N_MELS, "mel filters must be 80 rows");
        assert!(
            mel_filters.iter().all(|r| r.len() == N_FFT / 2 + 1),
            "each mel row must be 257"
        );
        let hann = (0..WIN_LEN)
            .map(|i| {
                0.5 - 0.5 * ((2.0 * std::f32::consts::PI * i as f32) / (WIN_LEN as f32 - 1.0)).cos()
            })
            .collect();
        let fft = FftPlanner::<f32>::new().plan_fft_forward(N_FFT);
        Self {
            mel_filters,
            hann,
            fft,
        }
    }

    /// Feature-major `[80 * fixed_frames]` log-mel plus the true (pre-padding) frame count.
    pub fn compute(&self, pcm: &[f32], fixed_frames: usize) -> (Vec<f32>, usize) {
        let max_samples = fixed_frames.saturating_sub(1) * HOP;
        let source: &[f32] = if pcm.len() > max_samples {
            &pcm[pcm.len() - max_samples..]
        } else {
            pcm
        };
        let mut features = vec![0f32; N_MELS * fixed_frames];
        if source.is_empty() {
            return (features, 1);
        }
        // torch.stft(center=True, pad_mode="reflect") → floor(samples/hop)+1 valid frames.
        let frame_count = (source.len() / HOP + 1).min(fixed_frames).max(1);

        let mut signal = vec![0f32; source.len()];
        signal[0] = source[0];
        for i in 1..source.len() {
            signal[i] = source[i] - PREEMPH * source[i - 1];
        }

        let window_offset = (N_FFT - WIN_LEN) / 2; // 56
        let log_guard = 2f32.powi(-24);
        let mut buf = vec![Complex { re: 0f32, im: 0f32 }; N_FFT];
        for t in 0..frame_count {
            let frame_start = t as i64 * HOP as i64 - CENTER_PAD;
            for c in buf.iter_mut() {
                *c = Complex { re: 0.0, im: 0.0 };
            }
            for i in 0..WIN_LEN {
                let idx = frame_start + window_offset as i64 + i as i64;
                buf[window_offset + i].re = sample_at(&signal, idx) * self.hann[i];
            }
            self.fft.process(&mut buf);
            for (m, filter) in self.mel_filters.iter().enumerate() {
                let mut energy = 0f32;
                for (k, &w) in filter.iter().enumerate() {
                    energy += (buf[k].re * buf[k].re + buf[k].im * buf[k].im) * w;
                }
                features[m * fixed_frames + t] = (energy + log_guard).ln();
            }
        }
        (features, frame_count)
    }
}

/// Reflect-pad indexing (matches librosa/torch `pad_mode="reflect"`).
fn sample_at(signal: &[f32], index: i64) -> f32 {
    let n = signal.len() as i64;
    if n == 0 {
        return 0.0;
    }
    if n == 1 {
        return signal[0];
    }
    let mut r = index;
    while r < 0 || r >= n {
        if r < 0 {
            r = -r;
        }
        if r >= n {
            r = 2 * n - 2 - r;
        }
    }
    signal[r as usize]
}

/// Load the shared `mel_filters_slaney_80x257.json` (list of 80 rows × 257 floats).
pub fn load_mel_filters(path: &str) -> std::io::Result<Vec<Vec<f32>>> {
    let data = std::fs::read(path)?;
    let rows: Vec<Vec<f32>> = serde_json::from_slice(&data)?;
    Ok(rows)
}
