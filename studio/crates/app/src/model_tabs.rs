//! A game's 3D models, each in a tab beside the game.
//!
//! A model's tab is Duck Mold's own Studio: the same screens, drawn by
//! the same code (the `duck-mold-studio` library), into the space the
//! tab gives them. Nothing of the model passes between the two programs
//! but files: the tab is given a recipe to open, and says which `.glb`
//! files it wrote, so the Studio can say so. (A running game hears about
//! a new `.glb` the way it hears about a new picture: the Studio looks at
//! the game's assets every frame.)
//!
//! Which tabs are open, and which is in front, is the core's to know
//! (`studio_core::models::Models`), so that commands can change it with
//! no window. This file keeps one Duck Mold Studio for each.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use duck_mold_studio::recent::Recent;
use eframe::egui::{self, RichText};
use studio_core::Command;
use studio_core::models::{Kind, Models, kind_of};

use crate::game_view::GameTexture;

/// How wide the small picture of the game is, in points.
const GAME_PICTURE: f32 = 220.0;

/// The Duck Mold Studios behind a game's model tabs.
#[derive(Default)]
pub struct ModelTabs {
    /// One for each tab, by its place in the game's folder.
    apps: BTreeMap<String, duck_mold_studio::StudioApp>,
    /// The game they were opened for.
    game: Option<PathBuf>,
    /// Keep Duck Mold's own settings (its Undo for each recipe, what was
    /// hidden, how to paint): only the real app does, so that tests never
    /// touch a person's settings.
    pub remembering: bool,
    /// The small picture of the running game is hidden.
    pub game_picture_hidden: bool,
}

impl ModelTabs {
    /// Keeps a Duck Mold Studio for every tab the game has, and lets go
    /// of the ones whose tabs are closed.
    pub fn follow(&mut self, game: Option<&Path>, models: &Models) {
        if self.game.as_deref() != game {
            self.apps.clear();
            self.game = game.map(Path::to_path_buf);
        }
        let Some(game) = game else {
            return;
        };
        self.apps.retain(|place, _| models.tabs.contains(place));
        for place in &models.tabs {
            if self.apps.contains_key(place) {
                continue;
            }
            let file = game.join(place);
            // A model file with no recipe can only be looked at.
            let look_only = kind_of(&file) != Some(Kind::Recipe);
            let mut app = duck_mold_studio::StudioApp::hosted(file, look_only).for_game(game);
            if self.remembering {
                app = app.remembering(Recent::of_this_person());
            }
            self.apps.insert(place.clone(), app);
        }
    }

    /// The Duck Mold Studio behind a tab, if it is open.
    pub fn app(&mut self, place: &str) -> Option<&mut duck_mold_studio::StudioApp> {
        self.apps.get_mut(place)
    }

