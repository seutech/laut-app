#!/usr/bin/env python3
"""Local JSON-lines inference worker. No sockets; parent denies all networking.

One model at a time stays warm. Downloads use a separate one-shot process.
fermion internal adapter is intentionally pinned to 0.2.9 in requirements-lock.txt.
"""
import argparse
import contextlib
import gc
import json
import os
from pathlib import Path
import sys
import time

_cached_key = None
_cached_model = None


def value(obj, key, default=None):
    return obj.get(key, default) if isinstance(obj, dict) else getattr(obj, key, default)


def load_local(kind, path):
    global _cached_key, _cached_model
    key = (kind, str(path))
    if _cached_key == key:
        return _cached_model
    _cached_model = None
    _cached_key = None
    gc.collect()
    import mlx.core as mx
    mx.clear_cache()
    if kind == "phonon":
        from fermion._speech import backends
        from fermion.transcribe import _resolve
        _, profile, pin, local_dir = _resolve(str(path))
        if local_dir is None:
            raise ValueError("Only an existing local Phonon directory is allowed")
        model = backends.load("mlx", local_dir, profile=profile, backend=pin["backend"], quiet=True)
    elif kind == "refine":
        from mlx_lm import load
        model = load(str(path))
    else:
        from mlx_audio.stt import load
        model = load(path, model_type="parakeet" if kind == "parakeet" else "qwen3_asr", strict=True)
    _cached_key, _cached_model = key, model
    return model


def parakeet_words(tokens):
    # These are subword tokens, not words. Spaces mark real word boundaries.
    words = []
    for token in tokens:
        text = value(token, "text", "")
        if not text:
            continue
        start, end = float(value(token, "start", 0)), float(value(token, "end", 0))
        if not words or text[0].isspace():
            words.append({"text": text.strip(), "start": start, "end": end})
        else:
            words[-1]["text"] += text
            words[-1]["end"] = end
    return [word for word in words if word["text"]]


def execute(request):
    operation = request["operation"]
    os.environ["HF_HUB_DISABLE_TELEMETRY"] = "1"
    os.environ["HF_HUB_DISABLE_IMPLICIT_TOKEN"] = "1"
    if operation == "download":
        from huggingface_hub import snapshot_download
        with contextlib.redirect_stdout(sys.stderr):
            path = snapshot_download(request["modelID"], cache_dir=request["cache"])
        return {"path": path}

    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
    model_path = Path(request["modelPath"])
    if not model_path.is_dir():
        raise ValueError("Modell fehlt. Bitte zuerst im Modellbereich herunterladen.")
    began = time.perf_counter()
    with contextlib.redirect_stdout(sys.stderr):
        kind = "refine" if operation == "refine" else request.get("engine", "parakeet")
        model = load_local(kind, model_path)
        if operation == "preload":
            return {"ready": True, "wall_seconds": time.perf_counter() - began}
        if kind == "refine":
            from mlx_lm import stream_generate
            model, tokenizer = model
            messages = [{"role": "system", "content": request["instructions"]}, {"role": "user", "content": request["text"]}]
            prompt = tokenizer.apply_chat_template(messages, tokenize=False, add_generation_prompt=True, enable_thinking=False)
            chunks = []
            final = None
            for response in stream_generate(model, tokenizer, prompt=prompt, max_tokens=4096):
                chunks.append(response.text)
                final = response
            if final is not None and final.finish_reason == "length":
                raise ValueError("Textmodell erreichte die Ausgabelänge. Bitte einen kürzeren Text auswählen.")
            return {"text": "".join(chunks), "wall_seconds": time.perf_counter() - began}
        if kind == "phonon":
            if hasattr(model, "set_hotwords"):
                model.set_hotwords(request.get("hotwords", [])[:25])
            output = model.transcribe_detailed(request["audio"])
            text, decode, duration = output.triple()
            return {"text": text, "segments": output.segments, "words": output.words if output.timed else [],
                    "duration_seconds": duration, "decode_seconds": decode,
                    "wall_seconds": time.perf_counter() - began, "truncated": bool(output.truncated)}

        options = {"chunk_duration": 30.0}
        if kind == "parakeet":
            options["overlap_duration"] = 2.0
        if kind == "qwen":
            language = {"de": "German", "en": "English"}.get(request.get("language"))
            if language:
                options["language"] = language
            options["hotwords"] = request.get("hotwords", [])
        output = model.generate(request["audio"], **options)
        import soundfile as sf
        duration = sf.info(request["audio"]).duration
        segments, words = [], []
        for segment in (value(output, "sentences", None) or value(output, "segments", None) or []):
            segments.append({"start": float(value(segment, "start", 0)), "end": float(value(segment, "end", duration)), "text": value(segment, "text", "")})
            tokens = value(segment, "tokens", None)
            if tokens:
                words.extend(parakeet_words(tokens))
            else:
                for word in value(segment, "words", []):
                    words.append({"text": value(word, "text", ""), "start": float(value(word, "start", 0)), "end": float(value(word, "end", 0))})
        text = value(output, "text", "")
        if not segments and text:
            segments = [{"start": 0.0, "end": duration, "text": text}]
        truncated = kind == "qwen" and value(output, "generation_tokens", 0) >= 8192
        return {"text": text, "segments": segments, "words": words, "duration_seconds": duration,
                "wall_seconds": time.perf_counter() - began, "truncated": truncated}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("request", nargs="?")
    parser.add_argument("--stream", action="store_true")
    args = parser.parse_args()
    if args.stream:
        for line in sys.stdin:
            try:
                result = execute(json.loads(line))
            except Exception as error:
                result = {"error": str(error)}
            print(json.dumps(result, ensure_ascii=False), flush=True)
    else:
        print(json.dumps(execute(json.loads(Path(args.request).read_text())), ensure_ascii=False))


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"Lokale Verarbeitung fehlgeschlagen: {error}", file=sys.stderr)
        sys.exit(1)
