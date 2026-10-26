use super::*;
use anyhow::Context;
use laut_engine::{
    library_search::LibraryHit,
    preferences::StateStore,
    research::{
        self as engine, Corpus, ExternalInference, LocalInference, Report, ResearchConfig,
        SearchResult, Source,
    },
};
use zeroize::{Zeroize, Zeroizing};

pub(super) struct CloudPlan {
    action: u8,
    corpus: Corpus,
    search: Option<SearchResult>,
    answer_sources: Vec<(String, Source)>,
    task: String,
    language: String,
}
impl CloudPlan {
    fn source_count(&self) -> usize {
        if self.action == 1 {
            self.answer_sources.len()
        } else {
            self.corpus.sources.len()
        }
    }
    fn source(&self, index: usize) -> Option<(String, &Source)> {
        if self.action == 1 {
            self.answer_sources
                .get(index)
                .map(|(id, source)| (id.clone(), source))
        } else {
            self.corpus
                .sources
                .get(index)
                .map(|source| (format!("S{}", index + 1), source))
        }
    }
    fn characters(&self) -> usize {
        self.task.len()
            + if self.action == 1 {
                self.answer_sources
                    .iter()
                    .map(|(_, source)| source.text.len() + source.label.len())
                    .sum::<usize>()
            } else {
                self.corpus
                    .sources
                    .iter()
                    .map(|source| source.text.len() + source.label.len())
                    .sum::<usize>()
            }
    }
    fn minimum_requests(&self) -> usize {
        if self.action == 1 {
            1
        } else {
            self.source_count().div_ceil(3).max(1)
        }
    }
}

pub(super) struct ResearchState {
    pub open: bool,
    pub return_to_results: bool,
    pub config: ResearchConfig,
    pub search: Option<SearchResult>,
    query: String,
    task: String,
    pub scope: String,
    pdfs: bool,
    analysis_pdfs: bool,
    tab: usize,
    reports: Vec<Report>,
    selected_report: Option<String>,
    warning: String,
    show_weak: bool,
    cloud_plan: Option<CloudPlan>,
    cloud_preview_index: usize,
    api_key_input: String,
    session_key: Option<Zeroizing<String>>,
    credential_status: String,
}
pub(super) enum ResearchEvent {
    Search(Result<SearchResult, String>),
    Report(Box<Result<Report, String>>),
    CloudReady(Box<Result<CloudPlan, String>>),
    Connection(Result<(), String>),
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
        Self{open:false,return_to_results:false,config,search:None,query:String::new(),task:"Fasse die zentralen Aussagen, konkreten Zahlen, Zeitpläne und offenen Fragen zusammen.".into(),scope:"current".into(),pdfs:true,analysis_pdfs:false,tab:0,reports:vec![],selected_report:None,warning:String::new(),show_weak:false,cloud_plan:None,cloud_preview_index:0,api_key_input:String::new(),session_key:None,credential_status:String::new()}
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
impl Drop for ResearchState {
    fn drop(&mut self) {
        self.api_key_input.zeroize();
    }
}

fn research_scope(
    ui: &mut egui::Ui,
    en: bool,
    scope: &mut String,
    collections: &[laut_engine::library::Collection],
    current: Option<&Recording>,
) {
    let t = |de, english| if en { english } else { de };
    let selected = if scope == "library" {
        t("Gesamte Bibliothek", "Whole library").to_owned()
    } else if scope == "current" {
        current
            .map(|recording| {
                format!(
                    "{}: {}",
                    t("Aktuelle Aufnahme", "Current recording"),
                    recording.title
                )
            })
            .unwrap_or_else(|| t("Aktuelle Aufnahme", "Current recording").to_owned())
    } else {
        collections
            .iter()
            .find(|collection| collection.id == *scope)
            .map(|collection| {
                format!(
                    "{}: {} ({} {})",
                    t("Sammlung", "Collection"),
                    collection.name,
                    collection.recording_ids.len(),
                    t("Aufnahmen", "recordings")
                )
            })
            .unwrap_or_else(|| t("Recherchebereich wählen", "Choose research scope").to_owned())
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(t("Recherchebereich", "Research scope"));
        egui::ComboBox::from_id_salt("research-scope")
            .selected_text(selected)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    scope,
                    "current".into(),
                    t("Aktuelle Aufnahme", "Current recording"),
                );
                for collection in collections {
                    ui.selectable_value(
                        scope,
                        collection.id.clone(),
                        format!(
                            "{}: {} ({})",
                            t("Sammlung", "Collection"),
                            collection.name,
                            collection.recording_ids.len()
                        ),
                    );
                }
                ui.selectable_value(
                    scope,
                    "library".into(),
                    t("Gesamte Bibliothek", "Whole library"),
                );
            });
    });
}

