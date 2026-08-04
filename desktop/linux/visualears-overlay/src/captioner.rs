//! Live caption pipeline — Rust port of the macOS `ShenavaLiveCaptioner` + roll-up caption state.
//! The decoder streams continuously until a real pause; finalized utterances freeze into stable
//! history while the live tail grows below them. That mirrors the macOS overlay and avoids the
//! old "line keeps chasing itself" feel.

use crate::engine::StreamingRecognizer;
use crate::rescore::StaticRescorer;

const FINALIZE_AFTER_QUIET_TICKS: usize = 8; // ~0.95s of quiet audio = utterance boundary
const MAX_LIVE_WORDS: usize = 24; // run-on safety when decode is clearly advancing
const DEFAULT_MAX_ACTIVE_SAMPLES: usize = 16_000 * 12; // hard cap so noisy rooms/system audio cannot fill up forever
const QUIET_RMS: f32 = 0.004;

#[derive(Debug, Clone)]
pub struct CaptionEvent {
    pub text: String,
    pub is_final: bool,
}

pub struct LiveCaptioner {
    rec: StreamingRecognizer,
    rescorer: Option<StaticRescorer>,
    current_text: String,
    last_text: String,
    quiet_ticks: usize,
    active_samples: usize,
    max_active_samples: usize,
}

impl LiveCaptioner {
    pub fn new(rec: StreamingRecognizer) -> Self {
        Self {
            rec,
            rescorer: None,
            current_text: String::new(),
            last_text: String::new(),
            quiet_ticks: 0,
            active_samples: 0,
            max_active_samples: DEFAULT_MAX_ACTIVE_SAMPLES,
        }
    }

    pub fn with_static_guide(mut rec: StreamingRecognizer, rescorer: StaticRescorer) -> Self {
        rec.set_collect_logprobs(true);
        Self {
            rec,
            rescorer: Some(rescorer),
            current_text: String::new(),
            last_text: String::new(),
            quiet_ticks: 0,
            active_samples: 0,
            max_active_samples: DEFAULT_MAX_ACTIVE_SAMPLES,
        }
    }

    pub fn with_max_active_seconds(mut self, seconds: f32) -> Self {
        let seconds = seconds.clamp(4.0, 20.0);
        self.max_active_samples = (seconds * 16_000.0).round() as usize;
        self
    }

    /// Feed one tick's worth of audio (~100ms). Returns caption events for this tick.
    pub fn feed_batch(&mut self, batch: &[f32]) -> Vec<CaptionEvent> {
        let mut events = Vec::new();
        if batch.is_empty() {
            self.quiet_ticks += 1;
            if self.quiet_ticks >= FINALIZE_AFTER_QUIET_TICKS {
                if let Some(e) = self.finalize() {
                    events.push(e);
                }
            }
            return events;
        }
        self.active_samples = self.active_samples.saturating_add(batch.len());
        let is_quiet_audio = rms(batch) < QUIET_RMS;
        let text = self.rec.accept(batch).unwrap_or_default();
        let words: Vec<&str> = text.split_whitespace().collect();

        if is_quiet_audio {
            self.quiet_ticks += 1;
        } else {
            self.quiet_ticks = 0;
        }
        if text != self.last_text {
            self.last_text = text.clone();
        }
        if !text.is_empty() {
            self.current_text = text.clone();
        }

        if !words.is_empty() {
            events.push(CaptionEvent {
                text: words.join(" "),
                is_final: false,
            });
        }

        let has_live_text = !words.is_empty() || !self.current_text.trim().is_empty();
        let hit_time_cap = self.active_samples >= self.max_active_samples && has_live_text;
        if (self.quiet_ticks >= FINALIZE_AFTER_QUIET_TICKS && has_live_text)
            || words.len() >= MAX_LIVE_WORDS
            || hit_time_cap
        {
            if let Some(e) = self.finalize() {
                events.push(e);
            }
        }
        events
    }

    fn finalize(&mut self) -> Option<CaptionEvent> {
        if let Ok(flushed) = self.rec.flush() {
            if !flushed.trim().is_empty() {
                self.current_text = flushed;
            }
        }
        let greedy = self.current_text.trim().to_string();
        let logprobs = if self.rescorer.is_some() {
            self.rec.take_logprobs()
        } else {
            Vec::new()
        };
        let _ = self.rec.reset();
        self.current_text.clear();
        self.last_text.clear();
        self.quiet_ticks = 0;
        self.active_samples = 0;
        let line = self
            .rescorer
            .as_ref()
            .and_then(|r| {
                let rescored = r.decode(&logprobs).trim().to_string();
                if rescored.is_empty() {
                    None
                } else {
                    Some(preserve_terminal_mark(&rescored, &greedy))
                }
            })
            .unwrap_or(greedy);
        if line.is_empty() {
            None
        } else {
            Some(CaptionEvent {
                text: line,
                is_final: true,
            })
        }
    }
}

