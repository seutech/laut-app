"""Real offline retrieval check using synthetic German notes, never the user's library."""
import json
import subprocess
import sys
from pathlib import Path
import time

root = Path(__file__).resolve().parents[2]
model = Path(sys.argv[1]).resolve()
process = subprocess.Popen(['/usr/bin/sandbox-exec', '-p', '(version 1) (allow default) (deny network*)',
                            str(root / '.runtime/venv/bin/python'), str(root / 'Resources/embedding_worker.py'), '--stream'],
                           stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
def request(texts, query=False):
    process.stdin.write(json.dumps({'modelPath': str(model), 'texts': texts, 'query': query}) + '\n')
    process.stdin.flush()
    reply = json.loads(process.stdout.readline())
    if 'error' in reply:
        raise AssertionError(reply['error'])
    return reply['vectors']
try:
    started = time.monotonic()
    vectors = request(['Wir verschieben die Veröffentlichung auf den nächsten Monat.',
                       'Zum Abendessen gibt es Kartoffeln und Gemüse.',
                       'Anna übernimmt die Rechnung und überweist den Betrag.'])
    load = time.monotonic() - started
    for question, expected in [('Was startet später als geplant?', 0), ('Wer kümmert sich um die Bezahlung?', 2), ('Was wird heute gekocht?', 1)]:
        q = request([question], query=True)[0]
        scores = [sum(a*b for a,b in zip(q,v)) for v in vectors]
        assert max(range(len(scores)), key=scores.__getitem__) == expected, scores
    assert all(abs(sum(v*v for v in row)-1) < 1e-4 for row in vectors)
    print(json.dumps({'german_queries_passed': 3, 'dimensions': len(vectors[0]), 'cold_three_passages_seconds': round(load, 2), 'network': 'denied'}))
finally:
    process.terminate()
    process.wait(timeout=10)
