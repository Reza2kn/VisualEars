/** Persian Inverse Text Normalization (ITN): spoken number words → digits, at
 *  display time. The ASR tokenizer carries no numeral tokens, so the models
 *  spell numbers out ("هشت ماه", "هزار و نهصد و شصت و نه"); this post-processor
 *  rewrites those to Persian digits ("۸ ماه", "۱۹۶۹") for the reader.
 *
 *  Pure, dependency-free, fast — safe to run on every live partial. It is the
 *  industry-standard ITN approach: reliable on the common cases, imperfect on
 *  genuinely ambiguous ones (which is why a couple of high-risk homographs are
 *  guarded below). Integers only for now; decimals/fractions ("ممیز", "نیم")
 *  are intentionally left spelled out.
 */

import { faDigits } from '../format';
import { lang } from '../lang';

/** Additive number words. `place` encodes Persian number grammar so combination
 *  is validated (a term may only extend a group with a strictly smaller place):
 *  hundreds(2) → tens(1) → units/teens(0). */
interface AddWord {
  value: number;
  place: 0 | 1 | 2;
}

const ADD: Record<string, AddWord> = {
  صفر: { value: 0, place: 0 },
  یک: { value: 1, place: 0 },
  یه: { value: 1, place: 0 }, // colloquial "a/one"
  دو: { value: 2, place: 0 },
  سه: { value: 3, place: 0 },
  چهار: { value: 4, place: 0 },
  چار: { value: 4, place: 0 }, // colloquial
  شهار: { value: 4, place: 0 }, // model garble of چهار (benchmark)
  پنج: { value: 5, place: 0 },
  شش: { value: 6, place: 0 },
  شیش: { value: 6, place: 0 }, // colloquial
  هفت: { value: 7, place: 0 },
  هشت: { value: 8, place: 0 },
  نه: { value: 9, place: 0 },
  // teens (atomic, < 100, behave as place 0)
  ده: { value: 10, place: 0 },
  یازده: { value: 11, place: 0 },
  دوازده: { value: 12, place: 0 },
  دوازد: { value: 12, place: 0 }, // model garble (benchmark)
  سیزده: { value: 13, place: 0 },
  چهارده: { value: 14, place: 0 },
  چارده: { value: 14, place: 0 },
  پانزده: { value: 15, place: 0 },
  پونزده: { value: 15, place: 0 },
  شانزده: { value: 16, place: 0 },
  شونزده: { value: 16, place: 0 },
  هفده: { value: 17, place: 0 },
  هیفده: { value: 17, place: 0 },
  هجده: { value: 18, place: 0 },
  هیجده: { value: 18, place: 0 },
  هژده: { value: 18, place: 0 },
  نوزده: { value: 19, place: 0 },
  نونزده: { value: 19, place: 0 }, // colloquial
  // tens
  بیست: { value: 20, place: 1 },
  سی: { value: 30, place: 1 },
  چهل: { value: 40, place: 1 },
  چل: { value: 40, place: 1 },
  پنجاه: { value: 50, place: 1 },
  شصت: { value: 60, place: 1 },
  هفتاد: { value: 70, place: 1 },
  هشتاد: { value: 80, place: 1 },
  نود: { value: 90, place: 1 },
  // hundreds (with the colloquial/spoken spellings the model tends to emit)
  صد: { value: 100, place: 2 },
  یکصد: { value: 100, place: 2 },
  دویست: { value: 200, place: 2 },
  سیصد: { value: 300, place: 2 },
  چهارصد: { value: 400, place: 2 },
  چارصد: { value: 400, place: 2 }, // colloquial
  پانصد: { value: 500, place: 2 },
  پونصد: { value: 500, place: 2 }, // colloquial
  پنصد: { value: 500, place: 2 }, // colloquial
  پنجصد: { value: 500, place: 2 }, // colloquial
  ششصد: { value: 600, place: 2 },
  شیشصد: { value: 600, place: 2 },
  هفتصد: { value: 700, place: 2 },
  هفصد: { value: 700, place: 2 }, // colloquial
  هشتصد: { value: 800, place: 2 },
  هشصد: { value: 800, place: 2 }, // colloquial
  نهصد: { value: 900, place: 2 },
};

