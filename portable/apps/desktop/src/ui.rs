mod appearance;
mod conference;
mod daily;
mod playback;
mod processing;
mod recording;
mod research;
mod shell;
mod storage;
use crate::player::Player;
use daily::DailyState;
use eframe::egui::{self, RichText};
use laut_core::{Command, Recording, paragraphs, plain_text, search, timestamp, word_ranges};
use laut_engine::{
    Session,
    jobs::JobConfig,
    library::{Catalog, Collection, Entry, Library, Revision},
};
use shell::{Section, ShellState};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

type OpenedProject = (Session, Option<(Library, Revision)>, bool);
enum Event {
    Opened(Result<OpenedProject, String>),
    Edited(Result<(Session, Revision), String>),
    Catalog(Result<(Library, Catalog), String>),
    LibrarySearch(Result<laut_engine::library_search::LibrarySearch, String>),
    Backup(Result<PathBuf, String>),
    Saved(Result<Session, String>),
    Progress(f32, String),
    Processing(processing::ProcessingEvent),
    Conference(conference::ConferenceEvent),
    Research(research::ResearchEvent),
    CaptureSaved(Result<PathBuf, String>),
    Storage(storage::StorageEvent),
    Waveform(PathBuf, Result<crate::waveform::Waveform, String>),
}
pub struct LautApp {
    daily: DailyState,
    shell: ShellState,
    conference: conference::ConferenceState,
    processing: processing::ProcessingState,
    research: research::ResearchState,
    recording: recording::RecordingState,
    playback: playback::PlaybackState,
    storage: storage::StorageState,
    session: Option<Session>,
    managed: Option<(Library, Revision)>,
    library_open: bool,
    library_root: String,
    library_entries: Vec<Entry>,
    library_warnings: Vec<String>,
    collections: Vec<Collection>,
    collection_open: bool,
    collection_editing: Option<Collection>,
    collection_remove: Option<String>,
    view: Option<Recording>,
    text: String,
    // Character ranges into the reading text, including accurate words where available.
    spans: Vec<(usize, usize, String, f64)>,
    selected: Option<String>,
    edit_text: String,
    speaker_names: Vec<(String, String)>,
    player: Option<Player>,
    seek: f64,
    dragging_seek: bool,
    query: String,
    tab: usize,
    english: bool,
    status: String,
    error: bool,
    busy: bool,
    running_job: bool,
    progress: f32,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    cancel: Arc<AtomicBool>,
    job_open: bool,
    job_fields: [String; 6],
    job_language: String,
    job_gpu: bool,
    discard_open: bool,
    pending_open: Option<PathBuf>,
    close_pending: bool,
}
impl LautApp {
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        appearance::configure(&cc.egui_ctx);
        let (tx, rx) = mpsc::channel();
        let daily = DailyState::new();
        appearance::theme(&cc.egui_ctx, &daily.preferences.theme);
        let mut app = Self {
            shell: Default::default(),
            english: daily.preferences.english,
            conference: conference::ConferenceState::new(daily.preferences.pdf.clone()),
            processing: processing::ProcessingState::new(&daily.preferences),
            research: research::ResearchState::new(daily.preferences.research.clone()),
            playback: Default::default(),
            storage: Default::default(),
            recording: Default::default(),
            daily,
            session: None,
            managed: None,
            library_open: false,
            library_root: laut_engine::library::default_root()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            library_entries: vec![],
            library_warnings: vec![],
            collections: vec![],
            collection_open: false,
            collection_editing: None,
            collection_remove: None,
            view: None,
            text: String::new(),
            spans: vec![],
            selected: None,
            edit_text: String::new(),
            speaker_names: vec![],
            player: None,
            seek: 0.,
            dragging_seek: false,
            query: String::new(),
            tab: 0,
            status: String::new(),
            error: false,
            busy: false,
            running_job: false,
            progress: 0.,
            tx,
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            job_open: false,
            job_fields: Default::default(),
            job_language: "auto".into(),
            job_gpu: false,
            discard_open: false,
            pending_open: None,
            close_pending: false,
        };
        if let Some(root) = &app.daily.preferences.library {
            app.library_root = root.to_string_lossy().into_owned();
        }
        for i in 0..4 {
            app.job_fields[i + 1] = app.daily.preferences.worker_paths[i].clone();
        }
        app.job_language = app.daily.preferences.transcription_language.clone();
        app.job_gpu = app.daily.preferences.use_gpu;
        app.detect_processing_files();
        app.status = app.daily.startup_warning.clone();
        app.error = !app.status.is_empty();
        let startup = path.or_else(|| {
            app.daily
                .preferences
                .last_project
                .clone()
                .filter(|p| p.exists())
        });
        if let Some(path) = startup {
            app.load(path, cc.egui_ctx.clone());
        } else {
            app.library_open = true;
        }
        app
    }
    fn l<'a>(&self, de: &'a str, en: &'a str) -> &'a str {
        if self.english { en } else { de }
    }
    fn load(&mut self, path: PathBuf, ctx: egui::Context) {
        if !self.save_recovery() {
            return;
        }
        self.busy = true;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<OpenedProject> {
                if let Some((library, id)) = Library::owner(&path)? {
                    let loaded = library.load(&id)?;
                    Ok((
                        loaded.session,
                        Some((library, loaded.revision)),
                        loaded.recovered,
                    ))
                } else {
                    Ok((Session::open(&path)?, None, false))
                }
            })();
            let _ = tx.send(Event::Opened(result.map_err(|e| format!("{e:#}"))));
            ctx.request_repaint();
        });
    }
    fn refresh(&mut self) {
        let Some(session) = &self.session else { return };
        let r = session.document.view();
        self.text = plain_text(&r);
        self.playback.ranges = playback::reading_ranges(&r);
        self.playback.last_followed = None;
        self.spans.clear();
        let mut offset = 0;
        for (pi, p) in paragraphs(&r).iter().enumerate() {
            if pi > 0 {
                offset += 2;
            }
            for (si, s) in p.segments.iter().enumerate() {
                if si > 0 {
                    offset += 1;
                }
                let len = s.text.chars().count();
                let ranges = word_ranges(s);
                if ranges.is_empty() {
                    self.spans
                        .push((offset, offset + len, s.id.clone(), s.start));
                } else {
                    let mut cursor = 0;
                    for w in ranges {
                        let start = s.text[..w.start].chars().count();
                        let end = s.text[..w.end].chars().count();
                        if start > cursor {
                            self.spans.push((
                                offset + cursor,
                                offset + start,
                                s.id.clone(),
                                s.start,
                            ));
                        }
                        self.spans
                            .push((offset + start, offset + end, s.id.clone(), w.time));
                        cursor = end;
                    }
                    if cursor < len {
                        self.spans
                            .push((offset + cursor, offset + len, s.id.clone(), s.start));
                    }
                }
                offset += len;
            }
        }
        let old_names: std::collections::BTreeMap<_, _> = self
            .speaker_names
            .iter()
            .filter(|(id, name)| {
                self.view
                    .as_ref()
                    .and_then(|v| v.speakers.iter().find(|s| &s.id == id))
                    .is_some_and(|s| &s.name != name)
            })
            .cloned()
            .collect();
        if self
            .view
            .as_ref()
            .is_none_or(|v| self.daily.title == v.title)
        {
            self.daily.title = r.title.clone();
        }
        if self
            .view
            .as_ref()
            .is_none_or(|v| self.daily.notes == v.notes)
        {
            self.daily.notes = r.notes.clone();
        }
        self.daily
            .texts
            .retain(|id, text| r.segments.iter().any(|s| &s.id == id && &s.text != text));
        if self
            .selected
            .as_ref()
            .is_some_and(|id| !r.segments.iter().any(|s| &s.id == id))
        {
            self.selected = None;
            self.edit_text.clear();
        }
        self.speaker_names = r
            .speakers
            .iter()
            .map(|s| {
                (
                    s.id.clone(),
                    old_names
                        .get(&s.id)
                        .cloned()
                        .unwrap_or_else(|| s.name.clone()),
                )
            })
            .collect();
        if let Some(id) = &self.selected {
            self.edit_text = r
                .segments
                .iter()
                .find(|s| &s.id == id)
                .map(|s| {
                    self.daily
                        .texts
                        .get(&s.id)
                        .cloned()
                        .unwrap_or_else(|| s.text.clone())
                })
                .unwrap_or_default();
        }
        self.conference.sync_vocabulary(
            self.view
                .as_ref()
                .and_then(|v| v.conference.as_ref())
                .map(|c| c.vocabulary.as_str())
                .unwrap_or(""),
            r.conference
                .as_ref()
                .map(|c| c.vocabulary.as_str())
                .unwrap_or(""),
        );
        self.view = Some(r);
    }
    fn start_waveform(&mut self, ctx: &egui::Context) {
        let Some(session) = &self.session else { return };
        let Ok(source) = session.audio_path() else {
            self.playback.waveform_loading = false;
            self.playback.waveform_error = Some(
                self.l(
                    "Keine Audiodatei für die Wellenform.",
                    "No audio file for the waveform.",
                )
                .into(),
            );
            return;
        };
        let Some(recording) = &self.view else { return };
        if self.playback.waveform_source.as_ref() == Some(&source)
            && (self.playback.waveform.is_some() || self.playback.waveform_loading)
        {
            return;
        }
        self.playback.waveform_cancel.store(true, Ordering::Relaxed);
        self.playback.waveform_cancel = Arc::new(AtomicBool::new(false));
        self.playback.waveform_source = Some(source.clone());
        self.playback.waveform = None;
        self.playback.waveform_error = None;
        self.playback.waveform_zoom = 1.;
        self.playback.waveform_start = 0.;
        let ffmpeg = PathBuf::from(&self.job_fields[3]);
        if !ffmpeg.is_file() {
            self.playback.waveform_loading = false;
            self.playback.waveform_error = Some(
                self.l(
                    "Wellenform benötigt FFmpeg unter Modelle → Lokale Programme.",
                    "Waveform needs FFmpeg under Models → Local programs.",
                )
                .into(),
            );
            return;
        }
        self.playback.waveform_loading = true;
        let duration = recording.duration;
        let id: String = recording
            .id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        let cache = laut_engine::preferences::StateStore::default_path()
            .unwrap_or_else(|_| std::env::temp_dir().join("LautPortableState"))
            .join("waveforms")
            .join(format!("{id}.json"));
        let cancel = self.playback.waveform_cancel.clone();
        let tx = self.tx.clone();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let result = crate::waveform::load(&ffmpeg, &source, duration, &cache, &cancel)
                .map_err(|error| format!("{error:#}"));
            let _ = tx.send(Event::Waveform(source, result));
            repaint.request_repaint();
        });
    }
    fn command(&mut self, command: Command) {
        if self.busy {
            return;
        }
        self.remember_editor();
        if matches!(
            &command,
            Command::Undo
                | Command::Redo
                | Command::SplitSegment { .. }
                | Command::MergeNext { .. }
                | Command::MergeSpeakers { .. }
                | Command::ReplaceText { .. }
                | Command::ApplySpeakerAnalysis { .. }
                | Command::EditConference { .. }
        ) && self.pending_fields()
        {
            self.error = true;
            self.status = self
                .l(
                    "Bitte zuerst alle Eingaben übernehmen.",
                    "Apply all inputs first.",
                )
                .into();
            return;
        }
        if !self.save_recovery() {
            return;
        }
        if let (Some(session), Some((library, revision))) = (&self.session, &self.managed) {
            let mut snapshot = session.clone();
            let library = library.clone();
            let mut revision = revision.clone();
            let tx = self.tx.clone();
            self.busy = true;
            self.error = false;
            self.status = self
                .l(
                    "Änderung wird lokal gespeichert …",
                    "Saving change locally …",
                )
                .into();
            std::thread::spawn(move || {
                let result = (|| -> anyhow::Result<_> {
                    snapshot.execute(command)?;
                    if snapshot.dirty {
                        library.save(&mut snapshot, &mut revision)?;
                    }
                    Ok((snapshot, revision))
                })();
                let _ = tx.send(Event::Edited(result.map_err(|e| format!("{e:#}"))));
            });
            return;
        }
        if let Some(s) = &mut self.session {
            match s.execute(command) {
                Ok(_) => {
                    self.error = false;
                    self.status = self
                        .l(
                            "Änderung übernommen · Kopie zum Speichern anlegen",
                            "Change applied · save a project copy to keep it",
                        )
                        .into();
                    self.refresh();
                }
                Err(e) => {
                    self.error = true;
                    self.status = e.to_string();
                }
            }
        }
    }
    fn select(&mut self, id: String, time: f64, play: bool) {
        if self.busy {
            return;
        }
        self.remember_editor();
        self.selected = Some(id.clone());
        if let Some(r) = &self.view {
            self.edit_text = r
                .segments
                .iter()
                .find(|s| s.id == id)
                .map(|s| {
                    self.daily
                        .texts
                        .get(&s.id)
                        .cloned()
                        .unwrap_or_else(|| s.text.clone())
                })
                .unwrap_or_default();
        }
        self.seek = time;
        self.daily.scroll_to_selection = true;
        self.playback.scroll_requested = true;
        if play || self.player.is_some() {
            self.seek_audio(time, play);
        }
    }
    fn seek_audio(&mut self, time: f64, play: bool) {
        let result = (|| -> anyhow::Result<()> {
            if self.player.is_none() {
                let path = self
                    .session
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Open a project first"))?
                    .audio_path()?;
                self.player = Some(Player::open(
                    &path,
                    self.job_fields[3].clone().into(),
                    self.daily.preferences.playback_speed,
                )?);
            }
            self.player.as_mut().unwrap().seek(time, play)?;
            Ok(())
        })();
        if let Err(e) = result {
            self.error = true;
            self.status = e.to_string();
        }
    }
    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(player) = &mut self.player {
            if let Some(error) = player.poll() {
                self.error = true;
                self.status = error;
            }
            if player.playing() || player.pending() {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Conference(event) => self.conference_event(event, ctx),
                Event::Research(event) => self.research_event(event),
                Event::CaptureSaved(result) => self.capture_saved(result, ctx, false),
                Event::Opened(result) => {
                    self.conference.clear();
                    self.processing.clear_analysis();
                    self.busy = false;
                    match result {
                        Ok((s, managed, recovered)) => {
                            self.shell.section = Section::Library;
                            self.daily.new_project();
                            self.view = None;
                            self.speaker_names.clear();
                            self.session = Some(s);
                            self.managed = managed;
                            if let Some((library, _)) = &self.managed {
                                self.collections = library.collections().unwrap_or_default();
                                let root = library.root.to_string_lossy().into_owned();
                                if self.library_root != root {
                                    self.library_entries.clear();
                                    self.library_warnings.clear();
                                    self.collections.clear();
                                    self.shell.selected_collection = None;
                                    self.shell.catalog_loaded = false;
                                }
                                self.daily.preferences.library = Some(PathBuf::from(&root));
                                self.library_root = root;
                                let r = self.session.as_ref().unwrap().document.view();
                                self.library_entries.retain(|e| e.id != r.id);
                                self.library_entries.insert(0,Entry {id:r.id,title:r.title,duration:r.duration,created_at:self.session.as_ref().unwrap().document.raw()["createdAt"].as_f64().unwrap_or(0.),recovered});
                                self.library_open = false;
                            }
                            self.player = None;
                            self.playback = Default::default();
                            self.selected = None;
                            self.seek = 0.;
                            self.dragging_seek = false;
                            self.query.clear();
                            self.error = false;
                            self.status = if recovered {
                                self.l("Vorherige gültige Fassung geladen · beschädigte Datei bleibt erhalten","Previous valid version loaded · damaged file retained")
                            } else if self.managed.is_some() {
                                self.l("Bibliotheksprojekt · übernommene Änderungen werden automatisch gespeichert","Library project · applied changes are saved automatically")
                            } else { self.l(
                                    "Projekt geladen · Original bleibt erhalten",
                                    "Project loaded · original retained",
                                ) }.into();
                            self.refresh();
                            self.start_waveform(ctx);
                            if let Some(hit) = self.daily.pending_hit.take() {
                                if let (Some(id), Some(time)) = (hit.segment_id, hit.time) {
                                    self.tab = 0;
                                    self.select(id, time, false);
                                } else if let (Some(id), Some(page)) = (hit.document_id, hit.page) {
                                    self.conference.open = true;
                                    self.show_pdf(id, page, ctx);
                                } else {
                                    self.tab = 3;
                                }
                            }
                        }
                        Err(e) => {
                            self.error = true;
                            self.status = e;
                        }
                    }
                }
                Event::Edited(result) => {
                    self.busy = false;
                    match result {
                        Ok((session, revision)) => {
                            self.session = Some(session);
                            if let Some((_, current)) = &mut self.managed {
                                *current = revision;
                            }
                            self.error = false;
                            self.status = self
                                .l(
                                    "Lokal gespeichert · vorherige Fassung gesichert",
                                    "Saved locally · previous version retained",
                                )
                                .into();
                            self.refresh();
                        }
                        Err(error) => {
                            self.error = true;
                            self.status = error;
                        }
                    }
                }
                Event::LibrarySearch(result) => {
                    self.busy = false;
                    match result {
                        Ok(result) => {
                            self.daily.hits = result.hits;
                            self.library_warnings = result.warnings;
                            self.daily.truncated = result.truncated;
                            self.daily.searched = true;
                            self.error = false;
                        }
                        Err(e) => {
                            self.error = true;
                            self.status = e;
                        }
                    }
                }
                Event::Backup(result) => {
                    self.busy = false;
                    match result {
                        Ok(path) => {
                            self.status = format!(
                                "{}: {}",
                                self.l("Sicherung geprüft", "Backup verified"),
                                path.display()
                            );
                            self.error = false;
                        }
                        Err(e) => {
                            self.error = true;
                            self.status = e;
                        }
                    }
                }
                Event::Catalog(result) => {
                    self.busy = false;
                    match result {
                        Ok((library, catalog)) => {
                            let collections = match library.collections() {
                                Ok(collections) => collections,
                                Err(error) => {
                                    self.error = true;
                                    self.status = error.to_string();
                                    continue;
                                }
                            };
                            if self
                                .managed
                                .as_ref()
                                .is_some_and(|(owner, _)| owner.root != library.root)
                            {
                                // A different empty library must not reopen the previous library's
                                // project (and silently switch back) on the next launch.
                                self.session = None;
                                self.managed = None;
                                self.view = None;
                                self.player = None;
                                self.selected = None;
                                self.speaker_names.clear();
                                self.text.clear();
                                self.spans.clear();
                                self.daily.new_project();
                                self.daily.preferences.last_project = None;
                                self.conference.clear();
                                self.processing.clear_analysis();
                                self.research.search = None;
                                self.shell.selected_collection = None;
                            }
                            self.daily.preferences.library = Some(library.root.clone());
                            self.library_root = library.root.to_string_lossy().into_owned();
                            self.library_entries = catalog.entries;
                            self.library_warnings = catalog.warnings;
                            self.collections = collections;
                            if self.shell.selected_collection.as_ref().is_some_and(|id| {
                                !self
                                    .collections
                                    .iter()
                                    .any(|collection| &collection.id == id)
                            }) {
                                self.shell.selected_collection = None;
                            }
                            self.error = false;
                            self.status = self.l("Bibliothek geladen", "Library loaded").into();
                        }
                        Err(error) => {
                            self.error = true;
                            self.status = error;
                        }
                    }
                }
                Event::Saved(result) => {
                    self.busy = false;
                    match result {
                        Ok(session) => {
                            self.error = false;
                            self.status = format!(
                                "{}: {}",
                                self.l("Gespeichert", "Saved"),
                                session.source.display()
                            );
                            self.session = Some(session);
                            self.managed = None;
                            self.refresh();
                        }
                        Err(e) => {
                            self.error = true;
                            self.status = e;
                        }
                    }
                }
                Event::Progress(p, s) => {
                    self.progress = p;
                    self.status = s;
                }
                Event::Processing(event) => self.processing_event(event, ctx),
                Event::Storage(event) => self.storage_event(event, ctx),
                Event::Waveform(source, result) => {
                    if self.playback.waveform_source.as_ref() == Some(&source) {
                        self.playback.waveform_loading = false;
                        match result {
                            Ok(waveform) => self.playback.waveform = Some(waveform),
                            Err(error) if error != "Waveform cancelled" => {
                                self.playback.waveform_error = Some(error);
                            }
                            Err(_) => {}
                        }
                    }
                }
            }
        }
    }
    fn toolbar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let accent = if ui.visuals().dark_mode {
            egui::Color32::from_rgb(16, 151, 145)
        } else {
            egui::Color32::from_rgb(0, 143, 136)
        };
        ui.horizontal(|ui| {
            if self.research.return_to_results && self.research.search.is_some() {
                if ui
                    .button(self.l("← Zur Trefferliste", "← Back to results"))
                    .clicked()
                {
                    self.conference.open = false;
                    self.conference.clear();
                    self.research.return_to_results = false;
                    self.show_section(Section::Search, ctx);
                }
                ui.separator();
            }
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new(
                        RichText::new(self.l("+ Importieren", "+ Import"))
                            .color(egui::Color32::WHITE),
                    )
                    .fill(accent)
                    .stroke(egui::Stroke::NONE),
                )
                .clicked()
            {
                self.choose_media(ctx);
            }
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new(self.l("YouTube-Link", "YouTube link")),
                )
                .clicked()
            {
                self.show_section(Section::Sources, ctx);
            }
            self.recording_buttons(ui);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button(self.l("Ablage", "File"), |ui| {
                    if ui
                        .add_enabled(
                            !self.busy,
                            egui::Button::new(self.l("Projekt öffnen …", "Open project …")),
                        )
                        .clicked()
                    {
                        ui.close();
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Laut recording", &["json"])
                            .pick_file()
                        {
                            self.load(path, ctx.clone());
                        }
                    }
                    if ui
                        .add_enabled(
                            !self.busy && self.session.is_some(),
                            egui::Button::new(
                                self.l("Projektkopie speichern …", "Save project copy …"),
                            ),
                        )
                        .clicked()
                    {
                        ui.close();
                        self.save_project_copy(ctx);
                    }
                    if ui
                        .add_enabled(
                            self.session.is_some(),
                            egui::Button::new(self.l("Exportieren …", "Export …")),
                        )
                        .clicked()
                    {
                        self.daily.export_open = true;
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .button(self.l("Entwürfe wiederherstellen …", "Recover drafts …"))
                        .clicked()
                    {
                        self.reload_drafts();
                        self.daily.recovery_open = true;
                        ui.close();
                    }
                });
            });
        });
    }
    fn save_project_copy(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("Laut-project-copy")
            .save_file()
        else {
            return;
        };
        let Some(s) = &self.session else { return };
        let snapshot = Session {
            document: s.document.clone(),
            source: s.source.clone(),
            dirty: s.dirty,
        };
        self.busy = true;
        self.status = self
            .l(
                "Projekt und Medien werden kopiert …",
                "Copying project and media …",
            )
            .into();
        let tx = self.tx.clone();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let result = snapshot
                .save_copy(&path)
                .and_then(|_| Session::open(&path))
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::Saved(result));
            repaint.request_repaint();
        });
    }
    fn left(&mut self, ui: &mut egui::Ui) {
        let Some(r) = &self.view else { return };
        ui.add_space(4.);
        ui.label(
            RichText::new(self.l("IM TRANSKRIPT", "IN THIS TRANSCRIPT"))
                .small()
                .strong(),
        );
        ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .desired_width(f32::INFINITY)
                .hint_text(if self.english {
                    "Search text or speaker"
                } else {
                    "Text oder Sprecher suchen"
                }),
        );
        ui.add_space(4.);
        let mut target = None;
        egui::ScrollArea::vertical().id_salt("navigate").show(ui, |ui| {
            if !self.query.trim().is_empty() {
                let hits = search(r, &self.query);
                ui.small(format!("{} {}", hits.len(), self.l("Treffer", "matches")));
                for hit in hits.iter().take(100) {
                    let label = format!("{}  {}", timestamp(hit.time), hit.text.chars().take(85).collect::<String>());
                    if ui.add(egui::Button::selectable(self.selected.as_ref() == Some(&hit.segment_id), label).wrap()).clicked() {
                        target = Some((hit.segment_id.clone(), hit.time));
                    }
                }
            } else {
                if let Some(c) = &r.conference {
                    for (i, chapter) in c.chapters.iter().enumerate() {
                        let active = self.seek >= chapter.start && c.chapters.get(i+1).is_none_or(|next| self.seek < next.start);
                        let label = format!("{}  {}", timestamp(chapter.start), chapter.title);
                        if ui.add(egui::Button::selectable(active, label).wrap()).clicked()
                            && let Some(s) = r.segments.iter().find(|s| s.end > chapter.start) {
                            target = Some((s.id.clone(), chapter.start));
                        }
                    }
                    if !c.chapters.is_empty() { ui.separator(); }
                }
                ui.collapsing(self.l("Sprecher", "Speakers"), |ui| {
                    for speaker in &r.speakers {
                        if ui.selectable_label(false, &speaker.name).clicked()
                            && let Some(s) = r.segments.iter().find(|s| s.speaker_id.as_ref() == Some(&speaker.id)) {
                            target = Some((s.id.clone(), s.start));
                        }
                    }
                });
                ui.add_space(8.);
                ui.collapsing(self.l("Bedienhinweise", "Reading tips"), |ui| {
                    ui.small(self.l("Doppelklick auf ein Wort startet das Audio dort. Text lässt sich über Absatzgrenzen markieren.", "Double-click a word to play from there. Select text across paragraphs."));
                    ui.small(self.l("Bestätigte Ausschlüsse fehlen in Lesefassung und Kopie. Die Abschnittsliste zeigt das Original.", "Confirmed exclusions are omitted from reading and copying. The segment list shows the original."));
                });
            }
        });
        if let Some((id, time)) = target {
            let playing = self.player.as_ref().is_some_and(|p| p.playing());
            self.select(id, time, playing);
            ui.close();
        }
    }
    fn body(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.view.is_none() {
            ui.add_space(60.);
            ui.heading(self.l(
                "Deine Transkripte. Lokal. Auf mehreren Plattformen.",
                "Your transcripts. Local. Across platforms.",
            ));
            ui.label(self.l("Öffne deine Bibliothek oder übernimm eine Projektkopie aus Laut. Korrekturen in Bibliotheksprojekten werden lokal gespeichert.","Open your library or import a project copy from Laut. Library corrections are saved locally."));
            if ui
                .button(self.l(
                    "Bibliothek öffnen oder anlegen …",
                    "Open or create library …",
                ))
                .clicked()
            {
                self.library_open = true;
                self.refresh_catalog(ctx);
            }
            ui.add_space(15.);
            ui.label(self.l(
                "Lesen · Korrigieren · Sprecher benennen · Audio hören · Text kopieren",
                "Read · Correct · Name speakers · Play audio · Copy text",
            ));
            return;
        }
        let r = self.view.as_ref().unwrap().clone();
        let title = r.title.clone();
        let duration = r.duration;
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).size(22.).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("•••", |ui| {
                    if ui
                        .add_enabled(
                            !self.busy,
                            egui::Button::new(self.l("PDFs & Konferenz …", "PDFs & conference …")),
                        )
                        .clicked()
                    {
                        self.conference.open = true;
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            !self.busy,
                            egui::Button::new(
                                self.l("Sprecher analysieren …", "Analyse speakers …"),
                            ),
                        )
                        .clicked()
                    {
                        self.show_section(Section::Speakers, ctx);
                        ui.close();
                    }
                    if ui
                        .button(self.l("KI-Auswertung …", "AI analysis …"))
                        .clicked()
                    {
                        self.show_section(Section::Search, ctx);
                        ui.close();
                    }
                })
                .response
                .on_hover_text(self.l("Werkzeuge", "Tools"));
            });
        });
        let mut source_metadata = false;
        if let Some(source) = self.session.as_ref().map(|s| &s.document.raw()["source"])
            && let Some(url) = source["url"].as_str()
            && let Ok(url) = laut_engine::downloads::youtube_url(url)
        {
            ui.horizontal_wrapped(|ui| {
                ui.hyperlink_to(self.l("YouTube-Quelle", "YouTube source"), url);
                if let Some(publisher) = source["publisher"].as_str() {
                    ui.small(publisher);
                }
                if let Some(date) = source["publishedOn"].as_str() {
                    ui.small(date);
                }
                ui.weak("·");
                ui.small(format!(
                    "{} · {} {}",
                    timestamp(duration),
                    r.segments.len(),
                    self.l("Abschnitte", "segments")
                ));
            });
            source_metadata = true;
        }
        if !source_metadata {
            ui.label(
                RichText::new(format!(
                    "{} · {} {}",
                    timestamp(duration),
                    r.segments.len(),
                    self.l("Abschnitte", "segments")
                ))
                .small()
                .weak(),
            );
        }
        if self
            .session
            .as_ref()
            .is_some_and(|s| s.document.raw()["portableCapture"]["recovered"] == true)
        {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                self.l(
                    "Wiederhergestellte Teilaufnahme – vor Verwendung auf Vollständigkeit prüfen.",
                    "Recovered partial recording – check completeness before using it.",
                ),
            );
        }
        if let Some(warnings) = self
            .session
            .as_ref()
            .and_then(|s| s.document.raw()["portableCapture"]["warnings"].as_array())
            .filter(|warnings| !warnings.is_empty())
        {
            ui.collapsing(
                self.l("Hinweise zur Aufnahme", "Recording warnings"),
                |ui| {
                    for warning in warnings.iter().filter_map(|v| v.as_str()) {
                        ui.colored_label(ui.visuals().warn_fg_color, warning);
                    }
                },
            );
        }
        let captured_without_text = r.segments.is_empty()
            && self
                .session
                .as_ref()
                .is_some_and(|s| s.document.raw()["portableCapture"].is_object());
        ui.horizontal_wrapped(|ui| {
            if captured_without_text
                && ui
                    .add_enabled(
                        !self.busy,
                        egui::Button::new(self.l("Lokal transkribieren", "Transcribe locally")),
                    )
                    .clicked()
            {
                self.transcribe_recording(ctx);
            }
        });
        ui.add_enabled_ui(self.recording.handle.is_none(), |ui| {
            self.waveform_view(ui);
            self.audio_controls(ui, ctx);
        });
        ui.add_space(4.);
        let mode_fill = if ui.visuals().dark_mode {
            egui::Color32::from_rgb(38, 40, 43)
        } else {
            egui::Color32::from_rgb(245, 247, 248)
        };
        egui::Frame::new()
            .fill(mode_fill)
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(5, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.tab,
                        0,
                        if self.english { "Read" } else { "Lesen" },
                    );
                    ui.selectable_value(
                        &mut self.tab,
                        1,
                        if self.english { "Edit" } else { "Bearbeiten" },
                    );
                    ui.selectable_value(
                        &mut self.tab,
                        2,
                        if self.english { "Speakers" } else { "Sprecher" },
                    );
                    ui.selectable_value(
                        &mut self.tab,
                        3,
                        if self.english {
                            "Notes & slides"
                        } else {
                            "Notizen & Folien"
                        },
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let raw = self.session.as_ref().unwrap().document.raw();
                        let undo = raw["editHistory"]["undo"]
                            .as_array()
                            .is_some_and(|a| !a.is_empty());
                        let redo = raw["editHistory"]["redo"]
                            .as_array()
                            .is_some_and(|a| !a.is_empty());
                        if ui
                            .add_enabled(
                                redo && !self.busy,
                                egui::Button::new(self.l("Wiederholen", "Redo")),
                            )
                            .clicked()
                        {
                            self.command(Command::Redo);
                        }
                        if ui
                            .add_enabled(
                                undo && !self.busy,
                                egui::Button::new(self.l("Rückgängig", "Undo")),
                            )
                            .clicked()
                        {
                            self.command(Command::Undo);
                        }
                    });
                });
            });
        ui.add_space(5.);
        match self.tab {
            0 => {
                let search_hint = self
                    .l(
                        "Transkript oder Sprecher suchen …",
                        "Search transcript or speakers …",
                    )
                    .to_owned();
                ui.horizontal(|ui| {
                    ui.menu_button(self.l("Kapitel", "Chapters"), |ui| {
                        ui.set_width(340.);
                        ui.set_max_height(380.);
                        self.left(ui);
                    });
                    ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .desired_width((ui.available_width() - 110.).max(140.))
                            .hint_text(search_hint),
                    );
                    if ui.button(self.l("Text kopieren", "Copy text")).clicked() {
                        ctx.copy_text(self.text.clone());
                        self.status = self.l("Gesamter Text kopiert", "All text copied").into();
                        self.error = false;
                    }
                });
                ui.add_space(4.);
                self.current_slide(ui, ctx);
                if self.query.trim().is_empty() {
                    self.reading_view(ui);
                } else {
                    let hits = search(self.view.as_ref().unwrap(), &self.query);
                    let mut selected = None;
                    ui.small(format!("{} {}", hits.len(), self.l("Treffer", "matches")));
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for hit in hits.iter().take(250) {
                            let label = format!("{}   {}", timestamp(hit.time), hit.text);
                            if ui
                                .add(egui::Button::selectable(false, label).wrap())
                                .clicked()
                            {
                                selected = Some((hit.segment_id.clone(), hit.time));
                            }
                        }
                    });
                    if let Some((id, time)) = selected {
                        self.select(id, time, false);
                        self.query.clear();
                    }
                }
            }
            1 => self.editor(ui),
            2 => {
                let mut rename = None;
                let en = self.english;
                for (id, name) in &mut self.speaker_names {
                    ui.horizontal(|ui| {
                        ui.add_enabled(!self.busy, egui::TextEdit::singleline(name));
                        if ui
                            .add_enabled(
                                !self.busy,
                                egui::Button::new(if en {
                                    "Apply name"
                                } else {
                                    "Name übernehmen"
                                }),
                            )
                            .clicked()
                        {
                            rename = Some((id.clone(), name.clone()));
                        }
                    });
                }
                self.speaker_tools(ui);
                ui.label(self.l(
                    "Namen werden von dir bestätigt. Stimmen allein beweisen keine Identität.",
                    "You confirm names. A voice alone does not establish identity.",
                ));
                if let Some((id, name)) = rename {
                    self.command(Command::RenameSpeaker { id, name });
                }
            }
            _ => self.details(ui),
        }
    }
    fn editor(&mut self, ui: &mut egui::Ui) {
        let en = self.english;
        let r = self.view.as_ref().unwrap();
        let mut select = None;
        let mut command = None;
        if let Some(id) = &self.selected
            && let Some(s) = r.segments.iter().find(|s| &s.id == id)
        {
            ui.label(format!("{} – {}", timestamp(s.start), timestamp(s.end)));
            ui.add_enabled(
                !self.busy,
                egui::TextEdit::multiline(&mut self.edit_text)
                    .desired_width(f32::INFINITY)
                    .desired_rows(4),
            );
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !self.busy,
                        egui::Button::new(self.l("Text übernehmen", "Apply text")),
                    )
                    .clicked()
                {
                    command = Some(Command::EditSegment {
                        id: id.clone(),
                        text: self.edit_text.clone(),
                    });
                }
                let mut speaker = s.speaker_id.clone();
                egui::ComboBox::from_id_salt("speaker-assignment")
                    .selected_text(r.speaker_name(speaker.as_deref()))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut speaker, None, "—");
                        for person in &r.speakers {
                            ui.selectable_value(
                                &mut speaker,
                                Some(person.id.clone()),
                                &person.name,
                            );
                        }
                    });
                if speaker != s.speaker_id && !self.busy {
                    command = Some(Command::AssignSpeaker {
                        segment_id: id.clone(),
                        speaker_id: speaker,
                    });
                }
            });
            ui.horizontal_wrapped(|ui| {
                let clean = self.edit_text == s.text;
                if ui
                    .add_enabled(
                        !self.busy && clean,
                        egui::Button::new(if en { "Split …" } else { "Teilen …" }),
                    )
                    .clicked()
                {
                    self.daily.split = Some((
                        id.clone(),
                        s.text.chars().count() / 2,
                        (s.start + s.end) / 2.,
                    ));
                }
                let index = r.segments.iter().position(|x| x.id == s.id).unwrap();
                if ui
                    .add_enabled(
                        !self.busy && clean && index + 1 < r.segments.len(),
                        egui::Button::new(if en {
                            "Merge with next …"
                        } else {
                            "Mit nächstem verbinden …"
                        }),
                    )
                    .clicked()
                {
                    self.daily.merge = Some(id.clone());
                }
                if ui
                    .add_enabled(
                        !self.busy,
                        egui::Button::new(if en {
                            "Find and replace …"
                        } else {
                            "Suchen und ersetzen …"
                        }),
                    )
                    .clicked()
                {
                    self.daily.replace_open = true;
                }
                if !clean {
                    ui.small(if en {
                        "Apply text before splitting/merging."
                    } else {
                        "Vor dem Teilen/Verbinden Text übernehmen."
                    });
                }
            });
            ui.separator();
        } else {
            ui.label(self.l(
                "Abschnitt zum Bearbeiten wählen.",
                "Choose a segment to edit.",
            ));
        }
        let height = ui.text_style_height(&egui::TextStyle::Body) + 8.;
        let mut scroll = egui::ScrollArea::vertical().id_salt("segments");
        if self.daily.scroll_to_selection
            && let Some(i) = r
                .segments
                .iter()
                .position(|s| Some(&s.id) == self.selected.as_ref())
        {
            scroll =
                scroll.vertical_scroll_offset(i as f32 * (height + ui.spacing().item_spacing.y));
            self.daily.scroll_to_selection = false;
        }
        scroll.show_rows(ui, height, r.segments.len(), |ui, range| {
            for i in range {
                let s = &r.segments[i];
                let label = format!(
                    "{}{} · {}  {}",
                    if self.daily.texts.contains_key(&s.id) {
                        "• "
                    } else {
                        ""
                    },
                    timestamp(s.start),
                    r.speaker_name(s.speaker_id.as_deref()),
                    s.text.chars().take(90).collect::<String>()
                );
                if ui
                    .selectable_label(self.selected.as_ref() == Some(&s.id), label)
                    .clicked()
                {
                    select = Some((s.id.clone(), s.start));
                }
            }
        });
        if let Some((id, t)) = select {
            self.select(id, t, false);
        }
        if let Some(command) = command {
            self.command(command);
        }
    }
    fn notes(&self, ui: &mut egui::Ui) {
        let r = self.view.as_ref().unwrap();
        egui::ScrollArea::vertical().id_salt("notes").show(ui,|ui|{

            if !r.refined_text.is_empty(){ui.heading("Text AI");ui.label(&r.refined_text);}
            if let Some(c)=&r.conference{
                ui.heading(self.l("Folienverweise","Slide references"));
                for slide in c.slides.iter().filter(|s|s.useful!=Some(false)){
                    if let Some(doc)=c.documents.iter().find(|d|d.id==slide.document_id){
                        let label=format!("{} · {} · {} {} · {}",timestamp(slide.start),doc.title,self.l("Seite","Page"),slide.page,if slide.confirmed{self.l("bestätigt","confirmed")}else{self.l("Vorschlag","suggestion")});
                        ui.collapsing(label,|ui|{if let Some(page)=doc.pages.iter().find(|p|p.number==slide.page){ui.label(&page.text);}else{ui.label(self.l("Kein Seitentext gespeichert","No saved page text"));}});
                    }
                }
                for doc in &c.documents {
                    ui.collapsing(format!("PDF · {}",doc.title),|ui|{for page in &doc.pages {
                        ui.collapsing(format!("{} {}",self.l("Seite","Page"),page.number),|ui|{let mut text=page.text.as_str();ui.add(egui::TextEdit::multiline(&mut text).desired_width(f32::INFINITY));});
                    }});
                }
                ui.label(self.l("PDF-Originale werden mit Projektkopien übernommen. Diese Vorschau zeigt gespeicherten Seitentext.","Project copies include original PDFs. This preview displays saved page text."));
            }
        });
    }
    fn library_dialog(&mut self, ctx: &egui::Context) {
        if !self.library_open {
            return;
        }
        let mut open = true;
        let mut action = None;
        let mut selected = None;
        egui::Window::new(self.l("Lokale Bibliothek","Local library")).open(&mut open).default_width(680.).show(ctx,|ui| {
            ui.label(self.l("Eigene Ablage für Laut. Vorhandene Projekte werden mit ihren Medien kopiert.","Separate storage for Laut. Existing projects are copied with their media."));
            if ui.add_enabled(!self.busy,egui::TextEdit::singleline(&mut self.library_root).desired_width(f32::INFINITY)).changed() { self.library_entries.clear();self.library_warnings.clear(); }
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(!self.busy,egui::Button::new(self.l("Ordner wählen …","Choose folder …"))).clicked()
                    && let Some(path)=rfd::FileDialog::new().pick_folder() {
                    self.library_root=path.to_string_lossy().into_owned();
                    self.library_entries.clear(); self.library_warnings.clear();
                }
                if ui.add_enabled(!self.busy,egui::Button::new(self.l("Öffnen / aktualisieren","Open / refresh"))).clicked(){action=Some(0);}
                if ui.add_enabled(!self.busy,egui::Button::new(self.l("Neue Bibliothek anlegen","Create new library"))).clicked(){action=Some(1);}
                if ui.add_enabled(!self.busy&&self.session.is_some(),egui::Button::new(self.l("Dieses Projekt aufnehmen","Import this project"))).clicked(){action=Some(2);}
            });
            ui.label(self.l("Übernommene Text- und Sprecherkorrekturen werden in Bibliotheksprojekten automatisch gespeichert. Ungesicherte Textfeldeingaben benötigen weiterhin ‚Text übernehmen‘.","Applied text and speaker corrections are saved automatically in library projects. Editor drafts still require ‘Apply text’."));
            self.library_tools(ui,ctx);
            for warning in &self.library_warnings { ui.colored_label(ui.visuals().error_fg_color,warning); }
            egui::ScrollArea::vertical().max_height(330.).show(ui,|ui| {
                for entry in self.library_entries.iter().filter(|e|self.daily.library_query.trim().is_empty()||e.title.to_lowercase().contains(&self.daily.library_query.trim().to_lowercase())) {
                    let label=format!("{} · {}{}",entry.title,timestamp(entry.duration),if entry.recovered { " · recovery" } else { "" });
                    if ui.add_enabled(!self.busy,egui::Button::new(label)).clicked(){selected=Some(entry.id.clone());}
                }
            });
        });
        self.library_open = open && !self.storage.open;
        if let Some(action) = action {
            if !self.save_recovery() {
                return;
            }
            let root = PathBuf::from(&self.library_root);
            let tx = self.tx.clone();
            let repaint = ctx.clone();
            let snapshot = self.session.clone();
            self.busy = true;
            self.error = false;
            self.status = self
                .l("Bibliothek wird vorbereitet …", "Preparing library …")
                .into();
            std::thread::spawn(move || {
                let library = if action == 0 {
                    Library::open(&root)
                } else {
                    Library::create(&root)
                };
                match library {
                    Ok(library) if action == 2 => {
                        let result = (|| -> anyhow::Result<OpenedProject> {
                            let loaded = library.import(
                                snapshot
                                    .as_ref()
                                    .ok_or_else(|| anyhow::anyhow!("Open a project first"))?,
                            )?;
                            Ok((
                                loaded.session,
                                Some((library, loaded.revision)),
                                loaded.recovered,
                            ))
                        })();
                        let _ = tx.send(Event::Opened(result.map_err(|e| format!("{e:#}"))));
                    }
                    other => {
                        let result = other.and_then(|library| {
                            library.catalog().map(|catalog| (library, catalog))
                        });
                        let _ = tx.send(Event::Catalog(result.map_err(|e| format!("{e:#}"))));
                    }
                }
                repaint.request_repaint();
            });
        }
        if let Some(id) = selected {
            let path = PathBuf::from(&self.library_root)
                .join(id)
                .join("recording.json");
            self.load(path, ctx.clone());
            self.library_open = false;
        }
    }
}
impl eframe::App for LautApp {
    fn on_exit(&mut self) {
        self.save_recovery();
        self.daily.force_flush();
        self.persist_daily(&egui::Context::default());
    }
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = root.ctx().clone();
        let ctx = &context;
        self.poll(ctx);
        self.recording_tick(ctx);
        if self.job_open {
            self.shell.section = match self.processing.tab {
                1 => Section::Models,
                3 => Section::Speakers,
                _ => Section::Sources,
            };
            self.job_open = false;
        }
        if self.research.open {
            self.shell.section = Section::Search;
            self.research.open = false;
        }
        if !self.shell.catalog_loaded && !self.busy {
            self.shell.catalog_loaded = true;
            self.refresh_catalog(ctx);
        }
        let dark_mode = root.visuals().dark_mode;
        let panel_fill = root.visuals().panel_fill;
        egui::Panel::left("app-sidebar")
            .default_size(232.)
            .size_range(214.0..=292.0)
            .resizable(true)
            .frame(
                egui::Frame::new()
                    .fill(if dark_mode {
                        egui::Color32::from_rgb(34, 36, 38)
                    } else {
                        egui::Color32::from_rgb(246, 248, 249)
                    })
                    .inner_margin(egui::Margin::symmetric(13, 9)),
            )
            .show(root, |ui| self.app_sidebar(ui, ctx));
        if ctx.input(|i| i.viewport().close_requested()) && (self.busy || !self.save_recovery()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_pending = true;
        }
        egui::Panel::top("toolbar")
            .frame(
                egui::Frame::new()
                    .fill(panel_fill)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .stroke(egui::Stroke::new(
                        0.5,
                        root.visuals().widgets.inactive.bg_stroke.color,
                    )),
            )
            .show(root, |ui| self.toolbar(ui, ctx));
        if self.recording.handle.is_some() {
            egui::Panel::top("recording-status").show(root, |ui| self.recording_bar(ui));
        }
        egui::Panel::bottom("status").show(root, |ui| {
            ui.horizontal_wrapped(|ui| {
                if self.busy {
                    ui.spinner();
                    if self.running_job && ui.button(self.l("Abbrechen", "Cancel")).clicked() {
                        self.cancel.store(true, Ordering::Relaxed);
                    }
                    if self.running_job {
                        ui.add(egui::ProgressBar::new(self.progress).desired_width(100.));
                    }
                    ctx.request_repaint_after(Duration::from_millis(150));
                }
                let text = RichText::new(&self.status).small();
                ui.label(if self.error {
                    text.color(ui.visuals().error_fg_color)
                } else {
                    text
                });
                if self.session.as_ref().is_some_and(|s| s.dirty) {
                    ui.label(self.l(
                        "• Projektkopie noch nicht gespeichert",
                        "• Project copy not saved yet",
                    ));
                }
                self.draft_status(ui);
            });
        });
        if self.player.is_some() && self.shell.section != Section::Library {
            egui::Panel::bottom("global-audio")
                .frame(
                    egui::Frame::new()
                        .fill(if dark_mode {
                            egui::Color32::from_rgb(30, 47, 49)
                        } else {
                            egui::Color32::from_rgb(235, 247, 246)
                        })
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .stroke(egui::Stroke::new(
                            0.5,
                            root.visuals().widgets.inactive.bg_stroke.color,
                        )),
                )
                .show(root, |ui| self.global_audio_bar(ui, ctx));
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(panel_fill)
                    .inner_margin(egui::Margin::symmetric(22, 14)),
            )
            .show(root, |ui| match self.shell.section {
                Section::Library => self.body(ui, ctx),
                Section::Sources | Section::Models | Section::Speakers => {
                    self.processing_page(ui, ctx)
                }
                Section::Search => self.research_page(ui, ctx),
                Section::Settings => self.settings_page(ui, ctx),
            });
        self.recording_dialog(ctx);
        self.conference_dialog(ctx);
        self.dropped_media(ctx);
        self.library_dialog(ctx);
        self.collection_dialog(ctx);
        self.storage_dialog(ctx);
        self.daily_dialogs(ctx);
        self.persist_daily(ctx);
        if self.discard_open {
            egui::Window::new(self.l("Ungespeicherte Änderungen", "Unsaved changes"))
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(self.l(
                        "Änderungen verwerfen und anderes Projekt öffnen?",
                        "Discard edits and open another project?",
                    ));
                    if ui
                        .button(self.l("Verwerfen und öffnen", "Discard and open"))
                        .clicked()
                    {
                        self.discard_open = false;
                        if let Some(p) = self.pending_open.take() {
                            self.load(p, ctx.clone());
                        }
                    }
                    if ui.button(self.l("Zurück", "Keep editing")).clicked() {
                        self.discard_open = false;
                        self.pending_open = None;
                    }
                });
        }
        if self.close_pending {
            egui::Window::new(self.l("Laut beenden", "Close Laut"))
                .collapsible(false)
                .show(ctx, |ui| {
                    if self.recording.handle.is_some() {
                        ui.label(self.l("Bitte die Aufnahme zuerst stoppen und lokal sichern.", "Stop and save the recording before closing."));
                        if ui.add_enabled(!self.recording.stopping, egui::Button::new(self.l("Aufnahme stoppen & sichern", "Stop & save recording"))).clicked() { self.stop_recording(); }
                    } else if self.busy && !self.running_job {
                        ui.label(self.l(
                            "Bitte warten, bis der Dateivorgang abgeschlossen ist.",
                            "Please wait for the file operation to finish.",
                        ));
                    } else if self.busy {
                        ui.label(self.l(
                            "Auftrag erst abbrechen und auf Abschluss warten.",
                            "Cancel the job and wait for it to finish first.",
                        ));
                        if ui
                            .button(self.l("Auftrag abbrechen", "Cancel job"))
                            .clicked()
                        {
                            self.cancel.store(true, Ordering::Relaxed);
                        }
                    } else {
                        ui.label(self.l(
                            "Die lokale Sicherung ist fehlgeschlagen. Bitte Fehler beheben oder eine Projektkopie speichern.",
                            "Local recovery could not be saved. Resolve the error or save a project copy.",
                        ));
                    }
                    if ui.button(self.l("Zurück", "Keep open")).clicked() {
                        self.close_pending = false;
                    }
                });
        }
    }
}
