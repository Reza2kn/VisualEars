//! De-risk proof: the full pure-Rust ensemble. koochik streaming CTC log-probs (from tract int4)
//! + vosk-rust hotwords (pure-Rust Vosk, no libvosk) -> shenava-ctc-beam hotword-boosted decode.
//! Shows keyword recovery vs the greedy baseline.
//! cargo run --release --example rescore_proof -- <vosk_dir> <logprobs.bin> <clip16k.bin> <tokens.txt>
use shenava_ctc_beam::{CtcBeamDecoder, Hotwords};
use std::collections::HashSet;
use std::io::Read;

fn read_logprobs(path: &str) -> Vec<Vec<f32>> {
    let mut f = std::fs::File::open(path).unwrap();
    let mut hdr = [0u8; 8];
    f.read_exact(&mut hdr).unwrap();
    let t = i32::from_le_bytes(hdr[0..4].try_into().unwrap()) as usize;
    let v = i32::from_le_bytes(hdr[4..8].try_into().unwrap()) as usize;
    let mut raw = vec![0u8; t * v * 4];
    f.read_exact(&mut raw).unwrap();
    let flat: Vec<f32> = raw
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    (0..t).map(|i| flat[i * v..(i + 1) * v].to_vec()).collect()
}
fn read_pcm(path: &str) -> Vec<f32> {
    let mut f = std::fs::File::open(path).unwrap();
    let mut hdr = [0u8; 8];
    f.read_exact(&mut hdr).unwrap();
    let n = i32::from_le_bytes(hdr[4..8].try_into().unwrap()) as usize;
    let mut raw = vec![0u8; n * 4];
    f.read_exact(&mut raw).unwrap();
    raw.chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect()
}
fn read_labels(path: &str) -> Vec<String> {
    let txt = std::fs::read_to_string(path).unwrap();
    let mut v = vec![String::new(); 1025];
    for line in txt.lines() {
        if let Some(sp) = line.rfind(' ') {
            if let Ok(id) = line[sp + 1..].trim().parse::<usize>() {
                if id < 1025 {
                    v[id] = line[..sp].to_string();
                }
            }
        }
    }
    v[1024] = String::new(); // CTC blank
    v
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (vosk_dir, lp_path, pcm_path, tok_path) = (&a[1], &a[2], &a[3], &a[4]);
    let logprobs = read_logprobs(lp_path);
    let pcm = read_pcm(pcm_path);
    let labels = read_labels(tok_path);

    // pure-Rust Vosk second pass -> hotwords (>=3 chars, deduped)
    let rec = vosk_rust::Recognizer::load(vosk_dir).unwrap();
    let vosk_text = rec.recognize(&pcm);
    let mut seen = HashSet::new();
    let hotwords: Vec<String> = vosk_text
        .split_whitespace()
        .filter(|w| w.chars().count() >= 3 && seen.insert(w.to_string()))
        .map(|w| w.to_string())
        .collect();

    let dec = CtcBeamDecoder::new(labels);
    let greedy = dec.decode(
        &logprobs,
        &Hotwords::new(Vec::<String>::new(), 0.0),
        80,
        -5.0,
        -10.0,
    );
    let rescored = dec.decode(
        &logprobs,
        &Hotwords::new(hotwords.clone(), 10.0),
        80,
        -5.0,
        -10.0,
    );

    println!("VOSK    : {vosk_text}");
    println!("HOTWORDS: {hotwords:?}");
    println!("GREEDY  : {greedy}");
    println!("RESCORED: {rescored}");
}
