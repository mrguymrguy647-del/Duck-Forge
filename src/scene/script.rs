//! Scripts: code that belongs to one object, the way Unity has them.
//!
//! A script is a type that implements [`Script`]. Its settings (the boxes
//! Duck Forge Studio's Inspector shows for it) are named in one place,
//! [`Script::settings`], and its work is done in [`Script::update`]:
//!
//! ```
//! use duckforge::prelude::*;
//!
//! #[derive(Default)]
//! pub struct Bee {
//!     speed: f32,
//! }
//!
//! impl Script for Bee {
//!     fn settings(&mut self, s: &mut Settings) {
//!         s.number("speed", &mut self.speed, 40.0).range(0.0, 200.0);
//!     }
//!
//!     fn update(&mut self, me: Id, scene: &mut Scene, ctx: &mut Context) {
//!         scene[me].at.x += self.speed * ctx.dt();
//!     }
//! }
//!
//! let scripts = Scripts::new().add::<Bee>("bee");
//! assert_eq!(scripts.names(), ["bee"]);
//! ```
//!
//! An object runs the script its `"kind"` names, once the scene has been
//! given the game's scripts ([`Scene::with_scripts`]):
//! `{"id": 4, "name": "sign", "kind": "bee", "speed": 60}`. Its settings
//! are read from its line: a setting the line leaves out has the script's
//! default, so changing a default in the code changes every object that
//! doesn't say otherwise.
//!
//! **Order.** Each update, scripted objects run in the order of their ids,
//! after the scene's clock and camera, so a game plays the same however
//! its file's lines are arranged. A script's [`start`](Script::start) runs
//! once, just before its first update.

use std::any::Any;
#[cfg(feature = "studio")]
use std::sync::Mutex;

use serde_json::{Value, json};

use super::object::{COMMON_KEYS, KEPT_FOR_LATER, PLAIN_KIND, colour_of};
use super::{Id, Object, Scene};
use crate::canvas::Canvas;
use crate::color::Color;
use crate::engine::Context;

/// Code for one object. Write a type with its settings as fields, derive
/// `Default`, and give it the parts it needs; every part has a default
/// that does nothing (or, for [`draw`](Script::draw), draws its picture).
///
/// See the [module docs](self) for an example.
pub trait Script: AsAny {
    /// Names its settings, each with the field it goes in and its value at
    /// the start: `s.number("speed", &mut self.speed, 40.0)`. The engine
    /// calls it to list them (for the Studio's Inspector), to give them the
    /// object's values when it starts, and again when the scene's file
    /// changes, so the fields that aren't settings keep what they have.
    fn settings(&mut self, s: &mut Settings) {
        let _ = s;
    }

    /// Once, before the object's first update (when the scene starts, or
    /// when the object is added or given this script).
    fn start(&mut self, me: Id, scene: &mut Scene, ctx: &mut Context) {
        let _ = (me, scene, ctx);
    }

    /// Every update (sixty times a second), while the game plays.
    fn update(&mut self, me: Id, scene: &mut Scene, ctx: &mut Context) {
        let _ = (me, scene, ctx);
    }

    /// Draws the object, in its place among the others. The usual draws
    /// its picture, as an object with no script is drawn.
    fn draw(&self, me: &Object, scene: &Scene, canvas: &mut Canvas) {
        scene.draw_picture(me, canvas);
    }
}

/// Lets the engine hand a script back as its own type
/// ([`Scene::script`]). Every type has it; there's nothing to write.
#[doc(hidden)]
pub trait AsAny: Any {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Any> AsAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ---- Settings ----

/// What kind of box a setting is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKind {
    /// A number: a slider when it has a range.
    Number,
    /// A whole number.
    Whole,
    /// Yes or no: a tick box.
    Flag,
    /// Words.
    Words,
    /// One of a list of words.
    Choice,
    /// A colour, `"#rrggbb"` in the file.
    Colour,
    /// A picture of the game's: `"assets/bee.png"`.
    Picture,
    /// A sound of the game's: `"assets/buzz.wav"`.
    Sound,
}

