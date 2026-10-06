"""Explicit online import helper. No accounts, browser cookies, plugins or remote code downloads.

Each job owns its directory. Completed media + result.json is the durable checkpoint;
failed partial downloads are discarded, while completed audio survives later ASR failure.
"""
import argparse
import datetime
import ipaddress
import json
import math
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import subprocess
import sys
import time
import urllib.parse
import urllib.request

MAX_BYTES = 2 * 1024**3
MAX_SECONDS = 6 * 3600
RESERVE_BYTES = 512 * 1024**2
ACTIVE = None
CANCELLED = False
PARENT_PID = os.getppid()


def check_cancelled():
    if CANCELLED or os.getppid() != PARENT_PID:
        raise InterruptedError('Auftrag unterbrochen. Fertiges Audio bleibt erhalten.')


def atomic_json(path, value):
    temp = path.with_suffix(path.suffix + '.tmp')
    temp.write_text(json.dumps(value, ensure_ascii=False), encoding='utf-8')
    temp.replace(path)


def stop_child():
    global ACTIVE
    child = ACTIVE
    if child is not None:
        try:
            os.killpg(child.pid, signal.SIGTERM)
            child.wait(timeout=2)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait()
        except ProcessLookupError:
            pass
        ACTIVE = None


def cancel(_signum, _frame):
    global CANCELLED
    CANCELLED = True
    stop_child()
    raise InterruptedError('Download pausiert. Fertiges Audio bleibt erhalten.')


def run(command, folder, label, timeout=180, enforce_size=False):
    global ACTIVE
    check_cancelled()
    out, err = folder / 'process.out', folder / 'process.err'
    atomic_json(folder / 'progress.json', {'message': label})
    began = time.monotonic()
    # Files avoid pipe deadlock, start_new_session permits cancellation of Deno/FFmpeg children too.
    with out.open('wb') as stdout, err.open('wb') as stderr:
        ACTIVE = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr,
                                  start_new_session=True)
        try:
            while ACTIVE.poll() is None:
                check_cancelled()
                if time.monotonic() - began > timeout:
                    raise TimeoutError('Zeitlimit erreicht. Bitte erneut versuchen.')
                if shutil.disk_usage(folder).free < RESERVE_BYTES:
                    raise RuntimeError('Zu wenig freier Speicher. Mindestens 512 MB müssen frei bleiben.')
                if out.stat().st_size + err.stat().st_size > 32 * 1024**2:
                    raise RuntimeError('Unerwartet große Antwort des Downloadwerkzeugs.')
                if enforce_size:
                    amount = sum(p.stat().st_size for p in folder.glob('source*') if p.is_file())
                    if amount > MAX_BYTES:
                        raise RuntimeError('Die Audiodatei überschreitet das Downloadlimit von 2 GiB.')
                    atomic_json(folder / 'progress.json', {'message': label, 'bytes': amount})
                time.sleep(0.2)
            code = ACTIVE.returncode
            if code:
                details = err.read_text(errors='replace')[-2500:]
                # Signed media URLs and tokens should not end up in the persistent UI error.
                details = re.sub(r'https?://\S+', '[Quellen-URL]', details)
                raise RuntimeError(details.strip() or 'Downloadwerkzeug wurde abgebrochen.')
            ACTIVE = None
        finally:
            stop_child()
    return out.read_text(encoding='utf-8')


def validate_public_url(url):
    parts = urllib.parse.urlsplit(url)
    if parts.scheme != 'https' or not parts.hostname or parts.username or parts.password or parts.port not in (None, 443):
        raise ValueError('Nur öffentliche HTTPS-Audiolinks werden unterstützt.')
    addresses = socket.getaddrinfo(parts.hostname, 443, type=socket.SOCK_STREAM)
    if not addresses or any(not ipaddress.ip_address(item[4][0]).is_global for item in addresses):
        raise ValueError('Lokale und private Netzwerkadressen werden nicht heruntergeladen.')
    return url


class PublicRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        validate_public_url(newurl)
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def fetch_audio(url, folder):
    began = time.monotonic()
    validate_public_url(url)
    path = folder / 'source.download'
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), PublicRedirect())
    request = urllib.request.Request(url, headers={'User-Agent': 'Laut/0.1.9', 'Accept': 'audio/*,application/octet-stream'})
    with opener.open(request, timeout=30) as response, path.open('wb') as output:
        content_type = response.headers.get_content_type()
        if not (content_type.startswith('audio/') or content_type in ('application/octet-stream', 'binary/octet-stream', 'application/ogg')):
            raise ValueError('Der Link liefert keine Audiodatei.')
        expected = int(response.headers.get('Content-Length', '0'))
        if expected > MAX_BYTES:
            raise ValueError('Die Audiodatei überschreitet das Downloadlimit von 2 GiB.')
        if expected and shutil.disk_usage(folder).free < expected * 2 + RESERVE_BYTES:
            raise ValueError('Zu wenig Speicher für Download und Audioverarbeitung.')
        count = 0
        while data := response.read(1024 * 1024):
            check_cancelled()
            if time.monotonic() - began > 3600:
                raise TimeoutError('Zeitlimit erreicht. Bitte erneut versuchen.')
            count += len(data)
            if count > MAX_BYTES or shutil.disk_usage(folder).free < RESERVE_BYTES:
                raise ValueError('Downloadlimit erreicht oder zu wenig freier Speicher.')
            output.write(data)
            atomic_json(folder / 'progress.json', {'message': 'Audiodatei laden', 'bytes': count, 'total': expected})
        if expected and count != expected:
            raise ValueError('Unvollständiger Download. Bitte erneut versuchen.')
    return path


def inspect_audio(path, folder, ffprobe):
    probe = json.loads(run([ffprobe, '-v', 'error', '-show_streams', '-show_format', '-of', 'json', str(path)], folder, 'Audiodatei prüfen'))
    streams = probe.get('streams', [])
    if not any(s.get('codec_type') == 'audio' for s in streams):
        raise ValueError('Die heruntergeladene Datei enthält keine Audiospur.')
    if any(s.get('codec_type') == 'video' and not s.get('disposition', {}).get('attached_pic') for s in streams):
        raise ValueError('Die Quelle enthält Video. Laut lädt ausschließlich Audio.')
    duration = float(probe.get('format', {}).get('duration', 0))
    if not math.isfinite(duration) or not 0 < duration <= MAX_SECONDS:
        raise ValueError('Der Linkimport unterstützt abgeschlossene Aufnahmen bis sechs Stunden.')
    return duration


def yt_arguments(args):
    return [args.ytdlp, '--ignore-config', '--no-plugin-dirs', '--no-remote-components', '--no-cache-dir',
            '--no-playlist', '--no-update', '--js-runtimes', 'deno:' + args.deno,
            '--socket-timeout', '20', '--retries', '2', '--fragment-retries', '2', '--abort-on-unavailable-fragments',
            '--extractor-retries', '1', '--format', 'bestaudio[ext=m4a]/bestaudio',
            '--max-filesize', str(MAX_BYTES), '--ffmpeg-location', str(Path(args.ffmpeg).parent)]


