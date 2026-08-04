//! Ensemble comparison: decode CTC logprobs with a given hotword set. Args:
//!   <labels.json> <logprobs.bin> <hw.jsonl> <out.jsonl> [hotword_weight=10] [canon=1]
//! logprobs.bin = [u32 N] then N × ([u32 T][u32 V][f32 T*V]); hw.jsonl in the same clip order.
//! `canon=1` applies the deterministic orthographic canonicalizer to the output.
use shenava_ctc_beam::{canonicalize, CtcBeamDecoder, Hotwords};
use std::io::{BufRead, Read, Write};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let labels: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(&a[1]).unwrap()).unwrap();
    let dec = CtcBeamDecoder::new(labels);
    let weight: f32 = a.get(5).and_then(|s| s.parse().ok()).unwrap_or(10.0);
    let canon: bool = a.get(6).map(|s| s != "0").unwrap_or(true);

    let hws: Vec<(String, Vec<String>)> =
        std::io::BufReader::new(std::fs::File::open(&a[3]).unwrap())
            .lines()
            .map(|l| {
                let v: serde_json::Value = serde_json::from_str(&l.unwrap()).unwrap();
                let id = v["id"].as_str().unwrap().to_string();
                let hw = v["hw"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_str().unwrap().to_string())
                    .collect();
                (id, hw)
            })
            .collect();

    let mut f = std::io::BufReader::new(std::fs::File::open(&a[2]).unwrap());
    let mut u = [0u8; 4];
    f.read_exact(&mut u).unwrap();
    let n = u32::from_le_bytes(u) as usize;
    let mut out = std::io::BufWriter::new(std::fs::File::create(&a[4]).unwrap());
    for i in 0..n {
        f.read_exact(&mut u).unwrap();
        let t = u32::from_le_bytes(u) as usize;
        f.read_exact(&mut u).unwrap();
        let v = u32::from_le_bytes(u) as usize;
        let mut raw = vec![0u8; t * v * 4];
        f.read_exact(&mut raw).unwrap();
        let lp: Vec<Vec<f32>> = (0..t)
            .map(|r| {
                (0..v)
                    .map(|c| {
                        f32::from_le_bytes(
                            raw[(r * v + c) * 4..(r * v + c) * 4 + 4]
                                .try_into()
                                .unwrap(),
                        )
                    })
                    .collect()
            })
            .collect();
        let (id, hw) = &hws[i];
        let h = Hotwords::new(hw.clone(), weight);
        let mut text = dec.decode(&lp, &h, 80, -5.0, -10.0);
        if canon {
            text = canonicalize(&text);
        }
        let esc = text.replace('\\', "\\\\").replace('"', "\\\"");
        writeln!(out, "{{\"id\":\"{id}\",\"text\":\"{esc}\"}}").unwrap();
    }
}
