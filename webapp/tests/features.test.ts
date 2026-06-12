import { describe, expect, it } from 'vitest';
import { FeatureExtractor, FIXED_FRAMES, N_MELS } from '../src/engine/features';
import golden from './fixtures/features_golden.json';

const TOLERANCE = 5e-3;

interface GoldenCase {
  name: string;
  pcm: number[];
  frameCount: number;
  fullValid?: number[][];
  /** 1 = energetic cell (comparable); 0 = float32 cancellation-floor noise. */
  stable?: number[][];
  melIdx?: number[];
  frameIdx?: number[];
  grid?: number[][];
  gridStable?: number[][];
}

describe('FeatureExtractor (log-mel parity vs Python reference)', () => {
  const extractor = new FeatureExtractor();

  for (const c of (golden as { cases: GoldenCase[] }).cases) {
    it(`matches golden case "${c.name}"`, () => {
      const pcm = Float32Array.from(c.pcm);
      const { features, frameCount } = extractor.compute(pcm);

      expect(frameCount).toBe(c.frameCount);

      if (c.fullValid) {
        let compared = 0;
        for (let m = 0; m < N_MELS; m++) {
          for (let t = 0; t < frameCount; t++) {
            if (c.stable && c.stable[m][t] !== 1) continue;
            const got = features[m * FIXED_FRAMES + t];
            const want = c.fullValid[m][t];
            expect(Math.abs(got - want), `mel ${m} frame ${t}`).toBeLessThan(TOLERANCE);
            compared++;
          }
        }
        // The mask must not silently swallow the comparison.
        expect(compared).toBeGreaterThan((N_MELS * frameCount) / 2);
      }

      if (c.grid && c.melIdx && c.frameIdx) {
        let compared = 0;
        c.melIdx.forEach((m, mi) => {
          c.frameIdx!.forEach((t, ti) => {
            if (c.gridStable && c.gridStable[mi][ti] !== 1) return;
            const got = features[m * FIXED_FRAMES + t];
            const want = c.grid![mi][ti];
            expect(Math.abs(got - want), `mel ${m} frame ${t}`).toBeLessThan(TOLERANCE);
            compared++;
          });
        });
        expect(compared).toBeGreaterThan((c.melIdx.length * c.frameIdx.length) / 2);
      }

      // Padding region must stay exactly zero.
      for (let m = 0; m < N_MELS; m += 13) {
        for (let t = frameCount; t < FIXED_FRAMES; t += 97) {
          expect(features[m * FIXED_FRAMES + t]).toBe(0);
        }
      }
    });
  }

  it('is reusable across calls (no state bleed)', () => {
    const a = (golden as { cases: GoldenCase[] }).cases[0];
    const first = extractor.compute(Float32Array.from(a.pcm));
    const snapshot = first.features.slice(0, 200);
    extractor.compute(new Float32Array(16000)); // silence in between
    const again = extractor.compute(Float32Array.from(a.pcm));
    for (let i = 0; i < 200; i++) {
      expect(Math.abs(again.features[i] - snapshot[i])).toBeLessThan(1e-9);
    }
  });
});
