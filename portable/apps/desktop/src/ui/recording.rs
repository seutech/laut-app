use super::*;
use laut_capture::{Devices, Event as CaptureEvent, Handle};
use laut_engine::recording::Manifest;
#[derive(Default)]
pub(super) struct RecordingState {
    pub handle: Option<Handle>,
    pub setup: bool,
    pub meeting: bool,
    title: String,
    devices: Devices,
    device_query: Option<mpsc::Receiver<Result<Devices, String>>>,
    pub stopping: bool,
    seconds: f64,
    peaks: [f32; 2],
    heard_microphone: bool,
    folder: Option<PathBuf>,
    pending_transcription: Option<PathBuf>,
    recoveries: Vec<(PathBuf, Manifest)>,
    recovery_loaded: bool,
}
impl LautApp {
    fn recording_root(&self) -> anyhow::Result<PathBuf> {
        Ok(laut_engine::preferences::StateStore::default_path()?.join("captures"))
    }
    fn refresh_recordings(&mut self) {
        match self
            .recording_root()
            .and_then(|root| laut_engine::recording::list(&root))
        {
            Ok(entries) => self.recording.recoveries = entries,
            Err(e) => {
                self.status = e.to_string();
                self.error = true;
            }
        }
        self.recording.recovery_loaded = true;
    }
    fn recording_devices(&mut self) {
        let (tx, rx) = mpsc::channel();
        self.recording.device_query = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(laut_capture::devices().map_err(|e| e.to_string()));
        });
    }
    fn show_recording_setup(&mut self, meeting: bool) {
        self.recording.meeting = meeting;
        self.recording.setup = true;
        self.recording.title = self
            .l(
                if meeting {
                    "Meeting"
                } else {
                    "Mikrofonaufnahme"
                },
                if meeting {
                    "Meeting"
                } else {
                    "Microphone recording"
                },
            )
            .into();
        self.recording_devices();
        self.refresh_recordings();
    }
    pub(super) fn recording_buttons(&mut self, ui: &mut egui::Ui) {
        for (meeting, de, en) in [
            (false, "Mikrofon", "Microphone"),
            (true, "Meeting", "Meeting"),
        ] {
            if ui
                .add_enabled(
                    !self.busy && (!meeting || laut_capture::meeting_supported()),
                    egui::Button::new(self.l(de, en)),
                )
                .clicked()
            {
                self.show_recording_setup(meeting);
            }
        }
    }
    pub(super) fn stop_recording(&mut self) {
        if let Some(handle) = &self.recording.handle {
            handle.stop();
            self.recording.stopping = true;
            self.status = self
                .l(
                    "Aufnahme wird lokal gesichert …",
                    "Saving recording locally …",
                )
                .into();
        }
    }
    pub(super) fn recording_tick(&mut self, ctx: &egui::Context) {
        if !self.recording.recovery_loaded {
            self.refresh_recordings();
        }
        if let Some(rx) = &self.recording.device_query
            && let Ok(result) = rx.try_recv()
        {
            self.recording.device_query = None;
            match result {
                Ok(devices) => self.recording.devices = devices,
                Err(e) => {
                    self.error = true;
                    self.status = e;
                }
            }
        }
        let events = self
            .recording
            .handle
            .as_ref()
            .map(|h| h.events.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for event in events {
            match event {
                CaptureEvent::Started(folder) => {
                    self.recording.folder = Some(folder);
                    self.status = self
                        .l("Aufnahme läuft · nur lokal", "Recording · local only")
                        .into();
                }
                CaptureEvent::Level {
                    seconds,
                    microphone,
                    system,
                } => {
                    self.recording.seconds = seconds;
                    self.recording.peaks = [microphone, system];
                    self.recording.heard_microphone |= microphone > 0.;
                }
                CaptureEvent::Finishing => {
                    self.recording.stopping = true;
                    self.status = self
                        .l(
                            "Audiospuren lokal sichern …",
                            "Saving audio tracks locally …",
                        )
                        .into();
                }
                CaptureEvent::Finished(result) => {
                    self.recording.handle.take();
                    self.capture_saved(result, ctx, self.daily.preferences.recording_transcribe);
                }
            }
        }
        if self.recording.handle.is_some() || self.recording.device_query.is_some() {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
        if !self.busy
            && let Some(path) = self.recording.pending_transcription.take()
            && self.session.as_ref().is_some_and(|s| s.source == path)
        {
            self.transcribe_recording(ctx);
        }
    }
    pub(super) fn capture_saved(
        &mut self,
        result: Result<PathBuf, String>,
        ctx: &egui::Context,
        transcribe: bool,
    ) {
        self.busy = false;
        self.running_job = false;
        self.recording.stopping = false;
        self.close_pending = false;
        self.refresh_recordings();
        match result {
            Ok(path) => {
                self.shell.catalog_loaded = false;
                if transcribe {
                    self.recording.pending_transcription = Some(path.clone());
                }
                self.load(path, ctx.clone());
            }
            Err(error) => {
                self.status=format!("{}\n{error}",self.l("Aufnahme nicht vollständig abgeschlossen. Vorhandene Audiodaten bleiben lokal erhalten; unter Quellen & Aufträge prüfen.","Recording did not finish completely. Existing audio is retained locally; review it under Sources & jobs."));
                self.error = true;
                self.shell.section = Section::Sources;
                self.processing.tab = 0;
            }
        }
    }
    pub(super) fn transcribe_recording(&mut self, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        if !self.job_fields[1..5]
            .iter()
            .all(|p| std::path::Path::new(p).is_file())
        {
            self.status=self.l("Aufnahme gespeichert. Für die Transkription zuerst Sprachmodell und Programme unter Modelle einrichten.","Recording saved. Set up a speech model and programs under Models before transcribing.").into();
            self.error = false;
            return;
        }
        let Some(session) = &self.session else {
            return;
        };
        if session.document.raw()["portableCapture"]["tracks"]
            .as_array()
            .is_some_and(|tracks| {
                !tracks.is_empty() && tracks.iter().all(|t| t["peak"].as_f64() == Some(0.))
            })
        {
            self.status = self.l("Aufnahme gespeichert, aber alle Tonspuren enthalten nur Stille. Mikrofon und Stummschaltung prüfen; keine Transkription gestartet.", "Recording saved, but all tracks contain only silence. Check microphone and mute settings; transcription was not started.").into();
            self.error = true;
            return;
        }
        let Some((library, _)) = &self.managed else {
            return;
        };
        let id = session.document.view().id;
        let library = library.root.clone();
        let config = JobConfig {
            input: PathBuf::new(),
            output: PathBuf::new(),
            whisper: self.job_fields[1].clone().into(),
            model: self.job_fields[2].clone().into(),
            ffmpeg: self.job_fields[3].clone().into(),
            ffprobe: self.job_fields[4].clone().into(),
            language: self.job_language.clone(),
            use_gpu: self.job_gpu,
            compact_audio: true,
            prompt: self.processing.vocabulary.clone(),
        };
        let queue = self.processing.queue();
        let diarization = self
            .processing
            .analyze_after
            .then(|| self.diarization_config());
        if !self.processing_start() {
            return;
        }
        let tx = self.tx.clone();
        let cancel = self.cancel.clone();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<_> {
                let queue = queue?;
                let job = queue.enqueue_recording(config, library, &id, diarization)?;
                Ok(queue.run_batch(&[job.id], &cancel, |p, s| {
                    let _ = tx.send(Event::Progress(p, s.into()));
                    repaint.request_repaint();
                }))
            })()
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::Processing(processing::ProcessingEvent::Batch(
                result,
            )));
            repaint.request_repaint();
        });
    }
    pub(super) fn recording_bar(&mut self, ui: &mut egui::Ui) {
        let starting = self.recording.folder.is_none();
        ui.horizontal_wrapped(|ui| {
            if starting {
                ui.spinner();
            } else {
                ui.colored_label(egui::Color32::from_rgb(210, 55, 55), "●");
            }
            ui.strong(self.l(
                if self.recording.meeting {
                    "Meetingaufnahme"
                } else {
                    "Mikrofonaufnahme"
                },
                if self.recording.meeting {
                    "Meeting recording"
                } else {
                    "Microphone recording"
                },
            ));
            if starting {
                ui.label(self.l("Startet …", "Starting …"));
            } else {
                ui.monospace(timestamp(self.recording.seconds));
            }
            if self.recording.stopping {
                ui.spinner();
                ui.label(self.l("Sichern …", "Saving …"));
            } else if ui
                .button(if starting {
                    self.l("Abbrechen", "Cancel")
                } else {
                    self.l("Stoppen & sichern", "Stop & save")
                })
                .clicked()
            {
                self.stop_recording();
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (index, de, en) in [
                (0, "Mikrofon", "Microphone"),
                (1, "Computerton", "Computer audio"),
            ] {
                if index == 1 && !self.recording.meeting {
                    continue;
                }
                ui.small(self.l(de, en));
                ui.add(
                    egui::ProgressBar::new(self.recording.peaks[index].clamp(0., 1.).sqrt())
                        .desired_width(110.),
                );
            }
            ui.small(self.l(
                "Lokale Aufnahme · keine Cloud",
                "Local recording · no cloud",
            ));
        });
        if self.recording.seconds >= 5. && !self.recording.heard_microphone {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                self.l(
                    "Bisher kein Mikrofonton. Mikrofon und Stummschaltung prüfen.",
                    "No microphone sound yet. Check microphone and mute settings.",
                ),
            );
        }
    }
    pub(super) fn recording_dialog(&mut self, ctx: &egui::Context) {
        if !self.recording.setup {
            return;
        }
        let mut open = true;
        let mut start = false;
        let en = self.english;
        let t = |de, en_text| if en { en_text } else { de };
        egui::Window::new(t("Aufnahme vorbereiten","Prepare recording")).open(&mut open).collapsible(false).default_width(500.).show(ctx,|ui|{
            ui.horizontal(|ui|{ui.selectable_value(&mut self.recording.meeting,false,t("Mikrofon","Microphone"));ui.add_enabled_ui(laut_capture::meeting_supported(),|ui|{ui.selectable_value(&mut self.recording.meeting,true,"Meeting");});});
            ui.label(t("Titel","Title"));ui.add(egui::TextEdit::singleline(&mut self.recording.title).desired_width(f32::INFINITY));
            ui.add_space(8.);
            device_menu(ui,"microphone",t("Mikrofon","Microphone"),&mut self.daily.preferences.recording_microphone,&self.recording.devices.microphones,en);
            if self.recording.meeting {
                #[cfg(windows)] device_menu(ui,"output",t("Audioausgabe","Audio output"),&mut self.daily.preferences.recording_output,&self.recording.devices.outputs,en);
                ui.label(t("Meeting nimmt Mikrofon und Computerton auf. Kopfhörer vermeiden, dass der Computerton zusätzlich über das Mikrofon aufgenommen wird.","Meeting records microphone and computer audio. Headphones prevent computer audio from being recorded again through the microphone."));
                #[cfg(target_os="macos")] ui.small(t("macOS benötigt Mikrofon- und Systemaudiofreigabe. Es werden keine Bildschirmbilder gespeichert.","macOS requires microphone and system-audio permission. No screen images are saved."));
            }
            if self.recording.device_query.is_some(){ui.spinner();}else if ui.button(t("Geräte aktualisieren","Refresh devices")).clicked(){self.recording_devices();}
            ui.add_space(8.);ui.checkbox(&mut self.daily.preferences.recording_transcribe,t("Nach dem Stoppen lokal transkribieren","Transcribe locally after stopping"));
            ui.small(t("Die Aufnahme wird vor der Transkription in deiner Bibliothek gespeichert. Meetings behalten beide Tonspuren.","Audio is saved to your library before transcription. Meetings retain both audio tracks."));
            let ready=std::path::Path::new(&self.library_root).join("laut-library.json").exists() && self.job_fields[3..5].iter().all(|p|std::path::Path::new(p).is_file());
            if !ready{ui.colored_label(ui.visuals().error_fg_color,t("Bibliothek sowie FFmpeg/FFprobe müssen eingerichtet sein.","Set up a library and FFmpeg/FFprobe first."));}
            start=ui.add_enabled(ready&&!self.busy&&!self.recording.title.trim().is_empty(),egui::Button::new(t("Aufnahme starten","Start recording"))).clicked();
        });
        self.recording.setup = open;
        if start {
            let config = self.recording_root().map(|root| laut_capture::Config {
                root,
                library: self.library_root.clone().into(),
                title: self.recording.title.trim().into(),
                meeting: self.recording.meeting,
                microphone: self.daily.preferences.recording_microphone.clone(),
                output: self.daily.preferences.recording_output.clone(),
                ffmpeg: self.job_fields[3].clone().into(),
                ffprobe: self.job_fields[4].clone().into(),
            });
            match config {
                Ok(config) => {
                    if let Some(player) = &mut self.player {
                        player.pause();
                    }
                    self.busy = true;
                    self.error = false;
                    self.running_job = false;
                    self.recording.seconds = 0.;
                    self.recording.peaks = [0.; 2];
                    self.recording.heard_microphone = false;
                    self.recording.stopping = false;
                    self.recording.folder = None;
                    self.recording.handle = Some(laut_capture::start(config));
                    self.recording.setup = false;
                    self.status = self
                        .l(
                            "Aufnahme startet · gegebenenfalls Systemfreigabe bestätigen …",
                            "Starting recording · check any system permission prompt …",
                        )
                        .into();
                }
                Err(e) => {
                    self.status = e.to_string();
                    self.error = true;
                }
            }
        }
    }
    pub(super) fn recording_recovery(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.recording.recoveries.is_empty() {
            return;
        }
        let mut selected = None;
        ui.collapsing(format!("{} ({})",self.l("Unfertige Aufnahmen","Unfinished recordings"),self.recording.recoveries.len()),|ui|{
            ui.label(self.l("Nach einem Abbruch vorhandene Tonspuren als Teilaufnahme sichern. Anschließend Vollständigkeit prüfen.","Save available tracks after an interruption as a partial recording, then check completeness."));
            for (path,m) in &self.recording.recoveries{ui.group(|ui|{ui.strong(&m.title);if let Some(e)=&m.error{ui.label(e);}ui.small(path.display().to_string());if ui.add_enabled(!self.busy,egui::Button::new(self.l("Teilaufnahme sichern","Save partial recording"))).clicked(){selected=Some(path.clone());}});}
        });
        if let Some(folder) = selected {
            self.busy = true;
            self.status = self
                .l(
                    "Teilaufnahme lokal sichern …",
                    "Saving partial recording locally …",
                )
                .into();
            let ffmpeg = self.job_fields[3].clone();
            let ffprobe = self.job_fields[4].clone();
            let tx = self.tx.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let result = laut_engine::recording::publish(
                    &folder,
                    std::path::Path::new(&ffmpeg),
                    std::path::Path::new(&ffprobe),
                    true,
                    &AtomicBool::new(false),
                )
                .map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::CaptureSaved(result));
                ctx.request_repaint();
            });
        }
    }
}
fn device_menu(
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    value: &mut String,
    devices: &[String],
    english: bool,
) {
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(id)
            .selected_text(if value.is_empty() {
                if english {
                    "System default"
                } else {
                    "Systemstandard"
                }
            } else {
                value.as_str()
            })
            .width(310.)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    value,
                    String::new(),
                    if english {
                        "System default"
                    } else {
                        "Systemstandard"
                    },
                );
                for device in devices {
                    ui.selectable_value(value, device.clone(), device);
                }
            });
    });
}
