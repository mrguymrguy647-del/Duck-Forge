//! The editor on a frozen game that has scenes: the Scene list, the bar of
//! tools above the view, selecting and moving in the view, the Inspector.
//!
//! The picture is still the game's own drawing of the frozen frame. With
//! it the game says where it drew each object, so the Studio only draws
//! what's on top (outlines, the grid, the arrows) and does the clicking.
//! Every click becomes a command (`select.set`, `object.move`...), the
//! same ones a script or an agent uses.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use duckforge::prelude::Vec2;
use duckforge::scene::Object;
use duckforge::studio_link::protocol::{DrawnObject, DrawnScene, Origin};
use eframe::egui::{
    self, Color32, CursorIcon, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, pos2, vec2,
};
use serde_json::{Map, Value, json};
use studio_core::scene_edit::{self, Needs, PLAIN_KINDS, RowState, SceneDoc, Show, Tool, pair};
use studio_core::studio::{Command, OpenGame};

use crate::files_panel::{self, DraggedFile};
use crate::keys::Placement;
use crate::theme;
use studio_core::game_files::FileKind;

/// The first stroke number of this panel, well away from the ones paint
/// strokes and Feel's sliders count up from: two edits join one Undo step
/// only if they're really the same drag.
const FIRST_STROKE: u64 = 1 << 40;

/// How often a drag sends the game where things are now.
const SEND_EVERY: Duration = Duration::from_millis(70);

/// How long the Move tool's arrows are, in points.
const ARROW: f32 = 44.0;

/// One direction a drag is kept to (an arrow of the Move tool).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    Across,
    Up,
}

/// An on-or-off key of an object: its name, its words, and its value.
type Flag = (&'static str, &'static str, fn(&Object) -> bool);

/// Where a picture is drawn on the canvas: its place before it's turned,
/// the point it turns round, and how far it's turned.
type PictureShape = (duckforge::prelude::Rect, Vec2, f32);

/// A move in the view, from press to release.
#[derive(Debug, Clone)]
struct Drag {
    axis: Option<Axis>,
    /// Where the pointer went down, in points.
    pressed: Pos2,
    /// The grabbed object's place when it was grabbed, in the scene.
    lead_at: Vec2,
    /// Scene pixels per canvas pixel, for the grabbed object's layer.
    scale: f32,
    /// Objects whose start moves (the file changes), with where each
    /// started.
    starts: Vec<(u64, Vec2)>,
    /// Objects moved for this run only, with where each was.
    now: Vec<(u64, Vec2)>,
    /// Where each dragged thing was drawn, on the canvas: the outlines
    /// follow the pointer at once, before the game has drawn again.
    shapes: Vec<PictureShape>,
    stroke: u64,
    /// How far it has been moved, as last sent, and when.
    sent: Option<(Vec2, Instant)>,
}

/// Turning round the ring, from press to release.
#[derive(Debug, Clone)]
struct Turning {
    ids: Vec<u64>,
    /// The first object as it was drawn, and its turn in the file.
    shape: PictureShape,
    start: f32,
    /// Where the pointer went down, on the canvas.
    pressed: Vec2,
    /// Its turn now, and as last sent (and when).
    latest: f32,
    sent: Option<(f32, Instant)>,
    stroke: u64,
}

/// Sizing by a corner, from press to release.
#[derive(Debug, Clone)]
struct Sizing {
    ids: Vec<u64>,
    /// The first object as it was drawn, and its size in the file.
    shape: PictureShape,
    start: Vec2,
    /// The corner grabbed, on the canvas.
    handle: Vec2,
    latest: Vec2,
    sent: Option<(Vec2, Instant)>,
    stroke: u64,
}

/// What the pointer is doing in the view.
#[derive(Debug, Clone)]
enum Gesture {
    Move(Drag),
    Turn(Turning),
    Size(Sizing),
    /// A box drawn from empty space, to select what it touches (`add`:
    /// Shift was held, so it adds to what's selected).
    Box {
        from: Pos2,
        to: Pos2,
        add: bool,
    },
}

/// The Add window: the kind picked, and the picture or level for it.
#[derive(Debug, Clone, Default)]
struct Adding {
    kind: Option<&'static str>,
    choice: Option<String>,
}

/// How long after one nudge with the arrow keys the next is the same
/// step for Undo.
const NUDGES_JOIN: Duration = Duration::from_millis(1500);

/// The scene editor's own state: the drag in progress, the text being
/// typed, what it has read from disk.
pub struct ScenePanel {
    drag: Option<Gesture>,
    next_stroke: u64,
    /// The box in use in the Inspector, and its stroke for Undo.
    field: Option<(egui::Id, u64)>,
    /// The text being typed: the object, the key, the text so far.
    typing: Option<(u64, String, String)>,
    /// The object being renamed in the Scene list.
    renaming: Option<u64>,
    /// The game's pictures, for the Picture box: the folder they're from,
    /// when it was read, and their names.
    pictures: Option<(PathBuf, Instant, Vec<String>)>,
    /// Sprite sheets' animation names, by file.
    animations: HashMap<PathBuf, Vec<String>>,
    /// The game's sounds, like `pictures`.
    sounds: Option<(PathBuf, Instant, Vec<String>)>,
    /// The last nudge with the arrow keys: its stroke, when, and what it
    /// moved.
    nudge: Option<(u64, Instant, Vec<u64>)>,
    /// The Add window, while it's open.
    adding: Option<Adding>,
    /// The Scene list's search.
    find: String,
    /// Objects being dragged up or down the Scene list.
    list_drag: Option<Vec<u64>>,
}

impl Default for ScenePanel {
    fn default() -> ScenePanel {
        ScenePanel {
            drag: None,
            next_stroke: FIRST_STROKE,
            field: None,
            typing: None,
            renaming: None,
            pictures: None,
            animations: HashMap::new(),
            sounds: None,
            nudge: None,
            adding: None,
            find: String::new(),
            list_drag: None,
        }
    }
}

/// Whether the tile map is what's selected: then the view belongs to the
/// paint tools, as it always has.
pub fn tile_map_selected(open: &OpenGame) -> bool {
    let Some(doc) = &open.scene else {
        return false;
    };
    open.selected.iter().any(|id| {
        doc.object(*id)
            .is_some_and(|object| object.kind == "tilemap")
    })
}

/// A rectangle of the game's picture as a rectangle on screen.
fn on_screen(placement: Placement, rect: duckforge::prelude::Rect) -> Rect {
    let each = placement.scale / placement.pixels_per_point;
    Rect::from_min_size(
        placement.rect.min + vec2(rect.x, rect.y) * each,
        vec2(rect.w, rect.h) * each,
    )
}

fn set_of(key: &str, value: Value) -> Map<String, Value> {
    Map::from_iter([(key.to_string(), value)])
}

/// A number as the scene file writes it: whole if it's whole.
fn number(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 1e9 {
        Value::from(value as i64)
    } else {
        Value::from((value * 1000.0).round() / 1000.0)
    }
}

/// A box of the Inspector that takes a file dragged from Files: it lights
/// up while one is over it (red, saying why, if it's the wrong kind of
/// file), and letting go gives the objects the file as their `key`.
fn take_drop(
    response: &egui::Response,
    wants: FileKind,
    ids: &[u64],
    key: &str,
    commands: &mut Vec<Command>,
) {
    if let Some(file) = response.dnd_hover_payload::<DraggedFile>() {
        let right = file.kind == wants;
        let colour = if right {
            theme::DUCK_YELLOW
        } else {
            theme::BAD
        };
        response
            .ctx
            .layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::unique("drop on a box"),
            ))
            .rect_stroke(
                response.rect.expand(2.0),
                3.0,
                Stroke::new(2.0, colour),
                StrokeKind::Outside,
            );
        let words = if right {
            format!("Let go to use {}", file.name())
        } else {
            format!("{} isn't a {key}", file.name())
        };
        response.show_tooltip_text(words);
    }
    if let Some(file) = response
        .dnd_release_payload::<DraggedFile>()
        .filter(|file| file.kind == wants)
    {
        commands.push(Command::FilesUse {
            path: file.path.clone(),
            at: None,
            ids: ids.to_vec(),
            key: Some(String::from(key)),
        });
    }
}

/// A point of the canvas, on screen.
fn screen_point(placement: Placement, point: Vec2) -> Pos2 {
    let each = placement.scale / placement.pixels_per_point;
    placement.rect.min + vec2(point.x, point.y) * each
}

/// A turned picture's corners, on screen.
fn screen_corners(placement: Placement, shape: PictureShape) -> Vec<Pos2> {
    scene_edit::corners(shape.0, shape.1, shape.2)
        .iter()
        .map(|corner| screen_point(placement, *corner))
        .collect()
}

/// A closed outline through `points`, dashed or not.
fn outline(painter: &egui::Painter, points: Vec<Pos2>, stroke: Stroke, dashed: bool) {
    if dashed {
        let mut round = points.clone();
        round.extend(points.first().copied());
        painter.extend(Shape::dashed_line(&round, stroke, 5.0, 4.0));
    } else {
        painter.add(Shape::closed_line(points, stroke));
    }
}

/// A picture's shape made `fx` and `fy` times bigger round its pivot.
fn sized_shape(shape: PictureShape, fx: f32, fy: f32) -> PictureShape {
    let (rect, pivot, turn) = shape;
    (
        duckforge::prelude::Rect::new(
            pivot.x + (rect.x - pivot.x) * fx,
            pivot.y + (rect.y - pivot.y) * fy,
            rect.w * fx,
            rect.h * fy,
        ),
        pivot,
        turn,
    )
}

/// The Turn tool's ring round a picture: its middle (what the picture
/// turns round) and how big it is, on screen.
fn ring_of(placement: Placement, shape: PictureShape) -> (Pos2, f32) {
    let middle = screen_point(placement, shape.1);
    let furthest = screen_corners(placement, shape)
        .iter()
        .map(|corner| corner.distance(middle))
        .fold(0.0, f32::max);
    (middle, (furthest + 12.0).max(24.0))
}

/// Whether a point is on the ring (or near enough to grab it).
fn on_ring(ring: (Pos2, f32), point: Pos2) -> bool {
    (point.distance(ring.0) - ring.1).abs() <= 9.0
}

/// The Size tool's corner under a point: where it is on the canvas.
fn handle_under(placement: Placement, shape: PictureShape, point: Pos2) -> Option<Vec2> {
    scene_edit::corners(shape.0, shape.1, shape.2)
        .into_iter()
        .find(|corner| screen_point(placement, *corner).distance(point) <= 8.0)
}

