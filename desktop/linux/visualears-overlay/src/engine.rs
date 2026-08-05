//! Cache-aware streaming recognizer on the native tract engine — a Rust port of the macOS
//! `ShenavaStreamingRuntime` (chunk windowing + greedy decode carrying state across chunks) fused
//! with the `visualears_tract` Recognizer (5-input streaming graph + threaded caches).
//!
//! Two decode heads share the SAME cache-aware encoder step:
//!   * CTC   (default) — argmax over the encoder's `ctc_logprobs` head, blank-collapse + dedup.
//!   * RNNT  (opt-in via [`StreamingRecognizer::attach_rnnt_decoder`]) — greedy transducer loop
//!     over the encoder's `enc_hidden` head, driving a separate prednet+joint STEP graph.
//! Both are kept: the encoder emits BOTH heads, so a future joint CTC+RNNT fusion is possible.

use crate::features::{FeatureExtractor, HOP, N_MELS};
use crate::model::ShenavaModel;
use tract_core::model::translator::Translate;
use tract_onnx::prelude::*;

const LEFT_CONTEXT: usize = 16; // left mel frames kept for stable reflection
const VOCAB: usize = 1025;
const BLANK_ID: usize = 1024;
const MAX_OUT_FRAMES: usize = 32;
const FINAL_TAIL_PAD_SAMPLES: usize = (16_000 * 72) / 100;
// RNNT prednet (single-layer LSTM): hidden state width and the greedy per-frame emit cap.
const PRED_HIDDEN: usize = 640;
const MAX_SYMBOLS: usize = 10;
const SOS_TOKEN: i64 = BLANK_ID as i64; // blank_as_pad → embed row 1024 is all-zeros = NeMo y=None SOS

/// Resolved encoder output positions (by ONNX output name, order-independent). The RNNT export
/// emits `[enc_hidden, ctc_logprobs, encoded_len, next_cache_last_channel, next_cache_last_time,
/// next_cache_last_channel_len]`; the older CTC-only export emits `[ctc_logprobs, encoded_len,
/// next_cache_last_channel, next_cache_last_time, next_cache_last_channel_len]`. We bind strictly by
/// name (both NeMo cache-aware exports use these exact labels) so either layout works and a renamed
/// or reordered graph is caught at load rather than silently mis-indexed. `enc_hidden` is optional
/// (absent on the CTC-only export); `enc_len` is optional (only the RNNT path consumes it, and it
/// falls back to all frames if absent).
struct EncOutIdx {
    enc_hidden: Option<usize>,
    ctc: usize,
    enc_len: Option<usize>,
    next_clc: usize,
    next_clt: usize,
    next_clcl: usize,
}

pub struct StreamingRecognizer {
    model: Arc<TypedRunnableModel>,
    enc_idx: EncOutIdx,
    // RNNT prednet+joint STEP graph (None = CTC-only, the default/fallback path).
    decoder: Option<Arc<TypedRunnableModel>>,
    ex: FeatureExtractor,
    tokens: Vec<String>,
    m: ShenavaModel,
    f16: bool,
    // streaming caches, stored owned (Tensor is Send, unlike a TValue that may wrap an Rc)
    clc: Tensor,  // cache_last_channel [1, L, 70, D]
    clt: Tensor,  // cache_last_time    [1, L, D, 8]
    clcl: Tensor, // cache_last_channel_len [1] i64
    // Optional CTC log-prob capture (None = off, zero overhead on the live path). When Some, decode_ctc
    // pushes each frame's [VOCAB] log-probs — for the DETERMINISTIC over-boost test (dump once, beam offline).
    lp_buf: Option<Vec<Vec<f32>>>,
    // RNNT prednet state, carried across chunks like the encoder cache (reset() clears it).
    h: Tensor,       // [1, 1, 640] f32 — LSTM hidden
    c: Tensor,       // [1, 1, 640] f32 — LSTM cell
    last_token: i64, // last EMITTED token (SOS=blank at stream start)
    // streaming state
    pending: Vec<f32>,
    base_frame: usize,
    chunk_index: usize,
    prev_token: i64,
    last_emit_tok: i64,
    frames_since_emit: usize,
    prev_chunk_had_emit: bool,
    emitted: Vec<usize>,
    committed_words: usize,
}

