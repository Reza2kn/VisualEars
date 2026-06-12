/** float32 ↔ float16 bit conversion — the fp16 "full IO" export takes and
 *  returns float16 tensors. Ported from the validated Space implementation. */

export function float32ToFloat16Bits(value: number): number {
  if (Number.isNaN(value)) return 0x7e00;
  if (value === Infinity) return 0x7c00;
  if (value === -Infinity) return 0xfc00;

  const sign = value < 0 || Object.is(value, -0) ? 0x8000 : 0;
  const abs = Math.abs(value);
  if (abs === 0) return sign;
  if (abs >= 65504) return sign | 0x7bff;
  if (abs < 5.960464477539063e-8) return sign;

  if (abs < 0.00006103515625) {
    return sign | Math.round(abs / 5.960464477539063e-8);
  }

  let exponent = Math.floor(Math.log2(abs));
  let mantissa = abs / 2 ** exponent - 1;
  let halfExponent = exponent + 15;
  let halfMantissa = Math.round(mantissa * 1024);
  if (halfMantissa === 1024) {
    halfMantissa = 0;
    halfExponent += 1;
  }
  if (halfExponent >= 31) return sign | 0x7bff;
  return sign | (halfExponent << 10) | (halfMantissa & 0x03ff);
}

export function float32ArrayToFloat16Bits(values: Float32Array): Uint16Array {
  const out = new Uint16Array(values.length);
  for (let i = 0; i < values.length; i++) out[i] = float32ToFloat16Bits(values[i]);
  return out;
}

export function float16BitsToFloat32(bits: number): number {
  const sign = bits & 0x8000 ? -1 : 1;
  const exponent = (bits >> 10) & 0x1f;
  const mantissa = bits & 0x03ff;
  if (exponent === 0) {
    return mantissa === 0 ? sign * 0 : sign * (mantissa / 1024) * 2 ** -14;
  }
  if (exponent === 31) {
    return mantissa ? NaN : sign * Infinity;
  }
  return sign * (1 + mantissa / 1024) * 2 ** (exponent - 15);
}

export type LogitsType = 'float32' | 'float16';

/** Read element `index` from a logits buffer of either precision. */
export function tensorValue(data: ArrayLike<number>, index: number, type: LogitsType): number {
  return type === 'float16' ? float16BitsToFloat32(data[index]) : data[index];
}
