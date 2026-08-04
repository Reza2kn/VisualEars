//! Shenava streaming model registry — per-model encoder dims + cache-aware chunk geometry.
//! Mirrors `ShenavaModel` in the macOS app. [70,1]=80ms → 25/9/16; Koochik HD
//! is the shipped 1040ms export → 121/105/112.

#[derive(Clone, Copy)]
pub struct ShenavaModel {
    pub key: &'static str,
    pub display: &'static str,
    pub num_layers: usize,
    pub d_model: usize,
    pub chunk_frames: usize,       // mel frames fed per step (input width)
    pub first_chunk_frames: usize, // primed first chunk
    pub shift_frames: usize,       // steady advance
    pub pre_encode_overlap: usize, // mel frames re-fed between chunks (0 = adjacent, cache-handled)
}

pub const MODELS: &[ShenavaModel] = &[
    ShenavaModel {
        key: "koochik",
        display: "Koochik · 114M (80ms)",
        num_layers: 17,
        d_model: 512,
        chunk_frames: 25,
        first_chunk_frames: 9,
        shift_frames: 16,
        pre_encode_overlap: 9,
    },
    ShenavaModel {
        key: "koochik_hd",
        display: "Koochik HD · 114M (1040ms)",
        num_layers: 17,
        d_model: 512,
        chunk_frames: 121,
        first_chunk_frames: 105,
        shift_frames: 112,
        pre_encode_overlap: 9,
    },
    ShenavaModel {
        key: "rizeh",
        display: "Rizeh · 32M (80ms)",
        num_layers: 16,
        d_model: 256,
        chunk_frames: 25,
        first_chunk_frames: 9,
        shift_frames: 16,
        pre_encode_overlap: 9,
    },
    ShenavaModel {
        key: "pizeh",
        display: "Rizeh-Pizeh · 6.9M (80ms)",
        num_layers: 12,
        d_model: 144,
        chunk_frames: 25,
        first_chunk_frames: 9,
        shift_frames: 16,
        pre_encode_overlap: 9,
    },
    // English FastConformer-medium streaming (nvidia stt_en_..._medium_streaming_80ms_pc):
    // single-latency [70,13]. NeMo streaming-cfg shift_size[1]=112 → each chunk is 112*160=17920
    // audio samples whose preprocessor emits 113 mel frames (112 valid). The encoder input width is
    // therefore 113 (chunk_frames); the steady advance is 112 (shift_frames). Verified against the
    // NeMo reference (stream_ctc_vs_rnnt.py) used to generate preds_stream_rnnt_deg.json.
    ShenavaModel {
        key: "english",
        display: "English · FC-medium 32M ([70,13])",
        num_layers: 16,
        d_model: 256,
        chunk_frames: 113,
        first_chunk_frames: 113,
        shift_frames: 112,
        pre_encode_overlap: 0,
    },
];

pub fn named(key: &str) -> ShenavaModel {
    MODELS
        .iter()
        .copied()
        .find(|m| m.key == key)
        .unwrap_or(MODELS[0])
}

/// tokens.txt is `<piece> <id>` per line; blank at id 1024. Returns a 1025-slot table.
pub fn load_tokens(path: &str) -> std::io::Result<Vec<String>> {
    let text = std::fs::read_to_string(path)?;
    let mut toks = vec![String::new(); 1025];
    for line in text.lines() {
        if let Some(sp) = line.rfind(' ') {
            let piece = &line[..sp];
            if let Ok(id) = line[sp + 1..].trim().parse::<usize>() {
                if id < toks.len() {
                    toks[id] = piece.to_string();
                }
            }
        }
    }
    Ok(toks)
}
