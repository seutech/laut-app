use super::*;
use laut_core::conference as core;
use laut_engine::{
    documents::{self, PdfConfig},
    preferences::StateStore,
};
use serde_json::{Value, json};

pub(super) struct ConferenceState {
    pub open: bool,
    tab: usize,
    pub config: PdfConfig,
    query: String,
    page: Option<(String, u32)>,
    texture: Option<egui::TextureHandle>,
    preview_open: bool,
    editing: Option<(String, Value)>,
    vocabulary: String,
    vocabulary_loaded: bool,
}
pub(super) enum ConferenceEvent {
    Imported(Result<(Session, Revision), String>),
    Preview(Result<(String, u32, egui::ColorImage), String>),
    Ocr(Result<(String, u32, String), String>),
    Export(Result<PathBuf, String>),
}
impl ConferenceState {
    pub fn new(mut config: PdfConfig) -> Self {
        for (p, name) in [
            (&mut config.pdfinfo, "pdfinfo"),
            (&mut config.pdftotext, "pdftotext"),
            (&mut config.pdftoppm, "pdftoppm"),
            (&mut config.tesseract, "tesseract"),
        ] {
            if !p.is_absolute() || !p.is_file() {
                let homebrew = PathBuf::from("/opt/homebrew/bin").join(name);
                if cfg!(target_os = "macos") && homebrew.is_file() {
                    *p = homebrew;
                } else if let Some(found) = processing::find_program(name) {
                    *p = found;
                }
            }
        }
        Self {
            open: false,
            tab: 0,
            config,
            query: String::new(),
            page: None,
            texture: None,
            preview_open: false,
            editing: None,
            vocabulary: String::new(),
            vocabulary_loaded: false,
        }
    }
    pub fn sync_vocabulary(&mut self, previous: &str, current: &str) {
        if self.vocabulary_loaded && self.vocabulary == previous {
            self.vocabulary = current.to_owned();
        }
    }
    pub fn clear(&mut self) {
        self.page = None;
        self.texture = None;
        self.preview_open = false;
        self.editing = None;
        self.vocabulary_loaded = false;
    }
}
impl LautApp {
    fn conference_start(&mut self) -> bool {
        if self.busy || !self.save_recovery() {
            return false;
        }
        if self.pending_fields() {
            self.status = self
                .l(
                    "Bitte zuerst Editor-Eingaben übernehmen.",
                    "Apply editor drafts first.",
                )
                .into();
            self.error = true;
            return false;
        }
        self.busy = true;
        self.running_job = true;
        self.progress = 0.;
        self.error = false;
        self.cancel = Arc::new(AtomicBool::new(false));
        true
    }
    pub(super) fn conference_event(&mut self, event: ConferenceEvent, ctx: &egui::Context) {
        self.busy = false;
        self.running_job = false;
        let result = match event {
            ConferenceEvent::Imported(result) => result.map(|(s, rev)| {
                self.session = Some(s);
                if let Some((_, r)) = &mut self.managed {
                    *r = rev;
                }
                self.refresh();
                self.status = self
                    .l(
                        "PDFs lokal aufgenommen. OCR und Diagramme am Original prüfen.",
                        "PDFs imported locally. Check OCR and charts against the originals.",
                    )
                    .into();
            }),
            ConferenceEvent::Preview(result) => result.map(|(id, n, img)| {
                self.conference.texture =
                    Some(ctx.load_texture("pdf-page", img, egui::TextureOptions::LINEAR));
                self.conference.page = Some((id, n));
                self.conference.preview_open = true;
                self.status = self
                    .l("PDF-Seite lokal gerendert.", "PDF page rendered locally.")
                    .into();
            }),
            ConferenceEvent::Ocr(result) => result.map(|(id, n, text)| {
                if let Some(s) = &self.session {
                    let mut c = core::context(&s.document);
                    for d in c["documents"].as_array_mut().unwrap() {
                        if d["id"] == id {
                            for p in d["pages"].as_array_mut().unwrap() {
                                if p["number"] == n {
                                    p["ocrText"] = json!(text);
                                    p["extraction"] =
                                        json!("PDF text + Tesseract OCR (unreviewed)");
                                    p.as_object_mut().unwrap().remove("portableWarning");
                                }
                            }
                        }
                    }
                    self.command(Command::EditConference { value: c });
                }
            }),
            ConferenceEvent::Export(result) => result.map(|p| {
                self.status = format!(
                    "{}: {}",
                    self.l("Rechercheexport gespeichert", "Research export saved"),
                    p.display()
                );
            }),
        };
        if let Err(e) = result {
            self.error = true;
            self.status = e;
        }
    }
    pub(super) fn show_pdf(&mut self, id: String, page: u32, ctx: &egui::Context) {
        if !self.conference_start() {
            return;
        }
        let Some(session) = self.session.clone() else {
            self.busy = false;
            self.running_job = false;
            return;
        };
        let config = self.conference.config.clone();
        let tx = self.tx.clone();
        let cancel = self.cancel.clone();
        let ctx = ctx.clone();
        self.status = self
            .l("PDF-Seite wird geladen …", "Loading PDF page …")
            .into();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<_> {
                let cache = StateStore::default_path()?.join("pdf-previews");
                let path = documents::render_page(&session, &id, page, &config, &cache, &cancel)?;
                let reader = image::ImageReader::open(path)?.with_guessed_format()?;
                let img = reader.decode()?.into_rgba8();
                let color = egui::ColorImage::from_rgba_unmultiplied(
                    [img.width() as usize, img.height() as usize],
                    img.as_raw(),
                );
                Ok((id, page, color))
            })()
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::Conference(ConferenceEvent::Preview(result)));
            ctx.request_repaint();
        });
    }
    pub(super) fn current_slide(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let selected = self
            .view
            .as_ref()
            .and_then(|r| r.conference.as_ref())
            .and_then(|c| {
                c.slides
                    .iter()
                    .filter(|s| s.confirmed && s.useful == Some(true) && s.start <= self.seek)
                    .max_by(|a, b| a.start.total_cmp(&b.start))
                    .and_then(|s| {
                        c.documents
                            .iter()
                            .find(|d| d.id == s.document_id)
                            .map(|d| (d.id.clone(), s.page, d.title.clone(), s.start))
                    })
            });
        if let Some((id, page, title, start)) = selected
            && ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new(format!(
                        "{} · {} · {} {} · {}",
                        self.l("Geprüfte PDF-Referenz", "Reviewed PDF reference"),
                        title,
                        self.l("Seite", "Page"),
                        page,
                        timestamp(start)
                    )),
                )
                .clicked()
        {
            self.show_pdf(id, page, ctx);
        }
    }
    pub(super) fn conference_dialog(&mut self, ctx: &egui::Context) {
        let en = self.english;
        let t = |de, english| if en { english } else { de };
        let mut preview = None;
        let mut ocr = None;
        let mut import = None;
        let mut export = None;
        let mut update = None;
        let mut rename = None;
        let mut seek = None;
        if self.conference.open {
            let mut open = true;
            egui::Window::new(t("Konferenz und Unterlagen","Conference and documents")).open(&mut open).default_width(850.).default_height(650.).show(ctx,|ui|{
                ui.horizontal_wrapped(|ui|{for (i,label) in [t("PDFs","PDFs"),t("Folien","Slides"),t("Kapitel","Chapters"),t("Einspieler","Video inserts"),t("Begriffe","Vocabulary"),t("Ablage / Programme","Storage / programs")].iter().enumerate(){ui.selectable_value(&mut self.conference.tab,i,*label);}});
                ui.separator();
                if self.busy {ui.spinner();ui.label(&self.status);if self.running_job&&ui.button(t("Abbrechen","Cancel")).clicked(){self.cancel.store(true,Ordering::Relaxed);}}
                let Some(session)=self.session.as_ref()else{ui.label(t("Zuerst ein Transkript öffnen.","Open a transcript first."));return;};
                let r=session.document.view();let c=core::context(&session.document);
                egui::ScrollArea::vertical().max_height(570.).show(ui,|ui|{ui.add_enabled_ui(!self.busy,|ui|{match self.conference.tab {
                    0=>{
                        ui.label(t("Original-PDFs bleiben erhalten. Suchtext und OCR ersetzen weder Grafiken noch Tabellenbeziehungen.","Original PDFs are retained. Search text and OCR cannot replace charts or table relationships."));
                        if self.managed.is_none(){ui.label(t("Für PDF-Import das Projekt zuerst in die eigene Bibliothek aufnehmen.","Add this project to your library before importing PDFs."));}
                        if ui.add_enabled(self.managed.is_some(),egui::Button::new(t("PDFs hinzufügen …","Add PDFs …"))).clicked(){import=rfd::FileDialog::new().add_filter("PDF",&["pdf"]).pick_files();}
                        ui.add(egui::TextEdit::singleline(&mut self.conference.query).hint_text(t("Alle PDF-Seiten durchsuchen","Search all PDF pages")).desired_width(f32::INFINITY));
                        let q=self.conference.query.trim().to_lowercase();let mut shown=0;
                        for doc in r.conference.iter().flat_map(|c|&c.documents){
                            ui.collapsing(format!("{} · {} {}",doc.title,doc.pages.len(),t("Seiten","pages")),|ui|{
                                for p in &doc.pages {let text=core::page_text(p);if !q.is_empty()&&!text.to_lowercase().contains(&q){continue;}
                                    ui.horizontal_wrapped(|ui|{ui.label(format!("{} {} · {}",t("Seite","Page"),p.number,p.extraction.as_deref().unwrap_or("PDF")));
                                        if ui.button(t("Ansehen","View")).clicked(){preview=Some((doc.id.clone(),p.number));}
                                        if ui.button(t("OCR ergänzen","Add OCR")).clicked(){ocr=Some((doc.id.clone(),p.number));}
                                        if ui.button(t("Hier zuordnen …","Link here …")).clicked(){self.conference.editing=Some(("slides".into(),json!({"id":core::id(),"documentID":doc.id,"page":p.number,"start":self.seek,"confirmed":false,"useful":true,"matchEvidence":"Manual selection"})));}
                                    });
                                    if let Some(raw)=c["documents"].as_array().unwrap().iter().find(|d|d["id"]==doc.id).and_then(|d|d["pages"].as_array()).and_then(|pp|pp.iter().find(|v|v["number"]==p.number))&&let Some(w)=raw["portableWarning"].as_str(){ui.colored_label(ui.visuals().error_fg_color,w);}
                                    ui.collapsing(t("Seitentext","Page text"),|ui|{let mut text=text.as_str();ui.add(egui::TextEdit::multiline(&mut text).desired_width(f32::INFINITY));});
                                }
                            });
                            if !q.is_empty(){for p in &doc.pages{let text=core::page_text(p);if text.to_lowercase().contains(&q)&&shown<100{shown+=1;if ui.button(format!("{} · {} {}",doc.title,t("Seite","Page"),p.number)).clicked(){preview=Some((doc.id.clone(),p.number));}ui.label(text.chars().take(220).collect::<String>());}}}
                        }
                        if !q.is_empty(){ui.label(format!("{shown} {}",t("Treffer (max. 100)","matches (max. 100)")));}
                    },
                    1=>{
                        ui.label(t("Thematische PDF-Treffer, kein Nachweis einer tatsächlich eingeblendeten Folie. Nur bestätigte, hilfreiche Seiten erscheinen beim Transkript und im Export.","Thematic PDF matches, not proof of an on-screen slide. Only confirmed useful pages appear alongside the transcript and in exports."));
                        if ui.button(t("Passende Seiten vorschlagen","Suggest relevant pages")).clicked(){let mut next=c.clone();next["slides"].as_array_mut().unwrap().retain(|s|s["confirmed"]==true||s["useful"]==false);next["slides"].as_array_mut().unwrap().extend(core::slide_suggestions(&r));update=Some(next);}
                        for s in c["slides"].as_array().unwrap(){let doc=c["documents"].as_array().unwrap().iter().find(|d|d["id"]==s["documentID"]);let title=doc.and_then(|d|d["title"].as_str()).unwrap_or("PDF");
                            ui.group(|ui|{ui.label(format!("{} · {} · {} {} · {}",timestamp(s["start"].as_f64().unwrap_or(0.)),title,t("Seite","Page"),s["page"],if s["confirmed"]==true{t("bestätigt","confirmed")}else{t("Vorschlag","suggestion")}));
                                if let Some(e)=s["matchEvidence"].as_str(){ui.small(e);}
                                ui.horizontal_wrapped(|ui|{if ui.button(t("PDF ansehen","View PDF")).clicked(){preview=Some((s["documentID"].as_str().unwrap_or("").into(),s["page"].as_u64().unwrap_or(1) as u32));}
if ui.button(t("Anhören","Listen")).clicked(){seek=s["start"].as_f64();}
if ui.button(t("Prüfen / ändern …","Review / edit …")).clicked(){self.conference.editing=Some(("slides".into(),s.clone()));}});
                            });
                        }
                    },
                    2|3=>{
                        let chapters=self.conference.tab==2;let key=if chapters{"chapters"}else{"ranges"};
                        ui.label(if chapters{t("Kapitelvorschläge sind Leseabschnitte. Titel und Zeiten lassen sich ändern.","Chapter suggestions are reading boundaries. Edit their titles and times.")}else{t("Einspieler-Vorschläge beruhen nur auf gesprochenen Ankündigungen. Ein Ausschluss wird erst nach Bestätigung wirksam; Originaltext und Audio bleiben erhalten.","Video-insert suggestions use spoken cues only. Exclusion requires confirmation; original text and audio are retained.")});
                        ui.horizontal_wrapped(|ui|{
                            if ui.button(t("Vorschläge ergänzen","Add suggestions")).clicked(){let mut next=c.clone();let proposed=if chapters{core::chapter_suggestions(&r)}else{core::range_suggestions(&r)};for p in proposed{if !next[key].as_array().unwrap().iter().any(|old|(old["start"].as_f64().unwrap_or(-1.)-p["start"].as_f64().unwrap_or(0.)).abs()<2.){next[key].as_array_mut().unwrap().push(p);}}update=Some(next);}
                            if ui.button(t("Manuell hinzufügen …","Add manually …")).clicked(){let item=if chapters{json!({"id":core::id(),"start":self.seek,"title":"","origin":"Manuell"})}else{json!({"id":core::id(),"start":self.seek,"end":(self.seek+30.).min(r.duration),"kind":"Einspieler / Video insert","evidence":"Manual review","confirmed":false,"excluded":false})};self.conference.editing=Some((key.into(),item));}
                        });
                        for item in c[key].as_array().unwrap(){ui.group(|ui|{ui.label(format!("{} · {}",timestamp(item["start"].as_f64().unwrap_or(0.)),item[if chapters{"title"}else{"kind"}].as_str().unwrap_or("")));if !chapters{ui.label(format!("→ {} · {}",timestamp(item["end"].as_f64().unwrap_or(0.)),if item["confirmed"]==true&&item["excluded"]==true{t("ausgeschlossen","excluded")}else{t("sichtbar","included")}));ui.small(item["evidence"].as_str().unwrap_or(""));}ui.horizontal(|ui|{if ui.button(t("Anhören","Listen")).clicked(){seek=item["start"].as_f64();}
if ui.button(t("Bearbeiten …","Edit …")).clicked(){self.conference.editing=Some((key.into(),item.clone()));}});});}
                    },
                    4=>{
                        if !self.conference.vocabulary_loaded{self.conference.vocabulary=c["vocabulary"].as_str().unwrap_or("").into();self.conference.vocabulary_loaded=true;}
                        ui.label(t("Bestätigte Namen und Fachbegriffe, eine Schreibweise pro Zeile. Diese Liste ändert keinen gesprochenen Text automatisch.","Confirmed names and terms, one spelling per line. This list never changes spoken text automatically."));
                        ui.add(egui::TextEdit::multiline(&mut self.conference.vocabulary).desired_rows(10).desired_width(f32::INFINITY));
                        if ui.button(t("Begriffe speichern","Save vocabulary")).clicked(){let mut next=c.clone();next["vocabulary"]=json!(self.conference.vocabulary);update=Some(next);}
                        ui.small(t("Neue Eingaben werden erst mit Speichern übernommen.","New entries are applied only when saved."));
                        if ui.button(t("Für neue Transkriptionen verwenden","Use for new transcriptions")).clicked(){self.processing.vocabulary=self.conference.vocabulary.lines().filter_map(|s|s.split('|').next()).collect::<Vec<_>>().join(", ").chars().take(800).collect();self.status=t("Begriffe für den nächsten Import übernommen.","Vocabulary copied to the next import.").into();}
                        for hint in core::name_suggestions(&r) { ui.group(|ui|{ui.label(format!("{} → {}",r.speaker_name(hint["speakerID"].as_str()),hint["name"].as_str().unwrap_or("")));ui.small(hint["evidence"].as_str().unwrap_or(""));ui.horizontal(|ui|{if ui.button(t("Anhören","Listen")).clicked(){seek=hint["time"].as_f64();}
if ui.button(t("Namen bestätigen","Confirm name")).clicked(){rename=Some(Command::RenameSpeaker{id:hint["speakerID"].as_str().unwrap().into(),name:hint["name"].as_str().unwrap().into()});}});}); }
                    },
                    _=>{
                        ui.label(t("Rechercheexport: vollständige Projektkopie, Original-PDFs, Suchtexte je Seite, TXT/Markdown/SRT und lokale HTML-Lesefassung mit geprüften Folien.","Research export: complete project copy, original PDFs, page search text, TXT/Markdown/SRT and a local HTML reading copy with reviewed slides."));
                        if ui.button(t("Rechercheordner exportieren …","Export research folder …")).clicked(){export=rfd::FileDialog::new().set_file_name("Laut-Recherche").save_file();}
                        ui.separator();ui.label(t("Lokale PDF- und OCR-Programme","Local PDF and OCR programs"));
                        ui.small(t("Poppler und Tesseract laufen auf der CPU. Nach Installation ist kein Netzwerk nötig. OCR-Sprache benötigt passende lokal installierte Sprachdaten.","Poppler and Tesseract run on CPU. No network is needed after installation. OCR requires locally installed data for the selected language."));
                        for (label,p) in [("pdfinfo",&mut self.conference.config.pdfinfo),("pdftotext",&mut self.conference.config.pdftotext),("pdftoppm",&mut self.conference.config.pdftoppm),("tesseract",&mut self.conference.config.tesseract)]{ui.horizontal(|ui|{ui.label(label);ui.label(p.to_string_lossy());if ui.small_button("…").clicked()&&let Some(path)=rfd::FileDialog::new().pick_file(){*p=path;}});}
                        ui.horizontal_wrapped(|ui|{ui.label(t("OCR-Sprache","OCR language"));for (value,label) in [("eng",t("Englisch","English")),("deu",t("Deutsch","German")),("deu+eng",t("Deutsch und Englisch","German and English"))]{ui.selectable_value(&mut self.conference.config.ocr_language,value.to_owned(),label);}});
                        ui.checkbox(&mut self.conference.config.auto_ocr,t("Textarme Seiten automatisch mit OCR ergänzen","Automatically OCR pages with little text"));
                    }
                }});});
            });
            self.conference.open = open;
        }
        if let Some((key, mut item)) = self.conference.editing.take() {
            let mut open = true;
            let mut save = false;
            let mut remove = false;
            egui::Window::new(t("Zuordnung bearbeiten", "Edit reference"))
                .open(&mut open)
                .collapsible(false)
                .default_width(560.)
                .show(ctx, |ui| {
                    time_field(
                        ui,
                        t(
                            "Beginn (Sekunden in der Audiodatei)",
                            "Start (seconds in local audio)",
                        ),
                        &mut item,
                        "start",
                    );
                    if key == "chapters" {
                        text_field(ui, t("Titel", "Title"), &mut item, "title");
                    }
                    if key == "ranges" {
                        time_field(ui, t("Ende (Sekunden)", "End (seconds)"), &mut item, "end");
                        text_field(ui, t("Bezeichnung", "Label"), &mut item, "kind");
                        text_field(
                            ui,
                            t("Beleg / Notiz", "Evidence / note"),
                            &mut item,
                            "evidence",
                        );
                        check(ui, t("Geprüft", "Reviewed"), &mut item, "confirmed");
                        check(
                            ui,
                            t("In Lesefassung ausschließen", "Exclude from reading copy"),
                            &mut item,
                            "excluded",
                        );
                    }
                    if key == "slides" {
                        ui.label(format!(
                            "PDF: {} · {} {}",
                            item["documentID"].as_str().unwrap_or(""),
                            t("Seite", "Page"),
                            item["page"]
                        ));
                        check(
                            ui,
                            t("Zuordnung geprüft", "Reference reviewed"),
                            &mut item,
                            "confirmed",
                        );
                        check(
                            ui,
                            t(
                                "Inhaltlich hilfreich – anzeigen und exportieren",
                                "Useful content – show and export",
                            ),
                            &mut item,
                            "useful",
                        );
                        text_field(
                            ui,
                            t("Beleg / Notiz", "Evidence / note"),
                            &mut item,
                            "matchEvidence",
                        );
                    }
                    ui.horizontal(|ui| {
                        save = ui
                            .add_enabled(!self.busy, egui::Button::new(t("Übernehmen", "Apply")))
                            .clicked();
                        remove = ui
                            .add_enabled(
                                !self.busy,
                                egui::Button::new(t("Markierung entfernen", "Remove marker")),
                            )
                            .clicked();
                    });
                    ui.small(t(
                        "Übernehmen und Entfernen sind mit Rückgängig umkehrbar.",
                        "Apply and remove can be undone.",
                    ));
                });
            if (save || remove)
                && let Some(s) = &self.session
            {
                let mut c = core::context(&s.document);
                c[&key]
                    .as_array_mut()
                    .unwrap()
                    .retain(|v| v["id"] != item["id"]);
                if save {
                    if key == "chapters" {
                        item["origin"] = json!("Manuell");
                    }
                    c[&key].as_array_mut().unwrap().push(item.clone());
                    c[&key].as_array_mut().unwrap().sort_by(|a, b| {
                        a["start"]
                            .as_f64()
                            .unwrap_or(0.)
                            .total_cmp(&b["start"].as_f64().unwrap_or(0.))
                    });
                }
                match core::validate(&c, s.document.view().duration) {
                    Ok(()) => {
                        update = Some(c);
                        open = false;
                    }
                    Err(e) => {
                        self.error = true;
                        self.status = e.to_string();
                    }
                }
            }
            if open {
                self.conference.editing = Some((key, item));
            }
        }
        if self.conference.preview_open {
            let mut open = true;
            egui::Window::new(t("PDF-Seite","PDF page")).open(&mut open).default_width(900.).default_height(700.).show(ctx,|ui|{
                if let Some((id,n))=&self.conference.page && let Some(r)=&self.view && let Some(doc)=r.conference.iter().flat_map(|c|&c.documents).find(|d|d.id==*id){
                    ui.label(format!("{} · {} {}",doc.title,t("Seite","Page"),n));
                    ui.horizontal(|ui|{if ui.add_enabled(!self.busy&&*n>1,egui::Button::new(t("Vorige Seite", "Previous page"))).clicked(){preview=Some((id.clone(),n-1));}
if ui.add_enabled(!self.busy&&doc.pages.iter().any(|p|p.number==n+1),egui::Button::new(t("Nächste Seite", "Next page"))).clicked(){preview=Some((id.clone(),n+1));}
                        if ui.button(t("Mit aktueller Audiostelle verknüpfen …","Link to current audio time …")).clicked(){self.conference.editing=Some(("slides".into(),json!({"id":core::id(),"documentID":id,"page":n,"start":self.seek,"confirmed":false,"useful":true,"matchEvidence":"Manual selection"})));}
                    });
                    egui::ScrollArea::both().show(ui,|ui|{if let Some(texture)=&self.conference.texture{ui.add(egui::Image::new(texture).max_width(ui.available_width()).maintain_aspect_ratio(true));}
if let Some(p)=doc.pages.iter().find(|p|p.number==*n){ui.separator();let text=core::page_text(p);let mut text=text.as_str();ui.add(egui::TextEdit::multiline(&mut text).desired_width(f32::INFINITY));}});
                }
            });
            self.conference.preview_open = open;
        }
        if let Some(command) = rename {
            self.command(command);
        }
        if let Some(c) = update {
            self.command(Command::EditConference { value: c });
        }
        if let Some(time) = seek {
            self.seek = time;
            self.seek_audio(time, true);
        }
        if let Some((id, n)) = preview {
            self.show_pdf(id, n, ctx);
        }
        if let Some(files) = import
            && self.conference_start()
        {
            if let (Some(mut session), Some((library, mut rev))) =
                (self.session.clone(), self.managed.clone())
            {
                let config = self.conference.config.clone();
                let cancel = self.cancel.clone();
                let tx = self.tx.clone();
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let result = documents::import_documents(
                        &mut session,
                        &library,
                        &mut rev,
                        &files,
                        &config,
                        &cancel,
                        |p, s| {
                            let _ = tx.send(Event::Progress(p, s.into()));
                            ctx.request_repaint();
                        },
                    )
                    .map(|_| (session, rev))
                    .map_err(|e| format!("{e:#}"));
                    let _ = tx.send(Event::Conference(ConferenceEvent::Imported(result)));
                    ctx.request_repaint();
                });
            } else {
                self.busy = false;
                self.running_job = false;
            }
        }
        if let Some((id, n)) = ocr
            && self.conference_start()
        {
            let session = self.session.clone().unwrap();
            let config = self.conference.config.clone();
            let tx = self.tx.clone();
            let cancel = self.cancel.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let result = (|| -> anyhow::Result<_> {
                    let pdf = documents::page_path(&session, &id, n)?;
                    let text = documents::ocr_page(&pdf, n, &config, &cancel)?;
                    Ok((id, n, text))
                })()
                .map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::Conference(ConferenceEvent::Ocr(result)));
                ctx.request_repaint();
            });
        }
        if let Some(path) = export
            && self.conference_start()
        {
            let session = self.session.clone().unwrap();
            let config = self.conference.config.clone();
            let tx = self.tx.clone();
            let cancel = self.cancel.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let result = laut_engine::conference_archive::export(
                    &session,
                    &path,
                    &config,
                    &cancel,
                    |p, s| {
                        let _ = tx.send(Event::Progress(p, s.into()));
                        ctx.request_repaint();
                    },
                )
                .map(|_| path)
                .map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::Conference(ConferenceEvent::Export(result)));
                ctx.request_repaint();
            });
        }
    }
}
fn text_field(ui: &mut egui::Ui, label: &str, item: &mut Value, key: &str) {
    ui.label(label);
    let mut value = item[key].as_str().unwrap_or("").to_owned();
    if ui.text_edit_singleline(&mut value).changed() {
        item[key] = json!(value);
    }
}
fn time_field(ui: &mut egui::Ui, label: &str, item: &mut Value, key: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        let mut value = item[key].as_f64().unwrap_or(0.);
        if ui
            .add(
                egui::DragValue::new(&mut value)
                    .speed(0.1)
                    .range(0.0..=86400.0),
            )
            .changed()
        {
            item[key] = json!(value);
        }
    });
}
fn check(ui: &mut egui::Ui, label: &str, item: &mut Value, key: &str) {
    let mut value = item[key].as_bool().unwrap_or(false);
    if ui.checkbox(&mut value, label).changed() {
        item[key] = json!(value);
    }
}
