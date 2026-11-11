//! Home: New game, My games, and the examples that came with Duck Forge.

use eframe::egui::{self, Color32, RichText, Sense, vec2};
use studio_core::Studio;
use studio_core::folder::{Card, STARTERS, name_from_title};
use studio_core::studio::HomeCard;

use crate::pictures::Pictures;
use crate::theme;

/// Where Duck Quill and Duck Echo live.
pub const DUCK_QUILL_PAGE: &str = "https://github.com/mrguymrguy647-del/duck-quill";
pub const DUCK_ECHO_PAGE: &str = "https://github.com/mrguymrguy647-del/Duck-Echo";

/// What a person chose on Home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeChoice {
    Open(String),
    NewGame,
    ChooseFolder,
}

/// The size of a card's picture, in points.
const PICTURE: egui::Vec2 = vec2(160.0, 120.0);

/// One game's card. Returns true if it was clicked.
fn card(ui: &mut egui::Ui, card: &HomeCard, pictures: &mut Pictures) -> bool {
    let picture = card
        .picture
        .as_ref()
        .and_then(|path| pictures.get(ui.ctx(), path));
    let inner = egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(PICTURE.x);
        ui.vertical(|ui| {
            picture_or_blank(ui, picture.as_ref(), &card.game.title);
            ui.label(RichText::new(&card.game.title).strong());
            match &card.card {
                Card::ChecksPass => {
                    ui.label(RichText::new("Checks pass").color(theme::GOOD));
                    ui.label(RichText::new(" ").small());
                }
                Card::NeedsALook(why) => {
                    ui.label(RichText::new("Needs a look").color(theme::LOOK));
                    ui.label(RichText::new(why).small().weak());
                }
            }
        });
    });
    let response = inner.response.interact(Sense::click());
    if response.hovered() {
        ui.painter().rect_stroke(
            response.rect,
            4.0,
            egui::Stroke::new(2.0, theme::NIGHT_INDIGO),
            egui::StrokeKind::Outside,
        );
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// Cards in rows, as many to a row as fit. Returns the name of the game
/// whose card was clicked.
fn cards(ui: &mut egui::Ui, games: &[&HomeCard], pictures: &mut Pictures) -> Option<String> {
    let mut clicked = None;
    let each = PICTURE.x + 24.0; // a card, its frame and the gap after it
    let across = ((ui.available_width() - 12.0) / each).floor().max(1.0) as usize;
    for row in games.chunks(across) {
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            for game in row {
                if card(ui, game, pictures) {
                    clicked = Some(game.game.name.clone());
                }
            }
        });
        ui.add_space(6.0);
    }
    clicked
}

/// A picture, or a plain tile with the name's first letters where there
/// isn't one yet.
fn picture_or_blank(ui: &mut egui::Ui, picture: Option<&egui::TextureHandle>, title: &str) {
    let (rect, _) = ui.allocate_exact_size(PICTURE, Sense::hover());
    match picture {
        Some(texture) => {
            let whole = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter()
                .image(texture.id(), rect, whole, Color32::WHITE);
        }
        None => {
            ui.painter().rect_filled(rect, 4.0, theme::INK_NAVY);
            let letters: String = title
                .split(' ')
                .filter_map(|word| word.chars().next())
                .take(2)
                .collect();
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                letters,
                egui::FontId::proportional(40.0),
                theme::DUCK_YELLOW,
            );
        }
    }
}

/// The Home screen.
pub fn home(ui: &mut egui::Ui, studio: &Studio, pictures: &mut Pictures) -> Option<HomeChoice> {
    let mut choice = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                let new_game = ui.add_enabled(
                    studio.folder.is_some(),
                    theme::primary_button("+ New game").min_size(vec2(140.0, 36.0)),
                );
                if new_game.clicked() {
                    choice = Some(HomeChoice::NewGame);
                }
                ui.add_space(24.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.hyperlink_to("Duck Quill", DUCK_QUILL_PAGE);
                        ui.label(RichText::new("draw tiles and sprites").weak());
                    });
                    ui.horizontal(|ui| {
                        ui.hyperlink_to("Duck Echo", DUCK_ECHO_PAGE);
                        ui.label(RichText::new("make sounds and music").weak());
                    });
                });
            });
            ui.add_space(12.0);
            if let Some(folder) = &studio.folder {
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(format!("Games folder: {}", folder.root().display())).weak(),
                    );
                    if ui.link("Change").clicked() {
                        choice = Some(HomeChoice::ChooseFolder);
                    }
                });
            }
            ui.add_space(8.0);
            let (mine, came_with): (Vec<&HomeCard>, Vec<&HomeCard>) =
                studio.cards.iter().partition(|card| !card.game.came_with);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.heading("My games");
            });
            ui.add_space(4.0);
            if mine.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label("No games of your own yet. Press New game to make one.");
                });
            }
            if let Some(name) = cards(ui, &mine, pictures) {
                choice = Some(HomeChoice::Open(name));
            }
            ui.add_space(16.0);
            egui::CollapsingHeader::new(
                RichText::new("Examples that came with Duck Forge").heading(),
            )
            .default_open(true)
            .show(ui, |ui| {
                if let Some(name) = cards(ui, &came_with, pictures) {
                    choice = Some(HomeChoice::Open(name));
                }
            });
            ui.add_space(16.0);
        });
    choice
}

