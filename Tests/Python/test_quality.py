import unittest
from check_transcript_quality import metrics

class QualityTests(unittest.TestCase):
    def test_punctuation_and_case_do_not_count_as_word_errors(self):
        self.assertEqual(metrics('Grüße, Anna!', 'grüße anna')['word_error_rate'], 0)

    def test_omissions_and_replacements_are_counted(self):
        result = metrics('alpha beta gamma', 'alpha delta')
        self.assertEqual(result['word_errors'], 2)
        self.assertAlmostEqual(result['word_error_rate'], 2/3)

    def test_empty_hypothesis_is_total_omission(self):
        self.assertEqual(metrics('Die vollständige Referenz', '')['word_error_rate'], 1)
        with self.assertRaises(ValueError):
            metrics('', 'hallucination')

if __name__ == '__main__':
    unittest.main()
