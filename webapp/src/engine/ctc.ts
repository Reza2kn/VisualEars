/** Greedy CTC decode over the fixed-core logits, plus the SentencePiece
 *  vocabulary (extracted from the .nemo — see tokens.json sidecar). */

import tokensJson from './tokens.json';
import { tensorValue, type LogitsType } from './fp16';

export const TOKENS: string[] = tokensJson.tokens;
export const BLANK_ID: number = tokensJson.blank_id; // 1024
export const UNK_ID: number = tokensJson.unk_id; // 0

export interface CtcDecodeResult {
  text: string;
  /** First/last output step carrying a kept token (−1 when text is empty).
   *  Each step spans output_stride×hop = 80 ms — used for timestamps. */
  firstStep: number;
  lastStep: number;
}

export function decodeCtcGreedy(
  logits: ArrayLike<number>,
  timeSteps: number,
  vocabSize: number,
  logitsType: LogitsType,
): CtcDecodeResult {
  let previous = -1;
  let firstStep = -1;
  let lastStep = -1;
  const pieces: string[] = [];
  for (let t = 0; t < timeSteps; t++) {
    let best = 0;
    let bestValue = -Infinity;
    const base = t * vocabSize;
    for (let i = 0; i < vocabSize; i++) {
      const v = tensorValue(logits, base + i, logitsType);
      if (v > bestValue) {
        bestValue = v;
        best = i;
      }
    }
    if (best !== BLANK_ID && best !== previous && best !== UNK_ID) {
      pieces.push(TOKENS[best] ?? '');
      if (firstStep < 0) firstStep = t;
      lastStep = t;
    }
    previous = best;
  }
  const text = pieces.join('').replaceAll('▁', ' ').replace(/\s+/g, ' ').trim();
  return { text, firstStep, lastStep };
}
