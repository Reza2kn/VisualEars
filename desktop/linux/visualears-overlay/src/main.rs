#[cfg(feature = "overlay")]
mod audio;
mod caption_render;
mod captioner;
#[cfg(feature = "control-panel")]
mod control_panel;
mod engine;
mod features;
mod model;
mod rescore;
mod style;
#[cfg(feature = "overlay")]
mod window;

use caption_render::{save_png, CaptionRenderer};

use captioner::{LiveCaptioner, TranscriptState};
use engine::StreamingRecognizer;
use features::{load_mel_filters, FeatureExtractor};
use rescore::StaticRescorer;
use style::{parse_style_args, OverlayStyle};

fn build_recognizer(
    model_key: &str,
    onnx: &str,
    tokens_path: &str,
    mel_path: &str,
    f16: bool,
) -> Result<StreamingRecognizer, Box<dyn std::error::Error>> {
    let ex = FeatureExtractor::new(load_mel_filters(mel_path)?);
    let tokens = model::load_tokens(tokens_path)?;
    let m = model::named(model_key);
    Ok(StreamingRecognizer::load(onnx, m, ex, tokens, f16)?)
}

fn static_rescorer_from_args(
    args: &[String],
    tokens_path: &str,
) -> Result<Option<StaticRescorer>, Box<dyn std::error::Error>> {
    args.iter()
        .position(|x| x == "--hotwords")
        .and_then(|j| args.get(j + 1))
        .map(|p| StaticRescorer::from_files(tokens_path, p))
        .transpose()
}

fn read_wav_mono_16k(path: &str) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    // Surface (don't swallow) per-sample decode errors: `collect::<Result<_,_>>()?` fails the whole
    // read on the first bad sample instead of silently returning a truncated/empty clip, which would
    // corrupt the batch/selftest harness's WER/parity numbers.
    let raw: Vec<f32> = match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Float, _) => {
            reader.samples::<f32>().collect::<Result<Vec<_>, _>>()?
        }
        (hound::SampleFormat::Int, 16) => reader
            .samples::<i16>()
            .map(|s| s.map(|v| v as f32 / 32768.0))
            .collect::<Result<Vec<_>, _>>()?,
        (hound::SampleFormat::Int, _) => reader
            .samples::<i32>()
            .map(|s| s.map(|v| v as f32 / 2_147_483_648.0))
            .collect::<Result<Vec<_>, _>>()?,
    };
    let mono = if spec.channels > 1 {
        raw.iter()
            .step_by(spec.channels as usize)
            .copied()
            .collect()
    } else {
        raw
    };
    if spec.sample_rate != 16_000 {
        eprintln!(
            "WARNING: wav is {} Hz, expected 16000 — decode will be wrong",
            spec.sample_rate
        );
    }
    Ok(mono)
}

fn selftest(args: &[String], f16: bool) -> Result<(), Box<dyn std::error::Error>> {
    let (model_key, onnx, tokens_path, mel_path, wav_path) =
        (&args[0], &args[1], &args[2], &args[3], &args[4]);
    let m = model::named(model_key);
    let mut rec = build_recognizer(model_key, onnx, tokens_path, mel_path, f16)?;

    let pcm = read_wav_mono_16k(wav_path)?;
    eprintln!(
        "model={} ({}) samples={} f16={}",
        model_key,
        m.display,
        pcm.len(),
        f16
    );

    let t0 = std::time::Instant::now();
    let mut idx = 0;
    let mut text = String::new();
    while idx < pcm.len() {
        let end = (idx + 1600).min(pcm.len()); // 100ms chunks, mirrors a live tick
        text = rec.accept(&pcm[idx..end])?;
        idx = end;
    }
    let flushed = rec.flush()?;
    if !flushed.is_empty() {
        text = flushed;
    }
    let dt = t0.elapsed().as_secs_f32();
    let audio_s = (pcm.len() as f32 / 16_000.0).max(1e-3);
    eprintln!(
        "INFERENCE: total={:.0}ms  RTF={:.2}  audio={:.1}s",
        dt * 1000.0,
        dt / audio_s,
        audio_s
    );
    println!("TRANSCRIPT: {text}");
    Ok(())
}

