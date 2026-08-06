# Windows runtime assets

These files make the public Windows source reproduce the exact contributor
build without depending on unrelated local macOS, web, or deployment folders.

The large `koochik_hd.onnx` model is stored with Git LFS:

- size: `138272920` bytes
- SHA-256: `64f5a3afbb5f603cdd44b56b3651d7a135562a69520e8a3386fe47b331c5a8f0`
- model family: [Shenava Koochik v1.0 tract streaming](https://huggingface.co/Reza2kn/Shenava-Koochik-v1.0-tract-streaming)

Run `git lfs install` and `git lfs pull` after cloning. `package.ps1` refuses to
package an LFS pointer, truncated file, or model with a different checksum.