/// A size or a turn in a few characters: 2, 1.5, 0.75.
fn short(n: f32) -> String {
    let text = format!("{n:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl ScenePanel {
    /// A drag is moving something right now.
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    fn new_stroke(&mut self) -> u64 {
        self.next_stroke += 1;
        self.next_stroke
    }

    /// The stroke for a box of the Inspector: one drag of it, or one go
    /// at typing in it, is one step for Undo.
    fn stroke_of(&mut self, response: &egui::Response) -> Option<u64> {
        if response.drag_started() || response.gained_focus() {
            let stroke = self.new_stroke();
            self.field = Some((response.id, stroke));
        }
        self.field
            .filter(|(id, _)| *id == response.id)
            .map(|(_, stroke)| stroke)
    }

    /// Whether the Add window is open.
    pub fn is_adding(&self) -> bool {
        self.adding.is_some()
    }

    /// The Scene list: every object of the scene, in drawing order, with
    /// the Add button and a search box. Rows can be dragged up and down to
    /// change the order, and right-clicked for more.
    pub fn list(&mut self, ui: &mut egui::Ui, open: &OpenGame) -> Vec<Command> {
        let mut commands = Vec::new();
        let Some(doc) = &open.scene else {
            return commands;
        };
        let can_change = doc.problem.is_none() && doc.file.is_some();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Scene").strong());
            ui.label(RichText::new(doc.short_name()).weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(can_change, theme::primary_button("+ Add"))
                    .on_hover_text("Add an object to the scene")
                    .clicked()
                {
                    self.adding = Some(Adding::default());
                }
            });
        });
        if let Some(problem) = &doc.problem {
            ui.label(RichText::new(problem).color(theme::LOOK));
            ui.label(
                RichText::new(
                    "The Studio won't change this file until that's fixed. Open it from Files.",
                )
                .weak()
                .small(),
            );
        }
        ui.add(
            egui::TextEdit::singleline(&mut self.find)
                .hint_text("Find an object...")
                .desired_width(f32::INFINITY),
        );
        let find = self.find.trim().to_lowercase();
        let rows: Vec<_> = open
            .scene_rows()
            .into_iter()
            .filter(|row| {
                find.is_empty()
                    || row.label.to_lowercase().contains(&find)
                    || row.kind.to_lowercase().contains(&find)
            })
            .collect();
        if rows.is_empty() && doc.problem.is_none() {
            ui.label(
                RichText::new(if find.is_empty() {
                    "This scene has no objects yet."
                } else {
                    "No object is called that."
                })
                .weak(),
            );
        }
        let (shift, ctrl) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
        // Where each of the file's rows is, for dragging them.
        let mut places: Vec<(u64, Rect)> = Vec::new();
        for row in &rows {
            if self.renaming == Some(row.id) {
                let name = doc
                    .object(row.id)
                    .map_or(String::new(), |object| object.name.clone());
                match self.text_box(ui, row.id, "name", &name, true) {
                    TextBox::Done(Some(name)) => {
                        self.renaming = None;
                        commands.push(Command::ObjectRename { id: row.id, name });
                    }
                    TextBox::Done(None) => self.renaming = None,
                    TextBox::Typing => {}
                }
                continue;
            }
            let mut text = RichText::new(&row.label);
            if row.state != RowState::There {
                text = text.italics().weak();
            }
            let response = ui
                .horizontal(|ui| {
                    let response = ui.selectable_label(row.selected, text);
                    if row.label != row.kind && row.kind != "picture" {
                        ui.label(RichText::new(&row.kind).weak().small());
                    }
                    if row.moved {
                        ui.label(RichText::new("moved").weak().small())
                            .on_hover_text(
                                "It isn't where it starts: the game has moved it since the start.",
                            );
                    }
                    response
                })
                .inner;
            let in_file = row.state != RowState::Made;
            let response = if in_file && can_change {
                places.push((row.id, response.rect));
                response.interact(Sense::drag())
            } else {
                response
            };
            let response = match row.state {
                RowState::There => response,
                RowState::Gone => response.on_hover_text(
                    "The game has removed it in this run. It's still in the scene's file.",
                ),
                RowState::Made => response.on_hover_text(
                    "The game made this while playing. It isn't in the scene's file.",
                ),
            };
            if let Some(painted) = &row.painted {
                ui.label(
                    RichText::new(format!("      {painted} painted on it"))
                        .weak()
                        .small(),
                );
            }
            // What a menu or a drag acts on: the selection, if this row is
            // in it.
            let ids = if row.selected {
                open.selected.clone()
            } else {
                vec![row.id]
            };
            if in_file && can_change && response.drag_started() {
                let order: Vec<u64> = open.scene_rows().iter().map(|row| row.id).collect();
                let mut dragged: Vec<u64> = order
                    .into_iter()
                    .filter(|id| ids.contains(id) && doc.object(*id).is_some())
                    .collect();
                if dragged.is_empty() {
                    dragged.push(row.id);
                }
                self.list_drag = Some(dragged);
            }
            response.context_menu(|ui| {
                if in_file && ui.button("Rename").clicked() {
                    self.renaming = Some(row.id);
                    self.typing = None;
                    ui.close();
                }
                if in_file
                    && ui
                        .add(egui::Button::new("Duplicate").shortcut_text("Ctrl+D"))
                        .clicked()
                {
                    commands.push(Command::ObjectDuplicate { ids: ids.clone() });
                    ui.close();
                }
                if ui
                    .add(egui::Button::new("Copy").shortcut_text("Ctrl+C"))
                    .clicked()
                {
                    commands.push(Command::ObjectCopy { ids: ids.clone() });
                    ui.close();
                }
                if in_file
                    && ui
                        .add(egui::Button::new("Delete").shortcut_text("Delete"))
                        .clicked()
                {
                    commands.push(Command::ObjectDelete { ids: ids.clone() });
                    ui.close();
                }
                if in_file {
                    ui.separator();
                    for (words, to) in [
                        ("Bring to the front (drawn last)", "front"),
                        ("Bring forward", "forward"),
                        ("Send backward", "backward"),
                        ("Send to the back (drawn first)", "back"),
                    ] {
                        if ui.button(words).clicked() {
                            commands.push(Command::ObjectOrder {
                                ids: ids.clone(),
                                to: Some(String::from(to)),
                                before: None,
                            });
                            ui.close();
                        }
                    }
                }
            });
            if response.double_clicked() && in_file {
                self.renaming = Some(row.id);
                self.typing = None;
            } else if response.clicked() {
                if shift || ctrl {
                    let mut ids = open.selected.clone();
                    if row.selected {
                        ids.retain(|id| *id != row.id);
                    } else {
                        ids.push(row.id);
                    }
                    commands.push(Command::SelectSet { ids });
                } else {
                    commands.push(Command::SelectSet { ids: vec![row.id] });
                }
            }
        }
        commands.extend(self.reorder(ui, &places));
        ui.label(
            RichText::new(
                "Click to select, double-click to rename, drag up or down to reorder, right-click \
                 for more. Later in the list is drawn on top.",
            )
            .weak()
            .small(),
        );
        commands
    }

    /// Rows being dragged in the list: a line shows where they'll go, and
    /// letting go moves them there.
    fn reorder(&mut self, ui: &egui::Ui, places: &[(u64, Rect)]) -> Vec<Command> {
        let Some(ids) = self.list_drag.clone() else {
            return Vec::new();
        };
        let (pointer, down) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_down()));
        let Some(pointer) = pointer else {
            self.list_drag = None;
            return Vec::new();
        };
        // Before the first row whose middle is below the pointer; or last.
        let before = places
            .iter()
            .find(|(_, rect)| rect.center().y > pointer.y)
            .map(|(id, _)| *id);
        let line_y = match before {
            Some(id) => places
                .iter()
                .find(|(have, _)| *have == id)
                .map(|(_, r)| r.top()),
            None => places.last().map(|(_, rect)| rect.bottom()),
        };
        if let (Some(y), Some((_, first))) = (line_y, places.first()) {
            let painter = ui.painter();
            painter.line_segment(
                [pos2(first.left(), y), pos2(ui.max_rect().right() - 8.0, y)],
                Stroke::new(2.0, theme::DUCK_YELLOW),
            );
        }
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        if down {
            return Vec::new();
        }
        self.list_drag = None;
        // Dropped on itself: before the next row that isn't moving.
        let before = before.and_then(|target| {
            let at = places.iter().position(|(id, _)| *id == target)?;
            places[at..]
                .iter()
                .map(|(id, _)| *id)
                .find(|id| !ids.contains(id))
                .map(Some)
        });
        let command = match before {
            Some(Some(before)) => Command::ObjectOrder {
                ids,
                to: None,
                before: Some(before),
            },
            _ => Command::ObjectOrder {
                ids,
                to: Some(String::from("front")),
                before: None,
            },
        };
        vec![command]
    }

    /// The bar above the view: the tools, snap, the grid, and what the
    /// frozen picture shows.
    pub fn tool_bar(&mut self, ui: &mut egui::Ui, open: &OpenGame) -> Vec<Command> {
        let mut commands = Vec::new();
        let view = open.view;
        ui.horizontal(|ui| {
            for (tool, words, hint) in [
                (
                    Tool::Select,
                    "Select",
                    "Click an object to select it; drag it to move it (V)",
                ),
                (
                    Tool::Move,
                    "Move",
                    "Drag the selected object, or an arrow to keep to one direction (G)",
                ),
                (
                    Tool::Turn,
                    "Turn",
                    "Drag round the ring to turn the selected object; it snaps to 15 degrees. Alt: freely (R)",
                ),
                (
                    Tool::Size,
                    "Size",
                    "Drag a corner to make the selected object bigger or smaller. Shift keeps its shape; Alt: freely (S)",
                ),
            ] {
                if ui
                    .selectable_label(view.tool == tool, words)
                    .on_hover_text(hint)
                    .clicked()
                {
                    commands.push(Command::ViewTool { tool });
                }
            }
            ui.separator();
            let mut snap = view.snap;
            if ui
                .checkbox(&mut snap, "Snap")
                .on_hover_text("Dragged objects land on the grid. Hold Alt to place freely.")
                .changed()
            {
                commands.push(Command::ViewSnap {
                    on: snap,
                    step: None,
                });
            }
            let mut step = view.snap_step;
            let squares = ui
                .add(
                    egui::DragValue::new(&mut step)
                        .range(1.0..=256.0)
                        .speed(0.25)
                        .suffix(" px"),
                )
                .on_hover_text("How big the grid's squares are");
            if squares.changed() {
                commands.push(Command::ViewSnap {
                    on: view.snap,
                    step: Some(step.round()),
                });
            }
            let mut grid = view.grid;
            if ui
                .checkbox(&mut grid, "Grid")
                .on_hover_text("Show the grid over the game")
                .changed()
            {
                commands.push(Command::ViewGrid { on: grid });
            }
            ui.separator();
            for (what, words, hint) in [
                (
                    Show::Frozen,
                    "Frozen moment",
                    "The game exactly as it is on this frame",
                ),
                (
                    Show::Start,
                    "As it starts",
                    "The scene as its file has it, with nothing moved. The game isn't touched.",
                ),
            ] {
                if ui
                    .selectable_label(view.show == what, words)
                    .on_hover_text(hint)
                    .clicked()
                {
                    commands.push(Command::ViewShow { what });
                }
            }
        });
        commands
    }

    /// The editor's keys. V, G, R and S pick the tool; Escape selects
    /// nothing; Ctrl+A selects everything; Ctrl+C, Ctrl+X, Ctrl+V and
    /// Ctrl+D copy, cut, paste and duplicate; Delete deletes; the arrow
    /// keys nudge by a pixel, or a grid square with Shift. Paste puts the
    /// copies where the pointer is, if it's over the game.
    pub fn keys(
        &mut self,
        ui: &egui::Ui,
        open: &OpenGame,
        placement: Option<Placement>,
    ) -> Vec<Command> {
        let mut commands = Vec::new();
        let tool_keys = [
            (egui::Key::V, Tool::Select),
            (egui::Key::G, Tool::Move),
            (egui::Key::R, Tool::Turn),
            (egui::Key::S, Tool::Size),
        ];
        let (events, modifiers, pointer) =
            ui.input(|i| (i.events.clone(), i.modifiers, i.pointer.latest_pos()));
        let key = |wanted: egui::Key, with: egui::Modifiers| {
            events.iter().any(|event| {
                matches!(event, egui::Event::Key { key, pressed: true, modifiers, .. }
                    if *key == wanted && modifiers.matches_logically(with))
            })
        };
        for (pressed, tool) in tool_keys {
            if key(pressed, egui::Modifiers::NONE) {
                commands.push(Command::ViewTool { tool });
            }
        }
        if key(egui::Key::Escape, egui::Modifiers::NONE)
            && !open.selected.is_empty()
            && self.drag.is_none()
        {
            commands.push(Command::SelectNone);
        }
        // While the tile map is selected, the view is the paint tools'.
        if tile_map_selected(open) || self.drag.is_some() {
            return commands;
        }
        let (Some(doc), drawn) = (&open.scene, open.drawn_scene()) else {
            return commands;
        };
        let selected = open.selected.clone();
        let something = !selected.is_empty();
        let command = egui::Modifiers::COMMAND;
        if key(egui::Key::A, command) {
            commands.push(Command::SelectAll);
        }
        let copy =
            events.iter().any(|e| matches!(e, egui::Event::Copy)) || key(egui::Key::C, command);
        let cut =
            events.iter().any(|e| matches!(e, egui::Event::Cut)) || key(egui::Key::X, command);
        if something && (copy || cut) {
            commands.push(Command::ObjectCopy {
                ids: selected.clone(),
            });
        }
        if something && cut {
            commands.push(Command::ObjectDelete {
                ids: selected.clone(),
            });
        }
        let pasted = events.iter().find_map(|event| match event {
            egui::Event::Paste(text) => Some(Some(text.clone())),
            egui::Event::PasteImage(_) => Some(None),
            _ => None,
        });
        let pasted = pasted.or_else(|| key(egui::Key::V, command).then_some(None));
        if let Some(text) = pasted {
            // Where the pointer is, if it's over the game.
            let at = match (placement, pointer, drawn) {
                (Some(placement), Some(pointer), Some(drawn))
                    if placement.rect.contains(pointer) =>
                {
                    let at = open
                        .view
                        .place(drawn.to_scene(placement.to_game(pointer)), modifiers.alt);
                    Some([at.x, at.y])
                }
                _ => None,
            };
            commands.push(Command::ObjectPaste { at, text });
        }
        if something && key(egui::Key::D, command) {
            commands.push(Command::ObjectDuplicate {
                ids: selected.clone(),
            });
        }
        if something
            && (key(egui::Key::Delete, egui::Modifiers::NONE)
                || key(egui::Key::Backspace, egui::Modifiers::NONE))
        {
            commands.push(Command::ObjectDelete {
                ids: selected.clone(),
            });
        }
        // Nudging.
        let step = if modifiers.shift {
            open.view.snap_step
        } else {
            1.0
        };
        let mut by = Vec2::ZERO;
        for (arrow, way) in [
            (egui::Key::ArrowLeft, Vec2::new(-1.0, 0.0)),
            (egui::Key::ArrowRight, Vec2::new(1.0, 0.0)),
            (egui::Key::ArrowUp, Vec2::new(0.0, -1.0)),
            (egui::Key::ArrowDown, Vec2::new(0.0, 1.0)),
        ] {
            if key(arrow, egui::Modifiers::NONE) || key(arrow, egui::Modifiers::SHIFT) {
                by += way * step;
            }
        }
        if something
            && by != Vec2::ZERO
            && let Some(drawn) = drawn
        {
            let stroke = match &self.nudge {
                Some((stroke, when, ids)) if when.elapsed() < NUDGES_JOIN && *ids == selected => {
                    *stroke
                }
                _ => self.new_stroke(),
            };
            self.nudge = Some((stroke, Instant::now(), selected.clone()));
            let moving = scene_edit::moving(doc, drawn, &selected);
            if !moving.starts.is_empty() {
                commands.push(Command::ObjectMove {
                    ids: moving.starts.iter().map(|(id, _)| *id).collect(),
                    by: Some([by.x, by.y]),
                    to: None,
                    stroke: Some(stroke),
                });
            }
            for (id, at) in moving.now {
                let to = at + by;
                commands.push(Command::ObjectMoveNow {
                    id,
                    to: [to.x, to.y],
                    stroke: Some(stroke),
                });
            }
        }
        commands
    }

    /// Over the game's frozen picture: the grid, the outlines, and the
    /// pointer selecting, moving, turning and sizing objects.
    /// Where a point of the screen is in the scene, snapped like a move
    /// is (Alt held: not snapped).
    pub fn scene_point(
        &self,
        open: &OpenGame,
        placement: Placement,
        pos: Pos2,
    ) -> Option<[f32; 2]> {
        let drawn = open.drawn_scene()?;
        let alt = false;
        let at = open.view.place(drawn.to_scene(placement.to_game(pos)), alt);
        Some([at.x, at.y])
    }

    pub fn on_the_view(
        &mut self,
        ui: &mut egui::Ui,
        open: &OpenGame,
        placement: Placement,
    ) -> Vec<Command> {
        let mut commands = Vec::new();
        let (Some(doc), Some(drawn)) = (&open.scene, open.drawn_scene()) else {
            self.drag = None;
            return commands;
        };
        let response = ui.interact(
            placement.rect,
            ui.scope_id().with("scene view"),
            Sense::click_and_drag(),
        );
        let painter = ui.painter_at(placement.rect);
        if open.view.grid {
            draw_grid(&painter, placement, drawn, open.view.snap_step);
        }
        let (shift, alt, press) =
            ui.input(|i| (i.modifiers.shift, i.modifiers.alt, i.pointer.press_origin()));
        let tool = open.view.tool;
        let idle = self.drag.is_none();
        let lead = open
            .selected
            .iter()
            .filter_map(|id| drawn.object(*id))
            .find(|object| object.rect.is_some());
        // Turning and sizing change the file: they're for its objects.
        let in_file: Vec<u64> = open
            .selected
            .iter()
            .copied()
            .filter(|id| doc.object(*id).is_some())
            .filter(|id| {
                drawn
                    .object(*id)
                    .is_some_and(|object| object.rect.is_some())
            })
            .collect();
        let lead_in_file = in_file
            .first()
            .and_then(|id| drawn.object(*id))
            .and_then(scene_edit::shape);
        let arrows = (tool == Tool::Move && idle)
            .then_some(lead)
            .flatten()
            .and_then(|object| object.rect)
            .map(|rect| on_screen(placement, rect).center());
        let ring = (tool == Tool::Turn && idle)
            .then_some(lead_in_file)
            .flatten()
            .map(|shape| ring_of(placement, shape));
        let corners = (tool == Tool::Size && idle)
            .then_some(lead_in_file)
            .flatten();

        // A drag begins: on the ring, on a corner, on an object, or on
        // empty space (a box).
        if response.drag_started()
            && let Some(pressed) = press
        {
            let point = placement.to_game(pressed);
            let on_the_ring = ring.is_some_and(|ring| on_ring(ring, pressed));
            let corner = corners.and_then(|shape| handle_under(placement, shape, pressed));
            if let (true, Some(shape)) = (on_the_ring, lead_in_file) {
                let start = doc.object(in_file[0]).map_or(0.0, |object| object.turn);
                self.drag = Some(Gesture::Turn(Turning {
                    ids: in_file.clone(),
                    shape,
                    start,
                    pressed: point,
                    latest: start,
                    sent: None,
                    stroke: self.new_stroke(),
                }));
            } else if let (Some(handle), Some(shape)) = (corner, lead_in_file) {
                let start = doc
                    .object(in_file[0])
                    .map_or(Vec2::new(1.0, 1.0), |object| object.scale);
                self.drag = Some(Gesture::Size(Sizing {
                    ids: in_file.clone(),
                    shape,
                    start,
                    handle,
                    latest: start,
                    sent: None,
                    stroke: self.new_stroke(),
                }));
            } else {
                let axis = arrows.and_then(|from| arrow_under(from, pressed));
                let covers = |id: &u64| {
                    drawn.object(*id).and_then(scene_edit::shape).is_some_and(
                        |(rect, pivot, turn)| scene_edit::covers(rect, pivot, turn, point),
                    )
                };
                let grabbed = match (axis, lead) {
                    (Some(_), Some(lead)) => Some((lead.id, false)),
                    _ => open
                        .selected
                        .iter()
                        .copied()
                        .find(covers)
                        .or_else(|| scene_edit::object_at(drawn, point, None))
                        .map(|id| (id, false))
                        .or_else(|| scene_edit::start_at(drawn, point).map(|id| (id, true))),
                };
                match grabbed {
                    Some((id, outline)) => {
                        let mut ids = open.selected.clone();
                        if !ids.contains(&id) {
                            ids = vec![id];
                            commands.push(Command::SelectSet { ids: ids.clone() });
                        }
                        self.drag = self
                            .begin_drag(doc, drawn, id, outline, &ids, axis, pressed)
                            .map(Gesture::Move);
                    }
                    None => {
                        self.drag = Some(Gesture::Box {
                            from: pressed,
                            to: pressed,
                            add: shift,
                        });
                    }
                }
            }
        }

        // The drag goes on, or ends.
        if let Some(gesture) = &mut self.drag {
            let pointer = response.interact_pointer_pos();
            let ended = response.drag_stopped() || !response.dragged();
            let due = |sent: Option<Instant>| sent.is_none_or(|when| when.elapsed() >= SEND_EVERY);
            let label = |text: String, at: Pos2| {
                painter.text(
                    at + vec2(14.0, 14.0),
                    egui::Align2::LEFT_TOP,
                    text,
                    egui::FontId::proportional(12.0),
                    Color32::WHITE,
                );
            };
            match gesture {
                Gesture::Move(drag) => {
                    ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                    let pointer = pointer.unwrap_or(drag.pressed);
                    let moved = (pointer - drag.pressed) * placement.pixels_per_point
                        / placement.scale
                        / drag.scale;
                    let mut to = drag.lead_at + Vec2::new(moved.x, moved.y);
                    match drag.axis {
                        Some(Axis::Across) => to.y = drag.lead_at.y,
                        Some(Axis::Up) => to.x = drag.lead_at.x,
                        None => {}
                    }
                    let mut placed = open.view.place(to, alt);
                    match drag.axis {
                        Some(Axis::Across) => placed.y = drag.lead_at.y,
                        Some(Axis::Up) => placed.x = drag.lead_at.x,
                        None => {}
                    }
                    let by = placed - drag.lead_at;
                    let sent = drag.sent.map_or(Vec2::ZERO, |(by, _)| by);
                    if by != sent && (due(drag.sent.map(|(_, when)| when)) || ended) {
                        if let Some((_, first)) = drag.starts.first() {
                            let to = *first + by;
                            commands.push(Command::ObjectMove {
                                ids: drag.starts.iter().map(|(id, _)| *id).collect(),
                                by: None,
                                to: Some([to.x, to.y]),
                                stroke: Some(drag.stroke),
                            });
                        }
                        for (id, was) in &drag.now {
                            let to = *was + by;
                            commands.push(Command::ObjectMoveNow {
                                id: *id,
                                to: [to.x, to.y],
                                stroke: Some(drag.stroke),
                            });
                        }
                        drag.sent = Some((by, Instant::now()));
                    }
                    // The outlines keep up with the pointer.
                    let shift = by * drag.scale;
                    for (rect, pivot, turn) in &drag.shapes {
                        let mut rect = *rect;
                        rect.x += shift.x;
                        rect.y += shift.y;
                        let points = screen_corners(placement, (rect, *pivot + shift, *turn));
                        outline(
                            &painter,
                            points,
                            Stroke::new(2.0, theme::DUCK_YELLOW),
                            false,
                        );
                    }
                    label(format!("{}, {}", placed.x, placed.y), pointer);
                }
                Gesture::Turn(turning) => {
                    ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                    if let Some(pointer) = pointer {
                        turning.latest = scene_edit::turned(
                            turning.start,
                            turning.shape.1,
                            turning.pressed,
                            placement.to_game(pointer),
                            alt,
                        );
                    }
                    let sent = turning.sent.map_or(turning.start, |(turn, _)| turn);
                    if turning.latest != sent && (due(turning.sent.map(|(_, w)| w)) || ended) {
                        commands.push(Command::ObjectTurn {
                            ids: turning.ids.clone(),
                            by: None,
                            to: Some(turning.latest),
                            stroke: Some(turning.stroke),
                        });
                        turning.sent = Some((turning.latest, Instant::now()));
                    }
                    let (rect, pivot, turn) = turning.shape;
                    let now = (rect, pivot, turn + turning.latest - turning.start);
                    let ring = ring_of(placement, turning.shape);
                    painter.circle_stroke(ring.0, ring.1, Stroke::new(3.0, theme::DUCK_YELLOW));
                    outline(
                        &painter,
                        screen_corners(placement, now),
                        Stroke::new(2.0, theme::DUCK_YELLOW),
                        false,
                    );
                    if let Some(pointer) = pointer {
                        label(format!("{}°", short(turning.latest)), pointer);
                    }
                }
                Gesture::Size(sizing) => {
                    ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                    if let Some(pointer) = pointer {
                        let (fx, fy) = scene_edit::resized(
                            sizing.shape,
                            sizing.handle,
                            placement.to_game(pointer),
                            shift,
                        );
                        sizing.latest = Vec2::new(
                            scene_edit::tidy_scale(sizing.start.x * fx, alt),
                            scene_edit::tidy_scale(sizing.start.y * fy, alt),
                        );
                    }
                    let sent = sizing.sent.map_or(sizing.start, |(size, _)| size);
                    if sizing.latest != sent && (due(sizing.sent.map(|(_, w)| w)) || ended) {
                        commands.push(Command::ObjectResize {
                            ids: sizing.ids.clone(),
                            by: None,
                            to: Some([sizing.latest.x, sizing.latest.y]),
                            stroke: Some(sizing.stroke),
                        });
                        sizing.sent = Some((sizing.latest, Instant::now()));
                    }
                    let now = sized_shape(
                        sizing.shape,
                        sizing.latest.x / sizing.start.x,
                        sizing.latest.y / sizing.start.y,
                    );
                    outline(
                        &painter,
                        screen_corners(placement, now),
                        Stroke::new(2.0, theme::DUCK_YELLOW),
                        false,
                    );
                    if let Some(pointer) = pointer {
                        let size = sizing.latest;
                        label(format!("{} x {}", short(size.x), short(size.y)), pointer);
                    }
                }
                Gesture::Box { from, to, add } => {
                    if let Some(pointer) = pointer {
                        *to = pointer;
                    }
                    let area = Rect::from_two_pos(*from, *to);
                    painter.rect_filled(area, 0.0, theme::NIGHT_INDIGO.gamma_multiply(0.25));
                    painter.rect_stroke(
                        area,
                        0.0,
                        Stroke::new(1.0, Color32::WHITE),
                        StrokeKind::Inside,
                    );
                    if ended {
                        let mut ids = if *add {
                            open.selected.clone()
                        } else {
                            Vec::new()
                        };
                        let found = scene_edit::in_box(
                            drawn,
                            placement.to_game(*from),
                            placement.to_game(*to),
                        );
                        for id in found {
                            if !ids.contains(&id) {
                                ids.push(id);
                            }
                        }
                        commands.push(Command::SelectSet { ids });
                    }
                }
            }
            if ended {
                self.drag = None;
            }
            return commands;
        }

        // A file from Files, over the game: where it'll go, and letting go
        // puts it there.
        if let Some(file) = response.dnd_hover_payload::<DraggedFile>() {
            // (hover_pos is None while something else is being dragged.)
            let pointer = ui
                .input(|i| i.pointer.latest_pos())
                .unwrap_or(placement.rect.center());
            let at = open
                .view
                .place(drawn.to_scene(placement.to_game(pointer)), alt);
            let words = match file.kind {
                FileKind::Sound => format!(
                    "Let go to add the sound \"{}\": it plays when the scene starts",
                    file.stem()
                ),
                FileKind::Level => format!("Let go to add {} as a tile map", file.name()),
                _ => format!(
                    "Let go to add \"{}\" here   {}, {}",
                    file.stem(),
                    at.x,
                    at.y
                ),
            };
            if file.kind == FileKind::Picture {
                // A dashed outline on the grid point where it will go.
                let here = screen_point(placement, at * drawn.scale + drawn.offset);
                let outline = Rect::from_center_size(here, vec2(40.0, 40.0));
                let corners = [
                    outline.left_top(),
                    outline.right_top(),
                    outline.right_bottom(),
                    outline.left_bottom(),
                    outline.left_top(),
                ];
                painter.extend(egui::Shape::dashed_line(
                    &corners,
                    Stroke::new(2.0, theme::DUCK_YELLOW),
                    6.0,
                    4.0,
                ));
                painter.circle_filled(here, 3.0, theme::DUCK_YELLOW);
            }
            let label =
                painter.layout_no_wrap(words, egui::FontId::proportional(14.0), theme::INK_NAVY);
            // Beside the pointer, inside the view.
            let view = placement.rect.shrink(8.0);
            let mut place = pointer + vec2(16.0, -28.0);
            if place.x + label.size().x > view.right() {
                place.x = pointer.x - 16.0 - label.size().x;
            }
            place.x = place.x.max(view.left());
            place.y = place.y.max(view.top());
            let back = Rect::from_min_size(place, label.size()).expand(5.0);
            painter.rect_filled(back, 4.0, theme::DUCK_YELLOW);
            painter.galley(place, label, theme::INK_NAVY);
        }
        if let Some(file) = files_panel::dragging(ui.ctx()) {
            // What can be done with it, along the bottom of the view.
            let words = match file.kind {
                FileKind::Level => format!(
                    "Dragging {} from Files. Drop it in the scene to add it as a tile map.",
                    file.name()
                ),
                _ => format!(
                    "Dragging {} from Files. Drop it in the scene to add it as an object, or on \
                     a Picture box in the Inspector to give that object this picture. A sound \
                     works the same way.",
                    file.name()
                ),
            };
            let strip = Rect::from_min_max(
                pos2(placement.rect.left(), placement.rect.bottom() - 30.0),
                placement.rect.max,
            );
            painter.rect_filled(strip, 0.0, theme::INK_NAVY.gamma_multiply(0.9));
            let label = painter.layout(
                words,
                egui::FontId::proportional(13.0),
                egui::Color32::WHITE,
                strip.width() - 16.0,
            );
            let place = pos2(strip.left() + 8.0, strip.center().y - label.size().y / 2.0);
            painter.galley(place, label, egui::Color32::WHITE);
        }
        if let Some(file) = response.dnd_release_payload::<DraggedFile>() {
            let pointer = ui.input(|i| i.pointer.latest_pos());
            let at = pointer
                .filter(|_| file.kind == FileKind::Picture)
                .map(|pointer| {
                    let at = open
                        .view
                        .place(drawn.to_scene(placement.to_game(pointer)), alt);
                    [at.x, at.y]
                });
            commands.push(Command::FilesUse {
                path: file.path.clone(),
                at,
                ids: Vec::new(),
                key: None,
            });
            return commands;
        }

        // Nothing is being dragged: what's selected, what has moved, the
        // tool's handles, and what's under the pointer.
        let hover = response.hover_pos();
        let hovered = hover
            .map(|place| placement.to_game(place))
            .and_then(|point| {
                scene_edit::object_at(drawn, point, None)
                    .or_else(|| scene_edit::start_at(drawn, point))
            });
        draw_outlines(&painter, placement, drawn, &open.selected, hovered);
        if let Some(from) = arrows {
            draw_arrows(&painter, from, hover);
        }
        if let Some(ring) = ring {
            let near = hover.is_some_and(|hover| on_ring(ring, hover));
            let width = if near { 5.0 } else { 3.0 };
            painter.circle_stroke(ring.0, ring.1, Stroke::new(width + 2.0, Color32::BLACK));
            painter.circle_stroke(ring.0, ring.1, Stroke::new(width, theme::DUCK_YELLOW));
            painter.circle_filled(ring.0, 3.5, Color32::WHITE);
            if near {
                ui.ctx().set_cursor_icon(CursorIcon::Grab);
            }
        }
        if let Some(shape) = corners {
            let near = hover.and_then(|hover| handle_under(placement, shape, hover));
            for corner in scene_edit::corners(shape.0, shape.1, shape.2) {
                let at = screen_point(placement, corner);
                let big = near == Some(corner);
                let square =
                    Rect::from_center_size(at, egui::Vec2::splat(if big { 11.0 } else { 8.0 }));
                painter.rect_filled(square, 1.0, Color32::WHITE);
                painter.rect_stroke(
                    square,
                    1.0,
                    Stroke::new(1.5, Color32::BLACK),
                    StrokeKind::Outside,
                );
            }
            if near.is_some() {
                ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
            }
        }
        if let Some(object) = hovered.and_then(|id| drawn.object(id)) {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            let name = if object.name.is_empty() {
                object.kind.clone()
            } else {
                object.name.clone()
            };
            response.clone().on_hover_text_at_pointer(name);
        }
        if response.clicked()
            && let Some(place) = response.interact_pointer_pos()
        {
            let point = placement.to_game(place);
            // Clicking the one selected object again goes to the one
            // under it.
            let under = match open.selected.as_slice() {
                [one] if !shift => Some(*one),
                _ => None,
            };
            let hit = scene_edit::object_at(drawn, point, under)
                .or_else(|| scene_edit::start_at(drawn, point));
            match hit {
                Some(id) if shift => {
                    let mut ids = open.selected.clone();
                    if ids.contains(&id) {
                        ids.retain(|have| *have != id);
                    } else {
                        ids.push(id);
                    }
                    commands.push(Command::SelectSet { ids });
                }
                Some(id) => commands.push(Command::SelectSet { ids: vec![id] }),
                None if !shift => commands.push(Command::SelectNone),
                None => {}
            }
        }
        commands
    }

    /// What a drag will move, decided once when it begins. Dragging an
    /// object's start outline moves where it starts. Dragging the object
    /// itself moves its start too if it's still there, and moves it for
    /// this run only if the game has moved it since.
    #[allow(clippy::too_many_arguments)]
    fn begin_drag(
        &mut self,
        doc: &SceneDoc,
        drawn: &DrawnScene,
        id: u64,
        outline: bool,
        selected: &[u64],
        axis: Option<Axis>,
        pressed: Pos2,
    ) -> Option<Drag> {
        let grabbed = drawn.object(id)?;
        let scale = if grabbed.stays_on_screen {
            1.0
        } else {
            drawn.scale
        };
        let mut drag = Drag {
            axis,
            pressed,
            lead_at: Vec2::ZERO,
            scale,
            starts: Vec::new(),
            now: Vec::new(),
            shapes: Vec::new(),
            stroke: self.new_stroke(),
            sent: None,
        };
        if outline {
            drag.lead_at = doc.object(id)?.at;
            drag.starts.push((id, drag.lead_at));
            drag.shapes.extend(scene_edit::start_shape(grabbed));
            return Some(drag);
        }
        // The grabbed one first: the others go along with it.
        let ids: Vec<u64> = std::iter::once(id)
            .chain(selected.iter().copied().filter(|have| *have != id))
            .collect();
        let moving = scene_edit::moving(doc, drawn, &ids);
        drag.lead_at = moving
            .starts
            .iter()
            .chain(&moving.now)
            .find(|(have, _)| *have == id)?
            .1;
        drag.shapes = moving
            .starts
            .iter()
            .chain(&moving.now)
            .filter_map(|(id, _)| drawn.object(*id))
            .filter_map(scene_edit::shape)
            .collect();
        drag.starts = moving.starts;
        drag.now = moving.now;
        Some(drag)
    }

    /// The Add window: the plain kinds, and the picture or level a kind
    /// needs. The new object appears in the middle of the view, selected.
    pub fn add_window(&mut self, ctx: &egui::Context, open: &OpenGame) -> Vec<Command> {
        let mut commands = Vec::new();
        let Some(mut adding) = self.adding.take() else {
            return commands;
        };
        let folder = open.game.folder.clone();
        let pictures = folder
            .as_deref()
            .map_or(Vec::new(), |folder| self.pictures_of(folder));
        let levels = folder.as_deref().map_or(Vec::new(), levels_in);
        let sounds = folder
            .as_deref()
            .map_or(Vec::new(), |folder| self.sounds_of(folder));
        let mut keep = true;
        let modal = egui::Modal::new(egui::Id::unique("add an object")).show(ctx, |ui| {
            ui.set_width(600.0);
            ui.heading("Add an object");
            ui.label(RichText::new("Plain kinds: they do nothing by themselves.").weak());
            ui.add_space(6.0);
            for pair in PLAIN_KINDS.chunks(2) {
                ui.horizontal(|ui| {
                    for plain in pair {
                        let chosen = adding.kind == Some(plain.kind);
                        let size = egui::vec2(290.0, 46.0);
                        let layout = egui::Layout::top_down(egui::Align::Min);
                        let inner = ui.allocate_ui_with_layout(size, layout, |ui| {
                            ui.set_min_size(size);
                            ui.add_enabled_ui(plain.not_yet.is_none(), |ui| {
                                let pick = ui
                                    .selectable_label(chosen, RichText::new(plain.title).strong());
                                ui.label(RichText::new(plain.says).weak().small());
                                pick
                            })
                            .inner
                        });
                        let pick = match plain.not_yet {
                            Some(why) => inner.inner.on_disabled_hover_text(why),
                            None => inner.inner,
                        };
                        if pick.clicked() && !chosen {
                            adding.kind = Some(plain.kind);
                            adding.choice = match plain.needs {
                                Needs::Picture => pictures.first().cloned(),
                                Needs::Level => levels.first().cloned(),
                                Needs::Sound => sounds.first().cloned(),
                                Needs::Nothing => None,
                            };
                        }
                    }
                });
            }
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Text and Button come with several scenes (step 7). Ready-made kinds, like \
                     a player or a moving platform, come in step 6, and kinds written in your \
                     own code in step 5. You can also drag a picture or a sound from Files \
                     straight into the scene.",
                )
                .weak()
                .small(),
            );
            let needs = adding
                .kind
                .and_then(scene_edit::plain_kind)
                .map(|plain| plain.needs);
            let (words, choices, none) = match needs {
                Some(Needs::Picture) => (
                    "Which picture",
                    &pictures,
                    "This game has no pictures yet. Add one in Files first.",
                ),
                Some(Needs::Level) => (
                    "Which level",
                    &levels,
                    "This game has no level yet: a .txt file with a .legend.json beside it.",
                ),
                Some(Needs::Sound) => (
                    "Which sound",
                    &sounds,
                    "This game has no sounds yet. Add a .wav file in Files first.",
                ),
                _ => ("", &pictures, ""),
            };
            if matches!(needs, Some(Needs::Picture | Needs::Level | Needs::Sound)) {
                ui.add_space(6.0);
                if choices.is_empty() {
                    ui.label(RichText::new(none).color(theme::LOOK));
                } else {
                    ui.horizontal(|ui| {
                        ui.label(words);
                        egui::ComboBox::from_id_salt("add which")
                            .selected_text(adding.choice.clone().unwrap_or_default())
                            .width(260.0)
                            .show_ui(ui, |ui| {
                                for choice in choices {
                                    let chosen = adding.choice.as_deref() == Some(choice);
                                    if ui.selectable_label(chosen, choice).clicked() {
                                        adding.choice = Some(choice.clone());
                                    }
                                }
                            });
                    });
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("It appears in the middle of the view, selected.")
                        .weak()
                        .small(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ready = match needs {
                        Some(Needs::Nothing) => true,
                        Some(_) => adding.choice.is_some(),
                        None => false,
                    };
                    if ui
                        .add_enabled(ready, theme::primary_button("Add it"))
                        .clicked()
                        && let Some(kind) = adding.kind
                    {
                        let key = match needs {
                            Some(Needs::Picture) => Some("picture"),
                            Some(Needs::Level) => Some("level"),
                            Some(Needs::Sound) => Some("sound"),
                            _ => None,
                        };
                        let mut set = Map::new();
                        if let (Some(key), Some(choice)) = (key, &adding.choice) {
                            set.insert(String::from(key), json!(choice));
                        }
                        commands.push(Command::ObjectAdd {
                            kind: String::from(kind),
                            at: None,
                            name: None,
                            set,
                        });
                        keep = false;
                    }
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                });
            });
        });
        if modal.should_close() {
            keep = false;
        }
        if keep {
            self.adding = Some(adding);
        }
        commands
    }

    /// A box of text for one key of one object. It says when the typing
    /// is done, and what was typed if that's something new.
    fn text_box(
        &mut self,
        ui: &mut egui::Ui,
        id: u64,
        key: &str,
        current: &str,
        focus: bool,
    ) -> TextBox {
        let mut text = match &self.typing {
            Some((have, of, text)) if *have == id && of == key => text.clone(),
            _ => current.to_string(),
        };
        let fresh = self
            .typing
            .as_ref()
            .is_none_or(|(have, of, _)| *have != id || of != key);
        let response = ui.add(egui::TextEdit::singleline(&mut text).desired_width(f32::INFINITY));
        if focus && fresh {
            response.request_focus();
            self.typing = Some((id, key.to_string(), text));
            return TextBox::Typing;
        }
        if response.lost_focus() {
            self.typing = None;
            return TextBox::Done((text != current).then_some(text));
        }
        if response.has_focus() {
            self.typing = Some((id, key.to_string(), text));
        }
        TextBox::Typing
    }

    /// The game's pictures, as scene files name them (`assets/duck.png`),
    /// read again every couple of seconds.
    fn pictures_of(&mut self, folder: &Path) -> Vec<String> {
        let fresh = self
            .pictures
            .as_ref()
            .is_some_and(|(of, when, _)| of == folder && when.elapsed() < Duration::from_secs(2));
        if !fresh {
            let mut found = Vec::new();
            look_for_pictures(folder, folder, 0, &mut found);
            found.sort();
            self.pictures = Some((folder.to_path_buf(), Instant::now(), found));
        }
        self.pictures
            .as_ref()
            .map_or(Vec::new(), |(_, _, pictures)| pictures.clone())
    }

    /// The animations of a sprite sheet (`assets/hero.json`).
    fn animations_of(&mut self, file: &Path) -> Vec<String> {
        self.animations
            .entry(file.to_path_buf())
            .or_insert_with(|| {
                duckforge::prelude::SpriteSheet::load(file)
                    .map(|sheet| sheet.names().map(String::from).collect())
                    .unwrap_or_default()
            })
            .clone()
    }

    /// The game's `.wav` sounds, as scene files name them, read again
    /// every couple of seconds.
    fn sounds_of(&mut self, folder: &Path) -> Vec<String> {
        let fresh = self
            .sounds
            .as_ref()
            .is_some_and(|(of, when, _)| of == folder && when.elapsed() < Duration::from_secs(2));
        if !fresh {
            let mut found = Vec::new();
            look_for(folder, folder, 0, &mut found, &|path: &Path| {
                path.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
            });
            found.sort();
            self.sounds = Some((folder.to_path_buf(), Instant::now(), found));
        }
        self.sounds
            .as_ref()
            .map_or(Vec::new(), |(_, _, sounds)| sounds.clone())
    }

    /// A Sound object's boxes: which sound (a sound from Files can be
    /// dropped on it), whether it loops, how loud.
    fn sound_boxes(&mut self, ui: &mut egui::Ui, open: &OpenGame, start: &Object) -> Vec<Command> {
        let mut commands = Vec::new();
        let id = start.id().number();
        let set = |commands: &mut Vec<Command>, key: &str, value: Value, stroke| {
            commands.push(Command::ObjectSet {
                ids: vec![id],
                set: set_of(key, value),
                stroke,
            });
        };
        let sounds = open
            .game
            .folder
            .as_deref()
            .map_or(Vec::new(), |folder| self.sounds_of(folder));
        let sound = start.text("sound").unwrap_or_default().to_string();
        ui.horizontal(|ui| {
            ui.label("Sound");
            let combo = egui::ComboBox::from_id_salt("sound")
                .selected_text(if sound.is_empty() { "(none)" } else { &sound })
                .width(150.0)
                .show_ui(ui, |ui| {
                    for name in &sounds {
                        if ui.selectable_label(*name == sound, name).clicked() && *name != sound {
                            set(&mut commands, "sound", json!(name), None);
                        }
                    }
                });
            take_drop(
                &combo.response,
                FileKind::Sound,
                &[id],
                "sound",
                &mut commands,
            );
            if ui
                .add_enabled(!sound.is_empty(), egui::Button::new("Play"))
                .on_hover_text("Hear it now")
                .clicked()
            {
                commands.push(Command::FilesPlay {
                    path: sound.clone(),
                });
            }
        });
        let mut loops = start.flag("loops").unwrap_or(false);
        if ui
            .checkbox(&mut loops, "Plays over and over (music)")
            .changed()
        {
            set(&mut commands, "loops", json!(loops), None);
        }
        ui.horizontal(|ui| {
            ui.label("Volume");
            let mut volume = start.number("volume").unwrap_or(1.0);
            let response = ui.add(egui::Slider::new(&mut volume, 0.0..=1.0).fixed_decimals(2));
            let stroke = self.stroke_of(&response);
            if response.changed() {
                let value = number((f64::from(volume) * 100.0).round() / 100.0);
                set(&mut commands, "volume", value, stroke);
            }
        });
        ui.label(
            RichText::new(
                "It plays when the scene starts. Press Tab to hear it in the game; Shown off \
                 keeps it quiet.",
            )
            .weak()
            .small(),
        );
        ui.add_space(6.0);
        commands
    }

    /// The Inspector: the selected object's settings.
    pub fn inspector(&mut self, ui: &mut egui::Ui, open: &OpenGame) -> Vec<Command> {
        let mut commands = Vec::new();
        let Some(doc) = &open.scene else {
            ui.label(
                RichText::new(
                    "This game has no scene: its objects are made in its code, so there's \
                     nothing to pick here. Feel and Files work as always.",
                )
                .weak(),
            );
            return commands;
        };
        let drawn = open.drawn_scene();
        match open.selected.as_slice() {
            [] => {
                ui.heading("Nothing is selected");
                ui.label(
                    RichText::new("Click an object in the game, or in the Scene list on the left.")
                        .weak(),
                );
                ui.add_space(8.0);
                if let Some(file) = &doc.file {
                    ui.label(RichText::new(doc.short_name()).strong());
                    if let Some(size) = file.size {
                        ui.label(format!("{} x {} pixels", size.x, size.y));
                    }
                    ui.label(format!(
                        "{} objects, on {} layers",
                        file.objects.len(),
                        file.layers.len()
                    ));
                }
            }
            [id] => {
                let (start, now) = (doc.object(*id), drawn.and_then(|d| d.object(*id)));
                match (start, now) {
                    (Some(start), now) => {
                        commands = self.one_object(ui, open, doc, start, now);
                    }
                    (None, Some(now)) => commands = self.made_object(ui, now),
                    (None, None) => {
                        ui.label(RichText::new("That object isn't in the scene any more.").weak());
                    }
                }
            }
            many => commands = self.several(ui, open, doc, many),
        }
        commands
    }

    /// The Inspector for several objects: the settings they share. A
    /// value that isn't the same for all of them says "different";
    /// changing it changes it for all of them.
    fn several(
        &mut self,
        ui: &mut egui::Ui,
        open: &OpenGame,
        doc: &SceneDoc,
        ids: &[u64],
    ) -> Vec<Command> {
        let mut commands = Vec::new();
        let objects: Vec<&Object> = ids.iter().filter_map(|id| doc.object(*id)).collect();
        ui.heading(format!("{} objects", ids.len()));
        let names: Vec<String> = objects.iter().map(|o| scene_edit::describe(o)).collect();
        ui.label(RichText::new(names.join(", ")).weak());
        let made = ids.len() - objects.len();
        if made > 0 {
            ui.label(
                RichText::new(format!(
                    "{made} of them made by the game while playing: they can be moved for this \
                     run, and nothing else."
                ))
                .weak()
                .small(),
            );
        }
        ui.label("Drag one of them in the game to move them all. A change here changes them all.");
        let (Some(first), Some(file)) = (objects.first(), &doc.file) else {
            return commands;
        };
        ui.add_space(6.0);
        let in_file: Vec<u64> = objects.iter().map(|o| o.id().number()).collect();
        let set = |commands: &mut Vec<Command>, key: &str, value: Value, stroke| {
            commands.push(Command::ObjectSet {
                ids: in_file.clone(),
                set: set_of(key, value),
                stroke,
            });
        };
        let different = |same: bool| if same { "" } else { " (different)" };

        // Picture.
        let pictures = open
            .game
            .folder
            .as_deref()
            .map_or(Vec::new(), |folder| self.pictures_of(folder));
        let same_picture = objects.iter().all(|o| o.picture == first.picture);
        let shown_picture = match (&first.picture, same_picture) {
            (_, false) => String::from("(different)"),
            (Some(picture), true) => picture.clone(),
            (None, true) => String::from("(none)"),
        };
        ui.horizontal(|ui| {
            ui.label("Picture");
            let combo = egui::ComboBox::from_id_salt("picture of several")
                .selected_text(shown_picture)
                .width(160.0)
                .show_ui(ui, |ui| {
                    for name in &pictures {
                        let chosen = same_picture && first.picture.as_deref() == Some(name);
                        if ui.selectable_label(chosen, name).clicked() && !chosen {
                            commands.push(Command::ObjectSet {
                                ids: in_file.clone(),
                                set: Map::from_iter([
                                    (String::from("picture"), json!(name)),
                                    (String::from("animation"), Value::Null),
                                ]),
                                stroke: None,
                            });
                        }
                    }
                });
            take_drop(
                &combo.response,
                FileKind::Picture,
                &in_file,
                "picture",
                &mut commands,
            );
        });
        // Layer.
        let same_layer = objects.iter().all(|o| o.layer == first.layer);
        ui.horizontal(|ui| {
            ui.label("Layer");
            egui::ComboBox::from_id_salt("layer of several")
                .selected_text(if same_layer {
                    first.layer.as_str()
                } else {
                    "(different)"
                })
                .show_ui(ui, |ui| {
                    for layer in &file.layers {
                        if ui
                            .selectable_label(same_layer && layer.name == first.layer, &layer.name)
                            .clicked()
                        {
                            set(&mut commands, "layer", json!(layer.name), None);
                        }
                    }
                });
        });
        // Turn and size.
        let same_turn = objects.iter().all(|o| o.turn == first.turn);
        ui.horizontal(|ui| {
            ui.label(format!("Turn{}", different(same_turn)));
            let mut turn = first.turn;
            let response = ui.add(egui::DragValue::new(&mut turn).speed(1.0).suffix("°"));
            let stroke = self.stroke_of(&response);
            if response.changed() {
                set(&mut commands, "turn", number(f64::from(turn)), stroke);
            }
        });
        let same_size = objects.iter().all(|o| o.scale == first.scale);
        ui.horizontal(|ui| {
            ui.label(format!("Size{}", different(same_size)));
            let (mut x, mut y) = (first.scale.x, first.scale.y);
            let across = ui.add(
                egui::DragValue::new(&mut x)
                    .speed(0.05)
                    .range(0.05..=64.0)
                    .prefix("x "),
            );
            let stroke_x = self.stroke_of(&across);
            let down = ui.add(
                egui::DragValue::new(&mut y)
                    .speed(0.05)
                    .range(0.05..=64.0)
                    .prefix("y "),
            );
            let stroke_y = self.stroke_of(&down);
            if across.changed() || down.changed() {
                let stroke = if across.changed() { stroke_x } else { stroke_y };
                let value = json!([number(f64::from(x)), number(f64::from(y))]);
                set(&mut commands, "scale", value, stroke);
            }
        });
        // On and off.
        let flags: [Flag; 3] = [
            ("flip", "Flipped", |o| o.flip),
            ("shown", "Shown", |o| o.shown),
            ("solid", "Solid (things stop at it)", |o| o.solid),
        ];
        for (key, words, value) in flags {
            let all = objects.iter().all(|o| value(o));
            let none = objects.iter().all(|o| !value(o));
            let mut on = all;
            let response = ui.add(egui::Checkbox::new(&mut on, words).indeterminate(!all && !none));
            if response.changed() {
                set(&mut commands, key, json!(on), None);
            }
        }
        commands
    }

    /// An object the game made while playing: it has no line in the file.
    fn made_object(&mut self, ui: &mut egui::Ui, now: &DrawnObject) -> Vec<Command> {
        let mut commands = Vec::new();
        ui.heading(if now.name.is_empty() {
            &now.kind
        } else {
            &now.name
        });
        ui.label(RichText::new(format!("{} · made while playing", now.kind)).weak());
        ui.label(
            "The game made this while it played, so it isn't in the scene's file. You can move \
             it for this run.",
        );
        if let Some(at) = made_at(now) {
            ui.add_space(6.0);
            ui.label(RichText::new("Position").strong());
            if let Some((to, stroke)) = self.point_boxes(ui, "Now", at) {
                commands.push(Command::ObjectMoveNow {
                    id: now.id,
                    to: [to.x, to.y],
                    stroke,
                });
            }
        }
        commands
    }

    /// Two number boxes for a point. Says the new point when one changes.
    fn point_boxes(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        at: Vec2,
    ) -> Option<(Vec2, Option<u64>)> {
        let mut changed = None;
        ui.horizontal(|ui| {
            ui.label(label);
            let (mut x, mut y) = (at.x, at.y);
            let across = ui.add(egui::DragValue::new(&mut x).speed(1.0).prefix("x "));
            let stroke_x = self.stroke_of(&across);
            let down = ui.add(egui::DragValue::new(&mut y).speed(1.0).prefix("y "));
            let stroke_y = self.stroke_of(&down);
            if across.changed() {
                changed = Some((Vec2::new(x, y), stroke_x));
            } else if down.changed() {
                changed = Some((Vec2::new(x, y), stroke_y));
            }
        });
        changed
    }

    /// The Inspector for one object of the file.
    fn one_object(
        &mut self,
        ui: &mut egui::Ui,
        open: &OpenGame,
        doc: &SceneDoc,
        start: &Object,
        now: Option<&DrawnObject>,
    ) -> Vec<Command> {
        let mut commands = Vec::new();
        let id = start.id().number();
        let ids = vec![id];
        let set = |commands: &mut Vec<Command>, key: &str, value: Value, stroke| {
            commands.push(Command::ObjectSet {
                ids: ids.clone(),
                set: set_of(key, value),
                stroke,
            });
        };
        let gone = now.is_some_and(|now| now.gone);
        // What the game has changed since the start, key by key.
        let changed = now.filter(|now| !now.gone).map(|now| &now.now);

        ui.horizontal(|ui| {
            ui.label("Name");
            if let TextBox::Done(Some(name)) = self.text_box(ui, id, "name", &start.name, false) {
                commands.push(Command::ObjectRename { id, name });
            }
        });
        ui.label(RichText::new(format!("Kind: {}", start.kind)).weak());
        if gone {
            ui.label(
                RichText::new(
                    "The game has removed it in this run. It's still in the scene's file: what \
                     you change here is how it starts.",
                )
                .color(theme::LOOK),
            );
        }
        ui.add_space(6.0);

        // Position: where it starts, and where it is now if that's
        // somewhere else.
        ui.label(RichText::new("Position").strong());
        let moved = changed
            .and_then(|changed| changed.get("at"))
            .and_then(scene_edit::point_of);
        let label = if moved.is_some() { "Starts at" } else { "At" };
        if let Some((to, stroke)) = self.point_boxes(ui, label, start.at) {
            set(&mut commands, "at", pair(to), stroke);
        }
        if let Some(at) = moved {
            if let Some((to, stroke)) = self.point_boxes(ui, "Now", at) {
                commands.push(Command::ObjectMoveNow {
                    id,
                    to: [to.x, to.y],
                    stroke,
                });
            }
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button("Start here")
                    .on_hover_text("Where it is now becomes where it starts (the file changes)")
                    .clicked()
                {
                    commands.push(Command::ObjectStartHere { ids: vec![id] });
                }
                if ui
                    .button("Back to its start")
                    .on_hover_text("Put it back where it starts, on this frame")
                    .clicked()
                {
                    commands.push(Command::ObjectBackToStart { ids: vec![id] });
                }
            });
            ui.label(
                RichText::new(
                    "The game has moved it since the start. The dashed outline is where it \
                     starts: drag that to change the file. Dragging the object itself moves it \
                     for this run only.",
                )
                .weak()
                .small(),
            );
        }
        ui.add_space(6.0);

        if start.kind == "tilemap" {
            ui.label(
                RichText::new(
                    "A tile map: while it's selected, the tools on the left paint on it, as \
                     they always have.",
                )
                .weak(),
            );
            ui.add_space(6.0);
        }
        if start.kind == "area" {
            // An area has no picture: its box is what it is.
            let size = start.hitbox.map_or(
                Vec2::new(scene_edit::AREA_SIZE, scene_edit::AREA_SIZE),
                |b| Vec2::new(b.w, b.h),
            );
            ui.horizontal(|ui| {
                ui.label("Box");
                let (mut w, mut h) = (size.x, size.y);
                let across = ui.add(
                    egui::DragValue::new(&mut w)
                        .speed(1.0)
                        .range(1.0..=4096.0)
                        .prefix("w "),
                );
                let stroke_w = self.stroke_of(&across);
                let down = ui.add(
                    egui::DragValue::new(&mut h)
                        .speed(1.0)
                        .range(1.0..=4096.0)
                        .prefix("h "),
                );
                let stroke_h = self.stroke_of(&down);
                if across.changed() || down.changed() {
                    let stroke = if across.changed() { stroke_w } else { stroke_h };
                    let value = json!([
                        0,
                        0,
                        number(f64::from(w.round())),
                        number(f64::from(h.round()))
                    ]);
                    set(&mut commands, "box", value, stroke);
                }
            });
            ui.label(
                RichText::new("An area isn't drawn in the game: the Studio shows it dashed.")
                    .weak()
                    .small(),
            );
            ui.add_space(6.0);
        }
        if start.kind == "sound" {
            commands.extend(self.sound_boxes(ui, open, start));
        }
        // Picture, and its animation if it's a sprite sheet.
        let folder = open.game.folder.clone();
        let pictures = folder
            .as_deref()
            .map_or(Vec::new(), |folder| self.pictures_of(folder));
        let picture = start.picture.clone().unwrap_or_default();
        let has_picture = !matches!(start.kind.as_str(), "tilemap" | "area" | "sound");
        if has_picture {
            ui.horizontal(|ui| {
                ui.label("Picture");
                let combo = egui::ComboBox::from_id_salt("picture")
                    .selected_text(if picture.is_empty() {
                        "(none)"
                    } else {
                        &picture
                    })
                    .width(160.0)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(picture.is_empty(), "(none)").clicked() {
                            commands.push(Command::ObjectSet {
                                ids: vec![id],
                                set: Map::from_iter([
                                    (String::from("picture"), Value::Null),
                                    (String::from("animation"), Value::Null),
                                ]),
                                stroke: None,
                            });
                        }
                        for name in &pictures {
                            if ui.selectable_label(*name == picture, name).clicked()
                                && *name != picture
                            {
                                // Another picture has other animations.
                                commands.push(Command::ObjectSet {
                                    ids: vec![id],
                                    set: Map::from_iter([
                                        (String::from("picture"), json!(name)),
                                        (String::from("animation"), Value::Null),
                                    ]),
                                    stroke: None,
                                });
                            }
                        }
                    });
                take_drop(
                    &combo.response,
                    FileKind::Picture,
                    &[id],
                    "picture",
                    &mut commands,
                );
            });
        }
        if picture.ends_with(".json")
            && let Some(folder) = &folder
        {
            let animations = self.animations_of(&folder.join(&picture));
            if !animations.is_empty() {
                let animation = start.animation.clone().unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label("Animation");
                    egui::ComboBox::from_id_salt("animation")
                        .selected_text(if animation.is_empty() {
                            "(the usual one)"
                        } else {
                            &animation
                        })
                        .show_ui(ui, |ui| {
                            for name in &animations {
                                if ui.selectable_label(*name == animation, name).clicked() {
                                    set(&mut commands, "animation", json!(name), None);
                                }
                            }
                        });
                });
            }
        }

        // Layer (a sound isn't drawn, so it has none worth choosing).
        if let Some(file) = doc.file.as_ref().filter(|_| start.kind != "sound") {
            ui.horizontal(|ui| {
                ui.label("Layer");
                egui::ComboBox::from_id_salt("layer")
                    .selected_text(&start.layer)
                    .show_ui(ui, |ui| {
                        for layer in &file.layers {
                            let words = if layer.stays_on_screen {
                                format!("{} (stays on screen)", layer.name)
                            } else {
                                layer.name.clone()
                            };
                            if ui
                                .selectable_label(layer.name == start.layer, words)
                                .clicked()
                                && layer.name != start.layer
                            {
                                set(&mut commands, "layer", json!(layer.name), None);
                            }
                        }
                    });
            });
        }
        ui.add_space(6.0);

        egui::CollapsingHeader::new("More")
            .default_open(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Turn");
                    let mut turn = start.turn;
                    let response = ui.add(egui::DragValue::new(&mut turn).speed(1.0).suffix("°"));
                    let stroke = self.stroke_of(&response);
                    if response.changed() {
                        set(&mut commands, "turn", number(f64::from(turn)), stroke);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Size");
                    let (mut x, mut y) = (start.scale.x, start.scale.y);
                    let across = ui.add(
                        egui::DragValue::new(&mut x)
                            .speed(0.05)
                            .range(0.05..=64.0)
                            .prefix("x "),
                    );
                    let stroke_x = self.stroke_of(&across);
                    let down = ui.add(
                        egui::DragValue::new(&mut y)
                            .speed(0.05)
                            .range(0.05..=64.0)
                            .prefix("y "),
                    );
                    let stroke_y = self.stroke_of(&down);
                    if across.changed() || down.changed() {
                        let stroke = if across.changed() { stroke_x } else { stroke_y };
                        let value = json!([number(f64::from(x)), number(f64::from(y))]);
                        set(&mut commands, "scale", value, stroke);
                    }
                    ui.label(RichText::new("times").weak());
                });
                for (key, words, value) in [
                    ("flip", "Flipped", start.flip),
                    ("shown", "Shown", start.shown),
                    ("solid", "Solid (things stop at it)", start.solid),
                ] {
                    let mut value = value;
                    if ui.checkbox(&mut value, words).changed() {
                        set(&mut commands, key, json!(value), None);
                    }
                }
                ui.horizontal(|ui| {
                    ui.label("Opacity");
                    let mut opacity = start.opacity;
                    let response = ui.add(egui::Slider::new(&mut opacity, 0.0..=1.0));
                    let stroke = self.stroke_of(&response);
                    if response.changed() {
                        let value = number((f64::from(opacity) * 100.0).round() / 100.0);
                        set(&mut commands, "opacity", value, stroke);
                    }
                });
                // The object's own settings.
                // A sound's own keys have boxes of their own, above.
                let shown_above: &[&str] = if start.kind == "sound" {
                    &["sound", "loops", "volume"]
                } else {
                    &[]
                };
                let names: Vec<String> = start
                    .setting_names()
                    .filter(|name| !shown_above.contains(name))
                    .map(String::from)
                    .collect();
                if !names.is_empty() {
                    ui.add_space(4.0);
                    ui.label(RichText::new("Its own settings").strong());
                }
                for name in names {
                    let Some(value) = start.setting(&name) else {
                        continue;
                    };
                    ui.horizontal(|ui| {
                        ui.label(&name);
                        match value {
                            Value::Number(n) => {
                                let mut n = n.as_f64().unwrap_or(0.0);
                                let response = ui.add(egui::DragValue::new(&mut n).speed(0.5));
                                let stroke = self.stroke_of(&response);
                                if response.changed() {
                                    set(&mut commands, &name, number(n), stroke);
                                }
                            }
                            Value::Bool(on) => {
                                let mut on = *on;
                                if ui.checkbox(&mut on, "").changed() {
                                    set(&mut commands, &name, json!(on), None);
                                }
                            }
                            Value::String(text) => {
                                if let TextBox::Done(Some(text)) =
                                    self.text_box(ui, id, &name, text, false)
                                {
                                    set(&mut commands, &name, json!(text), None);
                                }
                            }
                            other => {
                                ui.label(RichText::new(other.to_string()).weak())
                                    .on_hover_text("Change this one in the scene's file");
                            }
                        }
                    });
                }
            });

        // What else the game has changed since the start.
        let others: Vec<(&String, &Value)> = changed
            .into_iter()
            .flatten()
            .filter(|(key, _)| *key != "at")
            .collect();
        if !others.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new("Changed by the game since the start").strong());
            for (key, value) in others {
                ui.label(RichText::new(format!("{key}: now {value}")).weak());
            }
        }

        ui.add_space(8.0);
        let line = doc.line_of(id).map_or(String::new(), |line| {
            format!(" · line {line} of {}", doc.short_name())
        });
        ui.label(RichText::new(format!("id {id}{line}")).weak().small());
        commands
    }
}