// Single-wav RNNT selftest: encoder + decoder-step graph, greedy transducer decode via the
// reference-faithful batch chunking (independent 17920-sample chunks).
//   --selftest-rnnt <model> <encoder.onnx> <decoder_step.onnx> <tokens> <mel> <wav>
fn selftest_rnnt(args: &[String], f16: bool) -> Result<(), Box<dyn std::error::Error>> {
    let (model_key, enc, dec, tokens_path, mel_path, wav_path) =
        (&args[0], &args[1], &args[2], &args[3], &args[4], &args[5]);
    let m = model::named(model_key);
    let mut rec = build_recognizer(model_key, enc, tokens_path, mel_path, f16)?;
    rec.attach_rnnt_decoder(dec)?;

    let pcm = read_wav_mono_16k(wav_path)?;
    eprintln!(
        "[RNNT] model={} ({}) samples={} f16={}",
        model_key,
        m.display,
        pcm.len(),
        f16
    );
    let t0 = std::time::Instant::now();
    let ids = rec.decode_full_rnnt(&pcm)?;
    let text = rec.ids_to_text(&ids);
    let dt = t0.elapsed().as_secs_f32();
    let audio_s = (pcm.len() as f32 / 16_000.0).max(1e-3);
    eprintln!(
        "INFERENCE: total={:.0}ms  RTF={:.2}  audio={:.1}s",
        dt * 1000.0,
        dt / audio_s,
        audio_s
    );
    eprintln!("IDS: {ids:?}");
    println!("TRANSCRIPT: {text}");
    Ok(())
}

// Debug: dump one chunk's log-mel to compare against NeMo's preprocessor.
//   --dump-mel <model> <tokens> <mel> <wav> <chunk_idx>  → prints 80xW as CSV rows to stdout
fn dump_mel(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (model_key, tokens_path, mel_path, wav_path, idx) = (
        &args[0],
        &args[1],
        &args[2],
        &args[3],
        args[4].parse::<usize>()?,
    );
    // encoder path is irrelevant here; reuse the mel filters via a throwaway recognizer build is
    // overkill, so build the FeatureExtractor directly.
    let ex = features::FeatureExtractor::new(load_mel_filters(mel_path)?);
    let _ = (model_key, tokens_path);
    let pcm = read_wav_mono_16k(wav_path)?;
    // 17920-sample chunk, 113 mel frames — mirror decode_full_rnnt.
    const CS: usize = 112 * 160;
    const W: usize = 113;
    let s = idx * CS;
    let mut chunk = vec![0f32; CS];
    if s < pcm.len() {
        let v = (pcm.len() - s).min(CS);
        chunk[..v].copy_from_slice(&pcm[s..s + v]);
    }
    let (feats, _) = ex.compute(&chunk, W); // feature-major [80*113]
    for row in 0..80 {
        let vals: Vec<String> = (0..W)
            .map(|t| format!("{:.4}", feats[row * W + t]))
            .collect();
        println!("{}", vals.join(","));
    }
    Ok(())
}

