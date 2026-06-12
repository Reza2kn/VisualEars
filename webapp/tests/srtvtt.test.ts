import { describe, expect, it } from 'vitest';
import { splitIntoCues, toSrt, toVtt } from '../src/engine/srtvtt';
import type { Segment } from '../src/engine/mediaSession';

const SHORT: Segment[] = [
  { tStart: 0.4, tEnd: 2.96, speakerId: 0, text: 'سلام! امروز می‌خوام درباره هوش مصنوعی حرف بزنم.' },
  { tStart: 11, tEnd: 14.5, speakerId: 0, text: 'یعنی همه‌چیز بدون اینترنت کار می‌کنه؟' },
];

describe('SRT/VTT export', () => {
  it('renders SRT with comma clocks and 1-based indices', () => {
    expect(toSrt(SHORT)).toBe(
      `1\n00:00:00,400 --> 00:00:02,960\nسلام! امروز می‌خوام درباره هوش مصنوعی حرف بزنم.\n\n` +
        `2\n00:00:11,000 --> 00:00:14,500\nیعنی همه‌چیز بدون اینترنت کار می‌کنه؟\n`,
    );
  });

  it('renders VTT with header and dot clocks', () => {
    expect(toVtt(SHORT)).toBe(
      `WEBVTT\n\n00:00:00.400 --> 00:00:02.960\nسلام! امروز می‌خوام درباره هوش مصنوعی حرف بزنم.\n\n` +
        `00:00:11.000 --> 00:00:14.500\nیعنی همه‌چیز بدون اینترنت کار می‌کنه؟\n`,
    );
  });

  it('splits long segments into readable cues with monotonic times', () => {
    const words = Array.from({ length: 40 }, (_, i) => `کلمهٔ${i}`);
    const seg: Segment = { tStart: 10, tEnd: 28, speakerId: 0, text: words.join(' ') };
    const cues = splitIntoCues([seg]);
    expect(cues.length).toBeGreaterThan(1);
    // All words preserved, in order.
    expect(cues.map((c) => c.text).join(' ')).toBe(seg.text);
    // Times stay inside the segment and increase.
    let prevEnd = seg.tStart;
    for (const cue of cues) {
      expect(cue.tStart).toBeGreaterThanOrEqual(prevEnd - 1e-9);
      expect(cue.tEnd).toBeGreaterThan(cue.tStart);
      expect(cue.tEnd).toBeLessThanOrEqual(seg.tEnd + 1e-9);
      expect(cue.tEnd - cue.tStart).toBeLessThanOrEqual(6.6);
      prevEnd = cue.tEnd;
    }
  });

  it('skips segments with no words', () => {
    expect(splitIntoCues([{ tStart: 0, tEnd: 1, speakerId: 0, text: '  ' }])).toEqual([]);
  });
});
