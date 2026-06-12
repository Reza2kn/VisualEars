#!/usr/bin/env python3
"""Generate golden fixtures for the TypeScript log-mel feature extractor.

Implements the exact pipeline from webapp/src/engine/preprocessor.json in
float64 numpy (the JS implementation stores intermediates as float32 but does
arithmetic in float64, so a small tolerance absorbs the difference):

  hann(400, formula 0.5-0.5cos(2*pi*i/399)) -> rFFT 512 -> power/512
  -> HTK mel filters (bins floor((513*hz)/16000)) -> ln(max(e,1e-20))
  -> per-mel-bin mean/var normalization over valid frames (var+1e-5)

Writes webapp/tests/fixtures/features_golden.json with deterministic synthetic
signals and (sub)sampled expected values.
"""

import json
import math
from pathlib import Path

import numpy as np

SAMPLE_RATE = 16000
N_FFT = 512
WIN_LENGTH = 400
HOP_LENGTH = 160
N_MELS = 80
FIXED_FRAMES = 2005


def hz_to_mel(hz: float) -> float:
    return 2595 * math.log10(1 + hz / 700)


def mel_to_hz(mel: float) -> float:
    return 700 * (10 ** (mel / 2595) - 1)


def mel_filters() -> np.ndarray:
    min_mel = hz_to_mel(0)
    max_mel = hz_to_mel(SAMPLE_RATE / 2)
    mel_points = [min_mel + (max_mel - min_mel) * i / (N_MELS + 1) for i in range(N_MELS + 2)]
    bins = [int(math.floor((N_FFT + 1) * mel_to_hz(m) / SAMPLE_RATE)) for m in mel_points]
    filters = np.zeros((N_MELS, N_FFT // 2 + 1), dtype=np.float64)
    for m in range(1, N_MELS + 1):
        left, center, right = bins[m - 1], bins[m], bins[m + 1]
        for k in range(left, center):
            filters[m - 1, k] = (k - left) / max(1, center - left)
        for k in range(center, right):
            filters[m - 1, k] = (right - k) / max(1, right - center)
    return filters


def bit_reverse_permutation(n: int) -> list[int]:
    """Index permutation matching the JS in-place bit-reversal loop."""
    perm = list(range(n))
    j = 0
    for i in range(1, n):
        bit = n >> 1
        while j & bit:
            j ^= bit
            bit >>= 1
        j ^= bit
        if i < j:
            perm[i], perm[j] = perm[j], perm[i]
    return perm


_PERM = bit_reverse_permutation(N_FFT)
# Twiddles stored as float32, exactly like the JS Float32Array tables.
_COS = np.cos(-2 * np.pi * np.arange(N_FFT // 2) / N_FFT).astype(np.float32)
_SIN = np.sin(-2 * np.pi * np.arange(N_FFT // 2) / N_FFT).astype(np.float32)


def fft_power_f32(frame: np.ndarray) -> np.ndarray:
    """Radix-2 FFT replicating the JS float32 storage semantics: every value
    written back to re/im is rounded to float32; per-butterfly arithmetic runs
    in float64 (JS numbers)."""
    n = N_FFT
    re = frame[_PERM].astype(np.float32).astype(np.float64)
    im = np.zeros(n, dtype=np.float64)
    length = 2
    while length <= n:
        half = length >> 1
        step = n // length
        k = np.arange(half)
        wr = _COS[k * step].astype(np.float64)
        wi = _SIN[k * step].astype(np.float64)
        for i in range(0, n, length):
            ur = re[i : i + half].copy()
            ui = im[i : i + half].copy()
            vr = re[i + half : i + length] * wr - im[i + half : i + length] * wi
            vi = re[i + half : i + length] * wi + im[i + half : i + length] * wr
            re[i : i + half] = (ur + vr).astype(np.float32)
            im[i : i + half] = (ui + vi).astype(np.float32)
            re[i + half : i + length] = (ur - vr).astype(np.float32)
            im[i + half : i + length] = (ui - vi).astype(np.float32)
        length <<= 1
    half_spectrum = slice(0, n // 2 + 1)
    power = (re[half_spectrum] * re[half_spectrum] + im[half_spectrum] * im[half_spectrum]) / n
    return power.astype(np.float32).astype(np.float64)


def log_mel(pcm: np.ndarray) -> tuple[np.ndarray, np.ndarray, int]:
    frame_count = max(1, min(FIXED_FRAMES, (len(pcm) - WIN_LENGTH) // HOP_LENGTH + 1))
    hann = (0.5 - 0.5 * np.cos(2 * np.pi * np.arange(WIN_LENGTH) / (WIN_LENGTH - 1))).astype(
        np.float32
    )
    filters = mel_filters().astype(np.float32).astype(np.float64)
    features = np.zeros((N_MELS, FIXED_FRAMES), dtype=np.float64)
    for t in range(frame_count):
        frame = np.zeros(N_FFT, dtype=np.float64)
        offset = t * HOP_LENGTH
        avail = max(0, min(WIN_LENGTH, len(pcm) - offset))
        if avail > 0:
            windowed = pcm[offset : offset + avail] * hann[:avail].astype(np.float64)
            frame[:avail] = windowed.astype(np.float32)
        power = fft_power_f32(frame)
        mel = filters @ power
        features[:, t] = np.log(np.maximum(mel, 1e-20)).astype(np.float32)
    raw = features.copy()
    valid = features[:, :frame_count]
    mean = valid.mean(axis=1, keepdims=True)
    var = ((valid - mean) ** 2).mean(axis=1, keepdims=True)
    features[:, :frame_count] = ((valid - mean) / np.sqrt(var + 1e-5)).astype(np.float32)
    return features, raw, frame_count


def make_cases() -> list[dict]:
    cases = []

    # Cells whose raw log-mel energy sits at the float32 cancellation floor are
    # runtime-noise, not signal — they are masked out of the comparison.
    STABLE_LOG_ENERGY = -30.0

    # Synthetic tones get a deterministic -66 dB noise floor: real audio never
    # has digital-zero bins, and pure silence bins are float32-noise chaos that
    # no two runtimes reproduce bit-identically.
    rng_floor = np.random.default_rng(7)

    # 1. 440 Hz tone over a noise floor, 0.5 s — store the full valid feature block.
    n = 8000
    t = np.arange(n) / SAMPLE_RATE
    signal = 0.3 * np.sin(2 * np.pi * 440 * t) + 5e-4 * rng_floor.standard_normal(n)
    pcm = np.round(signal.astype(np.float32).astype(np.float64), 6)
    feats, raw, fc = log_mel(pcm)
    cases.append(
        {
            "name": "sine-440",
            "pcm": pcm.tolist(),
            "frameCount": fc,
            "fullValid": np.round(feats[:, :fc], 5).tolist(),
            "stable": (raw[:, :fc] > STABLE_LOG_ENERGY).astype(int).tolist(),
        }
    )

    # 2. Chirp 100→4000 Hz with amplitude ramp, 1.3 s, odd length — grid samples.
    n = 20801
    t = np.arange(n) / SAMPLE_RATE
    freq = 100 + (4000 - 100) * t / t[-1]
    signal = (0.05 + 0.4 * t / t[-1]) * np.sin(2 * np.pi * freq * t) + 5e-4 * rng_floor.standard_normal(n)
    pcm = np.round(signal.astype(np.float32).astype(np.float64), 6)
    feats, raw, fc = log_mel(pcm)
    mel_idx = [0, 7, 19, 33, 47, 61, 79]
    frame_idx = [0, 1, 17, 42, 77, fc - 2, fc - 1]
    cases.append(
        {
            "name": "chirp-ramp",
            "pcm": pcm.tolist(),
            "frameCount": fc,
            "melIdx": mel_idx,
            "frameIdx": frame_idx,
            "grid": np.round(feats[np.ix_(mel_idx, frame_idx)], 5).tolist(),
            "gridStable": (raw[np.ix_(mel_idx, frame_idx)] > STABLE_LOG_ENERGY).astype(int).tolist(),
        }
    )

    # 3. Shorter than one window (300 samples) — frameCount clamps to 1.
    rng = np.random.default_rng(42)
    pcm = np.round((0.1 * rng.standard_normal(300)).astype(np.float32).astype(np.float64), 6)
    feats, raw, fc = log_mel(pcm)
    cases.append(
        {
            "name": "tiny-noise",
            "pcm": pcm.tolist(),
            "frameCount": fc,
            "fullValid": np.round(feats[:, :fc], 5).tolist(),
            "stable": (raw[:, :fc] > STABLE_LOG_ENERGY).astype(int).tolist(),
        }
    )

    return cases


def main() -> None:
    out = Path(__file__).resolve().parent.parent / "webapp/tests/fixtures/features_golden.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps({"cases": make_cases()}, separators=(",", ":")))
    print(f"wrote {out} ({out.stat().st_size / 1024:.0f} KB)")


if __name__ == "__main__":
    main()
