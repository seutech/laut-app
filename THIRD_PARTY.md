# Third-party components

Laut does not claim ownership of the models or the libraries it integrates.

| Component | Attribution / source | License |
| --- | --- | --- |
| Rust dependencies | Exact versions in `portable/Cargo.lock`; source and license files collected into binary packages | Per-crate SPDX license and included license files |
| egui / eframe | [emilk/egui](https://github.com/emilk/egui) | MIT OR Apache-2.0 |
| CPAL / Rodio | [RustAudio](https://github.com/RustAudio) | Apache-2.0; MIT OR Apache-2.0 |
| whisper.cpp 1.9.5 worker | [ggml-org/whisper.cpp](https://github.com/ggml-org/whisper.cpp/releases/tag/v1.9.5) | MIT |
| Whisper GGML weights | [whisper.cpp model conversion](https://huggingface.co/ggerganov/whisper.cpp) | MIT, as declared by the pinned model source |
| llama.cpp 0.7.0 workers | [ggml-org/llama.cpp](https://github.com/ggml-org/llama.cpp/releases/tag/v0.7.0) | MIT |
| Qwen3 GGUF weights | [Qwen](https://huggingface.co/Qwen) and pinned catalog sources | Apache-2.0 |
| sherpa-onnx 1.13.8 | [k2-fsa/sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx/releases/tag/v1.13.8) | Apache-2.0 plus bundled notices |
| Pyannote segmentation 3.0 | [pyannote](https://huggingface.co/pyannote/segmentation-3.0) | MIT |
| TitaNet Small | [NVIDIA NeMo](https://catalog.ngc.nvidia.com/orgs/nvidia/nemo/models/titanet_small) | Apache-2.0 / NeMo Toolkit terms declared by source |
| Tesseract and tessdata_fast | [tesseract-ocr](https://github.com/tesseract-ocr) | Apache-2.0 |
| Poppler command-line tools (external) | [Poppler](https://poppler.freedesktop.org/) | GPL-2.0-or-later |
| FluidAudio 0.17.5 | [FluidInference](https://github.com/FluidInference/FluidAudio) | Apache-2.0 |
| Fermion runtime 0.2.9 | [Fermion Research](https://github.com/fermionresearch/phonon) | Apache-2.0 |
| Phonon-2 weights | [Fermion Research](https://huggingface.co/FermionResearch/Phonon-2), derived from NVIDIA Parakeet v3 | CC-BY-4.0; upstream NOTICE describes changes |
| Parakeet v3 weights | [NVIDIA](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3), [MLX conversion](https://huggingface.co/mlx-community/parakeet-tdt-0.6b-v3) | CC-BY-4.0 |
| Multilingual E5 Small / Base weights (optional download) | [Microsoft / intfloat](https://huggingface.co/intfloat/multilingual-e5-small) | MIT |
| SQLite (macOS system library) | [SQLite](https://sqlite.org/) | Public domain |
| MLX / MLX-LM | [Apple ML Explore](https://github.com/ml-explore) | MIT |
| MLX-Audio | [Blaizzy / contributors](https://github.com/Blaizzy/mlx-audio) | MIT |
| yt-dlp 2026.10.21 (explicit separate download) | [yt-dlp contributors](https://github.com/yt-dlp/yt-dlp/releases/tag/2026.10.21) | GPL-3.0+ for the selected standalone bundle; dependencies retain their licenses |
| yt-dlp-ejs 0.8.0 (compatible challenge scripts) | [yt-dlp EJS](https://github.com/yt-dlp/ejs) | Unlicense AND MIT AND ISC, per installed distribution metadata |
| Deno (external executable) | [Deno](https://github.com/denoland/deno) | MIT and dependency notices |
| FFmpeg / FFprobe (external executables) | [FFmpeg licensing](https://ffmpeg.org/legal.html) | LGPL or GPL depending on the installed build |

Python dependencies are installed separately using `requirements-lock.txt`. They and their transitive dependencies retain the notices installed in their distributions. Optional downloaded models retain their model-card licenses. The MIT license for Laut does not relicense any model weights.

Optional link-import dependencies are installed separately with `requirements-download.txt`; their license files remain in `.runtime/download-venv`. Laut does not bundle Deno or FFmpeg. Redistributors who choose to bundle these tools must preserve the corresponding licenses and satisfy the requirements of their particular builds.

Release packages include the project license, this notice, the pinned whisper.cpp/llama.cpp worker licenses and collected license files for resolved Rust dependencies. No model weight is bundled; model downloads remain explicit and retain their source license metadata. No third-party source has been vendored or modified in this repository. Before redistributing a self-contained Python runtime, downloaded model, FFmpeg build or another optional program, include all corresponding licenses and notices from that distribution.