/** Multiplicative scale words. */
const SCALE: Record<string, number> = {
  هزار: 1_000,
  هزاره: 1_000, // frequent model garble (trailing ه) — see benchmark predictions
  میلیون: 1_000_000,
  ملیون: 1_000_000,
  میلیارد: 1_000_000_000,
  ملیارد: 1_000_000_000,
};

/** The connector «و» (o / "and") that joins number words. */
const CONNECTOR = 'و';

/** Single-word numbers that are far more often something else in speech, so we
 *  leave them alone when they stand entirely on their own. Inside a larger
 *  number ("بیست و یک", "صد و نه") they still convert. */
const GUARD_STANDALONE = new Set(['نه', 'یک', 'یه']); // «نه» = no, «یک»/«یه» = a/an

/** Normalize a token core for lookup: unify Arabic/Persian kaf+yeh, drop ZWNJ. */
function normalize(core: string): string {
  return core
    .replace(/ي/g, 'ی') // ARABIC YEH → PERSIAN YEH
    .replace(/ك/g, 'ک') // ARABIC KAF → PERSIAN KEHEH
    .replace(/‌/g, ''); // strip ZWNJ
}

type Kind = 'add' | 'scale' | 'conn' | 'other';

interface WordEntry {
  /** Index of this word in the whitespace-split `segs` array. */
  seg: number;
  /** Leading / trailing punctuation peeled off the raw token. */
  lead: string;
  trail: string;
  /** Normalized core used for classification. */
  core: string;
  kind: Kind;
  add?: AddWord;
  scale?: number;
}

const STRIP_RE = /^([^\p{L}\p{N}]*)([\s\S]*?)([^\p{L}\p{N}]*)$/u;

/** A number sub-part produced by de-gluing a run-together numeral token. */
type Part =
  | { kind: 'add'; add: AddWord }
  | { kind: 'scale'; scale: number }
  | { kind: 'conn' };

/** Scale words, longest-first, for matching a glued trailing scale. */
const SCALE_KEYS = Object.keys(SCALE).sort((a, b) => b.length - a.length);

/**
 * Decompose a normalized core into number sub-parts, splitting tokens the model
 * sometimes glues together (non-deterministically frame to frame):
 *   «دوهزار» → دو + هزار, «هزارو» → هزار + و, «دوهزارو» → دو + هزار + و.
 * Returns null when the core is not (part of) a number — kept to the closed set
 * of known number words + scales + «و», so it can't over-split real words.
 */
function decompose(core: string): Part[] | null {
  if (core === CONNECTOR) return [{ kind: 'conn' }];
  if (Object.prototype.hasOwnProperty.call(ADD, core)) return [{ kind: 'add', add: ADD[core] }];
  if (Object.prototype.hasOwnProperty.call(SCALE, core)) return [{ kind: 'scale', scale: SCALE[core] }];

  // Trailing glued connector «…و».
  if (core.length > CONNECTOR.length && core.endsWith(CONNECTOR)) {
    const head = decompose(core.slice(0, -CONNECTOR.length));
    if (head) return [...head, { kind: 'conn' }];
  }

  // Trailing glued scale «<number><scale>», e.g. دوهزار، صدهزار، سی‌میلیون.
  for (const s of SCALE_KEYS) {
    if (core.length > s.length && core.endsWith(s)) {
      const head = decompose(core.slice(0, -s.length));
      if (head && head.every((p) => p.kind !== 'conn')) {
        return [...head, { kind: 'scale', scale: SCALE[s] }];
      }
    }
  }
  return null;
}

/** Classify one whitespace token into one OR MORE word entries (one usually;
 *  more when a glued numeral like «دوهزار» is split). All share the token's
 *  `seg`; lead punctuation sticks to the first, trailing to the last. */
