import importlib.util
from pathlib import Path
import unittest

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


if __name__ == "__main__":
    unittest.main()
