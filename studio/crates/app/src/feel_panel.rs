//! Build mode's right side: Feel (the game's tune values as sliders,
//! with presets and "Show me the code"), its sounds, and its pictures.
//! And the History window.

use std::path::PathBuf;

use eframe::egui::{self, RichText};
use studio_core::studio::{Command, OpenGame};

use crate::theme;

/// The Feel panel's own state: which slider drag is going on.
#[derive(Default)]
pub struct FeelPanel {
    stroke: u64,
    /// The preset picked in the list.
    preset: Option<String>,
}

impl FeelPanel {
    /// Feel, Sounds and Pictures. Returns the commands to run.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        open: &OpenGame,
        root: &std::path::Path,
    ) -> Vec<Command> {
        let mut commands = Vec::new();
        ui.heading("Feel");
        if let Some(problem) = &open.feel_problem {
            ui.label(RichText::new(&problem.what).color(theme::BAD));
        }
        let sliders = open.sliders();
        if sliders.is_empty() {
            ui.label(
                RichText::new(
                    "This game hasn't asked for any values yet (ctx.tune). Play a moment, then \
                     freeze it again.",
                )
                .weak(),
            );
        }
        let mut group = String::new();
        for slider in sliders {
            if slider.group != group {
                group = slider.group.clone();
                ui.add_space(4.0);
                ui.label(RichText::new(&group).strong());
            }
            let mut value = slider.value;
            ui.horizontal(|ui| {
                ui.label(&slider.label);
                if ui
                    .small_button("code")
                    .on_hover_text(format!("Show where the code asks for {}", slider.name))
                    .clicked()
                {
                    commands.push(Command::FeelShowCode {
                        name: slider.name.clone(),
                    });
                }
            });
            let mut widget = egui::Slider::new(&mut value, slider.from..=slider.to);
            if slider.step > 0.0 {
                widget = widget.step_by(f64::from(slider.step));
            }
            let response = ui
                .add(widget)
                .on_hover_text(format!("{} (in the code: {})", slider.name, slider.default));
            if response.drag_started() || response.gained_focus() {
                self.stroke += 1;
            }
            if response.changed() && value != slider.value {
                commands.push(Command::FeelSet {
                    name: slider.name.clone(),
                    value,
                    stroke: Some(self.stroke),
                });
            }
        }
        if !open.feel.presets.is_empty() {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let shown = self
                    .preset
                    .clone()
                    .unwrap_or_else(|| String::from("Presets"));
                egui::ComboBox::from_id_salt("presets")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        for name in open.feel.presets.keys() {
                            if ui
                                .selectable_label(self.preset.as_ref() == Some(name), name)
                                .clicked()
                            {
                                self.preset = Some(name.clone());
                                commands.push(Command::FeelPreset { name: name.clone() });
                            }
                        }
                    });
            });
        }

        ui.add_space(10.0);
        ui.heading("Sounds");
        let sounds = open.sounds();
        if sounds.is_empty() {
            ui.label(RichText::new("The sounds the game has played show up here.").weak());
        }
        for sound in sounds {
            ui.horizontal(|ui| {
                if ui
                    .small_button("Play")
                    .on_hover_text("The game plays it")
                    .clicked()
                {
                    commands.push(Command::AssetsPlaySound {
                        name: sound.name.clone(),
                    });
                }
                ui.label(&sound.name);
                if ui
                    .small_button("Recipe")
                    .on_hover_text("How it was made")
                    .clicked()
                {
                    commands.push(Command::AssetsShowRecipe {
                        name: sound.name.clone(),
                    });
                }
            });
        }

        ui.add_space(10.0);
        ui.heading("Pictures");
        let pictures = pictures_of(open);
        if pictures.is_empty() {
            ui.label(RichText::new("This game has no picture files in its folder.").weak());
        }
        for picture in pictures {
            let shown = picture
                .strip_prefix(root)
                .unwrap_or(&picture)
                .display()
                .to_string();
            let name = picture
                .file_name()
                .map_or(String::new(), |name| name.to_string_lossy().into_owned());
            if ui
                .link(name)
                .on_hover_text(format!(
                    "Open {shown} in Duck Quill. When it's saved, the game shows it."
                ))
                .clicked()
            {
                commands.push(Command::AssetsOpenInQuill { file: picture });
            }
        }
        commands
    }
}