function classify(seg: number, raw: string): WordEntry[] {
  const m = STRIP_RE.exec(raw);
  const lead = m ? m[1] : '';
  const rawCore = m ? m[2] : raw;
  const trail = m ? m[3] : '';
  const core = normalize(rawCore);

  const parts = decompose(core);
  if (!parts) return [{ seg, lead, trail, core, kind: 'other' }];

  const lastIdx = parts.length - 1;
  return parts.map((p, idx) => ({
    seg,
    lead: idx === 0 ? lead : '',
    trail: idx === lastIdx ? trail : '',
    // A lone (un-glued) token keeps its real core for the standalone guard;
    // de-glued sub-parts are never single-entry runs, so '' is harmless.
    core: parts.length === 1 ? core : '',
    kind: p.kind,
    add: p.kind === 'add' ? p.add : undefined,
    scale: p.kind === 'scale' ? p.scale : undefined,
  }));
}

/**
 * True when the last token of `text` looks like (part of) a spoken number — a
 * Persian/Latin digit, a number word, or the connector «و». The live session
 * uses this to avoid finalizing an utterance in the middle of a number (which
 * would split e.g. "دو … هزار" into "۲" and "۱۰۰۰").
 */
export function endsWithSpokenNumber(text: string): boolean {
  const trimmed = text.trim();
  if (!trimmed) return false;
  const words = trimmed.split(/\s+/);
  const m = STRIP_RE.exec(words[words.length - 1]);
  const rawCore = m ? m[2] : words[words.length - 1];
  if (/[۰-۹0-9]$/.test(rawCore)) return true; // already digitized
  if (lang === 'en') {
    const core = rawCore.toLowerCase();
    return core === EN_AND || enClassifyCore(core) !== null;
  }
  const core = normalize(rawCore);
  if (!core) return false;
  return decompose(core) !== null;
}

interface ParseState {
  total: number;
  current: number;
  minPlace: number;
  lastScale: number;
}

function canExtend(state: ParseState, e: WordEntry): boolean {
  if (e.kind === 'add') return (e.add as AddWord).place < state.minPlace;
  if (e.kind === 'scale') return (e.scale as number) < state.lastScale;
  return false;
}

function apply(state: ParseState, e: WordEntry): void {
  if (e.kind === 'add') {
    state.current += (e.add as AddWord).value;
    state.minPlace = (e.add as AddWord).place;
  } else if (e.kind === 'scale') {
    const mult = state.current === 0 ? 1 : state.current;
    state.total += mult * (e.scale as number);
    state.lastScale = e.scale as number;
    state.current = 0;
    state.minPlace = 3;
  }
}

interface Replacement {
  startSeg: number;
  endSeg: number;
  text: string;
}

interface TwoDigitParse {
  value: number;
  end: number;
}

function parseTwoDigit(words: WordEntry[], start: number): TwoDigitParse | null {
  const first = words[start];
  if (!first || first.kind !== 'add' || !first.add || first.add.value < 0 || first.add.value > 99) {
    return null;
  }
  let value = first.add.value;
  let end = start;
  if (first.add.place === 1) {
    const conn = words[start + 1];
    const maybeOne = conn?.kind === 'conn' ? words[start + 2] : words[start + 1];
    if (maybeOne?.kind === 'add' && maybeOne.add?.place === 0 && maybeOne.add.value < 10) {
      value += maybeOne.add.value;
      end = conn?.kind === 'conn' ? start + 2 : start + 1;
    }
  }
  return { value, end };
}

function parseSpokenYear(words: WordEntry[], start: number): Replacement | null {
  const first = parseTwoDigit(words, start);
  if (!first || first.value < 10 || first.value > 20) return null;
  const second = parseTwoDigit(words, first.end + 1);
  if (!second || second.value < 0 || second.value > 99) return null;
  const value = first.value * 100 + second.value;
  if (value < 1000 || value > 2099) return null;
  const startWord = words[start];
  const endWord = words[second.end];
  return {
    startSeg: startWord.seg,
    endSeg: endWord.seg,
    text: startWord.lead + faDigits(value) + endWord.trail,
  };
}