fn zero_caches(nl: usize, dm: usize, f16: bool) -> TractResult<(Tensor, Tensor, Tensor)> {
    let cast = |t: Tensor| -> TractResult<Tensor> {
        Ok(if f16 {
            t.cast_to::<tract_core::prelude::f16>()?.into_owned()
        } else {
            t
        })
    };
    Ok((
        cast(Tensor::zero::<f32>(&[1, nl, 70, dm])?)?,
        cast(Tensor::zero::<f32>(&[1, nl, dm, 8])?)?,
        tensor1(&[0i64]),
    ))
}

// RNNT prednet LSTM state, zero-initialised. The decoder-step graph is f32-only (it's small +
// numerically sensitive), so the state stays f32 even when the encoder runs in f16.
fn zero_pred_state() -> TractResult<(Tensor, Tensor)> {
    Ok((
        Tensor::zero::<f32>(&[1, 1, PRED_HIDDEN])?,
        Tensor::zero::<f32>(&[1, 1, PRED_HIDDEN])?,
    ))
}

// Map the encoder graph's output labels (ONNX names, captured pre-optimize; tract preserves output
// order) to fixed roles. Resolution is STRICTLY by unambiguous name match — no positional guessing —
// so a renamed/reordered export fails loudly at load instead of silently feeding (e.g.) a 1025-wide
// ctc tensor into a 256-wide RNNT step. `next_cache_last_channel` is matched exactly so it can't be
// confused with `next_cache_last_channel_len`.
fn resolve_enc_outputs(names: &[String]) -> TractResult<EncOutIdx> {
    let find = |needle: &str| names.iter().position(|n| n == needle);
    let find_any = |needles: &[&str]| needles.iter().find_map(|needle| find(needle));
    let require_any = |role: &str, needles: &[&str]| -> TractResult<usize> {
        find_any(needles).ok_or_else(|| {
            TractError::msg(format!(
                "encoder graph missing required output for {role}; got outputs {names:?}. \
                 Expected NeMo cache-aware names (ctc_logprobs, next_cache_last_channel, \
                 next_cache_last_time, next_cache_last_channel_len [+ enc_hidden, encoded_len for RNNT]) \
                 or the legacy names (logprobs, cache_last_channel_next, cache_last_time_next, \
                 cache_last_channel_next_len)."
            ))
        })
    };
    let idx = EncOutIdx {
        enc_hidden: find("enc_hidden"), // optional: absent on the CTC-only export
        ctc: require_any("CTC logits", &["ctc_logprobs", "logprobs"])?,
        enc_len: find_any(&["encoded_len", "encoded_lengths"]), // optional: only RNNT reads it
        next_clc: require_any(
            "next channel cache",
            &["next_cache_last_channel", "cache_last_channel_next"],
        )?,
        next_clt: require_any(
            "next time cache",
            &["next_cache_last_time", "cache_last_time_next"],
        )?,
        next_clcl: require_any(
            "next channel-cache length",
            &["next_cache_last_channel_len", "cache_last_channel_next_len"],
        )?,
    };
    Ok(idx)
}

// Ordered output names of a typed model (before into_optimized), for role resolution.
fn output_names(model: &TypedModel) -> Vec<String> {
    let outlets = model
        .output_outlets()
        .map(|o| o.to_vec())
        .unwrap_or_default();
    outlets
        .iter()
        .enumerate()
        .map(|(i, o)| {
            model
                .outlet_label(*o)
                .map(str::to_string)
                .unwrap_or_else(|| format!("output_{i}"))
        })
        .collect()
}