/// The game's picture files (its `assets` folder's PNGs).
fn pictures_of(open: &OpenGame) -> Vec<PathBuf> {
    let Some(folder) = &open.game.folder else {
        return Vec::new();
    };
    let mut pictures: Vec<PathBuf> = std::fs::read_dir(folder.join("assets"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|end| end == "png"))
        .collect();
    pictures.sort();
    pictures
}

/// The History window: every snapshot, newest first, each with "Go back
/// to here". Returns a command, and false if it was closed.
pub fn history(ctx: &egui::Context, open: &OpenGame, keep_open: &mut bool) -> Option<Command> {
    let mut command = None;
    egui::Window::new(format!("History: {}", open.game.title))
        .open(keep_open)
        .default_width(460.0)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label(
                RichText::new(
                    "Snapshots of the game's files, taken when you play after changes, use a \
                     preset, check, go back, and every five minutes while you work. They're plain \
                     folders in history/.",
                )
                .weak(),
            );
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .show(ui, |ui| {
                    let newest = open.snapshots.last().map(|s| s.number);
                    for snapshot in open.snapshots.iter().rev() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(when(snapshot.at)).monospace());
                            ui.label(&snapshot.title);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if Some(snapshot.number) != newest
                                        && ui.button("Go back to here").clicked()
                                    {
                                        command = Some(Command::EditGoBackTo {
                                            number: snapshot.number,
                                        });
                                    }
                                },
                            );
                        });
                    }
                });
            ui.separator();
            if ui.button("Take a snapshot now").clicked() {
                command = Some(Command::EditSnapshot {
                    title: String::from("Taken by hand"),
                });
            }
        });
    command
}

/// A snapshot's time: "Today 15:32", "Yesterday 09:10" or a date (in UTC:
/// the Studio has no time zone library).
fn when(at: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let (day, today) = (at / 86_400, now / 86_400);
    let clock = format!("{:02}:{:02}", at % 86_400 / 3600, at % 3600 / 60);
    match today.saturating_sub(day) {
        0 => format!("Today {clock}"),
        1 => format!("Yesterday {clock}"),
        days => format!("{days} days ago"),
    }
}

/// "Show me the code": the lines around where a value is asked for, and a
/// button that opens the file. Returns a file to open.
pub fn code_spot(ctx: &egui::Context, open: &OpenGame, keep_open: &mut bool) -> Option<PathBuf> {
    let (name, spot) = open.code_spot.as_ref()?;
    let mut to_open = None;
    egui::Window::new(format!("Where the code asks for {name}"))
        .open(keep_open)
        .default_width(560.0)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label(format!("{}, line {}", spot.file.display(), spot.line));
            ui.separator();
            for (number, line) in &spot.lines {
                let text = RichText::new(format!("{number:>4}  {line}")).monospace();
                ui.label(if *number == spot.line {
                    text.strong().color(theme::DUCK_YELLOW)
                } else {
                    text
                });
            }
            ui.separator();
            ui.label(
                RichText::new(
                    "The slider changes the value without touching the code: it's saved in the \
                     game's tune file. To change the default, edit this line.",
                )
                .weak(),
            );
            if ui.button("Open the file").clicked() {
                to_open = Some(spot.file.clone());
            }
        });
    to_open
}

/// A sound's recipe, as text.
pub fn recipe(ctx: &egui::Context, open: &OpenGame, keep_open: &mut bool) {
    let Some((name, recipe)) = &open.sound_recipe else {
        return;
    };
    egui::Window::new(format!("How {name} was made"))
        .open(keep_open)
        .default_width(460.0)
        .collapsible(false)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .show(ui, |ui| {
                    ui.label(RichText::new(recipe).monospace());
                });
        });
}

/// Opens a file in the program the computer uses for it.
pub fn open_with_the_system(file: &std::path::Path) {
    let result = if cfg!(windows) {
        std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(file)
            .spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(file).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(file).spawn()
    };
    let _ = result;
}
