#!/usr/bin/env python3
"""Export MohammadJRanjbar/parsbert-persian-punctuation to ONNX + int8.

BertForTokenClassification → single static graph (input_ids, attention_mask,
token_type_ids) → logits [B, T, 5]; labels: O, '.', ':', '،', '؟'.
int8 dynamic quantization covers MatMul/Gemm and the (huge, 100k×768)
embedding Gather so the artifact lands near ~165 MB.

Outputs into /tmp/ve_punct/export/:
  punct_fp32.onnx, punct_int8.onnx, tokenizer.json, config snippets,
  export_summary.json (incl. fp32↔int8 parity on validation sentences)
"""

import json
from pathlib import Path

import torch
import pyarrow.parquet as pq
from transformers import AutoTokenizer, BertForTokenClassification

MODEL_ID = "MohammadJRanjbar/parsbert-persian-punctuation"
OUT = Path("/tmp/ve_punct/export")
OUT.mkdir(parents=True, exist_ok=True)
VAL_PARQUET = "/tmp/ve_punct/validation-00000-of-00001.parquet"
MAX_LEN = 192
PARITY_N = 120

PUNCT_CHARS = {"،", ".", ":", "؟", "!", "؛", ",", "?", ";"}


def strip_words(sentence: str) -> list[str]:
    words = []
    for raw in sentence.split():
        w = raw.strip("".join(PUNCT_CHARS))
        if w:
            words.append(w)
    return words


def main() -> None:
    print("loading model + tokenizer…")
    tokenizer = AutoTokenizer.from_pretrained(MODEL_ID)
    model = BertForTokenClassification.from_pretrained(MODEL_ID, dtype=torch.float32)
    model.eval()
    print("labels:", model.config.id2label)

    tokenizer.save_pretrained(OUT / "tokenizer")

    sample = tokenizer("سلام دنیا چطوری", return_tensors="pt")
    inputs = (sample["input_ids"], sample["attention_mask"], sample["token_type_ids"])

    fp32_path = OUT / "punct_fp32.onnx"
    print("exporting fp32 onnx…")
    torch.onnx.export(
        model,
        inputs,
        str(fp32_path),
        input_names=["input_ids", "attention_mask", "token_type_ids"],
        output_names=["logits"],
        dynamic_axes={
            "input_ids": {0: "batch", 1: "seq"},
            "attention_mask": {0: "batch", 1: "seq"},
            "token_type_ids": {0: "batch", 1: "seq"},
            "logits": {0: "batch", 1: "seq"},
        },
        opset_version=17,
        dynamo=False,
    )
    print(f"fp32: {fp32_path.stat().st_size / 1e6:.1f} MB")

    print("quantizing int8 (weights incl. embedding Gather)…")
    from onnxruntime.quantization import QuantType, quantize_dynamic

    int8_path = OUT / "punct_int8.onnx"
    quantize_dynamic(
        str(fp32_path),
        str(int8_path),
        weight_type=QuantType.QInt8,
        op_types_to_quantize=["MatMul", "Gemm", "Gather"],
        extra_options={"MatMulConstBOnly": True},
    )
    print(f"int8: {int8_path.stat().st_size / 1e6:.1f} MB")

    print("parity check fp32(torch) vs int8(ort)…")
    import onnxruntime as ort

    sess = ort.InferenceSession(str(int8_path), providers=["CPUExecutionProvider"])
    rows = pq.ParquetFile(VAL_PARQUET).read_row_group(0).to_pylist()[:PARITY_N]
    total = agree = 0
    for row in rows:
        words = strip_words(row["sentence"])[:60]
        if len(words) < 3:
            continue
        enc = tokenizer(
            words,
            is_split_into_words=True,
            return_tensors="pt",
            truncation=True,
            max_length=MAX_LEN,
        )
        with torch.no_grad():
            ref = model(**enc).logits[0].argmax(-1).tolist()
        got = sess.run(
            ["logits"],
            {
                "input_ids": enc["input_ids"].numpy(),
                "attention_mask": enc["attention_mask"].numpy(),
                "token_type_ids": enc["token_type_ids"].numpy(),
            },
        )[0][0].argmax(-1).tolist()
        for a, b in zip(ref, got):
            total += 1
            agree += a == b
    parity = agree / max(1, total)
    print(f"argmax parity: {agree}/{total} = {parity:.4f}")

    summary = {
        "model": MODEL_ID,
        "labels": model.config.id2label,
        "fp32_bytes": fp32_path.stat().st_size,
        "int8_bytes": int8_path.stat().st_size,
        "parity_tokens": total,
        "parity_argmax_agreement": parity,
        "max_position_embeddings": model.config.max_position_embeddings,
    }
    (OUT / "export_summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=1))
    print(json.dumps(summary, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()
