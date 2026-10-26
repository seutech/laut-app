use super::*;
use laut_engine::{
    diarization::{self, DiarizationConfig},
    downloads::{UrlImport, youtube_url},
    models,
    preferences::{Preferences, StateStore},
    queue::{BatchResult, Queue, QueueItem},
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::Path,
};

pub(super) struct ProcessingState {
    pub(super) tab: usize,
    files: Vec<PathBuf>,
    urls: String,
    pub download_paths: [String; 2],
    preview: bool,
    preview_minutes: u32,
    jobs: Vec<QueueItem>,
    last_batch: Option<String>,
    root: Option<PathBuf>,
    pub diarizer: [String; 3],
    pub speakers: u32,
    pub compact: bool,
    pub vocabulary: String,
    pub(super) analyze_after: bool,
    analysis: Option<Value>,
    mapping: BTreeMap<String, String>,
    new_names: BTreeMap<String, String>,
    installed: BTreeMap<String, PathBuf>,
    remove: Option<String>,
    install_queue: VecDeque<String>,
    install_total: usize,
    install_done: usize,
}
pub(super) struct Enqueued {
    files: Vec<PathBuf>,
    urls: Vec<String>,
    errors: Vec<String>,
    added: usize,
}
pub(super) enum ProcessingEvent {
    Queued(Result<Enqueued, String>),
    Batch(Result<BatchResult, String>),
    Package(Result<(String, Option<PathBuf>), String>),
    Analysis(Result<Value, String>),
    Cleaned(Result<(), String>),
}
impl ProcessingState {
    pub fn show_models(&mut self) {
        self.tab = 1;
    }
    pub fn clear_analysis(&mut self) {
        self.analysis = None;
        self.mapping.clear();
        self.new_names.clear();
    }
    pub fn new(p: &Preferences) -> Self {
        let root = StateStore::default_path().ok();
        let installed = root
            .as_ref()
            .map(|r| {
                models::catalog()
                    .iter()
                    .filter_map(|p| {
                        models::installed(&r.join("models"), p).map(|path| (p.id.clone(), path))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            tab: 0,
            files: vec![],
            urls: String::new(),
            download_paths: p.download_paths.clone(),
            preview: false,
            preview_minutes: 30,
            jobs: vec![],
            last_batch: None,
            root,
            diarizer: p.diarizer_paths.clone(),
            speakers: p.speaker_count,
            compact: p.compact_audio,
            vocabulary: p.vocabulary.clone(),
            analyze_after: false,
            analysis: None,
            mapping: BTreeMap::new(),
            new_names: BTreeMap::new(),
            installed,
            remove: None,
            install_queue: VecDeque::new(),
            install_total: 0,
            install_done: 0,
        }
    }
    pub(super) fn queue(&self) -> anyhow::Result<Queue> {
        Ok(Queue::new(
            self.root
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Local data folder unavailable"))?
                .join("jobs"),
        ))
    }
}
impl LautApp {
    pub(super) fn detect_processing_files(&mut self) {
        for p in models::catalog()
            .iter()
            .filter(|p| p.platform == models::platform())
        {
            let index = match p.role.as_str() {
                "downloader" => Some(0),
                "javascript_runtime" => Some(1),
                _ => None,
            };
            if let Some(i) = index
                && !Path::new(&self.processing.download_paths[i]).is_file()
                && let Some(path) = self.processing.installed.get(&p.id).cloned()
            {
                self.use_package(&p.id, &path);
            }
        }
        for (i, name) in [(0, "yt-dlp"), (1, "deno")] {
            if !Path::new(&self.processing.download_paths[i]).is_file()
                && let Some(path) = find_program(name)
            {
                self.processing.download_paths[i] = path.to_string_lossy().into_owned();
            }
        }
        for (i, name) in [(1, "whisper-cli"), (3, "ffmpeg"), (4, "ffprobe")] {
            if !Path::new(&self.job_fields[i]).is_file()
                && let Some(p) = find_program(name)
            {
                self.job_fields[i] = p.to_string_lossy().into_owned();
            }
        }
        for id in [
            "ggml-base",
            "ggml-small-q5_1",
            "speaker-segmentation",
            "speaker-embedding",
            "ocr-eng",
            "ocr-deu",
            "qwen3-embedding-0-6b-q8",
            "qwen3-4b-instruct-q4",
            "qwen3-1-7b-q8",
        ] {
            if let Some(path) = self.processing.installed.get(id).cloned() {
                let missing = match id {
                    "speaker-segmentation" => !Path::new(&self.processing.diarizer[1]).is_file(),
                    "speaker-embedding" => !Path::new(&self.processing.diarizer[2]).is_file(),
                    "ocr-eng" | "ocr-deu" => true,
                    "qwen3-embedding-0-6b-q8" => !self.research.config.embedding_model.is_file(),
                    "qwen3-1-7b-q8" | "qwen3-4b-instruct-q4" => {
                        !self.research.config.text_model.is_file()
                    }
                    _ => !Path::new(&self.job_fields[2]).is_file(),
                };
                if missing {
                    self.use_package(id, &path);
                }
            }
        }
        let id = format!("sherpa-1-13-8-{}", models::platform());
        if !Path::new(&self.processing.diarizer[0]).is_file()
            && let Some(path) = self.processing.installed.get(&id).cloned()
        {
            self.use_package(&id, &path);
        }
    }

    pub(super) fn refresh_jobs(&mut self) {
        match self.processing.queue().and_then(|q| q.list()) {
            Ok(jobs) => self.processing.jobs = jobs,
            Err(e) => {
                self.error = true;
                self.status = e.to_string();
            }
        }
    }
    pub(super) fn processing_start(&mut self) -> bool {
        if self.busy || !self.save_recovery() {
            return false;
        }
        self.busy = true;
        self.running_job = true;
        self.error = false;
        self.progress = 0.;
        self.cancel = Arc::new(AtomicBool::new(false));
        true
    }
    fn package_operation(&mut self, id: String, action: u8, ctx: &egui::Context) {
        if !self.processing_start() {
            return;
        }
        let root = self.processing.root.clone();
        let tx = self.tx.clone();
        let cancel = self.cancel.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<_> {
                let root = root
                    .ok_or_else(|| anyhow::anyhow!("No data folder"))?
                    .join("models");
                let package = models::package(&id)?;
                let path = match action {
                    0 => Some(models::install(&root, &id, &cancel, |progress, status| {
                        let _ = tx.send(Event::Progress(progress, status.into()));
                        ctx.request_repaint();
                    })?),
                    1 => Some(models::verify(&root, &package)?),
                    _ => {
                        models::remove(&root, &id)?;
                        None
                    }
                };
                Ok((id, path))
            })()
            .map_err(|error| format!("{error:#}"));
            let _ = tx.send(Event::Processing(ProcessingEvent::Package(result)));
            ctx.request_repaint();
        });
    }
    fn install_packages(&mut self, ids: Vec<String>, ctx: &egui::Context) {
        let missing: VecDeque<_> = ids
            .into_iter()
            .filter(|id| !self.processing.installed.contains_key(id))
            .collect();
        if missing.is_empty() {
            self.error = false;
            self.status = self
                .l(
                    "Die gewählten lokalen Pakete sind bereits installiert.",
                    "The selected local packages are already installed.",
                )
                .into();
            return;
        }
        self.processing.install_total = missing.len();
        self.processing.install_done = 0;
        self.processing.install_queue = missing;
        if let Some(id) = self.processing.install_queue.pop_front() {
            self.package_operation(id, 0, ctx);
        }
    }
    pub(super) fn diarization_config(&self) -> DiarizationConfig {
        DiarizationConfig {
            executable: self.processing.diarizer[0].clone().into(),
            segmentation: self.processing.diarizer[1].clone().into(),
            embedding: self.processing.diarizer[2].clone().into(),
            ffmpeg: self.job_fields[3].clone().into(),
            speakers: self.processing.speakers,
            ..Default::default()
        }
    }
    fn use_package(&mut self, id: &str, path: &Path) {
        if let Ok(p) = models::package(id) {
            let path = path.to_string_lossy().into_owned();
            match p.role.as_str() {
                "downloader" => self.processing.download_paths[0] = path,
                "javascript_runtime" => self.processing.download_paths[1] = path,
                "whisper_model" => self.job_fields[2] = path,
                "diarizer" => self.processing.diarizer[0] = path,
                "segmentation" => self.processing.diarizer[1] = path,
                "embedding" => self.processing.diarizer[2] = path,
                "text_embedding" => self.research.config.embedding_model = path.into(),
                "text_model" => {
                    self.research.config.text_model = path.into();
                    self.research.config.text_profile = if id == "qwen3-1-7b-q8" {
                        "qwen3-thinking"
                    } else {
                        "qwen3-instruct"
                    }
                    .into();
                }
                "ocr_model" => {
                    self.conference
                        .config
                        .traineddata
                        .insert(id.trim_start_matches("ocr-").into(), path.into());
                }
                _ => {}
            }
        }
    }
    pub(super) fn processing_event(&mut self, event: ProcessingEvent, ctx: &egui::Context) {
        self.busy = false;
        self.running_job = false;
        let mut next_package = None;
        let result=match event {
            ProcessingEvent::Queued(result)=>{
                self.refresh_jobs();self.processing.tab=2;self.shell.section=Section::Sources;
                result.map(|report| {
                    self.processing.files=report.files; self.processing.urls=report.urls.join("\n");
                    let mut summary=format!("{} {}",report.added,self.l("Quellen vorgemerkt. Mit Starten beginnt der Import.","sources queued. Choose Start to import."));
                    if !report.errors.is_empty() { summary.push_str(&format!("\n{}",report.errors.join("\n"))); self.error=true; }
                    self.processing.last_batch=Some(summary.clone()); self.status=summary;
                })
            },
            ProcessingEvent::Batch(result) => result.map(|batch| {
                self.refresh_jobs(); self.processing.tab = 2;
                self.shell.catalog_loaded = false;
                if let Some(path) = batch.last { self.load(path, ctx.clone()); }
                self.status = format!("{} {} · {} {}{}", batch.completed, self.l("fertig", "complete"), batch.failed.len(), self.l("fehlgeschlagen (siehe Aufträge)", "failed (see Jobs)"), if batch.paused { self.l(" · pausiert", " · paused") } else { "" });
                self.error = !batch.failed.is_empty();
                self.processing.last_batch = Some(self.status.clone());
            }),
            ProcessingEvent::Package(result)=>result.map(|(id,path)|{
                if let Some(path)=path {self.use_package(&id,&path);self.processing.installed.insert(id,path);}
                else {if let Some(path)=self.processing.installed.remove(&id){
                    for p in self.job_fields.iter_mut().chain(self.processing.diarizer.iter_mut()).chain(self.processing.download_paths.iter_mut()){if Path::new(p)==path{p.clear();}}
                }}
                if self.processing.install_total > 0 {
                    self.processing.install_done += 1;
                    next_package = self.processing.install_queue.pop_front();
                    self.status = format!(
                        "{} / {} · {}",
                        self.processing.install_done,
                        self.processing.install_total,
                        self.l("lokale Pakete eingerichtet", "local packages set up")
                    );
                    if next_package.is_none() {
                        self.processing.install_total = 0;
                        self.processing.install_done = 0;
                    }
                } else {
                    self.status=self.l("Modellverwaltung abgeschlossen.","Model operation complete.").into();
                }
            }),
            ProcessingEvent::Analysis(result)=>result.and_then(|analysis|{
                let session=self.session.as_ref().ok_or("No open recording")?;
                self.processing.mapping=laut_core::speakers::suggested_mapping(&session.document,&analysis["turns"]).map_err(|e|e.to_string())?;
                self.processing.analysis=Some(analysis);self.processing.tab=3;self.job_open=true;
                self.status=self.l("Stimmen gefunden. Zuordnungen prüfen und übernehmen.","Voices found. Review and apply the assignments.").into();Ok(())
            }),
            ProcessingEvent::Cleaned(result)=>result.map(|_|{self.status=self.l("Temporäre Auftragsdateien entfernt; Bibliothek und Original bleiben erhalten.","Temporary job files removed; library and original retained.").into();}),
        };
        if let Err(e) = result {
            self.processing.install_queue.clear();
            self.processing.install_total = 0;
            self.processing.install_done = 0;
            self.error = true;
            self.status = e;
        } else if let Some(id) = next_package {
            self.package_operation(id, 0, ctx);
        }
        ctx.request_repaint();
    }
    pub(super) fn choose_media(&mut self, ctx: &egui::Context) {
        if let Some(files) = rfd::FileDialog::new()
            .add_filter(
                "Audio / Video",
                &[
                    "wav", "mp3", "m4a", "mp4", "mov", "mkv", "webm", "flac", "aac", "ogg", "opus",
                    "aiff",
                ],
            )
            .pick_files()
        {
            for p in files {
                if !self.processing.files.contains(&p) {
                    self.processing.files.push(p);
                }
            }
            self.show_section(Section::Sources, ctx);
        }
    }
    pub(super) fn dropped_media(&mut self, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        let files: Vec<_> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if files.is_empty() {
            return;
        }
        for p in files {
            if p.is_file()
                && p.extension().is_none_or(|e| e != "json")
                && !self.processing.files.contains(&p)
            {
                self.processing.files.push(p);
            }
        }
        self.processing.tab = 0;
        self.job_open = true;
        self.refresh_jobs();
    }
    pub(super) fn processing_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let en = self.english;
        let t = |de, en_text| if en { en_text } else { de };
        let mut enqueue = false;
        let mut run_job = None;
        let mut run_all = false;
        let mut package_action = None;
        let mut install_plan = None;
        let mut analyze = false;
        let mut apply = false;
        let mut cleanup = None;
        let section = self.shell.section;
        ui.heading(match section {
            Section::Models => t("Modelle", "Models"),
            Section::Speakers => t("Sprecheranalyse", "Speaker analysis"),
            _ => t("Quellen & Aufträge", "Sources & jobs"),
        });
        if section == Section::Sources {
            self.recording_recovery(ui, ctx);
        }
        ui.add_space(12.);
        if section == Section::Models {
            self.processing.tab = 1;
        } else if section == Section::Speakers {
            self.processing.tab = 3;
            if ui
                .button(t("Zurück zum Transkript", "Back to transcript"))
                .clicked()
            {
                self.shell.section = Section::Library;
            }
        } else {
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut self.processing.tab,
                    0,
                    t("Quelle hinzufügen", "Add source"),
                );
                ui.selectable_value(&mut self.processing.tab, 2, t("Aufträge", "Jobs"));
            });
        }
        ui.scope(|ui| {
            ui.separator();
            egui::ScrollArea::vertical().id_salt(("processing-page",self.processing.tab)).show(ui,|ui|{
                if self.busy {ui.horizontal(|ui|{ui.spinner();ui.label(&self.status);});ui.add(egui::ProgressBar::new(self.progress));if self.running_job && ui.button(t("Abbrechen / pausieren","Cancel / pause")).clicked(){self.cancel.store(true,Ordering::Relaxed);}}
                ui.add_enabled_ui(!self.busy,|ui|{match self.processing.tab {
                    0=>{
                        ui.label(t("Audio und Video lokal verarbeiten. Mehrere Dateien auswählen oder in das Laut-Fenster ziehen.","Process audio and video locally. Select multiple files or drop them into the Laut window."));
                        if ui.button(t("Dateien auswählen …","Choose files …")).clicked()&& let Some(paths)=rfd::FileDialog::new().add_filter("Audio / Video",&["wav","mp3","m4a","mp4","mov","mkv","webm","flac","aac","ogg","opus","aiff"]).pick_files(){for p in paths{if !self.processing.files.contains(&p){self.processing.files.push(p);}}}
                        let mut remove=None;
                        for (i,p) in self.processing.files.iter().enumerate(){ui.horizontal(|ui|{ui.label(p.file_name().unwrap_or_default().to_string_lossy()).on_hover_text(p.display().to_string());if ui.small_button("×").clicked(){remove=Some(i);}});}
                        if let Some(i)=remove{self.processing.files.remove(i);}
                        ui.separator();
                        ui.label(t("YouTube-Links · ein Link pro Zeile", "YouTube links · one per line"));
                        ui.add(egui::TextEdit::multiline(&mut self.processing.urls).desired_rows(2).desired_width(660.).hint_text("https://www.youtube.com/watch?v=…"));
                        ui.small(t("Nur Audio. Abgeschlossene Livestreams werden unterstützt; Playlists und laufende Streams nicht. Link-Zeitmarken werden ignoriert.","Audio only. Finished live archives are supported; playlists and active streams are not. Link timestamps are ignored."));
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.processing.preview,t("Nur die ersten", "Only the first"));
                            ui.add_enabled(self.processing.preview, egui::DragValue::new(&mut self.processing.preview_minutes).range(1..=1440));
                            ui.label(t("Minuten (YouTube)", "minutes (YouTube)"));
                        });
                        let urls_valid = self.processing.urls.lines().filter(|s| !s.trim().is_empty()).try_for_each(|s| youtube_url(s).map(|_| ()));
                        if let Err(e) = &urls_valid { ui.colored_label(ui.visuals().error_fg_color,e.to_string()); }
                        let has_urls = !self.processing.urls.trim().is_empty();
                        let download_ready = !has_urls || self.processing.download_paths.iter().all(|p| Path::new(p).is_file());
                        if !download_ready { if ui.button(t("Linkimport einrichten …","Set up link import …")).clicked(){self.shell.section=Section::Models;} ui.colored_label(ui.visuals().error_fg_color,t("yt-dlp und Deno unter Modelle installieren oder auswählen.","Install or select yt-dlp and Deno under Models.")); }
                        ui.separator();
                        ui.horizontal(|ui|{ui.label(t("Sprache","Language"));for (v,label) in [("auto",t("Automatisch","Automatic")),("de","Deutsch"),("en","English")]{ui.selectable_value(&mut self.job_language,v.to_owned(),label);}});
                        ui.label(t("Namen und Fachbegriffe (optional)","Names and specialist terms (optional)"));
                        ui.add(egui::TextEdit::singleline(&mut self.processing.vocabulary).desired_width(660.).char_limit(800));
                        let model=Path::new(&self.job_fields[2]).file_name().map(|s|s.to_string_lossy().into_owned()).unwrap_or_else(||t("Noch kein Modell","No model selected").into());
                        ui.label(format!("{}: {model}",t("Sprachmodell","Speech model")));
                        ui.label(t("Standard: lokale CPU, keine besondere Grafikkarte erforderlich.","Default: local CPU, no dedicated graphics card required."));
                        ui.checkbox(&mut self.processing.compact,t("Kompakte Audiokopie (AAC unverändert extrahieren, sonst 96 kbit/s)","Compact audio copy (extract AAC unchanged, otherwise 96 kbit/s)"));
                        ui.small(t("Die Quelldatei bleibt an ihrem bisherigen Ort. Die Bibliothek enthält die Audiokopie; Video und andere Tonspuren bleiben im Original.","The source stays in its original location. The library contains the audio copy; video and other audio tracks remain in the original."));
                        ui.checkbox(&mut self.processing.analyze_after,t("Sprecheranalyse vorbereiten; Namen anschließend prüfen","Prepare speaker analysis; review names afterwards"));
                        if self.processing.analyze_after{speaker_count(ui,en,&mut self.processing.speakers);}
                        ui.label(format!("{}: {}",t("Bibliothek","Library"),self.library_root));
                        let ready=self.job_fields[1..5].iter().all(|p|Path::new(p).is_file()) && (!self.processing.analyze_after||self.processing.diarizer.iter().all(|p|Path::new(p).is_file()));
                        if !ready{if ui.button(t("Verarbeitung einrichten …","Set up processing …")).clicked(){self.shell.section=Section::Models;}ui.colored_label(ui.visuals().error_fg_color,t("Unter Modelle fehlen noch lokale Dateien.","Some local files are missing under Models."));}
                        if ui.add_enabled(ready&&download_ready&&urls_valid.is_ok()&&(!self.processing.files.is_empty()||has_urls),egui::Button::new(t("Zur Warteschlange hinzufügen","Add to queue"))).clicked(){enqueue=true;}
                    },
                    1=>{
                        ui.label(t("Wähle zuerst, was du mit Laut machen möchtest. Laut zeigt nur die dafür nötigen lokalen Bausteine; Downloads beginnen erst nach einem Klick auf Installieren.","Start by choosing what you want to do with Laut. Laut shows only the local components needed for it; downloads begin only after you choose Install."));
                        ui.small(t("Empfehlung für normale Rechner ohne besondere Grafikkarte: Whisper Small Q5 für gute Transkripte, danach nur die Funktionen ergänzen, die du wirklich nutzt.","Recommendation for ordinary computers without a dedicated GPU: Whisper Small Q5 for good transcripts, then add only the features you actually use."));
                        ui.add_space(8.);

                        let transcription_ids=vec!["ggml-small-q5_1".to_owned()];
                        let (transcription_missing,transcription_bytes)=package_plan(&self.processing.installed,&transcription_ids);
                        let transcription_programs=self.job_fields[1].is_empty()||!Path::new(&self.job_fields[1]).is_file()||!Path::new(&self.job_fields[3]).is_file()||!Path::new(&self.job_fields[4]).is_file();
                        let transcription_ready=Path::new(&self.job_fields[2]).is_file()&&!transcription_programs;
                        if setup_card(ui,en,"1 · Transkribieren","1 · Transcribe",t("Für Audio, Video und Aufnahmen. Small Q5 ist der empfohlene Kompromiss aus Qualität, Tempo und Speicherbedarf.","For audio, video and recordings. Small Q5 is the recommended balance of quality, speed and storage."),transcription_ready,&transcription_missing,transcription_bytes,transcription_programs.then_some(t("Whisper-Programm, FFmpeg oder FFprobe", "Whisper program, FFmpeg or FFprobe"))){install_plan=Some(transcription_ids);}

                        let platform=models::platform();
                        let youtube_ids=vec![format!("yt-dlp-2026-10-21-{platform}"),format!("deno-v2-9-8-{platform}")];
                        let (youtube_missing,youtube_bytes)=package_plan(&self.processing.installed,&youtube_ids);
                        let youtube_ready=self.processing.download_paths.iter().all(|path|Path::new(path).is_file())&&Path::new(&self.job_fields[3]).is_file()&&Path::new(&self.job_fields[4]).is_file();
                        if setup_card(ui,en,"2 · YouTube importieren","2 · Import YouTube",t("Fertige Videos und abgeschlossene Livestreams landen direkt in der Warteschlange. Es wird nur die Audiospur gespeichert.","Finished videos and completed livestreams go straight to the queue. Only the audio track is stored."),youtube_ready,&youtube_missing,youtube_bytes,(!Path::new(&self.job_fields[3]).is_file()||!Path::new(&self.job_fields[4]).is_file()).then_some("FFmpeg / FFprobe")){install_plan=Some(youtube_ids);}

                        let diarizer_id=format!("sherpa-1-13-8-{platform}");
                        let diarizer_packaged=models::package(&diarizer_id).is_ok();
                        let mut speaker_ids=vec!["speaker-segmentation".to_owned(),"speaker-embedding".to_owned()];
                        if diarizer_packaged {speaker_ids.insert(0,diarizer_id);}
                        let (speaker_missing,speaker_bytes)=package_plan(&self.processing.installed,&speaker_ids);
                        let speaker_ready=self.processing.diarizer.iter().all(|path|Path::new(path).is_file())&&Path::new(&self.job_fields[3]).is_file();
                        let external_diarizer=!diarizer_packaged&&!Path::new(&self.processing.diarizer[0]).is_file();
                        let ffmpeg_missing=!Path::new(&self.job_fields[3]).is_file();
                        let speaker_programs=match (external_diarizer,ffmpeg_missing){(true,true)=>Some("Sherpa / FFmpeg"),(true,false)=>Some("Sherpa"),(false,true)=>Some("FFmpeg"),_=>None};
                        if setup_card(ui,en,"3 · Sprecher unterscheiden","3 · Separate speakers",t("Optional nach der Transkription. Stimmen werden lokal gruppiert; Namen prüfst und bestätigst du anschließend selbst.","Optional after transcription. Voices are grouped locally; you review and confirm their names afterwards."),speaker_ready,&speaker_missing,speaker_bytes,speaker_programs){install_plan=Some(speaker_ids);}

                        let search_ids=vec!["qwen3-embedding-0-6b-q8".to_owned()];
                        let (search_missing,search_bytes)=package_plan(&self.processing.installed,&search_ids);
                        let search_ready=self.research.config.embedding_worker.is_file()&&self.research.config.embedding_model.is_file();
                        if setup_card(ui,en,"4 · Semantisch suchen","4 · Semantic search",t("Findet sinngleiche Passagen auch ohne exakten Suchbegriff. Index und Suchanfrage bleiben vollständig lokal.","Finds passages with the same meaning even without the exact wording. The index and query remain entirely local."),search_ready,&search_missing,search_bytes,(!self.research.config.embedding_worker.is_file()).then_some(t("lokales Suchprogramm","local search program"))){install_plan=Some(search_ids);}

                        let analysis_ids=vec!["qwen3-4b-instruct-q4".to_owned()];
                        let (analysis_missing,analysis_bytes)=package_plan(&self.processing.installed,&analysis_ids);
                        let analysis_ready=self.research.config.generation_worker.is_file()&&self.research.config.text_model.is_file();
                        if setup_card(ui,en,"5 · Lokal auswerten","5 · Analyse locally",t("Beantwortet Fragen und erstellt Zusammenfassungen aus den gewählten Belegstellen. Das 4B-Modell ist genauer, auf CPU aber deutlich langsamer.","Answers questions and creates summaries from the selected evidence. The 4B model is more accurate, but considerably slower on CPU."),analysis_ready,&analysis_missing,analysis_bytes,(!self.research.config.generation_worker.is_file()).then_some(t("lokales Textprogramm","local text program"))){install_plan=Some(analysis_ids);}

                        if transcription_programs||!self.research.config.embedding_worker.is_file()||!self.research.config.generation_worker.is_file(){
                            ui.add_space(6.);
                            ui.colored_label(egui::Color32::from_rgb(184,116,35),t("Einige Programme fehlen noch. Laut kann bereits installierte Programme automatisch finden; unbekannte Programme lassen sich darunter einmalig auswählen.","Some programs are still missing. Laut can find already installed programs automatically; unknown programs can be selected once below."));
                            if ui.button(t("Vorhandene Programme jetzt suchen","Find installed programs now")).clicked(){self.detect_processing_files();}
                        }
                        if self.processing.install_total>0 {
                            ui.label(format!("{} / {} · {}",self.processing.install_done,self.processing.install_total,t("Pakete eingerichtet","packages set up")));
                        }
                        ui.add_space(8.);
                        let packages=models::catalog();
                        ui.collapsing(t("Alle Pakete und Alternativen","All packages and alternatives"),|ui|{
                            ui.small(t("Hier findest du kleinere Testmodelle, Sprachdaten und die technischen Einzelpakete. Für den normalen Einstieg reichen die Schritte oben.","This contains smaller test models, language data and individual technical packages. For normal setup, the steps above are sufficient."));
                            for p in packages.iter().filter(|p|p.platform=="all"||p.platform==models::platform()){
                                ui.group(|ui|{ui.horizontal_wrapped(|ui|{
                                    ui.strong(&p.name);ui.label(format!("{:.0} MB · {}",p.bytes as f64/1_000_000.,p.license));
                                    if let Some(path)=self.processing.installed.get(&p.id).cloned(){
                                        ui.label(t("Installiert","Installed"));
                                        if ui.small_button(t("Verwenden","Use")).clicked(){self.use_package(&p.id,&path);}
                                        if ui.small_button(t("Prüfen","Verify")).clicked(){package_action=Some((p.id.clone(),1));}
                                        if ui.small_button(t("Entfernen …","Remove …")).clicked(){self.processing.remove=Some(p.id.clone());}
                                    } else if ui.button(t("Installieren / fortsetzen","Install / resume")).clicked(){package_action=Some((p.id.clone(),0));}
                                    ui.hyperlink_to(t("Quelle / Lizenz","Source / license"),&p.source);
                                });});
                            }
                        });
                        ui.collapsing(t("Lokale Programme und eigene Modelle","Local programs and external models"),|ui|{
                            ui.small(t("Vorhandene Programme einmal auswählen. Die folgenden Pfade werden gespeichert; externe Dateien werden nicht entfernt.","Choose existing programs once. Paths are remembered; external files are never deleted."));
                            for (i,label) in [(1,"whisper-cli"),(2,"Whisper GGML"),(3,"FFmpeg"),(4,"FFprobe")]{path_field(ui,label,&mut self.job_fields[i]);}
                            for (i,label) in ["yt-dlp", "Deno"].iter().enumerate(){path_field(ui,label,&mut self.processing.download_paths[i]);}
                            for (i,label) in ["Sherpa diarization","Segmentation ONNX","Speaker embedding ONNX"].iter().enumerate(){path_field(ui,label,&mut self.processing.diarizer[i]);}
                            if ui.button(t("Programme automatisch suchen","Find installed programs")).clicked(){self.detect_processing_files();}
                            ui.checkbox(&mut self.job_gpu,t("Whisper mit GPU, falls vom Programm unterstützt","Use GPU for Whisper if supported by the program"));
                        });
                        if let Some(root)=&self.processing.root{ui.small(format!("{}: {}",t("Lokale Ablage","Local storage"),root.display()));}
                    },
                    2=>{
                        if let Some(summary)=&self.processing.last_batch { ui.label(summary); }
                        ui.label(t("Abgebrochene Aufträge bleiben gespeichert. Nach einem Neustart beginnt nichts automatisch; Starten setzt abgeschlossene Textabschnitte fort.","Interrupted jobs are retained. Nothing starts automatically after a restart; Start resumes completed transcript chunks."));
                        if ui.button(t("Offene Aufträge starten","Start pending jobs")).clicked(){run_all=true;}
                        for job in &self.processing.jobs {
                            ui.group(|ui|{
                                let state=match job.state.as_str(){"queued"=>t("Wartet","Queued"),"running"=>t("Unterbrochen / läuft in anderem Fenster","Interrupted / running in another window"),"paused"=>t("Pausiert","Paused"),"failed"=>t("Fehler","Failed"),_=>t("Fertig","Complete")};
                                ui.horizontal_wrapped(|ui|{ui.strong(job.url_import.as_ref().map(|s| s.url.clone()).unwrap_or_else(||job.config.input.file_name().unwrap_or_default().to_string_lossy().into_owned()));ui.label(state);
                                    if let Some(p)=&job.result{if ui.button(t("Öffnen","Open")).clicked(){run_job=Some((job.id.clone(),Some(p.clone())));}
if job.config.output.exists()&&ui.small_button(t("Temporäre Dateien entfernen","Remove temporary files")).clicked(){cleanup=Some(job.id.clone());}}
                                    else if ui.button(t("Starten / fortsetzen","Start / resume")).clicked(){run_job=Some((job.id.clone(),None));}
                                });
                                if let Some(e)=&job.error{ui.colored_label(ui.visuals().error_fg_color,e);}
                            });
                        }
                    },
                    _=>{
                        ui.label(t("Erkannte Stimmen erhalten zunächst neutrale Bezeichnungen. Namen lassen sich anhand der Aufnahme zuordnen. Bestätigte Zuordnungen bleiben geschützt.","Detected voices receive neutral labels. Assign names by listening to the recording. Confirmed assignments remain protected."));
                        speaker_count(ui,en,&mut self.processing.speakers);
                        if self.player.as_ref().is_some_and(|p|p.playing()) && ui.button(t("Wiedergabe pausieren","Pause playback")).clicked() && let Some(player)=&mut self.player{player.pause();}
                        let ready=self.session.is_some()&&self.processing.diarizer.iter().all(|p|Path::new(p).is_file())&&Path::new(&self.job_fields[3]).is_file()&&!self.pending_fields();
                        if self.pending_fields(){ui.label(t("Bitte zuerst Eingaben im Editor übernehmen.","Apply editor drafts first."));}
                        if !self.processing.diarizer.iter().all(|p|Path::new(p).is_file()){ui.label(t("Sprecherprogramme und Modelle unter Modelle einrichten.","Set up speaker programs and models under Models."));}
                        if ui.add_enabled(ready,egui::Button::new(t("Stimmen lokal analysieren","Analyse voices locally"))).clicked(){analyze=true;}
                        if let Some(analysis)=self.processing.analysis.clone(){
                            let clusters:BTreeSet<_>=analysis["turns"].as_array().into_iter().flatten().filter_map(|t|t["speaker"].as_str()).map(str::to_owned).collect();
                            ui.separator();ui.label(format!("{} {}",clusters.len(),t("Stimmen – Vorschläge vor dem Übernehmen prüfen","voices – review suggestions before applying")));
                            let speakers=self.view.as_ref().map(|r|r.speakers.clone()).unwrap_or_default();
                            for (i,cluster) in clusters.iter().enumerate(){ui.horizontal_wrapped(|ui|{
                                ui.label(format!("{} {}",t("Stimme","Voice"),i+1));
                                let selected=self.processing.mapping.get(cluster).cloned().unwrap_or_default();
                                let label=speakers.iter().find(|s|s.id==selected).map(|s|s.name.as_str()).unwrap_or(t("Neue unbenannte Person","New unnamed speaker"));
                                egui::ComboBox::from_id_salt(("speaker-preview",cluster)).selected_text(label).show_ui(ui,|ui|{
                                    if ui.selectable_label(selected.is_empty(),t("Neue unbenannte Person","New unnamed speaker")).clicked(){self.processing.mapping.remove(cluster);}
                                    for s in &speakers{if ui.selectable_label(s.id==selected,&s.name).clicked(){self.processing.mapping.insert(cluster.clone(),s.id.clone());}}
                                });
                                if selected.is_empty() {
                                    ui.add(egui::TextEdit::singleline(self.processing.new_names.entry(cluster.clone()).or_default()).hint_text(t("Name (optional)","Name (optional)")).desired_width(160.).char_limit(100));
                                }
                                let turn=analysis["turns"].as_array().unwrap().iter().filter(|t|t["speaker"]==*cluster).max_by(|a,b|{let length=|t:&Value|t["end"].as_f64().unwrap_or(0.)-t["start"].as_f64().unwrap_or(0.);length(a).total_cmp(&length(b))});
                                if let Some(turn)=turn&& ui.button(t("Hörprobe","Listen")).clicked(){let at=turn["start"].as_f64().unwrap_or(0.);self.seek=at;self.seek_audio(at, true);}
                            });}
                            ui.small(t("Ohne passende Wortzeiten bleibt ein Abschnitt zusammen. Unklare Überlappungen bleiben unzugeordnet. Übernehmen ist mit Rückgängig umkehrbar.","Without matching word timings, a segment stays intact. Ambiguous overlaps remain unassigned. Applying can be undone."));
                            if ui.add_enabled(!self.pending_fields(),egui::Button::new(t("Sprechervorschläge übernehmen","Apply speaker suggestions"))).clicked(){apply=true;}
                        }
                    }
                }});
            });
        });
        if let Some(id) = self.processing.remove.clone() {
            egui::Window::new(t("Modell entfernen?","Remove model?")).collapsible(false).show(ctx,|ui|{ui.label(t("Nur dieses installierte Paket wird entfernt. Aufnahmen bleiben erhalten; das Paket lässt sich erneut installieren.","Only this installed package will be removed. Recordings are retained; the package can be reinstalled."));if ui.button(t("Entfernen","Remove")).clicked(){package_action=Some((id,2));self.processing.remove=None;}
if ui.button(t("Behalten","Keep")).clicked(){self.processing.remove=None;}});
        }
        if let Some(ids) = install_plan {
            self.install_packages(ids, ctx);
        }
        if enqueue && self.processing_start() {
            let queue = self.processing.queue();
            let files = self.processing.files.clone();
            let urls: Vec<_> = self
                .processing
                .urls
                .lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
            let download_paths = self.processing.download_paths.clone();
            let first_seconds = self
                .processing
                .preview
                .then_some(self.processing.preview_minutes as f64 * 60.);
            let lib = PathBuf::from(&self.library_root);
            let config = JobConfig {
                input: PathBuf::new(),
                output: PathBuf::new(),
                whisper: self.job_fields[1].clone().into(),
                model: self.job_fields[2].clone().into(),
                ffmpeg: self.job_fields[3].clone().into(),
                ffprobe: self.job_fields[4].clone().into(),
                language: self.job_language.clone(),
                use_gpu: self.job_gpu,
                compact_audio: self.processing.compact,
                prompt: self.processing.vocabulary.clone(),
            };
            let diarization = self
                .processing
                .analyze_after
                .then(|| self.diarization_config());
            let tx = self.tx.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let result = (|| -> anyhow::Result<Enqueued> {
                    let queue = queue?;
                    let mut report = Enqueued {
                        files: vec![],
                        urls: vec![],
                        errors: vec![],
                        added: 0,
                    };
                    let mut seen = BTreeSet::new();
                    let sources = urls
                        .into_iter()
                        .map(|url| {
                            let mut source = UrlImport {
                                url,
                                yt_dlp: download_paths[0].clone().into(),
                                deno: download_paths[1].clone().into(),
                                first_seconds,
                            };
                            source.normalize()?;
                            Ok(source)
                        })
                        .collect::<anyhow::Result<Vec<_>>>()?;
                    for source in sources {
                        if !seen.insert(source.url.clone()) {
                            continue;
                        }
                        match queue.enqueue_url(
                            config.clone(),
                            lib.clone(),
                            diarization.clone(),
                            source.clone(),
                        ) {
                            Ok(_) => report.added += 1,
                            Err(e) => {
                                report.urls.push(source.url.clone());
                                report.errors.push(format!("{}: {e:#}", source.url));
                            }
                        }
                    }
                    for file in files {
                        let mut config = config.clone();
                        config.input = file.clone();
                        match queue.enqueue(config, lib.clone(), diarization.clone()) {
                            Ok(_) => report.added += 1,
                            Err(e) => {
                                report.errors.push(format!("{}: {e:#}", file.display()));
                                report.files.push(file);
                            }
                        }
                    }
                    Ok(report)
                })()
                .map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::Processing(ProcessingEvent::Queued(result)));
                ctx.request_repaint();
            });
        }
        if let Some((id, action)) = package_action {
            self.package_operation(id, action, ctx);
        }
        if let Some(id) = cleanup
            && self.processing_start()
        {
            let q = self.processing.queue();
            let tx = self.tx.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let result = q.and_then(|q| q.cleanup(&id)).map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::Processing(ProcessingEvent::Cleaned(result)));
                ctx.request_repaint();
            });
        }
        if let Some((_, Some(path))) = &run_job {
            self.processing.analysis = None;
            self.load(path.clone(), ctx.clone());
            self.job_open = false;
        }
        let ids: Vec<String> = if run_all {
            self.processing
                .jobs
                .iter()
                .filter(|j| j.result.is_none())
                .map(|j| j.id.clone())
                .collect()
        } else {
            run_job
                .filter(|(_, p)| p.is_none())
                .map(|(id, _)| vec![id])
                .unwrap_or_default()
        };
        if !ids.is_empty() && self.processing_start() {
            let queue = self.processing.queue();
            let download_paths = self.processing.download_paths.clone();
            let tx = self.tx.clone();
            let cancel = self.cancel.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let result = queue
                    .map(|queue| {
                        if download_paths.iter().all(|p| Path::new(p).is_file()) {
                            for id in &ids {
                                let _ = queue.download_programs(
                                    id,
                                    Path::new(&download_paths[0]),
                                    Path::new(&download_paths[1]),
                                );
                            }
                        }
                        queue.run_batch(&ids, &cancel, |p, s| {
                            let _ = tx.send(Event::Progress(p, s.into()));
                            ctx.request_repaint();
                        })
                    })
                    .map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::Processing(ProcessingEvent::Batch(result)));
                ctx.request_repaint();
            });
        }
        if analyze && self.processing_start() {
            self.processing.analysis = None;
            let session = self.session.clone().unwrap();
            let config = self.diarization_config();
            let root = self.processing.root.clone();
            let tx = self.tx.clone();
            let cancel = self.cancel.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let result = root
                    .ok_or_else(|| anyhow::anyhow!("No data folder"))
                    .and_then(|r| {
                        diarization::analyze(
                            &session,
                            &config,
                            &r.join("jobs/speaker-analyses"),
                            &cancel,
                            |p, s| {
                                let _ = tx.send(Event::Progress(p, s.into()));
                                ctx.request_repaint();
                            },
                        )
                    })
                    .map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::Processing(ProcessingEvent::Analysis(result)));
                ctx.request_repaint();
            });
        }
        if apply && let Some(mut analysis) = self.processing.analysis.take() {
            analysis["names"] = serde_json::json!(
                self.processing
                    .new_names
                    .iter()
                    .filter(
                        |(cluster, name)| !self.processing.mapping.contains_key(*cluster)
                            && !name.trim().is_empty()
                    )
                    .collect::<BTreeMap<_, _>>()
            );
            self.command(Command::ApplySpeakerAnalysis {
                analysis,
                mapping: self.processing.mapping.clone(),
            });
            self.job_open = false;
        }
    }
}
fn package_plan(installed: &BTreeMap<String, PathBuf>, ids: &[String]) -> (Vec<String>, u64) {
    let missing: Vec<_> = ids
        .iter()
        .filter(|id| !installed.contains_key(*id))
        .cloned()
        .collect();
    let bytes = missing
        .iter()
        .filter_map(|id| models::package(id).ok())
        .map(|package| package.bytes)
        .sum();
    (missing, bytes)
}
#[allow(clippy::too_many_arguments)]
fn setup_card(
    ui: &mut egui::Ui,
    en: bool,
    title_de: &str,
    title_en: &str,
    description: &str,
    ready: bool,
    missing_packages: &[String],
    download_bytes: u64,
    missing_programs: Option<&str>,
) -> bool {
    let t = |de, english| if en { english } else { de };
    let mut install = false;
    ui.group(|ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.strong(if en { title_en } else { title_de });
            if ready {
                ui.colored_label(
                    egui::Color32::from_rgb(46, 139, 87),
                    t("✓ Bereit", "✓ Ready"),
                );
            } else {
                ui.colored_label(
                    egui::Color32::from_rgb(184, 116, 35),
                    t("Noch nicht vollständig", "Not complete yet"),
                );
            }
        });
        ui.label(description);
        if !ready {
            let mut missing = Vec::new();
            if !missing_packages.is_empty() {
                missing.push(format!(
                    "{} {}",
                    missing_packages.len(),
                    t("lokale Pakete", "local packages")
                ));
            }
            if let Some(programs) = missing_programs {
                missing.push(programs.to_owned());
            }
            if !missing.is_empty() {
                ui.small(format!(
                    "{}: {}",
                    t("Fehlt", "Missing"),
                    missing.join(" · ")
                ));
            }
            if !missing_packages.is_empty()
                && ui
                    .button(format!(
                        "{} ({:.0} MB)",
                        t("Benötigte Pakete installieren", "Install required packages"),
                        download_bytes as f64 / 1_000_000.
                    ))
                    .clicked()
            {
                install = true;
            }
        }
    });
    ui.add_space(5.);
    install
}
fn speaker_count(ui: &mut egui::Ui, en: bool, count: &mut u32) {
    ui.horizontal(|ui| {
        ui.label(if en {
            "Known number of speakers (0 = automatic):"
        } else {
            "Bekannte Sprecherzahl (0 = automatisch):"
        });
        ui.add(egui::DragValue::new(count).range(0..=100));
    });
}
fn path_field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(label);
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(value).desired_width(560.));
        if ui.button("…").clicked()
            && let Some(p) = rfd::FileDialog::new().pick_file()
        {
            *value = p.to_string_lossy().into_owned();
        }
    });
}
pub(super) fn find_program(name: &str) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.into()
    };
    let mut dirs: Vec<_> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        dirs.push(parent.into());
        dirs.push(parent.join("tools"));
    }
    if cfg!(target_os = "macos") {
        dirs.extend([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ]);
    }
    dirs.into_iter()
        .map(|p| p.join(&name))
        .find(|p| p.is_file())
        .and_then(|p| std::fs::canonicalize(p).ok())
}