// Batch RNNT over a manifest.jsonl (one {"wav":...,"text":...} object per line). Writes a
// {wav_basename: transcript} JSON map to <out.json> and prints aggregate RTF to stderr. This is the
// tract-side of the parity gate vs the Python preds_stream_rnnt_deg.json.
//   --batch-rnnt <model> <encoder.onnx> <decoder_step.onnx> <tokens> <mel> <manifest.jsonl> <out.json>
fn batch_rnnt(args: &[String], f16: bool) -> Result<(), Box<dyn std::error::Error>> {
    let (model_key, enc, dec, tokens_path, mel_path, manifest, out_json) = (
        &args[0], &args[1], &args[2], &args[3], &args[4], &args[5], &args[6],
    );
    let mut rec = build_recognizer(model_key, enc, tokens_path, mel_path, f16)?;
    rec.attach_rnnt_decoder(dec)?;
    eprintln!("[BATCH-RNNT] enc={} dec={} f16={}", enc, dec, f16);

    let manifest_txt = std::fs::read_to_string(manifest)?;
    let mut results: Vec<(String, String)> = Vec::new();
    let (mut tot_audio, mut tot_infer) = (0.0f64, 0.0f64);
    let mut n = 0usize;
    for line in manifest_txt.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(line)?;
        let wav_path = v
            .get("wav")
            .and_then(|x| x.as_str())
            .ok_or("manifest row missing 'wav'")?;
        let base = std::path::Path::new(wav_path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(wav_path)
            .to_string();
        let pcm = read_wav_mono_16k(wav_path)?;
        let t0 = std::time::Instant::now();
        let ids = rec.decode_full_rnnt(&pcm)?;
        let text = rec.ids_to_text(&ids);
        tot_infer += t0.elapsed().as_secs_f64();
        tot_audio += pcm.len() as f64 / 16_000.0;
        results.push((base, text));
        n += 1;
        if n % 25 == 0 {
            eprintln!("  {n} done…");
        }
    }
    let map: serde_json::Map<String, serde_json::Value> = results
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::String(v)))
        .collect();
    std::fs::write(
        out_json,
        serde_json::to_string_pretty(&serde_json::Value::Object(map))?,
    )?;
    let rtf = if tot_audio > 0.0 {
        tot_infer / tot_audio
    } else {
        0.0
    };
    eprintln!(
        "[BATCH-RNNT] wrote {out_json}  n={n}  audio={:.1}s  infer={:.1}s  RTF={:.3}",
        tot_audio, tot_infer, rtf
    );
    Ok(())
}

// Batch CTC over a manifest.jsonl — the DEPLOYED Persian koochik_hd path (tract, int4, cache-aware
// streaming, CTC head). Reuses the exact app streaming path (accept 100ms ticks + flush) with reset()
// between clips (no per-clip model reload). Manifest rows: {"wav"|"audio_filepath":..., "id"?, "ref"|"text"?}.
// Writes {id, reference, hypothesis} JSONL for the S3 dropped-word analysis.
//   --batch-ctc <model> <onnx> <tokens> <mel> <manifest.jsonl> <out.jsonl>
fn batch_ctc(args: &[String], f16: bool) -> Result<(), Box<dyn std::error::Error>> {
    let (model_key, onnx, tokens_path, mel_path, manifest, out_path) =
        (&args[0], &args[1], &args[2], &args[3], &args[4], &args[5]);
    let mut rec = build_recognizer(model_key, onnx, tokens_path, mel_path, f16)?;
    eprintln!("[BATCH-CTC] model={} onnx={} f16={}", model_key, onnx, f16);

    let manifest_txt = std::fs::read_to_string(manifest)?;
    let mut out = String::new();
    let (mut tot_audio, mut tot_infer) = (0.0f64, 0.0f64);
    let mut n = 0usize;
    for line in manifest_txt.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(line)?;
        let wav_path = v
            .get("wav")
            .and_then(|x| x.as_str())
            .or_else(|| v.get("audio_filepath").and_then(|x| x.as_str()))
            .ok_or("manifest row missing 'wav'/'audio_filepath'")?;
        let id = v
            .get("id")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| n.to_string());
        let reference = v
            .get("ref")
            .and_then(|x| x.as_str())
            .or_else(|| v.get("text").and_then(|x| x.as_str()))
            .unwrap_or("")
            .to_string();
        let pcm = read_wav_mono_16k(wav_path)?;
        rec.reset()?;
        let t0 = std::time::Instant::now();
        let mut text = String::new();
        let mut idx = 0;
        while idx < pcm.len() {
            let end = (idx + 1600).min(pcm.len()); // 100ms ticks, mirrors selftest / a live tick
            text = rec.accept(&pcm[idx..end])?;
            idx = end;
        }
        let flushed = rec.flush()?;
        if !flushed.is_empty() {
            text = flushed;
        }
        tot_infer += t0.elapsed().as_secs_f64();
        tot_audio += pcm.len() as f64 / 16_000.0;
        let row = serde_json::json!({"id": id, "reference": reference, "hypothesis": text});
        out.push_str(&serde_json::to_string(&row)?);
        out.push('\n');
        n += 1;
        if n % 100 == 0 {
            eprintln!("  {n} done…");
        }
    }
    std::fs::write(out_path, out)?;
    let rtf = if tot_audio > 0.0 {
        tot_infer / tot_audio
    } else {
        0.0
    };
    eprintln!(
        "[BATCH-CTC] wrote {out_path}  n={n}  audio={:.1}s  infer={:.1}s  RTF={:.3}",
        tot_audio, tot_infer, rtf
    );
    Ok(())
}

