//! Static 3,669-word CTC beam second pass shared by Linux/Windows.
//!
//! Live captions stay greedy for latency. At an utterance boundary, the captioner
//! re-decodes the buffered CTC log-probs with the curated word list, mirroring the
//! macOS Koochik Static-3669 product path without shipping Vosk or ORT.

use shenava_ctc_beam::{canonicalize, CtcBeamDecoder, Hotwords};
use std::collections::HashSet;

const STATIC_HOTWORD_WEIGHT: f32 = 3.0;
const BEAM_WIDTH: usize = 80;
const TOKEN_MIN_LOGP: f32 = -5.0;
const BEAM_PRUNE_LOGP: f32 = -10.0;
const MIN_HOTWORD_CHARS: usize = 3;

pub struct StaticRescorer {
    dec: CtcBeamDecoder,
    hotwords: Hotwords,
}

impl StaticRescorer {
    pub fn from_files(
        tokens_path: &str,
        wordlist_path: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let labels = crate::model::load_tokens(tokens_path)?;
        let words = read_wordlist(wordlist_path)?;
        if words.is_empty() {
            return Err(format!("hotword list is empty: {wordlist_path}").into());
        }
        Ok(Self {
            dec: CtcBeamDecoder::new(labels),
            hotwords: Hotwords::new(words, STATIC_HOTWORD_WEIGHT),
        })
    }

    pub fn decode(&self, logprobs: &[Vec<f32>]) -> String {
        canonicalize(&self.dec.decode(
            logprobs,
            &self.hotwords,
            BEAM_WIDTH,
            TOKEN_MIN_LOGP,
            BEAM_PRUNE_LOGP,
        ))
    }
}

fn read_wordlist(path: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let txt = std::fs::read_to_string(path)?;
    let mut seen = HashSet::new();
    Ok(txt
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim().to_string())
        .filter(|w| w.chars().count() >= MIN_HOTWORD_CHARS && seen.insert(w.clone()))
        .collect())
}
