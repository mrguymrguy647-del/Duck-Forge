//! The Files panel: the open game's own folder as a tree, in Build mode's
//! right side.
//!
//! Click a picture to see it, a sound to hear it; anything else opens in
//! the program Windows uses for it. Files dropped from File Explorer are
//! copied into the game's folder (or the folder that's picked). New
//! folder, Rename and Delete (to the Recycle Bin). Everything it does is
//! one of the Studio's `files.*` commands; this only draws.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};
use studio_core::game_files::{self, FileEntry, FileKind};
use studio_core::studio::Command;

use crate::pictures::Pictures;
use crate::theme;

/// A file being dragged out of the panel: into the scene, or onto an
/// object's Picture or Sound box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraggedFile {
    /// Its place in the game's folder: `assets/sign.png`.
    pub path: String,
    pub kind: FileKind,
}

impl DraggedFile {
    /// Its own name: `sign.png`.
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// The name an object made of it gets: `sign`.
    pub fn stem(&self) -> &str {
        let name = self.name();
        name.split('.').next().unwrap_or(name)
    }
}

/// The file being dragged out of the panel right now, if one is.
pub fn dragging(ctx: &egui::Context) -> Option<std::sync::Arc<DraggedFile>> {
    egui::DragAndDrop::payload::<DraggedFile>(ctx)
}

/// What the panel remembers between frames.
#[derive(Default)]
pub struct FilesPanel {
    /// Folders shown open, by place.
    open_folders: BTreeSet<String>,
    /// Folders the person closed (every folder starts open).
    closed_folders: BTreeSet<String>,
    /// A name being typed: for a new folder, or a rename.
    naming: Option<Naming>,
}

/// A name being typed in the panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Naming {
    NewFolder { inside: String, typed: String },
    Rename { path: String, typed: String },
}

