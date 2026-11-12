//! Scenes: a game's objects in a plain file that the engine, Duck Forge
//! Studio and agents all read.
//!
//! A scene file is JSON with one object per line
//! (`examples/mygame/scenes/level1.json`):
//!
//! ```json
//! { "duckforge_scene": 1,
//!   "objects": [
//!     {"id": 1, "name": "sky", "kind": "background", "layer": "far", "picture": "assets/sky.png", "parallax": 0.2},
//!     {"id": 2, "name": "ground", "kind": "tilemap", "level": "level.txt"},
//!     {"id": 3, "name": "duck", "at": [40, 208], "picture": "assets/hero.json"},
//!     {"id": 4, "name": "sign", "at": [374, 208], "turn": 15, "scale": 2, "picture": "assets/sign.png"} ] }
//! ```
//!
//! A [`Scene`] is a value the game holds. It loads the file, draws its
//! objects in layers, and gives the code every object to read and change:
//!
//! ```no_run
//! # use duckforge::prelude::*;
//! # fn main() -> Result<(), duckforge::LoadError> {
//! let mut scene = Scene::load("examples/mygame/scenes/level1.json")?;
//! let duck = scene.find("duck").expect("the scene has a duck");
//! scene[duck].at.x += 2.0;
//! for coin in scene.touching(duck, "coin") {
//!     scene.remove(coin);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! In the game's `update`, call [`scene.update(ctx)`](Scene::update); in
//! `draw`, [`scene.draw(canvas)`](Scene::draw); in `reload`,
//! [`scene.reload(ctx, file)`](Scene::reload), so a change to the file
//! (made in Duck Forge Studio, or by hand while the game runs) reaches the
//! game without starting it again.
//!
//! What every object has is in [`Object`]. Three kinds mean something to
//! the engine already: `picture` (the usual one: it's drawn, and does
//! nothing), `tilemap` (a level painted on a grid: `"level": "level.txt"`,
//! read with [`TileMap::load_level`]) and `background` (a picture that
//! repeats and moves slower than the world: `"parallax": 0.2`). Any other
//! kind is a name the game's code can ask for
//! ([`scene.of_kind("coin")`](Scene::of_kind)); ready-made kinds that act
//! by themselves come later in Phase 5.
//!
//! **Order.** Objects are *drawn* in the file's order (inside their
//! layer), and *listed* by [`ids`](Scene::ids) in the order of their ids.
//! So moving an object up or down the file changes only what's drawn on
//! top, never how the game plays.
//!
//! **Starts and now.** The scene remembers how the file has each object
//! ([`starts_as`](Scene::starts_as)) beside how it is now. When the file
//! changes while the game runs, each changed key is taken by the object
//! unless the game has changed that key itself since the start: a coin
//! moved in the file moves; the duck, who has walked away from where it
//! starts, stays where it is.

mod file;
mod object;

use std::collections::{BTreeMap, BTreeSet};
use std::ops::{Index, IndexMut};
use std::path::{Path, PathBuf};

pub use file::{Layer, Problem, SCENE_FILE_VERSION, SceneFile, Space, write_ids};
pub use object::{Anchor, Id, Object};

use crate::background::Background;
use crate::camera::Camera2D;
use crate::canvas::{Canvas, Placed};
use crate::color::Color;
use crate::engine::Context;
use crate::error::LoadError;
use crate::math::{Rect, Vec2, vec2};
use crate::sprite::Sprite;
use crate::spritesheet::SpriteSheet;
use crate::tilemap::{Contacts, LetterKind, TileMap};
use crate::tileset::Tileset;
use object::{FIRST_MADE_ID, USUAL_LAYER};

/// Where a scene's camera is until the first update, when the screen's
/// size is known: if the game hasn't moved it by then, it goes to the
/// screen's middle, so the world is where the screen is.
const CAMERA_NOT_MOVED: Vec2 = vec2(160.0, 120.0);

/// What a picture's file turned out to be.
#[derive(Debug, Clone)]
enum Picture {
    Still(Sprite),
    Sheet(SpriteSheet),
    /// It couldn't be loaded: why, in one sentence.
    Missing(String),
}

/// A tile map object's level, loaded.
#[derive(Debug, Clone)]
struct MapPart {
    /// The level's path as the object has it (from the game's folder).
    level: String,
    map: TileMap,
    tiles: Option<Tileset>,
    /// The tiles' sheet, from the game's folder, for painted things'
    /// pictures.
    tiles_picture: Option<String>,
    /// What went wrong loading the tiles' pictures, if anything.
    wrong: Option<String>,
}

/// A scene being played: its objects as they are now, and as the file has
/// them. See the [module docs](self).
#[derive(Debug, Clone)]
pub struct Scene {
    /// The scene file, as the engine names files (empty: made from text).
    file: String,
    /// The game's folder: pictures and levels are named from here.
    folder: PathBuf,
    /// The file as last read: how everything starts.
    starts: SceneFile,
    /// Things painted on tile maps, as their levels have them.
    painted_starts: BTreeMap<Id, Object>,
    /// The objects now, in drawing order.
    objects: Vec<Object>,
    maps: BTreeMap<Id, MapPart>,
    pictures: BTreeMap<String, Picture>,
    /// The colour behind everything, if any.
    pub background: Option<Color>,
    /// The camera the world is drawn through. Move it as any
    /// [`Camera2D`]: `scene.camera.follow(target, 0.15, ctx.dt())`.
    pub camera: Camera2D,
    camera_placed: bool,
    /// Seconds played, for animations.
    time: f32,
    /// How many objects were made while playing (for their ids).
    made: u64,
    /// What's wrong, in plain words.
    problems: Vec<String>,
    /// The scene file's last text couldn't be read at all: why.
    unreadable: Option<String>,
    /// The problems already printed, so each is printed once.
    printed: BTreeSet<String>,
}

