#!/usr/bin/env python3
"""Calibrate boundary-decision thresholds for the punctuation scorer, following
the paper's protocol (Efficient Punctuation Restoration via Weighted Lookahead
Scoring, arXiv 2606.05179): boundary-wise labels on a fixed transcript,
validation grid-search of decision thresholds, punct-only macro F1 objective,
final report on the held-out test split.

Adaptation to a discriminative scorer (ParsBERT token classifier): the decision
margin at each word boundary is Δ_c = log p(c) − log p(O); the mark argmax_c is
inserted iff Δ > τ_c with per-class τ calibrated on validation (coordinate
ascent over the paper's τ grid).

Outputs /tmp/ve_punct/export/punct-config.json + calibration_summary.json
and a TS golden fixture for the webapp tests.
"""

import json
import math
from collections import defaultdict
from pathlib import Path

import numpy as np
import onnxruntime as ort
import pyarrow.parquet as pq
from transformers import AutoTokenizer

EXPORT = Path("/tmp/ve_punct/export")
MODEL = EXPORT / "punct_int8.onnx"
VAL_PARQUET = "/tmp/ve_punct/validation-00000-of-00001.parquet"
TEST_PARQUET = "/tmp/ve_punct/test-00000-of-00001.parquet"

LABELS = ["O", ".", ":", "،", "؟"]  # model id2label order: O, B-., B-:, B-،, B-؟
MARK_MAP = {"،": "،", ".": ".", ":": ":", "؟": "؟", "!": ".", "؛": "،", ",": "،", "?": "؟"}
STRIP = "".join(MARK_MAP) + ";"
TAU_GRID = [x / 4 for x in range(-12, 8)]  # −3.00 … 1.75 step 0.25 (paper)
MAX_LEN = 192
VAL_SENTENCES = 4000
TEST_SENTENCES = 5000
BATCH = 32


def boundary_rows(parquet: str, limit: int) -> list[tuple[list[str], list[str]]]:
    rows = []
    pf = pq.ParquetFile(parquet)
    for rg in range(pf.num_row_groups):
        for row in pf.read_row_group(rg).to_pylist():
            words, labels = [], []
            for raw in str(row["sentence"]).split():
                w = raw.strip(STRIP)
                if not w:
                    continue
                mark = "O"
                rest = raw[len(raw.rstrip(STRIP)) :]
                for ch in rest:
                    if ch in MARK_MAP:
                        mark = MARK_MAP[ch]
                        break
                words.append(w)
                labels.append(mark)
            if 4 <= len(words) <= 80 and any(l != "O" for l in labels):
                rows.append((words, labels))
            if len(rows) >= limit:
                return rows
    return rows


class Scorer:
    def __init__(self) -> None:
        self.tokenizer = AutoTokenizer.from_pretrained(EXPORT / "tokenizer")
        self.sess = ort.InferenceSession(str(MODEL), providers=["CPUExecutionProvider"])

    def word_logprobs(self, batch_words: list[list[str]], align: str) -> list[np.ndarray]:
        enc = self.tokenizer(
            batch_words,
            is_split_into_words=True,
            padding=True,
            truncation=True,
            max_length=MAX_LEN,
            return_tensors="np",
        )
        logits = self.sess.run(
            ["logits"],
            {
                "input_ids": enc["input_ids"].astype(np.int64),
                "attention_mask": enc["attention_mask"].astype(np.int64),
                "token_type_ids": enc["token_type_ids"].astype(np.int64),
            },
        )[0]
        out = []
        for b, words in enumerate(batch_words):
            word_ids = enc.word_ids(b)
            pos: dict[int, int] = {}
            for t, wid in enumerate(word_ids):
                if wid is None:
                    continue
                if align == "first":
                    pos.setdefault(wid, t)
                else:
                    pos[wid] = t
            lg = logits[b]
            rows = np.full((len(words), len(LABELS)), -30.0, dtype=np.float64)
            for wid, t in pos.items():
                if wid < len(words):
                    z = lg[t] - lg[t].max()
                    p = np.exp(z)
                    rows[wid] = np.log(p / p.sum() + 1e-12)
            out.append(rows)
        return out


def collect(scorer: Scorer, rows, align: str):
    """→ flat arrays: gold label idx, per-class Δ=logp(c)−logp(O), argmax class."""
    gold, deltas = [], []
    for i in range(0, len(rows), BATCH):
        batch = rows[i : i + BATCH]
        lps = scorer.word_logprobs([w for w, _ in batch], align)
        for (words, labels), lp in zip(batch, lps):
            for j in range(len(words)):
                gold.append(LABELS.index(labels[j]))
                deltas.append(lp[j, 1:] - lp[j, 0])
    return np.array(gold), np.array(deltas)


