//! Build mode: painting the level of a frozen game.
//!
//! The picture is the game's own drawing of the frozen frame; the Studio
//! only draws what's on top (the grid, the tile under the pointer, the
//! selection). Where the game drew its tile map comes with each picture,
//! so the grid sits exactly on the game's tiles. "Whole level" shows the
//! entire level instead, drawn by the Studio with the engine's own code
//! (tiles exactly as the game draws them; no backgrounds), for painting
//! where the game's camera isn't looking.
//!
//! Every click becomes a command (`level.paint`, `level.fill`...), the
//! same ones a script or an agent uses.

use std::collections::HashMap;

use duckforge::prelude::LetterKind;
use eframe::egui::{
    self, Color32, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, TextureHandle,
    TextureOptions, pos2, vec2,
};
use studio_core::level::{Cell, Level, LevelArt, Pixels};
use studio_core::studio::{Command, OpenGame};

use crate::keys::Placement;
use crate::pictures::game_image;
use crate::theme;

/// What a click on the level does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Paint,
    Erase,
    Fill,
    Things,
    Select,
    PlayFrom,
}

impl Tool {
    const ALL: [Tool; 6] = [
        Tool::Paint,
        Tool::Erase,
        Tool::Fill,
        Tool::Things,
        Tool::Select,
        Tool::PlayFrom,
    ];

    fn words(self) -> (&'static str, &'static str) {
        match self {
            Tool::Paint => (
                "Paint",
                "Click or drag to paint the tile you picked. Right-click erases.",
            ),
            Tool::Erase => ("Erase", "Click or drag to empty tiles."),
            Tool::Fill => (
                "Fill",
                "Click to fill an area of the same tile with the one you picked.",
            ),
            Tool::Things => (
                "Things",
                "Click to put the thing you picked. Drag a thing to move it; right-click takes it away.",
            ),
            Tool::Select => (
                "Select",
                "Drag to pick an area, then drag the area to move it.",
            ),
            Tool::PlayFrom => (
                "Play from here",
                "Click a tile: the game starts again from there, this time only.",
            ),
        }
    }
}

/// Where the level's cells are on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    /// The top left corner of cell (0, 0), in points.
    pub origin: Pos2,
    /// One cell's width, in points.
    pub cell: f32,
    pub columns: i32,
    pub rows: i32,
}

impl Grid {
    pub fn cell_at(&self, place: Pos2) -> Option<Cell> {
        let column = ((place.x - self.origin.x) / self.cell).floor() as i32;
        let row = ((place.y - self.origin.y) / self.cell).floor() as i32;
        (column >= 0 && row >= 0 && column < self.columns && row < self.rows)
            .then_some((column, row))
    }

    pub fn rect_of(&self, (column, row): Cell) -> Rect {
        Rect::from_min_size(
            self.origin + vec2(column as f32, row as f32) * self.cell,
            vec2(self.cell, self.cell),
        )
    }

    /// The grid over the game's own picture: where the game said it drew
    /// its tile map, enlarged as the picture is.
    pub fn over_the_game(open: &OpenGame, placement: Placement) -> Option<Grid> {
        let level = open.level.as_ref()?;
        let map = open.play.as_ref()?.maps.first()?;
        let points_per_game_pixel = placement.scale / placement.pixels_per_point;
        Some(Grid {
            origin: placement.rect.min + vec2(map.x, map.y) * points_per_game_pixel,
            cell: map.tile_size * map.scale * points_per_game_pixel,
            columns: level.columns() as i32,
            rows: level.rows() as i32,
        })
    }
}

/// Build mode's state: the tool and tile picked, the stroke being painted.
pub struct BuildMode {
    pub tool: Tool,
    pub tile: Option<char>,
    pub thing: Option<char>,
    /// Show the whole level (the Studio's drawing) instead of the game's
    /// frozen picture.
    pub whole_level: bool,
    /// Screen pixels per level pixel in the whole-level view.
    pub zoom: u32,
    /// The tile under the pointer, for the status bar ("column 14, row
    /// 3", as Check's lines say places).
    pub pointer_on: Option<Cell>,
    stroke: u64,
    last_cell: Option<Cell>,
    held_thing: Option<Cell>,
    selection: Option<(Cell, Cell)>,
    selecting_from: Option<Cell>,
    moving_from: Option<Cell>,
    palette: HashMap<char, TextureHandle>,
    palette_for: String,
    level_texture: Option<TextureHandle>,
    level_texture_text: String,
}

