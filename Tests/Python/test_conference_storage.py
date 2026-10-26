import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('conference_storage', Path(__file__).resolve().parents[2] / 'scripts/conference_storage.py')
storage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(storage)


class ConferenceStorageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name) / 'conference'
        self.root.mkdir()

    def tearDown(self):
        self.temp.cleanup()

    def test_versions_share_media_and_survive_move(self):
        for version in ['library/one', 'vortraege/one', 'automatische-fassungen/one']:
            folder = self.root / version
            folder.mkdir(parents=True)
            source = folder / 'audio.m4a'
            source.write_bytes(b'original audio bytes')
            storage.attach(self.root, folder, source.name, source, remove_local=True)
            self.assertFalse(source.exists())
        self.assertEqual(len(list((self.root / 'assets').glob('*.m4a'))), 1)
        moved = self.root.with_name('moved')
        self.root.rename(moved)
        self.assertEqual(storage.resolve(moved / 'vortraege/one', 'audio.m4a').read_bytes(), b'original audio bytes')

    def test_corrupt_pool_never_removes_original(self):
        folder = self.root / 'downloads/one'
        folder.mkdir(parents=True)
        source = folder / 'audio.m4a'
        source.write_bytes(b'original')
        key = storage.store(self.root, source)
        (self.root / key).write_bytes(b'damaged')
        with self.assertRaises(ValueError):
            storage.store(self.root, self.root / key)
        with self.assertRaises(ValueError):
            storage.attach(self.root, folder, source.name, source, remove_local=True)
        self.assertEqual(source.read_bytes(), b'original')

    def test_macos_unicode_names_resolve_from_native_json(self):
        folder = self.root / 'library/one'
        folder.mkdir(parents=True)
        source = folder / 'Ho\u0308ttges.m4a'
        source.write_bytes(b'original')
        asset = storage.attach(self.root, folder, source.name, source, remove_local=True)
        self.assertEqual(storage.resolve(folder, 'Höttges.m4a'), asset.resolve())
        refs = json.loads((folder / storage.SIDECAR).read_text())
        refs['files'] = {'Ho\u0308ttges.m4a': str(asset.relative_to(self.root))}
        storage.atomic_json(folder / storage.SIDECAR, refs)
        self.assertEqual(storage.resolve(folder, 'Höttges.m4a'), asset.resolve())

    def test_missing_reference_and_escape_are_rejected(self):
        folder = self.root / 'library/one'
        folder.mkdir(parents=True)
        source = folder / 'audio.m4a'
        source.write_bytes(b'original')
        asset = storage.attach(self.root, folder, source.name, source, remove_local=True)
        asset.unlink()
        with self.assertRaises(ValueError):
            storage.resolve(folder, source.name)
        refs = json.loads((folder / storage.SIDECAR).read_text())
        refs['files']['audio.m4a'] = '../../outside.m4a'
        storage.atomic_json(folder / storage.SIDECAR, refs)
        with self.assertRaises(ValueError):
            storage.resolve(folder, source.name)

    def test_manifest_retains_titles_and_external_originals(self):
        external = self.root.parent / 'Useful slides.pdf'
        external.write_bytes(b'PDF data')
        manifest = self.root / 'manifest.json'
        storage.atomic_json(manifest, {'items': [{'pdfPaths': [str(external)]}]})
        first = storage.prepare_manifest(manifest)
        second = storage.prepare_manifest(manifest)
        self.assertEqual(first, second)
        self.assertTrue(external.exists())
        self.assertEqual(first['items'][0]['pdfTitles'], ['Useful slides.pdf'])
        self.assertFalse(Path(first['items'][0]['pdfPaths'][0]).is_absolute())

    def test_migration_reuses_references_repairs_links_and_preserves_text(self):
        folder = self.root / 'vortraege/one'
        (folder / 'documents').mkdir(parents=True)
        pdf = folder / 'documents/slides.pdf'
        pdf.write_bytes(b'original PDF')
        (folder / 'audio.m4a').write_bytes(b'original audio')
        (folder / 'transcript.md').write_text('Speech stays.\n[Page](documents/slides.pdf#page=3)')
        (folder / 'recording.json').write_text('{"segments":[]}')
        storage.atomic_json(self.root / 'manifest.json', {'items': [{'pdfPaths': [str(pdf)]}]})
        original = (folder / 'recording.json').read_bytes()
        storage.migrate(self.root)
        storage.migrate(self.root)
        self.assertEqual((folder / 'recording.json').read_bytes(), original)
        self.assertIn('../../assets/', (folder / 'transcript.md').read_text())
        self.assertIn('#page=3', (folder / 'transcript.md').read_text())
        self.assertEqual(storage.resolve(folder, 'documents/slides.pdf').read_bytes(), b'original PDF')
        self.assertEqual(len(list((self.root / 'assets').glob('*.*'))), 2)


if __name__ == '__main__':
    unittest.main()