function parseDigitSequence(words: WordEntry[], start: number): Replacement | null {
  const digits: number[] = [];
  let end = start;
  while (end < words.length) {
    const word = words[end];
    if (word.kind !== 'add' || !word.add || word.add.value < 0 || word.add.value > 9) break;
    digits.push(word.add.value);
    end++;
  }
  if (digits.length < 3) return null;
  const startWord = words[start];
  const endWord = words[end - 1];
  return {
    startSeg: startWord.seg,
    endSeg: endWord.seg,
    text: startWord.lead + digits.map(faDigits).join('') + endWord.trail,
  };
}

/**
 * Rewrite spoken Persian numbers in `text` to Persian digits.
 * Examples: "هشت ماه" → "۸ ماه"; "هزار و نهصد و شصت و نه" → "۱۹۶۹";
 *           "بیست و سه" → "۲۳"; "دو میلیون و سیصد هزار" → "۲۳۰۰۰۰۰".
 */
export function itnPersian(text: string): string {
  if (!text) return text;

  // Keep whitespace runs as their own segments so output spacing is preserved.
  const segs = text.split(/(\s+)/);
  const words: WordEntry[] = [];
  for (let s = 0; s < segs.length; s++) {
    if (segs[s] === '' || /^\s+$/.test(segs[s])) continue;
    words.push(...classify(s, segs[s]));
  }

  const replacements: Replacement[] = [];

  let i = 0;
  while (i < words.length) {
    const spokenYear = parseSpokenYear(words, i);
    if (spokenYear) {
      replacements.push(spokenYear);
      while (i < words.length && words[i].seg <= spokenYear.endSeg) i++;
      continue;
    }

    const digitSequence = parseDigitSequence(words, i);
    if (digitSequence) {
      replacements.push(digitSequence);
      while (i < words.length && words[i].seg <= digitSequence.endSeg) i++;
      continue;
    }

    const start = words[i];
    if (start.kind !== 'add' && start.kind !== 'scale') {
      i++;
      continue;
    }

    const state: ParseState = { total: 0, current: 0, minPlace: 3, lastScale: Infinity };
    apply(state, start);
    let last = i; // index in `words` of the last consumed number word
    let count = 1;
    let j = i;

    // Extend the run while the next number word validly continues the number,
    // optionally across a single «و» connector. A connector is only consumed
    // when a valid number word follows it.
    for (;;) {
      let nextIdx = -1;
      const a = words[j + 1];
      if (a && (a.kind === 'add' || a.kind === 'scale')) {
        nextIdx = j + 1;
      } else if (a && a.kind === 'conn') {
        const b = words[j + 2];
        if (b && (b.kind === 'add' || b.kind === 'scale')) nextIdx = j + 2;
      }
      if (nextIdx < 0) break;
      const ne = words[nextIdx];
      if (!canExtend(state, ne)) break;
      apply(state, ne);
      j = nextIdx;
      last = nextIdx;
      count++;
    }

    // Leave high-risk single words ("نه" = no, "یک" = a) untouched.
    if (count === 1 && GUARD_STANDALONE.has(start.core)) {
      i++;
      continue;
    }

    const value = state.total + state.current;
    const endWord = words[last];
    replacements.push({
      startSeg: start.seg,
      endSeg: endWord.seg,
      text: start.lead + faDigits(value) + endWord.trail,
    });
    i = last + 1;
  }

  if (replacements.length === 0) return text;

  // Rebuild, replacing each number run's whole segment span with its digits.
  const byStart = new Map<number, Replacement>();
  for (const r of replacements) byStart.set(r.startSeg, r);

  const out: string[] = [];
  let k = 0;
  while (k < segs.length) {
    const r = byStart.get(k);
    if (r) {
      out.push(r.text);
      k = r.endSeg + 1;
    } else {
      out.push(segs[k]);
      k++;
    }
  }
  return out.join('');
}

/* ============================================================
   English ITN — spoken English numbers → digits. English grammar
   differs from Persian: "hundred" is a ×100 multiplier ("two
   hundred" = 200), and compounds are hyphenated ("twenty-three").
   The connector is "and". Year-pairs ("nineteen eighty-four") and
   digit-by-digit ("one nine six nine") are handled too.
   ============================================================ */