/// What a text box has to say after a frame.
enum TextBox {
    Typing,
    /// The typing is over: the new text, if it's different.
    Done(Option<String>),
}

/// Where an object the game made is, from its line.
fn made_at(object: &DrawnObject) -> Option<Vec2> {
    object.now.get("at").and_then(scene_edit::point_of)
}

/// Pictures and sprite sheets under `folder`, as `assets/duck.png`.
fn look_for_pictures(root: &Path, folder: &Path, depth: usize, found: &mut Vec<String>) {
    look_for(root, folder, depth, found, &|path: &Path| {
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        // A sprite sheet is a .json beside a .png of the same name.
        extension == "png" || (extension == "json" && path.with_extension("png").exists())
    });
}

/// Files under `folder` that `wanted` likes, as `assets/duck.png`.
fn look_for(
    root: &Path,
    folder: &Path,
    depth: usize,
    found: &mut Vec<String>,
    wanted: &dyn Fn(&Path) -> bool,
) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let hidden = path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.'));
            if depth < 3 && !hidden {
                look_for(root, &path, depth + 1, found, wanted);
            }
            continue;
        }
        if wanted(&path) {
            let name = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            found.push(name);
        }
    }
}

/// The game's levels a tile map can show, as scene files name them
/// (`level.txt`): text files with a legend beside them.
fn levels_in(folder: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut look = vec![(folder.to_path_buf(), 0)];
    while let Some((here, depth)) = look.pop() {
        let Ok(entries) = std::fs::read_dir(&here) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let hidden = path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with('.'));
                if depth < 2 && !hidden {
                    look.push((path, depth + 1));
                }
            } else if path.extension().is_some_and(|e| e == "txt")
                && path.with_extension("legend.json").exists()
            {
                let name = path
                    .strip_prefix(folder)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                found.push(name);
            }
        }
    }
    found.sort();
    found
}

