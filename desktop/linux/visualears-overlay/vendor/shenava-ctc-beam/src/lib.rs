//! Hotword-boosted CTC prefix-beam decoder — a faithful, dependency-free Rust port of the
//! `pyctcdecode` beam search (no-LM path) with per-utterance hotword boosting.
//!
//! Intended second pass for the Shenava on-device ASR stack: feed the FastConformer CTC
//! log-probs for a finished utterance plus a hotword list (e.g. from a Vosk pass), and get a
//! keyword-boosted transcript. Matches pyctcdecode's decode to (near) parity.
//!
//! Defaults mirror pyctcdecode: beam_width 100, token_min_logp -5.0, beam_prune_logp -10.0,
//! hotword_weight 10.0. BPE word-start marker is `▁` (U+2581); blank = the empty-string label.

use std::collections::HashMap;

pub mod rescore;

const BPE: char = '\u{2581}'; // ▁

/// Deterministic Persian orthographic canonicalizer for decoder output (no lexicon, no model, no
/// alloc-heavy work — ships even to the 1GB armv7 TV). Folds the "wrong-word" errors that are pure
/// orthography, not acoustics: Arabic→Persian letters (ي→ی, ك→ک, ة/ۀ→ه, أإآ→ا, ؤ→و, ئ→ی), strips
/// tatweel + harakat, normalizes Arabic-Indic/Persian digits to ASCII, ZWNJ→space, collapses runs.
pub fn canonicalize(text: &str) -> String {
    let mut buf = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            'ي' => buf.push('ی'),
            'ك' => buf.push('ک'),
            'ۀ' | 'ة' => buf.push('ه'),
            'أ' | 'إ' | 'آ' | 'ٱ' => buf.push('ا'),
            'ؤ' => buf.push('و'),
            'ئ' => buf.push('ی'),
            'ـ' => {}                                              // tatweel — drop
            '\u{064B}'..='\u{0652}' | '\u{0670}' => {}             // harakat/diacritics — drop
            '\u{200C}' | '\u{200F}' | '\u{FEFF}' => buf.push(' '), // ZWNJ/marks → space
            '٠'..='٩' => buf.push((b'0' + (c as u32 - 0x0660) as u8) as char),
            '۰'..='۹' => buf.push((b'0' + (c as u32 - 0x06F0) as u8) as char),
            _ => buf.push(c),
        }
    }
    buf.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Decoder holds the vocabulary (label per class index) and derived blank id.
pub struct CtcBeamDecoder {
    labels: Vec<String>,
    blank_id: usize,
}

/// Per-utterance hotwords (unigram words to boost).
pub struct Hotwords {
    set: std::collections::HashSet<String>,
    // char-prefix → min char-length of any unigram with that prefix. Makes partial-word credit
    // O(word length) instead of O(#words) — so the list scales to a full lexicon (100k–700k words).
    prefix_min: std::collections::HashMap<String, u16>,
    weight: f32,
}

impl Hotwords {
    /// Build from an iterator of words/phrases (phrases are split into unigrams). Scales to large
    /// lexicons: partial credit is a prefix→min-length map lookup, not a linear scan over words.
    pub fn new<I: IntoIterator<Item = String>>(words: I, weight: f32) -> Self {
        let mut set = std::collections::HashSet::new();
        let mut prefix_min: std::collections::HashMap<String, u16> =
            std::collections::HashMap::new();
        for w in words {
            for u in w.split_whitespace() {
                if u.is_empty() || !set.insert(u.to_string()) {
                    continue;
                }
                let ulen = u.chars().count().min(u16::MAX as usize) as u16;
                let mut pref = String::new();
                for c in u.chars() {
                    pref.push(c);
                    let e = prefix_min.entry(pref.clone()).or_insert(u16::MAX);
                    if ulen < *e {
                        *e = ulen;
                    }
                }
            }
        }
        Hotwords {
            set,
            prefix_min,
            weight,
        }
    }
    #[inline]
    fn is_word(&self, w: &str) -> bool {
        self.set.contains(w)
    }
    /// Partial-token credit: weight * len(word_part) / len(shortest unigram with that prefix).
    fn partial(&self, word_part: &str) -> f32 {
        if word_part.is_empty() {
            return 0.0;
        }
        match self.prefix_min.get(word_part) {
            Some(&min_len) if min_len != u16::MAX => {
                self.weight * (word_part.chars().count() as f32) / (min_len as f32)
            }
            _ => 0.0,
        }
    }
}

#[derive(Clone)]
struct Beam {
    text: String,      // completed words, space-joined
    word_part: String, // current in-progress word
    last_idx: i32,     // last emitted class idx (-1 = start/None); blank_id after a blank
    hw_count: u32,     // # hotword words already folded into `text`
    logit: f32,        // accumulated CTC log-prob (no hotword term)
}