const EN_SMALL: Record<string, number> = {
  zero: 0, oh: 0, one: 1, two: 2, three: 3, four: 4, five: 5, six: 6, seven: 7, eight: 8, nine: 9,
  ten: 10, eleven: 11, twelve: 12, thirteen: 13, fourteen: 14, fifteen: 15, sixteen: 16,
  seventeen: 17, eighteen: 18, nineteen: 19,
  twenty: 20, thirty: 30, forty: 40, fifty: 50, sixty: 60, seventy: 70, eighty: 80, ninety: 90,
};
const EN_SCALES: Record<string, number> = {
  thousand: 1_000, million: 1_000_000, billion: 1_000_000_000, trillion: 1_000_000_000_000,
};
const EN_AND = 'and';

type EnKind = 'small' | 'hundred' | 'scale' | 'and' | 'other';
interface EnPart {
  kind: EnKind;
  value?: number;
  scale?: number;
}

function enClassifyCore(core: string): EnPart | null {
  if (core === EN_AND) return { kind: 'and' };
  if (core === 'hundred') return { kind: 'hundred', scale: 100 };
  if (Object.prototype.hasOwnProperty.call(EN_SCALES, core)) return { kind: 'scale', scale: EN_SCALES[core] };
  if (Object.prototype.hasOwnProperty.call(EN_SMALL, core)) return { kind: 'small', value: EN_SMALL[core] };
  return null;
}

interface EnWord {
  seg: number;
  lead: string;
  trail: string;
  kind: EnKind;
  value?: number;
  scale?: number;
}

function enClassify(seg: number, raw: string): EnWord[] {
  const m = STRIP_RE.exec(raw);
  const lead = m ? m[1] : '';
  const rawCore = m ? m[2] : raw;
  const trail = m ? m[3] : '';
  const core = rawCore.toLowerCase();

  const whole = enClassifyCore(core);
  if (whole) return [{ seg, lead, trail, kind: whole.kind, value: whole.value, scale: whole.scale }];

  // Hyphenated number compound, e.g. "twenty-three" → twenty + three.
  if (core.includes('-')) {
    const parts = core.split('-').filter(Boolean);
    const cls = parts.map(enClassifyCore);
    if (parts.length > 1 && cls.every((c) => c && c.kind !== 'and' && c.kind !== 'other')) {
      const last = parts.length - 1;
      return parts.map((_part, idx) => {
        const c = cls[idx] as EnPart;
        return { seg, lead: idx === 0 ? lead : '', trail: idx === last ? trail : '', kind: c.kind, value: c.value, scale: c.scale };
      });
    }
  }
  return [{ seg, lead, trail, kind: 'other' }];
}

function enIsNum(w: EnWord | undefined): boolean {
  return !!w && (w.kind === 'small' || w.kind === 'hundred' || w.kind === 'scale');
}

/** Parse a maximal English number run from `start`; returns the value and the
 *  index of the last consumed number word, or null. */
function enParseRun(words: EnWord[], start: number): { value: number; last: number } | null {
  let total = 0;
  let current = 0;
  let last = -1;
  let i = start;
  for (;;) {
    const w = words[i];
    if (!w) break;
    if (w.kind === 'and') {
      if (last >= 0 && enIsNum(words[i + 1])) {
        i++;
        continue;
      }
      break;
    }
    if (w.kind === 'small') {
      current += w.value as number;
      last = i;
      i++;
    } else if (w.kind === 'hundred') {
      current = (current === 0 ? 1 : current) * 100;
      last = i;
      i++;
    } else if (w.kind === 'scale') {
      total += (current === 0 ? 1 : current) * (w.scale as number);
      current = 0;
      last = i;
      i++;
    } else {
      break;
    }
  }
  if (last < 0) return null;
  return { value: total + current, last };
}

/** A two-digit component for year parsing: a small (0-19), or a ten [+ unit]. */
function enTwoDigit(words: EnWord[], start: number): { value: number; last: number } | null {
  const a = words[start];
  if (!a || a.kind !== 'small') return null;
  let value = a.value as number;
  let last = start;
  if (value >= 20 && value % 10 === 0) {
    const b = words[start + 1];
    if (b && b.kind === 'small' && (b.value as number) < 10) {
      value += b.value as number;
      last = start + 1;
    }
  }
  return { value, last };
}

