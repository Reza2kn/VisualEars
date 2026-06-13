import { describe, expect, it } from 'vitest';
import { punctuate } from '../src/engine/punctuate';
import type { WordTiming } from '../src/engine/ctc';

/** Build word timings with given inter-word gaps (in seconds; 80 ms steps). */
function timed(words: string[], gapsSec: number[] = []): WordTiming[] {
  const out: WordTiming[] = [];
  let step = 0;
  words.forEach((text, i) => {
    const len = 3; // ~240 ms per word
    out.push({ text, startStep: step, endStep: step + len - 1 });
    const gap = gapsSec[i] ?? 0.08;
    step += len + Math.round(gap / 0.08) + 1;
  });
  return out;
}

describe('punctuate (prosody + cues)', () => {
  it('ends finalized utterances with a period', () => {
    expect(punctuate(timed(['سلام', 'دنیا']), { isFinal: true })).toBe('سلام دنیا.');
  });

  it('keeps partials open-ended', () => {
    expect(punctuate(timed(['سلام', 'دنیا']), { isFinal: false })).toBe('سلام دنیا');
  });

  it('uses ؟ when an interrogative cue is present', () => {
    expect(punctuate(timed(['کجا', 'می‌ری']), { isFinal: true })).toBe('کجا می‌ری؟');
    expect(punctuate(timed(['مگه', 'نگفتم', 'بیا']), { isFinal: true })).toBe('مگه نگفتم بیا؟');
  });

  it('inserts a comma on a medium pause', () => {
    const words = timed(['دیروز', 'رفتم', 'بازار', 'و', 'خرید', 'کردم'], [0.08, 0.08, 0.45, 0.08, 0.08]);
    expect(punctuate(words, { isFinal: true })).toBe('دیروز رفتم بازار، و خرید کردم.');
  });

  it('upgrades a long pause to a period', () => {
    const words = timed(['اومدم', 'خونه', 'هوا', 'سرد', 'بود'], [0.08, 0.9, 0.08, 0.08]);
    expect(punctuate(words, { isFinal: true })).toBe('اومدم خونه. هوا سرد بود.');
  });

  it('never marks right after conjunctions/prepositions', () => {
    const words = timed(['رفتم', 'به', 'خونه', 'مادرم'], [0.08, 0.5, 0.08]);
    expect(punctuate(words, { isFinal: true })).toBe('رفتم به خونه مادرم.');
  });

  it('marks a pause after a discourse opener', () => {
    const words = timed(['خب', 'بریم', 'سر', 'اصل', 'مطلب'], [0.5, 0.08, 0.08, 0.08]);
    expect(punctuate(words, { isFinal: true })).toBe('خب، بریم سر اصل مطلب.');
  });

  it('suppresses a second mark immediately after the first', () => {
    // pauses after consecutive words — only the first becomes a comma
    const words = timed(['اول', 'گفتم', 'بعد', 'دیدم', 'نشد'], [0.08, 0.4, 0.4, 0.08]);
    expect(punctuate(words, { isFinal: true })).toBe('اول گفتم، بعد دیدم نشد.');
  });

  it('handles empty and single-word inputs', () => {
    expect(punctuate([], { isFinal: true })).toBe('');
    expect(punctuate(timed(['باشه']), { isFinal: true })).toBe('باشه.');
    expect(punctuate(timed(['چی']), { isFinal: true })).toBe('چی؟');
  });
});