def download(args):
    check_cancelled()
    folder = Path(args.folder)
    folder.mkdir(parents=True, exist_ok=True, mode=0o700)
    result_path = folder / 'result.json'
    if result_path.exists():
        result = json.loads(result_path.read_text())
        filename = result.get('filename', '')
        if filename != 'audio.m4a' or result.get('source', {}).get('url') != args.url:
            raise ValueError('Ungültiger Download-Zwischenstand.')
        media = folder / filename
        if media.is_file() and not media.is_symlink() and media.stat().st_size == result.get('fileSize'):
            return result
        result_path.unlink()
    # Partial files are deliberately re-downloaded; never reuse an unverified partial as complete audio.
    for path in folder.glob('source*'):
        if path.is_file() or path.is_symlink():
            path.unlink()
    for name in ['audio.m4a', 'audio.partial.m4a']:
        (folder / name).unlink(missing_ok=True)
    if shutil.disk_usage(folder).free < 1024**3:
        raise ValueError('Für den Linkimport muss mindestens 1 GiB Speicher frei sein.')
    metadata = {'url': args.url, 'provider': args.provider,
                'importedAt': datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')}
    expected_duration = None
    if args.provider == 'YouTube':
        if not re.fullmatch(r'https://www\.youtube\.com/watch\?v=[A-Za-z0-9_-]{11}', args.url):
            raise ValueError('Ungültiger YouTube-Videolink.')
        info = json.loads(run(yt_arguments(args) + ['--dump-single-json', '--skip-download', '--', args.url], folder, 'YouTube-Quelle prüfen'))
        if info.get('_type') in ('playlist', 'multi_video') or info.get('is_live') or info.get('live_status') in ('is_live', 'is_upcoming', 'post_live'):
            raise ValueError('Bitte ein einzelnes, vollständig veröffentlichtes Video verwenden.')
        duration = float(info.get('duration') or 0)
        if not 0 < duration <= MAX_SECONDS:
            raise ValueError('Der Linkimport unterstützt Videos bis sechs Stunden.')
        expected_duration = duration
        if info.get('vcodec') not in ('none', None) or info.get('acodec') in ('none', None):
            raise ValueError('Keine separate Audiospur verfügbar.')
        size = info.get('filesize') or info.get('filesize_approx') or 0
        if size > MAX_BYTES or shutil.disk_usage(folder).free < size * 2 + RESERVE_BYTES:
            raise ValueError('Audiodatei zu groß oder zu wenig freier Speicher.')
        metadata.update(externalID=info.get('id'), originalTitle=info.get('title'),
                        publisher=info.get('channel') or info.get('uploader'), language=info.get('language'))
        published = info.get('upload_date')
        if published and re.fullmatch(r'\d{8}', published):
            metadata['publishedOn'] = datetime.datetime.strptime(published, '%Y%m%d').strftime('%Y-%m-%d')
        title = info.get('title') or 'YouTube-Audio'
        run(yt_arguments(args) + ['--no-progress', '--no-part', '-o', str(folder / 'source.%(ext)s'), '--', args.url],
            folder, 'YouTube-Audio laden', timeout=3600, enforce_size=True)
        sources = [p for p in folder.glob('source.*') if p.is_file() and not p.is_symlink()]
        if len(sources) != 1:
            raise ValueError('Download lieferte keine eindeutige vollständige Audiodatei.')
        source = sources[0]
    else:
        source = fetch_audio(args.url, folder)
        title = Path(urllib.parse.unquote(urllib.parse.urlsplit(args.url).path)).stem or 'Audio-Link'
        metadata['originalTitle'] = title
    duration = inspect_audio(source, folder, args.ffprobe)
    if expected_duration is not None and abs(duration - expected_duration) > max(3, expected_duration * 0.005):
        raise ValueError('Die geladene Audiospur stimmt nicht mit der angekündigten Länge überein. Bitte erneut versuchen.')
    if source.stat().st_size > MAX_BYTES:
        raise ValueError('Die Audiodatei überschreitet das Downloadlimit.')
    target = folder / 'audio.m4a'
    if source.suffix == '.m4a':
        source.replace(target)
    else:
        partial = folder / 'audio.partial.m4a'
        run([args.ffmpeg, '-nostdin', '-v', 'error', '-y', '-i', str(source), '-map', '0:a:0', '-vn',
             '-c:a', 'aac', '-b:a', '128k', '-movflags', '+faststart', str(partial)], folder, 'Audio für Laut vorbereiten', timeout=3600)
        converted = inspect_audio(partial, folder, args.ffprobe)
        if abs(converted - duration) > 2:
            raise ValueError('Die Audio-Konvertierung ist unvollständig.')
        partial.replace(target)
        source.unlink()
    result = {'filename': target.name, 'fileSize': target.stat().st_size, 'title': title, 'duration': duration, 'source': metadata}
    atomic_json(result_path, result)
    atomic_json(folder / 'progress.json', {'message': 'Audio vollständig geladen'})
    return result


def main():
    parser = argparse.ArgumentParser()
    for name in ('url', 'provider', 'folder', 'ytdlp', 'deno', 'ffmpeg', 'ffprobe'):
        parser.add_argument('--' + name, required=True)
    args = parser.parse_args()
    signal.signal(signal.SIGTERM, cancel)
    signal.signal(signal.SIGINT, cancel)
    os.umask(0o077)
    try:
        print(json.dumps(download(args), ensure_ascii=False))
    except Exception as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
    finally:
        stop_child()
        for name in ('process.out', 'process.err'):
            (Path(args.folder) / name).unlink(missing_ok=True)


if __name__ == '__main__':
    main()