impl Default for BuildMode {
    fn default() -> BuildMode {
        BuildMode {
            tool: Tool::Paint,
            tile: None,
            thing: None,
            whole_level: false,
            zoom: 2,
            pointer_on: None,
            stroke: 0,
            last_cell: None,
            held_thing: None,
            selection: None,
            selecting_from: None,
            moving_from: None,
            palette: HashMap::new(),
            palette_for: String::new(),
            level_texture: None,
            level_texture_text: String::new(),
        }
    }
}

/// How big a palette picture is, in points.
const SWATCH: f32 = 28.0;

impl BuildMode {
    /// The left panel's Build part for a game with a level: the tools,
    /// the tiles and things, the level's rules. Returns the commands to run.
    pub fn tools(&mut self, ui: &mut egui::Ui, open: &OpenGame) -> Vec<Command> {
        let mut commands = Vec::new();
        let Some(level) = &open.level else {
            return commands;
        };
        self.load_palette(ui.ctx(), level, open.level_art.as_ref(), open.art_version);
        ui.label(RichText::new("Tools").strong());
        egui::Grid::new("build tools")
            .num_columns(2)
            .show(ui, |ui| {
                for (index, tool) in Tool::ALL.iter().enumerate() {
                    let (words, hint) = tool.words();
                    if ui
                        .selectable_label(self.tool == *tool, words)
                        .on_hover_text(hint)
                        .clicked()
                    {
                        self.tool = *tool;
                        self.selection = None;
                    }
                    if index % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
        ui.label(RichText::new(self.tool.words().1).weak().small());
        ui.add_space(6.0);

        let tiles: Vec<_> = level
            .letters
            .iter()
            .filter(|l| l.kind != LetterKind::Thing)
            .collect();
        let things: Vec<_> = level
            .letters
            .iter()
            .filter(|l| l.kind == LetterKind::Thing)
            .collect();
        if self.tile.is_none() {
            self.tile = tiles.first().map(|l| l.letter);
        }
        if self.thing.is_none() {
            self.thing = things
                .iter()
                .find(|l| !l.start)
                .or(things.first())
                .map(|l| l.letter);
        }
        ui.label(RichText::new("Tiles").strong());
        for letter in tiles {
            if self.swatch(
                ui,
                letter.letter,
                &letter.title(),
                self.tile == Some(letter.letter),
            ) {
                self.tile = Some(letter.letter);
                if !matches!(self.tool, Tool::Paint | Tool::Fill) {
                    self.tool = Tool::Paint;
                }
            }
        }
        ui.add_space(6.0);
        ui.label(RichText::new("Things").strong());
        for letter in things {
            if self.swatch(
                ui,
                letter.letter,
                &letter.title(),
                self.thing == Some(letter.letter),
            ) {
                self.thing = Some(letter.letter);
                self.tool = Tool::Things;
            }
        }
        ui.add_space(6.0);
        let rules = level.broken_rules();
        for rule in &rules {
            ui.label(RichText::new(rule).color(theme::LOOK));
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.selectable_label(!self.whole_level, "Game view").on_hover_text("Paint on the frozen frame, as the game drew it").clicked() {
                self.whole_level = false;
            }
            if ui
                .selectable_label(self.whole_level, "Whole level")
                .on_hover_text("See and paint all of the level (drawn by the Studio: tiles exactly as the game draws them, without its backgrounds)")
                .clicked()
            {
                self.whole_level = true;
            }
        });
        if self.whole_level {
            ui.horizontal(|ui| {
                ui.label(format!("Zoom {}x", self.zoom));
                if ui.small_button("-").clicked() && self.zoom > 1 {
                    self.zoom -= 1;
                }
                if ui.small_button("+").clicked() && self.zoom < 8 {
                    self.zoom += 1;
                }
            });
        }
        if let Some((a, b)) = self.selection {
            let size = (b.0 - a.0 + 1, b.1 - a.1 + 1);
            ui.label(format!(
                "Picked {} by {} tiles. Drag it to move it.",
                size.0, size.1
            ));
            if ui.button("Erase the picked area").clicked() {
                let cells = (a.1..=b.1)
                    .flat_map(|row| (a.0..=b.0).map(move |column| (column, row)))
                    .collect();
                commands.push(Command::LevelErase {
                    cells,
                    stroke: None,
                });
                self.selection = None;
            }
        }
        commands
    }

    /// One row of the palette: the letter's picture and its name. True if
    /// clicked.
    fn swatch(&self, ui: &mut egui::Ui, letter: char, name: &str, chosen: bool) -> bool {
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), SWATCH + 4.0), Sense::click());
        let painter = ui.painter_at(rect);
        if chosen {
            painter.rect_filled(rect, 4.0, theme::NIGHT_INDIGO);
        } else if response.hovered() {
            painter.rect_filled(rect, 4.0, ui.visuals().widgets.hovered.bg_fill);
        }
        let picture = Rect::from_min_size(rect.min + vec2(4.0, 2.0), vec2(SWATCH, SWATCH));
        if let Some(texture) = self.palette.get(&letter) {
            let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(texture.id(), picture, whole, Color32::WHITE);
        }
        let text = if chosen {
            Color32::WHITE
        } else {
            ui.visuals().text_color()
        };
        painter.text(
            pos2(picture.right() + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            format!("{name}   {letter}"),
            egui::FontId::proportional(14.0),
            text,
        );
        response
            .on_hover_text(format!("The letter {letter} in level.txt"))
            .clicked()
    }

