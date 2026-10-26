//! Familiar desktop navigation; processing and storage stay in the shared Rust engine.
use super::*;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Section {
    #[default]
    Library,
    Sources,
    Search,
    Models,
    Settings,
    Speakers,
}
#[derive(Default)]
pub(super) struct ShellState {
    pub section: Section,
    pub history_query: String,
    pub catalog_loaded: bool,
    pub selected_collection: Option<String>,
}
impl LautApp {
    pub(super) fn show_section(&mut self, section: Section, ctx: &egui::Context) {
        self.shell.section = section;
        match section {
            Section::Sources => {
                self.processing.tab = 0;
                self.refresh_jobs();
            }
            Section::Models => self.processing.show_models(),
            Section::Search => self.research.reload_reports(),
            Section::Speakers => self.processing.tab = 3,
            Section::Library => self.refresh_catalog(ctx),
            Section::Settings => {}
        }
    }
    pub(super) fn app_sidebar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(8.);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(34., 34.), egui::Sense::hover());
            let accent = egui::Color32::from_rgb(0, 143, 136);
            ui.painter().rect_filled(rect, 9., accent);
            for (i, h) in [9., 16., 23., 14., 7.].iter().enumerate() {
                let x = rect.left() + 8. + i as f32 * 4.5;
                ui.painter().line_segment(
                    [
                        egui::pos2(x, rect.center().y - h / 2.),
                        egui::pos2(x, rect.center().y + h / 2.),
                    ],
                    egui::Stroke::new(2., egui::Color32::WHITE),
                );
            }
            ui.label(RichText::new("Laut").size(19.).strong());
        });
        ui.add_space(11.);
        let mut target = None;
        for (section, de, en) in [
            (Section::Library, "Bibliothek", "Library"),
            (Section::Sources, "Quellen & Aufträge", "Sources & jobs"),
            (Section::Search, "Suche & KI", "Search & AI"),
            (Section::Models, "Modelle", "Models"),
            (Section::Settings, "Einstellungen", "Settings"),
        ] {
            let active = self.shell.section == section
                || (section == Section::Library && self.shell.section == Section::Speakers);
            if navigation_row(ui, section, self.l(de, en), active).clicked() {
                target = Some(section);
            }
        }
        if let Some(section) = target {
            self.show_section(section, ctx);
        }
        ui.add_space(12.);
        ui.separator();
        ui.add_space(8.);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(self.l("SAMMLUNGEN", "COLLECTIONS"))
                    .size(10.)
                    .strong()
                    .weak(),
            );
            if ui
                .small_button(self.l("+ Verwalten", "+ Manage"))
                .on_hover_text(self.l(
                    "Sammlung anlegen oder bearbeiten",
                    "Create or edit a collection",
                ))
                .clicked()
            {
                self.collection_open = true;
            }
        });
        let all_active = self.shell.selected_collection.is_none();
        if collection_row(
            ui,
            self.l("Alle Aufnahmen", "All recordings"),
            self.library_entries.len(),
            all_active,
            false,
        )
        .clicked()
        {
            self.shell.selected_collection = None;
            self.research.scope = "library".into();
        }
        let mut choose_collection = None;
        for collection in &self.collections {
            let available = collection
                .recording_ids
                .iter()
                .filter(|id| self.library_entries.iter().any(|entry| &entry.id == *id))
                .count();
            let selected = self.shell.selected_collection.as_ref() == Some(&collection.id);
            if collection_row(ui, &collection.name, available, selected, true).clicked() {
                choose_collection = Some(collection.id.clone());
            }
        }
        if let Some(id) = choose_collection {
            self.shell.selected_collection = Some(id.clone());
            self.research.scope = id;
        }
        if self.collections.is_empty() {
            ui.weak(self.l(
                "Noch keine Sammlung · Termine und Themen lassen sich bündeln.",
                "No collection yet · group events and topics here.",
            ));
        }
        ui.add_space(8.);
        ui.separator();
        ui.add_space(8.);
        ui.label(
            RichText::new(self.l("AUFNAHMEN", "RECORDINGS"))
                .size(10.)
                .strong()
                .weak(),
        );
        ui.add_space(4.);
        ui.add(
            egui::TextEdit::singleline(&mut self.shell.history_query)
                .desired_width(f32::INFINITY)
                .hint_text(if self.english {
                    "Recordings…"
                } else {
                    "Aufnahmen suchen …"
                }),
        );
        ui.add_space(6.);
        let mut selected = None;
        let query = self.shell.history_query.to_lowercase();
        let current = self.view.as_ref().map(|r| r.id.as_str());
        let selected_recordings = self
            .shell
            .selected_collection
            .as_ref()
            .and_then(|id| {
                self.collections
                    .iter()
                    .find(|collection| &collection.id == id)
            })
            .map(|collection| collection.recording_ids.clone());
        egui::ScrollArea::vertical()
            .id_salt("sidebar-history")
            .max_height((ui.available_height() - 42.).max(80.))
            .show(ui, |ui| {
                for entry in self.library_entries.iter().filter(|entry| {
                    selected_recordings
                        .as_ref()
                        .is_none_or(|ids| ids.contains(&entry.id))
                        && entry.title.to_lowercase().contains(&query)
                }) {
                    let label = format!("{}\n{}", entry.title, timestamp(entry.duration));
                    let button = egui::Button::selectable(
                        current == Some(entry.id.as_str())
                            && self.shell.section == Section::Library,
                        "",
                    )
                    .left_text(RichText::new(label).size(12.))
                    .wrap()
                    .min_size(egui::vec2(ui.available_width(), 44.))
                    .corner_radius(7.);
                    if ui.add_enabled(!self.busy, button).clicked() {
                        selected = Some(entry.id.clone());
                    }
                    ui.add_space(3.);
                }
                if self.library_entries.is_empty() {
                    ui.weak(self.l("Noch keine Aufnahmen", "No recordings yet"));
                } else if selected_recordings.is_some()
                    && !self.library_entries.iter().any(|entry| {
                        selected_recordings
                            .as_ref()
                            .is_some_and(|ids| ids.contains(&entry.id))
                    })
                {
                    ui.weak(self.l(
                        "Keine aktive Aufnahme in dieser Sammlung",
                        "No active recording in this collection",
                    ));
                }
            });
        if let Some(id) = selected {
            self.load(
                PathBuf::from(&self.library_root)
                    .join(id.to_uppercase())
                    .join("recording.json"),
                ctx.clone(),
            );
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.add_space(8.);
            ui.label(
                RichText::new(self.l(
                    "Kein Konto. Deine Dateien bleiben lokal.",
                    "No account. Your files stay local.",
                ))
                .size(10.)
                .weak(),
            );
        });
    }
    pub(super) fn collection_dialog(&mut self, ctx: &egui::Context) {
        if !self.collection_open && self.collection_remove.is_none() {
            return;
        }
        let en = self.english;
        let t = |de, english| if en { english } else { de };
        let mut open = self.collection_open;
        let mut edit = None;
        let mut save = None;
        let mut request_remove = None;
        let mut cancel_edit = false;
        if self.collection_open {
            egui::Window::new(t("Sammlungen", "Collections"))
                .open(&mut open)
                .default_width(620.)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(t(
                        "Bündele mehrere Aufnahmen eines Termins oder Themenblocks. Sammlungen bleiben lokal, werden mit der Bibliothek gesichert und können als Such- und KI-Bereich verwendet werden.",
                        "Group recordings from one event or topic. Collections stay local, are included in library backups and can be used as a Search and AI scope.",
                    ));
                    ui.add_space(8.);
                    if let Some(collection) = &mut self.collection_editing {
                        ui.strong(t("Sammlung bearbeiten", "Edit collection"));
                        ui.add(
                            egui::TextEdit::singleline(&mut collection.name)
                                .desired_width(f32::INFINITY)
                                .char_limit(120)
                                .hint_text(t("Name, z. B. Telekom AI Investor Day", "Name, e.g. Telecom AI Investor Day")),
                        );
                        ui.label(t(
                            "Aufnahmen auswählen – ihre Dateien werden nicht verschoben oder dupliziert.",
                            "Select recordings – their files are neither moved nor duplicated.",
                        ));
                        egui::ScrollArea::vertical()
                            .id_salt("collection-members")
                            .max_height(330.)
                            .show(ui, |ui| {
                                for entry in &self.library_entries {
                                    let mut member = collection.recording_ids.contains(&entry.id);
                                    if ui
                                        .checkbox(
                                            &mut member,
                                            format!("{} · {}", entry.title, timestamp(entry.duration)),
                                        )
                                        .changed()
                                    {
                                        if member {
                                            collection.recording_ids.push(entry.id.clone());
                                        } else {
                                            collection.recording_ids.retain(|id| id != &entry.id);
                                        }
                                    }
                                }
                            });
                        collection.recording_ids.sort();
                        collection.recording_ids.dedup();
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(
                                    !collection.name.trim().is_empty()
                                        && !collection.recording_ids.is_empty(),
                                    egui::Button::new(t("Sammlung speichern", "Save collection")),
                                )
                                .clicked()
                            {
                                collection.name = collection.name.trim().to_owned();
                                save = Some(collection.clone());
                            }
                            if ui.button(t("Abbrechen", "Cancel")).clicked() {
                                cancel_edit = true;
                            }
                        });
                    } else {
                        if ui.button(t("+ Neue Sammlung", "+ New collection")).clicked() {
                            edit = Some(Collection::new(""));
                        }
                        for collection in &self.collections {
                            ui.group(|ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.strong(&collection.name);
                                    ui.label(format!(
                                        "{} {}",
                                        collection.recording_ids.len(),
                                        t("Aufnahmen", "recordings")
                                    ));
                                    if ui.button(t("Bearbeiten", "Edit")).clicked() {
                                        edit = Some(collection.clone());
                                    }
                                    if ui.small_button(t("Entfernen …", "Remove …")).clicked() {
                                        request_remove = Some(collection.id.clone());
                                    }
                                });
                            });
                        }
                    }
                });
        }
        if self.collection_open && !open {
            self.collection_editing = None;
        }
        self.collection_open = open;
        if let Some(collection) = edit {
            self.collection_editing = Some(collection);
            self.collection_open = true;
        }
        if cancel_edit {
            self.collection_editing = None;
        }
        if let Some(id) = request_remove {
            self.collection_remove = Some(id);
        }
        if let Some(collection) = save {
            let mut collections = self.collections.clone();
            if let Some(existing) = collections
                .iter_mut()
                .find(|existing| existing.id == collection.id)
            {
                *existing = collection.clone();
            } else {
                collections.push(collection.clone());
            }
            collections.sort_by_key(|collection| collection.name.to_lowercase());
            match Library::open(&PathBuf::from(&self.library_root))
                .and_then(|library| library.save_collections(&collections))
            {
                Ok(()) => {
                    self.collections = collections;
                    self.collection_editing = None;
                    self.shell.selected_collection = Some(collection.id.clone());
                    self.research.scope = collection.id;
                    self.status =
                        t("Sammlung lokal gespeichert.", "Collection saved locally.").into();
                    self.error = false;
                }
                Err(error) => {
                    self.error = true;
                    self.status = error.to_string();
                }
            }
        }
        if let Some(id) = self.collection_remove.clone() {
            egui::Window::new(t("Sammlung entfernen?", "Remove collection?"))
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label(t(
                        "Nur die Sammlung wird entfernt. Aufnahmen, Transkripte, PDFs und Ergebnisse bleiben erhalten.",
                        "Only the collection is removed. Recordings, transcripts, PDFs and results are retained.",
                    ));
                    if ui.button(t("Sammlung entfernen", "Remove collection")).clicked() {
                        let collections: Vec<_> = self
                            .collections
                            .iter()
                            .filter(|collection| collection.id != id)
                            .cloned()
                            .collect();
                        match Library::open(&PathBuf::from(&self.library_root))
                            .and_then(|library| library.save_collections(&collections))
                        {
                            Ok(()) => {
                                self.collections = collections;
                                if self.shell.selected_collection.as_ref() == Some(&id) {
                                    self.shell.selected_collection = None;
                                    self.research.scope = "library".into();
                                }
                                self.collection_remove = None;
                                self.status = t(
                                    "Sammlung entfernt; Aufnahmen bleiben erhalten.",
                                    "Collection removed; recordings retained.",
                                )
                                .into();
                                self.error = false;
                            }
                            Err(error) => {
                                self.error = true;
                                self.status = error.to_string();
                            }
                        }
                    }
                    if ui.button(t("Behalten", "Keep")).clicked() {
                        self.collection_remove = None;
                    }
                });
        }
    }
    pub(super) fn settings_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading(self.l("Einstellungen", "Settings"));
        ui.weak(format!("Laut {}", env!("CARGO_PKG_VERSION")));
        ui.add_space(16.);
        egui::ScrollArea::vertical().show(ui,|ui| {
            ui.strong(self.l("Darstellung","Appearance"));
            ui.add_space(8.);
            ui.horizontal(|ui| {
                ui.label(self.l("Sprache","Language"));
                ui.selectable_value(&mut self.english,false,"Deutsch");
                ui.selectable_value(&mut self.english,true,"English");
            });
            ui.horizontal(|ui| {
                ui.label(self.l("Farbschema","Appearance"));
                for (value,de,en) in [("system","System","System"),("light","Hell","Light"),("dark","Dunkel","Dark")] {
                    let label=self.l(de,en);
                    if ui.selectable_value(&mut self.daily.preferences.theme,value.to_owned(),label).changed() { appearance::theme(ctx,&self.daily.preferences.theme); }
                }
            });
            ui.horizontal(|ui| {
                ui.label(self.l("Schriftgröße im Transkript","Transcript text size"));
                for size in [14.,16.,18.,20.] { ui.selectable_value(&mut self.daily.preferences.reading_font_size,size,format!("{size:.0}")); }
            });
            let label=self.l("Text folgt der Wiedergabe","Follow playback");
            ui.checkbox(&mut self.daily.preferences.follow_playback,label);
            ui.add_space(20.); ui.separator(); ui.add_space(12.);
            ui.strong(self.l("Bibliothek & Sicherung","Library & backup"));
            ui.label(&self.library_root);
            if ui.add_enabled(!self.busy,egui::Button::new(self.l("Bibliothek verwalten …","Manage library …"))).clicked() { self.library_open=true; self.refresh_catalog(ctx); }
            self.storage_button(ui);
            if ui.button(self.l("Entwürfe wiederherstellen …","Recover drafts …")).clicked() { self.reload_drafts(); self.daily.recovery_open=true; }
            ui.add_space(20.);ui.separator();ui.add_space(12.);
            ui.strong(self.l("Verarbeitung","Processing"));
            ui.label(self.l("Sprachmodelle und Programme richtest du unter Modelle ein. Dateien und Links gehören zu Quellen & Aufträge.","Set up speech models and programs under Models. Add files and links under Sources & jobs."));
            if ui.button(self.l("Modelle öffnen","Open models")).clicked() { self.show_section(Section::Models,ctx); }
        });
    }
}
fn navigation_row(ui: &mut egui::Ui, section: Section, text: &str, active: bool) -> egui::Response {
    let response = ui.add(
        egui::Button::selectable(active, "")
            .left_text(format!("       {text}"))
            .min_size(egui::vec2(ui.available_width(), 32.))
            .corner_radius(7.),
    );
    let c = response.rect.left_center() + egui::vec2(17., 0.);
    let color = if active {
        ui.visuals().selection.stroke.color
    } else {
        ui.visuals().text_color()
    };
    let stroke = egui::Stroke::new(1.4, color);
    let p = ui.painter();
    match section {
        Section::Library => {
            for dx in [-5., 0., 5.] {
                p.line_segment([c + egui::vec2(dx, -6.), c + egui::vec2(dx, 6.)], stroke);
            }
        }
        Section::Sources => {
            p.line_segment([c + egui::vec2(0., -7.), c + egui::vec2(0., 3.)], stroke);
            p.line_segment([c + egui::vec2(-4., 0.), c + egui::vec2(0., 4.)], stroke);
            p.line_segment([c + egui::vec2(0., 4.), c + egui::vec2(4., 0.)], stroke);
            p.line_segment([c + egui::vec2(-7., 7.), c + egui::vec2(7., 7.)], stroke);
        }
        Section::Search => {
            p.circle_stroke(c - egui::vec2(2., 2.), 5., stroke);
            p.line_segment([c + egui::vec2(2., 2.), c + egui::vec2(7., 7.)], stroke);
        }
        Section::Models => {
            p.rect_stroke(
                egui::Rect::from_center_size(c, egui::vec2(11., 11.)),
                2.,
                stroke,
                egui::StrokeKind::Inside,
            );
            for d in [-3., 3.] {
                p.line_segment([c + egui::vec2(d, -9.), c + egui::vec2(d, -5.)], stroke);
                p.line_segment([c + egui::vec2(d, 5.), c + egui::vec2(d, 9.)], stroke);
            }
        }
        _ => {
            for (y, x) in [(-5., -3.), (0., 3.), (5., -1.)] {
                p.line_segment([c + egui::vec2(-7., y), c + egui::vec2(7., y)], stroke);
                p.circle_filled(c + egui::vec2(x, y), 2.5, color);
            }
        }
    }
    response
}