// Dump the DETERMINISTIC CTC log-probs for one wav (the synchronous accept path — no real-time thread,
// so it's reproducible, unlike --replay-live). Output = the shenava-ctc-beam static_proof format:
// [T i32 LE][VOCAB i32 LE][T*VOCAB f32 LE]. Use it to validate hotword over-boost offline + stably.
//   --dump-logprobs <model> <onnx> <tokens> <mel> <wav> <out.bin>
fn dump_logprobs(args: &[String], f16: bool) -> Result<(), Box<dyn std::error::Error>> {
    let (model_key, onnx, tokens_path, mel_path, wav_path, out_path) =
        (&args[0], &args[1], &args[2], &args[3], &args[4], &args[5]);
    let mut rec = build_recognizer(model_key, onnx, tokens_path, mel_path, f16)?;
    rec.set_collect_logprobs(true);
    let pcm = read_wav_mono_16k(wav_path)?;
    let mut idx = 0;
    while idx < pcm.len() {
        let end = (idx + 1600).min(pcm.len());
        rec.accept(&pcm[idx..end])?;
        idx = end;
    }
    rec.flush()?;
    let lp = rec.take_logprobs(); // [T][VOCAB]
    let t = lp.len();
    let v = if t > 0 { lp[0].len() } else { 0 };
    let mut buf: Vec<u8> = Vec::with_capacity(8 + t * v * 4);
    buf.extend_from_slice(&(t as i32).to_le_bytes());
    buf.extend_from_slice(&(v as i32).to_le_bytes());
    for row in &lp {
        for &x in row {
            buf.extend_from_slice(&x.to_le_bytes());
        }
    }
    std::fs::write(out_path, &buf)?;
    eprintln!("[DUMP-LOGPROBS] {out_path}  T={t} V={v}");
    Ok(())
}

// Warm resident server: load the model ONCE (pays the ~72s into_optimized cost a single
// time), then stream requests on stdin. Each request line = "<wav_path>\t<out_logprobs.bin>".
// Replies one line per request: "OK\t<out>" or "ERR\t<message>". Emits "READY" once loaded.
// Requests are handled strictly one at a time (caller serializes), keeping RAM ~one model.
//   --serve <model> <onnx> <tokens> <mel>
fn serve(args: &[String], f16: bool) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{BufRead, Write};
    let (model_key, onnx, tokens_path, mel_path) = (&args[0], &args[1], &args[2], &args[3]);
    let mut rec = build_recognizer(model_key, onnx, tokens_path, mel_path, f16)?;
    rec.set_collect_logprobs(true);
    let mut stdout = std::io::stdout();
    eprintln!("[SERVE] model loaded, ready");
    writeln!(stdout, "READY")?;
    stdout.flush()?;
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, '\t');
        let wav = parts.next().unwrap_or("").to_string();
        let out = parts.next().unwrap_or("").to_string();
        let res = (|| -> Result<(), Box<dyn std::error::Error>> {
            rec.reset()?;
            let pcm = read_wav_mono_16k(&wav)?;
            let mut idx = 0;
            while idx < pcm.len() {
                let end = (idx + 1600).min(pcm.len());
                rec.accept(&pcm[idx..end])?;
                idx = end;
            }
            rec.flush()?;
            let lp = rec.take_logprobs();
            let t = lp.len();
            let v = if t > 0 { lp[0].len() } else { 0 };
            let mut buf: Vec<u8> = Vec::with_capacity(8 + t * v * 4);
            buf.extend_from_slice(&(t as i32).to_le_bytes());
            buf.extend_from_slice(&(v as i32).to_le_bytes());
            for row in &lp {
                for &x in row {
                    buf.extend_from_slice(&x.to_le_bytes());
                }
            }
            std::fs::write(&out, &buf)?;
            Ok(())
        })();
        match res {
            Ok(()) => writeln!(stdout, "OK\t{out}")?,
            Err(e) => writeln!(stdout, "ERR\t{}", e.to_string().replace(['\n', '\t'], " "))?,
        }
        stdout.flush()?;
    }
    Ok(())
}

