//! The app: one frame at a time, it reads what the person did, tells the
//! Studio, and draws what the Studio holds.

use std::time::{Duration, Instant};

use duckforge::InputAction;
use eframe::egui::{self, RichText};
use studio_core::build::BuildState;
use studio_core::play::{Ending, PlayState};
use studio_core::studio::Screen;
use studio_core::{Command, Problem, Studio};

use crate::build_mode::{self, BuildMode};
use crate::feel_panel::{self, FeelPanel};
use crate::files_panel::{self, FilesPanel};
use crate::game_view::{self, GameTexture};
use crate::home::{self, GameDialogChoice, HomeChoice, NewGame, NewGameChoice};
use crate::keys::{self, Placement};
use crate::panels::{self, CheckChoice, ProblemChoice};
use crate::pictures::Pictures;
use crate::theme;
use crate::update_banner::{self, BannerChoice, UpdateState};

/// Which side of Build mode's right panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightTab {
    /// The selected object's settings (a game with scenes).
    Inspector,
    Feel,
    Files,
}

/// Duck Forge Studio's window.
pub struct StudioApp {
    pub studio: Studio,
    game_texture: GameTexture,
    pictures: Pictures,
    /// Where the game was drawn last frame, for the pointer.
    placement: Option<Placement>,
    /// The Studio is holding the pointer for the game (mouse look).
    holding_pointer: bool,
    modifiers: egui::Modifiers,
    pub new_game: Option<NewGame>,
    pub check_open: bool,
    pub about_open: bool,
    console_line: String,
    show_details: bool,
    /// The last clip's result has been seen.
    clip_seen: bool,
    /// A short note in the status bar ("Saved a screenshot: ..."), and
    /// when it was said. It goes after a few seconds.
    said: Option<(String, Instant)>,
    /// Build mode: the tool and tile picked.
    pub build: BuildMode,
    /// Build mode's right side: Feel, Sounds, Pictures.
    feel: FeelPanel,
    /// ...or the game's files.
    files: FilesPanel,
    /// The scene editor: the Scene list, the view's tools, the Inspector.
    scene: crate::scene_panel::ScenePanel,
    pub right_tab: RightTab,
    /// Objects just copied, as text for Windows' clipboard.
    copied: Option<String>,
    /// Rename, Duplicate or Delete, from a card on Home.
    pub game_dialog: Option<home::GameDialog>,
    pub history_open: bool,
    pub code_open: bool,
    pub recipe_open: bool,
    /// Is there a newer Studio? Only the real app checks (see
    /// [`StudioApp::checking_for_updates`]); tests never use the internet.
    pub update: UpdateState,
}

impl StudioApp {
    pub fn new(studio: Studio) -> StudioApp {
        StudioApp {
            studio,
            game_texture: GameTexture::default(),
            pictures: Pictures::default(),
            placement: None,
            holding_pointer: false,
            modifiers: egui::Modifiers::NONE,
            new_game: None,
            check_open: false,
            about_open: false,
            console_line: String::new(),
            show_details: false,
            clip_seen: false,
            said: None,
            build: BuildMode::default(),
            feel: FeelPanel::default(),
            files: FilesPanel::default(),
            scene: crate::scene_panel::ScenePanel::default(),
            right_tab: RightTab::Feel,
            copied: None,
            game_dialog: None,
            history_open: false,
            code_open: false,
            recipe_open: false,
            update: UpdateState::Off,
        }
    }

    /// Starts looking for a newer Studio, if Settings says to.
    pub fn checking_for_updates(mut self) -> StudioApp {
        if self.studio.settings.check_for_updates {
            self.update = UpdateState::check();
        }
        self
    }

    /// Does a command and shows what it said in the status bar.
    fn run_and_say(&mut self, command: Command) {
        if matches!(
            command,
            Command::FilesAdd { .. } | Command::FilesNewFolder { .. } | Command::FilesSelect { .. }
        ) {
            self.right_tab = RightTab::Files;
        }
        // Picking something, or making something, shows its settings.
        if matches!(
            command,
            Command::SelectSet { .. }
                | Command::SelectAdd { .. }
                | Command::SelectAll
                | Command::ObjectAdd { .. }
                | Command::ObjectPaste { .. }
                | Command::ObjectDuplicate { .. }
                | Command::FilesUse { .. }
        ) {
            self.right_tab = RightTab::Inspector;
        }
        let copying = matches!(command, Command::ObjectCopy { .. });
        let opens = match &command {
            Command::FeelShowCode { .. } => Some(&mut self.code_open),
            Command::AssetsShowRecipe { .. } => Some(&mut self.recipe_open),
            _ => None,
        };
        if let Some(window) = opens {
            *window = true;
        }
        let done = self.studio.run(command);
        if copying && done.is_ok() {
            // Onto Windows' clipboard too, as lines of a scene file, so
            // they can be pasted into a file in Notepad, or into another
            // game.
            self.copied = Some(studio_core::scene_edit::clipboard_text(
                &self.studio.clipboard,
            ));
        }
        if let Ok(said) = done {
            self.said = Some((said, Instant::now()));
        }
    }

