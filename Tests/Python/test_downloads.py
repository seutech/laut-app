import hashlib
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'Resources'))
import model_download

class DownloadTests(unittest.TestCase):
    def test_pins_revision_filters_duplicate_formats_checks_hash_and_marks_complete(self):
        self.run_download(False)

    def test_bad_checksum_never_marks_complete(self):
        self.run_download(True)

    def run_download(self, corrupt):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            content = b'synthetic model bytes'
            digest = hashlib.sha256(content).hexdigest()
            files = [SimpleNamespace(rfilename='model.safetensors', size=len(content), lfs=SimpleNamespace(sha256=digest)),
                     SimpleNamespace(rfilename='onnx/model.onnx', size=999, lfs=None)]
            api = SimpleNamespace(model_info=lambda *args, **kwargs: SimpleNamespace(sha='fixed-revision', siblings=files))
            called = []
            def fetch(repo, filename, **kwargs):
                self.assertEqual(kwargs['revision'], 'fixed-revision')
                self.assertIs(kwargs['token'], False)
                called.append(filename)
                path = root / filename
                path.write_bytes(b'X' * len(content) if corrupt else content)
                return str(path)
            with patch('huggingface_hub.HfApi', return_value=api), patch('huggingface_hub.hf_hub_download', side_effect=fetch):
                request = {'modelID': 'intfloat/multilingual-e5-small', 'cache': str(root), 'progressPath': str(root/'progress.json')}
                if corrupt:
                    with self.assertRaisesRegex(ValueError, 'Prüfsumme'):
                        model_download.download(request)
                    self.assertFalse((root/'.laut-complete.json').exists())
                else:
                    result = model_download.download(request)
                    self.assertEqual(result['path'], str(root))
                    self.assertEqual(json.loads((root/'progress.json').read_text())['completed'], len(content))
                    self.assertEqual(json.loads((root/'.laut-complete.json').read_text())['revision'], 'fixed-revision')
            self.assertEqual(called, ['model.safetensors'])

if __name__ == '__main__':
    unittest.main()
