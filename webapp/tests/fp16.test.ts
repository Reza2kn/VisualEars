import { describe, expect, it } from 'vitest';
import {
  float16BitsToFloat32,
  float32ArrayToFloat16Bits,
  float32ToFloat16Bits,
} from '../src/engine/fp16';

describe('fp16 conversion', () => {
  it('encodes well-known values', () => {
    expect(float32ToFloat16Bits(0)).toBe(0x0000);
    expect(float32ToFloat16Bits(-0)).toBe(0x8000);
    expect(float32ToFloat16Bits(1)).toBe(0x3c00);
    expect(float32ToFloat16Bits(-2)).toBe(0xc000);
    expect(float32ToFloat16Bits(65504)).toBe(0x7bff);
    expect(float32ToFloat16Bits(Infinity)).toBe(0x7c00);
    expect(float32ToFloat16Bits(-Infinity)).toBe(0xfc00);
    expect(float32ToFloat16Bits(1e9)).toBe(0x7bff); // clamps to max half
  });

  it('decodes well-known values', () => {
    expect(float16BitsToFloat32(0x3c00)).toBe(1);
    expect(float16BitsToFloat32(0xc000)).toBe(-2);
    expect(float16BitsToFloat32(0x7c00)).toBe(Infinity);
    expect(Number.isNaN(float16BitsToFloat32(0x7e00))).toBe(true);
    expect(float16BitsToFloat32(0x0001)).toBeCloseTo(5.960464477539063e-8, 12);
  });

  it('roundtrips typical feature values within half precision', () => {
    const values = new Float32Array([0.0001, -0.5, 0.33333, 3.14159, -7.25, 42.42, -0.00006104]);
    const bits = float32ArrayToFloat16Bits(values);
    for (let i = 0; i < values.length; i++) {
      const back = float16BitsToFloat32(bits[i]);
      const scale = Math.max(1, Math.abs(values[i]));
      // half precision: ~2^-11 relative error
      expect(Math.abs(back - values[i]) / scale).toBeLessThan(1.5e-3);
    }
  });

  it('flushes sub-subnormal magnitudes to signed zero', () => {
    expect(float32ToFloat16Bits(1e-9)).toBe(0x0000);
    expect(float32ToFloat16Bits(-1e-9)).toBe(0x8000);
  });
});
