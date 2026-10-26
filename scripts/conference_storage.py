"""Version 1 portable conference media store, shared with LautCore.ConferenceMedia."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import tempfile
import unicodedata
from urllib.parse import quote

MARKER = 'conference-storage.json'
SIDECAR = 'media-references.json'


def atomic_json(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temp = tempfile.mkstemp(prefix='.' + path.name, dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as stream:
            json.dump(data, stream, ensure_ascii=False, indent=2)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temp, path)
    finally:
        Path(temp).unlink(missing_ok=True)


def digest(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def initialize(root):
    marker = root / MARKER
    if marker.exists():
        if json.loads(marker.read_text()).get('version') != 1:
            raise ValueError('Unsupported conference storage version')
    else:
        atomic_json(marker, {'version': 1, 'layout': 'content-addressed-media'})
    (root / 'assets').mkdir(exist_ok=True)


def logical_path(value):
    if not value or value.startswith('/') or '\\' in value or any(x in ('', '.', '..') for x in value.split('/')):
        raise ValueError('Invalid logical media path')
    return value


def resolve(folder, logical):
    logical_path(logical)
    sidecar = folder / SIDECAR
    if sidecar.exists():
        refs = json.loads(sidecar.read_text())
        if refs['version'] != 1:
            raise ValueError('Unsupported media reference version')
        # macOS filenames may be decomposed while native JSON uses composed Unicode.
        matches = [value for name, value in refs['files'].items() if unicodedata.normalize('NFC', name) == unicodedata.normalize('NFC', logical)]
        if matches:
            if len(set(matches)) != 1:
                raise ValueError('Conflicting Unicode media references')
            ancestor, key = refs['root'], matches[0]
            if ancestor != '.' and (len(ancestor.split('/')) > 8 or any(x != '..' for x in ancestor.split('/'))):
                raise ValueError('Invalid media root')
            if not re.fullmatch(r'assets/[a-f0-9]{64}\.[a-z0-9]+', key):
                raise ValueError('Invalid shared media key')
            root = (folder / ancestor).resolve()
            if json.loads((root / MARKER).read_text())['version'] != 1:
                raise ValueError('Unsupported media root')
            target = (root / key).resolve()
            if not target.is_relative_to(root / 'assets') or not target.is_file():
                raise ValueError('Missing or unsafe shared media file: ' + key)
            return target
    path = (folder / logical).resolve()
    if not path.is_relative_to(folder.resolve()) or not path.is_file():
        raise ValueError('Missing local media: ' + str(path))
    return path


def store(root, source):
    initialize(root)
    extension = source.suffix.lower().lstrip('.')
    if not re.fullmatch('[a-z0-9]+', extension):
        raise ValueError('Unsupported media extension')
    checksum = digest(source)
    if source.resolve().parent == (root / 'assets').resolve() and source.stem != checksum:
        raise ValueError('Corrupt source asset; original reference preserved')
    key = 'assets/' + checksum + '.' + extension
    target = root / key
    if target.exists():
        if digest(target) != checksum:
            raise ValueError('Corrupt shared media: ' + key)
    else:
        fd, temp = tempfile.mkstemp(prefix='.media-', dir=root / 'assets')
        os.close(fd)
        try:
            shutil.copyfile(source, temp)
            if digest(Path(temp)) != checksum:
                raise ValueError('Media changed during copy')
            with open(temp, 'rb') as stream:
                os.fsync(stream.fileno())
            os.replace(temp, target)
        finally:
            Path(temp).unlink(missing_ok=True)
    return key


def attach(root, folder, logical, source, remove_local=False):
    logical_path(logical)
    logical = unicodedata.normalize('NFC', logical)
    if not folder.resolve().is_relative_to(root.resolve()):
        raise ValueError('Media folder outside conference')
    key = store(root, source)
    path = folder / SIDECAR
    ancestor = os.path.relpath(root, folder)
    refs = json.loads(path.read_text()) if path.exists() else {'version': 1, 'root': ancestor, 'files': {}}
    if refs['version'] != 1 or refs['root'] != ancestor:
        raise ValueError('Conflicting media reference root')
    refs['files'] = {name: value for name, value in refs['files'].items() if unicodedata.normalize('NFC', name) != logical}
    refs['files'][logical] = key
    atomic_json(path, refs)
    local = folder / logical
    if remove_local and local.exists():
        if local.is_symlink() or local.resolve() == (root / key).resolve() or digest(local) != Path(key).stem:
            raise ValueError('Local media changed; preserved: ' + str(local))
        local.unlink()
    return root / key


def prepare_manifest(path):
    """Ingest PDFs once. Paths within the archive become relative and movable."""
    root = path.parent
    manifest = json.loads(path.read_text())
    for item in manifest['items']:
        titles = item.get('pdfTitles') or [Path(p).name for p in item['pdfPaths']]
        paths = []
        for value in item['pdfPaths']:
            source = Path(value) if Path(value).is_absolute() else root / value
            if not source.exists():
                source = resolve(source.parent.parent, 'documents/' + source.name) if source.parent.name == 'documents' else resolve(source.parent, source.name)
            if source.is_relative_to(root) and not source.is_relative_to(root / 'assets'):
                folder = source.parent.parent if source.parent.name == 'documents' else source.parent
                asset = attach(root, folder, str(source.relative_to(folder)), source, remove_local=True)
                paths.append(str(asset.relative_to(root)))
            else:
                paths.append(store(root, source))
        item['pdfPaths'], item['pdfTitles'] = paths, titles
    for key in ('vocabularyPath', 'documentCacheRecordingPath'):
        if manifest.get(key):
            p = Path(manifest[key])
            if p.is_absolute() and p.is_relative_to(root):
                manifest[key] = str(p.relative_to(root))
    if manifest.get('documentCacheRecordingPath'):
        value = Path(manifest['documentCacheRecordingPath'])
        cache = value if value.is_absolute() else root / value
        if cache.is_relative_to(root):
            record = json.loads(cache.read_text())
            for doc in record.get('conference', {}).get('documents', []):
                attach(root, cache.parent, doc['filename'], resolve(cache.parent, doc['filename']), remove_local=True)
    atomic_json(path, manifest)
    return manifest


def migrate(root):
    """Idempotent migration: verified asset -> atomic reference -> remove exact duplicate."""
    root = root.resolve()
    for p in (root / 'library').glob('*/recording.json'):
        record = json.loads(p.read_text())
        if record.get('state') == 'processing' or record.get('transcriptionProgress') is not None:
            raise ValueError('Finish or resume active transcription before migration')
    # Save the original small manifest for provenance, without retaining duplicate binaries.
    manifest_file = root / 'manifest.json'
    backup = root / 'manifest.before-storage-v1.json'
    if not backup.exists():
        atomic_json(backup, json.loads(manifest_file.read_text()))
    prepare_manifest(manifest_file)
    candidates = [p for p in root.rglob('*') if p.is_file() and p.suffix.lower() in ('.pdf', '.m4a') and not p.is_relative_to(root / 'assets')]
    for source in candidates:
        if source.is_symlink():
            raise ValueError('Unexpected symlink: ' + str(source))
        # References live beside recording.json, or beside result.json / original PDFs.
        folder = source.parent.parent if source.parent.name == 'documents' else source.parent
        logical = str(source.relative_to(folder))
        attach(root, folder, logical, source, remove_local=True)
    for sidecar in root.rglob(SIDECAR):
        folder = sidecar.parent
        refs = json.loads(sidecar.read_text())
        for logical in refs['files']:
            target = resolve(folder, logical)
            if digest(target) != target.stem:
                raise ValueError('Asset verification failed')
        # Repair generated relative links; transcript bodies and native recording data stay intact.
        markdown = folder / 'transcript.md'
        if markdown.exists():
            text = markdown.read_text()
            for logical in refs['files']:
                relative = os.path.relpath(resolve(folder, logical), folder)
                for old in (logical, quote(logical)):
                    text = text.replace('](' + old + ')', '](' + quote(relative) + ')').replace('](' + old + '#', '](' + quote(relative) + '#')
            markdown.write_text(text)
        doc_manifest = folder / 'derived/documents/manifest.json'
        if doc_manifest.exists():
            docs = json.loads(doc_manifest.read_text())
            for doc in docs:
                old = doc['original']
                if old in refs['files']:
                    doc['original'] = os.path.relpath(resolve(folder, old), folder)
                    text_file = folder / doc['text']
                    text_file.write_text(text_file.read_text().replace('Original: ' + old + '\n', 'Original: ' + doc['original'] + '\n'))
            atomic_json(doc_manifest, docs)
        if (folder / 'ABLAGE.txt').exists():
            (folder / 'ABLAGE.txt').write_text('Originale liegen einmalig im assets-Ordner des Konferenzarchivs. media-references.json ordnet logische Dateinamen zu. Den gesamten Konferenzordner zusammen aufbewahren. Texte, Rohfassungen, PDFs und Bearbeitungshistorien bleiben erhalten.\n')
    assets = list((root / 'assets').glob('*.*'))
    report = {'version': 1, 'mediaFiles': len(assets), 'mediaBytes': sum(p.stat().st_size for p in assets), 'archiveBytes': sum(p.stat().st_size for p in root.rglob('*') if p.is_file()), 'verified': True}
    atomic_json(root / 'storage-report.json', report)
    return report


if __name__ == '__main__':
    import sys
    print(json.dumps(migrate(Path(sys.argv[1])), indent=2))