impl SettingKind {
    /// Its name in the Studio's list: `"number"`, `"flag"`...
    pub fn word(self) -> &'static str {
        match self {
            SettingKind::Number => "number",
            SettingKind::Whole => "whole",
            SettingKind::Flag => "flag",
            SettingKind::Words => "words",
            SettingKind::Choice => "choice",
            SettingKind::Colour => "colour",
            SettingKind::Picture => "picture",
            SettingKind::Sound => "sound",
        }
    }
}

/// One setting of a script, as [`Script::settings`] names it.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingInfo {
    pub name: String,
    pub kind: SettingKind,
    /// Its value at the start, as the file would have it.
    pub default: Value,
    /// The smallest and largest a number can be, if it says.
    pub range: Option<(f32, f32)>,
    /// What the Inspector calls it (its name, if it doesn't say).
    pub label: Option<String>,
    /// A sentence the Inspector shows under the pointer.
    pub help: Option<String>,
    /// A heading it goes under in the Inspector.
    pub group: Option<String>,
    /// Hidden in Beginner.
    pub expert: bool,
    /// For a choice: the words it can be.
    pub choices: Vec<String>,
}

impl SettingInfo {
    /// As JSON, for the Studio.
    pub fn to_json(&self) -> Value {
        let mut line = serde_json::Map::new();
        line.insert("name".into(), json!(self.name));
        line.insert("kind".into(), json!(self.kind.word()));
        line.insert("default".into(), self.default.clone());
        if let Some((low, high)) = self.range {
            line.insert("range".into(), json!([low, high]));
        }
        for (key, value) in [
            ("label", &self.label),
            ("help", &self.help),
            ("group", &self.group),
        ] {
            if let Some(value) = value {
                line.insert(key.into(), json!(value));
            }
        }
        if self.expert {
            line.insert("expert".into(), json!(true));
        }
        if !self.choices.is_empty() {
            line.insert("choices".into(), json!(self.choices));
        }
        Value::Object(line)
    }

    /// Is `value` right for this setting? If not, why, in words.
    fn wrong(&self, value: &Value) -> Option<String> {
        let like = |example: &str| format!("should be {example}");
        match self.kind {
            SettingKind::Number => (!value.is_number()).then(|| like("a number, like 40")),
            SettingKind::Whole => (!value.is_i64()).then(|| like("a whole number, like 3")),
            SettingKind::Flag => (!value.is_boolean()).then(|| like("true or false")),
            SettingKind::Words | SettingKind::Picture | SettingKind::Sound => {
                (!value.is_string()).then(|| like("words in quotes, like \"hello\""))
            }
            SettingKind::Colour => value
                .as_str()
                .and_then(colour_of)
                .is_none()
                .then(|| like("a colour, like \"#ffcc2e\"")),
            SettingKind::Choice => match value.as_str() {
                Some(word) if self.choices.iter().any(|choice| choice == word) => None,
                _ => Some(format!(
                    "should be one of {}",
                    self.choices
                        .iter()
                        .map(|choice| format!("{choice:?}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
            },
        }
    }
}

/// What [`Settings`] is doing: noting what the settings are, or giving
/// the fields an object's values.
enum Doing<'a> {
    List(&'a mut Vec<SettingInfo>),
    Load(&'a Object),
}

/// The settings of a script, as its [`Script::settings`] names them. Each
/// call names one, gives the field it goes in, and its value at the
/// start; what comes back can say more about it:
///
/// ```
/// # use duckforge::prelude::*;
/// # #[derive(Default)] struct Lift { how_far: f32, moves: String, carries: bool }
/// # impl Script for Lift {
/// fn settings(&mut self, s: &mut Settings) {
///     s.number("how_far", &mut self.how_far, 64.0).range(0.0, 400.0).label("Moves how far");
///     s.choice("moves", &mut self.moves, "up_down", &["up_down", "side_to_side"]);
///     s.flag("carries", &mut self.carries, true).help("Things standing on it go with it");
/// }
/// # }
/// ```
pub struct Settings<'a> {
    doing: Doing<'a>,
}

/// One setting just named, to say more about it. (Only noted when the
/// settings are being listed; it changes nothing else.)
pub struct Setting<'s> {
    info: Option<&'s mut SettingInfo>,
}

impl Setting<'_> {
    /// The smallest and largest it can be: the Inspector shows a slider.
    pub fn range(self, low: f32, high: f32) -> Self {
        self.with(|info| info.range = Some((low, high)))
    }

    /// What the Inspector calls it, if not its name.
    pub fn label(self, words: &str) -> Self {
        self.with(|info| info.label = Some(words.to_string()))
    }

    /// A sentence the Inspector shows under the pointer.
    pub fn help(self, words: &str) -> Self {
        self.with(|info| info.help = Some(words.to_string()))
    }

    /// A heading it goes under in the Inspector.
    pub fn group(self, words: &str) -> Self {
        self.with(|info| info.group = Some(words.to_string()))
    }

    /// Shown only in Expert.
    pub fn expert(self) -> Self {
        self.with(|info| info.expert = true)
    }

    fn with(mut self, change: impl FnOnce(&mut SettingInfo)) -> Self {
        if let Some(info) = self.info.as_deref_mut() {
            change(info);
        }
        self
    }
}

impl<'a> Settings<'a> {
    /// A number.
    pub fn number(&mut self, name: &str, field: &mut f32, default: f32) -> Setting<'_> {
        self.one(name, SettingKind::Number, json!(default), &[], |value| {
            *field = value.and_then(Value::as_f64).map_or(default, |n| n as f32);
        })
    }

    /// A whole number.
    pub fn whole(&mut self, name: &str, field: &mut i32, default: i32) -> Setting<'_> {
        self.one(name, SettingKind::Whole, json!(default), &[], |value| {
            *field = value.and_then(Value::as_i64).map_or(default, |n| {
                n.clamp(i32::MIN as i64, i32::MAX as i64) as i32
            });
        })
    }