/// The grid, where the scene's pixels are on screen.
fn draw_grid(painter: &egui::Painter, placement: Placement, drawn: &DrawnScene, step: f32) {
    let each = placement.scale / placement.pixels_per_point; // points per canvas pixel
    let square = step * drawn.scale * each;
    if square < 6.0 {
        return;
    }
    let rect = placement.rect;
    let faint = Stroke::new(1.0, Color32::from_white_alpha(18));
    let strong = Stroke::new(1.0, Color32::from_white_alpha(40));
    // The scene's (0, 0) on screen.
    let origin = rect.min + vec2(drawn.offset.x, drawn.offset.y) * each;
    let first = ((rect.min - origin) / square).floor();
    let mut column = first.x as i64;
    loop {
        let x = origin.x + column as f32 * square;
        if x > rect.right() {
            break;
        }
        if x >= rect.left() {
            let stroke = if column.rem_euclid(4) == 0 {
                strong
            } else {
                faint
            };
            painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], stroke);
        }
        column += 1;
    }
    let mut row = first.y as i64;
    loop {
        let y = origin.y + row as f32 * square;
        if y > rect.bottom() {
            break;
        }
        if y >= rect.top() {
            let stroke = if row.rem_euclid(4) == 0 {
                strong
            } else {
                faint
            };
            painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], stroke);
        }
        row += 1;
    }
}