    fn load_palette(
        &mut self,
        ctx: &egui::Context,
        level: &Level,
        art: Option<&LevelArt>,
        version: u64,
    ) {
        let key = format!("{} {version}", level.path.display());
        if self.palette_for == key {
            return;
        }
        // Another level: nothing picked yet. New pictures: the same picks.
        if !self
            .palette_for
            .starts_with(&format!("{} ", level.path.display()))
        {
            self.tile = None;
            self.thing = None;
        }
        self.palette.clear();
        self.palette_for = key;
        self.level_texture_text.clear();
        let Some(art) = art else {
            return;
        };
        let size = level.tile_size.round().max(8.0) as usize;
        for letter in &level.letters {
            let picture = art.picture_of(letter, size);
            self.palette.insert(
                letter.letter,
                texture(ctx, &format!("tile {}", letter.letter), &picture),
            );
        }
    }

    /// The whole level, drawn by the Studio, in a scrolling view. Returns
    /// the grid and the response for the pointer.
    pub fn whole_level_view(
        &mut self,
        ui: &mut egui::Ui,
        open: &OpenGame,
    ) -> Option<(Grid, Response)> {
        let level = open.level.as_ref()?;
        let art = open.level_art.as_ref()?;
        let text = level.text();
        if self.level_texture.is_none() || self.level_texture_text != text {
            let picture = art.draw_level(level);
            match &mut self.level_texture {
                Some(existing) => existing.set(image(&picture), TextureOptions::NEAREST),
                None => self.level_texture = Some(texture(ui.ctx(), "the whole level", &picture)),
            }
            self.level_texture_text = text;
        }
        let texture = self.level_texture.clone()?;
        let pixels_per_point = ui.ctx().pixels_per_point();
        let size = texture.size_vec2() * self.zoom as f32 / pixels_per_point;
        let mut result = None;
        egui::ScrollArea::both()
            // Dragging paints; the wheel and the bars scroll.
            .scroll_source(egui::containers::scroll_area::ScrollSource {
                drag: egui::containers::scroll_area::DragScroll::Never,
                ..egui::containers::scroll_area::ScrollSource::ALL
            })
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
                // On whole screen pixels, so each level pixel is a sharp square.
                let corner = (rect.min.to_vec2() * pixels_per_point).round() / pixels_per_point;
                let rect = Rect::from_min_size(corner.to_pos2(), size);
                let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                ui.painter()
                    .image(texture.id(), rect, whole, Color32::WHITE);
                let grid = Grid {
                    origin: rect.min,
                    cell: level.tile_size * self.zoom as f32 / pixels_per_point,
                    columns: level.columns() as i32,
                    rows: level.rows() as i32,
                };
                result = Some((grid, response));
            });
        result
    }

    /// What the pointer does on the level, with the tool picked. Returns
    /// the commands to run.
    pub fn use_tool(
        &mut self,
        ui: &egui::Ui,
        response: &Response,
        grid: Grid,
        level: &Level,
    ) -> Vec<Command> {
        let mut commands = Vec::new();
        let hovered = response.hover_pos().and_then(|place| grid.cell_at(place));
        self.pointer_on = hovered;
        let (primary, secondary, pressed, released) = ui.input(|i| {
            (
                i.pointer.primary_down(),
                i.pointer.secondary_down(),
                i.pointer.any_pressed(),
                i.pointer.any_released(),
            )
        });
        let name_of = |letter: Option<char>| -> Option<String> {
            letter.and_then(|l| level.letter(l)).map(|l| l.name.clone())
        };
        if pressed && hovered.is_some() {
            self.stroke += 1;
            self.last_cell = None;
        }
        match (self.tool, hovered) {
            (Tool::Paint | Tool::Erase, Some(cell))
                if (primary || secondary) && response.contains_pointer() =>
            {
                if self.last_cell != Some(cell) {
                    self.last_cell = Some(cell);
                    let erase = secondary || self.tool == Tool::Erase;
                    commands.push(match (erase, name_of(self.tile)) {
                        (false, Some(tile)) => Command::LevelPaint {
                            cells: vec![cell],
                            tile,
                            stroke: Some(self.stroke),
                        },
                        _ => Command::LevelErase {
                            cells: vec![cell],
                            stroke: Some(self.stroke),
                        },
                    });
                }
            }
            (Tool::Fill, Some(cell)) if response.clicked() => {
                if let Some(tile) = name_of(self.tile) {
                    commands.push(Command::LevelFill { at: cell, tile });
                }
            }
            (Tool::Things, Some(cell)) => {
                if response.secondary_clicked() {
                    commands.push(Command::LevelDeleteThing { at: cell });
                } else if pressed && primary {
                    let holding = level
                        .letter(level.letter_at(cell))
                        .is_some_and(|l| l.kind == LetterKind::Thing);
                    self.held_thing = holding.then_some(cell);
                } else if released {
                    match self.held_thing.take() {
                        Some(from) if from != cell => {
                            commands.push(Command::LevelMoveThing { from, to: cell })
                        }
                        Some(_) => {}
                        None => {
                            if let Some(thing) = name_of(self.thing) {
                                commands.push(Command::LevelPlaceThing { at: cell, thing });
                            }
                        }
                    }
                }
            }
            (Tool::Select, Some(cell)) => {
                let inside = self.selection.is_some_and(|(a, b)| {
                    (a.0..=b.0).contains(&cell.0) && (a.1..=b.1).contains(&cell.1)
                });
                if pressed && primary {
                    if inside {
                        self.moving_from = Some(cell);
                    } else {
                        self.selecting_from = Some(cell);
                        self.selection = Some((cell, cell));
                    }
                } else if primary {
                    if let Some(from) = self.selecting_from {
                        self.selection = Some((
                            (from.0.min(cell.0), from.1.min(cell.1)),
                            (from.0.max(cell.0), from.1.max(cell.1)),
                        ));
                    }
                } else if released {
                    self.selecting_from = None;
                    if let (Some(from), Some((a, b))) = (self.moving_from.take(), self.selection) {
                        let shift = (cell.0 - from.0, cell.1 - from.1);
                        if shift != (0, 0) {
                            commands.push(Command::LevelMoveArea {
                                from: a,
                                size: (b.0 - a.0 + 1, b.1 - a.1 + 1),
                                to: (a.0 + shift.0, a.1 + shift.1),
                            });
                            self.selection = Some((
                                (a.0 + shift.0, a.1 + shift.1),
                                (b.0 + shift.0, b.1 + shift.1),
                            ));
                        }
                    }
                }
            }
            (Tool::PlayFrom, Some(cell)) if response.clicked() => {
                commands.push(Command::LevelPlayFrom { at: cell });
            }
            _ => {}
        }
        if released {
            self.last_cell = None;
        }
        commands
    }

    /// What Build mode draws on top of the picture: a faint grid, the cell
    /// under the pointer, the picked area.
    pub fn draw_on_top(&self, ui: &egui::Ui, clip: Rect, grid: Grid, hovered: Option<Cell>) {
        let painter = ui.painter_at(clip);
        let line = Stroke::new(1.0, Color32::from_white_alpha(28));
        let visible = clip.intersect(Rect::from_min_size(
            grid.origin,
            vec2(grid.columns as f32, grid.rows as f32) * grid.cell,
        ));
        if grid.cell >= 6.0 && visible.is_positive() {
            let first = ((visible.min - grid.origin) / grid.cell).floor();
            let last = ((visible.max - grid.origin) / grid.cell).ceil();
            for column in first.x as i32..=last.x as i32 {
                let x = grid.origin.x + column as f32 * grid.cell;
                painter.line_segment([pos2(x, visible.top()), pos2(x, visible.bottom())], line);
            }
            for row in first.y as i32..=last.y as i32 {
                let y = grid.origin.y + row as f32 * grid.cell;
                painter.line_segment([pos2(visible.left(), y), pos2(visible.right(), y)], line);
            }
        }
        if let Some((a, b)) = self.selection {
            let rect = grid.rect_of(a).union(grid.rect_of(b));
            painter.rect_stroke(
                rect,
                0.0,
                Stroke::new(2.0, theme::DUCK_YELLOW),
                StrokeKind::Inside,
            );
        }
        if let Some(cell) = hovered {
            painter.rect_stroke(
                grid.rect_of(cell),
                0.0,
                Stroke::new(2.0, Color32::WHITE),
                StrokeKind::Inside,
            );
        }
        if let Some(cell) = self.held_thing {
            painter.rect_stroke(
                grid.rect_of(cell),
                0.0,
                Stroke::new(2.0, theme::DUCK_YELLOW),
                StrokeKind::Inside,
            );
        }
    }
}