impl Scene {
    /// Loads a scene file, with its pictures and tile maps.
    ///
    /// Names inside the file (`"picture": "assets/duck.png"`) are from the
    /// game's folder: the folder that holds the file, or the one above it
    /// if the file is in a folder called `scenes`.
    ///
    /// `Err` only if the file is missing or can't be read as a scene at
    /// all. Smaller mistakes (a missing picture, a key with the wrong kind
    /// of value) are printed once, kept in [`problems`](Scene::problems),
    /// and drawn round: a missing picture is a marked box, so one typo
    /// never loses the scene.
    pub fn load(path: impl AsRef<Path>) -> Result<Scene, LoadError> {
        let path = path.as_ref();
        let text = crate::files::read_text(path)
            .map_err(|error| crate::sprite::missing_file_error(path, "scene", &error))?;
        let starts =
            SceneFile::parse(&text).map_err(|problem| file::cannot_read(path, &problem))?;
        let mut scene = Scene::empty(crate::files::name_of(path), game_folder_of(path));
        scene.start_from(starts);
        Ok(scene)
    }

    /// A scene from its text, with `folder` as the game's folder. (For
    /// tests and tools: nothing is noted as a file the game reads.)
    pub fn from_text(text: &str, folder: impl AsRef<Path>) -> Result<Scene, LoadError> {
        let starts = SceneFile::parse(text)
            .map_err(|problem| file::cannot_read(Path::new("the scene's text"), &problem))?;
        let mut scene = Scene::empty(String::new(), folder.as_ref().to_path_buf());
        scene.start_from(starts);
        Ok(scene)
    }

    fn empty(file: String, folder: PathBuf) -> Scene {
        Scene {
            file,
            folder,
            starts: SceneFile::parse("{\"duckforge_scene\": 1, \"objects\": []}")
                .expect("an empty scene reads"),
            painted_starts: BTreeMap::new(),
            objects: Vec::new(),
            maps: BTreeMap::new(),
            pictures: BTreeMap::new(),
            background: None,
            camera: Camera2D::new(CAMERA_NOT_MOVED.x * 2.0, CAMERA_NOT_MOVED.y * 2.0),
            camera_placed: false,
            time: 0.0,
            made: 0,
            problems: Vec::new(),
            unreadable: None,
            printed: BTreeSet::new(),
        }
    }

    /// Everything as the file has it: the first load.
    fn start_from(&mut self, starts: SceneFile) {
        self.background = starts.background;
        self.objects = starts.objects.clone();
        self.starts = starts;
        self.load_maps();
        self.arrange();
        self.load_new_pictures();
        self.say_problems();
    }

    // ---- Asking about the scene ----

    /// The scene file, as the engine names files:
    /// `examples/mygame/scenes/level1.json`.
    pub fn file(&self) -> &str {
        &self.file
    }

    /// The game's folder, which the file's names are from.
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// Does the scene read `file`: its own file, a tile map's level or
    /// legend, or one of its pictures?
    pub fn reads_from(&self, file: &Path) -> bool {
        let name = crate::files::name_of(file);
        (!self.file.is_empty() && name == self.file)
            || self.maps.values().any(|part| part.map.reads_from(file))
            || self.is_a_picture_of_it(file)
    }

    /// What's wrong with the scene, each in one plain sentence with its
    /// file and line. Empty if nothing is.
    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    /// The file as it was last read: how every object starts, the layers,
    /// the rules.
    pub fn starts(&self) -> &SceneFile {
        &self.starts
    }

    /// The scene's layers, in drawing order.
    pub fn layers(&self) -> &[Layer] {
        &self.starts.layers
    }

    /// The scene's size in pixels: what its file says, or else its first
    /// tile map's.
    pub fn size(&self) -> Option<Vec2> {
        self.starts.size.or_else(|| {
            self.tilemap()
                .map(|map| vec2(map.bounds().w, map.bounds().h))
        })
    }

    /// The scene that comes after this one, if the file names one.
    pub fn next(&self) -> Option<&str> {
        self.starts.next.as_deref()
    }

    /// Seconds played: what animations are drawn at.
    pub fn time(&self) -> f32 {
        self.time
    }

    // ---- Finding objects ----

    /// The first object called `name` (the one with the lowest id).
    pub fn find(&self, name: &str) -> Option<Id> {
        self.objects
            .iter()
            .filter(|object| object.name == name)
            .map(|object| object.id)
            .min()
    }

    /// Every object called `name`, in the order of their ids.
    pub fn all(&self, name: &str) -> Vec<Id> {
        self.ids_where(|object| object.name == name)
    }

    /// Every object of a kind, in the order of their ids.
    pub fn of_kind(&self, kind: &str) -> Vec<Id> {
        self.ids_where(|object| object.kind == kind)
    }

    /// Every object, in the order of their ids: the file's objects, then
    /// those made while playing, then things painted on tile maps. This
    /// order doesn't change when objects are moved up or down the file.
    pub fn ids(&self) -> Vec<Id> {
        self.ids_where(|_| true)
    }