function enParseYear(words: EnWord[], start: number): { value: number; last: number } | null {
  const first = enTwoDigit(words, start);
  if (!first || first.value < 10 || first.value > 20) return null;
  const second = enTwoDigit(words, first.last + 1);
  if (!second || second.value < 0 || second.value > 99) return null;
  const value = first.value * 100 + second.value;
  if (value < 1000 || value > 2099) return null;
  return { value, last: second.last };
}

function enParseDigitSeq(words: EnWord[], start: number): { value: string; last: number } | null {
  const digits: number[] = [];
  let i = start;
  while (words[i] && words[i].kind === 'small' && (words[i].value as number) < 10) {
    digits.push(words[i].value as number);
    i++;
  }
  if (digits.length < 3) return null;
  return { value: digits.join(''), last: i - 1 };
}

export function itnEnglish(text: string): string {
  if (!text) return text;
  const segs = text.split(/(\s+)/);
  const words: EnWord[] = [];
  for (let s = 0; s < segs.length; s++) {
    if (segs[s] === '' || /^\s+$/.test(segs[s])) continue;
    words.push(...enClassify(s, segs[s]));
  }

  const replacements: Replacement[] = [];
  let i = 0;
  while (i < words.length) {
    const start = words[i];

    const year = enParseYear(words, i);
    if (year) {
      const endWord = words[year.last];
      replacements.push({ startSeg: start.seg, endSeg: endWord.seg, text: start.lead + String(year.value) + endWord.trail });
      i = year.last + 1;
      continue;
    }

    const seq = enParseDigitSeq(words, i);
    if (seq) {
      const endWord = words[seq.last];
      replacements.push({ startSeg: start.seg, endSeg: endWord.seg, text: start.lead + seq.value + endWord.trail });
      i = seq.last + 1;
      continue;
    }

    if (!enIsNum(start)) {
      i++;
      continue;
    }
    const run = enParseRun(words, i);
    if (!run) {
      i++;
      continue;
    }
    const endWord = words[run.last];
    replacements.push({ startSeg: start.seg, endSeg: endWord.seg, text: start.lead + String(run.value) + endWord.trail });
    i = run.last + 1;
  }

  if (replacements.length === 0) return text;
  const byStart = new Map<number, Replacement>();
  for (const r of replacements) byStart.set(r.startSeg, r);
  const out: string[] = [];
  let k = 0;
  while (k < segs.length) {
    const r = byStart.get(k);
    if (r) {
      out.push(r.text);
      k = r.endSeg + 1;
    } else {
      out.push(segs[k]);
      k++;
    }
  }
  return out.join('');
}

/** Common standalone acronyms whose following number is usually a separate
 *  count, not part of a product code — don't glue these (IT 5 سال ≠ IT5). */
const NO_GLUE_CODES = new Set(['IT', 'TV', 'PC', 'AI', 'PR', 'HR', 'EU', 'UN', 'UK', 'US', 'OK', 'DJ', 'ID', 'CEO', 'VIP']);

/** Glue a Latin product-code to the number right after it, as ASCII digits:
 *  "UCF ۲۱۱" → "UCF211", "DC ۱۲۵" → "DC125". Only fires on all-letter codes
 *  (2+ caps) directly followed by a number, sparing common standalone acronyms
 *  and codes that already carry a digit (MP3 ۱۶ stays "MP3 16"). */
function glueCodeNumbers(text: string): string {
  return text.replace(/\b([A-Z]{2,})\s+([0-9۰-۹]+)/g, (m, code: string, num: string) => {
    if (NO_GLUE_CODES.has(code)) return m;
    const ascii = num.replace(/[۰-۹]/g, (d) => String('۰۱۲۳۴۵۶۷۸۹'.indexOf(d)));
    return code + ascii;
  });
}

/** Inverse text normalization for the active UI language. */
export function itn(text: string): string {
  if (lang === 'en') return itnEnglish(text);
  return glueCodeNumbers(itnPersian(text));
}
