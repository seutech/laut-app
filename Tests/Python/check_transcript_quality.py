"""Compare local plain-text references and transcripts; print numeric metrics only."""
import argparse
import json
from pathlib import Path
import re


def words(text):
    return re.findall(r'\w+', text.casefold(), flags=re.UNICODE)


def metrics(reference, hypothesis):
    expected, actual = words(reference), words(hypothesis)
    if not expected:
        raise ValueError('A nonempty reference is required')
    # Levenshtein word distance with linear memory. Case and punctuation are ignored.
    previous = list(range(len(actual) + 1))
    for i, token in enumerate(expected, 1):
        current = [i]
        for j, other in enumerate(actual, 1):
            current.append(min(current[-1] + 1, previous[j] + 1, previous[j-1] + (token != other)))
        previous = current
    return {'reference_words': len(expected), 'hypothesis_words': len(actual),
            'word_errors': previous[-1], 'word_error_rate': previous[-1] / len(expected)}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reference', required=True, type=Path)
    parser.add_argument('--transcript', required=True, type=Path)
    parser.add_argument('--max-wer', type=float)
    args = parser.parse_args()
    result = metrics(args.reference.read_text(), args.transcript.read_text())
    print(json.dumps(result))
    if args.max_wer is not None and result['word_error_rate'] > args.max_wer:
        raise SystemExit(1)