    /// Every object, in the order they're drawn: layer by layer, and
    /// inside a layer in the file's order.
    pub fn ids_as_drawn(&self) -> Vec<Id> {
        let mut drawn: Vec<(usize, usize)> = self
            .objects
            .iter()
            .enumerate()
            .map(|(index, object)| (self.layer_of(object), index))
            .collect();
        drawn.sort();
        drawn
            .into_iter()
            .map(|(_, index)| self.objects[index].id)
            .collect()
    }

    fn ids_where(&self, wanted: impl Fn(&Object) -> bool) -> Vec<Id> {
        let mut ids: Vec<Id> = self
            .objects
            .iter()
            .filter(|object| wanted(object))
            .map(|object| object.id)
            .collect();
        ids.sort();
        ids
    }

    /// The object with this id, if it's (still) in the scene.
    pub fn get(&self, id: Id) -> Option<&Object> {
        self.objects.iter().find(|object| object.id == id)
    }

    /// The same, to change it.
    pub fn get_mut(&mut self, id: Id) -> Option<&mut Object> {
        self.objects.iter_mut().find(|object| object.id == id)
    }

    /// Is this object (still) in the scene?
    pub fn has(&self, id: Id) -> bool {
        self.get(id).is_some()
    }

    /// How many objects the scene has now.
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Has the scene no objects?
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// How the file has this object: where it starts, and with what. For
    /// a thing painted on a tile map, how its level has it. `None` for an
    /// object made while playing.
    pub fn starts_as(&self, id: Id) -> Option<&Object> {
        self.starts
            .object(id)
            .or_else(|| self.painted_starts.get(&id))
    }

    // ---- Adding and removing ----

    /// Adds an object made by the code (a bullet, a spark) and gives its
    /// id. It isn't in the file, and it's drawn after the file's objects
    /// on its layer.
    pub fn add(&mut self, mut object: Object) -> Id {
        self.made += 1;
        object.id = Id(FIRST_MADE_ID + self.made);
        let id = object.id;
        self.objects.push(object);
        self.load_new_pictures();
        id
    }

    /// Takes an object out of the scene, and gives it back. `None` if it
    /// was already gone.
    pub fn remove(&mut self, id: Id) -> Option<Object> {
        let index = self.objects.iter().position(|object| object.id == id)?;
        self.maps.remove(&id);
        Some(self.objects.remove(index))
    }

    // ---- Where things are ----

    /// The object's collision box in the scene: its `box` if it has one,
    /// or else its whole picture. Its `turn` doesn't turn the box.
    #[track_caller]
    pub fn rect(&self, id: Id) -> Rect {
        self.rect_of(&self[id])
    }

    /// The rectangle the object's picture covers in the scene (before it
    /// is turned).
    #[track_caller]
    pub fn picture_rect(&self, id: Id) -> Rect {
        self.picture_rect_of(&self[id])
    }

    /// Every other object called `what` (a name or a kind) whose box
    /// overlaps this one's, in the order of their ids. Hidden objects
    /// count; objects with no size don't.
    #[track_caller]
    pub fn touching(&self, id: Id, what: &str) -> Vec<Id> {
        let mine = self.rect(id);
        if mine.w <= 0.0 || mine.h <= 0.0 {
            return Vec::new();
        }
        self.ids_where(|other| {
            other.id != id && (other.name == what || other.kind == what) && {
                let theirs = self.rect_of(other);
                theirs.w > 0.0 && theirs.h > 0.0 && theirs.overlaps(&mine)
            }
        })
    }

    /// Do these two objects' boxes overlap?
    #[track_caller]
    pub fn overlaps(&self, a: Id, b: Id) -> bool {
        let (a, b) = (self.rect(a), self.rect(b));
        a.w > 0.0 && a.h > 0.0 && b.w > 0.0 && b.h > 0.0 && a.overlaps(&b)
    }

    /// The objects whose picture or box is under a point of the scene, the
    /// one drawn on top first. (Tile maps and backgrounds aren't counted.)
    pub fn at_point(&self, point: Vec2) -> Vec<Id> {
        let mut under: Vec<Id> = self
            .ids_as_drawn()
            .into_iter()
            .filter(|&id| {
                let object = &self[id];
                !matches!(object.kind.as_str(), "tilemap" | "background")
                    && (self.picture_rect_of(object).contains(point)
                        || self.rect_of(object).contains(point))
            })
            .collect();
        under.reverse();
        under
    }