def decide(deltas: np.ndarray, taus: np.ndarray) -> np.ndarray:
    """0 = O, else 1..4; insert best class only when its margin clears τ_class."""
    best = deltas.argmax(axis=1)
    margin = deltas[np.arange(len(deltas)), best]
    return np.where(margin > taus[best], best + 1, 0)


def f1_report(gold: np.ndarray, pred: np.ndarray):
    per = {}
    for c in range(len(LABELS)):
        tp = int(((pred == c) & (gold == c)).sum())
        fp = int(((pred == c) & (gold != c)).sum())
        fn = int(((pred != c) & (gold == c)).sum())
        p = tp / (tp + fp) if tp + fp else 0.0
        r = tp / (tp + fn) if tp + fn else 0.0
        f1 = 2 * p * r / (p + r) if p + r else 0.0
        per[LABELS[c]] = {"precision": round(p, 4), "recall": round(r, 4), "f1": round(f1, 4)}
    punct = [per[l]["f1"] for l in LABELS[1:]]
    return {
        "per_class": per,
        "macro_f1_punct_only": round(sum(punct) / len(punct), 4),
        "macro_f1_with_O": round(sum(per[l]["f1"] for l in LABELS) / len(LABELS), 4),
    }


def punct_macro(gold, pred) -> float:
    return f1_report(gold, pred)["macro_f1_punct_only"]


def main() -> None:
    scorer = Scorer()
    val = boundary_rows(VAL_PARQUET, VAL_SENTENCES)
    test = boundary_rows(TEST_PARQUET, TEST_SENTENCES)
    print(f"val sentences: {len(val)} | test sentences: {len(test)}")

    # 1. Pick subword↔word alignment by raw argmax F1 on a validation slice.
    probe = val[:600]
    best_align, best_score = "first", -1.0
    for align in ("first", "last"):
        g, d = collect(scorer, probe, align)
        score = punct_macro(g, decide(d, np.zeros(4) - 100))  # τ=−∞ ⇒ plain argmax
        print(f"alignment={align}: raw punct macro F1 {score:.4f}")
        if score > best_score:
            best_align, best_score = align, score
    print("chosen alignment:", best_align)

    g_val, d_val = collect(scorer, val, best_align)
    print(f"val boundaries: {len(g_val)}")

    # 2. Coordinate-ascent per-class τ over the paper's grid.
    taus = np.zeros(4)
    raw = punct_macro(g_val, decide(d_val, np.zeros(4) - 100))
    best = punct_macro(g_val, decide(d_val, taus))
    for _ in range(2):
        for c in range(4):
            for t in TAU_GRID:
                trial = taus.copy()
                trial[c] = t
                s = punct_macro(g_val, decide(d_val, trial))
                if s > best:
                    best, taus = s, trial
    print(f"val raw argmax: {raw:.4f} → calibrated: {best:.4f} | taus={taus.tolist()}")

    # 3. Held-out test report.
    g_test, d_test = collect(scorer, test, best_align)
    report_raw = f1_report(g_test, decide(d_test, np.zeros(4) - 100))
    report_cal = f1_report(g_test, decide(d_test, taus))
    print("TEST raw:      ", json.dumps(report_raw, ensure_ascii=False))
    print("TEST calibrated:", json.dumps(report_cal, ensure_ascii=False))

    config = {
        "version": 1,
        "model": "parsbert-persian-punctuation int8 onnx",
        "labels": LABELS,
        "alignment": best_align,
        "thresholds": {LABELS[c + 1]: taus[c] for c in range(4)},
        "max_len": MAX_LEN,
        "protocol": "arXiv 2606.05179 boundary-wise decisions, margin Δ=logp(c)−logp(O) > τ_c, τ grid −3.00..1.75/0.25, punct-only macro F1 objective",
        "validation": {"sentences": len(val), "raw_macro_f1": raw, "calibrated_macro_f1": best},
        "test": {"raw": report_raw, "calibrated": report_cal},
    }
    (EXPORT / "punct-config.json").write_text(json.dumps(config, ensure_ascii=False, indent=1))

    # 4. Golden fixture for the webapp tests (tokenizer + walker parity).
    golden = []
    for words, labels in test[:6]:
        enc = scorer.tokenizer([words], is_split_into_words=True)
        lp = scorer.word_logprobs([words], best_align)[0]
        d = lp[:, 1:] - lp[:, [0]]
        pred = decide(d, taus)
        golden.append(
            {
                "words": words,
                "gold": labels,
                "token_ids": enc["input_ids"][0],
                "pred_marks": [("O" if p == 0 else LABELS[p]) for p in pred],
            }
        )
    (EXPORT / "punct_golden.json").write_text(json.dumps(golden, ensure_ascii=False))
    print("wrote punct-config.json + punct_golden.json")


if __name__ == "__main__":
    main()
