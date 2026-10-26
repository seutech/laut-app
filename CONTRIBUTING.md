# Contributing to Laut

Laut is a local-first transcription and research application. Contributions are welcome, especially reproducible Windows/Linux reports, long-recording measurements, accessibility fixes and careful transcript-quality evaluations.

## Before opening an issue

- Check existing issues and the limitations in `portable/VALIDATION.md`.
- State the Laut version, operating system, CPU architecture and whether a dedicated GPU is present.
- Include the exact operation, expected result and observed result.
- Attach logs only after removing names, source URLs, local paths and transcript/audio content you cannot publish.
- Do not upload private recordings, model weights, API credentials or copyrighted conference material.

## Development checks

```sh
cd portable
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 check-layers.py
```

Changes to platform capture code should also be exercised on real hardware. A cross-target compile is useful evidence but does not replace a device test.

Keep `laut-core` and `laut-engine` independent of egui and audio-device libraries. Preserve unknown `recording.json` fields, original media and reversible edit evidence. Never add a silent cloud fallback.

## Pull requests

Describe the concrete user problem, resulting behavior, relevant platform limits and the checks you ran. Keep unrelated formatting or generated build output out of the change. By submitting a contribution, you agree that it is licensed under this repository's MIT License.
