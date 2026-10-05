import importlib.util
from pathlib import Path
import unittest
from unittest.mock import Mock, patch
from types import SimpleNamespace

spec = importlib.util.spec_from_file_location("worker", Path(__file__).parents[2] / "Resources/mlx_worker.py")
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)


class AlignmentTests(unittest.TestCase):
    def test_subword_tokens_join_before_speaker_assignment(self):
        tokens = [
            {"text": " Guten", "start": 0, "end": .2},
            {"text": " Mor", "start": .3, "end": .5},
            {"text": "gen", "start": .5, "end": .7},
            {"text": ".", "start": .7, "end": .8},
        ]
        self.assertEqual(worker.parakeet_words(tokens), [
            {"text": "Guten", "start": 0.0, "end": .2},
            {"text": "Morgen.", "start": .3, "end": .8},
        ])

    def test_empty_tokens_do_not_create_empty_words(self):
        self.assertEqual(worker.parakeet_words([{"text": ""}, {"text": " "}]), [])


class WarmupTests(unittest.TestCase):
    def setUp(self):
        worker._warmed_key = None
        self.fake_numpy = SimpleNamespace(random=SimpleNamespace(default_rng=lambda _: SimpleNamespace(normal=lambda *args: SimpleNamespace(astype=lambda _: "synthetic signal"))), float32="float32")

    def test_repeated_preload_does_not_repeat_inference(self):
        model = Mock()
        with patch.dict("sys.modules", {"numpy": self.fake_numpy}):
            worker.warm_local("phonon", Path("/model-a"), model)
            worker.warm_local("phonon", Path("/model-a"), model)
        model.transcribe_array_detailed.assert_called_once_with("synthetic signal")

    def test_failed_warmup_is_not_reported_as_ready(self):
        model = Mock()
        model.transcribe_array_detailed.side_effect = RuntimeError("GPU unavailable")
        with patch.dict("sys.modules", {"numpy": self.fake_numpy}):
            with self.assertRaises(RuntimeError):
                worker.warm_local("phonon", Path("/model-a"), model)
        self.assertIsNone(worker._warmed_key)


if __name__ == "__main__":
    unittest.main()
