//! The look of the app: dark or light, and a light touch of the brand
//! colours, the same as Duck Quill. The game is the star, so almost
//! everything stays the calm grey egui gives us; only the "this one is
//! chosen" colour and the main button use the brand.

use eframe::egui::{self, Button, Color32, RichText, Stroke, Theme, ThemePreference};

/// Night indigo: what's chosen (Build or Play, a line of the check).
pub const NIGHT_INDIGO: Color32 = Color32::from_rgb(0x55, 0x46, 0xD0);
/// Duck yellow: the main button (New game, Make it, Restart).
pub const DUCK_YELLOW: Color32 = Color32::from_rgb(0xFF, 0xCC, 0x2E);
/// Ink navy: text on duck yellow, dark enough to read easily.
pub const INK_NAVY: Color32 = Color32::from_rgb(0x1B, 0x14, 0x46);
/// "Checks pass", and a line of the check that's fine.
pub const GOOD: Color32 = Color32::from_rgb(0x4C, 0xB8, 0x6A);
/// "Needs a look", and a warning.
pub const LOOK: Color32 = Color32::from_rgb(0xF0, 0xA0, 0x30);
/// A problem.
pub const BAD: Color32 = Color32::from_rgb(0xE5, 0x4B, 0x5F);

/// Gives both the dark and the light look the brand's selection colour.
/// Done once at start: egui keeps a style for each theme.
pub fn apply_brand(ctx: &egui::Context) {
    for theme in [Theme::Dark, Theme::Light] {
        ctx.style_mut_of(theme, |style| {
            style.visuals.selection.bg_fill = NIGHT_INDIGO;
            style.visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        });
    }
}

/// The main button of a screen or a dialog: duck yellow with navy text,
/// so it's the obvious one to press.
pub fn primary_button(text: &str) -> Button<'static> {
    Button::new(RichText::new(text).color(INK_NAVY).strong()).fill(DUCK_YELLOW)
}

/// The Theme menu: dark, light, or whatever Windows is set to.
pub fn theme_menu(ui: &mut egui::Ui) -> Option<ThemePreference> {
    let ctx = ui.ctx().clone();
    let mut choice = ctx.options(|o| o.theme_preference);
    let before = choice;
    ui.menu_button("Theme", |ui| {
        ui.radio_value(&mut choice, ThemePreference::Dark, "Dark");
        ui.radio_value(&mut choice, ThemePreference::Light, "Light");
        ui.radio_value(&mut choice, ThemePreference::System, "Same as Windows");
    });
    if choice != before {
        ctx.set_theme(choice);
        return Some(choice);
    }
    None
}

/// The theme saved in the settings ("dark", "light", "system"); dark if
/// none was saved.
pub fn from_name(name: Option<&str>) -> ThemePreference {
    match name {
        Some("light") => ThemePreference::Light,
        Some("system") => ThemePreference::System,
        _ => ThemePreference::Dark,
    }
}

/// The name a theme is saved under.
pub fn name(theme: ThemePreference) -> &'static str {
    match theme {
        ThemePreference::Dark => "dark",
        ThemePreference::Light => "light",
        ThemePreference::System => "system",
    }
}
