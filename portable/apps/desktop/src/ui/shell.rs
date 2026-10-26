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
        ui.add_space(12.);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(30., 30.), egui::Sense::hover());
            let accent = egui::Color32::from_rgb(0, 128, 128);
            ui.painter().rect_filled(rect, 8., accent);
            for (i, h) in [7., 14., 21., 11., 5.].iter().enumerate() {
                let x = rect.left() + 7. + i as f32 * 4.;
                ui.painter().line_segment(
                    [
                        egui::pos2(x, rect.center().y - h / 2.),
                        egui::pos2(x, rect.center().y + h / 2.),
                    ],
                    egui::Stroke::new(2., egui::Color32::WHITE),
                );
            }
            ui.vertical(|ui| {
                ui.label(RichText::new("Laut").size(19.).strong());
                ui.label(
                    RichText::new(self.l("Deine Stimme. Dein Text.", "Your voice. Your words."))
                        .size(11.)
                        .weak(),
                );
            });
        });
        ui.add_space(20.);
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
        ui.add_space(18.);
        ui.separator();
        ui.add_space(10.);
        ui.label(
            RichText::new(self.l("VERLAUF", "RECENT"))
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
        egui::ScrollArea::vertical()
            .id_salt("sidebar-history")
            .max_height((ui.available_height() - 42.).max(80.))
            .show(ui, |ui| {
                for entry in self
                    .library_entries
                    .iter()
                    .filter(|e| e.title.to_lowercase().contains(&query))
                {
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
    pub(super) fn settings_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading(self.l("Einstellungen", "Settings"));
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
            ui.label(self.l("Sprachmodelle und Programme richtest du unter Modelle ein. Dateien und YouTube-Links gehören zu Quellen & Aufträge.","Set up speech models and programs under Models. Add files and YouTube links under Sources & jobs."));
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
