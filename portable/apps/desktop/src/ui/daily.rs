use super::*;
use laut_engine::{
    library_search::LibraryHit,
    preferences::{Draft, Preferences, StateStore},
};
use std::{collections::BTreeMap, time::Instant};

pub(super) struct DailyState {
    pub preferences: Preferences,
    store: Option<StateStore>,
    saved_preferences: Preferences,
    preferences_writable: bool,
    pub startup_warning: String,
    pub title: String,
    pub notes: String,
    pub texts: BTreeMap<String, String>,
    draft_id: String,
    last_draft: Option<Draft>,
    last_flush: Instant,
    recoveries: Vec<(String, Draft)>,
    recovery_warnings: Vec<String>,
    pub recovery_open: bool,
    pub export_open: bool,
    export_format: usize,
    pub split: Option<(String, usize, f64)>,
    pub merge: Option<String>,
    pub replace_open: bool,
    find: String,
    replacement: String,
    new_speaker: String,
    merge_from: String,
    merge_into: String,
    merge_speakers_open: bool,
    pub library_query: String,
    pub hits: Vec<LibraryHit>,
    pub truncated: bool,
    pub searched: bool,
    pub pending_hit: Option<LibraryHit>,
    pub scroll_to_selection: bool,
}
impl DailyState {
    pub fn dialog_open(&self) -> bool {
        self.recovery_open
            || self.export_open
            || self.split.is_some()
            || self.merge.is_some()
            || self.replace_open
            || self.merge_speakers_open
    }
    pub fn new() -> Self {
        let mut warning = String::new();
        let store = match StateStore::default_path() {
            Ok(p) => Some(StateStore::new(p)),
            Err(e) => {
                warning = e.to_string();
                None
            }
        };
        let mut writable = true;
        let preferences = match store.as_ref().map(|s| s.preferences()).transpose() {
            Ok(p) => p.unwrap_or_default(),
            Err(e) => {
                warning = e.to_string();
                writable = false;
                Preferences::default()
            }
        };
        let (recoveries, recovery_warnings) = match store.as_ref().map(|s| s.drafts()).transpose() {
            Ok(v) => v.unwrap_or_default(),
            Err(e) => (vec![], vec![e.to_string()]),
        };
        Self {
            saved_preferences: preferences.clone(),
            preferences,
            store,
            preferences_writable: writable,
            startup_warning: warning,
            title: String::new(),
            notes: String::new(),
            texts: BTreeMap::new(),
            draft_id: StateStore::draft_id(),
            last_draft: None,
            last_flush: Instant::now(),
            recovery_open: !recoveries.is_empty() || !recovery_warnings.is_empty(),
            recoveries,
            recovery_warnings,
            export_open: false,
            export_format: 0,
            split: None,
            merge: None,
            replace_open: false,
            find: String::new(),
            replacement: String::new(),
            new_speaker: String::new(),
            merge_from: String::new(),
            merge_into: String::new(),
            merge_speakers_open: false,
            library_query: String::new(),
            hits: vec![],
            truncated: false,
            searched: false,
            pending_hit: None,
            scroll_to_selection: false,
        }
    }
    pub fn force_flush(&mut self) {
        self.last_flush = Instant::now() - Duration::from_secs(2);
    }
    pub fn new_project(&mut self) {
        self.title.clear();
        self.notes.clear();
        self.texts.clear();
        self.draft_id = StateStore::draft_id();
        self.last_draft = None;
        self.split = None;
        self.merge = None;
    }
}
impl LautApp {
    pub(super) fn remember_editor(&mut self) {
        if let Some(id) = &self.selected {
            if self
                .view
                .as_ref()
                .and_then(|r| r.segments.iter().find(|s| &s.id == id))
                .is_some_and(|s| s.text != self.edit_text)
            {
                self.daily.texts.insert(id.clone(), self.edit_text.clone());
            } else {
                self.daily.texts.remove(id);
            }
        }
    }
    pub(super) fn pending_fields(&self) -> bool {
        self.view.as_ref().is_some_and(|r| {
            !self.daily.texts.is_empty()
                || self.daily.title != r.title
                || self.daily.notes != r.notes
                || self.speaker_names.iter().any(|(id, name)| {
                    r.speakers
                        .iter()
                        .find(|s| &s.id == id)
                        .is_some_and(|s| &s.name != name)
                })
        })
    }
    pub(super) fn save_recovery(&mut self) -> bool {
        self.remember_editor();
        let Some(session) = &self.session else {
            return true;
        };
        let pending = session.dirty || self.pending_fields();
        let result = (|| -> anyhow::Result<()> {
            let store = self
                .daily
                .store
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Local recovery folder unavailable"))?;
            if !pending {
                if self.daily.last_draft.is_some() {
                    store.remove_draft(&self.daily.draft_id)?;
                    self.daily.last_draft = None;
                }
                return Ok(());
            }
            let draft = Draft {
                version: 1,
                source: session.source.clone(),
                document: session.document.raw().clone(),
                dirty: session.dirty,
                selected: self.selected.clone(),
                texts: self.daily.texts.clone(),
                title: self.daily.title.clone(),
                notes: self.daily.notes.clone(),
                speaker_names: self.speaker_names.clone(),
            };
            if self.daily.last_draft.as_ref() != Some(&draft) {
                store.save_draft(&self.daily.draft_id, &draft)?;
                self.daily.last_draft = Some(draft);
            }
            Ok(())
        })();
        if let Err(e) = result {
            self.status = format!(
                "{}: {e}",
                self.l(
                    "Entwurfssicherung fehlgeschlagen",
                    "Draft recovery save failed"
                )
            );
            self.error = true;
            false
        } else {
            true
        }
    }
    pub(super) fn persist_daily(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_secs(1));
        if self.daily.last_flush.elapsed() < Duration::from_millis(800) {
            return;
        }
        self.daily.last_flush = Instant::now();
        if !self.busy {
            self.save_recovery();
        }
        self.daily.preferences.english = self.english;
        if let Some(s) = &self.session {
            self.daily.preferences.last_project = Some(s.source.clone());
        }
        for i in 0..4 {
            self.daily.preferences.worker_paths[i] = self.job_fields[i + 1].clone();
        }
        self.daily.preferences.transcription_language = self.job_language.clone();
        self.daily.preferences.use_gpu = self.job_gpu;
        self.daily.preferences.diarizer_paths = self.processing.diarizer.clone();
        self.daily.preferences.speaker_count = self.processing.speakers;
        self.daily.preferences.compact_audio = self.processing.compact;
        self.daily.preferences.vocabulary = self.processing.vocabulary.clone();
        self.daily.preferences.download_paths = self.processing.download_paths.clone();
        self.daily.preferences.pdf = self.conference.config.clone();
        self.daily.preferences.research = self.research.config.clone();
        if self.daily.preferences != self.daily.saved_preferences && self.daily.preferences_writable
        {
            let result = self
                .daily
                .store
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Settings folder unavailable"))
                .and_then(|s| s.save_preferences(&self.daily.preferences));
            match result {
                Ok(()) => self.daily.saved_preferences = self.daily.preferences.clone(),
                Err(e) => {
                    self.status = e.to_string();
                    self.error = true;
                }
            }
        }
    }
    pub(super) fn reload_drafts(&mut self) {
        self.save_recovery();
        if let Some(store) = &self.daily.store {
            match store.drafts() {
                Ok((entries, warnings)) => {
                    self.daily.recoveries = entries;
                    self.daily.recovery_warnings = warnings;
                }
                Err(e) => {
                    self.status = e.to_string();
                    self.error = true;
                }
            }
        }
    }
    fn restore_draft(&mut self, id: String, draft: Draft) {
        if self.busy || !self.save_recovery() {
            return;
        }
        let recovered_original = draft.clone();
        let result = (|| -> anyhow::Result<_> {
            let session = draft.session()?;
            // Rejoin a library only if its saved document still matches the recovery base.
            let managed = Library::owner(&session.source)
                .ok()
                .flatten()
                .and_then(|(lib, id)| {
                    lib.load(&id)
                        .ok()
                        .filter(|loaded| {
                            loaded.session.document.raw() == session.document.raw()
                                && !session.dirty
                        })
                        .map(|loaded| (lib, loaded.revision))
                });
            Ok((session, managed))
        })();
        match result {
            Ok((mut session, managed)) => {
                if managed.is_none() {
                    session.dirty = true;
                }
                self.daily.new_project();
                self.view = None;
                self.session = Some(session);
                self.managed = managed;
                self.player = None;
                self.selected = None;
                self.speaker_names.clear();
                self.refresh();
                self.daily.draft_id = StateStore::draft_id();
                self.daily.title = draft.title;
                self.daily.notes = draft.notes;
                self.daily.texts = draft.texts;
                self.speaker_names = draft.speaker_names;
                self.selected = draft.selected;
                self.edit_text = self
                    .selected
                    .as_ref()
                    .and_then(|id| {
                        self.daily.texts.get(id).cloned().or_else(|| {
                            self.view
                                .as_ref()?
                                .segments
                                .iter()
                                .find(|s| &s.id == id)
                                .map(|s| s.text.clone())
                        })
                    })
                    .unwrap_or_default();
                self.daily.last_draft = None;
                self.daily.recovery_open = false;
                self.library_open = false;
                self.tab = if self.selected.is_some() { 1 } else { 3 };
                self.status=self.l("Entwurf wiederhergestellt · Eingaben prüfen und übernehmen. Bei zwischenzeitlichen Dateiänderungen als neue Projektkopie speichern.","Draft recovered · review and apply inputs. If files changed meanwhile, save as a new project copy.").into();
                self.error = false;
                if self.save_recovery()
                    && let Some(store) = &self.daily.store
                {
                    // Archive only after a separate, durable recovery copy exists.
                    if let Err(e) = store.archive_draft(&id, &recovered_original) {
                        self.status = format!("{} · {e}", self.status);
                    }
                }
            }
            Err(e) => {
                self.status = e.to_string();
                self.error = true;
            }
        }
    }
    pub(super) fn details(&mut self, ui: &mut egui::Ui) {
        let en = self.english;
        ui.label(if en { "Title" } else { "Titel" });
        ui.add_enabled(
            !self.busy,
            egui::TextEdit::singleline(&mut self.daily.title).desired_width(f32::INFINITY),
        );
        ui.label(if en { "Notes" } else { "Notizen" });
        egui::ScrollArea::vertical()
            .id_salt("notes-editor")
            .max_height(180.)
            .show(ui, |ui| {
                ui.add_enabled(
                    !self.busy,
                    egui::TextEdit::multiline(&mut self.daily.notes)
                        .desired_width(f32::INFINITY)
                        .desired_rows(6),
                );
            });
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new(if en {
                    "Apply title and notes"
                } else {
                    "Titel und Notizen übernehmen"
                }),
            )
            .clicked()
        {
            self.command(Command::UpdateDetails {
                title: self.daily.title.clone(),
                notes: self.daily.notes.clone(),
            });
        }
        ui.small(if en {
            "Unapplied inputs are kept in local draft recovery."
        } else {
            "Noch nicht übernommene Eingaben werden lokal als Entwurf gesichert."
        });
        self.notes(ui);
    }
    pub(super) fn speaker_tools(&mut self, ui: &mut egui::Ui) {
        let en = self.english;
        ui.separator();
        ui.horizontal(|ui| {
            ui.add_enabled(
                !self.busy,
                egui::TextEdit::singleline(&mut self.daily.new_speaker).hint_text(if en {
                    "New speaker"
                } else {
                    "Neue Person"
                }),
            );
            if ui
                .add_enabled(
                    !self.busy && !self.daily.new_speaker.trim().is_empty(),
                    egui::Button::new(if en {
                        "Add speaker"
                    } else {
                        "Person hinzufügen"
                    }),
                )
                .clicked()
            {
                self.command(Command::AddSpeaker {
                    name: self.daily.new_speaker.clone(),
                });
            }
            if ui
                .add_enabled(
                    !self.busy && self.view.as_ref().is_some_and(|r| r.speakers.len() > 1),
                    egui::Button::new(if en {
                        "Merge speakers …"
                    } else {
                        "Personen zusammenführen …"
                    }),
                )
                .clicked()
            {
                self.daily.merge_speakers_open = true;
            }
        });
    }
    pub(super) fn library_tools(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let en = self.english;
        let mut search = false;
        ui.separator();
        ui.horizontal(|ui| {
            let response = ui.add_enabled(
                !self.busy,
                egui::TextEdit::singleline(&mut self.daily.library_query)
                    .hint_text(if en {
                        "Search all projects"
                    } else {
                        "Alle Projekte durchsuchen"
                    })
                    .desired_width(360.),
            );
            if response.changed() {
                self.daily.hits.clear();
                self.daily.searched = false;
            }
            search = ui
                .add_enabled(
                    !self.busy && !self.daily.library_query.trim().is_empty(),
                    egui::Button::new(if en {
                        "Search contents"
                    } else {
                        "Inhalte durchsuchen"
                    }),
                )
                .clicked()
                || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        });
        if search && !self.busy {
            let root = PathBuf::from(&self.library_root);
            let q = self.daily.library_query.clone();
            let tx = self.tx.clone();
            let ctx = ctx.clone();
            self.busy = true;
            std::thread::spawn(move || {
                let result = Library::open(&root)
                    .and_then(|lib| lib.search(&q))
                    .map_err(|e| e.to_string());
                let _ = tx.send(Event::LibrarySearch(result));
                ctx.request_repaint();
            });
        }
        let mut target = None;
        if self.daily.searched {
            ui.label(format!(
                "{} {}{}",
                self.daily.hits.len(),
                if en { "matches" } else { "Treffer" },
                if self.daily.truncated {
                    if en {
                        " · first 200; refine search"
                    } else {
                        " · erste 200; Suche eingrenzen"
                    }
                } else {
                    ""
                }
            ));
            egui::ScrollArea::vertical()
                .id_salt("library-results")
                .max_height(230.)
                .show(ui, |ui| {
                    for hit in &self.daily.hits {
                        let label = format!(
                            "{} · {}{}\n{}",
                            hit.title,
                            match hit.kind.as_str() {
                                "title" =>
                                    if en {
                                        "Title"
                                    } else {
                                        "Titel"
                                    },
                                "notes" =>
                                    if en {
                                        "Notes"
                                    } else {
                                        "Notizen"
                                    },
                                "analysis" => "Text AI",
                                "document" => "PDF",
                                _ =>
                                    if en {
                                        "Transcript"
                                    } else {
                                        "Transkript"
                                    },
                            },
                            hit.time
                                .map(|t| format!(" · {}", timestamp(t)))
                                .unwrap_or_default(),
                            hit.text
                        );
                        if ui
                            .add_enabled(!self.busy, egui::Button::new(label).wrap())
                            .clicked()
                        {
                            target = Some(hit.clone());
                        }
                    }
                });
        }
        if let Some(hit) = target {
            let path = PathBuf::from(&self.library_root)
                .join(hit.recording_id.to_uppercase())
                .join("recording.json");
            self.daily.pending_hit = Some(hit);
            self.load(path, ctx.clone());
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new(if en {
                        "Back up library …"
                    } else {
                        "Bibliothek sichern …"
                    }),
                )
                .clicked()
                && let Some(destination) = rfd::FileDialog::new()
                    .set_file_name("Laut-backup")
                    .save_file()
            {
                if !self.save_recovery() {
                    return;
                }
                let root = PathBuf::from(&self.library_root);
                let tx = self.tx.clone();
                let ctx = ctx.clone();
                self.busy = true;
                std::thread::spawn(move || {
                    let result = Library::open(&root)
                        .and_then(|lib| lib.backup(&destination))
                        .map(|_| destination)
                        .map_err(|e| format!("{e:#}"));
                    let _ = tx.send(Event::Backup(result));
                    ctx.request_repaint();
                });
            }
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new(if en {
                        "Restore backup …"
                    } else {
                        "Sicherung wiederherstellen …"
                    }),
                )
                .clicked()
                && let Some(source) = rfd::FileDialog::new().pick_folder()
                && let Some(destination) = rfd::FileDialog::new()
                    .set_file_name("Laut-restored-library")
                    .save_file()
            {
                let tx = self.tx.clone();
                let ctx = ctx.clone();
                self.busy = true;
                std::thread::spawn(move || {
                    let result = Library::restore_backup(&source, &destination)
                        .and_then(|lib| lib.catalog().map(|cat| (lib, cat)))
                        .map_err(|e| format!("{e:#}"));
                    let _ = tx.send(Event::Catalog(result));
                    ctx.request_repaint();
                });
            }
            self.storage_button(ui);
        });
        ui.small(if en{"Backups include saved projects, previous versions and media. Unapplied drafts remain in this computer’s separate recovery folder. Restore creates a new library."}else{"Sicherungen enthalten gespeicherte Projekte, vorherige Fassungen und Medien. Nicht übernommene Entwürfe bleiben in der separaten lokalen Entwurfsablage. Wiederherstellung erzeugt eine neue Bibliothek."});
    }
    pub(super) fn daily_dialogs(&mut self, ctx: &egui::Context) {
        let en = self.english;
        if self.daily.export_open {
            let mut open = true;
            let mut export = false;
            egui::Window::new(if en{"Export transcript"}else{"Transkript exportieren"}).open(&mut open).show(ctx,|ui|{
                for (i,label) in ["TXT","Markdown","SRT","JSON"].iter().enumerate(){ui.radio_value(&mut self.daily.export_format,i,*label);}
                ui.label(if en{"Exports use applied changes. TXT/Markdown/SRT omit confirmed exclusions. SRT uses local audio time; JSON retains the complete original document and history. JSON alone does not include media."}else{"Exportiert werden übernommene Änderungen. TXT/Markdown/SRT berücksichtigen bestätigte Ausschlüsse. SRT verwendet die lokale Audiozeit; JSON enthält das vollständige Dokument mit Verlauf. JSON allein enthält keine Medien."});
                export=ui.add_enabled(!self.busy&&self.session.is_some(),egui::Button::new(if en{"Save export …"}else{"Export speichern …"})).clicked();
            });
            self.daily.export_open = open;
            if export && let Some(s) = &self.session {
                let ext = ["txt", "md", "srt", "json"][self.daily.export_format];
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name(format!("transcript.{ext}"))
                    .save_file()
                {
                    let r = s.document.view();
                    let data = match self.daily.export_format {
                        0 => Ok(laut_core::plain_text(&r)),
                        1 => Ok(laut_core::markdown(&r)),
                        2 => Ok(laut_core::srt(&r)),
                        _ => serde_json::to_string_pretty(s.document.raw())
                            .map_err(anyhow::Error::from),
                    };
                    match data.and_then(|d| laut_engine::write_new(&path, d.as_bytes())) {
                        Ok(()) => {
                            self.status = format!(
                                "{}: {}",
                                if en { "Exported" } else { "Exportiert" },
                                path.display()
                            );
                            self.error = false;
                            self.daily.export_open = false;
                        }
                        Err(e) => {
                            self.status = e.to_string();
                            self.error = true;
                        }
                    }
                }
            }
        }
        if self.daily.recovery_open {
            let mut open = true;
            let mut restore = None;
            egui::Window::new(if en{"Local drafts"}else{"Lokale Entwürfe"}).open(&mut open).default_width(600.).show(ctx,|ui|{
                ui.label(if en{"Drafts keep unapplied inputs and unsaved external projects. Recovery never silently overwrites a changed recording."}else{"Entwürfe bewahren noch nicht übernommene Eingaben und ungespeicherte externe Projekte. Wiederherstellung überschreibt keine zwischenzeitlich geänderte Aufnahme."});
                for warning in &self.daily.recovery_warnings {ui.colored_label(ui.visuals().error_fg_color,warning);}
                if self.daily.recoveries.is_empty(){ui.label(if en{"No recovery drafts."}else{"Keine wiederherzustellenden Entwürfe."});}
                egui::ScrollArea::vertical().max_height(350.).show(ui,|ui|{for (id,draft) in &self.daily.recoveries {ui.horizontal(|ui|{if ui.add_enabled(!self.busy,egui::Button::new(format!("{} · {}",if en{"Recover"}else{"Wiederherstellen"},draft.title))).clicked(){restore=Some((id.clone(),draft.clone()));}ui.small(draft.source.to_string_lossy());});}});
                if let Some(store)=&self.daily.store {ui.small(format!("{}: {}",if en{"Local folder"}else{"Lokale Ablage"},store.root.display()));}
            });
            self.daily.recovery_open = open;
            if let Some((id, draft)) = restore {
                self.restore_draft(id, draft);
            }
        }
        let mut command = None;
        if let Some((id, mut offset, mut at)) = self.daily.split.clone() {
            if let Some(segment) = self
                .view
                .as_ref()
                .and_then(|r| r.segments.iter().find(|s| s.id == id))
            {
                let mut open = true;
                egui::Window::new(if en{"Split segment"}else{"Abschnitt teilen"}).open(&mut open).default_width(600.).show(ctx,|ui|{
                    let boundaries=laut_core::text_boundaries(&segment.text);
                    let max=boundaries.len().saturating_sub(2);offset=offset.clamp(1,max.max(1));
                    ui.label(if en{"Choose the text boundary and the corresponding time. New parts retain their original evidence, without invented word timings."}else{"Textgrenze und passenden Zeitpunkt wählen. Die Originaldaten bleiben erhalten; Wortzeiten werden nicht erfunden."});
                    ui.add(egui::Slider::new(&mut offset,1..=max.max(1)).text(if en{"Text boundary"}else{"Textgrenze"}));
                    let b=boundaries.get(offset).copied().unwrap_or(segment.text.len());ui.label(format!("{} | {}",&segment.text[..b],&segment.text[b..]));
                    ui.horizontal(|ui|{ui.label(if en{"Time in seconds"}else{"Zeitpunkt in Sekunden"});ui.add(egui::DragValue::new(&mut at).speed(0.1));});
                    ui.label(format!("{:.2}–{:.2} s",segment.start,segment.end));
                    if ui.add_enabled(!self.busy&&at>segment.start&&at<segment.end&&max>0,egui::Button::new(if en{"Split"}else{"Teilen"})).clicked(){command=Some(Command::SplitSegment{id:id.clone(),at,offset});}
                });
                self.daily.split = if open { Some((id, offset, at)) } else { None };
            } else {
                self.daily.split = None;
            }
        }
        if let Some(id) = self.daily.merge.clone() {
            let mut open = true;
            egui::Window::new(if en {
                "Merge segments"
            } else {
                "Abschnitte verbinden"
            })
            .open(&mut open)
            .show(ctx, |ui| {
                if let Some(r) = &self.view
                    && let Some(i) = r.segments.iter().position(|s| s.id == id)
                    && let Some(next) = r.segments.get(i + 1)
                {
                    ui.label(format!("{}\n+\n{}", r.segments[i].text, next.text));
                    ui.label(format!(
                        "{}: {}",
                        if en {
                            "Speaker for both parts"
                        } else {
                            "Person für beide Teile"
                        },
                        r.speaker_name(r.segments[i].speaker_id.as_deref())
                    ));
                    if ui
                        .add_enabled(
                            !self.busy,
                            egui::Button::new(if en { "Merge" } else { "Verbinden" }),
                        )
                        .clicked()
                    {
                        command = Some(Command::MergeNext { id: id.clone() });
                    }
                }
            });
            if !open {
                self.daily.merge = None;
            }
        }
        if self.daily.replace_open {
            let mut open = true;
            egui::Window::new(if en{"Find and replace"}else{"Suchen und ersetzen"}).open(&mut open).show(ctx,|ui|{
                ui.label(if en{"Exact spelling, case-sensitive. Applies to every original segment, including hidden exclusions. Original recognition stays intact. One undo restores all replacements."}else{"Exakte Schreibweise mit Groß-/Kleinschreibung. Gilt für alle Abschnitte, einschließlich ausgeblendeter Einspieler. Der erkannte Originaltext bleibt erhalten. Ein Rückgängig stellt alles wieder her."});
                ui.label(if en{"Find"}else{"Suchen"});ui.text_edit_singleline(&mut self.daily.find);
                ui.label(if en{"Replace with"}else{"Ersetzen durch"});ui.text_edit_singleline(&mut self.daily.replacement);
                let count=self.view.as_ref().map(|r|r.segments.iter().map(|s|if self.daily.find.is_empty(){0}else{s.text.matches(&self.daily.find).count()}).sum::<usize>()).unwrap_or(0);
                ui.label(format!("{count} {}",if en{"occurrences"}else{"Vorkommen"}));
                if self.pending_fields(){ui.label(if en{"Apply your drafts before replacing text."}else{"Vor dem Ersetzen bitte Entwürfe übernehmen."});}
                if ui.add_enabled(!self.busy&&count>0&&!self.pending_fields(),egui::Button::new(if en{"Replace all"}else{"Alle ersetzen"})).clicked(){command=Some(Command::ReplaceText{find:self.daily.find.clone(),replacement:self.daily.replacement.clone()});}
            });
            self.daily.replace_open = open;
        }
        if self.daily.merge_speakers_open {
            let mut open = true;
            egui::Window::new(if en{"Merge speakers"}else{"Personen zusammenführen"}).open(&mut open).show(ctx,|ui|{
                if let Some(r)=&self.view {
                    for (salt,label,value) in [("from",if en{"Replace person"}else{"Diese Person ersetzen"},&mut self.daily.merge_from),("into",if en{"Use this person"}else{"Mit dieser Person"},&mut self.daily.merge_into)] {
                        ui.label(label);egui::ComboBox::from_id_salt(salt).selected_text(r.speaker_name(Some(value))).show_ui(ui,|ui|{for s in &r.speakers {ui.selectable_value(value,s.id.clone(),&s.name);}});
                    }
                    ui.label(if en{"All affected segments will be assigned to the destination person and protected as manual assignments. Undo remains available."}else{"Alle betroffenen Abschnitte werden der Zielperson zugeordnet und als manuelle Zuordnung geschützt. Rückgängig bleibt verfügbar."});
                    if ui.add_enabled(!self.busy&&!self.daily.merge_from.is_empty()&&!self.daily.merge_into.is_empty()&&self.daily.merge_from!=self.daily.merge_into,egui::Button::new(if en{"Merge people"}else{"Personen zusammenführen"})).clicked(){command=Some(Command::MergeSpeakers{from:self.daily.merge_from.clone(),into:self.daily.merge_into.clone()});}
                }
            });
            self.daily.merge_speakers_open = open;
        }
        if let Some(command) = command {
            self.command(command);
            self.daily.split = None;
            self.daily.merge = None;
            self.daily.replace_open = false;
            self.daily.merge_speakers_open = false;
        }
        // Native text controls keep their own undo; document shortcuts only outside text input.
        if !ctx.egui_wants_keyboard_input() && !self.busy && !self.storage.open {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) {
                self.command(Command::Undo);
            }
            if ctx.input_mut(|i| {
                i.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::Z,
                )
            }) {
                self.command(Command::Redo);
            }
        }
    }
}

impl LautApp {
    pub(super) fn draft_status(&mut self, ui: &mut egui::Ui) {
        self.remember_editor();
        if self.pending_fields() {
            ui.label(self.l(
                "• Eingaben noch nicht übernommen",
                "• Inputs not applied yet",
            ));
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new(self.l("Alle Eingaben übernehmen", "Apply all inputs")),
                )
                .clicked()
            {
                self.command(Command::ApplyDraft {
                    title: self.daily.title.clone(),
                    notes: self.daily.notes.clone(),
                    texts: self.daily.texts.clone(),
                    names: self.speaker_names.iter().cloned().collect(),
                });
            }
        }
    }
    pub(super) fn refresh_catalog(&mut self, ctx: &egui::Context) {
        if self.busy || self.library_root.is_empty() {
            return;
        }
        let root = PathBuf::from(&self.library_root);
        if !root.join("laut-library.json").exists() {
            return;
        }
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        self.busy = true;
        std::thread::spawn(move || {
            let result = Library::open(&root)
                .and_then(|library| library.catalog().map(|catalog| (library, catalog)))
                .map_err(|e| e.to_string());
            let _ = tx.send(Event::Catalog(result));
            ctx.request_repaint();
        });
    }
}