fn rms(batch: &[f32]) -> f32 {
    if batch.is_empty() {
        return 0.0;
    }
    (batch.iter().map(|x| x * x).sum::<f32>() / batch.len() as f32).sqrt()
}

/// Display state: committed history + one live draft, matching the macOS roll-up caption model.
/// Finalized lines do not reflow under the user's eyes; the live draft grows underneath them.
#[derive(Default)]
pub struct TranscriptState {
    pub history: Vec<String>,
    pub draft: Option<String>,
}

impl TranscriptState {
    pub fn apply(&mut self, e: &CaptionEvent) {
        let cleaned =
            persian_number_normalize(&e.text.split_whitespace().collect::<Vec<_>>().join(" "));
        if e.is_final {
            let cleaned = punctuate_final(&cleaned);
            if !cleaned.is_empty() {
                self.history.push(cleaned);
                if self.history.len() > 300 {
                    self.history.drain(0..self.history.len() - 300);
                }
            }
            self.draft = None;
        } else {
            self.draft = if cleaned.is_empty() {
                None
            } else {
                Some(cleaned)
            };
        }
    }

    /// Ordered immutable finalized blocks plus the live tail, bounded exactly like macOS
    /// `RollUpCaption.blocks(keepingLast:)` so wrapping work stays constant over long sessions.
    pub fn blocks(&self, keeping_last: usize) -> Vec<String> {
        let mut blocks: Vec<String> = self
            .history
            .iter()
            .rev()
            .take(keeping_last)
            .cloned()
            .collect();
        blocks.reverse();
        if let Some(draft) = self.draft.as_ref().filter(|text| !text.is_empty()) {
            if blocks.len() == keeping_last && !blocks.is_empty() {
                blocks.remove(0);
            }
            blocks.push(draft.clone());
        }
        blocks
    }

    /// Frozen finalized lines plus the current live line, capped to the visible caption rows.
    pub fn visible_lines(&self, max_lines: usize) -> Vec<String> {
        let draft = self.draft.as_deref().filter(|s| !s.is_empty());
        let history_rows = if draft.is_some() {
            max_lines.saturating_sub(1)
        } else {
            max_lines
        };
        let mut rows: Vec<String> = self
            .history
            .iter()
            .rev()
            .take(history_rows)
            .cloned()
            .collect();
        rows.reverse();
        if let Some(draft) = draft {
            rows.push(draft.to_string());
        }
        if rows.len() > max_lines {
            rows.drain(0..rows.len() - max_lines);
        }
        rows
    }

    /// Balance a single utterance across up to `max_lines` rows (mirrors the Swift reflow).
    pub fn reflow(text: &str, max_lines: usize) -> Vec<String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        if max_lines <= 1 || words.len() <= 2 {
            return vec![text.to_string()];
        }
        let mut lines = Vec::new();
        let mut cursor = 0;
        for line_idx in 0..max_lines {
            let remaining = words.len() - cursor;
            if remaining == 0 {
                break;
            }
            let take = ((remaining as f32) / ((max_lines - line_idx) as f32)).ceil() as usize;
            let end = (cursor + take.max(1)).min(words.len());
            lines.push(words[cursor..end].join(" "));
            cursor = end;
        }
        lines
    }
}

#[derive(Clone, Copy)]
struct ParsedNumber {
    value: i32,
    end: usize,
    numeric_count: usize,
    has_scale: bool,
}

