# Laut: vollständiger Funktionsumfang auf mehreren Plattformen

Stand: 26. Oktober 2026. Ziel: den bisherigen Funktionsumfang von Laut möglichst vollständig lokal anbieten. Erste Zielplattform neben macOS: **Windows ohne spezielle Grafikkarte**. Linux wird parallel gebaut; seine Anforderungen werden von Anfang an berücksichtigt.

Die öffentliche Rust-Alpha ist die Ausgangsbasis für weitere Plattformtests. Ein erfolgreich kompiliertes Paket bedeutet weder Funktionsgleichheit noch geprüfte Transkriptionsqualität auf jedem Gerät.

## Verbindliche Leitlinien

- Gemeinsamer Rust-Kern für Bibliothek, Dokumente, Bearbeitung, Suche, Aufträge und Exporte; gleiche Regeln in Oberfläche und CLI.
- Bestehende lokale Inferenzbibliotheken einbinden. Eine Anwendung in Rust muss ihre Spracherkennung und PDF-Verarbeitung nicht neu in Rust implementieren.
- CPU als vollständiger Ausführungspfad. Beschleunigung über Metal, CUDA oder andere unterstützte Geräte bleibt optional. Keine stillen Cloud-Ausweichlösungen.
- Downloads und Quellenabruf benötigen Netz; Transkription, Sprecheranalyse, OCR, Suche und Textauswertung sollen nach Installation der Modelle lokal funktionieren. Modell- und Programmdateien erhalten feste Versionen und Integritätsprüfungen.
- Lokale Verarbeitung und eine vom Betriebssystem erzwungene Netzsperre sind unterschiedliche Eigenschaften. Letztere muss pro Plattform umgesetzt und getestet werden; die heutige Vorschau garantiert sie für frei gewählte Fremdprogramme noch nicht.
- Originalaudio, PDFs, unbearbeiteter Text, Wortzeiten, Korrekturen und Herkunft bleiben erhalten. Namen, Folienzuordnungen und Ausschlüsse behalten ihren Bestätigungsstatus. Unklare Sprecher bleiben unklar.
- Die Swift-Bibliothek wird nicht automatisch übernommen oder parallel beschrieben. Import in eine ausdrücklich angelegte Rust-Bibliothek erfolgt als unabhängige Kopie.
- Cloud-APIs sind ausschließlich eine ausdrücklich gewählte Zusatzoption. Der lokale Funktionsumfang setzt weiterhin keinen API-Schlüssel voraus.

## Bestandsabgleich

Grundlage: Root-README sowie `Sources/Laut`, `Sources/LautCore`, `Sources/LautAudio`, `Resources` und vorhandene Tests. „Offen“ heißt: gehört weiterhin zum Zielumfang.