fn image(picture: &Pixels) -> egui::ColorImage {
    game_image(picture.width, picture.height, &picture.pixels)
}

fn texture(ctx: &egui::Context, name: &str, picture: &Pixels) -> TextureHandle {
    ctx.load_texture(name, image(picture), TextureOptions::NEAREST)
}

/// Build mode for a game with no tile level: what there is to do here.
pub fn no_level_to_paint(ui: &mut egui::Ui, has_a_folder: bool) {
    ui.label(
        "This game has no level to paint (the platformer and top-down starters have one). \
         Here is what you can do in Build mode:",
    );
    ui.add_space(4.0);
    let mut item = |title: &str, words: &str| {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(title).strong());
            ui.label(words);
        });
    };
    item(
        "Feel:",
        "change how it moves and plays with the sliders on the right. Press Tab and it plays \
         with them at once.",
    );
    item(
        "Sounds:",
        "play the sounds the game has made again, and read how each one was made.",
    );
    item(
        "Pictures:",
        "open its pictures in Duck Quill. When one is saved, the game shows it.",
    );
    if has_a_folder {
        item(
            "Files:",
            "the Files tab on the right shows everything in the game's folder. Click a picture \
             to see it or a sound to hear it, and drop new ones in from File Explorer.",
        );
    }
    item(
        "Frame by frame:",
        "Play one frame or Play ten frames (below), and look at what changed. Tab carries on \
         from there.",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_on_screen_is_a_cell_and_back() {
        let grid = Grid {
            origin: pos2(100.0, 50.0),
            cell: 32.0,
            columns: 80,
            rows: 15,
        };
        assert_eq!(grid.cell_at(pos2(100.0, 50.0)), Some((0, 0)));
        assert_eq!(grid.cell_at(pos2(165.0, 83.0)), Some((2, 1)));
        assert_eq!(grid.cell_at(pos2(99.0, 60.0)), None, "left of the level");
        assert_eq!(
            grid.cell_at(pos2(200.0, 50.0 + 15.0 * 32.0)),
            None,
            "below it"
        );
        assert_eq!(
            grid.rect_of((2, 1)),
            Rect::from_min_size(pos2(164.0, 82.0), vec2(32.0, 32.0))
        );
    }
}