impl FilesPanel {
    /// Draws the panel for the game whose folder is `folder` (`None` for a
    /// one-file game). With `drag`, pictures, sounds and levels can be
    /// dragged out into the scene. Returns the commands to run.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        folder: Option<&Path>,
        selected: Option<&str>,
        pictures: &mut Pictures,
        drag: bool,
    ) -> Vec<Command> {
        let mut commands = Vec::new();
        let Some(folder) = folder else {
            ui.label(
                "This game is one file, with no folder of its own for pictures and sounds. \
                 Games made from a starter (all but Tiny) have one.",
            );
            return commands;
        };
        let entries = game_files::tree(folder);
        // Where new things go: the folder that's picked, or the folder of
        // the file that's picked, or the game's own folder.
        let target = target_folder(&entries, selected);

        ui.horizontal_wrapped(|ui| {
            if ui
                .button("New folder")
                .on_hover_text(match target.as_str() {
                    "" => String::from("In the game's folder"),
                    inside => format!("In {inside}"),
                })
                .clicked()
            {
                self.naming = Some(Naming::NewFolder {
                    inside: target.clone(),
                    typed: String::new(),
                });
            }
            if ui
                .button("Show in File Explorer")
                .on_hover_text("Open the game's folder")
                .clicked()
            {
                // The game's own folder is the place "".
                commands.push(Command::FilesOpen {
                    path: String::new(),
                });
            }
        });
        ui.label(
            RichText::new(match target.as_str() {
                "" => String::from(
                    "Drop pictures and sounds here from File Explorer to copy them in.",
                ),
                inside => format!("Drop files here to copy them into {inside}."),
            })
            .small()
            .weak(),
        );
        ui.add_space(4.0);

        // The tree.
        let mut hidden_under: Option<String> = None;
        for entry in &entries {
            if let Some(closed) = &hidden_under {
                if entry.path.starts_with(&format!("{closed}/")) {
                    continue;
                }
                hidden_under = None;
            }
            let is_open = self.is_open(&entry.path);
            if entry.kind == FileKind::Folder && !is_open {
                hidden_under = Some(entry.path.clone());
            }
            let picked = selected == Some(entry.path.as_str());
            ui.horizontal(|ui| {
                ui.add_space(entry.depth as f32 * 14.0);
                let text = match entry.kind {
                    FileKind::Folder => {
                        format!("{} {}", if is_open { "v" } else { ">" }, entry.name)
                    }
                    _ => entry.name.clone(),
                };
                let usable = drag
                    && matches!(
                        entry.kind,
                        FileKind::Picture | FileKind::Sound | FileKind::Level
                    );
                let response = ui.selectable_label(picked, text);
                let response = if usable {
                    response
                        .interact(egui::Sense::drag())
                        .on_hover_text(drag_text(entry))
                } else {
                    response.on_hover_text(hover_text(entry))
                };
                if usable && response.drag_started() {
                    egui::DragAndDrop::set_payload(
                        ui.ctx(),
                        DraggedFile {
                            path: entry.path.clone(),
                            kind: entry.kind,
                        },
                    );
                }
                if response.clicked() {
                    commands.extend(self.clicked(entry));
                }
                ui.label(RichText::new(entry.kind.word()).small().weak());
            });
        }
        if entries.is_empty() {
            ui.label(RichText::new("The game's folder is empty.").weak());
        }

        // The file that's picked: what it is, and what can be done to it.
        if let Some(entry) = selected.and_then(|place| entries.iter().find(|e| e.path == place)) {
            ui.separator();
            ui.label(RichText::new(&entry.path).strong());
            if entry.kind == FileKind::Picture {
                let path = folder.join(&entry.path);
                match pictures.get(ui.ctx(), &path) {
                    Some(texture) => {
                        let size = texture.size_vec2();
                        let fit = (ui.available_width() / size.x)
                            .clamp(0.1, 8.0)
                            .floor()
                            .max(1.0);
                        let fit = if size.x * fit > ui.available_width() {
                            ui.available_width() / size.x
                        } else {
                            fit
                        };
                        ui.image((texture.id(), size * fit));
                        ui.label(
                            RichText::new(format!("{} x {} pixels", size.x, size.y))
                                .small()
                                .weak(),
                        );
                    }
                    None => {
                        ui.label(RichText::new("Only PNG pictures show here.").weak());
                    }
                }
            } else if entry.kind != FileKind::Folder {
                ui.label(RichText::new(size_words(entry.size)).small().weak());
            }
            ui.horizontal_wrapped(|ui| {
                if entry.kind == FileKind::Sound && ui.button("Play").clicked() {
                    commands.push(Command::FilesPlay {
                        path: entry.path.clone(),
                    });
                }
                if entry.kind != FileKind::Folder
                    && ui
                        .button("Open")
                        .on_hover_text("In its own program")
                        .clicked()
                {
                    commands.push(Command::FilesOpen {
                        path: entry.path.clone(),
                    });
                }
                let is_main = entry.path == "main.rs";
                if ui
                    .add_enabled(!is_main, egui::Button::new("Rename"))
                    .clicked()
                {
                    self.naming = Some(Naming::Rename {
                        path: entry.path.clone(),
                        typed: entry.name.clone(),
                    });
                }
                if ui
                    .add_enabled(!is_main, egui::Button::new("Delete"))
                    .on_hover_text("Moves it to the Recycle Bin")
                    .on_disabled_hover_text("main.rs is where the game starts")
                    .clicked()
                {
                    commands.push(Command::FilesDelete {
                        path: entry.path.clone(),
                    });
                }
            });
        }

        // Typing a name.
        if let Some(command) = self.naming_box(ui) {
            commands.push(command);
        }
        commands
    }

    fn is_open(&self, folder: &str) -> bool {
        self.open_folders.contains(folder) || !self.closed_folders.contains(folder)
    }

    /// A click on a line: a folder opens or closes, a picture shows, a
    /// sound plays, anything else opens in its own program.
    fn clicked(&mut self, entry: &FileEntry) -> Vec<Command> {
        let select = Command::FilesSelect {
            path: Some(entry.path.clone()),
        };
        match entry.kind {
            FileKind::Folder => {
                if self.is_open(&entry.path) {
                    self.open_folders.remove(&entry.path);
                    self.closed_folders.insert(entry.path.clone());
                } else {
                    self.closed_folders.remove(&entry.path);
                    self.open_folders.insert(entry.path.clone());
                }
                vec![select]
            }
            FileKind::Picture => vec![select],
            FileKind::Sound => vec![
                select,
                Command::FilesPlay {
                    path: entry.path.clone(),
                },
            ],
            _ => vec![
                select,
                Command::FilesOpen {
                    path: entry.path.clone(),
                },
            ],
        }
    }

    /// The little box for a new folder's name, or a new name.
    fn naming_box(&mut self, ui: &mut egui::Ui) -> Option<Command> {
        let naming = self.naming.as_mut()?;
        let mut command = None;
        let mut done = false;
        ui.separator();
        let (title, typed) = match naming {
            Naming::NewFolder { inside, typed } => (
                if inside.is_empty() {
                    String::from("New folder:")
                } else {
                    format!("New folder in {inside}:")
                },
                typed,
            ),
            Naming::Rename { path, typed } => (format!("New name for {path}:"), typed),
        };
        ui.label(title);
        let field = ui.add(egui::TextEdit::singleline(typed).desired_width(f32::INFINITY));
        field.request_focus();
        let check = game_files::check_name(typed);
        if let Err(problem) = &check
            && !typed.is_empty()
        {
            ui.label(RichText::new(&problem.what).small().color(theme::LOOK));
        }
        ui.horizontal(|ui| {
            let ready = check.is_ok();
            let entered = ready && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.add_enabled(ready, theme::primary_button("OK")).clicked() || entered {
                command = Some(match &*naming {
                    Naming::NewFolder { inside, typed } => Command::FilesNewFolder {
                        path: if inside.is_empty() {
                            typed.trim().to_string()
                        } else {
                            format!("{inside}/{}", typed.trim())
                        },
                    },
                    Naming::Rename { path, typed } => Command::FilesRename {
                        path: path.clone(),
                        to: typed.trim().to_string(),
                    },
                });
                done = true;
            }
            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                done = true;
            }
        });
        if done {
            self.naming = None;
        }
        command
    }
}

