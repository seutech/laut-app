# Third-party components

Laut does not claim ownership of the models or the libraries it integrates.

| Component | Attribution / source | License |
| --- | --- | --- |
| FluidAudio 0.17.5 | [FluidInference](https://github.com/FluidInference/FluidAudio) | Apache-2.0 |
| Fermion runtime 0.2.9 | [Fermion Research](https://github.com/fermionresearch/phonon) | Apache-2.0 |
| Phonon-2 weights | [Fermion Research](https://huggingface.co/FermionResearch/Phonon-2), derived from NVIDIA Parakeet v3 | CC-BY-4.0; upstream NOTICE describes changes |
| Parakeet v3 weights | [NVIDIA](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3), [MLX conversion](https://huggingface.co/mlx-community/parakeet-tdt-0.6b-v3) | CC-BY-4.0 |
| Multilingual E5 Small / Base weights (optional download) | [Microsoft / intfloat](https://huggingface.co/intfloat/multilingual-e5-small) | MIT |
| SQLite (macOS system library) | [SQLite](https://sqlite.org/) | Public domain |
| MLX / MLX-LM | [Apple ML Explore](https://github.com/ml-explore) | MIT |
| MLX-Audio | [Blaizzy / contributors](https://github.com/Blaizzy/mlx-audio) | MIT |
| yt-dlp 2026.08.19 (optional, separate runtime) | [yt-dlp contributors](https://github.com/yt-dlp/yt-dlp) | Unlicense; dependencies retain their licenses |
| yt-dlp-ejs 0.8.0 (compatible challenge scripts) | [yt-dlp EJS](https://github.com/yt-dlp/ejs) | Unlicense AND MIT AND ISC, per installed distribution metadata |
| Deno (external executable) | [Deno](https://github.com/denoland/deno) | MIT and dependency notices |
| FFmpeg / FFprobe (external executables) | [FFmpeg licensing](https://ffmpeg.org/legal.html) | LGPL or GPL depending on the installed build |

Python dependencies are installed separately using `requirements-lock.txt`. They and their transitive dependencies retain the notices installed in their distributions. Optional downloaded models retain their model-card licenses. The MIT license for Laut does not relicense any model weights.

Optional link-import dependencies are installed separately with `requirements-download.txt`; their license files remain in `.runtime/download-venv`. Laut does not bundle Deno or FFmpeg. Redistributors who choose to bundle these tools must preserve the corresponding licenses and satisfy the requirements of their particular builds.

The local app bundle includes FluidAudio's license and this notice. No third-party source has been vendored or modified in this repository. Before distributing a self-contained Python runtime or bundled models, include all corresponding licenses and notices from those distributions.
