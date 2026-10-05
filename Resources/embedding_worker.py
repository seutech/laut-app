#!/usr/bin/env python3
"""Laut's local E5 adapter. JSON lines over pipes; the parent denies network access."""
import contextlib
import json
import os
from pathlib import Path
import sys

_model = None
_tokenizer = None
_path = None


def execute(request):
    global _model, _tokenizer, _path
    for name in ('HF_HUB_OFFLINE', 'TRANSFORMERS_OFFLINE', 'HF_HUB_DISABLE_TELEMETRY', 'HF_HUB_DISABLE_IMPLICIT_TOKEN'):
        os.environ[name] = '1'
    path = str(Path(request['modelPath']).resolve(strict=True))
    with contextlib.redirect_stdout(sys.stderr):
        import torch
        from transformers import AutoModel, AutoTokenizer
        torch.set_num_threads(2)
        if _path != path:
            _model = None
            _tokenizer = AutoTokenizer.from_pretrained(path, local_files_only=True, trust_remote_code=False)
            _model = AutoModel.from_pretrained(path, local_files_only=True, trust_remote_code=False, use_safetensors=True).eval()
            _path = path
        texts = request['texts']
        if not 0 < len(texts) <= 16 or any(not isinstance(t, str) or len(t) > 50000 for t in texts):
            raise ValueError('Ungültige Textmenge für die lokale Suche.')
        prefix = 'query: ' if request.get('query') else 'passage: '
        result = []
        with torch.inference_mode():
            for text in texts:
                # Overflow windows retain even unusually long compounds/multilingual text.
                encoded = _tokenizer(prefix + text, return_tensors='pt', max_length=512,
                                     truncation=True, padding=True, return_overflowing_tokens=True, stride=32)
                encoded.pop('overflow_to_sample_mapping', None)
                window_vectors = []
                for offset in range(0, len(encoded['input_ids']), 4):
                    batch = {k: v[offset:offset + 4] for k, v in encoded.items()}
                    hidden = _model(**batch).last_hidden_state
                    mask = batch['attention_mask'].unsqueeze(-1).to(hidden.dtype)
                    pooled = (hidden * mask).sum(dim=1) / mask.sum(dim=1).clamp(min=1)
                    window_vectors.append(pooled)
                pooled = torch.cat(window_vectors).mean(dim=0)
                normalized = pooled / torch.linalg.vector_norm(pooled).clamp(min=1e-12)
                result.append(normalized.tolist())
        return {'vectors': result}


if __name__ == '__main__':
    for line in sys.stdin:
        try:
            response = execute(json.loads(line))
        except Exception as error:
            response = {'error': str(error)}
        print(json.dumps(response), flush=True)