/// What a line says when it can be dragged into the scene.
fn drag_text(entry: &FileEntry) -> String {
    match entry.kind {
        FileKind::Picture => String::from(
            "Click to see it. Drag it into the scene to add it there, or onto a Picture box",
        ),
        FileKind::Sound => {
            String::from("Click to hear it. Drag it into the scene for music, or onto a Sound box")
        }
        _ => String::from("Click to open it. Drag it into the scene to add it as a tile map"),
    }
}

/// Files dropped on the Studio's window from File Explorer this frame.
pub fn dropped(ctx: &egui::Context) -> Vec<PathBuf> {
    ctx.input(|i| {
        i.raw
            .dropped_files
            .iter()
            .map(|file| file.path().to_path_buf())
            .collect()
    })
}

/// Whether files are being dragged over the window.
pub fn being_dragged_in(ctx: &egui::Context) -> bool {
    ctx.input(|i| !i.raw.hovered_files.is_empty())
}

/// The command for files dropped on a game's screen: copy them into its
/// folder, or the folder picked in the Files panel.
pub fn add_dropped(folder: &Path, selected: Option<&str>, files: Vec<PathBuf>) -> Command {
    let entries = game_files::tree(folder);
    Command::FilesAdd {
        files,
        to: target_folder(&entries, selected),
    }
}

/// Which of the files dropped from File Explorer a scene can use straight
/// away (pictures, sprite sheets and sounds); the rest are only copied in.
/// A picture dropped with its sheet comes in with the sheet, so it isn't
/// one by itself.
pub fn usable_in_a_scene(files: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let extension = |path: &Path| {
        path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
    };
    files.iter().cloned().partition(|path| {
        let name = path
            .file_name()
            .map_or(String::new(), |n| n.to_string_lossy().to_lowercase());
        match extension(path).as_str() {
            "wav" => true,
            "json" => !name.ends_with(".legend.json"),
            "png" => !files.contains(&path.with_extension("json")),
            _ => false,
        }
    })
}

/// Where new folders and dropped files go: the folder that's picked, the
/// folder of the file that's picked, or the game's own folder ("").
pub fn target_folder(entries: &[FileEntry], selected: Option<&str>) -> String {
    let Some(entry) = selected.and_then(|place| entries.iter().find(|e| e.path == place)) else {
        return String::new();
    };
    if entry.kind == FileKind::Folder {
        entry.path.clone()
    } else {
        entry
            .path
            .rsplit_once('/')
            .map_or(String::new(), |(folder, _)| folder.to_string())
    }
}

fn hover_text(entry: &FileEntry) -> String {
    match entry.kind {
        FileKind::Folder => String::from("Click to open or close it"),
        FileKind::Picture => String::from("Click to see it"),
        FileKind::Sound => String::from("Click to hear it"),
        _ => String::from("Click to open it in its own program"),
    }
}

fn size_words(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} bytes"),
        1024..1_048_576 => format!("{:.1} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, kind: FileKind) -> FileEntry {
        FileEntry {
            path: path.to_string(),
            name: path.rsplit('/').next().unwrap().to_string(),
            kind,
            depth: path.matches('/').count(),
            size: 0,
        }
    }

    #[test]
    fn new_things_go_in_the_picked_folder_or_beside_the_picked_file() {
        let entries = [
            entry("assets", FileKind::Folder),
            entry("assets/duck.png", FileKind::Picture),
            entry("main.rs", FileKind::Code),
        ];
        assert_eq!(target_folder(&entries, None), "");
        assert_eq!(target_folder(&entries, Some("assets")), "assets");
        assert_eq!(target_folder(&entries, Some("assets/duck.png")), "assets");
        assert_eq!(target_folder(&entries, Some("main.rs")), "");
        assert_eq!(target_folder(&entries, Some("gone.png")), "");
        assert_eq!(size_words(512), "512 bytes");
        assert_eq!(size_words(2048), "2.0 KB");
    }

    #[test]
    fn clicks_are_commands() {
        let mut panel = FilesPanel::default();
        let sound = entry("assets/jump.wav", FileKind::Sound);
        assert_eq!(
            panel.clicked(&sound),
            [
                Command::FilesSelect {
                    path: Some(String::from("assets/jump.wav"))
                },
                Command::FilesPlay {
                    path: String::from("assets/jump.wav")
                },
            ]
        );
        let code = entry("bot.rs", FileKind::Code);
        assert!(matches!(panel.clicked(&code)[1], Command::FilesOpen { .. }));
        let folder = entry("assets", FileKind::Folder);
        assert!(panel.is_open("assets"), "folders start open");
        panel.clicked(&folder);
        assert!(!panel.is_open("assets"));
        panel.clicked(&folder);
        assert!(panel.is_open("assets"));
    }
}
