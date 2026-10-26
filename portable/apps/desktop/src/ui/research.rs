use super::*;
use laut_engine::{
    library_search::LibraryHit,
    preferences::StateStore,
    research::{self as engine, Corpus, LocalInference, Report, ResearchConfig, SearchResult},
};

pub(super) struct ResearchState {
    pub open: bool,
    pub config: ResearchConfig,
    pub search: Option<SearchResult>,
    query: String,
    task: String,
    all_library: bool,
    pdfs: bool,
    analysis_pdfs: bool,
    tab: usize,
    reports: Vec<Report>,
    selected_report: Option<String>,
    warning: String,
}
pub(super) enum ResearchEvent {
    Search(Result<SearchResult, String>),
    Report(Box<Result<Report, String>>),
}
impl ResearchState {
    pub fn new(mut config: ResearchConfig) -> Self {
        let executable = std::env::current_exe().ok();
        for (path, name) in [
            (&mut config.embedding_worker, "llama-embedding"),
            (&mut config.generation_worker, "llama-completion"),
        ] {
            if !path.is_file() {
                let filename = if cfg!(windows) {
                    format!("{name}.exe")
                } else {
                    name.into()
                };
                let bundled = executable
                    .as_ref()
                    .and_then(|e| e.parent())
                    .map(|d| d.join("tools").join(filename));
                if let Some(p) = bundled
                    .filter(|p| p.is_file())
                    .or_else(|| processing::find_program(name))
                {
                    *path = p;
                }
            }
        }
        Self{open:false,config,search:None,query:String::new(),task:"Fasse die zentralen Aussagen, konkreten Zahlen, Zeitpläne und offenen Fragen zusammen.".into(),all_library:false,pdfs:true,analysis_pdfs:false,tab:0,reports:vec![],selected_report:None,warning:String::new()}
    }
    pub fn reload_reports(&mut self) {
        match StateStore::default_path().and_then(|p| engine::reports(&p.join("research/results")))
        {
            Ok(v) => {
                self.reports = v;
                self.warning.clear();
            }
            Err(e) => self.warning = e.to_string(),
        }
    }
}
impl LautApp {
    pub(super) fn research_event(&mut self, event: ResearchEvent) {
        self.busy = false;
        self.running_job = false;
        let result = match event {
            ResearchEvent::Search(v) => v.map(|r| {
                self.research.search = Some(r);
                self.status = self
                    .l("Lokale Suche abgeschlossen.", "Local search ready.")
                    .into();
            }),
            ResearchEvent::Report(v) => (*v).map(|r| {
                self.research.selected_report = Some(r.id.clone());
                self.research.reload_reports();
                self.research.tab = 2;
                self.status = self
                    .l(
                        "KI-Entwurf lokal gespeichert. Aussagen an den Quellen prüfen.",
                        "AI draft saved locally. Check claims against the sources.",
                    )
                    .into();
            }),
        };
        if let Err(e) = result {
            self.error = true;
            self.status = e;
        } else {
            self.error = false;
        }
    }
    fn research_start(&mut self, action: u8, ctx: &egui::Context) {
        if self.busy || !self.save_recovery() {
            return;
        }
        if self.pending_fields() {
            self.error = true;
            self.status = self
                .l(
                    "Editor-Eingaben zuerst übernehmen.",
                    "Apply editor inputs first.",
                )
                .into();
            return;
        }
        let Some((library, _)) = self.managed.clone() else {
            self.error = true;
            self.status = self
                .l(
                    "Zuerst ein Projekt aus der eigenen Bibliothek öffnen.",
                    "Open a project in your own library first.",
                )
                .into();
            return;
        };
        let current = self.view.as_ref().map(|r| r.id.clone());
        let scope = if action == 2 || !self.research.all_library {
            current
        } else {
            None
        };
        let config = self.research.config.clone();
        let query = if action == 2 {
            self.research.task.clone()
        } else {
            self.research.query.clone()
        };
        let pdfs = if action == 2 {
            self.research.analysis_pdfs
        } else {
            self.research.pdfs
        };
        let tx = self.tx.clone();
        let repaint = ctx.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = cancel.clone();
        self.busy = true;
        self.running_job = true;
        self.error = false;
        self.status = self
            .l(
                "Lokale Recherche wird vorbereitet …",
                "Preparing local research …",
            )
            .into();
        self.progress = 0.;
        std::thread::spawn(move || {
            let run = (|| -> anyhow::Result<(Option<SearchResult>, Option<Report>)> {
                let root = StateStore::default_path()?.join("research");
                let corpus = Corpus::from_library(&library, scope.as_deref(), pdfs)?;
                let worker = LocalInference::new(config.clone(), action != 2, action != 0)?;
                let progress = |f, s: &str| {
                    let _ = tx.send(Event::Progress(f, s.into()));
                    repaint.request_repaint();
                };
                if action == 2 {
                    let result = engine::analyse(
                        &worker,
                        &corpus,
                        &root.join("steps"),
                        &query,
                        &config.language,
                        &cancel,
                        progress,
                    )?;
                    engine::save_report(&root.join("results"), &result)?;
                    Ok((None, Some(result)))
                } else {
                    let found = engine::search(
                        &worker,
                        &corpus,
                        &root.join("vectors"),
                        &query,
                        &cancel,
                        progress,
                    )?;
                    if action == 0 {
                        Ok((Some(found), None))
                    } else {
                        progress(
                            0.95,
                            if config.language == "de" {
                                "Lokale KI liest die Fundstellen und formuliert eine Antwort …"
                            } else {
                                "Local AI is reading the sources and drafting an answer …"
                            },
                        );
                        let result = engine::answer(
                            &worker,
                            &corpus,
                            &found,
                            &query,
                            &config.language,
                            &cancel,
                        )?;
                        engine::save_report(&root.join("results"), &result)?;
                        Ok((None, Some(result)))
                    }
                }
            })();
            let event = match run {
                Ok((Some(r), _)) => ResearchEvent::Search(Ok(r)),
                Ok((_, Some(r))) => ResearchEvent::Report(Box::new(Ok(r))),
                Ok(_) => unreachable!(),
                Err(e) => {
                    if action == 0 {
                        ResearchEvent::Search(Err(format!("{e:#}")))
                    } else {
                        ResearchEvent::Report(Box::new(Err(format!("{e:#}"))))
                    }
                }
            };
            let _ = tx.send(Event::Research(event));
            repaint.request_repaint();
        });
    }
    pub(super) fn research_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let en = self.english;
        let de_task = "Fasse die zentralen Aussagen, konkreten Zahlen, Zeitpläne und offenen Fragen zusammen.";
        let en_task = "Summarise key claims, specific numbers, timelines and open questions.";
        if en && self.research.task == de_task {
            self.research.task = en_task.into();
        }
        if !en && self.research.task == en_task {
            self.research.task = de_task.into();
        }
        let t = |de, english| if en { english } else { de };
        let mut action = None;
        let mut source = None;
        let mut append = None;
        let mut models = false;
        ui.heading(t("Suche & KI", "Search & AI"));
        ui.add_space(12.);
        ui.scope(|ui|{
            ui.horizontal_wrapped(|ui|{for (i,name) in [t("Suchen / Fragen","Search / ask"),t("Gesamtauswertung","Full analysis"),t("Gespeicherte Ergebnisse","Saved results"),t("Einrichtung","Setup")].iter().enumerate(){ui.selectable_value(&mut self.research.tab,i,*name);}});
            ui.label(t("Lokal auf der CPU. KI-Ergebnisse sind prüfbare Entwürfe; Originale bleiben erhalten.","Local CPU processing. AI results are drafts to review; originals are retained."));
            if !self.research.warning.is_empty(){ui.colored_label(ui.visuals().error_fg_color,&self.research.warning);}
            if self.busy {ui.spinner();ui.label(&self.status);if self.running_job && ui.button(t("Abbrechen","Cancel")).clicked(){self.cancel.store(true,Ordering::Relaxed);}}
            egui::ScrollArea::vertical().id_salt("research-page").show(ui,|ui|{ui.add_enabled_ui(!self.busy,|ui|{
                match self.research.tab {
                    0=>{
                        ui.add(egui::TextEdit::multiline(&mut self.research.query).hint_text(t("Frage oder Thema – auch anders formuliert als im Original","Question or topic, including different wording from the source")).desired_rows(2).desired_width(f32::INFINITY));
                        ui.checkbox(&mut self.research.all_library,t("Gesamte Bibliothek durchsuchen","Search the whole library"));ui.checkbox(&mut self.research.pdfs,t("PDF-Text und OCR einbeziehen","Include PDF text and OCR"));
                        ui.horizontal(|ui|{if ui.button(t("Semantisch suchen","Semantic search")).clicked(){action=Some(0);}
if ui.button(t("Frage lokal beantworten","Answer locally")).clicked(){action=Some(1);}});
                        ui.small(t("Beim ersten Mal wird der Suchindex aufgebaut. Danach werden nur geänderte Passagen neu berechnet. Antworten verwenden ausgewählte Suchtreffer, keine vollständige Konferenzauswertung.","The first search builds the index; later searches reuse unchanged passages. Answers use selected search hits, not the complete conference."));
                        if let Some(found)=&self.research.search {
                            ui.label(format!("{} {}",found.passages,t("Passagen durchsucht · Ähnlichkeit ist keine Gewissheit","passages searched · similarity is not certainty")));
                            for w in &found.warnings{ui.colored_label(ui.visuals().error_fg_color,w);}
                            for h in &found.hits {ui.group(|ui|{ui.strong(format!("{} · {}",h.source.title,h.source.label));ui.label(&h.source.text);if ui.button(t("Fundstelle öffnen","Open source")).clicked(){source=Some(h.source.clone());}});}
                        }
                    }
                    1=>{
                        ui.label(t("Verarbeitet den gesamten ausgewählten Text des geöffneten Projekts in Abschnitten. Bestätigte Ausschlüsse bleiben ausgenommen. Abschnittsnotizen bleiben zusätzlich zum Überblick erhalten.","Processes all selected text in the open project in sections. Confirmed exclusions are omitted. Section notes remain available alongside the overview."));
                        ui.add(egui::TextEdit::multiline(&mut self.research.task).desired_rows(3).desired_width(f32::INFINITY));
                        ui.checkbox(&mut self.research.analysis_pdfs,t("Alle angehängten PDF-Seitentexte ebenfalls auswerten","Also analyse all attached PDF page text"));
                        if let Some(session)=&self.session{let n=Corpus::from_session(session,self.research.analysis_pdfs).sources.len();ui.label(format!("{n} {}",t("Passagen · lange Aufnahmen können auf der CPU deutlich dauern","passages · long recordings can take substantial CPU time")));}
                        if ui.button(t("Auswerten / fortsetzen","Analyse / resume")).clicked(){action=Some(2);}
                        ui.small(t("Fertige Abschnitte werden lokal zwischengespeichert. Mit derselben Aufgabe und denselben Quellen wird nach Abbruch fortgesetzt. Kein Hochladen, kein API-Schlüssel.","Completed sections are checkpointed locally. Resume with the same task and sources after cancellation. No upload or API key."));
                    }
                    2=>{
                        if self.research.reports.is_empty(){ui.label(t("Noch keine gespeicherten Ergebnisse.","No saved results yet."));}
                        for report in &self.research.reports{if ui.selectable_label(self.research.selected_report.as_ref()==Some(&report.id),format!("{} · {}",report.question,if report.mode=="Search passages only" {t("Antwort aus Suchtreffern","Answer from search hits")} else {t("Gesamtauswertung","Full analysis")})).clicked(){self.research.selected_report=Some(report.id.clone());}}
                        if let Some(r)=self.research.reports.iter().find(|r|Some(&r.id)==self.research.selected_report.as_ref()){
                            ui.separator();ui.label(format!("{} / {} {}",r.processed_passages,r.total_passages,t("Passagen verwendet · Fundstellenstand zum Erstellzeitpunkt","passages used · source snapshot at creation")));
                            if r.findings.is_empty(){ui.label(t("Keine ausreichend gestützten Aussagen erzeugt. Quellen und Abschnittsnotizen prüfen.","No supported findings generated. Inspect sources and section notes."));}
                            if !r.warnings.is_empty(){ui.collapsing(t("Hinweise zu fehlendem Quellentext","Missing source text warnings"),|ui|{for warning in &r.warnings{ui.colored_label(ui.visuals().error_fg_color,warning);}});}
                            ui.horizontal_wrapped(|ui|{
                                if ui.button(t("Alles kopieren","Copy complete report")).clicked(){ui.ctx().copy_text(r.markdown());}
                                if ui.button(t("Als Text exportieren …","Export text …")).clicked() && let Some(p)=rfd::FileDialog::new().set_file_name("Laut-Recherche.txt").save_file(){match laut_engine::write_new(&p,r.markdown().as_bytes()){Ok(())=>self.status=t("Ergebnis exportiert.","Report exported.").into(),Err(e)=>{self.error=true;self.status=e.to_string();}}}
                                let same=self.view.as_ref().is_some_and(|v|r.sources.values().all(|s|s.recording_id==v.id));
                                if ui.add_enabled(same,egui::Button::new(t("Im Projekt als Auswertung ergänzen","Append to project analysis"))).clicked(){append=Some(r.markdown());}
                            });
                            for f in &r.findings {ui.group(|ui|{ui.label(&f.text);if let Some(q)=&f.quote{ui.small(format!("Beleg / Evidence: {q}"));}ui.horizontal_wrapped(|ui|{for id in &f.sources{if ui.button(format!("[{id}]")).clicked(){source=r.sources.get(id).cloned();}}});});}
                            ui.collapsing(t("Alle Abschnittsnotizen","All section notes"),|ui|{for (i,section) in r.sections.iter().enumerate(){ui.strong(format!("{}",i+1));for f in section{ui.label(&f.text);if let Some(q)=&f.quote{ui.small(format!("Beleg / Evidence: {q}"));}ui.horizontal_wrapped(|ui|{for id in &f.sources{if ui.button(format!("[{id}]")).clicked(){source=r.sources.get(id).cloned();}}});}}});
                            ui.collapsing(t("Gespeicherte Quellentexte","Saved source texts"),|ui|{for (id,s) in &r.sources{ui.strong(format!("[{id}] {} · {}",s.title,s.label));ui.label(&s.text);}});
                        }
                    }
                    _=>{
                        ui.label(t("Suchmodell und Textmodell werden einmal heruntergeladen. Danach laufen Suche und Auswertung ohne Netzwerk. Verwendet wird das Qwen3-GGUF-Format mit llama.cpp.","Download the search and text models once. Search and analysis then work offline using Qwen3 GGUF with llama.cpp."));
                        if ui.button(t("Modellverwaltung öffnen","Open model manager")).clicked(){models=true;}
                        for (label,path) in [(t("Suchprogramm","Embedding worker"),&mut self.research.config.embedding_worker),(t("Textprogramm","Text worker"),&mut self.research.config.generation_worker),(t("Suchmodell","Embedding model"),&mut self.research.config.embedding_model),(t("Textmodell","Text model"),&mut self.research.config.text_model)]{ui.horizontal_wrapped(|ui|{ui.label(label);ui.label(path.to_string_lossy());if ui.button("…").clicked()&&let Some(p)=rfd::FileDialog::new().pick_file(){*path=p;}});}
                        ui.horizontal(|ui|{ui.label(t("Modellformat","Model format"));ui.selectable_value(&mut self.research.config.text_profile,"qwen3-instruct".into(),"Qwen3 Instruct");ui.selectable_value(&mut self.research.config.text_profile,"qwen3-thinking".into(),"Qwen3 / no-think");});
                        ui.horizontal(|ui|{ui.label(t("CPU-Threads","CPU threads"));ui.add(egui::DragValue::new(&mut self.research.config.threads).range(1..=32));});
                        ui.horizontal(|ui|{ui.label(t("Ergebnissprache","Output language"));ui.selectable_value(&mut self.research.config.language,"de".into(),"Deutsch");ui.selectable_value(&mut self.research.config.language,"en".into(),"English");});
                        if let Ok(p)=StateStore::default_path(){ui.small(format!("{}: {}",t("Index, Zwischenstände und Ergebnisse","Index, checkpoints and results"),p.join("research").display()));}
                    }
                }
            });});
        });
        if models {
            self.processing.show_models();
            self.job_open = true;
        }
        if let Some(text) = append {
            self.command(Command::AppendAnalysis { text });
        }
        if let Some(s) = source {
            let result = (|| -> anyhow::Result<_> {
                let lib = Library::open(&PathBuf::from(&self.library_root))?;
                engine::verify_source(&lib, &s)?;
                Ok(lib.root.join(&s.recording_id).join("recording.json"))
            })();
            match result {
                Ok(path) => {
                    self.daily.pending_hit = Some(LibraryHit {
                        recording_id: s.recording_id,
                        title: s.title,
                        kind: s.kind,
                        text: s.text,
                        segment_id: s.segment_id,
                        time: s.time,
                        document_id: s.document_id,
                        page: s.page,
                    });
                    self.research.open = false;
                    self.load(path, ctx.clone());
                }
                Err(e) => {
                    self.error = true;
                    self.status = e.to_string();
                }
            }
        }
        if let Some(action) = action {
            self.research_start(action, ctx);
        }
    }
}
