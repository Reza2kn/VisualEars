//! Static-hotword proof: NO Vosk. A curated word list boosts the CTC beam over koochik's buffered
//! log-probs — the lightweight keyword recovery for Koochik-solo (32-bit TV).
//! cargo run --release --example static_proof -- <logprobs.bin> <tokens.txt> <wordlist.txt>
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
    v[1024] = String::new();
    v
}
fn read_wordlist(path: &str) -> Vec<String> {
    let txt = std::fs::read_to_string(path).unwrap();
    let mut seen = HashSet::new();
    txt.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim().to_string())
        .filter(|w| w.chars().count() >= 3 && seen.insert(w.clone()))
        .collect()
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let logprobs = read_logprobs(&a[1]);
    let labels = read_labels(&a[2]);
    let words = read_wordlist(&a[3]);
    let weight: f32 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(10.0);
    let dec = CtcBeamDecoder::new(labels);
    let hw = Hotwords::new(words.clone(), weight); // build once, reuse
    let greedy = dec.decode(
        &logprobs,
        &Hotwords::new(Vec::<String>::new(), 0.0),
        80,
        -5.0,
        -10.0,
    );
    let boosted = dec.decode(&logprobs, &hw, 80, -5.0, -10.0);
    println!("wordlist: {} terms, weight {weight}", words.len());
    println!("GREEDY : {greedy}");
    println!("STATIC : {boosted}");
}