| Bereich | Rust heute | Nächster vollständiger Ausbau |
| --- | --- | --- |
| Datenkompatibilität | Originale, unbekannte Metadaten, Wörter, Konferenzinformationen und Undo-Verlauf werden erhalten | fortlaufender Swift/Rust-Vergleich mit echten und synthetischen Projekten |
| Bibliothek und Speichern | unabhängige Bibliothek, Import von Projektkopien, Liste, gespeicherte Einstellungen, Entwurfswiederherstellung, Speichern übernommener Korrekturen, vorherige gültige Fassung, Konfliktschutz; lokaler Papierkorb mit Wiederherstellung vollständiger Projekte | gezieltes endgültiges Entfernen, weitere Filter |
| Speicher/Sicherung | integritätsgeprüfte Projektkopien, Bibliothekssicherung einschließlich Papierkorb und Wiederherstellung in neue Ablage, Speicheranzeige je Projekt und Bibliothek, Fehlermeldungen bei fehlenden Medien | geplante Sicherungen, Audio getrennt entfernen, Platzprüfung für alle Aufträge |
| Import | Projekte, Audio/Video, Mehrfachauswahl/Drop, kompakte Audiokopien, Herkunft, Fortschritt/Abbruch, Bibliotheksablage | weitere Audio-/Videospuren gezielt auswählen; Windows-Gerätetest |
| Links/Aufträge | dauerhafte Warteschlange, öffentliche HTTPS-Einzelquellen über yt-dlp/Deno (u. a. YouTube/Vimeo, kompatible Podcast-/Mediatheksseiten und Direktmedien), Ausschnitte, Quellenmetadaten/Kapitel, Fehlerisolierung je Quelle, Fortsetzen und geprüfte Cache-Bereinigung | reale Provider-Matrix, echte Windows-/Linux-Gerätetests |
| Transkription | Whisper-CPU, Base/Small-Q5, Wortzeiten bei passender Token-Evidenz, Wortschatz, Checkpoints; echter 6-Minuten-Test | VAD/Abschnittsüberlappung, Parakeet-Vergleich, Warmhalten/Entladen, 7-Stunden-/Windows-Messung |
| Modelle | fester Katalog, Downloads mit SHA-256, Fortsetzen, Prüfen, Entfernen, externe Modelle, Paketgrößen, CPU-Sherpa für Mac/Windows/Linux | vollständiges Offline-Paket einschließlich FFmpeg, tatsächliche Hardwareprüfung |
| Sprecher | lokale CPU-Diarisierung, bekannte Sprecherzahl, Hörproben und Namensprüfung, sichere Ausrichtung, Schutz manueller Zuordnungen, eigene Analyse ohne ASR, Undo | Wortzeit-/Überlappungsqualität weiter prüfen; lange Aufnahme und Windows-Gerätetest |
| Redaktion | Text bearbeiten, Abschnitte teilen/zusammenführen, Titel/Notizen, Ersetzungen, gesammelt übernehmen, Undo/Redo, Entwürfe, Lesefassung, vollständiges Kopieren | weitere Tastenkürzel, gespeicherte KI-Ergebnisse bearbeiten |
| Audio-Navigation | Abspielen/Pause, Zeitregler, lokale Wellenform mit 1×–16× Zoom und Cache, Doppelklick im Wort, Fünf-Sekunden- und Abschnittssprünge, direkte Zeitmarken, aktuelle Passage markieren und mitführen, Suchtreffer sichtbar anspringen; gespeichertes Tempo 0,5×–2× ohne Tonhöhenänderung über lokales FFmpeg, ursprüngliche Zeitachse | Hör-/Windows-Gerätetest und weitere Prüfung langer Aufnahmen |
| Suche | Text/Sprecher im geöffneten Projekt sowie Bibliothekssuche über Titel, Transkripte, Notizen, gespeicherte Auswertung und PDF-Text/OCR mit direktem Seitenaufruf; zusätzliche mehrsprachige Qwen3-Semantik über Transkripte/PDFs mit lokalem Vektorcache | SQLite-FTS5/ANN für große Bibliotheken, weitere Filter, Skalierungsmessung |
| Konferenzen | PDF-Import/Text/OCR, Seitenvorschau und genaue Fundstellen, vorsichtige Folienvorschläge mit Prüfung, Kapitel und Namenshinweise, reversible Einspieler-Ausschlüsse, vollständiger Rechercheexport | visuelle Erkennung aus Video, semantische Folienauswahl, siebenstündige Recherche und Windows-Gerätetest |
| Exporte | TXT, Markdown, SRT, JSON und vollständige Projektkopie | automatisches Markdown-Archiv mit Konfliktschutz |
| Lokale Text-KI | Qwen3/llama.cpp auf CPU, eigene Anweisungen, Antworten aus Suchtreffern, vollständige abschnittsweise Textauswertung mit Zwischenständen, getrennte Ergebnisse und belegte Quellen, Kopieren/Export/undo-fähiges Ergänzen | Qualitätsbenchmark, mehr Vorlagen, große Konferenz und Windows-Gerätetest |
| Optionale externe KI | OpenAI Responses API und kompatible HTTPS-Endpunkte; Betriebssystem-Schlüsselspeicher oder reine Sitzung, lokaler Such-/Auswahlweg, explizite Textvorschau je Lauf, Verbindungstest, Tokenhinweis, Abbruch und Löschen des Zugangs; Quellen-/Zitatprüfung bleibt verbindlich | weitere Anbieteradapter, bekannte Modell-/Preisprofile, Streaming-Abbruch und reale Windows-/Linux-Tresortests |
| Aufnahme | lokale Mikrofonaufnahme, getrennte Meeting-Tonspuren, Pegel/Stillehinweis, Stoppen/Sichern, Teilaufnahme-Wiederherstellung und anschließende Transkription im selben Eintrag; Mac-/Windows-Adapter | Mac-Systemaudio-Freigabe und echter Meetingtest offen; Windows-Gerätetests, Linux-Systemaudio, Echovermeidung, Schlafschutz und Langzeittest |
| Diktieren/Integration | offen | globale Tasten, Auswahl bearbeiten, Einfügen in andere Programme, berechtigungsabhängige Plattformadapter |
| Sprache/Bedienung | gespeicherte DE/EN-Sprache, feste Seitenleiste mit Verlauf, eigene Seiten für Quellen/Modelle/Suche/Einstellungen, Systemschrift mit Rückfall, Hell/Dunkel/System, kompakte Werkzeugleiste, Inline-Wellenform/Player, sichtbare Kapitel/Suche/Kopie und Sprecher-/Zeitgasse im Lesetext | vollständiger Katalog, native Texteingabe/Bedienungshilfen, lange Transkripte |
| Lebenszyklus | sichere Checkpoints für Dateitranskription | Schlafschutz, erzwungenen Schlaf abfangen, sauberes Beenden, wiederaufnehmbare Aufträge, Crash-/Plattenfehler |
| Verteilung | reproduzierbare Alpha-Archive für macOS ARM64, Windows x64 und Linux x64; Rust-Testmatrix und mitgelieferte Lizenznachweise | echte Windows-/Linux-Gerätetests, Signierung/Notarisierung, Installer und Updates |