/// Display-layer port of macOS `PersianNumberNormalizer`. It deliberately leaves a lone ambiguous
/// number word in prose untouched, but converts confident compounds, years, digit sequences, and
/// single numbers followed by an explicit unit.
fn persian_number_normalize(text: &str) -> String {
    let parts: Vec<&str> = text.split_whitespace().collect();
    if parts.is_empty() {
        return text.to_string();
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < parts.len() {
        if let Some((display, end)) = parse_spoken_year(&parts, i)
            .or_else(|| parse_digit_sequence(&parts, i))
            .or_else(|| parse_single_with_unit(&parts, i))
            .or_else(|| parse_number(&parts, i).map(|p| (persian_digits(p.value), p.end)))
        {
            out.push(display);
            i = end;
        } else {
            out.push(parts[i].to_string());
            i += 1;
        }
    }
    restore_alef_madda(&out.join(" "))
}

fn parse_number(parts: &[&str], start: usize) -> Option<ParsedNumber> {
    let mut end = start;
    let mut has_scale = false;
    let mut value = 0i32;
    let mut current = 0i32;
    let mut numeric_count = 0usize;
    while end < parts.len() {
        let word = clean_number_word(parts[end]);
        if word == "و" {
            if end == start
                || end + 1 >= parts.len()
                || !is_numeric_word(&clean_number_word(parts[end + 1]))
            {
                break;
            }
        } else if word == "هزار" {
            value += (if current == 0 { 1 } else { current }) * 1000;
            current = 0;
            numeric_count += 1;
            has_scale = true;
        } else if let Some(n) = hundreds(&word) {
            current += n;
            numeric_count += 1;
            has_scale = true;
        } else if let Some(n) = tens(&word).or_else(|| teens(&word)).or_else(|| ones(&word)) {
            current += n;
            numeric_count += 1;
        } else {
            break;
        }
        end += 1;
    }
    if end == start {
        return None;
    }
    let total = value + current;
    (has_scale || numeric_count >= 2 || total >= 10).then_some(ParsedNumber {
        value: total,
        end,
        numeric_count,
        has_scale,
    })
}

fn parse_single_with_unit(parts: &[&str], start: usize) -> Option<(String, usize)> {
    if start + 1 >= parts.len() {
        return None;
    }
    let value = ones(&clean_number_word(parts[start]))
        .or_else(|| teens(&clean_number_word(parts[start])))
        .or_else(|| tens(&clean_number_word(parts[start])))?;
    let unit = clean_number_word(parts[start + 1]);
    matches!(
        unit.as_str(),
        "ماه"
            | "هفته"
            | "ساعت"
            | "دقیقه"
            | "ثانیه"
            | "نفر"
            | "تا"
            | "درصد"
            | "تومن"
            | "تومان"
            | "وات"
            | "ولت"
    )
    .then(|| (persian_digits(value), start + 1))
}

fn parse_spoken_year(parts: &[&str], start: usize) -> Option<(String, usize)> {
    let first = parse_two_digit_component(parts, start)?;
    if !(10..=20).contains(&first.value) || first.has_scale {
        return None;
    }
    let second = parse_two_digit_component(parts, first.end)?;
    if !(0..=99).contains(&second.value) || second.has_scale {
        return None;
    }
    let value = first.value * 100 + second.value;
    (1000..=2099)
        .contains(&value)
        .then(|| (persian_digits(value), second.end))
}

fn parse_two_digit_component(parts: &[&str], start: usize) -> Option<ParsedNumber> {
    let word = clean_number_word(*parts.get(start)?);
    if let Some(value) = teens(&word) {
        return Some(ParsedNumber {
            value,
            end: start + 1,
            numeric_count: 1,
            has_scale: false,
        });
    }
    if let Some(mut value) = tens(&word) {
        let mut end = start + 1;
        if end + 1 < parts.len() && clean_number_word(parts[end]) == "و" {
            if let Some(one) = ones(&clean_number_word(parts[end + 1])) {
                value += one;
                end += 2;
            }
        } else if end < parts.len() {
            if let Some(one) = ones(&clean_number_word(parts[end])) {
                value += one;
                end += 1;
            }
        }
        return Some(ParsedNumber {
            value,
            end,
            numeric_count: if end > start + 1 { 2 } else { 1 },
            has_scale: false,
        });
    }
    ones(&word).map(|value| ParsedNumber {
        value,
        end: start + 1,
        numeric_count: 1,
        has_scale: false,
    })
}

fn parse_digit_sequence(parts: &[&str], start: usize) -> Option<(String, usize)> {
    let mut digits = Vec::new();
    let mut end = start;
    while end < parts.len() {
        let Some(digit) = ones(&clean_number_word(parts[end])) else {
            break;
        };
        digits.push(digit);
        end += 1;
    }
    (digits.len() >= 2).then(|| {
        (
            digits.into_iter().map(persian_digits).collect::<String>(),
            end,
        )
    })
}

fn clean_number_word(word: &str) -> String {
    word.trim_matches(|c| matches!(c, '،' | '.' | '؟' | '!' | '?' | ':' | ';'))
        .replace('ي', "ی")
        .replace('ك', "ک")
        .replace('\u{200c}', "")
}

fn is_numeric_word(word: &str) -> bool {
    word == "هزار"
        || ones(word).is_some()
        || teens(word).is_some()
        || tens(word).is_some()
        || hundreds(word).is_some()
}

fn ones(word: &str) -> Option<i32> {
    match word {
        "صفر" => Some(0),
        "یک" | "یه" => Some(1),
        "دو" => Some(2),
        "سه" => Some(3),
        "چهار" | "چار" => Some(4),
        "پنج" => Some(5),
        "شش" | "شیش" => Some(6),
        "هفت" => Some(7),
        "هشت" => Some(8),
        "نه" => Some(9),
        _ => None,
    }
}

fn teens(word: &str) -> Option<i32> {
    match word {
        "ده" => Some(10),
        "یازده" => Some(11),
        "دوازده" => Some(12),
        "سیزده" => Some(13),
        "چهارده" => Some(14),
        "پانزده" | "پونزده" => Some(15),
        "شانزده" => Some(16),
        "هفده" | "هیفده" => Some(17),
        "هجده" | "هیجده" => Some(18),
        "نوزده" => Some(19),
        _ => None,
    }
}

fn tens(word: &str) -> Option<i32> {
    match word {
        "بیست" => Some(20),
        "سی" => Some(30),
        "چهل" | "چهلم" => Some(40),
        "پنجاه" => Some(50),
        "شصت" => Some(60),
        "هفتاد" => Some(70),
        "هشتاد" => Some(80),
        "نود" => Some(90),
        _ => None,
    }
}

fn hundreds(word: &str) -> Option<i32> {
    match word {
        "صد" | "یکصد" => Some(100),
        "دویست" => Some(200),
        "سیصد" => Some(300),
        "چهارصد" => Some(400),
        "پانصد" | "پونصد" | "پنجصد" => Some(500),
        "ششصد" | "شیشصد" => Some(600),
        "هفتصد" => Some(700),
        "هشتصد" => Some(800),
        "نهصد" => Some(900),
        _ => None,
    }
}

fn persian_digits(value: i32) -> String {
    const DIGITS: [char; 10] = ['۰', '۱', '۲', '۳', '۴', '۵', '۶', '۷', '۸', '۹'];
    value
        .to_string()
        .chars()
        .map(|ch| {
            ch.to_digit(10)
                .map(|digit| DIGITS[digit as usize])
                .unwrap_or(ch)
        })
        .collect()
}

fn punctuate_final(text: &str) -> String {
    let cleaned = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(" ،", "،")
        .replace(" .", ".")
        .replace(" ؟", "؟")
        .replace(" !", "!")
        .replace(" :", ":")
        .replace(" ؛", "؛")
        .trim()
        .to_string();
    if cleaned.is_empty() {
        return cleaned;
    }
    if cleaned
        .chars()
        .last()
        .is_some_and(|c| matches!(c, '،' | '؛' | ':'))
    {
        let mut out = cleaned;
        out.pop();
        out.push('؟');
        return out;
    }
    if cleaned.ends_with('.') && looks_like_question_text(&cleaned) {
        let mut out = cleaned;
        out.pop();
        out.push('؟');
        return out;
    }
    if cleaned
        .chars()
        .last()
        .is_some_and(|c| matches!(c, '؟' | '!' | '?'))
    {
        return cleaned;
    }
    if looks_like_question_text(&cleaned) {
        return format!("{cleaned}؟");
    }
    cleaned
}

fn looks_like_question_text(text: &str) -> bool {
    let normalized = text
        .replace('آ', "ا")
        .replace('ي', "ی")
        .replace('ك', "ک")
        .trim_end_matches(|c| matches!(c, '.' | '؟' | '!' | '?' | '،' | '؛' | ':'))
        .trim()
        .to_string();
    let words: Vec<&str> = normalized.split_whitespace().collect();
    let Some(first) = words.first() else {
        return false;
    };
    matches!(
        *first,
        "ایا"
            | "چرا"
            | "چطور"
            | "چگونه"
            | "کجا"
            | "کی"
            | "چی"
            | "کدام"
            | "کدوم"
            | "چند"
            | "چقدر"
            | "مگر"
            | "مگه"
    ) || matches!(
        words.as_slice(),
        ["چه", "خبرا", ..] | ["به", "چه", ..] | ["از", "کجا", ..] | ["برای", "چی", ..]
    )
}

fn restore_alef_madda(text: &str) -> String {
    text.split_whitespace()
        .map(restore_alef_madda_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn restore_alef_madda_token(token: &str) -> String {
    let suffix_len = token
        .chars()
        .rev()
        .take_while(|c| matches!(c, '،' | '.' | '؟' | '!' | '?' | ':' | ';'))
        .map(char::len_utf8)
        .sum::<usize>();
    let split = token.len().saturating_sub(suffix_len);
    let (core, suffix) = token.split_at(split);
    if core.is_empty() || core.contains('آ') {
        return token.to_string();
    }
    format!("{}{}", restore_alef_madda_core(core), suffix)
}

fn restore_alef_madda_core(word: &str) -> String {
    const PREFIXES: &[(&str, &str)] = &[
        ("ازاد", "آزاد"),
        ("اموز", "آموز"),
        ("اماده", "آماده"),
        ("ارام", "آرام"),
        ("اغاز", "آغاز"),
        ("اخر", "آخر"),
        ("افتاب", "آفتاب"),
        ("اتش", "آتش"),
        ("اسمان", "آسمان"),
        ("اسان", "آسان"),
        ("اسیب", "آسیب"),
        ("اشنا", "آشنا"),
        ("اشپز", "آشپز"),
        ("اشوب", "آشوب"),
        ("ارزو", "آرزو"),
        ("ارایش", "آرایش"),
        ("اینده", "آینده"),
        ("ایین", "آیین"),
        ("ادرس", "آدرس"),
        ("امار", "آمار"),
        ("المان", "آلمان"),
        ("امریکا", "آمریکا"),
        ("اپارتمان", "آپارتمان"),
        ("اپارات", "آپارات"),
        ("اکادمی", "آکادمی"),
        ("اکادمیک", "آکادمیک"),
    ];
    for (plain, restored) in PREFIXES {
        if let Some(rest) = word.strip_prefix(plain) {
            return format!("{restored}{rest}");
        }
    }
    match word {
        "اب" => "آب",
        "ابی" => "آبی",
        "اباد" => "آباد",
        "ابادی" => "آبادی",
        "ابان" => "آبان",
        "ادم" => "آدم",
        "ادما" => "آدما",
        "ادمی" => "آدمی",
        "ادمها" => "آدمها",
        "ادمایی" => "آدمایی",
        "اقا" => "آقا",
        "اقای" => "آقای",
        "اقایی" => "آقایی",
        "اخ" => "آخ",
        "الو" => "آلو",
        "اره" => "آره",
        "اری" => "آری",
        "ایا" => "آیا",
        "ان" => "آن",
        "انها" => "آنها",
        "انقدر" => "آنقدر",
        _ => word,
    }
    .to_string()
}

fn preserve_terminal_mark(text: &str, tone_source: &str) -> String {
    let cleaned = text.trim();
    if cleaned.is_empty() || terminal_mark(cleaned).is_some() {
        return cleaned.to_string();
    }
    match terminal_mark(tone_source) {
        Some(mark) => format!("{cleaned}{mark}"),
        None => cleaned.to_string(),
    }
}

fn terminal_mark(text: &str) -> Option<char> {
    text.trim()
        .chars()
        .last()
        .filter(|c| matches!(c, '.' | '؟' | '!' | '?'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_rolls_up_finalized_line_with_live_tail() {
        let mut ts = TranscriptState::default();
        ts.apply(&CaptionEvent {
            text: "سلام دنیا".to_string(),
            is_final: true,
        });
        ts.apply(&CaptionEvent {
            text: "این یک جمله زنده است".to_string(),
            is_final: false,
        });

        assert_eq!(
            ts.visible_lines(2),
            vec!["سلام دنیا".to_string(), "این یک جمله زنده است".to_string()]
        );
    }

    #[test]
    fn transcript_caps_to_recent_visible_rows() {
        let mut ts = TranscriptState::default();
        for text in ["یک", "دو", "سه"] {
            ts.apply(&CaptionEvent {
                text: text.to_string(),
                is_final: true,
            });
        }

        assert_eq!(
            ts.visible_lines(2),
            vec!["دو".to_string(), "سه".to_string()]
        );
    }

    #[test]
    fn persian_itn_matches_macos_acceptance_examples() {
        assert_eq!(
            persian_number_normalize("سال هزار و نهصد و شصت و نه"),
            "سال ۱۹۶۹"
        );
        assert_eq!(persian_number_normalize("هشت ماه"), "۸ ماه");
        assert_eq!(persian_number_normalize("یک نه شش نه"), "۱۹۶۹");
        assert_eq!(persian_number_normalize("یک روز خوب"), "یک روز خوب");
    }
}
