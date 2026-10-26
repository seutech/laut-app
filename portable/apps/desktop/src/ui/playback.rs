use super::*;
use std::{ops::Range, path::Path};

#[derive(Clone, Debug)]
pub(super) struct ReadingRange {
    pub bytes: Range<usize>,
    pub characters: Range<usize>,
    pub start: f64,
    pub end: f64,
}

#[derive(Default)]
pub(super) struct PlaybackState {
    pub ranges: Vec<ReadingRange>,
    pub last_followed: Option<usize>,
    pub scroll_requested: bool,
    jump: String,
    pub waveform_source: Option<PathBuf>,
    pub waveform: Option<crate::waveform::Waveform>,
    pub waveform_loading: bool,
    pub waveform_error: Option<String>,
    pub waveform_zoom: f64,
    pub waveform_start: f64,
    pub waveform_cancel: Arc<AtomicBool>,
}

// These ranges follow the same included segments and separators as the copyable reading text.
pub(super) fn reading_ranges(r: &Recording) -> Vec<ReadingRange> {
    let mut out = Vec::new();
    let (mut bytes, mut characters) = (0, 0);
    for (pi, p) in paragraphs(r).iter().enumerate() {
        if pi > 0 {
            bytes += 2;
            characters += 2;
        }
        for (si, s) in p.segments.iter().enumerate() {
            if si > 0 {
                bytes += 1;
                characters += 1;
            }
            let end_bytes = bytes + s.text.len();
            let end_chars = characters + s.text.chars().count();
            out.push(ReadingRange {
                bytes: bytes..end_bytes,
                characters: characters..end_chars,
                start: s.start,
                end: s.end,
            });
            bytes = end_bytes;
            characters = end_chars;
        }
    }
    out
}

fn active_range(ranges: &[ReadingRange], at: f64) -> Option<usize> {
    ranges
        .iter()
        .enumerate()
        .filter(|(_, r)| r.start <= at && at < r.end)
        .max_by(|(_, a), (_, b)| a.start.total_cmp(&b.start))
        .map(|(i, _)| i)
}

fn scroll_anchor(ranges: &[ReadingRange], at: f64) -> Option<usize> {
    active_range(ranges, at)
        .or_else(|| {
            ranges
                .iter()
                .enumerate()
                .filter(|(_, r)| r.start > at)
                .min_by(|(_, a), (_, b)| a.start.total_cmp(&b.start))
                .map(|(i, _)| i)
        })
        .or_else(|| {
            ranges
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.start.total_cmp(&b.start))
                .map(|(i, _)| i)
        })
}

fn adjacent_time(ranges: &[ReadingRange], at: f64, forward: bool) -> Option<f64> {
    if forward {
        ranges
            .iter()
            .filter(|r| r.start > at + 0.05)
            .map(|r| r.start)
            .min_by(f64::total_cmp)
    } else {
        // A press within the first two seconds goes to the previous passage.
        let limit = active_range(ranges, at)
            .map(|i| {
                if at - ranges[i].start < 2. {
                    ranges[i].start - 0.05
                } else {
                    at
                }
            })
            .unwrap_or(at);
        ranges
            .iter()
            .filter(|r| r.start <= limit)
            .map(|r| r.start)
            .max_by(f64::total_cmp)
    }
}

fn parse_time(value: &str) -> Option<f64> {
    let fields: Vec<_> = value.trim().split(':').collect();
    if fields.is_empty() || fields.len() > 3 {
        return None;
    }
    let mut total = 0.;
    for (i, field) in fields.iter().enumerate() {
        let last = i + 1 == fields.len();
        if field.is_empty()
            || !field
                .chars()
                .all(|c| c.is_ascii_digit() || (last && c == '.'))
        {
            return None;
        }
        let part: f64 = field.parse().ok()?;
        if !part.is_finite() || part < 0. || (i > 0 && part >= 60.) {
            return None;
        }
        total = total * 60. + part;
    }
    total.is_finite().then_some(total)
}

fn passage_button(ui: &mut egui::Ui, enabled: bool, glyph: &str, label: &str) -> bool {
    let response = ui
        .add_enabled(enabled, egui::Button::new(glyph))
        .on_hover_text(label);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, response.enabled(), label)
    });
    response.clicked()
}

