/** Persian digit + formatting helpers. Persian digits are used in Persian UI
 *  text (Vazirmatn ss01 via .ve-fa-digits); LTR meta/stat captions keep Latin. */

const FA_DIGITS = ['۰', '۱', '۲', '۳', '۴', '۵', '۶', '۷', '۸', '۹'];

export function faDigits(value: string | number): string {
  return String(value).replace(/[0-9]/g, (d) => FA_DIGITS[Number(d)]);
}

export function faPercent(pct: number): string {
  return `${faDigits(Math.max(0, Math.min(100, Math.round(pct))))}٪`;
}

/** m:ss (hours roll into minutes only past 60m → h:mm:ss), Persian digits. */
export function faTimestamp(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, '0');
  const base = h > 0 ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
  return faDigits(base);
}

/** Decimal megabytes with Latin digits, for LTR meta lines. */
export function latinMB(bytes: number): string {
  return `${Math.round(bytes / 1e6)} MB`;
}

/** SRT/VTT cue clock: HH:MM:SS,mmm or HH:MM:SS.mmm */
export function cueClock(seconds: number, decimalSep: ',' | '.'): string {
  const ms = Math.max(0, Math.round(seconds * 1000));
  const h = String(Math.floor(ms / 3600000)).padStart(2, '0');
  const m = String(Math.floor((ms % 3600000) / 60000)).padStart(2, '0');
  const s = String(Math.floor((ms % 60000) / 1000)).padStart(2, '0');
  const frac = String(ms % 1000).padStart(3, '0');
  return `${h}:${m}:${s}${decimalSep}${frac}`;
}