## Reihenfolge mit überprüfbaren Ergebnissen

### 1. Verlässliche tägliche Arbeit mit bestehenden Transkripten

Bibliothek, Speicherung, Wiederherstellung, Einstellungen, vollständiger Editor, Exporte und grundlegende Bibliothekssuche. Die Arbeitsabläufe dieses Meilensteins sind jetzt implementiert: Einstellungen, Titel/Notizen, strukturelle Bearbeitung, Sammelübernahme, Wiederherstellung von Eingaben, TXT/MD/SRT/JSON, Bibliothekssuche und geprüfte Bibliothekskopien. Die Mac-Bedienprüfung und der Nachweis der Windows-Ausführung werden getrennt dokumentiert; noch keine vollständige Gleichheit mit Swift.

Abnahme: eine Projektkopie öffnen, korrigieren, benennen, notieren, schließen, neu öffnen, Undo/Redo ausführen und exportieren. Originaldateien bleiben identisch. Eine beschädigte Hauptdatei wird mit einer verständlichen Meldung aus der vorherigen gültigen Fassung gelesen. Gleichzeitige Bearbeiter überschreiben einander nicht. Keine still verlorenen Eingaben.

### 2. Lokale Verarbeitung und erste benutzbare Windows-Version

Zwischenstand 26. Oktober: Die Dateiverarbeitung, Modellverwaltung und prüfbare Sprecheranalyse sind implementiert. Der CPU-Whisper-Worker wird reproduzierbar aus einer geprüften Quelle gebaut und mit den Alpha-Paketen gebündelt. Der Modellmanager enthält das überprüfte Windows-Sherpa-Paket einschließlich der erforderlichen DLL-Suchpfade. Windows-Typprüfung und automatisierter Paketbau sind eingerichtet, aber ein nativer Windows-Gerätetest bleibt offen. FFmpeg ist weiterhin eine lokale Voraussetzung. Die Import-, Modell-, Auftrags- und Sprecheransichten wurden auf dem Mac mit einem kurzen Video bedient und geprüft. YouTube-URL-Import wurde mit einem echten einminütigen Merz/Huang-Ausschnitt auf dem Mac geprüft. Parakeet-Vergleich und die echte siebenstündige Verarbeitung bleiben offen; daher ist dieser Meilenstein noch nicht vollständig abgenommen.