fn highlighted_source(ui: &mut egui::Ui, text: &str, terms: &[String]) {
    let terms: std::collections::BTreeSet<_> = terms.iter().cloned().collect();
    let mut ranges = vec![];
    let mut start = None;
    for (index, character) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        if character.is_alphanumeric() {
            start.get_or_insert(index);
        } else if let Some(begin) = start.take()
            && terms.contains(&text[begin..index].to_lowercase())
        {
            ranges.push(begin..index);
        }
    }
    let normal = egui::TextFormat {
        font_id: egui::TextStyle::Body.resolve(ui.style()),
        color: ui.visuals().text_color(),
        ..Default::default()
    };
    let mut marked = normal.clone();
    marked.background = if ui.visuals().dark_mode {
        egui::Color32::from_rgb(15, 91, 91)
    } else {
        egui::Color32::from_rgb(190, 235, 229)
    };
    marked.color = ui.visuals().strong_text_color();
    let mut job = egui::text::LayoutJob::default();
    let mut cursor = 0;
    for range in ranges {
        job.append(&text[cursor..range.start], 0., normal.clone());
        job.append(&text[range.clone()], 0., marked.clone());
        cursor = range.end;
    }
    job.append(&text[cursor..], 0., normal);
    job.wrap.max_width = ui.available_width();
    ui.add(egui::Label::new(job).wrap());
}
impl LautApp {
    pub(super) fn research_event(&mut self, event: ResearchEvent) {
        self.busy = false;
        self.running_job = false;
        let result = match event {
            ResearchEvent::Search(v) => v.map(|r| {
                self.research.search = Some(r);
                self.research.show_weak = false;
                self.research.return_to_results = false;
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
                        "KI-Entwurf gespeichert. Aussagen an den Quellen prüfen.",
                        "AI draft saved. Check claims against the sources.",
                    )
                    .into();
            }),
            ResearchEvent::CloudReady(v) => (*v).map(|plan| {
                self.research.cloud_plan = Some(plan);
                self.research.cloud_preview_index = 0;
                self.status = self
                    .l(
                        "Quellen lokal ausgewählt. Übertragung jetzt prüfen.",
                        "Sources selected locally. Review the transfer now.",
                    )
                    .into();
            }),
            ResearchEvent::Connection(v) => v.map(|()| {
                self.research.credential_status = self
                    .l(
                        "Verbindung erfolgreich geprüft.",
                        "Connection test succeeded.",
                    )
                    .into();
                self.status = self.research.credential_status.clone();
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
        let scope = match self.research.scope.as_str() {
            "library" => None,
            "current" => current
                .map(|id| std::collections::BTreeSet::from([id]))
                .or_else(|| Some(Default::default())),
            id => self
                .collections
                .iter()
                .find(|collection| collection.id == id)
                .map(|collection| collection.recording_ids.iter().cloned().collect()),
        };
        if scope
            .as_ref()
            .is_some_and(std::collections::BTreeSet::is_empty)
        {
            self.error = true;
            self.status = self
                .l(
                    "Der gewählte Recherchebereich enthält keine aktive Aufnahme.",
                    "The selected research scope contains no active recording.",
                )
                .into();
            return;
        }
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
        self.research.return_to_results = false;
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
                let corpus = Corpus::from_library_ids(&library, scope.as_ref(), pdfs)?;
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

    fn external_key(&self) -> anyhow::Result<Zeroizing<String>> {
        if let Some(secret) = &self.research.session_key {
            return Ok(Zeroizing::new(secret.as_str().to_owned()));
        }
        crate::secrets::load(&self.research.config.external_profile)
    }

    fn external_prepare(&mut self, action: u8, ctx: &egui::Context) {
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
        let current = self.view.as_ref().map(|recording| recording.id.clone());
        let scope = match self.research.scope.as_str() {
            "library" => None,
            "current" => current
                .map(|id| std::collections::BTreeSet::from([id]))
                .or_else(|| Some(Default::default())),
            id => self
                .collections
                .iter()
                .find(|collection| collection.id == id)
                .map(|collection| collection.recording_ids.iter().cloned().collect()),
        };
        if scope
            .as_ref()
            .is_some_and(std::collections::BTreeSet::is_empty)
        {
            self.error = true;
            self.status = self
                .l(
                    "Der gewählte Recherchebereich enthält keine aktive Aufnahme.",
                    "The selected research scope contains no active recording.",
                )
                .into();
            return;
        }
        let config = self.research.config.clone();
        let key = match self.external_key() {
            Ok(key) => key,
            Err(error) => {
                self.error = true;
                self.status = format!(
                    "{}: {error:#}",
                    self.l(
                        "Externen Zugang zuerst unter Einrichtung hinterlegen",
                        "Add the external credential under Setup first"
                    )
                );
                self.research.tab = 3;
                return;
            }
        };
        if let Err(error) =
            ExternalInference::new(&config.external_endpoint, &config.external_model, key)
        {
            self.error = true;
            self.status = error.to_string();
            self.research.tab = 3;
            return;
        }
        let task = if action == 2 {
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
                "Quellen werden lokal ausgewählt …",
                "Selecting sources locally …",
            )
            .into();
        self.progress = 0.;
        std::thread::spawn(move || {
            let run = (|| -> anyhow::Result<CloudPlan> {
                let root = StateStore::default_path()?.join("research");
                let corpus = Corpus::from_library_ids(&library, scope.as_ref(), pdfs)?;
                let (search, answer_sources) = if action == 1 {
                    let worker = LocalInference::new(config.clone(), true, false)?;
                    let progress = |fraction, status: &str| {
                        let _ = tx.send(Event::Progress(fraction, status.into()));
                        repaint.request_repaint();
                    };
                    let found = engine::search(
                        &worker,
                        &corpus,
                        &root.join("vectors"),
                        &task,
                        &cancel,
                        progress,
                    )?;
                    let sources = engine::answer_sources(&found)?.into_iter().collect();
                    (Some(found), sources)
                } else {
                    (None, vec![])
                };
                Ok(CloudPlan {
                    action,
                    corpus,
                    search,
                    answer_sources,
                    task,
                    language: config.language,
                })
            })();
            let _ = tx.send(Event::Research(ResearchEvent::CloudReady(Box::new(
                run.map_err(|error| format!("{error:#}")),
            ))));
            repaint.request_repaint();
        });
    }

    fn external_start(&mut self, plan: CloudPlan, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        let key = match self.external_key() {
            Ok(key) => key,
            Err(error) => {
                self.error = true;
                self.status = error.to_string();
                return;
            }
        };
        let config = self.research.config.clone();
        let worker =
            match ExternalInference::new(&config.external_endpoint, &config.external_model, key) {
                Ok(worker) => worker,
                Err(error) => {
                    self.error = true;
                    self.status = error.to_string();
                    return;
                }
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
                "Ausgewählte Texte werden extern ausgewertet …",
                "Selected text is being analysed externally …",
            )
            .into();
        self.progress = 0.;
        std::thread::spawn(move || {
            let run = (|| -> anyhow::Result<Report> {
                let root = StateStore::default_path()?.join("research");
                let progress = |fraction, status: &str| {
                    let _ = tx.send(Event::Progress(fraction, status.into()));
                    repaint.request_repaint();
                };
                let result = if plan.action == 1 {
                    engine::answer(
                        &worker,
                        &plan.corpus,
                        plan.search
                            .as_ref()
                            .context("Missing local search selection")?,
                        &plan.task,
                        &plan.language,
                        &cancel,
                    )?
                } else {
                    engine::analyse(
                        &worker,
                        &plan.corpus,
                        &root.join("steps"),
                        &plan.task,
                        &plan.language,
                        &cancel,
                        progress,
                    )?
                };
                engine::save_report(&root.join("results"), &result)?;
                Ok(result)
            })();
            let _ = tx.send(Event::Research(ResearchEvent::Report(Box::new(
                run.map_err(|error| format!("{error:#}")),
            ))));
            repaint.request_repaint();
        });
    }

    fn external_connection_test(&mut self, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        let key = match self.external_key() {
            Ok(key) => key,
            Err(error) => {
                self.error = true;
                self.status = error.to_string();
                return;
            }
        };
        let worker = match ExternalInference::new(
            &self.research.config.external_endpoint,
            &self.research.config.external_model,
            key,
        ) {
            Ok(worker) => worker,
            Err(error) => {
                self.error = true;
                self.status = error.to_string();
                return;
            }
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
                "Verbindung wird mit einem künstlichen Testsatz geprüft …",
                "Testing the connection with a synthetic sentence …",
            )
            .into();
        std::thread::spawn(move || {
            let result = worker
                .test_connection(&cancel)
                .map_err(|error| format!("{error:#}"));
            let _ = tx.send(Event::Research(ResearchEvent::Connection(result)));
            repaint.request_repaint();
        });
    }

    fn save_external_key(&mut self, session_only: bool) {
        let secret = self.research.api_key_input.as_str();
        let validation = ExternalInference::new(
            &self.research.config.external_endpoint,
            &self.research.config.external_model,
            Zeroizing::new(secret.to_owned()),
        );
        if let Err(error) = validation {
            self.error = true;
            self.status = error.to_string();
            return;
        }
        if session_only {
            let secret = std::mem::take(&mut self.research.api_key_input);
            self.research.session_key = Some(Zeroizing::new(secret));
            self.research.credential_status = self
                .l(
                    "Schlüssel nur für diese Sitzung übernommen.",
                    "Key retained for this session only.",
                )
                .into();
        } else if let Err(error) = crate::secrets::available()
            .and_then(|()| crate::secrets::save(&self.research.config.external_profile, secret))
        {
            self.error = true;
            self.status = format!("{error:#}");
            return;
        } else {
            self.research.session_key = None;
            self.research.api_key_input.zeroize();
            self.research.api_key_input.clear();
            self.research.credential_status = self
                .l(
                    "Schlüssel sicher im Betriebssystem gespeichert.",
                    "Key saved securely in the operating system.",
                )
                .into();
        }
        self.error = false;
        self.status = self.research.credential_status.clone();
    }

    fn delete_external_key(&mut self) {
        self.research.session_key = None;
        match crate::secrets::delete(&self.research.config.external_profile) {
            Ok(()) => {
                self.error = false;
                self.research.credential_status = self
                    .l("Gespeicherten Schlüssel gelöscht.", "Stored key deleted.")
                    .into();
                self.status = self.research.credential_status.clone();
            }
            Err(error) => {
                self.error = true;
                self.status = format!("{error:#}");
            }
        }
    }

    fn external_preview_window(&mut self, ctx: &egui::Context) {
        let Some(plan) = self.research.cloud_plan.as_ref() else {
            return;
        };
        let en = self.english;
        let t = |de, english| if en { english } else { de };
        let total = plan.source_count();
        self.research.cloud_preview_index = self
            .research
            .cloud_preview_index
            .min(total.saturating_sub(1));
        let mut index = self.research.cloud_preview_index;
        let mut confirm = false;
        let mut cancel = false;
        egui::Window::new(t("Externe Übertragung prüfen", "Review external transfer"))
            .collapsible(false)
            .resizable(true)
            .default_width(720.)
            .show(ctx, |ui| {
                ui.label(t(
                    "Nur die unten einzeln prüfbaren Texte und die Aufgabe werden an den gewählten Anbieter übertragen. Audio, Video, PDFs, Bibliothek und API-Schlüssel gehören nicht zum Anfrageinhalt.",
                    "Only the individually reviewable text below and the task are sent to the selected provider. Audio, video, PDFs, the library and the API key are not part of the request content.",
                ));
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    t(
                        "Dabei können Gebühren des Anbieters entstehen.",
                        "Provider charges may apply.",
                    ),
                );
                ui.separator();
                ui.label(format!("{}: {}", t("Modell", "Model"), self.research.config.external_model));
                ui.label(format!("{}: {}", t("API-Adresse", "API address"), self.research.config.external_endpoint));
                ui.label(format!(
                    "{} · ~{} {} · {} {}",
                    plan.source_count(),
                    plan.characters().div_ceil(4),
                    t("Eingabetokens", "input tokens"),
                    t("mindestens", "at least"),
                    plan.minimum_requests()
                ));
                ui.strong(t("Aufgabe", "Task"));
                ui.add(egui::Label::new(&plan.task).selectable(true).wrap());
                ui.separator();
                if let Some((id, source)) = plan.source(index) {
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .add_enabled(index > 0, egui::Button::new("←"))
                            .clicked()
                        {
                            index -= 1;
                        }
                        ui.label(format!(
                            "{} {}/{} · [{}] {} · {}",
                            t("Text", "Text"),
                            index + 1,
                            total,
                            id,
                            source.title,
                            source.label
                        ));
                        if ui
                            .add_enabled(index + 1 < total, egui::Button::new("→"))
                            .clicked()
                        {
                            index += 1;
                        }
                        if ui.button(t("Text kopieren", "Copy text")).clicked() {
                            ui.ctx().copy_text(source.text.clone());
                        }
                    });
                    egui::ScrollArea::vertical()
                        .id_salt("external-transfer-source")
                        .max_height(320.)
                        .show(ui, |ui| {
                            ui.add(egui::Label::new(&source.text).selectable(true).wrap());
                        });
                }
                if plan.action == 2 {
                    ui.small(t(
                        "Bei einer Gesamtauswertung sendet Laut diese Quelltexte in kleinen Abschnitten. Daraus erzeugte belegte Zwischennotizen können zur Verdichtung erneut an denselben Anbieter gesendet werden.",
                        "For a full analysis, Laut sends these source texts in small sections. The source-linked notes it creates may be sent to the same provider again for reduction.",
                    ));
                }
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button(t(
                            "Ausgewählte Texte übertragen und auswerten",
                            "Send selected text and analyse",
                        ))
                        .clicked()
                    {
                        confirm = true;
                    }
                    if ui.button(t("Abbrechen", "Cancel")).clicked() {
                        cancel = true;
                    }
                });
            });
        self.research.cloud_preview_index = index;
        if cancel {
            self.research.cloud_plan = None;
            self.status = t("Übertragung abgebrochen.", "Transfer cancelled.").into();
        } else if confirm && let Some(plan) = self.research.cloud_plan.take() {
            self.external_start(plan, ctx);
        }
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
        let mut save_key = false;
        let mut session_key = false;
        let mut delete_key = false;
        let mut test_connection = false;
        let external = self.research.config.generation_backend == "external";
        ui.heading(t("Suche & KI", "Search & AI"));
        ui.add_space(12.);
        ui.scope(|ui|{
            ui.horizontal_wrapped(|ui|{for (i,name) in [t("Suchen / Fragen","Search / ask"),t("Gesamtauswertung","Full analysis"),t("Gespeicherte Ergebnisse","Saved results"),t("Einrichtung","Setup")].iter().enumerate(){ui.selectable_value(&mut self.research.tab,i,*name);}});
            ui.label(if external {t("Suche und Quellenauswahl bleiben lokal. Externe Textauswertung beginnt erst nach einer Vorschau und ausdrücklichen Freigabe.","Search and source selection stay local. External text analysis starts only after a preview and explicit approval.")} else {t("Lokal auf der CPU. KI-Ergebnisse sind prüfbare Entwürfe; Originale bleiben erhalten.","Local CPU processing. AI results are drafts to review; originals are retained.")});
            if !self.research.warning.is_empty(){ui.colored_label(ui.visuals().error_fg_color,&self.research.warning);}
            if self.busy {ui.spinner();ui.label(&self.status);if self.running_job && ui.button(t("Abbrechen","Cancel")).clicked(){self.cancel.store(true,Ordering::Relaxed);}}
            egui::ScrollArea::vertical().id_salt("research-page").show(ui,|ui|{ui.add_enabled_ui(!self.busy,|ui|{
                match self.research.tab {
                    0=>{
                        ui.add(egui::TextEdit::multiline(&mut self.research.query).hint_text(t("Kurze Frage oder 2–5 Kernbegriffe","A short question or 2–5 key terms")).desired_rows(2).desired_width(f32::INFINITY));
                        research_scope(ui,en,&mut self.research.scope,&self.collections,self.view.as_ref());
                        ui.checkbox(&mut self.research.pdfs,t("PDF-Text und OCR einbeziehen","Include PDF text and OCR"));
                        let query_valid=!self.research.query.trim().is_empty()&&self.research.query.len()<=1000;
                        if self.research.query.len()>1000{ui.colored_label(ui.visuals().error_fg_color,format!("{} / 1000 {}",self.research.query.len(),t("Bytes – bitte auf Namen, Zahlen und Kernbegriffe kürzen","bytes – shorten to names, numbers and key terms")));}
                        ui.horizontal_wrapped(|ui|{if ui.add_enabled(query_valid,egui::Button::new(t("Exakt + semantisch suchen","Exact + semantic search"))).clicked(){action=Some(0);}
if ui.add_enabled(query_valid,egui::Button::new(if external {t("Mit externer KI beantworten …","Answer with external AI …")} else {t("Frage lokal beantworten","Answer locally")})).clicked(){action=Some(1);}});
                        ui.small(t("Namen am besten kurz suchen, z. B. „SpaceX Musk Starlink“. Direkte Worttreffer werden markiert; schwache Ähnlichkeiten bleiben zunächst ausgeblendet. Der erste Lauf baut den lokalen Index auf.","For names, use a short query such as ‘SpaceX Musk Starlink’. Literal matches are highlighted; weak similarities stay hidden initially. The first run builds the local index."));
                        if let Some(found)=self.research.search.clone() {
                            ui.label(format!("{} {}",found.passages,t("eindeutige Passagen durchsucht","unique passages searched")));
                            if found.omitted_duplicates > 0 {ui.small(format!("{} {}",found.omitted_duplicates,t("doppelte Fundstellen zusammengeführt","duplicate sources collapsed")));}
                            if !found.warnings.is_empty(){ui.collapsing(format!("{} {}",found.warnings.len(),t("PDF-Seiten ohne Suchtext","PDF pages without searchable text")),|ui|{for w in &found.warnings{ui.colored_label(ui.visuals().warn_fg_color,w);}});}
                            if found.hits.is_empty(){ui.colored_label(ui.visuals().warn_fg_color,t("Keine direkten oder ausreichend ähnlichen Treffer. Das ist belastbarer als erzwungene Top-Treffer.","No literal or sufficiently similar matches. This is more reliable than forced top results."));}
                            let mut recordings=std::collections::BTreeMap::new();
                            for hit in &found.hits{*recordings.entry(hit.source.title.clone()).or_insert(0usize)+=1;}
                            if recordings.len()>1{ui.collapsing(format!("{} {}",recordings.len(),t("Aufnahmen mit Treffern","recordings with matches")),|ui|{for (title,count) in recordings{ui.label(format!("{count} × {title}"));}});}
                            let mut show_hit=|ui:&mut egui::Ui,h:&laut_engine::research::SearchHit,weak:bool|{ui.group(|ui|{
                                ui.strong(format!("{} · {}",h.source.title,h.source.label));
                                if !h.matched_terms.is_empty(){ui.colored_label(egui::Color32::from_rgb(0,128,128),format!("{}: {}",t("Worttreffer","Literal match"),h.matched_terms.join(", ")));}
                                else if weak{ui.weak(t("Schwache thematische Ähnlichkeit – kein konkreter Worttreffer","Weak topical similarity – no literal match"));}
                                else{ui.weak(t("Semantischer Treffer – kein konkreter Worttreffer","Semantic match – no literal match"));}
                                highlighted_source(ui,&h.source.text,&h.matched_terms);
                                if ui.button(t("Fundstelle öffnen","Open source")).clicked(){source=Some(h.source.clone());}
                            });};
                            for h in &found.hits {show_hit(ui,h,false);}
                            if !found.weak_hits.is_empty(){
                                ui.checkbox(&mut self.research.show_weak,format!("{} ({})",t("Schwache Ähnlichkeiten anzeigen","Show weak similarities"),found.weak_hits.len()));
                                if self.research.show_weak{for h in &found.weak_hits{show_hit(ui,h,true);}}
                            }
                        }
                    }
                    1=>{
                        ui.label(t("Verarbeitet den gesamten ausgewählten Text des Recherchebereichs in Abschnitten. So lassen sich auch alle Teile eines Termins gemeinsam auswerten. Bestätigte Ausschlüsse bleiben ausgenommen.","Processes all selected text in the research scope in sections, including every recording in one event collection. Confirmed exclusions are omitted."));
                        research_scope(ui,en,&mut self.research.scope,&self.collections,self.view.as_ref());
                        ui.add(egui::TextEdit::multiline(&mut self.research.task).desired_rows(3).desired_width(f32::INFINITY));
                        ui.checkbox(&mut self.research.analysis_pdfs,t("Alle angehängten PDF-Seitentexte ebenfalls auswerten","Also analyse all attached PDF page text"));
                        if self.research.scope=="current"{if let Some(session)=&self.session{let n=Corpus::from_session(session,self.research.analysis_pdfs).sources.len();ui.label(format!("{n} {}",t("Passagen · lange Aufnahmen können auf der CPU deutlich dauern","passages · long recordings can take substantial CPU time")));}}else{ui.label(t("Der genaue Umfang wird nach dem Start lokal ermittelt. Sammlungen mit vielen Stunden können auf der CPU lange dauern.","The exact scope is calculated locally after starting. Multi-hour collections can take a long time on CPU."));}
                        if ui.button(if external {t("Externe Auswertung vorbereiten …","Prepare external analysis …")} else {t("Auswerten / fortsetzen","Analyse / resume")}).clicked(){action=Some(2);}
                        ui.small(if external {t("Zuerst werden Umfang und sämtliche Quelltexte lokal vorbereitet. Vor der ersten Übertragung kannst du jeden ausgewählten Abschnitt prüfen oder abbrechen.","The scope and every source passage are prepared locally first. Before the first transfer you can inspect every selected section or cancel.")} else {t("Fertige Abschnitte werden lokal zwischengespeichert. Mit derselben Aufgabe und denselben Quellen wird nach Abbruch fortgesetzt. Kein Hochladen, kein API-Schlüssel.","Completed sections are checkpointed locally. Resume with the same task and sources after cancellation. No upload or API key.")});
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
                        ui.strong(t("Semantische Suche", "Semantic search"));
                        ui.label(t("Die Suche verwendet immer das lokale Qwen3-Suchmodell. Suchanfragen, Index und Treffer werden nicht an einen KI-Anbieter geschickt.","Search always uses the local Qwen3 embedding model. Queries, index and hits are not sent to an AI provider."));
                        if ui.button(t("Modellverwaltung öffnen","Open model manager")).clicked(){models=true;}
                        for (label,path) in [(t("Suchprogramm","Embedding worker"),&mut self.research.config.embedding_worker),(t("Suchmodell","Embedding model"),&mut self.research.config.embedding_model)]{ui.horizontal_wrapped(|ui|{ui.label(label);ui.label(path.to_string_lossy());if ui.button("…").clicked()&&let Some(p)=rfd::FileDialog::new().pick_file(){*path=p;}});}
                        ui.add_space(16.);ui.separator();ui.add_space(10.);
                        ui.strong(t("Textauswertung", "Text analysis"));
                        ui.horizontal_wrapped(|ui|{
                            ui.label(t("Ausführung", "Execution"));
                            ui.selectable_value(&mut self.research.config.generation_backend,"local".into(),t("Lokal (Standard)","Local (default)"));
                            ui.selectable_value(&mut self.research.config.generation_backend,"external".into(),t("Externe API – optional","External API – optional"));
                        });
                        if self.research.config.generation_backend=="local" {
                            ui.label(t("Das lokale Textmodell benötigt keinen Zugang und überträgt keine Inhalte. Auf Rechnern ohne GPU ist es langsamer, bleibt aber vollständig offline.","The local text model needs no account and transfers no content. It is slower without a GPU, but stays fully offline."));
                            for (label,path) in [(t("Textprogramm","Text worker"),&mut self.research.config.generation_worker),(t("Textmodell","Text model"),&mut self.research.config.text_model)]{ui.horizontal_wrapped(|ui|{ui.label(label);ui.label(path.to_string_lossy());if ui.button("…").clicked()&&let Some(p)=rfd::FileDialog::new().pick_file(){*path=p;}});}
                            ui.horizontal(|ui|{ui.label(t("Modellformat","Model format"));ui.selectable_value(&mut self.research.config.text_profile,"qwen3-instruct".into(),"Qwen3 Instruct");ui.selectable_value(&mut self.research.config.text_profile,"qwen3-thinking".into(),"Qwen3 / no-think");});
                        } else {
                            ui.colored_label(ui.visuals().warn_fg_color,t("Bei jeder externen Auswertung verlassen ausgewählte Transkript- oder PDF-Texte den Rechner. Laut zeigt sie vorab einzeln an. Es werden niemals Audio-, Video- oder PDF-Dateien automatisch hochgeladen.","For every external analysis, selected transcript or PDF text leaves the computer. Laut shows each passage beforehand. Audio, video and PDF files are never uploaded automatically."));
                            ui.horizontal_wrapped(|ui|{ui.label(t("API-Basisadresse","API base URL"));ui.add(egui::TextEdit::singleline(&mut self.research.config.external_endpoint).desired_width(390.).hint_text("https://api.openai.com/v1"));});
                            ui.small(t("OpenAI Responses API oder kompatibler HTTPS-Endpunkt. Zugangsdaten in der URL sind nicht erlaubt.","OpenAI Responses API or a compatible HTTPS endpoint. Credentials in the URL are not allowed."));
                            ui.horizontal_wrapped(|ui|{ui.label(t("Modell","Model"));ui.add(egui::TextEdit::singleline(&mut self.research.config.external_model).desired_width(260.).hint_text("gpt-5-mini"));});
                            ui.horizontal_wrapped(|ui|{
                                ui.label(t("API-Schlüssel","API key"));
                                ui.add(egui::TextEdit::singleline(&mut self.research.api_key_input).password(true).desired_width(330.).hint_text(t("wird nie in Einstellungen gespeichert","never stored in preferences")));
                            });
                            ui.horizontal_wrapped(|ui|{
                                if ui.add_enabled(!self.research.api_key_input.is_empty(),egui::Button::new(t("Sicher im Betriebssystem speichern","Save securely in operating system"))).clicked(){save_key=true;}
                                if ui.add_enabled(!self.research.api_key_input.is_empty(),egui::Button::new(t("Nur für diese Sitzung","Use for this session only"))).clicked(){session_key=true;}
                                if ui.button(t("Gespeicherten Zugang löschen","Delete stored access")).clicked(){delete_key=true;}
                            });
                            ui.small(t("Dauerhafte Speicherung nutzt macOS-Schlüsselbund, Windows-Anmeldeinformationsverwaltung oder Linux Secret Service. Fehlt ein sicherer Tresor, speichert Laut den Schlüssel nicht in einer Datei; dann bleibt nur die Sitzungsauswahl.","Persistent storage uses macOS Keychain, Windows Credential Manager or Linux Secret Service. If no secure vault is available, Laut does not write the key to a file; use the session option instead."));
                            if !self.research.credential_status.is_empty(){ui.label(&self.research.credential_status);}
                            if ui.button(t("Verbindung testen","Test connection")).clicked(){test_connection=true;}
                            ui.small(t("Der Test sendet nur einen künstlichen Satz, keine Bibliotheksinhalte. Er kann eine kleine API-Gebühr verursachen.","The test sends only a synthetic sentence, never library content. It may incur a small API charge."));
                        }
                        ui.horizontal(|ui|{ui.label(t("CPU-Threads für lokale Suche/Analyse","CPU threads for local search/analysis"));ui.add(egui::DragValue::new(&mut self.research.config.threads).range(1..=32));});
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
        if save_key {
            self.save_external_key(false);
        }
        if session_key {
            self.save_external_key(true);
        }
        if delete_key {
            self.delete_external_key();
        }
        if test_connection {
            self.external_connection_test(ctx);
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
                    self.research.return_to_results = true;
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
            if action > 0 && self.research.config.generation_backend == "external" {
                self.external_prepare(action, ctx);
            } else {
                self.research_start(action, ctx);
            }
        }
        self.external_preview_window(ctx);
    }
}
