#!/usr/bin/env python3
"""Build a portable, offline search and listening page from Laut conference exports."""
import html
import json
from pathlib import Path
import sys
from urllib.parse import quote
from conference_storage import resolve

root = Path(sys.argv[1]).resolve()
manifest = json.loads((root / 'manifest.json').read_text())
e = html.escape
sections = []
full_text = []
duration = 0
for item in manifest['items']:
    folder = root / 'vortraege' / item['slug']
    record = json.loads((folder / 'recording.json').read_text())
    duration += record['duration']
    prefix = 'vortraege/' + item['slug'] + '/'
    speakers = {s['id']: s['name'] for s in record.get('speakers', [])}
    audio = quote(str(resolve(folder, record['audioFilename']).relative_to(root)))
    rows = []
    chapters = iter(sorted(record.get('conference', {}).get('chapters', []), key=lambda c: c['start']))
    chapter = next(chapters, None)
    ranges = [r for r in record.get('conference', {}).get('ranges', []) if r.get('confirmed') and r.get('excluded')]
    for segment in record['segments']:
        # The catalog uses the same conservative whole-segment exclusions. Partial exclusions
        # remain visible here; the canonical reading export is transcript.md.
        if any(r['start'] <= segment['start'] and r['end'] >= segment['end'] for r in ranges):
            continue
        while chapter is not None and chapter['start'] <= segment['start']:
            rows.append('<h3>' + e(chapter['title']) + '</h3>')
            chapter = next(chapters, None)
        sec = int(segment['start'])
        stamp = f'{sec//3600:02}:{sec//60%60:02}:{sec%60:02}'
        speaker = speakers.get(segment.get('speakerID'), 'Sprecher ungeprüft')
        rows.append(f'<div class="passage"><button class="seek" data-time="{segment["start"]}">{stamp}</button> <b>{e(speaker)}</b><p>{e(segment["text"])}</p></div>')
    documents = []
    selected = []
    context = record.get('conference', {})
    for slide in context.get('slides', []):
        if not (slide.get('confirmed') and slide.get('useful')):
            continue
        doc = next((d for d in context.get('documents', []) if d['id'] == slide['documentID']), None)
        if doc:
            url = quote(str(resolve(folder, doc['filename']).relative_to(root))) + '#page=' + str(slide['page'])
            selected.append(f'<div class="passage"><button class="seek" data-time="{slide["start"]}">Passenden Audioabschnitt hören</button> <a href="{url}">{e(doc["title"])} · PDF-Seite {slide["page"]}</a><p class="note">{e(slide.get("matchEvidence") or "Inhaltliche Referenz; keine geprüfte Einblendzeit.")}</p></div>')
    for doc in record.get('conference', {}).get('documents', []):
        pages = []
        for page in doc['pages']:
            url = quote(str(resolve(folder, doc['filename']).relative_to(root))) + '#page=' + str(page['number'])
            text = page['text'] + '\n' + (page.get('ocrText') or '')
            pages.append(f'<div class="passage"><a href="{url}">Seite {page["number"]} im Original-PDF</a><pre>{e(text)}</pre></div>')
        documents.append('<details class="pdf"><summary>' + e(doc['title']) + '</summary>' + ''.join(pages) + '</details>')
    sections.append(f'''<section id="{item['slug']}"><h2>{e(record['title'])}</h2>
<p><a href="{e(item['url'])}">YouTube-Quelle</a> · <a href="{quote(prefix+'transcript.md')}">Markdown</a> · <a href="{quote(prefix+'transcript.txt')}">Text</a> · <a href="{quote(prefix+'transcript.srt')}">SRT</a> · <a href="{quote(prefix+'pruefvorschlaege.json')}">Prüfvorschläge</a></p>
<audio controls preload="none" src="{audio}"></audio>
<p class="note">{e(record.get('notes', ''))}</p><h3>Ausgewählte Folienbezüge</h3>{''.join(selected) or '<p>Keine geprüfte Folienauswahl.</p>'}
<div class="transcript">{''.join(rows)}</div><h3>PDFs mit Suchtext</h3>{''.join(documents)}</section>''')
    full_text.append(record['title'] + '\nQuelle: ' + item['url'] + '\n\n' + (folder / 'transcript.txt').read_text())