    /// Yes or no: a tick box.
    pub fn flag(&mut self, name: &str, field: &mut bool, default: bool) -> Setting<'_> {
        self.one(name, SettingKind::Flag, json!(default), &[], |value| {
            *field = value.and_then(Value::as_bool).unwrap_or(default);
        })
    }

    /// Words.
    pub fn words(&mut self, name: &str, field: &mut String, default: &str) -> Setting<'_> {
        self.text(name, SettingKind::Words, field, default, &[])
    }

    /// One of a list of words.
    pub fn choice(
        &mut self,
        name: &str,
        field: &mut String,
        default: &str,
        choices: &[&str],
    ) -> Setting<'_> {
        self.text(name, SettingKind::Choice, field, default, choices)
    }

    /// A colour (`"#rrggbb"` in the file).
    pub fn colour(&mut self, name: &str, field: &mut Color, default: Color) -> Setting<'_> {
        let word = format!("#{:02x}{:02x}{:02x}", default.r, default.g, default.b);
        self.one(name, SettingKind::Colour, json!(word), &[], |value| {
            *field = value
                .and_then(Value::as_str)
                .and_then(colour_of)
                .unwrap_or(default);
        })
    }

    /// A picture of the game's (`"assets/bee.png"`): a picture dragged from
    /// Files can be dropped on its box.
    pub fn picture(&mut self, name: &str, field: &mut String, default: &str) -> Setting<'_> {
        self.text(name, SettingKind::Picture, field, default, &[])
    }

    /// A sound of the game's (`"assets/buzz.wav"`).
    pub fn sound(&mut self, name: &str, field: &mut String, default: &str) -> Setting<'_> {
        self.text(name, SettingKind::Sound, field, default, &[])
    }

    fn text(
        &mut self,
        name: &str,
        kind: SettingKind,
        field: &mut String,
        default: &str,
        choices: &[&str],
    ) -> Setting<'_> {
        self.one(name, kind, json!(default), choices, |value| {
            let word = value.and_then(Value::as_str);
            let right = match kind {
                SettingKind::Choice => word.filter(|word| choices.contains(word)),
                _ => word,
            };
            *field = right.unwrap_or(default).to_string();
        })
    }

    /// One setting: listed, or given the object's value (or its default,
    /// when the object hasn't one, or one of the wrong kind).
    fn one(
        &mut self,
        name: &str,
        kind: SettingKind,
        default: Value,
        choices: &[&str],
        give: impl FnOnce(Option<&Value>),
    ) -> Setting<'_> {
        match &mut self.doing {
            Doing::List(list) => {
                give(None);
                list.push(SettingInfo {
                    name: name.to_string(),
                    kind,
                    default,
                    range: None,
                    label: None,
                    help: None,
                    group: None,
                    expert: false,
                    choices: choices.iter().map(|choice| choice.to_string()).collect(),
                });
                Setting {
                    info: list.last_mut(),
                }
            }
            Doing::Load(object) => {
                let value = object.setting(name);
                give(value);
                Setting { info: None }
            }
        }
    }
}