    /// Build mode's right side: Feel, Sounds and Pictures, or Files.
    fn feel_panel(&mut self, ui: &mut egui::Ui) {
        let mut commands = Vec::new();
        let mut choose_quill = false;
        egui::Panel::right("feel")
            .default_size(270.0)
            .min_size(220.0)
            .show(ui, |ui| {
                let has_scene = self
                    .studio
                    .open
                    .as_ref()
                    .is_some_and(|open| open.scene.is_some());
                if self.right_tab == RightTab::Inspector && !has_scene {
                    self.right_tab = RightTab::Feel;
                }
                // A game with scenes has Files on the left, under its
                // objects, so they can be dragged into the scene.
                if self.right_tab == RightTab::Files && has_scene {
                    self.right_tab = RightTab::Inspector;
                }
                ui.horizontal_wrapped(|ui| {
                    if has_scene {
                        ui.selectable_value(&mut self.right_tab, RightTab::Inspector, "Inspector")
                            .on_hover_text("The selected object's settings");
                    }
                    ui.selectable_value(
                        &mut self.right_tab,
                        RightTab::Feel,
                        if has_scene {
                            "Feel"
                        } else {
                            "Feel, sounds, pictures"
                        },
                    )
                    .on_hover_text("Feel, sounds and pictures");
                    if !has_scene {
                        ui.selectable_value(&mut self.right_tab, RightTab::Files, "Files");
                    }
                });
                ui.separator();
                if self.right_tab == RightTab::Inspector {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if let Some(open) = &self.studio.open {
                            commands = self.scene.inspector(ui, open);
                        }
                    });
                    return;
                }
                if self.right_tab == RightTab::Files {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if let Some(open) = &self.studio.open {
                            ui.heading(format!("Files of {}", open.game.title));
                            commands = self.files.show(
                                ui,
                                open.game.folder.as_deref(),
                                open.files_selected.as_deref(),
                                &mut self.pictures,
                                false,
                            );
                        }
                    });
                    return;
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if let (Some(open), Some(folder)) = (&self.studio.open, &self.studio.folder) {
                        commands = self.feel.show(ui, open, folder.root());
                    }
                    let found = studio_core::assets::find_duck_quill(
                        self.studio.settings.duck_quill.as_deref(),
                    );
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(match &found {
                                Some(_) => "Duck Quill is ready.",
                                None => "Duck Quill isn't found yet.",
                            })
                            .weak(),
                        );
                        if ui
                            .small_button("Choose...")
                            .on_hover_text("Where DuckQuill.exe is")
                            .clicked()
                        {
                            choose_quill = true;
                        }
                    });
                });
            });
        if choose_quill
            && let Some(file) = rfd::FileDialog::new()
                .set_title("Where is Duck Quill?")
                .pick_file()
        {
            self.studio.settings.duck_quill = Some(file);
            self.studio.settings.save();
        }
        for command in commands {
            self.run_and_say(command);
        }
    }

    /// Does a command. A problem is kept by the Studio and shown.
    fn run(&mut self, command: Command) {
        let _ = self.studio.run(command);
    }

    fn play_state(&self) -> Option<&PlayState> {
        self.studio.play().map(|play| &play.state)
    }

    fn is_playing(&self) -> bool {
        self.play_state() == Some(&PlayState::Playing)
    }

    /// One frame of the whole app.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let now = Instant::now();
        let dialog = self.new_game.is_some()
            || self.about_open
            || self.game_dialog.is_some()
            || self.scene.is_adding();
        // A text box (the Console's line, a name) has the keyboard.
        let typing = ctx.egui_wants_keyboard_input();

        // Tab is the Studio's own key on the game screen: Build and Play.
        if self.studio.screen == Screen::Game
            && !dialog
            && !typing
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab))
        {
            self.run(Command::GameToggle);
        }
        // Undo and Redo, in Build mode (while playing, those keys are the game's).
        let frozen = self.play_state() == Some(&PlayState::Frozen);
        if self.studio.screen == Screen::Game && frozen && !dialog && !typing {
            let (undo, redo) = ui.input_mut(|i| {
                let redo = i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y)
                    || i.consume_key(
                        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                        egui::Key::Z,
                    );
                (i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z), redo)
            });
            if undo {
                self.run_and_say(Command::EditUndo);
            }
            if redo {
                self.run_and_say(Command::EditRedo);
            }
        }
        // The scene editor's keys: V and G pick the tool, Escape lets go.
        if self.studio.screen == Screen::Game && frozen && !dialog && !typing {
            let commands = match &self.studio.open {
                Some(open) if open.scene.is_some() => self.scene.keys(ui, open, self.placement),
                _ => Vec::new(),
            };
            for command in commands {
                self.run_and_say(command);
            }
        }
        // The keys every game has in its own window. They still reach the
        // game too, as they do there.
        if self.studio.screen == Screen::Game && !dialog && !typing {
            let (f1, f3, f12) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::F1),
                    i.key_pressed(egui::Key::F3),
                    i.key_pressed(egui::Key::F12),
                )
            });
            if f1 {
                self.run_and_say(Command::GameDebugOverlay { on: None });
            }
            if f3 {
                // The Studio's own frame counter is the Timing line.
                self.studio.settings.show_timing = !self.studio.settings.show_timing;
            }
            if f12 {
                self.run_and_say(Command::GameScreenshot);
            }
        }
        // Another window became the active one: freeze, and stay frozen.
        if !ui.input(|i| i.focused) && self.is_playing() {
            self.run(Command::GameFreeze);
        }

        // What's really held right now, for carrying on after a freeze.
        let (events, modifiers, pointer) = ui.input(|i| {
            self.studio.keys_down = i
                .keys_down
                .iter()
                .filter_map(|key| keys::key(*key))
                .collect();
            self.studio.buttons_down = [
                egui::PointerButton::Primary,
                egui::PointerButton::Secondary,
                egui::PointerButton::Middle,
            ]
            .into_iter()
            .filter(|button| i.pointer.button_down(*button))
            .filter_map(keys::button)
            .collect();
            (i.events.clone(), i.modifiers, i.pointer.latest_pos())
        });

        // The game's inputs for this frame, and its tick.
        let mut actions: Vec<InputAction> = Vec::new();
        if self.is_playing() && !dialog {
            if !typing {
                actions.extend(keys::modifier_changes(self.modifiers, modifiers));
            }
            actions.extend(keys::game_inputs(
                &events,
                self.placement,
                pointer,
                !typing,
                self.holding_pointer,
            ));
        }
        self.modifiers = modifiers;
        self.studio.begin_frame(now, &actions);

        match self.studio.screen {
            Screen::Home => self.home_screen(ui),
            Screen::Game => self.game_screen(ui),
        }
        self.dialogs(&ctx);
        self.hold_or_free_the_pointer(&ctx, &events);
        // The new version's installer has started: close, so it can update.
        if self.update.poll() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // Draw again soon: at once while the game plays, now and then
        // while something is running or the code might change.
        match self.play_state() {
            Some(PlayState::Playing | PlayState::Starting | PlayState::Replaying) => {
                ctx.request_repaint();
            }
            _ if self.studio.is_busy() || self.studio.making.is_some() => {
                ctx.request_repaint_after(Duration::from_millis(50));
            }
            _ => ctx.request_repaint_after(Duration::from_millis(250)),
        }
    }

    fn home_screen(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("home bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Duck Forge Studio");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("About").clicked() {
                        self.about_open = true;
                    }
                    self.settings_menu(ui);
                    self.theme_menu(ui);
                    self.console_toggle(ui);
                });
            });
        });
        if self.update.shows() {
            let chosen = egui::Panel::top("new version")
                .show(ui, |ui| update_banner::banner(ui, &self.update))
                .inner;
            match chosen {
                Some(BannerChoice::WhatsNew(page)) => {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(page));
                }
                Some(BannerChoice::Update) => self.update.update(ui.ctx()),
                Some(BannerChoice::NotNow) => self.update = UpdateState::Dismissed,
                None => {}
            }
        }
        self.console_panel(ui);
        let mut choice = None;
        egui::CentralPanel::default().show(ui, |ui| {
            // Without a games folder, or without Rust, say so first.
            if let Some(problem) = self.studio.folder_problem.clone() {
                let chosen = panels::problem(
                    ui,
                    "Where is Duck Forge?",
                    &problem,
                    &["Choose the Duck Forge folder"],
                    &mut self.show_details,
                );
                if matches!(chosen, Some(ProblemChoice::Button(_))) {
                    choice = Some(HomeChoice::ChooseFolder);
                }
                return;
            }
            if let Err(problem) = self.studio.rust.clone() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    self.rust_problem(ui, &problem);
                });
            }
            if let Some(making) = &self.studio.making {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("Making {}...", making.title));
                });
            }
            if let Some(problem) = self.studio.problem.clone() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    let chosen = panels::problem(
                        ui,
                        "That didn't work",
                        &problem,
                        &["OK"],
                        &mut self.show_details,
                    );
                    self.handle_problem_choice(ui.ctx(), chosen, |app| app.studio.problem = None);
                });
            }
            choice = home::home(ui, &self.studio, &mut self.pictures);
        });
        match choice {
            Some(HomeChoice::Open(name)) => {
                self.game_texture.forget();
                self.check_open = false;
                self.run(Command::GameOpen { name });
            }
            Some(HomeChoice::NewGame) => self.new_game = Some(NewGame::default()),
            Some(HomeChoice::Dialog(dialog)) => self.game_dialog = Some(dialog),
            Some(HomeChoice::OpenFolder) => {
                if let Some(folder) = &self.studio.folder {
                    open_folder(folder.root());
                }
            }
            Some(HomeChoice::ChooseFolder) => {
                if let Some(folder) = rfd::FileDialog::new()
                    .set_title("Choose the Duck Forge folder")
                    .pick_folder()
                {
                    self.run(Command::ChooseFolder { folder });
                }
            }
            None => {}
        }
    }

    /// The "Duck Forge needs Rust" panel.
    fn rust_problem(&mut self, ui: &mut egui::Ui, problem: &Problem) {
        let chosen = panels::problem(
            ui,
            "Duck Forge needs Rust",
            problem,
            &["Check again"],
            &mut self.show_details,
        );
        self.handle_problem_choice(ui.ctx(), chosen, |app| app.run(Command::CheckAgain));
    }

    /// Opens a problem's page, or does `button` for its own button.
    fn handle_problem_choice(
        &mut self,
        ctx: &egui::Context,
        chosen: Option<ProblemChoice>,
        button: impl FnOnce(&mut StudioApp),
    ) {
        match chosen {
            Some(ProblemChoice::OpenLink(address)) => {
                ctx.open_url(egui::OpenUrl::new_tab(address));
            }
            Some(ProblemChoice::Button(_)) => button(self),
            None => {}
        }
    }

    /// Settings: for now, whether to look for new versions.
    fn settings_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Settings", |ui| {
            let mut check = self.studio.settings.check_for_updates;
            if ui
                .checkbox(&mut check, "Tell me when a new version is out")
                .on_hover_text(
                    "When the Studio starts, it looks at Duck Forge's releases page on GitHub. \
                     Nothing about you is sent, and nothing is downloaded unless you press \
                     Update.",
                )
                .changed()
            {
                self.studio.settings.check_for_updates = check;
                self.studio.settings.save();
                self.update = if check {
                    UpdateState::check()
                } else {
                    UpdateState::Off
                };
            }
        });
    }

    fn theme_menu(&mut self, ui: &mut egui::Ui) {
        if let Some(chosen) = theme::theme_menu(ui) {
            self.studio.settings.theme = Some(theme::name(chosen).to_string());
            self.studio.settings.save();
        }
    }

    fn console_toggle(&mut self, ui: &mut egui::Ui) {
        let shown = self.studio.settings.show_console;
        if ui
            .selectable_label(shown, "Console")
            .on_hover_text("The commands behind the buttons, with their real output")
            .clicked()
        {
            self.studio.settings.show_console = !shown;
            self.studio.settings.save();
        }
    }

    fn console_panel(&mut self, ui: &mut egui::Ui) {
        if !self.studio.settings.show_console {
            return;
        }
        let typed = egui::Panel::bottom("console")
            .resizable(true)
            .default_size(240.0)
            .min_size(140.0)
            .show(ui, |ui| {
                panels::console(ui, &self.studio.console, &mut self.console_line)
            })
            .inner;
        if let Some(line) = typed {
            self.run(Command::ConsoleRun { line });
        }
    }

    fn game_screen(&mut self, ui: &mut egui::Ui) {
        self.take_dropped_files(ui.ctx());
        let title = self
            .studio
            .open
            .as_ref()
            .map_or(String::new(), |open| open.game.title.clone());
        let frozen = matches!(self.play_state(), Some(PlayState::Frozen));
        let playing = self.is_playing();

        egui::Panel::top("game bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("< Home").clicked() {
                    self.run(Command::HomeOpen);
                    self.game_texture.forget();
                    self.placement = None;
                }
                ui.heading(&title);
                ui.add_space(16.0);
                if ui
                    .selectable_label(frozen, "Build")
                    .on_hover_text("Freeze the game on this frame (Tab)")
                    .clicked()
                    && playing
                {
                    self.run(Command::GameFreeze);
                }
                if ui
                    .selectable_label(playing, "Play")
                    .on_hover_text("Carry on from this frame (Tab)")
                    .clicked()
                    && !playing
                {
                    self.run(Command::GamePlay);
                }
                if frozen {
                    ui.add_space(16.0);
                    let undo = self
                        .studio
                        .open
                        .as_ref()
                        .and_then(|o| o.undo.next_undo().map(String::from));
                    let redo = self
                        .studio
                        .open
                        .as_ref()
                        .and_then(|o| o.undo.next_redo().map(String::from));
                    let undo_button = ui
                        .add_enabled(undo.is_some(), egui::Button::new("Undo"))
                        .on_hover_text(undo.map_or(String::from("Nothing to undo"), |what| {
                            format!("Undo: {what} (Ctrl+Z)")
                        }));
                    if undo_button.clicked() {
                        self.run_and_say(Command::EditUndo);
                    }
                    let redo_button = ui
                        .add_enabled(redo.is_some(), egui::Button::new("Redo"))
                        .on_hover_text(redo.map_or(String::from("Nothing to redo"), |what| {
                            format!("Redo: {what} (Ctrl+Y)")
                        }));
                    if redo_button.clicked() {
                        self.run_and_say(Command::EditRedo);
                    }
                    if ui
                        .selectable_label(self.history_open, "History")
                        .on_hover_text("Snapshots of the game's files: go back to any of them")
                        .clicked()
                    {
                        self.history_open = !self.history_open;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.console_toggle(ui);
                    let timing = self.studio.settings.show_timing;
                    if ui
                        .selectable_label(timing, "Timing")
                        .on_hover_text("Frames per second and how long the game and the link take")
                        .clicked()
                    {
                        self.studio.settings.show_timing = !timing;
                        self.studio.settings.save();
                    }
                    ui.separator();
                    let popped = self.play_state() == Some(&PlayState::PoppedOut);
                    let words = if popped { "Bring it back" } else { "Pop out" };
                    if ui
                        .button(words)
                        .on_hover_text("Play the game in its own window, from this frame")
                        .clicked()
                    {
                        self.run(if popped {
                            Command::GameBringBack
                        } else {
                            Command::GamePopOut
                        });
                    }
                    let making_clip = self.studio.open.as_ref().is_some_and(|o| o.clip.is_some());
                    if ui
                        .add_enabled(!making_clip, egui::Button::new("Clip"))
                        .on_hover_text("Save the last ten seconds as a video")
                        .clicked()
                    {
                        self.clip_seen = false;
                        self.run(Command::ClipMake);
                    }
                    if ui
                        .selectable_label(self.check_open, "Check")
                        .on_hover_text("Build it, run its tests, replay it, try random keys")
                        .clicked()
                    {
                        self.check_open = !self.check_open;
                        let open = self.studio.open.as_ref();
                        let never =
                            open.is_some_and(|o| o.check.is_none() && o.check_result.is_none());
                        if self.check_open && never {
                            self.run(Command::CheckRun);
                        }
                    }
                });
            });
        });
        self.notes_strip(ui);
        self.status_bar(ui);
        self.console_panel(ui);
        self.check_panel(ui);
        if frozen {
            self.build_panel(ui);
            // Check takes Feel's place while it's open, so the game keeps
            // its room (Check's lines name places in the level).
            if !self.check_open {
                self.feel_panel(ui);
            }
        }
        if frozen && self.overlay().is_none() {
            let mut commands = Vec::new();
            if let Some(open) = &self.studio.open
                && open.scene.is_some()
            {
                egui::Panel::top("view tools").show(ui, |ui| {
                    commands = self.scene.tool_bar(ui, open);
                });
                // Picking Select or Move while the tile map has the view
                // gives the view back to the objects.
                let picked = commands
                    .iter()
                    .any(|command| matches!(command, Command::ViewTool { .. }));
                if picked && crate::scene_panel::tile_map_selected(open) {
                    commands.push(Command::SelectNone);
                }
            }
            for command in commands {
                self.run_and_say(command);
            }
        }
        egui::CentralPanel::no_frame().show(ui, |ui| self.game_area(ui));
        if frozen {
            let commands = match &self.studio.open {
                Some(open) => self.scene.add_window(ui.ctx(), open),
                None => Vec::new(),
            };
            for command in commands {
                self.run_and_say(command);
            }
            // Add's New script: Files asks for its name.
            if self.scene.take_new_script() {
                let scripts_folder = self
                    .studio
                    .open
                    .as_ref()
                    .and_then(|open| open.game.folder.as_ref())
                    .is_some_and(|folder| folder.join("scripts").is_dir());
                self.files
                    .new_script(if scripts_folder { "scripts" } else { "" });
            }
        }
        if let Some(text) = self.copied.take() {
            ui.ctx().copy_text(text);
        }
    }

    /// Files dropped on a game's screen from File Explorer are copied into
    /// its folder, and the Files panel shows them. While they're being
    /// dragged over, the window says where they'll go.
    fn take_dropped_files(&mut self, ctx: &egui::Context) {
        let Some(open) = &self.studio.open else {
            return;
        };
        let Some(folder) = open.game.folder.clone() else {
            return;
        };
        // While frozen in a game with scenes, pictures and sounds dropped
        // on it become objects of the scene, where they're let go.
        let into_the_scene = open.scene.is_some()
            && self.play_state() == Some(&PlayState::Frozen)
            && self.placement.is_some();
        if files_panel::being_dragged_in(ctx) {
            let words = if into_the_scene {
                String::from(
                    "Drop pictures and sounds to add them to the scene. They're copied into \
                     assets/ first.",
                )
            } else {
                format!("Drop to copy into {}'s folder", open.game.title)
            };
            let screen = ctx.content_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::unique("dropping files"),
            ));
            painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha(150));
            painter.text(
                screen.center(),
                egui::Align2::CENTER_CENTER,
                words,
                egui::FontId::proportional(28.0),
                theme::DUCK_YELLOW,
            );
        }
        let dropped = files_panel::dropped(ctx);
        if dropped.is_empty() {
            return;
        }
        let (usable, others) = if into_the_scene {
            files_panel::usable_in_a_scene(&dropped)
        } else {
            (Vec::new(), dropped)
        };
        let mut commands = Vec::new();
        if !others.is_empty() {
            let selected = open.files_selected.clone();
            commands.push(files_panel::add_dropped(
                &folder,
                selected.as_deref(),
                others,
            ));
        }
        // Where it was let go; Windows doesn't always say, and then it's
        // the middle of the view. More than one go side by side.
        let pointer = ctx.input(|i| i.pointer.latest_pos());
        let mut at = match (self.placement, pointer) {
            (Some(placement), Some(pointer)) if placement.rect.contains(pointer) => {
                self.scene.scene_point(open, placement, pointer)
            }
            _ => None,
        };
        for file in usable {
            let picture = !file
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("wav"));
            commands.push(Command::FilesUse {
                path: file.to_string_lossy().into_owned(),
                at: at.filter(|_| picture),
                ids: Vec::new(),
                key: None,
            });
            if picture && let Some([x, y]) = at {
                at = Some([x + 32.0, y]);
            }
        }
        for command in commands {
            self.run_and_say(command);
        }
    }

    /// The game's own notes ("Inside the Studio, Duck Craft starts a fresh
    /// island and doesn't save"), under the top bar.
    fn notes_strip(&mut self, ui: &mut egui::Ui) {
        let notes: Vec<String> = self
            .studio
            .play()
            .map_or(Vec::new(), |play| play.notes.clone());
        if notes.is_empty() {
            return;
        }
        egui::Panel::top("notes").show(ui, |ui| {
            for note in notes {
                ui.label(RichText::new(note).color(theme::LOOK));
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let mut restart = None;
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                let Some(open) = &self.studio.open else {
                    return;
                };
                // What the game is doing.
                match open.play.as_ref().map(|play| (&play.state, play.frame)) {
                    Some((PlayState::Playing, frame)) => {
                        ui.label(format!("Playing   frame {frame}"));
                        ui.label(RichText::new("Tab: freeze and build").weak());
                    }
                    Some((PlayState::Frozen, frame)) => {
                        ui.label(format!("Frozen on frame {frame}"));
                        ui.label(RichText::new("Tab: play on from here").weak());
                        // Where the pointer is, said the way Check says
                        // places.
                        if let Some((column, row)) = self.build.pointer_on {
                            ui.separator();
                            ui.label(format!("column {column}, row {row}"));
                        }
                    }
                    Some((PlayState::PoppedOut, _)) => {
                        ui.label("Playing in its own window");
                    }
                    Some((PlayState::Replaying, _)) => {
                        ui.label("Replaying your moves...");
                    }
                    Some((PlayState::Starting, _)) => {
                        ui.label("Starting the game...");
                    }
                    Some((PlayState::Stopped { .. }, _)) => {
                        ui.label("The game isn't running");
                    }
                    None => {}
                }
                // The build.
                if let Some(build) = &open.build {
                    ui.separator();
                    match &build.state {
                        BuildState::Running => {
                            ui.spinner();
                            match build.progress() {
                                Some(progress) => {
                                    ui.label(format!(
                                        "Building: {} of {} parts",
                                        build.parts.min(build.parts_expected),
                                        build.parts_expected
                                    ));
                                    ui.add(egui::ProgressBar::new(progress).desired_width(120.0));
                                }
                                None => {
                                    ui.label(format!(
                                        "Building for the first time ({} parts so far). This \
                                         takes a few minutes; later builds take seconds.",
                                        build.parts
                                    ));
                                }
                            }
                        }
                        BuildState::Done { .. } if open.new_build_ready => {
                            ui.label(RichText::new("A new build is ready.").color(theme::GOOD));
                            if ui.add(theme::primary_button("Restart")).clicked() {
                                restart = Some(Command::GameRestart);
                            }
                            if ui.button("Restart and replay my moves").clicked() {
                                restart = Some(Command::GameReplayMyMoves);
                            }
                        }
                        BuildState::Done { .. } => {
                            ui.label(RichText::new("Built").weak());
                        }
                        BuildState::Failed(_) => {
                            ui.label(RichText::new("The last build failed").color(theme::BAD));
                        }
                    }
                }
                if open.check.is_some() {
                    ui.separator();
                    ui.spinner();
                    ui.label("Checking...");
                }
                if open.clip.is_some() {
                    ui.separator();
                    ui.spinner();
                    ui.label("Making a clip...");
                }
                if let Some((said, when)) = &self.said
                    && when.elapsed().as_secs() < 5
                {
                    ui.separator();
                    ui.label(RichText::new(said).color(theme::GOOD));
                }
            });
            if self.studio.settings.show_timing {
                let timing = self.studio.timing(Instant::now());
                let renderer = self
                    .studio
                    .play()
                    .map_or(String::new(), |play| play.drawn_by());
                ui.label(
                    RichText::new(format!(
                        "Timing (last 5 s): {:.1} updates a second, {:.1} pictures a second, \
                         longest gap {:.1} ms, game {:.2} ms, link {:.2} ms, late {} of {}.  \
                         Drawn by: {}",
                        timing.updates_per_second,
                        timing.pictures_per_second,
                        timing.longest_gap,
                        timing.game,
                        timing.link,
                        timing.late,
                        timing.ticks,
                        renderer
                    ))
                    .monospace()
                    .small(),
                );
            }
        });
        if let Some(command) = restart {
            self.game_texture.forget();
            self.run(command);
        }
    }

    fn check_panel(&mut self, ui: &mut egui::Ui) {
        if !self.check_open {
            return;
        }
        let Some(open) = &self.studio.open else {
            return;
        };
        let title = open.game.title.clone();
        let running = open
            .getting_ready()
            .or(open.check.as_ref().map(|check| check.doing()));
        let result = open.check_result.clone();
        let shown = open.shown_line;
        let choice = egui::Panel::right("check")
            .resizable(true)
            .default_size(380.0)
            .show(ui, |ui| {
                panels::check(
                    ui,
                    &title,
                    running,
                    result.as_ref(),
                    shown,
                    &mut self.pictures,
                    &mut self.show_details,
                )
            })
            .inner;
        match choice {
            Some(CheckChoice::CheckAgain) => self.run(Command::CheckRun),
            Some(CheckChoice::Close) => self.check_open = false,
            Some(CheckChoice::RecordAgain) => self.run_and_say(Command::CheckRecordAgain),
            Some(CheckChoice::Show(line)) => self.run(Command::CheckShowProblem { line }),
            None => {}
        }
    }

    /// Build mode's own panel, while the game is frozen. The level editor
    /// arrives in the next step; for now it steps the frozen game.
    fn build_panel(&mut self, ui: &mut egui::Ui) {
        let mut commands = Vec::new();
        let frame = self.studio.play().map_or(0, |play| play.frame);
        egui::Panel::left("build")
            .default_size(230.0)
            .min_size(200.0)
            .show(ui, |ui| {
                // A game with scenes: Files under the Scene list, to drag
                // pictures and sounds from into the scene.
                let scene_files = self
                    .studio
                    .open
                    .as_ref()
                    .filter(|open| open.scene.is_some() && open.game.folder.is_some());
                if let Some(open) = scene_files {
                    egui::Panel::bottom("files of a scene game")
                        .resizable(true)
                        .default_size(260.0)
                        .min_size(90.0)
                        .show(ui, |ui| {
                            ui.add_space(4.0);
                            ui.heading("Files");
                            // The game's scripts can be dragged onto objects.
                            self.files.scripts = open
                                .game
                                .folder
                                .as_deref()
                                .and_then(studio_core::scripts::ScriptsFile::read)
                                .map(|list| list.entries.into_iter().map(|e| e.path).collect())
                                .unwrap_or_default();
                            egui::ScrollArea::vertical()
                                .id_salt("files under the scene list")
                                .show(ui, |ui| {
                                    commands.extend(self.files.show(
                                        ui,
                                        open.game.folder.as_deref(),
                                        open.files_selected.as_deref(),
                                        &mut self.pictures,
                                        true,
                                    ));
                                });
                        });
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Build");
                    ui.label(
                        RichText::new(format!(
                            "Frozen on frame {frame}. Edits are saved as you make them. Tab plays on."
                        ))
                        .weak(),
                    );
                    ui.add_space(6.0);
                    if let Some(open) = &self.studio.open {
                        if open.level_needs_restart {
                            ui.label(
                                RichText::new(
                                    "This game reads its level only when it starts. Restart it to \
                                     see your changes.",
                                )
                                .color(theme::LOOK),
                            );
                            if ui.add(theme::primary_button("Restart")).clicked() {
                                commands.push(Command::GameRestart);
                            }
                        }
                        if let Some(problem) = &open.level_problem {
                            ui.label(RichText::new(&problem.what).color(theme::BAD));
                            ui.label(&problem.next);
                        }
                        if open.scene.is_some() {
                            // A game with scenes: its objects, and the
                            // paint tools when the tile map is the one
                            // selected.
                            commands.extend(self.scene.list(ui, open));
                            if crate::scene_panel::tile_map_selected(open) && open.level.is_some()
                            {
                                ui.separator();
                                ui.label(
                                    RichText::new(
                                        "The tile map is selected: these tools paint on it. \
                                         Esc, or a click on another object in the list, stops.",
                                    )
                                    .weak()
                                    .small(),
                                );
                                commands.extend(self.build.tools(ui, open));
                            }
                        } else if open.level.is_some() {
                            commands.extend(self.build.tools(ui, open));
                        } else if open.level_problem.is_none() {
                            build_mode::no_level_to_paint(ui, open.game.folder.is_some());
                        }
                    }
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Play one frame").clicked() {
                            commands.push(Command::GameStep { frames: 1 });
                        }
                        if ui.button("Play ten frames").clicked() {
                            commands.push(Command::GameStep { frames: 10 });
                        }
                        if ui.button("Restart the game").clicked() {
                            commands.push(Command::GameRestart);
                        }
                    });
                });
            });
        for command in commands {
            self.run_and_say(command);
        }
    }

    /// Build mode over the game's frozen picture (or the whole level): the
    /// grid, the tile under the pointer, and the tools at work.
    fn build_on_the_level(&mut self, ui: &mut egui::Ui, space: egui::Rect) -> bool {
        let Some(open) = &self.studio.open else {
            return false;
        };
        let Some(level) = &open.level else {
            return false;
        };
        if open.scene.is_some() && !crate::scene_panel::tile_map_selected(open) {
            return false; // the view belongs to the scene's objects
        }
        let (grid, response, clip) = if self.build.whole_level {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(space));
            child
                .painter()
                .rect_filled(space, 0.0, egui::Color32::BLACK);
            match self.build.whole_level_view(&mut child, open) {
                Some((grid, response)) => (grid, response, space),
                None => return false,
            }
        } else {
            let Some(placement) = self.placement else {
                return false;
            };
            let Some(grid) = build_mode::Grid::over_the_game(open, placement) else {
                return false;
            };
            let response = ui.interact(
                placement.rect,
                ui.scope_id().with("build mode level"),
                egui::Sense::click_and_drag(),
            );
            (grid, response, placement.rect)
        };
        let hovered = response.hover_pos().and_then(|place| grid.cell_at(place));
        self.build.draw_on_top(ui, clip, grid, hovered);
        if let Some(cell) = hovered {
            let what = level
                .letter(level.letter_at(cell))
                .map_or(String::from("empty"), |l| l.title());
            response
                .clone()
                .on_hover_text(format!("Column {}, row {}: {what}", cell.0, cell.1));
        }
        let commands = self.build.use_tool(ui, &response, grid, level);
        for command in commands {
            self.run_and_say(command);
        }
        true
    }

    /// The middle of the game screen: the game, or what's in its way.
    fn game_area(&mut self, ui: &mut egui::Ui) {
        let space = ui.available_rect_before_wrap();
        // Wait for this frame's picture, so it's shown without a frame's delay.
        self.studio.wait_for_picture(studio_core::play::PATIENCE);
        let picture = self.studio.play().and_then(|play| play.picture.clone());
        let overlay = self.overlay();
        let frozen = self.play_state() == Some(&PlayState::Frozen);
        if frozen
            && overlay.is_none()
            && self.build.whole_level
            && self.build_on_the_level(ui, space)
        {
            self.clip_result(ui.ctx());
            return;
        }
        match &picture {
            Some(picture) => {
                let dimmed = overlay.is_some();
                self.placement = Some(game_view::paint(
                    ui,
                    space,
                    &mut self.game_texture,
                    picture,
                    dimmed,
                ));
            }
            None => {
                ui.painter().rect_filled(space, 0.0, egui::Color32::BLACK);
                self.placement = None;
            }
        }
        if frozen && overlay.is_none() && !self.build_on_the_level(ui, space) {
            self.scene_on_the_view(ui);
        }
        if let Some(overlay) = overlay {
            self.show_overlay(ui, space, overlay);
        }
        self.clip_result(ui.ctx());
    }

    /// The scene editor over the game's frozen picture: selecting and
    /// moving objects.
    fn scene_on_the_view(&mut self, ui: &mut egui::Ui) {
        let (Some(open), Some(placement)) = (&self.studio.open, self.placement) else {
            return;
        };
        if open.scene.is_none() {
            return;
        }
        let commands = self.scene.on_the_view(ui, open, placement);
        for command in commands {
            self.run_and_say(command);
        }
    }

    /// What should be shown over the game, if anything.
    fn overlay(&self) -> Option<Overlay> {
        let open = self.studio.open.as_ref()?;
        if let Err(problem) = &self.studio.rust {
            return Some(Overlay::NeedsRust(problem.clone()));
        }
        if let Some(problem) = open.build_problem() {
            // A game that's still running on its last good build keeps
            // playing; the problem then waits in the status bar's place.
            let running = open.play.as_ref().is_some_and(|play| play.is_running());
            if !running {
                return Some(Overlay::BuildFailed(problem.clone()));
            }
        }
        let Some(play) = &open.play else {
            return open.is_building().then_some(Overlay::Building);
        };
        if play.not_answering {
            return Some(Overlay::NotAnswering);
        }
        match &play.state {
            PlayState::Starting => Some(Overlay::Words("Starting the game...")),
            PlayState::Replaying => Some(Overlay::Words("Replaying your moves...")),
            PlayState::PoppedOut => Some(Overlay::PoppedOut),
            PlayState::Stopped {
                ending: Ending::Crashed,
                problem: Some(problem),
            } => Some(Overlay::Stopped(problem.clone())),
            PlayState::Stopped { .. } => Some(Overlay::Ended),
            PlayState::Playing | PlayState::Frozen => {
                self.studio.problem.clone().map(Overlay::Problem)
            }
        }
    }

    fn show_overlay(&mut self, ui: &mut egui::Ui, space: egui::Rect, overlay: Overlay) {
        let width = 460.0_f32.min(space.width() - 24.0);
        let area = egui::Rect::from_center_size(space.center(), egui::vec2(width, 10.0));
        let mut command = None;
        let mut dismiss = false;
        let mut check_again = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(width - 24.0);
                match &overlay {
                    Overlay::NeedsRust(problem) => {
                        let chosen = panels::problem(
                            ui,
                            "Duck Forge needs Rust",
                            problem,
                            &["Check again"],
                            &mut self.show_details,
                        );
                        match chosen {
                            Some(ProblemChoice::OpenLink(address)) => {
                                ui.ctx().open_url(egui::OpenUrl::new_tab(address));
                            }
                            Some(ProblemChoice::Button(_)) => check_again = true,
                            None => {}
                        }
                    }
                    Overlay::BuildFailed(problem) => {
                        let chosen = panels::problem(
                            ui,
                            "The game couldn't be built",
                            problem,
                            &["Try again"],
                            &mut self.show_details,
                        );
                        match chosen {
                            Some(ProblemChoice::OpenLink(address)) => {
                                ui.ctx().open_url(egui::OpenUrl::new_tab(address));
                            }
                            Some(ProblemChoice::Button(_)) => command = Some(Command::GameBuild),
                            None => {}
                        }
                    }
                    Overlay::Stopped(problem) => {
                        let chosen = panels::problem(
                            ui,
                            "The game stopped",
                            problem,
                            &["Restart", "Restart and replay my moves"],
                            &mut self.show_details,
                        );
                        match chosen {
                            Some(ProblemChoice::Button("Restart")) => {
                                command = Some(Command::GameRestart);
                            }
                            Some(ProblemChoice::Button(_)) => {
                                command = Some(Command::GameReplayMyMoves);
                            }
                            _ => {}
                        }
                    }
                    Overlay::Problem(problem) => {
                        let chosen = panels::problem(
                            ui,
                            "That didn't work",
                            problem,
                            &["OK"],
                            &mut self.show_details,
                        );
                        match chosen {
                            Some(ProblemChoice::OpenLink(address)) => {
                                ui.ctx().open_url(egui::OpenUrl::new_tab(address));
                            }
                            Some(ProblemChoice::Button(_)) => dismiss = true,
                            None => {}
                        }
                    }
                    Overlay::NotAnswering => {
                        ui.heading("The game isn't answering");
                        ui.label(
                            "It hasn't sent a picture for two seconds. It may be busy, or stuck \
                             in a loop.",
                        );
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Waiting...").weak());
                            if ui.add(theme::primary_button("Restart")).clicked() {
                                command = Some(Command::GameRestart);
                            }
                        });
                    }
                    Overlay::PoppedOut => {
                        ui.heading("Playing in its own window");
                        ui.label("The game carried on there from the frame it was on.");
                        ui.add_space(8.0);
                        if ui.add(theme::primary_button("Bring it back")).clicked() {
                            command = Some(Command::GameBringBack);
                        }
                    }
                    Overlay::Ended => {
                        ui.heading("The game has ended");
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.add(theme::primary_button("Play again")).clicked() {
                                command = Some(Command::GameRestart);
                            }
                            if ui.button("Replay my moves").clicked() {
                                command = Some(Command::GameReplayMyMoves);
                            }
                        });
                    }
                    Overlay::Building => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.heading("Building the game...");
                        });
                        ui.label("It starts playing here as soon as it's built.");
                    }
                    Overlay::Words(words) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(*words);
                        });
                    }
                }
            });
        });
        if check_again {
            self.run(Command::CheckAgain);
        }
        if dismiss {
            self.studio.problem = None;
        }
        if let Some(command) = command {
            self.studio.problem = None;
            self.game_texture.forget();
            self.run(command);
        }
    }

    /// A small window saying where the finished clip is.
    fn clip_result(&mut self, ctx: &egui::Context) {
        let Some(result) = self
            .studio
            .open
            .as_ref()
            .and_then(|open| open.clip_result.clone())
        else {
            return;
        };
        if self.clip_seen {
            return;
        }
        let mut close = false;
        egui::Window::new("Clip")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::RIGHT_TOP, [-12.0, 48.0])
            .show(ctx, |ui| match &result {
                Ok(clip) => {
                    let name = clip
                        .file
                        .file_name()
                        .map_or(String::new(), |name| name.to_string_lossy().into_owned());
                    ui.label(RichText::new(format!("Saved: {name}")).strong());
                    ui.label(format!(
                        "In {}",
                        clip.file
                            .parent()
                            .map_or(String::new(), |p| p.display().to_string())
                    ));
                    if clip.needs_ffmpeg {
                        ui.add_space(4.0);
                        ui.label(
                            "It's a GIF with no sound, because ffmpeg isn't installed. Install \
                             ffmpeg for sound.",
                        );
                        ui.hyperlink_to("Open the ffmpeg page", studio_core::clip::FFMPEG_PAGE);
                    } else if clip.with_sound {
                        ui.label("A video with sound.");
                    } else {
                        ui.label("A video (this run made no sound).");
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add(theme::primary_button("Show it in its folder"))
                            .clicked()
                        {
                            show_in_folder(&clip.file);
                        }
                        if ui.button("Close").clicked() {
                            close = true;
                        }
                    });
                }
                Err(problem) => {
                    let chosen = panels::problem(
                        ui,
                        "The clip couldn't be made",
                        problem,
                        &["OK"],
                        &mut self.show_details,
                    );
                    match chosen {
                        Some(ProblemChoice::OpenLink(address)) => {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(address));
                        }
                        Some(ProblemChoice::Button(_)) => close = true,
                        None => {}
                    }
                }
            });
        if close {
            self.clip_seen = true;
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.studio.screen == Screen::Game
            && let Some(open) = &self.studio.open
        {
            let mut command = None;
            if self.history_open {
                command = feel_panel::history(ctx, open, &mut self.history_open);
            }
            if self.code_open
                && let Some(file) = feel_panel::code_spot(ctx, open, &mut self.code_open)
            {
                feel_panel::open_with_the_system(&file);
            }
            if self.recipe_open {
                feel_panel::recipe(ctx, open, &mut self.recipe_open);
            }
            if let Some(command) = command {
                self.run_and_say(command);
            }
        }
        if let Some(dialog) = &mut self.game_dialog {
            match home::game_dialog(ctx, dialog, &self.studio) {
                Some(GameDialogChoice::Cancel) => self.game_dialog = None,
                Some(GameDialogChoice::Run(command)) => {
                    self.game_dialog = None;
                    self.run_and_say(command);
                }
                None => {}
            }
        }
        if let Some(dialog) = &mut self.new_game {
            match home::new_game(ctx, dialog, &self.studio, &mut self.pictures) {
                Some(NewGameChoice::Cancel) => self.new_game = None,
                Some(NewGameChoice::Make { starter, name }) => {
                    self.new_game = None;
                    self.run(Command::GameNew { starter, name });
                }
                None => {}
            }
        }
        if self.about_open {
            let mut close = false;
            egui::Modal::new(egui::Id::unique("about")).show(ctx, |ui| {
                ui.set_width(360.0);
                ui.heading("Duck Forge Studio");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.add_space(6.0);
                ui.label(
                    "Make games without a terminal. The game plays inside the Studio; Tab flips \
                     between Build and Play on the same spot.",
                );
                ui.add_space(6.0);
                if let Ok(rust) = &self.studio.rust {
                    ui.label(RichText::new(format!("Rust: {}", rust.version)).weak());
                }
                ui.add_space(8.0);
                if ui.add(theme::primary_button("Close")).clicked() {
                    close = true;
                }
            });
            if close {
                self.about_open = false;
            }
        }
    }

    /// Holds the pointer for the game (hidden, kept in the middle of the
    /// game) while the game asks for it, and gives it back otherwise.
    ///
    /// The game never touches the real pointer inside the Studio: it only
    /// says it wants it held, and the Studio does the holding. So a stuck
    /// game can never keep the pointer.
    fn hold_or_free_the_pointer(&mut self, ctx: &egui::Context, events: &[egui::Event]) {
        let wants = self.is_playing()
            && self.studio.play().is_some_and(|play| play.wants_pointer)
            && ctx.input(|i| i.focused);
        let clicked_the_game = events.iter().any(|event| {
            matches!(event, egui::Event::PointerButton { pos, pressed: true, .. }
                if self.placement.is_some_and(|p| p.rect.contains(*pos)))
        });
        // Taken when the game wants it and you click in the game; kept
        // until the game stops asking, or stops playing.
        let hold = wants && (self.holding_pointer || clicked_the_game);
        let middle = self.placement.map(|placement| placement.rect.center());
        if hold && !self.holding_pointer {
            // Windows can only keep the pointer inside the window, so
            // there it's also put back in the middle every frame (below).
            // Elsewhere it's locked in place where the system can do that.
            let grab = if cfg!(windows) {
                egui::CursorGrab::Confined
            } else {
                egui::CursorGrab::Locked
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(grab));
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(false));
        } else if !hold && self.holding_pointer {
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(egui::CursorGrab::None));
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(true));
        }
        if let (true, Some(middle)) = (hold || self.holding_pointer, middle) {
            // While held, and once more as it's let go: the pointer comes
            // back in the middle of the game, ready for the Build tools.
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorPosition(middle));
        }
        self.holding_pointer = hold;
    }

    /// Whether the Studio is holding the pointer for the game right now.
    /// Where the game's picture was drawn last frame (tests click on it).
    pub fn game_placement(&self) -> Option<Placement> {
        self.placement
    }

    pub fn is_holding_pointer(&self) -> bool {
        self.holding_pointer
    }
}

/// What's shown over the game.
#[derive(Debug, Clone)]
enum Overlay {
    NeedsRust(Problem),
    BuildFailed(Problem),
    Stopped(Problem),
    Problem(Problem),
    NotAnswering,
    PoppedOut,
    Ended,
    Building,
    Words(&'static str),
}

/// Opens the folder a file is in, in the system's file browser.
fn show_in_folder(file: &std::path::Path) {
    if let Some(folder) = file.parent() {
        open_folder(folder);
    }
}

/// Opens a folder in the system's file browser.
fn open_folder(folder: &std::path::Path) {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program).arg(folder).spawn();
}

impl eframe::App for StudioApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        StudioApp::ui(self, ui);
    }

    fn on_exit(&mut self) {
        // Closing the Studio ends the game politely (so it can say
        // goodbye) and saves the settings.
        let _ = self.studio.run(Command::HomeOpen);
        self.studio.settings.save();
    }
}