impl LautApp {
    pub(super) fn waveform_view(&mut self, ui: &mut egui::Ui) {
        let en = self.english;
        let accent = if ui.visuals().dark_mode {
            egui::Color32::from_rgb(84, 190, 190)
        } else {
            egui::Color32::from_rgb(0, 128, 128)
        };
        let fill = if ui.visuals().dark_mode {
            egui::Color32::from_rgb(31, 52, 55)
        } else {
            egui::Color32::from_rgb(234, 246, 246)
        };
        let mut seek = None;
        let mut zoom_change = None;
        egui::Frame::new()
            .fill(fill)
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(12, 9))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("Audio");
                    ui.small(if en {
                        "Click or drag to seek"
                    } else {
                        "Klicken oder ziehen zum Spulen"
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button("+")
                            .on_hover_text(if en {
                                "Zoom in"
                            } else {
                                "Wellenform vergrößern"
                            })
                            .clicked()
                        {
                            zoom_change = Some((self.playback.waveform_zoom.max(1.) * 2.).min(16.));
                        }
                        ui.monospace(format!("{}×", self.playback.waveform_zoom.max(1.) as u32));
                        if ui
                            .add_enabled(
                                self.playback.waveform_zoom > 1.,
                                egui::Button::new("−").small(),
                            )
                            .on_hover_text(if en {
                                "Zoom out"
                            } else {
                                "Wellenform verkleinern"
                            })
                            .clicked()
                        {
                            zoom_change = Some((self.playback.waveform_zoom / 2.).max(1.));
                        }
                    });
                });
                if let Some(waveform) = &self.playback.waveform {
                    let zoom = self.playback.waveform_zoom.max(1.);
                    let span = waveform.duration / zoom;
                    let max_start = (waveform.duration - span).max(0.);
                    self.playback.waveform_start =
                        self.playback.waveform_start.clamp(0., max_start);
                    if zoom > 1.
                        && (self.seek < self.playback.waveform_start
                            || self.seek > self.playback.waveform_start + span)
                    {
                        self.playback.waveform_start = (self.seek - span / 2.).clamp(0., max_start);
                    }
                    let start = self.playback.waveform_start;
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 52.),
                        egui::Sense::click_and_drag(),
                    );
                    let bars = (rect.width() / 3.).max(1.) as usize;
                    let total = waveform.peaks.len();
                    let from_peak = ((start / waveform.duration) * total as f64) as usize;
                    let to_peak =
                        (((start + span) / waveform.duration) * total as f64).ceil() as usize;
                    let visible = &waveform.peaks
                        [from_peak.min(total - 1)..to_peak.clamp(from_peak + 1, total)];
                    let scale = visible.iter().copied().fold(0.02_f32, f32::max);
                    for bar in 0..bars {
                        let from = bar * visible.len() / bars;
                        let to = ((bar + 1) * visible.len() / bars)
                            .max(from + 1)
                            .min(visible.len());
                        let peak = visible[from..to].iter().copied().fold(0., f32::max);
                        let height = (peak / scale * (rect.height() - 6.)).max(1.5);
                        let at = start + bar as f64 / bars as f64 * span;
                        let color = if at <= self.seek {
                            accent
                        } else {
                            ui.visuals().weak_text_color()
                        };
                        let x = rect.left() + bar as f32 / bars as f32 * rect.width();
                        ui.painter().rect_filled(
                            egui::Rect::from_center_size(
                                egui::pos2(x, rect.center().y),
                                egui::vec2(1.5, height),
                            ),
                            1.,
                            color,
                        );
                    }
                    if self.seek >= start && self.seek <= start + span {
                        let x = rect.left() + ((self.seek - start) / span) as f32 * rect.width();
                        ui.painter().line_segment(
                            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                            egui::Stroke::new(1.5, ui.visuals().text_color()),
                        );
                    }
                    if (response.clicked() || response.dragged())
                        && let Some(pointer) = response.interact_pointer_pos()
                    {
                        seek = Some(
                            (start
                                + f64::from(
                                    ((pointer.x - rect.left()) / rect.width()).clamp(0., 1.),
                                ) * span)
                                .clamp(0., waveform.duration),
                        );
                    }
                    ui.horizontal(|ui| {
                        ui.monospace(timestamp(start));
                        if zoom > 1. {
                            ui.add(
                                egui::Slider::new(
                                    &mut self.playback.waveform_start,
                                    0.0..=max_start.max(0.001),
                                )
                                .show_value(false),
                            );
                        } else {
                            ui.add_space((ui.available_width() - 82.).max(0.));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.monospace(timestamp((start + span).min(waveform.duration)));
                        });
                    });
                } else {
                    ui.horizontal(|ui| {
                        if self.playback.waveform_loading {
                            ui.spinner();
                        }
                        ui.weak(self.playback.waveform_error.as_deref().unwrap_or(if en {
                            "Calculating waveform locally …"
                        } else {
                            "Wellenform wird lokal berechnet …"
                        }));
                    });
                    ui.add_space(12.);
                }
            });
        if let Some(zoom) = zoom_change
            && let Some(waveform) = &self.playback.waveform
        {
            self.playback.waveform_zoom = zoom;
            let span = waveform.duration / zoom;
            self.playback.waveform_start =
                (self.seek - span / 2.).clamp(0., (waveform.duration - span).max(0.));
        }
        if let Some(time) = seek {
            self.jump_audio(time);
        }
    }

    fn toggle_audio(&mut self) {
        if self.player.as_ref().is_some_and(|p| p.pending()) {
            self.player = None;
        } else if self.player.as_ref().is_some_and(|p| p.playing()) {
            self.player.as_mut().unwrap().pause();
        } else {
            self.seek_audio(self.seek, true);
        }
    }

    fn jump_audio(&mut self, time: f64) {
        let duration = self.view.as_ref().map(|r| r.duration).unwrap_or(0.);
        self.seek = time.clamp(0., duration);
        self.playback.scroll_requested = true;
        let playing = self.player.as_ref().is_some_and(|p| p.playing());
        self.seek_audio(self.seek, playing);
    }

    pub(super) fn audio_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(r) = &self.view else { return };
        let duration = r.duration;
        let audio_available = r.audio_filename.is_some() || r.secondary_audio_filename.is_some();
        let en = self.english;
        if let Some(p) = &self.player
            && !p.pending()
            && !self.dragging_seek
        {
            self.seek = p.position().min(duration);
        }
        let playing = self.player.as_ref().is_some_and(|p| p.playing());
        let pending = self.player.as_ref().is_some_and(|p| p.pending());
        ui.add_enabled_ui(audio_available, |ui| {
            ui.horizontal(|ui| {
                let previous = adjacent_time(&self.playback.ranges, self.seek, false);
                if passage_button(
                    ui,
                    previous.is_some(),
                    "|‹",
                    if en {
                        "Previous passage"
                    } else {
                        "Vorheriger Abschnitt"
                    },
                ) {
                    self.jump_audio(previous.unwrap());
                }
                if ui
                    .button("−5 s")
                    .on_hover_text(if en {
                        "Back 5 seconds · ←"
                    } else {
                        "5 Sekunden zurück · ←"
                    })
                    .clicked()
                {
                    self.jump_audio(self.seek - 5.);
                }
                let label = if pending {
                    if en {
                        "Cancel audio"
                    } else {
                        "Audio abbrechen"
                    }
                } else if playing {
                    "Pause"
                } else if en {
                    "Play"
                } else {
                    "Abspielen"
                };
                if ui
                    .button(label)
                    .on_hover_text(if en { "Space" } else { "Leertaste" })
                    .clicked()
                {
                    self.toggle_audio();
                }
                if ui
                    .button("+5 s")
                    .on_hover_text(if en {
                        "Forward 5 seconds · →"
                    } else {
                        "5 Sekunden vor · →"
                    })
                    .clicked()
                {
                    self.jump_audio(self.seek + 5.);
                }
                let next = adjacent_time(&self.playback.ranges, self.seek, true);
                if passage_button(
                    ui,
                    next.is_some(),
                    "›|",
                    if en {
                        "Next passage"
                    } else {
                        "Nächster Abschnitt"
                    },
                ) {
                    self.jump_audio(next.unwrap());
                }
                ui.spacing_mut().slider_width = (ui.available_width() - 255.).max(80.);
                let response = ui.add(
                    egui::Slider::new(&mut self.seek, 0.0..=duration.max(0.1)).show_value(false),
                );
                if response.drag_started() {
                    self.dragging_seek = true;
                }
                if response.drag_stopped() || (response.changed() && !response.dragged()) {
                    self.jump_audio(self.seek);
                    self.dragging_seek = false;
                }
                ui.monospace(format!(
                    "{} / {}",
                    timestamp(self.seek),
                    timestamp(duration)
                ));
                if pending || self.player.as_ref().is_some_and(|p| p.buffering()) {
                    ui.spinner();
                }
            });
            ui.horizontal_wrapped(|ui| {
                let old_speed = self.daily.preferences.playback_speed;
                ui.label(if en { "Speed" } else { "Tempo" });
                egui::ComboBox::from_id_salt("playback_speed")
                    .width(62.)
                    .height(280.)
                    .selected_text(speed_label(old_speed, en))
                    .show_ui(ui, |ui| {
                        for rate in crate::player::RATES {
                            if ui.selectable_value(
                                &mut self.daily.preferences.playback_speed,
                                rate,
                                speed_label(rate, en),
                            ).clicked() {
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text(if en {
                        "Preserves voice pitch. Times refer to the original recording."
                    } else {
                        "Tonhöhe bleibt erhalten. Zeitmarken beziehen sich auf die Originalaufnahme."
                    });
                if old_speed != self.daily.preferences.playback_speed {
                    let rate = self.daily.preferences.playback_speed;
                    if rate != 1. && !Path::new(&self.job_fields[3]).is_file() {
                        self.daily.preferences.playback_speed = old_speed;
                        self.error = true;
                        self.status = if en {
                            "For playback speed, select FFmpeg under Models → Local programs."
                        } else {
                            "Für das Wiedergabetempo bitte FFmpeg unter Modelle → Lokale Programme auswählen."
                        }.into();
                    } else if let Some(player) = &mut self.player
                        && let Err(error) = player.set_rate(rate, self.job_fields[3].clone().into())
                    {
                        self.daily.preferences.playback_speed = old_speed;
                        self.error = true;
                        self.status = error.to_string();
                    }
                }
                ui.separator();
                if ui
                    .checkbox(
                        &mut self.daily.preferences.follow_playback,
                        if en {
                            "Follow playback"
                        } else {
                            "Text folgt Wiedergabe"
                        },
                    )
                    .changed()
                {
                    self.playback.last_followed = None;
                }
                ui.separator();
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.playback.jump)
                        .desired_width(72.)
                        .hint_text("mm:ss"),
                );
                let at = parse_time(&self.playback.jump).filter(|t| *t <= duration);
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui
                    .add_enabled(
                        at.is_some(),
                        egui::Button::new(if en { "Jump" } else { "Springen" }),
                    )
                    .clicked()
                    || (enter && at.is_some())
                {
                    self.jump_audio(at.unwrap());
                }
                if !self.playback.jump.is_empty() && at.is_none() {
                    ui.small(if en {
                        "Enter a time within the recording."
                    } else {
                        "Zeitpunkt innerhalb der Aufnahme eingeben."
                    });
                } else {
                    ui.small(if en {
                        "mm:ss or hh:mm:ss"
                    } else {
                        "mm:ss oder hh:mm:ss"
                    });
                }
            });
        });
        if audio_available
            && self.shell.section == Section::Library
            && !self.busy
            && !ctx.egui_wants_keyboard_input()
            && !self.library_open
            && !self.storage.open
            && !self.job_open
            && !self.conference.open
            && !self.research.open
            && !self.daily.dialog_open()
            && !egui::Popup::is_any_open(ctx)
            && !self.close_pending
        {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space)) {
                self.toggle_audio();
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft)) {
                self.jump_audio(self.seek - 5.);
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight)) {
                self.jump_audio(self.seek + 5.);
            }
        }
    }

    pub(super) fn reading_view(&mut self, ui: &mut egui::Ui) {
        let active = active_range(&self.playback.ranges, self.seek);
        let explicit = self.playback.scroll_requested;
        let follow = explicit
            || (self.daily.preferences.follow_playback && active != self.playback.last_followed);
        let anchor = if explicit {
            scroll_anchor(&self.playback.ranges, self.seek)
        } else {
            active
        };
        let highlight = active.map(|i| self.playback.ranges[i].bytes.clone());
        let font_size = self.daily.preferences.reading_font_size.clamp(14., 20.);
        let mut target = None;
        egui::ScrollArea::vertical()
            .id_salt("reading")
            .show(ui, |ui| {
                // Full text stays one selectable buffer, including across paragraph boundaries.
                let mut layout = |ui: &egui::Ui, buffer: &dyn egui::TextBuffer, width: f32| {
                    let text = buffer.as_str();
                    let format = egui::TextFormat {
                        font_id: egui::FontId::proportional(font_size),
                        color: if ui.visuals().dark_mode {
                            egui::Color32::from_rgb(218, 222, 226)
                        } else {
                            egui::Color32::from_rgb(38, 44, 49)
                        },
                        line_height: Some(font_size * 1.5),
                        ..Default::default()
                    };
                    let mut job = egui::text::LayoutJob::default();
                    if let Some(range) = &highlight {
                        job.append(&text[..range.start], 0., format.clone());
                        let mut marked = format.clone();
                        marked.background = ui.visuals().selection.bg_fill;
                        job.append(&text[range.clone()], 0., marked);
                        job.append(&text[range.end..], 0., format);
                    } else {
                        job.append(text, 0., format);
                    }
                    job.wrap.max_width = width;
                    ui.fonts_mut(|f| f.layout_job(job))
                };
                let markers = reading_markers(self.view.as_ref().unwrap());
                let mut text = self.text.as_str();
                let mut output = None;
                ui.horizontal_top(|ui| {
                    ui.add_space(118.);
                    output = Some(
                        egui::TextEdit::multiline(&mut text)
                            .id_salt("reading-text")
                            .frame(egui::Frame::NONE)
                            .margin(egui::vec2(2., 8.))
                            .desired_width(f32::INFINITY)
                            .layouter(&mut layout)
                            .show(ui),
                    );
                });
                let output = output.unwrap();
                for marker in &markers {
                    let cursor = egui::text::CCursor::new(marker.characters);
                    let position = output
                        .galley
                        .pos_from_cursor(cursor)
                        .translate(output.galley_pos.to_vec2())
                        .min;
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(output.response.rect.left() - 116., position.y - 2.),
                        egui::vec2(108., font_size * 2.7),
                    );
                    let label = format!("{}\n{}", marker.speaker, timestamp(marker.start));
                    let response = ui.put(
                        rect,
                        egui::Button::new(
                            RichText::new(label)
                                .size((font_size - 3.).max(10.))
                                .color(ui.visuals().weak_text_color()),
                        )
                        .frame(false)
                        .wrap(),
                    );
                    if response.clicked() {
                        target = Some((marker.segment_id.clone(), marker.start));
                    }
                    if let Some(chapter) = &marker.chapter {
                        let chapter_rect = egui::Rect::from_min_size(
                            egui::pos2(output.response.rect.left(), position.y - font_size * 1.45),
                            egui::vec2(output.response.rect.width(), font_size * 1.35),
                        );
                        ui.painter().text(
                            chapter_rect.left_bottom(),
                            egui::Align2::LEFT_BOTTOM,
                            chapter,
                            egui::FontId::proportional(font_size + 1.),
                            ui.visuals().strong_text_color(),
                        );
                    }
                }
                if follow && let Some(i) = anchor {
                    let cursor = egui::text::CCursor::new(self.playback.ranges[i].characters.start);
                    let rect = output
                        .galley
                        .pos_from_cursor(cursor)
                        .translate(output.galley_pos.to_vec2());
                    ui.scroll_to_rect(rect, Some(egui::Align::Center));
                }
                if output.response.double_clicked()
                    && let Some(cursor) = output.cursor_range
                {
                    let pos = cursor.primary.index.0;
                    target = self
                        .spans
                        .iter()
                        .find(|(start, end, _, _)| pos >= *start && pos < *end)
                        .map(|(_, _, id, time)| (id.clone(), *time));
                }
            });
        self.playback.last_followed = active;
        self.playback.scroll_requested = false;
        if let Some((id, time)) = target {
            self.select(id, time, true);
        }
    }
}