nav = ''.join(f'<a href="#{x["slug"]}">{e(x["slug"])}</a> ' for x in manifest['items'])
title = e(manifest.get('event', root.name))
description = e(manifest['eventDate']) + f' · {len(manifest["items"])} Veröffentlichungen · {int(duration)//3600} Stunden {int(duration)//60%60} Minuten'
page = '''<!doctype html><html lang="de"><meta charset="utf-8"><meta name="viewport" content="width=device-width">
<title>''' + title + ''' – lokale Recherche</title><style>
body{font:17px/1.6 system-ui,sans-serif;max-width:1100px;margin:auto;padding:32px;background:#f5f6f8;color:#162337}
h1,h2,h3{line-height:1.25}h1{font-size:34px}h2{font-size:25px}section{background:white;padding:28px;margin:28px 0;border-radius:12px;border:1px solid #d8dee7}
a{color:#00649c}nav a{display:inline-block;margin:4px 12px 4px 0}header{position:sticky;top:0;padding:12px 0;background:#f5f6f8;z-index:1}input{box-sizing:border-box;width:100%;padding:12px;font:inherit;border:1px solid #6e7b90;border-radius:6px}
.passage{padding:12px 0;border-bottom:1px solid #edf0f4}.passage p{margin:5px 0}button{cursor:pointer;color:#00649c;border:1px solid #bed0df;background:#edf5fb;padding:5px 9px;border-radius:5px}audio{width:100%;margin:10px 0}pre{white-space:pre-wrap;font:inherit;font-size:14px}details{margin:10px 0}summary{cursor:pointer;font-weight:600}[hidden]{display:none!important}.note{color:#516079;font-size:15px}
</style><h1>''' + title + '''</h1><p>''' + description + '''</p>
<p class="note">Lokale automatische Transkripte. Namen und Sprechergruppen nur soweit belegt; offene Vorschläge bleiben ungeprüft. PDF-Text ist eine Suchhilfe, das Original bleibt maßgeblich. Zeitmarken spielen das lokal gespeicherte Audio ab. Keine externen Skripte oder Dienste.</p>
<nav>''' + nav + '''</nav><header><label for="search">Alle Transkripte und PDF-Texte durchsuchen</label><input id="search" type="search" placeholder="Zum Beispiel: RAN Guardian"><span id="count" aria-live="polite"></span></header>''' + ''.join(sections) + '''
<script>
const entries=[...document.querySelectorAll('.passage')];
document.getElementById('search').addEventListener('input',e=>{
 const terms=e.target.value.toLocaleLowerCase().trim().split(/\\s+/).filter(Boolean);let count=0;
 entries.forEach(p=>{p.hidden=!terms.every(t=>p.textContent.toLocaleLowerCase().includes(t));if(!p.hidden)count++});
 document.querySelectorAll('section').forEach(s=>{s.hidden=![...s.querySelectorAll('.passage')].some(p=>!p.hidden)});
 document.querySelectorAll('.pdf').forEach(d=>{d.open=terms.length>0&&[...d.querySelectorAll('.passage')].some(p=>!p.hidden)});
 document.getElementById('count').textContent=terms.length?' '+count+' passende Textstellen':'';
});
document.querySelectorAll('.seek').forEach(b=>b.addEventListener('click',()=>{
 const a=b.closest('section').querySelector('audio');document.querySelectorAll('audio').forEach(other=>{if(other!==a)other.pause()});a.currentTime=Number(b.dataset.time);a.play().catch(()=>{});
}));
</script></html>'''
(root / 'START.html').write_text(page)
(root / 'Gesamttranskript.txt').write_text('\n\n' + ('\n\n' + '='*80 + '\n\n').join(full_text))
print(root / 'START.html')