// Batch version of --dump-logprobs: load the model ONCE, dump deterministic CTC log-probs for every clip
// in a manifest ({"wav"|"audio_filepath":..., "id"?}) to <out_dir>/<id>.bin. For the 69-clip over-boost
// sweep (per-clip model reload is too slow).
//   --batch-dump-logprobs <model> <onnx> <tokens> <mel> <manifest.jsonl> <out_dir>
fn batch_dump_logprobs(args: &[String], f16: bool) -> Result<(), Box<dyn std::error::Error>> {
    let (model_key, onnx, tokens_path, mel_path, manifest, out_dir) =
        (&args[0], &args[1], &args[2], &args[3], &args[4], &args[5]);
    let mut rec = build_recognizer(model_key, onnx, tokens_path, mel_path, f16)?;
    rec.set_collect_logprobs(true);
    std::fs::create_dir_all(out_dir)?;
    let mut n = 0usize;
    for line in std::fs::read_to_string(manifest)?.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(line)?;
        let wav = v
            .get("wav")
            .and_then(|x| x.as_str())
            .or_else(|| v.get("audio_filepath").and_then(|x| x.as_str()))
            .ok_or("row missing wav")?;
        let id = v
            .get("id")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                std::path::Path::new(wav)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("clip")
                    .to_string()
            });
        let pcm = read_wav_mono_16k(wav)?;
        rec.reset()?;
        rec.set_collect_logprobs(true); // reset() clears the buffer setting; re-enable
        let mut idx = 0;
        while idx < pcm.len() {
            let end = (idx + 1600).min(pcm.len());
            rec.accept(&pcm[idx..end])?;
            idx = end;
        }
        rec.flush()?;
        let lp = rec.take_logprobs();
        let (t, vv) = (lp.len(), if lp.is_empty() { 0 } else { lp[0].len() });
        let mut buf = Vec::with_capacity(8 + t * vv * 4);
        buf.extend_from_slice(&(t as i32).to_le_bytes());
        buf.extend_from_slice(&(vv as i32).to_le_bytes());
        for row in &lp {
            for &x in row {
                buf.extend_from_slice(&x.to_le_bytes());
            }
        }
        std::fs::write(format!("{out_dir}/{id}.bin"), &buf)?;
        n += 1;
        if n % 20 == 0 {
            eprintln!("  dumped {n}…");
        }
    }
    eprintln!("[BATCH-DUMP-LOGPROBS] {n} clips -> {out_dir}");
    Ok(())
}

fn replay_live(args: &[String], f16: bool) -> Result<(), Box<dyn std::error::Error>> {
    let (model_key, onnx, tokens_path, mel_path, wav_path) =
        (&args[0], &args[1], &args[2], &args[3], &args[4]);
    let rec = build_recognizer(model_key, onnx, tokens_path, mel_path, f16)?;
    let all_args: Vec<String> = std::env::args().collect();
    let mut cap = match static_rescorer_from_args(&all_args, tokens_path)? {
        Some(r) => LiveCaptioner::with_static_guide(rec, r),
        None => LiveCaptioner::new(rec),
    };
    let mut ts = TranscriptState::default();
    let pcm = read_wav_mono_16k(wav_path)?;
    eprintln!(
        "replay-live: {:.1}s through the real captioner (rolling window + pause-finalize)",
        pcm.len() as f32 / 16_000.0
    );

    let mut last = String::new();
    let emit = |ev: &captioner::CaptionEvent, ts: &mut TranscriptState, last: &mut String| {
        ts.apply(ev);
        let screen = ts.visible_lines(2).join("\n");
        if screen != *last {
            *last = screen.clone();
            println!(
                "{} «{}»",
                if ev.is_final { "FINAL" } else { "live " },
                screen.replace('\n', " ⏎ ")
            );
        }
    };

    let mut idx = 0;
    while idx < pcm.len() {
        let end = (idx + 1600).min(pcm.len());
        for ev in cap.feed_batch(&pcm[idx..end]) {
            emit(&ev, &mut ts, &mut last);
        }
        idx = end;
    }
    for _ in 0..15 {
        // trailing silence so the last utterance finalizes
        for ev in cap.feed_batch(&[]) {
            emit(&ev, &mut ts, &mut last);
        }
    }
    Ok(())
}