/// Gives a script an object's values for its settings.
pub(crate) fn load(script: &mut dyn Script, object: &Object) {
    script.settings(&mut Settings {
        doing: Doing::Load(object),
    });
}

// ---- The game's scripts ----

/// Makes a fresh script of one type.
type Maker = fn() -> Box<dyn Script>;

fn make<T: Script + Default>() -> Box<dyn Script> {
    Box::new(T::default())
}

/// One script the game has: its name in scenes, how to make one, and its
/// settings.
#[derive(Clone)]
struct Known {
    name: String,
    make: Maker,
    settings: Vec<SettingInfo>,
    /// The type's name in the code, for messages.
    type_name: &'static str,
}

/// The game's scripts, by the names scenes give them. A game made with
/// Duck Forge Studio has them in `scripts.rs`, which the Studio keeps:
///
/// ```ignore
/// pub fn all() -> Scripts {
///     Scripts::new()
///         .add::<bee::Bee>("bee")
/// }
/// ```
///
/// and gives them to its scenes: `Scene::load(...)?.with_scripts(scripts::all())`.
#[derive(Clone, Default)]
pub struct Scripts {
    known: Vec<Known>,
}

impl std::fmt::Debug for Scripts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.names()).finish()
    }
}

impl Scripts {
    /// No scripts yet.
    pub fn new() -> Scripts {
        Scripts::default()
    }

    /// Adds a script: objects whose `"kind"` is `name` run it. A name
    /// given twice keeps the later one.
    // `add` reads as what it does in a list of scripts (`scripts.rs`); it
    // isn't `+`, and takes a type as well as a name.
    #[allow(clippy::should_implement_trait)]
    pub fn add<T: Script + Default>(mut self, name: &str) -> Scripts {
        let mut settings = Vec::new();
        T::default().settings(&mut Settings {
            doing: Doing::List(&mut settings),
        });
        self.known.retain(|known| known.name != name);
        self.known.push(Known {
            name: name.to_string(),
            make: make::<T>,
            settings,
            type_name: std::any::type_name::<T>(),
        });
        self
    }

    /// The scripts' names, in the order they were added.
    pub fn names(&self) -> Vec<&str> {
        self.known.iter().map(|known| known.name.as_str()).collect()
    }

    /// Is there a script called this?
    pub fn has(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn is_empty(&self) -> bool {
        self.known.is_empty()
    }

    /// A script's settings, as its `settings` names them.
    pub fn settings_of(&self, name: &str) -> Option<&[SettingInfo]> {
        self.get(name).map(|known| known.settings.as_slice())
    }

    fn get(&self, name: &str) -> Option<&Known> {
        self.known.iter().find(|known| known.name == name)
    }

    pub(crate) fn make(&self, name: &str) -> Option<Box<dyn Script>> {
        self.get(name).map(|known| (known.make)())
    }

    /// Every script with its settings, as JSON: what the Studio's
    /// Inspector draws its boxes from.
    pub fn to_json(&self) -> Value {
        Value::Array(
            self.known
                .iter()
                .map(|known| {
                    json!({
                        "name": known.name,
                        "type": short_type_name(known.type_name),
                        "settings": known.settings.iter().map(SettingInfo::to_json).collect::<Vec<_>>(),
                    })
                })
                .collect(),
        )
    }

    /// What's wrong with the scripts themselves (a setting with a name
    /// every object has), in words.
    pub(crate) fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for known in &self.known {
            if known.name == PLAIN_KIND
                || matches!(
                    known.name.as_str(),
                    "tilemap" | "background" | "sound" | "area" | "text" | "button"
                )
            {
                problems.push(format!(
                    "the script {:?} has the name of a kind Duck Forge already has, so \
                     objects of that kind won't run it; give it another name",
                    known.name
                ));
            }
            for setting in &known.settings {
                if COMMON_KEYS.contains(&setting.name.as_str())
                    || KEPT_FOR_LATER.contains(&setting.name.as_str())
                {
                    problems.push(format!(
                        "the script {:?} has a setting called {:?}, which every object \
                         already has, so it can't be one of a script's own; call it \
                         something else",
                        known.name, setting.name
                    ));
                }
            }
        }
        problems
    }