/// Outlines: where each moved object starts (dashed), the selection, what's
/// under the pointer, and areas (which the game doesn't draw), each turned
/// as its picture is.
fn draw_outlines(
    painter: &egui::Painter,
    placement: Placement,
    drawn: &DrawnScene,
    selected: &[u64],
    hovered: Option<u64>,
) {
    let area_colour = Color32::from_rgba_unmultiplied(120, 220, 255, 170);
    for object in &drawn.objects {
        if object.origin == Origin::Painted {
            continue;
        }
        let chosen = selected.contains(&object.id);
        let hover = hovered == Some(object.id);
        if let Some(start) = scene_edit::start_shape(object) {
            // Where it starts: only the selected and the hovered one show
            // strongly, so a busy scene stays readable.
            let colour = if chosen {
                theme::DUCK_YELLOW
            } else if hover {
                Color32::WHITE
            } else {
                Color32::from_white_alpha(110)
            };
            let points = screen_corners(placement, start);
            if !object.gone || chosen || hover {
                outline(
                    painter,
                    points.clone(),
                    Stroke::new(if chosen { 2.0 } else { 1.0 }, colour),
                    true,
                );
            }
            if chosen && let Some(now) = scene_edit::shape(object) {
                painter.line_segment(
                    [
                        screen_point(placement, start.1),
                        screen_point(placement, now.1),
                    ],
                    Stroke::new(1.0, theme::DUCK_YELLOW.gamma_multiply(0.6)),
                );
            }
            if chosen {
                let top = points
                    .iter()
                    .copied()
                    .min_by(|a, b| a.y.total_cmp(&b.y))
                    .unwrap_or_default();
                painter.text(
                    top - vec2(0.0, 2.0),
                    egui::Align2::LEFT_BOTTOM,
                    "starts here",
                    egui::FontId::proportional(11.0),
                    theme::DUCK_YELLOW,
                );
            }
        }
        let Some(now) = scene_edit::shape(object) else {
            continue;
        };
        let points = screen_corners(placement, now);
        if chosen {
            outline(painter, points, Stroke::new(2.0, theme::DUCK_YELLOW), false);
        } else if hover {
            outline(painter, points, Stroke::new(1.0, Color32::WHITE), false);
        } else if object.kind == "area" {
            outline(painter, points, Stroke::new(1.0, area_colour), true);
        }
    }
}

