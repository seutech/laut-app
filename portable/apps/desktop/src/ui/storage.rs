use super::*;
use laut_engine::library_storage::{Storage, StoredProject};

#[derive(Default)]
pub(super) struct StorageState {
    pub open: bool,
    root: PathBuf,
    report: Option<Storage>,
    refresh: bool,
    trash_tab: bool,
    pending: Option<StoredProject>,
    filter: String,
}
pub(super) enum StorageEvent {
    Loaded(PathBuf, Result<(PathBuf, Storage), String>),
    Moved(PathBuf, String, Result<String, String>),
    Restored(PathBuf, Result<PathBuf, String>),
}

fn bytes(value: u64) -> String {
    if value >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", value as f64 / (1024. * 1024. * 1024.))
    } else if value >= 1024 * 1024 {
        format!("{:.1} MiB", value as f64 / (1024. * 1024.))
    } else if value >= 1024 {
        format!("{:.1} KiB", value as f64 / 1024.)
    } else {
        format!("{value} B")
    }
}

impl LautApp {
    pub(super) fn storage_button(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new(self.l("Speicher & Papierkorb …", "Storage & trash …")),
            )
            .clicked()
        {
            self.storage = StorageState {
                open: true,
                root: self.library_root.clone().into(),
                refresh: true,
                ..Default::default()
            };
            self.library_open = false;
        }
    }
    pub(super) fn storage_event(&mut self, event: StorageEvent, ctx: &egui::Context) {
        self.busy = false;
        match event {
            StorageEvent::Loaded(root, result) => {
                if root != self.storage.root {
                    return;
                }
                match result {
                    Ok((canonical_root, report)) => {
                        self.storage.root = canonical_root;
                        self.storage.report = Some(report);
                        self.error = false;
                        self.status = self
                            .l("Speicherübersicht aktualisiert", "Storage overview updated")
                            .into();
                    }
                    Err(error) => {
                        self.error = true;
                        self.status = error;
                    }
                }
            }
            StorageEvent::Moved(root, id, result) => match result {
                Ok(_) => {
                    if self
                        .managed
                        .as_ref()
                        .is_some_and(|(lib, _)| lib.root == root)
                        && self.view.as_ref().is_some_and(|r| r.id == id)
                    {
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
                    }
                    self.library_entries.retain(|e| e.id != id);
                    self.daily.hits.clear();
                    self.daily.searched = false;
                    self.research.search = None;
                    self.storage.report = None;
                    self.storage.refresh = true;
                    self.storage.trash_tab = true;
                    self.storage.filter.clear();
                    self.error = false;
                    self.status = self
                        .l(
                            "Im lokalen Papierkorb · jederzeit wiederherstellbar",
                            "In local trash · can be restored",
                        )
                        .into();
                }
                Err(error) => {
                    self.error = true;
                    self.status = error;
                }
            },
            StorageEvent::Restored(root, result) => match result {
                Ok(path) => {
                    self.storage.report = None;
                    self.storage.open = false;
                    self.daily.hits.clear();
                    self.daily.searched = false;
                    self.research.search = None;
                    self.library_root = root.to_string_lossy().into_owned();
                    self.shell.catalog_loaded = false;
                    self.load(path, ctx.clone());
                }
                Err(error) => {
                    self.error = true;
                    self.status = error;
                }
            },
        }
    }
    pub(super) fn storage_dialog(&mut self, ctx: &egui::Context) {
        if !self.storage.open {
            return;
        }
        if self.storage.refresh && !self.busy {
            self.storage.refresh = false;
            let root = self.storage.root.clone();
            let tx = self.tx.clone();
            let ctx = ctx.clone();
            self.busy = true;
            self.status = self
                .l("Dateigrößen werden ermittelt …", "Measuring file sizes …")
                .into();
            std::thread::spawn(move || {
                let result = Library::open(&root)
                    .and_then(|lib| lib.storage().map(|report| (lib.root, report)))
                    .map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::Storage(StorageEvent::Loaded(root, result)));
                ctx.request_repaint();
            });
        }
        let en = self.english;
        let t = |de, en_text| if en { en_text } else { de };
        let mut open = true;
        let mut restore = None;
        egui::Window::new(t("Speicher & Papierkorb", "Storage & trash"))
            .open(&mut open).default_width(700.).show(ctx, |ui| {
            ui.small(self.storage.root.display().to_string());
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.storage.trash_tab, false, t("Aufnahmen", "Recordings"));
                let trash_count = self.storage.report.as_ref().map_or(0, |r|r.trash.len());
                ui.selectable_value(&mut self.storage.trash_tab, true, format!("{} ({trash_count})", t("Papierkorb", "Trash")));
                if ui.add_enabled(!self.busy, egui::Button::new(t("Aktualisieren", "Refresh"))).clicked() { self.storage.refresh = true; }
            });
            if let Some(report) = &self.storage.report {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("{}: {}",t("Aufnahmen", "Recordings"),bytes(report.active_bytes)));
                    ui.label(format!("{}: {}",t("Papierkorb", "Trash"),bytes(report.trash_bytes)));
                    ui.label(format!("{}: {}",t("Sonstige Dateien", "Other files"),bytes(report.other_bytes)));
                });
                ui.small(format!("{}: {}", t("Auf dem Laufwerk verfügbar", "Available on this drive"), bytes(report.available_bytes)));
                ui.small(t("Dateigrößen dieser Bibliothek einschließlich Audio und PDFs. Modelle und Arbeitsdateien außerhalb der Bibliothek sind nicht enthalten.", "File sizes in this library, including audio and PDFs. Models and working files outside the library are not included."));
                if !report.warnings.is_empty() {
                    ui.collapsing(t("Erfassung unvollständig – Hinweise prüfen", "Scan incomplete – review warnings"), |ui| {
                        for warning in &report.warnings { ui.colored_label(ui.visuals().warn_fg_color, warning); }
                    });
                }
                ui.separator();
                ui.add(egui::TextEdit::singleline(&mut self.storage.filter).hint_text(t("Titel suchen …", "Filter by title …")).desired_width(f32::INFINITY));
                let query = self.storage.filter.trim().to_lowercase();
                egui::ScrollArea::vertical().id_salt("storage-list").max_height(340.).show(ui, |ui| {
                    if self.storage.trash_tab {
                        if report.trash.is_empty() { ui.label(t("Der Papierkorb ist leer.", "Trash is empty.")); }
                        for entry in report.trash.iter().filter(|e| e.title.to_lowercase().contains(&query)) {
                            ui.push_id(&entry.token, |ui| {
                                ui.horizontal(|ui| {
                                    ui.allocate_ui_with_layout(egui::vec2((ui.available_width()-165.).max(120.),0.),egui::Layout::top_down(egui::Align::Min),|ui| {ui.strong(&entry.title); ui.small(bytes(entry.bytes));
                                        if entry.recovered {ui.small(t("Vorherige gültige Fassung – nach Wiederherstellung prüfen", "Previous valid version – review after restoring"));}
                                    });
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.add_enabled(!self.busy,egui::Button::new(t("Wiederherstellen", "Restore"))).clicked() {restore=Some(entry.token.clone());}
                                    });
                                }); ui.separator();
                            });
                        }
                    } else {
                        if report.projects.is_empty() {ui.label(t("Keine aktiven Aufnahmen.", "No active recordings."));}
                        for entry in report.projects.iter().filter(|e| e.title.to_lowercase().contains(&query)) {
                            ui.push_id(&entry.id, |ui| {
                                ui.horizontal(|ui| {
                                    ui.allocate_ui_with_layout(egui::vec2((ui.available_width()-165.).max(120.),0.),egui::Layout::top_down(egui::Align::Min),|ui| {ui.strong(&entry.title);ui.small(bytes(entry.bytes));});
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.add_enabled(!self.busy,egui::Button::new(t("In Papierkorb …", "Move to trash …"))).clicked() {self.storage.pending=Some(entry.clone());}
                                    });
                                }); ui.separator();
                            });
                        }
                    }
                });
                ui.small(t("Der Papierkorb bleibt lokal und wird mitgesichert. Verschieben gibt noch keinen Speicher frei. Es werden keine Dateien endgültig gelöscht.", "Trash stays local and is included in backups. Moving an item does not free disk space. No files are permanently deleted."));
            } else if self.busy {ui.spinner();}
        });
        self.storage.open = open;
        if !open {
            self.storage.pending = None;
            return;
        }
        if let Some(project) = self.storage.pending.clone() {
            let mut confirm = false;
            let mut cancel = false;
            egui::Window::new(t("Aufnahme in Papierkorb verschieben?", "Move recording to trash?"))
                .collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.strong(&project.title);
                    ui.label(t("Transkript, Audio, PDFs und Verlauf bleiben zusammen erhalten. Du kannst die Aufnahme hier wiederherstellen.", "Transcript, audio, PDFs and history stay together. You can restore the recording here."));
                    ui.horizontal(|ui| {
                        confirm=ui.add_enabled(!self.busy,egui::Button::new(t("In Papierkorb verschieben", "Move to trash"))).clicked();
                        cancel=ui.button(t("Abbrechen", "Cancel")).clicked();
                    });
                });
            if cancel {
                self.storage.pending = None;
            }
            if confirm {
                self.storage.pending = None;
                self.remember_editor();
                let current = self
                    .managed
                    .as_ref()
                    .is_some_and(|(lib, _)| lib.root == self.storage.root)
                    && self.view.as_ref().is_some_and(|r| r.id == project.id);
                if current
                    && (self.pending_fields() || self.session.as_ref().is_some_and(|s| s.dirty))
                {
                    self.error = true;
                    self.status=t("Bitte offene Eingaben zuerst übernehmen und speichern. Die Aufnahme wurde nicht verschoben.", "Apply and save pending inputs first. The recording was not moved.").into();
                } else if self.save_recovery() {
                    if current {
                        self.player = None;
                    }
                    let root = self.storage.root.clone();
                    let tx = self.tx.clone();
                    let ctx = ctx.clone();
                    self.busy = true;
                    std::thread::spawn(move || {
                        let result = Library::open(&root)
                            .and_then(|lib| lib.move_to_trash(&project.id, &project.revision))
                            .map_err(|e| format!("{e:#}"));
                        let _ = tx.send(Event::Storage(StorageEvent::Moved(
                            root, project.id, result,
                        )));
                        ctx.request_repaint();
                    });
                }
            }
        }
        if let Some(token) = restore {
            let root = self.storage.root.clone();
            let tx = self.tx.clone();
            let ctx = ctx.clone();
            self.busy = true;
            std::thread::spawn(move || {
                let result = Library::open(&root)
                    .and_then(|lib| lib.restore_from_trash(&token))
                    .map(|l| l.session.source)
                    .map_err(|e| format!("{e:#}"));
                let _ = tx.send(Event::Storage(StorageEvent::Restored(root, result)));
                ctx.request_repaint();
            });
        }
    }
}