/// The New game dialog: pick a starter, then name the game.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewGame {
    /// 0: pick a starter. 1: name it.
    pub page: u8,
    pub starter: usize,
    pub name: String,
}

/// What a person chose in the New game dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewGameChoice {
    Cancel,
    Make { starter: String, name: String },
}

/// Shows the New game dialog.
pub fn new_game(
    ctx: &egui::Context,
    dialog: &mut NewGame,
    studio: &Studio,
    pictures: &mut Pictures,
) -> Option<NewGameChoice> {
    let mut choice = None;
    egui::Modal::new(egui::Id::unique("new game")).show(ctx, |ui| {
        ui.set_width(880.0);
        if dialog.page == 0 {
            ui.horizontal(|ui| {
                ui.heading("New game: pick a starter");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("1 of 2").weak());
                });
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                for (index, starter) in STARTERS.iter().enumerate() {
                    let picture = studio
                        .folder
                        .as_ref()
                        .and_then(|folder| folder.card_picture(starter.example))
                        .and_then(|path| pictures.get(ui.ctx(), &path));
                    let chosen = dialog.starter == index;
                    let frame = egui::Frame::group(ui.style()).stroke(if chosen {
                        egui::Stroke::new(2.0, theme::NIGHT_INDIGO)
                    } else {
                        ui.style().visuals.widgets.noninteractive.bg_stroke
                    });
                    let inner = frame.show(ui, |ui| {
                        ui.set_width(PICTURE.x);
                        ui.vertical(|ui| {
                            picture_or_blank(ui, picture.as_ref(), starter.title);
                            ui.label(RichText::new(starter.title).strong());
                        });
                    });
                    if inner.response.interact(Sense::click()).clicked() {
                        dialog.starter = index;
                    }
                }
            });
            ui.add_space(8.0);
            let starter = &STARTERS[dialog.starter.min(STARTERS.len() - 1)];
            ui.label(starter.about);
            if !starter.has_level {
                ui.label(
                    RichText::new(
                        "This starter has no tile level, so Build mode will offer Feel only.",
                    )
                    .weak(),
                );
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.add(theme::primary_button("Next")).clicked() {
                    dialog.page = 1;
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(NewGameChoice::Cancel);
                }
            });
        } else {
            ui.horizontal(|ui| {
                ui.heading("New game: name it");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("2 of 2").weak());
                });
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label("Name:");
                ui.add(
                    egui::TextEdit::singleline(&mut dialog.name)
                        .hint_text("Bee Maze")
                        .desired_width(320.0),
                )
                .request_focus();
            });
            let taken = |name: &str| {
                studio
                    .folder
                    .as_ref()
                    .is_some_and(|folder| folder.game(name).is_some())
            };
            let ready = match name_from_title(&dialog.name) {
                Ok(name) if taken(&name) => {
                    ui.label(
                        RichText::new("There's already a game with that name. Pick another.")
                            .color(theme::LOOK),
                    );
                    false
                }
                Ok(name) => {
                    ui.label(RichText::new(format!("Its folder will be examples/{name}")).weak());
                    true
                }
                Err(why) => {
                    ui.label(RichText::new(why).weak());
                    false
                }
            };
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let make = ui.add_enabled(ready, theme::primary_button("Make it"));
                let entered = ready && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if make.clicked() || entered {
                    choice = Some(NewGameChoice::Make {
                        starter: STARTERS[dialog.starter].kind.to_string(),
                        name: dialog.name.clone(),
                    });
                }
                if ui.button("Back").clicked() {
                    dialog.page = 0;
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(NewGameChoice::Cancel);
                }
            });
        }
    });
    choice
}
