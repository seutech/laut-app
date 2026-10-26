#!/usr/bin/env python3
"""Download a reviewed conference manifest through Laut's normal audio importer.

Usage: .runtime/venv/bin/python scripts/download-conference.py MANIFEST.json
Successful downloads are reused by media_download.py; failures do not stop other talks.
"""
import concurrent.futures
import json
from pathlib import Path
import subprocess
import sys
import re
from conference_storage import prepare_manifest, resolve, attach, digest

project = Path(__file__).resolve().parent.parent
manifest = Path(sys.argv[1]).resolve()
items = prepare_manifest(manifest)["items"]


def download(item):
    if not re.fullmatch(r"[a-z0-9-]+", item["slug"]):
        raise ValueError("Invalid conference slug")
    folder = manifest.parent / "downloads" / item["slug"]
    folder.mkdir(parents=True, exist_ok=True)
    result_file = folder / 'result.json'
    if result_file.exists():
        data = json.loads(result_file.read_text())
        if data['source']['url'] != item['url'] or data['filename'] != 'audio.m4a' or data['source'].get('clipStart') is not None:
            raise ValueError('Conflicting completed download')
        media = resolve(folder, data['filename'])
        if media.stat().st_size != data['fileSize'] or (media.parent.name == 'assets' and digest(media) != media.stem):
            raise ValueError('Completed download failed verification')
        attach(manifest.parent, folder, data['filename'], media, remove_local=True)
        print('REUSE ' + item['slug'], flush=True)
        return {'slug': item['slug'], 'ok': True, 'duration': data['duration'], 'bytes': data['fileSize'], 'reused': True}
    command = [str(project / ".runtime/venv/bin/python"), str(project / "Resources/media_download.py"),
               "--url", item["url"], "--provider", "YouTube", "--folder", str(folder),
               "--ytdlp", str(project / ".runtime/download-venv/bin/yt-dlp"),
               "--deno", "/opt/homebrew/bin/deno", "--ffmpeg", "/opt/homebrew/bin/ffmpeg",
               "--ffprobe", "/opt/homebrew/bin/ffprobe"]
    print("START " + item["slug"], flush=True)
    with (folder / "download.log").open("w") as log:
        result = subprocess.run(command, stdout=log, stderr=log)
    status = {"slug": item["slug"], "ok": result.returncode == 0}
    if status["ok"]:
        data = json.loads((folder / "result.json").read_text())
        attach(manifest.parent, folder, data['filename'], folder / data['filename'], remove_local=True)
        status.update(duration=data["duration"], bytes=data["fileSize"])
    print(json.dumps(status), flush=True)
    return status


def guarded_download(item):
    try:
        return download(item)
    except Exception as error:
        status = {'slug': item['slug'], 'ok': False, 'error': str(error)}
        print(json.dumps(status), flush=True)
        return status


with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
    results = list(pool.map(guarded_download, items))
(manifest.parent / "download-report.json").write_text(json.dumps(results, indent=2))
sys.exit(0 if all(r["ok"] for r in results) else 1)