    /// Moves an object by `movement`, stopping at the scene's tile map
    /// (the first one in the file) and at solid objects, and says what it
    /// bumped into: the same [`Contacts`] as
    /// [`TileMap::move_and_slide`].
    ///
    /// ```ignore
    /// let touched = scene.move_and_slide(duck, self.velocity * ctx.dt());
    /// if touched.ground { self.velocity.y = 0.0; }
    /// ```
    #[track_caller]
    pub fn move_and_slide(&mut self, id: Id, movement: Vec2) -> Contacts {
        let start = self.rect(id);
        let solids: Vec<Rect> = self
            .objects
            .iter()
            .filter(|other| other.solid && other.id != id)
            .map(|other| self.rect_of(other))
            .filter(|rect| rect.w > 0.0 && rect.h > 0.0)
            .collect();
        let map = self.maps.values().next().map(|part| &part.map);
        let mut body = start;
        let mut touched = Contacts::default();
        // In steps short enough that a thin solid object isn't stepped over.
        let longest = movement.x.abs().max(movement.y.abs());
        let steps = if solids.is_empty() || !longest.is_finite() {
            1
        } else {
            ((longest / 4.0).ceil() as usize).clamp(1, 256)
        };
        let step = movement * (1.0 / steps as f32);
        for _ in 0..steps {
            for sideways in [true, false] {
                let part = if sideways {
                    vec2(step.x, 0.0)
                } else {
                    vec2(0.0, step.y)
                };
                match map {
                    Some(map) => {
                        let now = map.move_and_slide(&mut body, part);
                        touched.ground |= now.ground;
                        touched.ceiling |= now.ceiling;
                        touched.wall_left |= now.wall_left;
                        touched.wall_right |= now.wall_right;
                    }
                    None => body = Rect::new(body.x + part.x, body.y + part.y, body.w, body.h),
                }
                for solid in &solids {
                    if !body.overlaps(solid) {
                        continue;
                    }
                    if sideways && part.x > 0.0 {
                        body.x = solid.left() - body.w;
                        touched.wall_right = true;
                    } else if sideways && part.x < 0.0 {
                        body.x = solid.right();
                        touched.wall_left = true;
                    } else if !sideways && part.y > 0.0 {
                        body.y = solid.top() - body.h;
                        touched.ground = true;
                    } else if !sideways && part.y < 0.0 {
                        body.y = solid.bottom();
                        touched.ceiling = true;
                    }
                }
            }
        }
        let moved = vec2(body.x - start.x, body.y - start.y);
        self[id].at += moved;
        touched
    }

    /// The scene's tile map: the first one in the file. Everything a
    /// [`TileMap`] can do works on it.
    pub fn tilemap(&self) -> Option<&TileMap> {
        self.maps.values().next().map(|part| &part.map)
    }

    /// The tile map of one `tilemap` object.
    pub fn tilemap_of(&self, id: Id) -> Option<&TileMap> {
        self.maps.get(&id).map(|part| &part.map)
    }

    // ---- Every frame ----

    /// Moves the scene on by one update: its animations' clock, and the
    /// camera's shake. Call it once in the game's `update`.
    pub fn update(&mut self, ctx: &mut Context) {
        self.time += ctx.dt();
        self.camera.screen_size = vec2(ctx.width(), ctx.height());
        if !self.camera_placed {
            // Until the game moves it, the world is where the screen is.
            if self.camera.position == CAMERA_NOT_MOVED {
                self.camera.position = self.camera.screen_size * 0.5;
            }
            self.camera_placed = true;
        }
        self.camera.update(ctx);
        // A picture the code named since the last update.
        if self.load_new_pictures() {
            self.say_problems();
        }
    }

    /// Draws the whole scene: its background colour, then every layer in
    /// order. Call it in the game's `draw`, then draw what's the game's
    /// own (a score, say) on top.
    pub fn draw(&self, canvas: &mut Canvas) {
        if let Some(colour) = self.background {
            canvas.clear(colour);
        }
        for layer in 0..self.starts.layers.len() {
            self.draw_layer_at(canvas, layer);
        }
    }

    /// Draws one layer, to draw the game's own things between layers:
    ///
    /// ```ignore
    /// scene.draw_layer(canvas, "back");
    /// self.draw_rain(canvas);
    /// scene.draw_layer(canvas, "world");
    /// ```
    ///
    /// Panics, naming the layers the scene has, if there's no such layer.
    #[track_caller]
    pub fn draw_layer(&self, canvas: &mut Canvas, name: &str) {
        let Some(layer) = self
            .starts
            .layers
            .iter()
            .position(|layer| layer.name == name)
        else {
            let names: Vec<&str> = self
                .starts
                .layers
                .iter()
                .map(|layer| layer.name.as_str())
                .collect();
            panic!(
                "the scene has no layer {name:?}; it has: {}",
                names.join(", ")
            );
        };
        self.draw_layer_at(canvas, layer);
    }

    fn draw_layer_at(&self, canvas: &mut Canvas, layer: usize) {
        let stays = self.starts.layers[layer].stays_on_screen;
        // The camera, sized to the canvas it's drawn on.
        let mut camera = self.camera.clone();
        camera.screen_size = vec2(canvas.width() as f32, canvas.height() as f32);
        if !self.camera_placed && camera.position == CAMERA_NOT_MOVED {
            camera.position = camera.screen_size * 0.5;
        }
        let mut in_camera = false;
        let mut through_camera = |canvas: &mut Canvas, wanted: bool| {
            if wanted && !in_camera {
                canvas.push_camera(&camera);
            } else if !wanted && in_camera {
                canvas.pop_camera();
            }
            in_camera = wanted;
        };
        for object in &self.objects {
            if !object.shown || self.layer_of(object) != layer {
                continue;
            }
            match object.kind.as_str() {
                "background" => {
                    through_camera(canvas, false);
                    self.draw_background(canvas, object, &camera);
                }
                "tilemap" => {
                    through_camera(canvas, !stays);
                    if let Some(part) = self.maps.get(&object.id) {
                        match &part.tiles {
                            Some(tiles) => part.map.draw(canvas, tiles, self.time),
                            None => part.map.mark_drawn(canvas),
                        }
                    }
                }
                _ => {
                    through_camera(canvas, !stays);
                    self.draw_object(canvas, object);
                }
            }
        }
        through_camera(canvas, false);
    }

