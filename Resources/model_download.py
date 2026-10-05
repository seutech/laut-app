"""Explicit, resumable model downloads. Never receives library text or audio."""
import json
import hashlib
from pathlib import Path
import threading
import time


def download(request):
    from huggingface_hub import HfApi, hf_hub_download
    from tqdm.auto import tqdm
    model_id = request['modelID']
    info = HfApi().model_info(model_id, files_metadata=True, token=False)
    files = info.siblings
    if model_id.startswith('intfloat/multilingual-e5-'):
        allowed = {'config.json', 'model.safetensors', 'tokenizer.json', 'tokenizer_config.json',
                   'special_tokens_map.json', 'sentencepiece.bpe.model', 'README.md'}
        files = [f for f in files if f.rfilename in allowed]
        if not any(f.rfilename == 'model.safetensors' for f in files):
            raise ValueError('Suchmodell enthält keine Safetensors-Gewichte.')
    total = sum(f.size or 0 for f in files)
    completed = 0
    lock = threading.Lock()
    progress_path = request.get('progressPath')
    last_write = 0.0
    current_file = ''

    def report(part=0, force=False):
        nonlocal last_write
        if not progress_path:
            return
        with lock:
            now = time.monotonic()
            if not force and now - last_write < 0.15:
                return
            last_write = now
            target = Path(progress_path)
            temp = target.with_suffix('.tmp')
            temp.write_text(json.dumps({'completed': min(total, completed + part), 'total': total, 'file': current_file}))
            temp.replace(target)

    class Progress(tqdm):
        def update(self, n=1):
            result = super().update(n)
            report(self.n)
            return result

    destination = None
    for file in files:
        current_file = file.rfilename
        report(force=True)
        downloaded = hf_hub_download(model_id, file.rfilename, revision=info.sha,
                                     cache_dir=request['cache'], token=False, tqdm_class=Progress)
        actual = Path(downloaded)
        if file.size is not None and actual.stat().st_size != file.size:
            raise ValueError('Modelldatei hat eine unerwartete Größe: ' + file.rfilename)
        if file.lfs and file.lfs.sha256:
            with actual.open('rb') as stream:
                digest = hashlib.file_digest(stream, 'sha256').hexdigest()
            if digest != file.lfs.sha256:
                raise ValueError('Prüfsumme stimmt nicht: ' + file.rfilename)
        completed += file.size or 0
        report(force=True)
        # Snapshot root, even when the first file lives in a nested directory.
        destination = Path(downloaded)
        for _ in Path(file.rfilename).parts:
            destination = destination.parent
    if destination is None:
        raise ValueError('Leeres Modell-Repository.')
    marker = destination / '.laut-complete.json'
    marker.write_text(json.dumps({'modelID': model_id, 'revision': info.sha}))
    return {'path': str(destination)}