impl StreamingRecognizer {
    /// Shared model-build stage. A raw `.onnx` must be parsed, typed and (optionally) f16-converted
    /// - a slow, minute-scale pass - while a pre-optimized decluttered `.nnef.tar` skips all of it.
    /// `f16` conversion is applied here, so an exported NNEF is already in the target precision.
    fn build_typed_model(
        m: &ShenavaModel,
        model_path: &str,
        f16: bool,
    ) -> TractResult<TypedModel> {
        let (nl, dm, chunk) = (m.num_layers, m.d_model, m.chunk_frames);
        let is_nnef = model_path.ends_with(".nnef.tar")
            || model_path.ends_with(".nnef.tgz")
            || model_path.ends_with(".nnef");
        Ok(if is_nnef {
            eprintln!("[model] loading pre-optimized NNEF…");
            tract_nnef::nnef().model_for_path(model_path)?
        } else {
            eprintln!("[model] parsing ONNX graph + typing…");
            let mut t = tract_onnx::onnx()
                .model_for_path(model_path)?
                .with_input_fact(
                    0,
                    InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 80, chunk)),
                )?
                .with_input_fact(1, InferenceFact::dt_shape(i64::datum_type(), tvec!(1)))?
                .with_input_fact(
                    2,
                    InferenceFact::dt_shape(f32::datum_type(), tvec!(1, nl, 70, dm)),
                )?
                .with_input_fact(
                    3,
                    InferenceFact::dt_shape(f32::datum_type(), tvec!(1, nl, dm, 8)),
                )?
                .with_input_fact(4, InferenceFact::dt_shape(i64::datum_type(), tvec!(1)))?
                .into_typed()?;
            if f16 {
                eprintln!("[model] ONNX typed; converting to f16…");
                t = tract_core::floats::FloatPrecisionTranslator::new(
                    f32::datum_type(),
                    tract_core::prelude::f16::datum_type(),
                )
                .translate_model(&t)?;
            }
            t
        })
    }

    /// Export the typed (already f16/f32-converted) model as a pre-optimized NNEF `.nnef.tar`, so
    /// later loads skip the slow onnx->typed / precision-conversion pass entirely.
    pub fn export_nnef_to_tar(
        onnx_path: &str,
        m: &ShenavaModel,
        f16: bool,
        out_path: &str,
    ) -> TractResult<()> {
        let typed = Self::build_typed_model(m, onnx_path, f16)?;
        // Constant-folding (into_optimized) folds weight Casts away, giving the NNEF serializer
        // a graph it can write. The runnable build on load is per-device + fast anyway.
        let opt = typed.into_optimized()?;
        let file = std::fs::File::create(out_path)?;
        tract_nnef::nnef().write_to_tar(&opt, file)?;
        eprintln!("[model] wrote pre-optimized NNEF -> {out_path}");
        Ok(())
    }

    pub fn load(
        model_path: &str,
        m: ShenavaModel,
        ex: FeatureExtractor,
        tokens: Vec<String>,
        f16: bool,
    ) -> TractResult<Self> {
        let t_load_start = std::time::Instant::now();
        let typed = Self::build_typed_model(&m, model_path, f16)?;
        // Capture output labels BEFORE optimize (tract preserves graph output order through
        // optimization, so positional index == index in the runnable's output vec).
        let enc_idx = resolve_enc_outputs(&output_names(&typed))?;
        eprintln!("[model] building optimized runnable…");
        let model = typed.into_optimized()?.into_runnable()?;
        eprintln!("[model] ready in {:.1}s", t_load_start.elapsed().as_secs_f32());
        let (nl, dm, _chunk) = (m.num_layers, m.d_model, m.chunk_frames);
        let (clc, clt, clcl) = zero_caches(nl, dm, f16)?;
        let (h, c) = zero_pred_state()?;
        Ok(Self {
            model,
            enc_idx,
            decoder: None,
            ex,
            tokens,
            m,
            f16,
            clc,
            clt,
            clcl,
            lp_buf: None,
            h,
            c,
            last_token: SOS_TOKEN,
            pending: Vec::new(),
            base_frame: 0,
            chunk_index: 0,
            prev_token: -1,
            last_emit_tok: -1,
            frames_since_emit: 999,
            prev_chunk_had_emit: true,
            emitted: Vec::new(),
            committed_words: 0,
        })
    }

    /// Enable the RNNT greedy path by loading the prednet+joint STEP graph as a second runnable
    /// model. Inputs `enc_t[1,256] token[1] h_in[1,1,640] c_in[1,1,640]` → `logits[1,1025]
    /// h_out[1,1,640] c_out[1,1,640]`. The step graph is f32 (small + numerically sensitive); the
    /// encoder can still be f16/int4 independently. After this call, `step` decodes via RNNT.
    pub fn attach_rnnt_decoder(&mut self, decoder_path: &str) -> TractResult<()> {
        let dm = self.m.d_model;
        let typed = tract_onnx::onnx()
            .model_for_path(decoder_path)?
            .with_input_fact(0, InferenceFact::dt_shape(f32::datum_type(), tvec!(1, dm)))?
            .with_input_fact(1, InferenceFact::dt_shape(i64::datum_type(), tvec!(1)))?
            .with_input_fact(
                2,
                InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 1, PRED_HIDDEN)),
            )?
            .with_input_fact(
                3,
                InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 1, PRED_HIDDEN)),
            )?
            .into_typed()?;
        // into_runnable() already yields an Arc<TypedRunnableModel> (RunnableModel = SimplePlan,
        // wrapped by into_runnable), matching the `model` field — so no extra Arc::new here.
        self.decoder = Some(typed.into_optimized()?.into_runnable()?);
        Ok(())
    }

    pub fn reset(&mut self) -> TractResult<()> {
        let (clc, clt, clcl) = zero_caches(self.m.num_layers, self.m.d_model, self.f16)?;
        self.clc = clc;
        self.clt = clt;
        self.clcl = clcl;
        let (h, c) = zero_pred_state()?;
        self.h = h;
        self.c = c;
        self.last_token = SOS_TOKEN;
        self.pending.clear();
        self.base_frame = 0;
        self.chunk_index = 0;
        self.prev_token = -1;
        self.last_emit_tok = -1;
        self.frames_since_emit = 999;
        self.prev_chunk_had_emit = true;
        self.emitted.clear();
        self.committed_words = 0;
        Ok(())
    }

    /// Feed 16 kHz mono samples; returns the running decode of the live (uncommitted) tail.
    pub fn accept(&mut self, pcm: &[f32]) -> TractResult<String> {
        self.pending.extend_from_slice(pcm);
        self.drain(false)?;
        Ok(self.decoded_text())
    }

    pub fn flush(&mut self) -> TractResult<String> {
        self.pending
            .extend(std::iter::repeat(0.0).take(FINAL_TAIL_PAD_SAMPLES));
        self.drain(true)?;
        Ok(self.decoded_text())
    }

    // ---- RNNT batch decode (parity path) --------------------------------------------------------
    // Faithful port of the Python reference `stream_encoder` + `rnnt_greedy` (stream_ctc_vs_rnnt.py):
    // independent `CS`-sample raw-audio chunks (zero-padded), each mel'd to `CHUNK_MEL` frames with
    // `length = valid_samples/HOP`, fed to the cache-aware encoder; enc_hidden frames trimmed to
    // `encoded_len` and concatenated; then greedy RNNT with prednet state persistent across the whole
    // clip. This bypasses the rolling-window live path (`accept`) so tract matches the ONNX-loop
    // reference numerically. `attach_rnnt_decoder` must have been called.
    pub fn decode_full_rnnt(&mut self, pcm: &[f32]) -> TractResult<Vec<usize>> {
        // Streaming-cfg geometry for the english [70,13] export (verified against NeMo):
        //   shift_size[1]=112 → CS = 112*160 = 17920 samples/chunk; preprocessor emits 113 mel
        //   frames per full chunk, of which 112 are valid (length = valid_samples/HOP).
        const CS: usize = 112 * HOP; // 17920 samples per streaming chunk
        const CHUNK_MEL: usize = 113; // mel width per chunk (floor(CS/HOP)+1)
        self.reset()?;
        let n = pcm.len();
        let mut s = 0usize;
        while s < n {
            let v = (n - s).min(CS); // valid samples in this chunk
                                     // Zero-pad the tail chunk to a full CS window (NeMo pads then masks by length).
            let mut chunk = vec![0f32; CS];
            chunk[..v].copy_from_slice(&pcm[s..s + v]);
            let length = (v / HOP) as i64; // NeMo: valid mel frames = valid_samples // HOP
            let (feats, _) = self.ex.compute(&chunk, CHUNK_MEL); // feature-major [80*113]
            self.encoder_step_rnnt(&feats, CHUNK_MEL, length)?;
            s += CS;
        }
        Ok(self.emitted.clone())
    }

    // One encoder step for the batch RNNT path: run the cache-aware encoder on a [1,80,mel_w] chunk,
    // thread the caches, then greedy-decode the emitted enc_hidden frames (bounded by encoded_len).
    fn encoder_step_rnnt(
        &mut self,
        features: &[f32],
        mel_w: usize,
        length: i64,
    ) -> TractResult<()> {
        let mut audio = Tensor::from_shape(&[1usize, N_MELS, mel_w], features)?;
        if self.f16 {
            audio = audio.cast_to::<tract_core::prelude::f16>()?.into_owned();
        }
        let len_t = tensor1(&[length]);
        let out = self.model.run(tvec!(
            audio.into(),
            len_t.into(),
            self.clc.clone().into(),
            self.clt.clone().into(),
            self.clcl.clone().into()
        ))?;
        self.decode_rnnt(&out)?;
        self.clc = out[self.enc_idx.next_clc].clone().into_tensor();
        self.clt = out[self.enc_idx.next_clt].clone().into_tensor();
        self.clcl = out[self.enc_idx.next_clcl].cast_to::<i64>()?.into_owned();
        Ok(())
    }

    // Text from a raw token-id list (SentencePiece ▁ → space), for the batch harness.
    pub fn ids_to_text(&self, ids: &[usize]) -> String {
        let mut s = String::new();
        for &id in ids {
            if id < self.tokens.len() {
                s.push_str(&self.tokens[id]);
            }
        }
        s.replace('\u{2581}', " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn chunk_start(&self, idx: usize) -> usize {
        if idx == 0 {
            0
        } else {
            (self.m.first_chunk_frames - self.m.pre_encode_overlap)
                + (idx - 1) * self.m.shift_frames
        }
    }

    fn drain(&mut self, flush: bool) -> TractResult<()> {
        let mut fired = 0;
        while fired < 16 {
            let start = self.chunk_start(self.chunk_index);
            let target = if self.chunk_index == 0 {
                self.m.first_chunk_frames
            } else {
                self.m.chunk_frames
            };
            let avail_abs = self.base_frame + self.pending.len() / HOP + 1;
            if avail_abs < start + target && !flush {
                break;
            }
            if avail_abs <= start {
                break;
            }
            let true_len = target.min(avail_abs - start);
            if true_len == 0 {
                break;
            }
            let feats = self.mel_chunk(start);
            self.step(&feats, true_len as i64)?;
            self.chunk_index += 1;
            fired += 1;
            self.trim_buffer();
        }
        Ok(())
    }

    // Un-normalized log-mel for absolute frames [start, start+chunk_frames) as [80, chunk], zero-padded.
    fn mel_chunk(&self, start_abs: usize) -> Vec<f32> {
        let chunk = self.m.chunk_frames;
        let mut out = vec![0f32; N_MELS * chunk];
        let rel_start = start_abs - self.base_frame;
        let stride = rel_start + chunk + 4;
        if self.pending.is_empty() {
            return out;
        }
        let (feats, avail) = self.ex.compute(&self.pending, stride);
        for m in 0..N_MELS {
            for t in 0..chunk {
                let src_t = rel_start + t;
                if src_t < avail {
                    out[m * chunk + t] = feats[m * stride + src_t];
                }
            }
        }
        out
    }

    fn step(&mut self, features: &[f32], length: i64) -> TractResult<()> {
        let chunk = self.m.chunk_frames;
        let mut audio = Tensor::from_shape(&[1usize, 80, chunk], features)?;
        if self.f16 {
            audio = audio.cast_to::<tract_core::prelude::f16>()?.into_owned();
        }
        let len_t = tensor1(&[length]);
        let out = self.model.run(tvec!(
            audio.into(),
            len_t.into(),
            self.clc.clone().into(),
            self.clt.clone().into(),
            self.clcl.clone().into()
        ))?;

        if self.decoder.is_some() {
            self.decode_rnnt(&out)?;
        } else {
            self.decode_ctc(&out)?;
        }

        // Thread next-caches back in, resolved by name (order differs between CTC-only & RNNT exports).
        self.clc = out[self.enc_idx.next_clc].clone().into_tensor();
        self.clt = out[self.enc_idx.next_clt].clone().into_tensor();
        self.clcl = out[self.enc_idx.next_clcl].cast_to::<i64>()?.into_owned();
        Ok(())
    }

    // Greedy CTC over the encoder's `ctc_logprobs` head: argmax per frame, blank-collapse + dedup.
    fn decode_ctc(&mut self, out: &[TValue]) -> TractResult<()> {
        const DEDUP_GAP: usize = 6;
        let lp = out[self.enc_idx.ctc].cast_to::<f32>()?.into_owned();
        let shape = lp.shape().to_vec(); // [1, T', 1025]
        let frames = shape[1];
        let view = lp.to_plain_array_view::<f32>()?;
        let mut chunk_emitted = false;
        if let Some(flat) = view.as_slice() {
            let n = frames.min(MAX_OUT_FRAMES);
            for f in 0..n {
                let base = f * VOCAB;
                let mut best = 0usize;
                let mut bv = flat[base];
                for k in 1..VOCAB {
                    let v = flat[base + k];
                    if v > bv {
                        bv = v;
                        best = k;
                    }
                }
                self.frames_since_emit += 1;
                if best as i64 != self.prev_token && best != BLANK_ID {
                    if best as i64 == self.last_emit_tok
                        && self.frames_since_emit <= DEDUP_GAP
                        && !self.prev_chunk_had_emit
                    {
                        self.frames_since_emit = 0;
                    } else {
                        self.emitted.push(best);
                        self.last_emit_tok = best as i64;
                        self.frames_since_emit = 0;
                        chunk_emitted = true;
                    }
                }
                self.prev_token = best as i64;
                if let Some(buf) = &mut self.lp_buf {
                    buf.push(flat[base..base + VOCAB].to_vec());
                }
            }
        }
        self.prev_chunk_had_emit = chunk_emitted;
        Ok(())
    }

    /// Enable/disable deterministic CTC log-prob capture (for the offline over-boost test).
    pub fn set_collect_logprobs(&mut self, on: bool) {
        self.lp_buf = if on { Some(Vec::new()) } else { None };
    }
    /// Take the captured [frames][VOCAB] log-probs (leaves capture enabled+empty).
    pub fn take_logprobs(&mut self) -> Vec<Vec<f32>> {
        match &mut self.lp_buf {
            Some(b) => std::mem::take(b),
            None => Vec::new(),
        }
    }

    // Greedy RNNT over the encoder's `enc_hidden[1,T',256]` head, driving the prednet+joint STEP
    // graph. Per encoder frame t: repeatedly run the step with the current prednet state; on blank
    // (or MAX_SYMBOLS) advance t WITHOUT touching state; otherwise emit, adopt the new h/c, and set
    // last_token = k. State (h/c/last_token) persists across chunks — only reset() clears it.
    fn decode_rnnt(&mut self, out: &[TValue]) -> TractResult<()> {
        let dec = self.decoder.clone().ok_or_else(|| {
            TractError::msg("decode_rnnt called without an attached RNNT decoder")
        })?;
        // The RNNT head REQUIRES a named enc_hidden output; refuse rather than positionally guessing
        // index 0 (which on a CTC-only/mis-resolved graph is a 1025-wide ctc tensor and would be fed
        // into the 256-wide decoder-step input).
        let ehx = self.enc_idx.enc_hidden.ok_or_else(|| {
            TractError::msg("encoder graph has no 'enc_hidden' output — cannot run the RNNT head")
        })?;
        let enc = out[ehx].cast_to::<f32>()?.into_owned();
        let shape = enc.shape().to_vec(); // [1, T', D]
        let frames = shape[1];
        let d = shape[2];
        // Bound frames by encoded_len (a padded last chunk emits fewer valid frames), matching the
        // Python reference which slices enc[:, :encoded_len]. Absent name => decode all frames.
        let valid = match self.encoded_len(out)? {
            Some(n) => {
                if n == 0 && frames > 0 {
                    // A named encoded_len of 0 while the tensor carries frames is unusual (a genuine
                    // fully-padded tail chunk gives length 0). Surface it instead of silently
                    // dropping the whole chunk, then honour the encoder (skip the padded frames).
                    eprintln!(
                        "[rnnt] encoded_len=0 for a chunk with {frames} enc frames — treating as fully padded (0 valid)"
                    );
                }
                n.min(frames)
            }
            None => frames, // no encoded_len output on this graph → decode every frame
        };
        let enc_v = enc.to_plain_array_view::<f32>()?;
        let flat = match enc_v.as_slice() {
            Some(s) => s,
            None => return Ok(()), // non-contiguous shouldn't happen for a fresh cast tensor
        };
        for t in 0..valid {
            let frame = &flat[t * d..(t + 1) * d];
            let enc_t = Tensor::from_shape(&[1usize, d], frame)?;
            let mut symbols = 0usize;
            loop {
                let tok = tensor1(&[self.last_token]);
                let res = dec.run(tvec!(
                    enc_t.clone().into(),
                    tok.into(),
                    self.h.clone().into(),
                    self.c.clone().into()
                ))?;
                // logits[1,1025] (RAW — argmax invariant to log_softmax), h_out, c_out.
                let logits = res[0].cast_to::<f32>()?.into_owned();
                let lv = logits.to_plain_array_view::<f32>()?;
                let lf = lv.as_slice().ok_or_else(|| {
                    TractError::msg("decoder-step logits tensor is not contiguous")
                })?;
                let mut k = 0usize;
                let mut bv = lf[0];
                for j in 1..VOCAB {
                    if lf[j] > bv {
                        bv = lf[j];
                        k = j;
                    }
                }
                if k == BLANK_ID || symbols == MAX_SYMBOLS {
                    break; // advance t; do NOT update h/c/last_token on blank
                }
                self.emitted.push(k);
                self.last_token = k as i64;
                self.h = res[1].clone().into_tensor();
                self.c = res[2].clone().into_tensor();
                symbols += 1;
            }
        }
        Ok(())
    }

    // Read the encoder's `encoded_len` scalar, resolved by name (enc_idx.enc_len). Returns:
    //   Ok(None)      — the graph exposes no encoded_len output (CTC-only export) → caller uses all frames.
    //   Ok(Some(n))   — the named scalar's value (a genuine 0 for a fully-padded tail chunk is preserved).
    //   Err(..)       — the named output exists but couldn't be read as an i64 scalar (a real fault).
    fn encoded_len(&self, out: &[TValue]) -> TractResult<Option<usize>> {
        let idx = match self.enc_idx.enc_len {
            Some(i) => i,
            None => return Ok(None),
        };
        let t = out
            .get(idx)
            .ok_or_else(|| TractError::msg("encoded_len output index out of range"))?;
        let v = t.cast_to::<i64>()?;
        let view = v.to_plain_array_view::<i64>()?;
        let val = view
            .iter()
            .next()
            .ok_or_else(|| TractError::msg("encoded_len tensor is empty"))?;
        Ok(Some((*val).max(0) as usize))
    }

    fn trim_buffer(&mut self) {
        let next_start = self.chunk_start(self.chunk_index);
        let keep_from = next_start.saturating_sub(LEFT_CONTEXT);
        let drop_frames = keep_from.saturating_sub(self.base_frame);
        if drop_frames > 0 {
            let drop_samples = (drop_frames * HOP).min(self.pending.len());
            self.pending.drain(0..drop_samples);
            self.base_frame += drop_samples / HOP;
        }
    }

    fn all_words(&self) -> Vec<String> {
        let mut s = String::new();
        for &id in &self.emitted {
            if id < self.tokens.len() {
                s.push_str(&self.tokens[id]);
            }
        }
        s = s.replace('\u{2581}', " ");
        s.split_whitespace().map(str::to_string).collect()
    }

    fn decoded_text(&self) -> String {
        let w = self.all_words();
        if w.len() > self.committed_words {
            w[self.committed_words..].join(" ")
        } else {
            String::new()
        }
    }

    pub fn mark_committed(&mut self) {
        self.committed_words = self.all_words().len();
    }
}