/// Where the Move tool's two arrows end.
fn arrow_tips(from: Pos2) -> (Pos2, Pos2) {
    (from + vec2(ARROW, 0.0), from + vec2(0.0, -ARROW))
}

/// The arrow of the Move tool under the pointer, if any.
fn arrow_under(from: Pos2, pointer: Pos2) -> Option<Axis> {
    let (across, up) = arrow_tips(from);
    let near = |tip: Pos2| {
        let along = tip - from;
        let t = ((pointer - from).dot(along) / along.length_sq()).clamp(0.25, 1.15);
        (pointer - (from + along * t)).length() <= 7.0
    };
    if near(across) {
        Some(Axis::Across)
    } else if near(up) {
        Some(Axis::Up)
    } else {
        None
    }
}

fn draw_arrows(painter: &egui::Painter, from: Pos2, pointer: Option<Pos2>) {
    let over = pointer.and_then(|pointer| arrow_under(from, pointer));
    let (across, up) = arrow_tips(from);
    for (tip, axis, colour) in [
        (across, Axis::Across, theme::BAD),
        (up, Axis::Up, theme::GOOD),
    ] {
        let width = if over == Some(axis) { 5.0 } else { 3.0 };
        let along = (tip - from).normalized();
        let side = vec2(-along.y, along.x);
        let neck = tip - along * 10.0;
        let head = |grow: f32| {
            vec![
                tip + along * grow,
                neck + side * (6.0 + grow) - along * grow,
                neck - side * (6.0 + grow) - along * grow,
            ]
        };
        // Dark underneath, so they show on any picture.
        painter.line_segment([from, neck], Stroke::new(width + 2.0, Color32::BLACK));
        painter.add(Shape::convex_polygon(
            head(1.5),
            Color32::BLACK,
            Stroke::NONE,
        ));
        painter.line_segment([from, neck], Stroke::new(width, colour));
        painter.add(Shape::convex_polygon(head(0.0), colour, Stroke::NONE));
    }
    painter.circle_filled(from, 4.5, Color32::BLACK);
    painter.circle_filled(from, 3.5, Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arrows_are_grabbed_along_their_length_not_at_the_middle() {
        let from = pos2(100.0, 100.0);
        assert_eq!(arrow_under(from, pos2(130.0, 102.0)), Some(Axis::Across));
        assert_eq!(arrow_under(from, pos2(98.0, 70.0)), Some(Axis::Up));
        assert_eq!(
            arrow_under(from, pos2(101.0, 99.0)),
            None,
            "the middle is the object itself: a free drag"
        );
        assert_eq!(arrow_under(from, pos2(130.0, 130.0)), None);
    }

    #[test]
    fn a_rectangle_of_the_picture_is_where_it_shows_on_screen() {
        let placement = Placement {
            rect: Rect::from_min_size(pos2(200.0, 50.0), vec2(640.0, 480.0)),
            scale: 3.0,
            pixels_per_point: 1.5,
        };
        let rect = on_screen(
            placement,
            duckforge::prelude::Rect::new(10.0, 20.0, 16.0, 8.0),
        );
        assert_eq!(rect.min, pos2(220.0, 90.0));
        assert_eq!(rect.size(), vec2(32.0, 16.0));
    }

    #[test]
    fn numbers_are_written_whole_when_they_are() {
        assert_eq!(number(8.0).to_string(), "8");
        assert_eq!(number(1.5).to_string(), "1.5");
        assert_eq!(number(0.30000001).to_string(), "0.3");
    }

    #[test]
    fn the_ring_goes_round_the_picture_and_is_grabbed_near_its_line() {
        let placement = Placement {
            rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(640.0, 480.0)),
            scale: 2.0,
            pixels_per_point: 1.0,
        };
        // 16 x 16 standing on its feet at (50, 50): its far corners are
        // 16 * sqrt(1.25) = 17.9 game pixels away, 35.8 points.
        let shape = (
            duckforge::prelude::Rect::new(42.0, 34.0, 16.0, 16.0),
            Vec2::new(50.0, 50.0),
            0.0,
        );
        let (middle, radius) = ring_of(placement, shape);
        assert_eq!(middle, pos2(100.0, 100.0));
        assert!((radius - 47.8).abs() < 0.1, "{radius}");
        assert!(on_ring((middle, radius), pos2(100.0 + radius + 5.0, 100.0)));
        assert!(
            !on_ring((middle, radius), pos2(110.0, 100.0)),
            "inside is the picture"
        );
        assert_eq!(
            handle_under(placement, shape, pos2(116.0, 69.0)),
            Some(Vec2::new(58.0, 34.0)),
            "the top-right corner"
        );
        assert_eq!(handle_under(placement, shape, pos2(100.0, 80.0)), None);
    }

    #[test]
    fn a_sized_picture_keeps_its_feet_where_they_are() {
        let shape = (
            duckforge::prelude::Rect::new(42.0, 34.0, 16.0, 16.0),
            Vec2::new(50.0, 50.0),
            15.0,
        );
        let (rect, pivot, turn) = sized_shape(shape, 2.0, 1.5);
        assert_eq!(rect, duckforge::prelude::Rect::new(34.0, 26.0, 32.0, 24.0));
        assert_eq!((pivot, turn), (Vec2::new(50.0, 50.0), 15.0));
        assert_eq!(short(1.5), "1.5");
        assert_eq!(short(2.0), "2");
        assert_eq!(short(-7.25), "-7.25");
    }
}