#[inline]
fn log_sum_exp(a: f32, b: f32) -> f32 {
    if a >= b {
        a + (1.0 + (b - a).exp()).ln()
    } else {
        b + (1.0 + (a - b).exp()).ln()
    }
}

impl CtcBeamDecoder {
    /// `labels[i]` = the token for class i; the empty-string label marks CTC blank.
    pub fn new(labels: Vec<String>) -> Self {
        let blank_id = labels
            .iter()
            .position(|s| s.is_empty())
            .unwrap_or(labels.len() - 1);
        CtcBeamDecoder { labels, blank_id }
    }

    /// Decode `log_probs` (T rows × V log-probabilities) with per-utterance `hotwords`.
    pub fn decode(
        &self,
        log_probs: &[Vec<f32>],
        hotwords: &Hotwords,
        beam_width: usize,
        token_min_logp: f32,
        beam_prune_logp: f32,
    ) -> String {
        self.decode_nbest(
            log_probs,
            hotwords,
            beam_width,
            token_min_logp,
            beam_prune_logp,
            1,
        )
        .into_iter()
        .next()
        .map(|h| h.0)
        .unwrap_or_default()
    }

    /// Decode returning the top-`nbest` hypotheses as `(text, hotword-augmented score)`, best first.
    /// This exposes the beam lattice (single-best `decode` throws it away) so a neural reranker /
    /// generative corrector can consume real alternatives. Scores are comparable within one call only.
    pub fn decode_nbest(
        &self,
        log_probs: &[Vec<f32>],
        hotwords: &Hotwords,
        beam_width: usize,
        token_min_logp: f32,
        beam_prune_logp: f32,
        nbest: usize,
    ) -> Vec<(String, f32)> {
        let beams = self.run_beam(
            log_probs,
            hotwords,
            beam_width,
            token_min_logp,
            beam_prune_logp,
        );
        // finalize: fold trailing word_part, merge equal transcripts by log-sum-exp, rank desc
        let mut finals: HashMap<String, f32> = HashMap::new();
        for beam in &beams {
            let (text, hw) = self.fold(&beam.text, &beam.word_part, beam.hw_count, hotwords);
            let s = beam.logit + hotwords.weight * hw as f32;
            finals
                .entry(text)
                .and_modify(|e| *e = log_sum_exp(*e, s))
                .or_insert(s);
        }
        let mut out: Vec<(String, f32)> = finals.into_iter().collect();
        out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        out.truncate(nbest.max(1));
        out
    }

    /// The CTC prefix-beam expansion loop; returns the surviving beams after the last frame.
    fn run_beam(
        &self,
        log_probs: &[Vec<f32>],
        hotwords: &Hotwords,
        beam_width: usize,
        token_min_logp: f32,
        beam_prune_logp: f32,
    ) -> Vec<Beam> {
        let mut beams: Vec<Beam> = vec![Beam {
            text: String::new(),
            word_part: String::new(),
            last_idx: -1,
            hw_count: 0,
            logit: 0.0,
        }];

        for col in log_probs {
            // candidate classes: those >= token_min_logp, plus the argmax
            let mut argmax = 0usize;
            let mut argmax_v = f32::NEG_INFINITY;
            let mut cands: Vec<usize> = Vec::new();
            for (i, &v) in col.iter().enumerate() {
                if v > argmax_v {
                    argmax_v = v;
                    argmax = i;
                }
                if v >= token_min_logp {
                    cands.push(i);
                }
            }
            if !cands.contains(&argmax) {
                cands.push(argmax);
            }

            // expand
            let mut merged: HashMap<(String, String, i32), Beam> = HashMap::new();
            let mut push = |b: Beam| {
                let key = (b.text.clone(), b.word_part.clone(), b.last_idx);
                merged
                    .entry(key)
                    .and_modify(|e| e.logit = log_sum_exp(e.logit, b.logit))
                    .or_insert(b);
            };
            for &idx in &cands {
                let p = col[idx];
                let is_blank = idx == self.blank_id;
                let tok = &self.labels[idx];
                for beam in &beams {
                    if is_blank || idx as i32 == beam.last_idx {
                        // blank or repeat -> stay (CTC collapse)
                        push(Beam {
                            text: beam.text.clone(),
                            word_part: beam.word_part.clone(),
                            last_idx: idx as i32,
                            hw_count: beam.hw_count,
                            logit: beam.logit + p,
                        });
                    } else if tok.starts_with(BPE) {
                        // word boundary: fold current word_part into text, start new word
                        let (text, hw) =
                            self.fold(&beam.text, &beam.word_part, beam.hw_count, hotwords);
                        let clean: String = tok
                            .trim_start_matches(BPE)
                            .trim_end_matches(BPE)
                            .to_string();
                        push(Beam {
                            text,
                            word_part: clean,
                            last_idx: idx as i32,
                            hw_count: hw,
                            logit: beam.logit + p,
                        });
                    } else {
                        // continue current word
                        let mut wp = beam.word_part.clone();
                        wp.push_str(tok);
                        push(Beam {
                            text: beam.text.clone(),
                            word_part: wp,
                            last_idx: idx as i32,
                            hw_count: beam.hw_count,
                            logit: beam.logit + p,
                        });
                    }
                }
            }

            // score (logit + hotword full-word + hotword partial), prune, trim
            let mut scored: Vec<(f32, Beam)> = merged
                .into_values()
                .map(|b| {
                    let s = b.logit
                        + hotwords.weight * b.hw_count as f32
                        + hotwords.partial(&b.word_part);
                    (s, b)
                })
                .collect();
            let max_s = scored.iter().map(|x| x.0).fold(f32::NEG_INFINITY, f32::max);
            scored.retain(|x| x.0 >= max_s + beam_prune_logp);
            scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            scored.truncate(beam_width);
            beams = scored.into_iter().map(|x| x.1).collect();
        }

        beams
    }

