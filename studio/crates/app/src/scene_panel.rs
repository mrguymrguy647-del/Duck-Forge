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
use studio_core::scene_edit::{self, RowState, SceneDoc, Show, Tool, at_now, pair};
use studio_core::studio::{Command, OpenGame};

use crate::keys::Placement;
use crate::theme;

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

/// A drag in the view, from press to release.
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
    rects: Vec<duckforge::prelude::Rect>,
    stroke: u64,
    /// How far it has been moved, as last sent, and when.
    sent: Option<(Vec2, Instant)>,
}

/// The scene editor's own state: the drag in progress, the text being
/// typed, what it has read from disk.
pub struct ScenePanel {
    drag: Option<Drag>,
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

fn dashed(painter: &egui::Painter, rect: Rect, stroke: Stroke) {
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    painter.extend(Shape::dashed_line(&corners, stroke, 5.0, 4.0));
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

    /// The Scene list: every object of the scene, in drawing order.
    pub fn list(&mut self, ui: &mut egui::Ui, open: &OpenGame) -> Vec<Command> {
        let mut commands = Vec::new();
        let Some(doc) = &open.scene else {
            return commands;
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new("Scene").strong());
            ui.label(RichText::new(doc.short_name()).weak());
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
        let rows = open.scene_rows();
        if rows.is_empty() && doc.problem.is_none() {
            ui.label(RichText::new("This scene has no objects yet.").weak());
        }
        let (shift, ctrl) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
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
            if response.double_clicked() && row.state != RowState::Made {
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
        ui.label(
            RichText::new("Click to select, double-click to rename. The last is drawn on top.")
                .weak()
                .small(),
        );
        commands
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

    /// V and G pick the tool, Escape selects nothing.
    pub fn keys(&mut self, ui: &egui::Ui, open: &OpenGame) -> Vec<Command> {
        let mut commands = Vec::new();
        let (v, g, escape) = ui.input(|i| {
            let plain = i.modifiers.is_none();
            (
                plain && i.key_pressed(egui::Key::V),
                plain && i.key_pressed(egui::Key::G),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if v {
            commands.push(Command::ViewTool { tool: Tool::Select });
        }
        if g {
            commands.push(Command::ViewTool { tool: Tool::Move });
        }
        if escape && !open.selected.is_empty() && self.drag.is_none() {
            commands.push(Command::SelectNone);
        }
        commands
    }

    /// Over the game's frozen picture: the grid, the outlines, and the
    /// pointer selecting and moving objects.
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
        let lead = open
            .selected
            .iter()
            .filter_map(|id| drawn.object(*id))
            .find(|object| object.rect.is_some());
        let arrows = (open.view.tool == Tool::Move && self.drag.is_none())
            .then_some(lead)
            .flatten()
            .and_then(|object| object.rect)
            .map(|rect| on_screen(placement, rect).center());

        // A drag begins.
        if response.drag_started()
            && let Some(pressed) = press
        {
            let point = placement.to_game(pressed);
            let axis = arrows.and_then(|from| arrow_under(from, pressed));
            let grabbed = match (axis, lead) {
                (Some(_), Some(lead)) => Some((lead.id, false)),
                _ => open
                    .selected
                    .iter()
                    .copied()
                    .find(|id| {
                        drawn
                            .object(*id)
                            .and_then(|object| object.rect)
                            .is_some_and(|rect| rect.contains(point))
                    })
                    .or_else(|| scene_edit::object_at(drawn, point, None))
                    .map(|id| (id, false))
                    .or_else(|| scene_edit::start_at(drawn, point).map(|id| (id, true))),
            };
            if let Some((id, outline)) = grabbed {
                let mut ids = open.selected.clone();
                if !ids.contains(&id) {
                    ids = vec![id];
                    commands.push(Command::SelectSet { ids: ids.clone() });
                }
                self.drag = self.begin_drag(doc, drawn, id, outline, &ids, axis, pressed);
            }
        }

        // The drag goes on, or ends.
        if let Some(drag) = &mut self.drag {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            let pointer = response.interact_pointer_pos().unwrap_or(drag.pressed);
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
            let ended = response.drag_stopped() || !response.dragged();
            let due = drag
                .sent
                .is_none_or(|(_, when)| when.elapsed() >= SEND_EVERY);
            let sent = drag.sent.map_or(Vec2::ZERO, |(by, _)| by);
            if by != sent && (due || ended) {
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
            for rect in &drag.rects {
                let mut rect = *rect;
                rect.x += by.x * drag.scale;
                rect.y += by.y * drag.scale;
                painter.rect_stroke(
                    on_screen(placement, rect),
                    0.0,
                    Stroke::new(2.0, theme::DUCK_YELLOW),
                    StrokeKind::Outside,
                );
            }
            painter.text(
                pointer + vec2(14.0, 14.0),
                egui::Align2::LEFT_TOP,
                format!("{}, {}", placed.x, placed.y),
                egui::FontId::proportional(12.0),
                Color32::WHITE,
            );
            if ended {
                self.drag = None;
            }
            return commands;
        }

        // Nothing is being dragged: what's selected, what has moved, and
        // what's under the pointer.
        let hovered = response
            .hover_pos()
            .map(|place| placement.to_game(place))
            .and_then(|point| {
                scene_edit::object_at(drawn, point, None)
                    .or_else(|| scene_edit::start_at(drawn, point))
            });
        draw_outlines(&painter, placement, drawn, &open.selected, hovered);
        if let Some(from) = arrows {
            draw_arrows(&painter, from, response.hover_pos());
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
            rects: Vec::new(),
            stroke: self.new_stroke(),
            sent: None,
        };
        if outline {
            drag.lead_at = doc.object(id)?.at;
            drag.starts.push((id, drag.lead_at));
            drag.rects.extend(grabbed.start_rect);
            return Some(drag);
        }
        drag.lead_at = at_now(doc, Some(drawn), id).or_else(|| made_at(grabbed))?;
        // The grabbed one first: the others go along with it.
        let ids = std::iter::once(id).chain(selected.iter().copied().filter(|have| *have != id));
        for id in ids {
            let Some(object) = drawn.object(id) else {
                continue;
            };
            if object.gone || matches!(object.kind.as_str(), "tilemap" | "background") {
                continue;
            }
            let moved = object.origin == Origin::Made || object.now.contains_key("at");
            match (moved, doc.object(id)) {
                (false, Some(start)) => drag.starts.push((id, start.at)),
                _ => {
                    let Some(at) = at_now(doc, Some(drawn), id).or_else(|| made_at(object)) else {
                        continue;
                    };
                    drag.now.push((id, at));
                }
            }
            drag.rects.extend(object.rect);
        }
        (!drag.starts.is_empty() || !drag.now.is_empty()).then_some(drag)
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
            many => {
                ui.heading(format!("{} objects", many.len()));
                let objects: Vec<&Object> = many.iter().filter_map(|id| doc.object(*id)).collect();
                for object in &objects {
                    ui.label(RichText::new(scene_edit::describe(object)).weak());
                }
                ui.add_space(6.0);
                ui.label("Drag one of them in the game to move them all.");
                if let (Some(first), Some(file)) = (objects.first(), &doc.file) {
                    let shared = objects.iter().all(|object| object.layer == first.layer);
                    let shown = if shared {
                        first.layer.as_str()
                    } else {
                        "(different)"
                    };
                    let ids: Vec<u64> = objects.iter().map(|o| o.id().number()).collect();
                    ui.horizontal(|ui| {
                        ui.label("Layer");
                        egui::ComboBox::from_id_salt("layer of several")
                            .selected_text(shown)
                            .show_ui(ui, |ui| {
                                for layer in &file.layers {
                                    if ui
                                        .selectable_label(
                                            shared && layer.name == first.layer,
                                            &layer.name,
                                        )
                                        .clicked()
                                    {
                                        commands.push(Command::ObjectSet {
                                            ids: ids.clone(),
                                            set: set_of("layer", json!(layer.name)),
                                            stroke: None,
                                        });
                                    }
                                }
                            });
                    });
                }
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
        // Picture, and its animation if it's a sprite sheet.
        let folder = open.game.folder.clone();
        let pictures = folder
            .as_deref()
            .map_or(Vec::new(), |folder| self.pictures_of(folder));
        let picture = start.picture.clone().unwrap_or_default();
        let has_picture = start.kind != "tilemap";
        if has_picture {
            ui.horizontal(|ui| {
                ui.label("Picture");
                egui::ComboBox::from_id_salt("picture")
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

        // Layer.
        if let Some(file) = &doc.file {
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
                let names: Vec<String> = start.setting_names().map(String::from).collect();
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
                look_for_pictures(root, &path, depth + 1, found);
            }
            continue;
        }
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        // A sprite sheet is a .json beside a .png of the same name.
        let is_sheet = extension == "json" && path.with_extension("png").exists();
        if extension == "png" || is_sheet {
            let name = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            found.push(name);
        }
    }
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

/// Outlines: where each moved object starts (dashed), the selection, and
/// what's under the pointer.
fn draw_outlines(
    painter: &egui::Painter,
    placement: Placement,
    drawn: &DrawnScene,
    selected: &[u64],
    hovered: Option<u64>,
) {
    for object in &drawn.objects {
        if object.origin == Origin::Painted {
            continue;
        }
        let chosen = selected.contains(&object.id);
        if let Some(start) = object.start_rect {
            // Where it starts: only the selected and the hovered one show
            // strongly, so a busy scene stays readable.
            let start = on_screen(placement, start);
            let colour = if chosen {
                theme::DUCK_YELLOW
            } else if hovered == Some(object.id) {
                Color32::WHITE
            } else {
                Color32::from_white_alpha(110)
            };
            if !object.gone || chosen || hovered == Some(object.id) {
                dashed(
                    painter,
                    start,
                    Stroke::new(if chosen { 2.0 } else { 1.0 }, colour),
                );
            }
            if chosen && let Some(rect) = object.rect {
                painter.line_segment(
                    [start.center(), on_screen(placement, rect).center()],
                    Stroke::new(1.0, theme::DUCK_YELLOW.gamma_multiply(0.6)),
                );
            }
            if chosen {
                painter.text(
                    start.left_top() - vec2(0.0, 2.0),
                    egui::Align2::LEFT_BOTTOM,
                    "starts here",
                    egui::FontId::proportional(11.0),
                    theme::DUCK_YELLOW,
                );
            }
        }
        let Some(rect) = object.rect else {
            continue;
        };
        let rect = on_screen(placement, rect);
        if chosen {
            painter.rect_stroke(
                rect,
                0.0,
                Stroke::new(2.0, theme::DUCK_YELLOW),
                StrokeKind::Outside,
            );
        } else if hovered == Some(object.id) {
            painter.rect_stroke(
                rect,
                0.0,
                Stroke::new(1.0, Color32::WHITE),
                StrokeKind::Outside,
            );
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
}