    fn draw_background(&self, canvas: &mut Canvas, object: &Object, camera: &Camera2D) {
        let Some(sprite) = self.frame_of(object) else {
            return;
        };
        let parallax = object
            .point("parallax")
            .or_else(|| object.number("parallax").map(|p| vec2(p, p)))
            .unwrap_or(Vec2::ZERO);
        let background = Background::new(sprite)
            .parallax_xy(parallax.x, parallax.y)
            .offset(object.at)
            .repeat_x(object.flag("repeat_x").unwrap_or(true))
            .repeat_y(object.flag("repeat_y").unwrap_or(false));
        canvas.draw_background(&background, camera);
    }

    fn draw_object(&self, canvas: &mut Canvas, object: &Object) {
        let area = self.picture_rect_of(object);
        match self.frame_of(object) {
            Some(sprite) => {
                let pivot = self.pivot_of(object, sprite.width() as f32, sprite.height() as f32);
                canvas.draw_placed(
                    &sprite,
                    object.at,
                    Placed {
                        pivot,
                        scale: object.scale,
                        turn: object.turn,
                        flip_x: object.flip,
                        flip_y: false,
                        tint: object.tint,
                        opacity: object.opacity,
                    },
                );
            }
            None if object.picture.is_some() => {
                // A picture that couldn't be loaded: a marked box, so the
                // object is still there to see and to fix.
                let marked = Color::rgb(255, 0, 255);
                canvas.draw_rect(area, marked);
                canvas.draw_line(area.top_left(), area.bottom_right(), marked);
                canvas.draw_line(area.top_right(), area.bottom_left(), marked);
            }
            None => {}
        }
        let named = if area.w > 0.0 && area.h > 0.0 {
            area
        } else {
            self.rect_of(object)
        };
        if named.w > 0.0 && named.h > 0.0 {
            canvas.label(&object.name, named);
        }
    }

    // ---- When a file changes ----

    /// A file changed while the game runs: if it's one of the scene's (its
    /// own file, a tile map's level or legend, a picture), the scene takes
    /// the change and returns `true`. Call it from the game's
    /// [`reload`](crate::engine::Game::reload):
    ///
    /// ```ignore
    /// fn reload(&mut self, ctx: &mut Context, file: &Path) -> bool {
    ///     self.scene.reload(ctx, file)
    /// }
    /// ```
    ///
    /// The game carries on from where it is. Each key that changed in the
    /// file is taken by its object, unless the game has changed that key
    /// itself since the scene started (the duck has walked on; its score
    /// went up): then the object keeps what it has now, and
    /// [`starts_as`](Scene::starts_as) has the file's new value. New
    /// objects appear, objects taken out of the file go, and an object the
    /// game removed (a coin that was taken) stays gone.
    ///
    /// A file with a mistake in it changes nothing: the scene stays as it
    /// was, and [`problems`](Scene::problems) says what's wrong.
    pub fn reload(&mut self, _ctx: &mut Context, file: &Path) -> bool {
        self.reload_file(file)
    }

    /// [`reload`](Scene::reload), for code with no `ctx` (tests, tools).
    pub fn reload_file(&mut self, file: &Path) -> bool {
        let name = crate::files::name_of(file);
        if !self.file.is_empty() && name == self.file {
            match crate::files::read_text(file) {
                Ok(text) => self.take_text(&text),
                Err(error) => self.note_unreadable(format!("{name}: couldn't be read ({error})")),
            }
            return true;
        }
        let of_map: Vec<Id> = self
            .maps
            .iter()
            .filter(|(_, part)| part.map.reads_from(file))
            .map(|(id, _)| *id)
            .collect();
        if !of_map.is_empty() {
            for id in of_map {
                self.maps.remove(&id);
            }
            self.load_maps();
            self.arrange();
            self.load_new_pictures();
            self.say_problems();
            return true;
        }
        if self.is_a_picture_of_it(file) {
            // Pictures are read from disk again: all of them, which is
            // quick, and right whichever of a sheet's two files changed.
            self.pictures.clear();
            let maps: Vec<Id> = self.maps.keys().copied().collect();
            for id in maps {
                if let Some(part) = self.maps.get_mut(&id) {
                    let (tiles, tiles_picture, wrong) = tiles_of(&part.map, &self.folder);
                    part.tiles = tiles;
                    part.tiles_picture = tiles_picture;
                    part.wrong = wrong;
                }
            }
            self.load_new_pictures();
            self.say_problems();
            return true;
        }
        false
    }

