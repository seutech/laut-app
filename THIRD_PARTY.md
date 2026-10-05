# Third-party components

Laut does not claim ownership of the models or the libraries it integrates.

| Component | Attribution / source | License |
| --- | --- | --- |
| FluidAudio 0.17.5 | [FluidInference](https://github.com/FluidInference/FluidAudio) | Apache-2.0 |
| Fermion runtime 0.2.9 | [Fermion Research](https://github.com/fermionresearch/phonon) | Apache-2.0 |
| Phonon-2 weights | [Fermion Research](https://huggingface.co/FermionResearch/Phonon-2), derived from NVIDIA Parakeet v3 | CC-BY-4.0; upstream NOTICE describes changes |
| Parakeet v3 weights | [NVIDIA](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3), [MLX conversion](https://huggingface.co/mlx-community/parakeet-tdt-0.6b-v3) | CC-BY-4.0 |
| MLX / MLX-LM | [Apple ML Explore](https://github.com/ml-explore) | MIT |
| MLX-Audio | [Blaizzy / contributors](https://github.com/Blaizzy/mlx-audio) | MIT |

Python dependencies are installed separately using `requirements-lock.txt`. They and their transitive dependencies retain the notices installed in their distributions. Optional downloaded models retain their model-card licenses. The MIT license for Laut does not relicense any model weights.

The local app bundle includes FluidAudio's license and this notice. No third-party source has been vendored or modified in this repository. Before distributing a self-contained Python runtime or bundled models, include all corresponding licenses and notices from those distributions.
