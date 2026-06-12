#!/usr/bin/env python3
"""Upload tokenizer + preprocessor sidecars to the VisualEars ONNX model repos.

The browser (and future native apps) need two small files next to the acoustic
core: the CTC vocabulary (tokens.json, extracted from the .nemo) and the exact
log-mel feature spec (preprocessor.json). This publishes the copies committed
in webapp/src/engine/ as the shared contract.

Usage:
  HF_TOKEN=… python3 tools/upload_sidecars.py          # uses env or .context/.env
"""

import os
from pathlib import Path

from huggingface_hub import HfApi

ROOT = Path(__file__).resolve().parent.parent
FILES = {
    "tokens.json": ROOT / "webapp/src/engine/tokens.json",
    "preprocessor.json": ROOT / "webapp/src/engine/preprocessor.json",
}
REPOS = [
    "Reza2kn/visualears-fastconformer-fa-full-ab-onnx-fp16",
    "Reza2kn/visualears-fastconformer-fa-full-ab-onnx-fp",
    "Reza2kn/visualears-fastconformer-fa-full-ab-onnx-w4",
]


def load_token() -> str:
    token = os.environ.get("HF_TOKEN")
    if token:
        return token
    env = ROOT / ".context/.env"
    if env.exists():
        for line in env.read_text().splitlines():
            if line.startswith("HF_TOKEN="):
                return line.split("=", 1)[1].strip()
    raise SystemExit("HF_TOKEN not found (env var or .context/.env)")


def main() -> None:
    api = HfApi(token=load_token())
    for repo in REPOS:
        for name, path in FILES.items():
            api.upload_file(
                path_or_fileobj=str(path),
                path_in_repo=name,
                repo_id=repo,
                repo_type="model",
                commit_message=f"Add {name} sidecar (browser/native runtime contract)",
            )
            print(f"uploaded {name} -> {repo}")


if __name__ == "__main__":
    main()