    /// Takes the scene file's new text, as [`reload`](Scene::reload) does
    /// once it has read it. (Tools that hold the text use this.)
    pub fn take_text(&mut self, text: &str) {
        let newer = match SceneFile::parse(text) {
            Ok(newer) => newer,
            Err(problem) => {
                let file = if self.file.is_empty() {
                    "the scene's text"
                } else {
                    &self.file
                };
                return self.note_unreadable(format!(
                    "{} (the scene is kept as it was)",
                    problem.in_file(file)
                ));
            }
        };
        self.unreadable = None;
        if self.background == self.starts.background {
            self.background = newer.background;
        }
        let was: BTreeMap<Id, &Object> = self
            .starts
            .objects
            .iter()
            .map(|object| (object.id, object))
            .collect();
        let mut now: Vec<Object> = Vec::new();
        for start in &newer.objects {
            match was.get(&start.id) {
                // New in the file: it appears.
                None => now.push(start.clone()),
                // Changed or not: the object as it is now takes what
                // changed. If the game removed it, it stays gone.
                Some(before) => {
                    if let Some(live) = self.get(start.id) {
                        now.push(merged(before, start, live));
                    }
                }
            }
        }
        // Objects not from the file stay; `arrange` puts them in place.
        now.extend(
            self.objects
                .iter()
                .filter(|object| !object.id.is_from_the_file())
                .cloned(),
        );
        self.objects = now;
        self.starts = newer;
        self.load_maps();
        self.arrange();
        self.load_new_pictures();
        self.say_problems();
    }

    fn note_unreadable(&mut self, text: String) {
        self.unreadable = Some(text);
        self.say_problems();
    }

    // ---- Inside ----

    /// The index of the layer an object is drawn on: its own, or "world"
    /// if the scene hasn't got that one, or the last.
    fn layer_of(&self, object: &Object) -> usize {
        let layers = &self.starts.layers;
        let named = |name: &str| layers.iter().position(|layer| layer.name == name);
        named(&object.layer)
            .or_else(|| named(USUAL_LAYER))
            .unwrap_or(layers.len().saturating_sub(1))
    }

    /// The picture an object shows right now: its still picture, or its
    /// animation's frame at the scene's time.
    fn frame_of(&self, object: &Object) -> Option<Sprite> {
        match self.pictures.get(object.picture.as_deref()?)? {
            Picture::Still(sprite) => Some(sprite.clone()),
            Picture::Sheet(sheet) => {
                let animation = object
                    .animation
                    .as_deref()
                    .and_then(|name| sheet.get(name))
                    .or_else(|| sheet.get(usual_animation(sheet)?))?;
                Some(animation.frame_at(self.time).clone())
            }
            Picture::Missing(_) => None,
        }
    }

    /// The size of an object's picture in its own pixels: a frame of its
    /// sheet, or the whole still picture. With no picture, its box's size.
    fn picture_size_of(&self, object: &Object) -> Vec2 {
        let picture = object
            .picture
            .as_deref()
            .and_then(|picture| self.pictures.get(picture));
        match picture {
            Some(Picture::Still(sprite)) => vec2(sprite.width() as f32, sprite.height() as f32),
            Some(Picture::Sheet(sheet)) => {
                let (w, h) = sheet.frame_size();
                vec2(w as f32, h as f32)
            }
            // A picture that's missing still has a place: one tile's worth.
            Some(Picture::Missing(_)) => vec2(16.0, 16.0),
            None => object.hitbox.map_or(Vec2::ZERO, |b| vec2(b.w, b.h)),
        }
    }

    /// The point of the picture that sits on the object's `at`.
    fn pivot_of(&self, object: &Object, width: f32, height: f32) -> Vec2 {
        if let Some(anchor) = object.anchor {
            return anchor.point(width, height);
        }
        let sheet = object
            .picture
            .as_deref()
            .and_then(|picture| self.pictures.get(picture));
        match sheet {
            Some(Picture::Sheet(sheet)) => sheet.pivot(),
            _ => Anchor::of_kind(&object.kind).point(width, height),
        }
    }

    fn picture_rect_of(&self, object: &Object) -> Rect {
        let size = self.picture_size_of(object);
        let mut pivot = self.pivot_of(object, size.x, size.y);
        if object.flip {
            pivot.x = size.x - pivot.x;
        }
        Rect::new(
            object.at.x - pivot.x * object.scale.x,
            object.at.y - pivot.y * object.scale.y,
            size.x * object.scale.x,
            size.y * object.scale.y,
        )
    }

    fn rect_of(&self, object: &Object) -> Rect {
        let whole = self.picture_rect_of(object);
        let Some(hitbox) = object.hitbox else {
            return whole;
        };
        if object.picture.is_none() {
            return whole; // with no picture, the box is the whole of it
        }
        let size = self.picture_size_of(object);
        let left = if object.flip {
            size.x - hitbox.x - hitbox.w
        } else {
            hitbox.x
        };
        Rect::new(
            whole.x + left * object.scale.x,
            whole.y + hitbox.y * object.scale.y,
            hitbox.w * object.scale.x,
            hitbox.h * object.scale.y,
        )
    }

    /// Is `file` one of the scene's pictures (or the other file of one of
    /// its sheets: `hero.json` and `hero.png` go together)?
    fn is_a_picture_of_it(&self, file: &Path) -> bool {
        let stem = crate::files::name_of(&file.with_extension(""));
        let same = |picture: &str| {
            crate::files::name_of(&self.folder.join(picture).with_extension("")) == stem
        };
        self.pictures.keys().any(|picture| same(picture))
            || self
                .maps
                .values()
                .any(|part| part.tiles_picture.as_deref().is_some_and(same))
    }

