/** SRT / VTT generation from transcript segments — long segments are split
 *  into readable cues (~≤6 s / ~≤84 chars) at word boundaries with times
 *  interpolated proportionally. */

import { cueClock } from '../format';
import type { Segment } from './mediaSession';

interface Cue {
  tStart: number;
  tEnd: number;
  text: string;
}

const MAX_CUE_SECONDS = 6;
const MAX_CUE_CHARS = 84;

export function splitIntoCues(segments: Segment[]): Cue[] {
  const cues: Cue[] = [];
  for (const seg of segments) {
    const duration = Math.max(0.4, seg.tEnd - seg.tStart);
    const words = seg.text.split(' ').filter(Boolean);
    if (!words.length) continue;
    const parts = Math.max(
      1,
      Math.ceil(Math.max(duration / MAX_CUE_SECONDS, seg.text.length / MAX_CUE_CHARS)),
    );
    if (parts === 1) {
      cues.push({ tStart: seg.tStart, tEnd: seg.tEnd, text: seg.text });
      continue;
    }
    const perPart = Math.ceil(words.length / parts);
    const totalChars = seg.text.length;
    let elapsedChars = 0;
    for (let p = 0; p < parts; p++) {
      const slice = words.slice(p * perPart, (p + 1) * perPart);
      if (!slice.length) break;
      const text = slice.join(' ');
      const startFrac = elapsedChars / totalChars;
      elapsedChars += text.length + 1;
      const endFrac = Math.min(1, elapsedChars / totalChars);
      cues.push({
        tStart: seg.tStart + duration * startFrac,
        tEnd: seg.tStart + duration * endFrac,
        text,
      });
    }
  }
  return cues;
}

export function toSrt(segments: Segment[]): string {
  return splitIntoCues(segments)
    .map(
      (cue, i) =>
        `${i + 1}\n${cueClock(cue.tStart, ',')} --> ${cueClock(cue.tEnd, ',')}\n${cue.text}`,
    )
    .join('\n\n')
    .concat('\n');
}

export function toVtt(segments: Segment[]): string {
  const body = splitIntoCues(segments)
    .map((cue) => `${cueClock(cue.tStart, '.')} --> ${cueClock(cue.tEnd, '.')}\n${cue.text}`)
    .join('\n\n');
  return `WEBVTT\n\n${body}\n`;
}

export function downloadText(filename: string, content: string): void {
  const blob = new Blob([content], { type: 'text/plain;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 5000);
}
