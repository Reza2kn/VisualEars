/** Zero-cost Persian punctuation from prosody + lexical cues.
 *
 *  Uses signals the pipeline already produces — no model, no download, no
 *  inference: CTC gives every word an 80 ms-resolution span, the VAD finds
 *  the silences. Inspired by the boundary-decision framing of
 *  "Efficient Punctuation Restoration via Weighted Lookahead Scoring"
 *  (arXiv 2606.05179), with acoustic pauses standing in for the scorer:
 *
 *   - ،  intra-utterance pause of COMMA_GAP…PERIOD_GAP seconds
 *   - .  longer intra-utterance pause, and at finalized utterance ends
 *        (the ≥900 ms VAD silence *is* the period signal)
 *   - ؟  final mark flips when interrogative cue words appear
 *
 *  Pure function — safe to run on every live partial. */

import type { WordTiming } from './ctc';
import { HOP_LENGTH, OUTPUT_STRIDE, SAMPLE_RATE } from './features';

const STEP_SECONDS = (OUTPUT_STRIDE * HOP_LENGTH) / SAMPLE_RATE; // 0.08

/** Pause length that reads as a comma vs a sentence break (seconds). */
const COMMA_GAP = 0.35;
const PERIOD_GAP = 0.68;
/** Minimum words on either side of an inserted mark. */
const MIN_WORDS_BETWEEN = 2;

/** Standalone interrogative words (informal + formal spoken Persian). */
const QUESTION_WORDS = new Set([
  'آیا',
  'چرا',
  'چطور',
  'چطوری',
  'چگونه',
  'کجا',
  'کجاست',
  'کی',
  'کیه',
  'چی',
  'چیه',
  'چیکار',
  'چه‌کار',
  'کدوم',
  'کدام',
  'چند',
  'چندتا',
  'مگه',
  'مگر',
]);

/** Conjunctions that a comma must not directly precede a break after. */
const NO_MARK_AFTER = new Set(['و', 'یا', 'که', 'تا', 'با', 'به', 'از', 'در', 'بی', 'هر']);

function isQuestion(words: WordTiming[]): boolean {
  return words.some((w) => QUESTION_WORDS.has(w.text));
}

export interface PunctuateOptions {
  /** Utterance ended on real silence (or media chunk boundary) → final mark. */
  isFinal: boolean;
}

/** Render words to display text with prosody punctuation. */
export function punctuate(words: WordTiming[], opts: PunctuateOptions): string {
  if (words.length === 0) return '';

  const marks: string[] = new Array(words.length).fill('');
  let sinceMark = Infinity; // words since the last inserted mark

  for (let i = 0; i < words.length - 1; i++) {
    sinceMark++;
    const gap = (words[i + 1].startStep - words[i].endStep - 1) * STEP_SECONDS;
    if (gap < COMMA_GAP) continue;
    if (sinceMark < MIN_WORDS_BETWEEN || words.length - 1 - i < MIN_WORDS_BETWEEN) continue;
    if (NO_MARK_AFTER.has(words[i].text)) continue;
    marks[i] = gap >= PERIOD_GAP ? '.' : '،';
    sinceMark = 0;
  }

  if (opts.isFinal) {
    marks[words.length - 1] = isQuestion(words) ? '؟' : '.';
  }

  return words
    .map((w, i) => w.text + marks[i])
    .join(' ')
    .trim();
}