    /// Loads the pictures objects name that aren't loaded yet. Says
    /// whether there were any.
    fn load_new_pictures(&mut self) -> bool {
        let all_loaded = self
            .objects
            .iter()
            .filter_map(|object| object.picture.as_ref())
            .all(|picture| self.pictures.contains_key(picture));
        if all_loaded {
            return false; // the usual case: nothing to do, nothing made
        }
        let wanted: Vec<String> = self
            .objects
            .iter()
            .filter_map(|object| object.picture.as_ref())
            .filter(|picture| !self.pictures.contains_key(*picture))
            .cloned()
            .collect();
        let any = !wanted.is_empty();
        for picture in wanted {
            if self.pictures.contains_key(&picture) {
                continue;
            }
            let path = self.folder.join(&picture);
            let loaded = if picture.ends_with(".json") {
                SpriteSheet::load(&path).map(Picture::Sheet)
            } else {
                Sprite::load_png(&path).map(Picture::Still)
            };
            let loaded = loaded.unwrap_or_else(|error| Picture::Missing(error.to_string()));
            self.pictures.insert(picture, loaded);
        }
        any
    }

    /// Loads the level of every `tilemap` object that hasn't one loaded,
    /// forgets the maps of objects that are gone, and brings the things
    /// painted on them up to date.
    fn load_maps(&mut self) {
        let wanted: Vec<(Id, String)> = self
            .objects
            .iter()
            .filter(|object| object.kind == "tilemap")
            .map(|object| {
                (
                    object.id,
                    object.text("level").unwrap_or_default().to_string(),
                )
            })
            .collect();
        self.maps.retain(|id, part| {
            wanted
                .iter()
                .any(|(i, level)| i == id && *level == part.level)
        });
        for (id, level) in &wanted {
            if self.maps.contains_key(id) || level.is_empty() {
                continue;
            }
            if let Ok(map) = TileMap::load_level(self.folder.join(level)) {
                let (tiles, tiles_picture, wrong) = tiles_of(&map, &self.folder);
                self.maps.insert(
                    *id,
                    MapPart {
                        level: level.clone(),
                        map,
                        tiles,
                        tiles_picture,
                        wrong,
                    },
                );
            }
        }
        self.sync_painted();
    }

    /// Makes the things painted on the tile maps match their levels: new
    /// ones appear, ones painted out go, and ones the game removed stay
    /// gone.
    fn sync_painted(&mut self) {
        let mut painted: BTreeMap<Id, Object> = BTreeMap::new();
        for (map_id, part) in &self.maps {
            let layer = self
                .get(*map_id)
                .map_or(USUAL_LAYER.to_string(), |object| object.layer.clone());
            let tile = part.map.tile_size();
            for letter in part.map.legend().letters() {
                if letter.kind != LetterKind::Thing {
                    continue;
                }
                let has_picture = part
                    .tiles
                    .as_ref()
                    .is_some_and(|tiles| tiles.get(&letter.name).is_some());
                for at in part.map.spawns(&letter.name) {
                    let (column, row) = part.map.cell_at(vec2(at.x, at.y - tile / 2.0));
                    let mut thing = Object::new(&letter.name).at(at).on_layer(&layer);
                    thing.id = Id::painted(*map_id, column.max(0) as usize, row.max(0) as usize);
                    if has_picture {
                        thing.picture = part.tiles_picture.clone();
                        thing.animation = Some(letter.name.clone());
                    } else {
                        // No picture: a tile's worth of box, so it touches.
                        thing.hitbox = Some(Rect::new(0.0, 0.0, tile, tile));
                    }
                    painted.insert(thing.id, thing);
                }
            }
        }
        let mut now: Vec<Object> = Vec::new();
        for object in &self.objects {
            if !object.id.is_painted() {
                now.push(object.clone());
            } else if let (Some(before), Some(start)) =
                (self.painted_starts.get(&object.id), painted.get(&object.id))
            {
                now.push(merged(before, start, object));
            }
        }
        for (id, start) in &painted {
            if !self.painted_starts.contains_key(id) {
                now.push(start.clone());
            }
        }
        self.objects = now;
        self.painted_starts = painted;
    }

    /// Puts the objects in drawing order: the file's objects in the file's
    /// order, each tile map followed by the things painted on it, then the
    /// objects made while playing, in the order they were made.
    fn arrange(&mut self) {
        let place: BTreeMap<Id, usize> = self
            .starts
            .objects
            .iter()
            .enumerate()
            .map(|(index, object)| (object.id, index))
            .collect();
        let last = place.len();
        let key = |object: &Object| -> (usize, u8, u64) {
            if let Some(index) = place.get(&object.id) {
                (*index, 0, 0)
            } else if object.id.is_painted() {
                // After its tile map: the map's id is inside its own.
                let map = Id((object.id.0 >> 32) & 0xffff);
                (place.get(&map).copied().unwrap_or(last), 1, object.id.0)
            } else {
                (last, 2, object.id.0)
            }
        };
        self.objects.sort_by_key(key);
    }

