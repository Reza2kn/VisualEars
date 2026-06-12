import { describe, expect, it } from 'vitest';
import { BLANK_ID, TOKENS, UNK_ID, decodeCtcGreedy } from '../src/engine/ctc';
import { float32ToFloat16Bits } from '../src/engine/fp16';

const VOCAB = TOKENS.length; // 1025

/** Build a logits matrix where each step's argmax is the given token id. */
function logitsFor(ids: number[]): Float32Array {
  const logits = new Float32Array(ids.length * VOCAB);
  ids.forEach((id, t) => {
    logits[t * VOCAB + id] = 10;
  });
  return logits;
}

describe('decodeCtcGreedy', () => {
  it('exposes the published vocab contract', () => {
    expect(VOCAB).toBe(1025);
    expect(BLANK_ID).toBe(1024);
    expect(UNK_ID).toBe(0);
    expect(TOKENS[BLANK_ID]).toBe('<blank>');
    expect(TOKENS[UNK_ID]).toBe('<unk>');
  });

  it('collapses repeats, drops blank/unk, and joins pieces with ▁ → space', () => {
    // Pick two real sentencepiece pieces: one word-initial (▁x) and one continuation.
    const wordInitial = TOKENS.findIndex((t) => t.startsWith('▁') && t.length > 1);
    const continuation = TOKENS.findIndex((t, i) => i > 0 && !t.startsWith('▁') && !t.startsWith('<'));
    const ids = [
      BLANK_ID,
      wordInitial,
      wordInitial, // repeat — collapsed
      BLANK_ID,
      UNK_ID, // dropped
      continuation,
      BLANK_ID,
      wordInitial, // same piece again after blank — kept (new emission)
    ];
    const { text, firstStep, lastStep } = decodeCtcGreedy(logitsFor(ids), ids.length, VOCAB, 'float32');
    const w = TOKENS[wordInitial].slice(1); // without ▁
    const c = TOKENS[continuation];
    expect(text).toBe(`${w}${c} ${w}`);
    expect(firstStep).toBe(1);
    expect(lastStep).toBe(7);
  });

  it('re-emits a token after a blank separator (xx-blank-x → x x)', () => {
    const piece = TOKENS.findIndex((t) => t.startsWith('▁') && t.length > 1);
    const ids = [piece, piece, BLANK_ID, piece];
    const { text } = decodeCtcGreedy(logitsFor(ids), ids.length, VOCAB, 'float32');
    const w = TOKENS[piece].slice(1);
    expect(text).toBe(`${w} ${w}`);
  });

  it('returns empty text and -1 steps for all-blank logits', () => {
    const ids = [BLANK_ID, BLANK_ID, BLANK_ID];
    const { text, firstStep, lastStep } = decodeCtcGreedy(logitsFor(ids), ids.length, VOCAB, 'float32');
    expect(text).toBe('');
    expect(firstStep).toBe(-1);
    expect(lastStep).toBe(-1);
  });

  it('decodes float16 logits identically', () => {
    const piece = TOKENS.findIndex((t) => t.startsWith('▁') && t.length > 1);
    const ids = [piece, BLANK_ID, piece];
    const f32 = logitsFor(ids);
    const f16 = new Uint16Array(f32.length);
    for (let i = 0; i < f32.length; i++) f16[i] = float32ToFloat16Bits(f32[i]);
    const a = decodeCtcGreedy(f32, ids.length, VOCAB, 'float32');
    const b = decodeCtcGreedy(f16, ids.length, VOCAB, 'float16');
    expect(b.text).toBe(a.text);
  });
});
