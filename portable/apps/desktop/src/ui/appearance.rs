use super::*;

pub(super) fn theme(ctx: &egui::Context, name: &str) {
    ctx.set_theme(match name {
        "light" => egui::ThemePreference::Light,
        "dark" => egui::ThemePreference::Dark,
        _ => egui::ThemePreference::System,
    });
}
pub(super) fn configure(ctx: &egui::Context) {
    // Use the locally installed system UI font, with egui's bundled font as a fallback.
    let font = if cfg!(target_os = "macos") {
        Some(PathBuf::from("/System/Library/Fonts/SFNS.ttf"))
    } else if cfg!(windows) {
        std::env::var_os("WINDIR").map(|p| PathBuf::from(p).join("Fonts/segoeui.ttf"))
    } else {
        None
    };
    if let Some(path) = font
        && let Ok(bytes) = std::fs::read(path)
    {
        let mut fonts = egui::FontDefinitions::default();
        fonts
            .font_data
            .insert("system-ui".into(), egui::FontData::from_owned(bytes).into());
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "system-ui".into());
        ctx.set_fonts(fonts);
    }
    ctx.all_styles_mut(|style| {
        for (kind, size) in [
            (egui::TextStyle::Body, 13.),
            (egui::TextStyle::Button, 13.),
            (egui::TextStyle::Heading, 23.),
            (egui::TextStyle::Small, 11.),
        ] {
            style
                .text_styles
                .insert(kind, egui::FontId::proportional(size));
        }
        style.spacing.item_spacing = egui::vec2(8., 7.);
        style.spacing.button_padding = egui::vec2(10., 5.);
        style.spacing.interact_size.y = 28.;
        style.spacing.slider_width = 220.;
        let dark = style.visuals.dark_mode;
        style.visuals.panel_fill = if dark {
            egui::Color32::from_rgb(28, 29, 32)
        } else {
            egui::Color32::from_rgb(253, 253, 254)
        };
        style.visuals.window_fill = if dark {
            egui::Color32::from_rgb(39, 40, 44)
        } else {
            egui::Color32::from_rgb(250, 250, 252)
        };
        style.visuals.window_corner_radius = egui::CornerRadius::same(10);
        style.visuals.menu_corner_radius = egui::CornerRadius::same(8);
        style.visuals.selection.bg_fill = if dark {
            egui::Color32::from_rgb(33, 72, 77)
        } else {
            egui::Color32::from_rgb(214, 235, 235)
        };
        style.visuals.selection.stroke = egui::Stroke::new(
            1.,
            if dark {
                egui::Color32::from_rgb(168, 228, 226)
            } else {
                egui::Color32::from_rgb(0, 92, 98)
            },
        );
        style.visuals.widgets.inactive.weak_bg_fill = if dark {
            egui::Color32::from_rgb(48, 49, 53)
        } else {
            egui::Color32::from_rgb(243, 243, 246)
        };
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(
            0.5,
            if dark {
                egui::Color32::from_gray(70)
            } else {
                egui::Color32::from_gray(216)
            },
        );
        for widget in [
            &mut style.visuals.widgets.inactive,
            &mut style.visuals.widgets.hovered,
            &mut style.visuals.widgets.active,
            &mut style.visuals.widgets.open,
        ] {
            widget.corner_radius = egui::CornerRadius::same(6);
        }
    });
}