Modell-/Programmverwaltung, Audio-/Videoimport, Quellenabruf, Aufträge, ASR und Sprecheranalyse. Einen echten Windows-Build früh erzeugen; den vollständigen Ausbau auf macOS und Windows gemeinsam testen. Nicht erst alle Mac-Funktionen fertigstellen und anschließend Windows ausprobieren.

Die optionale externe Text-KI ist als getrennte Analyseebene umgesetzt, nicht als Voraussetzung für Transkription oder Suche. Der geheime Schlüssel liegt in macOS Keychain, Windows Credential Manager beziehungsweise Linux Secret Service; Einstellungen speichern nur eine Profilkennung. Fehlt ein nutzbarer sicherer Tresor, gibt es ausschließlich eine sitzungsgebundene Eingabe und keinen Klartext-Rückfall. Vor jedem Cloud-Lauf zeigt Laut Aufgabe, Umfang und jeden ausgewählten Textausschnitt; Audio-, Video- und PDF-Dateien werden nicht automatisch übertragen. Schlüssel erscheinen weder in Bibliothek, Einstellungen, Auftragsdateien, Sicherungen, Kommandozeilen noch Protokollen. Der vorhandene Belegstellen- und Quellenprüfpfad ist auch für externe Antworten verbindlich. Offen sind weitere Anbieteradapter, bekannte Preisprofile, ein sofortiger Netzwerkabbruch statt Verwerfen nach dem laufenden HTTPS-Aufruf sowie reale Tresortests unter Windows/Linux.

Für CPU-ASR zwei Kandidaten auf denselben deutschen und englischen Aufnahmen vergleichen:

- Der vorhandene **whisper.cpp**-Adapter als Referenz; praxistaugliche Modelle und Quantisierung prüfen.
- **Parakeet v3 über sherpa-onnx/ONNX** als Kandidat für die bisherigen Parakeet-Arbeitsabläufe. Die offizielle Dokumentation enthält ein INT8-Modell, CPU-Ausführung und einen Windows-Aufruf. Tokenzeiten müssen sauber in verwendbare Wort-/Abschnittszeiten übersetzt werden. Die Geschwindigkeit auf dem Zielrechner ist noch nicht gemessen.

Für Sprecheranalyse zunächst **sherpa-onnx** evaluieren. Modelllizenz und Verteilbarkeit getrennt von der Laufzeit prüfen. Die vorhandenen Laut-Regeln zur Zuordnung und zum Schutz manueller Korrekturen in Rust übernehmen; nicht durch ein neues Clustering allein ersetzen.

Abnahme: mehrstündiges Audio ohne GPU importieren, lokal verarbeiten, abbrechen, neu starten und fortsetzen. Verarbeitungszeit, Spitzen-RAM, temporärer Speicher und Qualität getrennt messen. Namen, Zahlen, kurze Zwischenrufe, Überlappungen und Abschnittsgrenzen mit Hörproben kontrollieren. Der Merz/Huang-Talk eignet sich als Regressionstest für übermäßige Sprecherwechsel. Keine Identitätszuordnung allein aus einer Stimme.

### 3. Konferenz- und Recherchefunktionen

Zwischenstand 26. Oktober: Die PDF- und redaktionellen Konferenzfunktionen sind implementiert. Auf dem Mac wurden sieben Originaldecks mit 194 Seiten lokal verarbeitet, 48 Seiten zusätzlich mit OCR. Original-PDFs bleiben bytegleich; durchsuchbarer Text, Seitenbild und Audiostelle sind verknüpft. Eine ausgewählte thematische Folie wurde am PDF und Transkript geprüft und exportiert. Einspieler-Erkennung nutzt bislang nur sprachliche Ankündigungen; Folienabgleich nutzt charakteristische gemeinsame Wörter. Beides benötigt redaktionelle Bestätigung. Der anschließende Ausbau ergänzt semantische Suche und lokale Textauswertung mit Qwen3/llama.cpp auf CPU. Windows-/Linux-Pakete werden automatisiert gebaut; native Geräte- und Bedienprüfungen dort bleiben offen.