    /// What's wrong with an object's values for its script's settings.
    pub(crate) fn wrong_values(&self, object: &Object) -> Vec<String> {
        let Some(known) = self.get(&object.kind) else {
            return Vec::new();
        };
        known
            .settings
            .iter()
            .filter_map(|setting| {
                let value = object.setting(&setting.name)?;
                let why = setting.wrong(value)?;
                Some(format!(
                    "{:?}'s {:?} {why}; it's {value}, so it has the script's {}",
                    object.name, setting.name, setting.default
                ))
            })
            .collect()
    }
}

/// `scene_demo::scripts::bob::Bob` is `Bob`.
fn short_type_name(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

// ---- Scripts running in a scene ----

/// One object's script, running.
pub(crate) struct Running {
    /// The script's name: the object's kind when it started.
    pub(crate) name: String,
    pub(crate) script: Box<dyn Script>,
    pub(crate) started: bool,
}

/// The scripts running in a scene, by object. (A plain list: a scene
/// has few, and it keeps exported games small.) A copy of a scene starts
/// its scripts again, so this copies as empty.
#[derive(Default)]
pub(crate) struct RunningScripts(Vec<(Id, Running)>);

impl RunningScripts {
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(Id, &Running) -> bool) {
        self.0.retain(|(id, running)| keep(*id, running));
    }

    pub(crate) fn take(&mut self, id: Id) -> Option<Running> {
        let index = self.0.iter().position(|(have, _)| *have == id)?;
        Some(self.0.swap_remove(index).1)
    }

    pub(crate) fn put(&mut self, id: Id, running: Running) {
        self.take(id);
        self.0.push((id, running));
    }

    pub(crate) fn get(&self, id: Id) -> Option<&Running> {
        self.0.iter().find(|(have, _)| *have == id).map(|(_, r)| r)
    }

    pub(crate) fn get_mut(&mut self, id: Id) -> Option<&mut Running> {
        self.0
            .iter_mut()
            .find(|(have, _)| *have == id)
            .map(|(_, r)| r)
    }

    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = (Id, &mut Running)> {
        self.0.iter_mut().map(|(id, running)| (*id, running))
    }
}

impl Clone for RunningScripts {
    fn clone(&self) -> Self {
        RunningScripts::default()
    }
}

impl std::fmt::Debug for RunningScripts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entries(self.0.iter().map(|(id, running)| (id, &running.name)))
            .finish()
    }
}

// ---- For Duck Forge Studio ----

/// The scripts the game gave its scenes, as JSON, for the Studio's
/// question "scripts". `None` if no scene has been given any.
#[cfg(feature = "studio")]
static FOR_THE_STUDIO: Mutex<Option<String>> = Mutex::new(None);

#[cfg(feature = "studio")]
pub(crate) fn note_for_the_studio(scripts: &Scripts) {
    if let Ok(mut noted) = FOR_THE_STUDIO.lock() {
        *noted = Some(scripts.to_json().to_string());
    }
}

/// The answer to the Studio's question "scripts": whether the game gives
/// its scenes scripts at all, and which.
#[cfg(feature = "studio")]
pub fn studio_answer() -> String {
    let noted = FOR_THE_STUDIO.lock().ok().and_then(|noted| noted.clone());
    match noted {
        Some(list) => format!("{{\"given\": true, \"scripts\": {list}}}"),
        None => String::from("{\"given\": false, \"scripts\": []}"),
    }
}