fn render_caption_test(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (text, out) = (&args[0], &args[1]);
    let (w, h, font_px) = (1400usize, 340usize, 66f32);
    let lines = TranscriptState::reflow(text, 2).join("\n");
    let mut r = CaptionRenderer::new();
    let px = r.render(&lines, w, h, font_px, (20, 26, 36, 255)); // opaque dark bg for the PNG test
    save_png(&px, w as u32, h as u32, out)?;
    eprintln!("wrote {out} ({w}x{h})");
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let f16 = args.iter().any(|a| a == "--f16");
    if let Some(i) = args.iter().position(|a| a == "--export-nnef") {
        if args.len() >= i + 4 {
            let spec = model::named(&args[i + 1]);
            return Ok(StreamingRecognizer::export_nnef_to_tar(&args[i + 2], &spec, f16, &args[i + 3])?);
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--render-caption") {
        if args.len() >= i + 3 {
            return render_caption_test(&args[i + 1..i + 3]);
        }
    }
    let overlay_style: OverlayStyle = parse_style_args(&args);
    #[cfg(feature = "control-panel")]
    if let Some(i) = args.iter().position(|a| a == "--control") {
        if args.len() >= i + 5 {
            let hotwords = args
                .iter()
                .position(|x| x == "--hotwords")
                .and_then(|j| args.get(j + 1))
                .cloned();
            let model_url = args
                .iter()
                .position(|x| x == "--model-url")
                .and_then(|j| args.get(j + 1))
                .cloned()
                .unwrap_or_else(|| control_panel::DEFAULT_MODEL_URL.to_string());
            return control_panel::run(control_panel::ControlArgs {
                model_key: args[i + 1].clone(),
                model_path: args[i + 2].clone(),
                tokens_path: args[i + 3].clone(),
                mel_path: args[i + 4].clone(),
                hotwords_path: hotwords,
                model_url,
            });
        }
    }
    #[cfg(feature = "overlay")]
    if let Some(i) = args.iter().position(|a| a == "--overlay-demo") {
        if let Some(text) = args.get(i + 1) {
            return window::run_static_caption(text, overlay_style);
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--selftest") {
        if args.len() >= i + 6 {
            return selftest(&args[i + 1..i + 6], f16);
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--selftest-rnnt") {
        if args.len() >= i + 7 {
            return selftest_rnnt(&args[i + 1..i + 7], f16);
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--batch-rnnt") {
        if args.len() >= i + 8 {
            return batch_rnnt(&args[i + 1..i + 8], f16);
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--batch-ctc") {
        if args.len() >= i + 7 {
            return batch_ctc(&args[i + 1..i + 7], f16);
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--serve") {
        if args.len() >= i + 5 {
            return serve(&args[i + 1..i + 5], f16);
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--dump-logprobs") {
        if args.len() >= i + 7 {
            return dump_logprobs(&args[i + 1..i + 7], f16);
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--batch-dump-logprobs") {
        if args.len() >= i + 7 {
            return batch_dump_logprobs(&args[i + 1..i + 7], f16);
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--dump-mel") {
        if args.len() >= i + 6 {
            return dump_mel(&args[i + 1..i + 6]);
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--replay-live") {
        if args.len() >= i + 6 {
            return replay_live(&args[i + 1..i + 6], f16);
        }
    }
    #[cfg(feature = "overlay")]
    if args.iter().any(|a| a == "--list-audio") {
        return audio::list_input_devices();
    }
    #[cfg(feature = "overlay")]
    if let Some(i) = args.iter().position(|a| a == "--overlay-standby") {
        if args.len() >= i + 5 {
            let a = &args[i + 1..i + 5];
            let rec = build_recognizer(&a[0], &a[1], &a[2], &a[3], f16)?;
            let dev = args
                .iter()
                .position(|x| x == "--device")
                .and_then(|j| args.get(j + 1))
                .map(|s| s.as_str());
            let rescorer = static_rescorer_from_args(&args, &a[2])?;
            return window::run_standby(rec, rescorer, overlay_style, dev);
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--overlay") {
        if args.len() >= i + 5 {
            let a = &args[i + 1..i + 5];
            let rec = build_recognizer(&a[0], &a[1], &a[2], &a[3], f16)?;
            // optional `--device "<name substring>"` — e.g. --device "BlackHole" to caption SYSTEM audio
            let dev = args
                .iter()
                .position(|x| x == "--device")
                .and_then(|j| args.get(j + 1))
                .map(|s| s.as_str());
            let (tx, rx) = std::sync::mpsc::channel();
            let _capture = audio::start(tx, dev)?; // live while the event loop runs
            eprintln!(
                "Shenava running — audio @ {} Hz. Ctrl-C to quit.",
                _capture.source_rate
            );
            let rescorer = static_rescorer_from_args(&args, &a[2])?;
            return window::run(rec, rx, rescorer, overlay_style);
        }
    }
    // Windowed replay: feed a wav (real time) into the overlay instead of the mic — lets the actual
    // window be screenshotted under Xvfb on a mic-less box.
    #[cfg(feature = "overlay")]
    if let Some(i) = args.iter().position(|a| a == "--overlay-wav") {
        if args.len() >= i + 6 {
            let a = &args[i + 1..i + 6];
            let rec = build_recognizer(&a[0], &a[1], &a[2], &a[3], f16)?;
            let pcm = read_wav_mono_16k(&a[4])?;
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let mut idx = 0;
                while idx < pcm.len() {
                    let end = (idx + 1600).min(pcm.len());
                    if tx.send(pcm[idx..end].to_vec()).is_err() {
                        break;
                    }
                    idx = end;
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            });
            let rescorer = static_rescorer_from_args(&args, &a[2])?;
            return window::run(rec, rx, rescorer, overlay_style);
        }
    }
    eprintln!("usage:");
    eprintln!("  visualears-overlay --overlay      <model> <onnx> <tokens> <mel> [--hotwords hotwords_fa.txt]  # live mic overlay");
    eprintln!("  visualears-overlay --overlay-standby <model> <onnx> <tokens> <mel> [--device s]  # persist, hidden until 'show' on stdin");
    eprintln!("    style: [--animation slide|pop|karaoke|typewriter] [--vertical-position 0..1] [--visible-lines 1..4] [--font-size 38..92] [--max-width 0.5..0.94] [--shadow-blur 0..44] [--shadow-opacity 0..1] [--shadow-lift -12..18]");
    eprintln!("  visualears-overlay --control      <model> <onnx> <tokens> <mel> [--hotwords hotwords_fa.txt] [--model-url URL]  # control panel");
    eprintln!("  visualears-overlay --overlay-demo \"<text>\"                         # show demo overlay window");
    eprintln!("  visualears-overlay --selftest     <model> <onnx> <tokens> <mel> <wav>      # headless CTC decode");
    eprintln!("  visualears-overlay --selftest-rnnt <model> <enc.onnx> <dec.onnx> <tokens> <mel> <wav>          # RNNT decode");
    eprintln!("  visualears-overlay --batch-rnnt   <model> <enc.onnx> <dec.onnx> <tokens> <mel> <manifest> <out.json>  # RNNT batch");
    eprintln!("  visualears-overlay --replay-live  <model> <onnx> <tokens> <mel> <wav>      # headless caption timeline");
    eprintln!("  visualears-overlay --render-caption \"<text>\" <out.png>                     # headless render test");
    Ok(())
}