PDF-Originale behalten, vorhandenen Text extrahieren und nur bei Bedarf OCR ergänzen. PDF-Seite, Textfundstelle und Audiozeit verknüpfen; Tabellen, Grafiken und Abbildungen bleiben im Original erreichbar. Folienvorschläge nach Relevanz anzeigen. Ausschlüsse sind eine reversible Ansicht über dem Original. Kapitel und Namensvorschläge erhalten Herkunft und Prüfstatus.

Bibliotheksweite semantische Suche und lokale Textauswertung sind über **llama.cpp** umgesetzt. Der Standard nutzt Qwen3 Embedding 0.6B und Qwen3 4B Instruct Q4. Die Gesamtauswertung durchläuft den gesamten ausgewählten Quellentext ohne feste 24.000-Zeichen-Grenze, behält Abschnittsnotizen und Quellenkopien und setzt nach Abbruch bei fertigen Zwischenschritten fort. Exakte Belegzitate werden geprüft; die journalistische Richtigkeit der Interpretation bleibt zu kontrollieren. Eine echte siebenstündige KI-Auswertung ist noch nicht gemessen.

Abnahme: Talk plus PDFs vollständig lokal recherchieren, eine Behauptung bis zur Audio-/PDF-Stelle zurückverfolgen, nur ausgewählte sinnvolle Folien anzeigen und einen durchsuchbaren Textbestand exportieren, ohne die Originalgrafiken zu verlieren.

### 4. Aufnahme, Systemintegration und Veröffentlichung

Gemeinsame Bedienung mit plattformspezifischen Adaptern: macOS-Audio/Berechtigungen, Windows-Mikrofon und WASAPI-Loopback, später Linux-Audio und Desktopportale. Getrennte Spuren, Echo/Dopplungen, Gerätewechsel, Energiesparen und Beenden benötigen echte Gerätetests. Globale Tasten und Einfügen sind besonders unter Linux/Wayland von Desktop und Berechtigungen abhängig; Einschränkungen müssen sichtbar sein.

CPU-fähige Programme bündeln und auf einer frischen Windows-Installation ohne Entwicklungswerkzeuge prüfen. Danach Linux auf einer festgelegten Distribution und unter Wayland/X11 testen. Die CI-Konfiguration baut und testet alle drei Systeme; erfolgreiche Läufe werden im öffentlichen Actions-Verlauf nachgewiesen. Signierung, Notarisierung und automatische Updates bleiben spätere Veröffentlichungsschritte. Drittanbieter-Lizenzen werden mit jedem Alpha-Archiv ausgeliefert.

## Definition von „übertragen“

Eine Funktion ist erst übertragen, wenn sie erreichbar ist, Daten zuverlässig speichert, den passenden automatischen Test besteht und die erforderliche Interaktion auf der Zielplattform geprüft wurde. Inferenz braucht zusätzlich einen Qualitäts-/Leistungstest. Bestehende experimentelle Funktionen der Swift-App werden als solche markiert, bis sie diesen Nachweis erbringen. Unterschiede einzelner Modelle sind zulässig; verdeckte Datenverluste oder weggefallene Arbeitsabläufe sind es nicht.

## Technische Primärquellen

- [whisper.cpp: lokale CPU-Ausführung und Plattformen](https://github.com/ggml-org/whisper.cpp)
- [sherpa-onnx: Parakeet v3, INT8, CPU und Windows-Beispiel](https://k2-fsa.github.io/sherpa/onnx/pretrained_models/offline-transducer/nemo-transducer-models.html)
- [sherpa-onnx: Sprecheranalyse und Rust-API](https://k2-fsa.github.io/sherpa/onnx/speaker-diarization/index.html)
- [llama.cpp: lokale Textmodell-Laufzeit](https://github.com/ggml-org/llama.cpp)
- [Microsoft: WASAPI-Loopback](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)

Diese Quellen belegen verfügbare Bausteine. Sie ersetzen keine Integration oder Messung in Laut.