    #[inline]
    fn fold(&self, text: &str, word_part: &str, hw_count: u32, hw: &Hotwords) -> (String, u32) {
        if word_part.is_empty() {
            return (text.to_string(), hw_count);
        }
        let new_text = if text.is_empty() {
            word_part.to_string()
        } else {
            format!("{} {}", text, word_part)
        };
        let new_hw = hw_count + if hw.is_word(word_part) { 1 } else { 0 };
        (new_text, new_hw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotword_flips_close_call() {
        // "▁cat" (−0.7) vs "▁cap" (−0.8): acoustic prefers "cat"; hotword "cap" should flip it.
        let labels = vec!["▁cat".into(), "▁cap".into(), "".into()];
        let dec = CtcBeamDecoder::new(labels);
        let lp = vec![vec![-0.7f32, -0.8, -5.0]];
        let none = Hotwords::new(Vec::<String>::new(), 10.0);
        assert_eq!(dec.decode(&lp, &none, 10, -5.0, -10.0), "cat");
        let hw = Hotwords::new(vec!["cap".into()], 10.0);
        assert_eq!(dec.decode(&lp, &hw, 10, -5.0, -10.0), "cap");
    }

    #[test]
    fn greedy_two_words_with_blank_reset() {
        // ▁one · blank · ▁two · blank  ->  "one two"
        let labels = vec!["▁one".into(), "▁two".into(), "".into()];
        let dec = CtcBeamDecoder::new(labels);
        let none = Hotwords::new(Vec::<String>::new(), 10.0);
        let lp = vec![
            vec![-0.1f32, -5.0, -5.0],
            vec![-5.0, -5.0, -0.1],
            vec![-5.0, -0.1, -5.0],
            vec![-5.0, -5.0, -0.1],
        ];
        assert_eq!(dec.decode(&lp, &none, 10, -5.0, -10.0), "one two");
    }

    #[test]
    fn canonicalize_folds_orthography() {
        assert_eq!(canonicalize("كيميا"), "کیمیا"); // Arabic ك ي -> Persian ک ی
        assert_eq!(canonicalize("مي‌روم"), "می روم"); // ZWNJ -> space
        assert_eq!(canonicalize("سـلام"), "سلام"); // tatweel dropped
        assert_eq!(canonicalize("۱۲۳ و ٤٥"), "123 و 45"); // Persian + Arabic-Indic digits -> ASCII
    }

    #[test]
    fn nbest_returns_ranked_alternatives() {
        // "▁cat" (−0.7) vs "▁cap" (−0.8): n-best should list both, cat first (higher acoustic).
        let labels = vec!["▁cat".into(), "▁cap".into(), "".into()];
        let dec = CtcBeamDecoder::new(labels);
        let lp = vec![vec![-0.7f32, -0.8, -5.0]];
        let none = Hotwords::new(Vec::<String>::new(), 10.0);
        let nb = dec.decode_nbest(&lp, &none, 10, -5.0, -10.0, 5);
        assert_eq!(nb[0].0, "cat");
        assert!(nb.iter().any(|(t, _)| t == "cap"));
        assert!(nb[0].1 >= nb[1].1); // ranked descending
    }

    #[test]
    fn repeat_collapses_to_single_word() {
        // ▁hi · ▁hi(repeat, no blank between) -> "hi" (CTC collapse, not "hi hi")
        let labels = vec!["▁hi".into(), "".into()];
        let dec = CtcBeamDecoder::new(labels);
        let none = Hotwords::new(Vec::<String>::new(), 10.0);
        let lp = vec![vec![-0.1f32, -5.0], vec![-0.1, -5.0]];
        assert_eq!(dec.decode(&lp, &none, 10, -5.0, -10.0), "hi");
    }
}
