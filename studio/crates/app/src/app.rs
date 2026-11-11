//! The app: one frame at a time, it reads what the person did, tells the
//! Studio, and draws what the Studio holds.

use std::time::{Duration, Instant};

use duckforge::InputAction;
use eframe::egui::{self, RichText};
use studio_core::build::BuildState;
use studio_core::play::{Ending, PlayState};
use studio_core::studio::Screen;
use studio_core::{Command, Problem, Studio};

use crate::game_view::{self, GameTexture};
use crate::home::{self, HomeChoice, NewGame, NewGameChoice};
use crate::keys::{self, Placement};
use crate::panels::{self, CheckChoice, ProblemChoice};
use crate::pictures::Pictures;
use crate::theme;

/// How long the Studio waits, within one of its own frames, for the
/// picture of the tick it just sent. If the game answers in time, the
/// picture is shown in this very frame: no added delay.
const PATIENCE: Duration = Duration::from_millis(10);

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
        let dialog = self.new_game.is_some() || self.about_open;
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
                    self.theme_menu(ui);
                    self.console_toggle(ui);
                });
            });
        });
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
        }
        egui::CentralPanel::no_frame().show(ui, |ui| self.game_area(ui));
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
            });
            if self.studio.settings.show_timing {
                let timing = self.studio.timing(Instant::now());
                let renderer = self
                    .studio
                    .play()
                    .map_or(String::new(), |play| play.renderer.clone());
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
                        if renderer.is_empty() { "?" } else { &renderer }
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
            Some(CheckChoice::Show(line)) => self.run(Command::CheckShowProblem { line }),
            None => {}
        }
    }

    /// Build mode's own panel, while the game is frozen. The level editor
    /// arrives in the next step; for now it steps the frozen game.
    fn build_panel(&mut self, ui: &mut egui::Ui) {
        let mut command = None;
        egui::Panel::left("build")
            .default_size(190.0)
            .show(ui, |ui| {
                ui.heading("Build");
                ui.label("The game is frozen on this frame. Nothing moves until you press Tab.");
                ui.add_space(8.0);
                if ui.button("Play one frame").clicked() {
                    command = Some(Command::GameStep { frames: 1 });
                }
                if ui.button("Play ten frames").clicked() {
                    command = Some(Command::GameStep { frames: 10 });
                }
                ui.add_space(8.0);
                if ui.button("Restart the game").clicked() {
                    command = Some(Command::GameRestart);
                }
                ui.add_space(12.0);
                ui.label(
                    RichText::new("Painting the level, Feel and Undo come in the next steps.")
                        .weak(),
                );
            });
        if let Some(command) = command {
            self.run(command);
        }
    }

    /// The middle of the game screen: the game, or what's in its way.
    fn game_area(&mut self, ui: &mut egui::Ui) {
        let space = ui.available_rect_before_wrap();
        // Wait for this frame's picture, so it's shown without a frame's delay.
        self.studio.wait_for_picture(PATIENCE);
        let picture = self.studio.play().and_then(|play| play.picture.clone());
        let overlay = self.overlay();
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
        if let Some(overlay) = overlay {
            self.show_overlay(ui, space, overlay);
        }
        self.clip_result(ui.ctx());
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
    let Some(folder) = file.parent() else {
        return;
    };
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