    /// Works out what's wrong with the scene now, and prints what's new.
    fn say_problems(&mut self) {
        let file = if self.file.is_empty() {
            String::from("the scene")
        } else {
            self.file.clone()
        };
        let mut problems: Vec<String> = self
            .starts
            .problems
            .iter()
            .map(|problem| problem.in_file(&file))
            .collect();
        for object in &self.objects {
            let called = &object.name;
            if let Some(picture) = &object.picture {
                match self.pictures.get(picture) {
                    Some(Picture::Missing(why)) => problems.push(format!(
                        "{file}: {called:?} has the picture {picture:?}, which couldn't be \
                         loaded, so it's drawn as a marked box ({why})"
                    )),
                    Some(Picture::Sheet(sheet)) => {
                        let unknown = object
                            .animation
                            .as_ref()
                            .filter(|animation| sheet.get(animation).is_none());
                        if let Some(animation) = unknown {
                            let names: Vec<&str> = sheet.names().collect();
                            problems.push(format!(
                                "{file}: {called:?} asks for the animation {animation:?}, which \
                                 {picture} doesn't have (it has {}); it shows {:?}",
                                names.join(", "),
                                usual_animation(sheet).unwrap_or_default()
                            ));
                        }
                    }
                    _ => {}
                }
            }
            if object.kind == "tilemap" {
                match (object.text("level"), self.maps.get(&object.id)) {
                    (None, _) => problems.push(format!(
                        "{file}: the tile map {called:?} needs \"level\": the file with its \
                         letters, like \"level.txt\""
                    )),
                    (Some(level), None) => {
                        let why = TileMap::load_level(self.folder.join(level))
                            .err()
                            .map_or(String::new(), |error| format!(" ({error})"));
                        problems.push(format!(
                            "{file}: the tile map {called:?} couldn't read its level \
                             {level:?}{why}"
                        ));
                    }
                    (Some(_), Some(part)) => {
                        if let Some(wrong) = &part.wrong {
                            problems.push(format!("{file}: the tile map {called:?}: {wrong}"));
                        }
                        if object.at != Vec2::ZERO {
                            problems.push(format!(
                                "{file}: the tile map {called:?} has \"at\", but tile maps \
                                 can't be moved yet: it's drawn from the scene's corner"
                            ));
                        }
                    }
                }
            }
        }
        // A file that couldn't be read at all stays said until one can.
        problems.extend(self.unreadable.clone());
        self.problems = problems;
        self.print_new_problems();
    }

    fn print_new_problems(&mut self) {
        for problem in &self.problems {
            if self.printed.insert(problem.clone()) {
                eprintln!("duckforge: {problem}");
            }
        }
        // A problem that's gone can be said again if it comes back.
        let now: BTreeSet<&String> = self.problems.iter().collect();
        self.printed.retain(|problem| now.contains(problem));
    }
}

impl Index<Id> for Scene {
    type Output = Object;

    /// The object with this id. Panics, saying so, if it isn't in the
    /// scene (any more): ask first with [`Scene::get`] if it may be gone.
    #[track_caller]
    fn index(&self, id: Id) -> &Object {
        self.get(id).unwrap_or_else(|| {
            panic!(
                "the scene has no object with the id {id}: it was removed, or was never \
                 there. `scene.get(id)` asks without panicking"
            )
        })
    }
}

impl IndexMut<Id> for Scene {
    #[track_caller]
    fn index_mut(&mut self, id: Id) -> &mut Object {
        self.get_mut(id).unwrap_or_else(|| {
            panic!(
                "the scene has no object with the id {id}: it was removed, or was never \
                 there. `scene.get_mut(id)` asks without panicking"
            )
        })
    }
}

/// The animation an object shows when it names none (or one its sheet
/// hasn't got): `idle` if the sheet has it, or else its first.
fn usual_animation(sheet: &SpriteSheet) -> Option<&str> {
    if sheet.get("idle").is_some() {
        Some("idle")
    } else {
        sheet.names().next()
    }
}

/// The game's folder for a scene file: the folder it's in, or the one
/// above if that folder is called `scenes`.
fn game_folder_of(scene_file: &Path) -> PathBuf {
    let own = scene_file.parent().unwrap_or(Path::new(""));
    if own.file_name().is_some_and(|name| name == "scenes") {
        own.parent().unwrap_or(Path::new("")).to_path_buf()
    } else {
        own.to_path_buf()
    }
}

/// A tile map's tiles: the tileset its legend names, the sheet's path from
/// the game's folder (for painted things' pictures), and what went wrong.
fn tiles_of(map: &TileMap, folder: &Path) -> (Option<Tileset>, Option<String>, Option<String>) {
    let Some(file) = map.legend().tiles_file() else {
        return (
            None,
            None,
            Some(String::from(
                "its legend doesn't name the tiles' pictures (\"tiles\": \"assets/tiles.json\"), \
                 so its tiles aren't drawn",
            )),
        );
    };
    match SpriteSheet::load(file) {
        Ok(sheet) => {
            let from_folder = file.strip_prefix(folder).ok().map(crate::files::name_of);
            (Some(Tileset::from_sheet(&sheet)), from_folder, None)
        }
        Err(error) => (
            None,
            None,
            Some(format!("its tiles' pictures couldn't be loaded ({error})")),
        ),
    }
}

/// An object as it is now, taking what changed in the file: every key
/// whose value differs between `before` (how the file had it) and `start`
/// (how the file has it now), unless the object's own value for that key
/// is no longer what the file had (the game changed it).
fn merged(before: &Object, start: &Object, live: &Object) -> Object {
    let (was, is, mut now) = (
        before.to_json(true),
        start.to_json(true),
        live.to_json(true),
    );
    let keys: BTreeSet<&String> = was.keys().chain(is.keys()).collect();
    for key in keys {
        let (old, new) = (was.get(key), is.get(key));
        if old != new && now.get(key) == old {
            match new {
                Some(value) => now.insert(key.clone(), value.clone()),
                None => now.remove(key),
            };
        }
    }
    let mut object = Object::from_json(&now, &mut Vec::new());
    object.id = live.id;
    object
}

#[cfg(test)]
mod tests;