fn collection_row(
    ui: &mut egui::Ui,
    name: &str,
    count: usize,
    active: bool,
    folder: bool,
) -> egui::Response {
    let response = ui.add(
        egui::Button::selectable(active, "")
            .left_text(format!("       {name}  ({count})"))
            .min_size(egui::vec2(ui.available_width(), 31.))
            .corner_radius(7.),
    );
    let center = response.rect.left_center() + egui::vec2(17., 0.);
    let color = if active {
        ui.visuals().selection.stroke.color
    } else {
        ui.visuals().weak_text_color()
    };
    let stroke = egui::Stroke::new(1.25, color);
    let painter = ui.painter();
    if folder {
        let body = egui::Rect::from_center_size(center + egui::vec2(0., 1.5), egui::vec2(14., 10.));
        painter.rect_stroke(body, 2., stroke, egui::StrokeKind::Inside);
        painter.line_segment(
            [center + egui::vec2(-6., -4.), center + egui::vec2(-2., -7.)],
            stroke,
        );
        painter.line_segment(
            [center + egui::vec2(-2., -7.), center + egui::vec2(3., -7.)],
            stroke,
        );
    } else {
        painter.rect_stroke(
            egui::Rect::from_center_size(center, egui::vec2(13., 12.)),
            3.,
            stroke,
            egui::StrokeKind::Inside,
        );
        painter.line_segment(
            [center + egui::vec2(-3., -3.), center + egui::vec2(4., -3.)],
            stroke,
        );
        painter.line_segment(
            [center + egui::vec2(-3., 1.), center + egui::vec2(4., 1.)],
            stroke,
        );
    }
    response
}