    /// The row of tabs: the game first, then each model with a cross to
    /// close it. Nothing is drawn while no model is open.
    pub fn strip(&self, ui: &mut egui::Ui, models: &Models) -> Vec<Command> {
        let mut commands = Vec::new();
        if models.tabs.is_empty() {
            return commands;
        }
        egui::Panel::top("model tabs").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(models.front.is_none(), "Game")
                    .on_hover_text("The game itself")
                    .clicked()
                {
                    commands.push(Command::ModelShow { path: None });
                }
                for place in &models.tabs {
                    let name = place.rsplit('/').next().unwrap_or(place);
                    let in_front = models.front.as_deref() == Some(place.as_str());
                    ui.separator();
                    let tab = ui.selectable_label(in_front, name);
                    tab.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::Role::Button,
                            true,
                            in_front,
                            format!("Model {name}"),
                        )
                    });
                    if tab
                        .on_hover_text(format!("{place}: a 3D model, in Duck Mold"))
                        .clicked()
                    {
                        commands.push(Command::ModelShow {
                            path: Some(place.clone()),
                        });
                    }
                    let close = ui.small_button("x");
                    close.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Close {name}"))
                    });
                    if close.on_hover_text("Close this tab").clicked() {
                        commands.push(Command::ModelClose {
                            path: place.clone(),
                        });
                    }
                }
            });
        });
        commands
    }

    /// Draws the tab in front: Duck Mold's Studio, in all the space
    /// left. Returns the `.glb` files it wrote this frame.
    pub fn show(&mut self, ui: &mut egui::Ui, place: &str) -> Vec<PathBuf> {
        let Some(app) = self.apps.get_mut(place) else {
            return Vec::new();
        };
        egui::CentralPanel::no_frame().show(ui, |ui| {
            // Duck Mold's own look for the things the two programs set
            // differently: no text smaller than 12 points, faint text
            // light enough to read, and scroll bars that take their own
            // room.
            let dark = ui.visuals().dark_mode;
            let style = ui.style_mut();
            style.text_styles.insert(
                egui::TextStyle::Small,
                egui::FontId::proportional(duck_mold_studio::theme::SMALLEST_TEXT),
            );
            style.spacing.scroll = egui::style::ScrollStyle::solid();
            style.visuals.weak_text_color = Some(if dark {
                egui::Color32::from_gray(160)
            } else {
                egui::Color32::from_gray(95)
            });
            // Duck Mold's panels, bars and keys, as in its own window.
            duck_mold_studio::StudioApp::ui(app, ui);
        });
        app.take_exported()
    }

    /// The small picture of the game, in a corner of the model's
    /// picture: for looking only. True if it was clicked (to go to the
    /// game).
    pub fn game_picture(
        &mut self,
        ui: &mut egui::Ui,
        place: &str,
        texture: &mut GameTexture,
        picture: Option<&studio_core::play::Picture>,
        running: bool,
    ) -> bool {
        let Some(picture) = picture else {
            return false;
        };
        // Where the model's own picture is drawn.
        let Some((view, _)) = self.apps.get(place).and_then(|app| app.viewport()) else {
            return false;
        };
        let mut go_to_game = false;
        if self.game_picture_hidden {
            egui::Area::new(egui::Id::unique("show the game's picture"))
                .order(egui::Order::Foreground)
                .fixed_pos(view.left_bottom() + egui::vec2(8.0, -64.0))
                .show(ui.ctx(), |ui| {
                    if ui
                        .small_button("Show the game")
                        .on_hover_text("A small picture of the game, here in the corner")
                        .clicked()
                    {
                        self.game_picture_hidden = false;
                    }
                });
            return false;
        }
        let wide = GAME_PICTURE.min(view.width() / 3.0);
        let high = wide * picture.height as f32 / picture.width.max(1) as f32;
        egui::Area::new(egui::Id::unique("the game's picture"))
            .order(egui::Order::Foreground)
            .fixed_pos(view.left_bottom() + egui::vec2(8.0, -(high + 96.0)))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .inner_margin(4.0)
                    .show(ui, |ui| {
                        // No wider than the picture itself.
                        ui.set_max_width(wide);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(if running {
                                    "The game, running"
                                } else {
                                    "The game"
                                })
                                .small()
                                .weak(),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let hide = ui.small_button("Hide");
                                    hide.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::Role::Button,
                                            true,
                                            "Hide the game's picture",
                                        )
                                    });
                                    if hide.clicked() {
                                        self.game_picture_hidden = true;
                                    }
                                },
                            );
                        });
                        let handle = texture.show(ui.ctx(), picture);
                        let image = egui::Image::new((handle.id(), egui::vec2(wide, high)))
                            .sense(egui::Sense::click());
                        let shown = ui.add(image);
                        shown.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::Role::Button,
                                true,
                                "Go to the game",
                            )
                        });
                        if shown
                            .on_hover_text(
                                "The game as it is now, for looking: \"Send to game\" shows the model in it at once. Click to go to the game",
                            )
                            .clicked()
                        {
                            go_to_game = true;
                        }
                    });
            });
        go_to_game
    }
}