#[derive(Debug, PartialEq)]
struct ReadingMarker {
    characters: usize,
    segment_id: String,
    start: f64,
    speaker: String,
    chapter: Option<String>,
}

fn reading_markers(recording: &Recording) -> Vec<ReadingMarker> {
    let mut characters = 0;
    let mut result = Vec::new();
    let mut previous_start = None;
    for (index, paragraph) in paragraphs(recording).iter().enumerate() {
        if index > 0 {
            characters += 2;
        }
        let start = paragraph.start();
        let chapter = recording.conference.as_ref().and_then(|conference| {
            conference
                .chapters
                .iter()
                .filter(|chapter| {
                    chapter.start <= start
                        && previous_start.is_none_or(|previous| chapter.start > previous)
                })
                .max_by(|a, b| a.start.total_cmp(&b.start))
                .map(|chapter| chapter.title.clone())
        });
        result.push(ReadingMarker {
            characters,
            segment_id: paragraph.segments[0].id.clone(),
            start,
            speaker: recording.speaker_name(paragraph.speaker_id()).to_owned(),
            chapter,
        });
        characters += paragraph.text.chars().count();
        previous_start = Some(start);
    }
    result
}

fn speed_label(rate: f64, english: bool) -> String {
    let number = rate.to_string();
    format!(
        "{}×",
        if english {
            number
        } else {
            number.replace('.', ",")
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Recording {
        laut_core::Document::parse(include_bytes!("../../../../fixtures/conversation.json"))
            .unwrap()
            .view()
    }
    #[test]
    fn reading_offsets_preserve_unicode_and_exclusions() {
        let mut r = fixture();
        let ranges = reading_ranges(&r);
        let text = plain_text(&r);
        for (range, segment) in ranges.iter().zip(&r.segments) {
            assert_eq!(&text[range.bytes.clone()], segment.text);
            assert_eq!(
                text.chars()
                    .skip(range.characters.start)
                    .take(range.characters.len())
                    .collect::<String>(),
                segment.text
            );
        }
        let value =
            serde_json::json!({"start": 0., "end": 3., "confirmed": true, "excluded": true});
        r.conference.as_mut().unwrap().ranges = Some(vec![serde_json::from_value(value).unwrap()]);
        let ranges = reading_ranges(&r);
        assert_eq!(ranges.len(), 2);
        assert!(active_range(&ranges, 1.).is_none());
        assert_eq!(&plain_text(&r)[ranges[0].bytes.clone()], "Second sentence.");
        assert_eq!(adjacent_time(&ranges, 0., true), Some(3.));
        assert_eq!(scroll_anchor(&ranges, 1.), Some(0));
    }
    #[test]
    fn playback_navigation_handles_gaps_boundaries_and_long_times() {
        let ranges = reading_ranges(&fixture());
        assert_eq!(active_range(&ranges, 3.), Some(1));
        assert_eq!(active_range(&ranges, 6.5), None);
        assert_eq!(scroll_anchor(&ranges, 6.5), Some(2));
        assert_eq!(scroll_anchor(&ranges, 12.), Some(2));
        assert_eq!(scroll_anchor(&[], 0.), None);
        assert_eq!(adjacent_time(&ranges, 3.5, false), Some(0.));
        assert_eq!(adjacent_time(&ranges, 5.5, false), Some(3.));
        assert_eq!(adjacent_time(&ranges, 6.5, true), Some(7.));
        assert_eq!(adjacent_time(&ranges, 10., true), None);
        assert_eq!(parse_time("7:02:03.5"), Some(25323.5));
        assert_eq!(parse_time("90:00"), Some(5400.));
        for value in [
            "", "-1", "NaN", "1:60", "1:60:00", "1::2", "1:2:3:4", "1e9", ".",
        ] {
            assert_eq!(parse_time(value), None, "{value}");
        }
    }
    #[test]
    fn reading_markers_follow_copy_text_offsets_and_paragraph_speakers() {
        let recording = fixture();
        let text = plain_text(&recording);
        let markers = reading_markers(&recording);
        assert_eq!(markers.len(), paragraphs(&recording).len());
        for (marker, paragraph) in markers.iter().zip(paragraphs(&recording)) {
            assert_eq!(
                text.chars()
                    .skip(marker.characters)
                    .take(paragraph.text.chars().count())
                    .collect::<String>(),
                paragraph.text
            );
            assert_eq!(marker.start, paragraph.start());
            assert_eq!(marker.segment_id, paragraph.segments[0].id);
            assert_eq!(
                marker.speaker,
                recording.speaker_name(paragraph.speaker_id())
            );
        }
    }
}
