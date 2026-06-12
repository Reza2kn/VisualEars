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

interface F16ArrayCtor {
  new (buffer: ArrayBufferLike, byteOffset?: number, length?: number): ArrayBufferView &
    ArrayLike<number>;
}

/** Native Float16Array (ES2024+). When it exists, ORT requires fp16 tensor
 *  data to be a Float16Array — Uint16Array is only accepted without it. */
export const NativeFloat16Array = (globalThis as Record<string, unknown>).Float16Array as
  | F16ArrayCtor
  | undefined;

/** Wrap packed fp16 bits in whatever container this runtime's ORT accepts.
 *  A Float16Array view reinterprets the same buffer — no copy, no conversion. */
export function fp16TensorData(packed: Uint16Array): ArrayLike<number> {
  return NativeFloat16Array
    ? new NativeFloat16Array(packed.buffer, packed.byteOffset, packed.length)
    : packed;
}

/** Normalize an fp16/fp32 logits payload to values + how to read them:
 *  native Float16Array elements are already numbers ('float32' semantics),
 *  Uint16Array carries raw bits ('float16'). */
export function logitsNumericView(
  data: unknown,
  declaredType: string,
): { values: ArrayLike<number>; type: LogitsType } {
  if (declaredType === 'float16') {
    if (NativeFloat16Array && data instanceof (NativeFloat16Array as unknown as new () => object)) {
      return { values: data as ArrayLike<number>, type: 'float32' };
    }
    return { values: data as ArrayLike<number>, type: 'float16' };
  }
  return { values: data as ArrayLike<number>, type: 'float32' };
}
