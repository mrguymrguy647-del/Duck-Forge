//! Pip, a small explorer made with all three Duck apps, walking round a
//! yard of plank crates.
//!
//! Run it with:   cargo run --example pip_walk
//! Controls:      the arrow keys walk Pip about, Escape quits.
//!
//! - Pip's **face** was painted in Duck Quill: `art/textures/pip_face.json`
//!   is the script, `pip_face.png` what it draws.
//! - Pip was **put together and animated** in Duck Mold: `art/pip.toml` is
//!   the recipe, with the animations `idle` and `walk`, a `footstep` on
//!   each step, hit shapes and two far-away versions. Its `export` line
//!   sends the model to `assets/pip.glb`, which is what this game loads.
//!   The crates are `art/plank_crate.toml`, with two more pictures.
//! - Here, in Duck Forge, Pip **walks**: the game moves Pip along the
//!   ground and plays `walk` while it does; the crates are in the way.
//!
//! The pictures on the models take no code at all: a model with images
//! loads and draws with the same lines as one without.
//!
//! **Change Pip while the game runs.** Open `art/pip.toml` in Duck Mold
//! (or click it in Duck Forge Studio's Files panel, where it opens in a
//! tab), make a change, and export: Pip in the game has it at once,
//! still mid-step. The one line that does it is in `reload` below.

use std::f32::consts::{PI, TAU};
use std::path::Path;

use duckforge::LoadError;
use duckforge::glb::AnimatedModel;
use duckforge::prelude::*;

const ASSETS: &str = "examples/pip_walk/assets";

const SKY: Color = Color::from_hex(0x8F_C7_E8);
const GRASS_LIGHT: Color = Color::from_hex(0x6F_A8_55);
const GRASS_DARK: Color = Color::from_hex(0x64_9C_4C);

/// How fast Pip walks, in metres a second. The walk animation takes two
/// steps in 0.6 seconds, each about a quarter of a metre long: at this
/// speed the boots don't slide.
const WALK_SPEED: f32 = 0.85;
/// How fast Pip turns to face the way it walks: a full turn in a second.
const TURN_SPEED: f32 = TAU;
/// How far from the middle of the yard Pip may go.
const EDGE: f32 = 2.4;
/// How much room Pip takes on the ground: a circle this far round.
const PIP_RADIUS: f32 = 0.2;

/// Where the crates stand (the middle of each one's underside) and how
/// big each is: 1 is a metre along every side, as its recipe makes it.
const CRATES: [(Vec3, f32); 4] = [
    (vec3(-1.6, 0.0, -0.6), 1.0),
    (vec3(1.5, 0.0, 0.5), 1.0),
    (vec3(0.3, 0.0, -2.0), 1.0),
    (vec3(1.45, 1.0, 0.45), 0.6),
];

struct PipWalk {
    camera: Camera3D,
    pip: AnimatedModel,
    /// The plank crate: one mesh, drawn in four places.
    crate_mesh: Mesh,
    yard: Mesh,
    /// Where Pip stands.
    position: Vec3,
    /// The way Pip faces, in radians: 0 is towards the camera (+z).
    facing: f32,
    /// How many steps Pip has taken: the `footstep` events of its walk.
    steps: u32,
}

impl PipWalk {
    fn new() -> Result<PipWalk, LoadError> {
        let mut pip = AnimatedModel::load(format!("{ASSETS}/pip.glb"))?;
        pip.play("idle");
        Ok(PipWalk {
            // In front of Pip (its +z side) and above, looking back and
            // down at the yard.
            camera: Camera3D::looking_at(vec3(0.0, 2.1, 3.9), vec3(0.0, 0.5, -0.2)),
            pip,
            crate_mesh: Mesh::load_glb(format!("{ASSETS}/plank_crate.glb"))?,
            yard: Mesh::checkerboard(14, 1.0, GRASS_LIGHT, GRASS_DARK),
            position: vec3(0.0, 0.0, 1.2),
            facing: 0.0,
            steps: 0,
        })
    }

    /// Where Pip is drawn: where it stands, facing its way.
    fn place(&self) -> Transform {
        Transform::at(self.position).rotated(vec3(0.0, self.facing, 0.0))
    }

    /// Keeps Pip out of the crates that stand on the ground: if the
    /// circle Pip takes up reaches into a crate's square, Pip is moved
    /// out the short way.
    fn stay_out_of_the_crates(&mut self) {
        for (at, size) in CRATES {
            if at.y > 0.5 {
                continue; // on top of another: nothing to walk into
            }
            let reach = size / 2.0 + PIP_RADIUS;
            let (off_x, off_z) = (self.position.x - at.x, self.position.z - at.z);
            if off_x.abs() >= reach || off_z.abs() >= reach {
                continue;
            }
            // Out through the nearer side.
            if reach - off_x.abs() < reach - off_z.abs() {
                self.position.x = at.x + reach.copysign(off_x);
            } else {
                self.position.z = at.z + reach.copysign(off_z);
            }
        }
    }
}

impl Game for PipWalk {
    fn update(&mut self, ctx: &mut Context) {
        // The camera looks back at Pip from +z, so your right is -x and
        // "away" is -z.
        let step = vec3(
            ctx.input.axis(Key::Right, Key::Left),
            0.0,
            ctx.input.axis(Key::Up, Key::Down),
        );
        let walking = step != Vec3::ZERO;
        if walking {
            let step = step.normalized();
            self.position += step * (WALK_SPEED * ctx.dt());
            self.position.x = self.position.x.clamp(-EDGE, EDGE);
            self.position.z = self.position.z.clamp(-EDGE, EDGE);
            self.stay_out_of_the_crates();
            // Turn towards the way it goes, the short way round.
            let wanted = step.x.atan2(step.z);
            let off = (wanted - self.facing + PI).rem_euclid(TAU) - PI;
            let most = TURN_SPEED * ctx.dt();
            self.facing = (self.facing + off.clamp(-most, most)).rem_euclid(TAU);
        }

        // `play` changes nothing if that is playing already, so it can
        // be asked every frame.
        self.pip.play(if walking { "walk" } else { "idle" });
        self.pip.update(ctx.dt());
        // Each `footstep` the walk passed, once.
        for event in self.pip.events() {
            if event.name == "footstep" {
                self.steps += 1;
            }
        }

        ctx.report("x", self.position.x);
        ctx.report("z", self.position.z);
        ctx.report("facing_degrees", self.facing.to_degrees());
        ctx.report("animation", self.pip.playing().unwrap_or("none"));
        ctx.report("steps", self.steps as f32);
        ctx.report("model", self.pip.model().name.as_str());
    }

    /// A model was saved again while the game runs (changed in Duck Mold
    /// and exported, or sent from Duck Forge Studio). Pip takes its new
    /// model and carries on, mid-step; the crate is a plain mesh, so it
    /// is loaded again.
    fn reload(&mut self, _ctx: &mut Context, file: &Path) -> bool {
        if self.pip.reload_if_changed(file) {
            return true;
        }
        if file.ends_with("plank_crate.glb") {
            match Mesh::load_glb(file) {
                Ok(mesh) => self.crate_mesh = mesh,
                Err(error) => println!("pip_walk: {error} The crates stay as they were."),
            }
            return true;
        }
        false
    }

    fn draw(&self, canvas: &mut Canvas) {
        canvas.clear(SKY);
        // The sun comes from above, a little left, and from the camera's
        // side, so Pip's face is lit.
        canvas.set_lighting(Lighting {
            sun_direction: vec3(-0.4, 1.0, 0.8),
            ambient: 0.5,
            ..Lighting::default()
        });
        canvas.draw_mesh(&self.yard, &Transform::default(), &self.camera);
        for (at, size) in CRATES {
            // The top one is turned a little, as if set down in a hurry.
            let turn = if at.y > 0.5 { 0.5 } else { 0.0 };
            let place = Transform::at(at)
                .rotated(vec3(0.0, turn, 0.0))
                .scaled(vec3(size, size, size));
            canvas.draw_mesh(&self.crate_mesh, &place, &self.camera);
        }
        // A model with images is drawn like any other.
        self.pip.draw_mesh(canvas, &self.place(), &self.camera);
        if let Some((point, _)) = self
            .camera
            .project(self.position + vec3(0.0, 0.55, 0.0), canvas)
        {
            canvas.label("pip", Rect::new(point.x - 14.0, point.y - 28.0, 28.0, 56.0));
        }

        canvas.draw_text(
            "PIP: PAINTED IN DUCK QUILL, BUILT IN DUCK MOLD",
            vec2(4.0, 4.0),
            1,
            Color::from_hex(0x1B_14_46),
        );
        canvas.draw_text(
            "ARROWS: WALK",
            vec2(4.0, 12.0),
            1,
            Color::from_hex(0x2F_4A_5E),
        );
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config {
        title: String::from("Pip walk: a character made with all three Duck apps - duckforge"),
        ..Config::default()
    };
    duckforge::run(config, PipWalk::new()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simulation() -> Simulation<PipWalk> {
        Simulation::new(Config::default(), PipWalk::new().expect("the models load"))
    }

    #[test]
    fn pip_stands_idle_until_an_arrow_walks_it() {
        let mut sim = simulation();
        sim.run(30);
        assert_eq!(sim.text("animation"), "idle");
        assert_eq!(sim.text("model"), "pip");
        assert_eq!((sim.number("x"), sim.number("z")), (0.0, 1.2));
        assert_eq!(sim.number("steps"), 0.0);

        // Down is towards you: +z.
        sim.press(Key::Down);
        sim.run(60);
        assert_eq!(sim.text("animation"), "walk");
        let z = sim.number("z");
        assert!(
            (z - 1.2 - f64::from(WALK_SPEED)).abs() < 0.02,
            "a second's walk: {z}"
        );
        // Two steps to a round of 0.6 seconds: three or four in a second.
        let steps = sim.number("steps");
        assert!((3.0..=4.0).contains(&steps), "{steps}");

        sim.release(Key::Down);
        sim.run(30);
        assert_eq!(sim.text("animation"), "idle");
        assert_eq!(sim.number("z"), z, "standing still");
        assert_eq!(sim.number("steps"), steps, "and making no more steps");
    }

    #[test]
    fn a_crate_is_in_the_way() {
        let mut sim = simulation();
        // The crate on the right of the picture is at x = -1.6, from
        // z = -1.1 to -0.1. Pip walks up beside it, then into its side.
        sim.press(Key::Up);
        sim.run(120);
        sim.release(Key::Up);
        let z = sim.number("z");
        assert!((-0.9..=-0.3).contains(&z), "beside the crate: {z}");
        sim.press(Key::Right);
        sim.run(240);
        // Stopped at the crate's side: half a metre and Pip's own
        // radius from its middle.
        let x = sim.number("x");
        assert!((x + 1.6 - f64::from(0.5 + PIP_RADIUS)).abs() < 0.01, "{x}");
        assert_eq!(sim.text("animation"), "walk", "still trying");
        // The other way there is nothing: on to the yard's edge.
        sim.release(Key::Right);
        sim.press(Key::Left);
        sim.run(60);
        assert!(sim.number("x") > x + 0.5);
    }

    #[test]
    fn the_models_come_with_their_pictures() {
        let sim = simulation();
        let game = sim.game();
        // Pip's face and the crate's planks: each mesh has its picture,
        // and the triangles that show one say where on it they are.
        assert!(game.pip.mesh().texture.is_some());
        assert!(game.crate_mesh.texture.is_some());
        let pictured = |mesh: &Mesh| mesh.uvs.iter().filter(|place| place.is_some()).count();
        assert_eq!(
            pictured(game.pip.mesh()),
            2,
            "the two triangles of the face"
        );
        assert_eq!(pictured(&game.crate_mesh), 12, "every side of the crate");
    }

    #[test]
    fn the_face_is_in_the_picture_with_sharp_pixels() {
        let mut sim = simulation();
        sim.run(5);
        // Pip faces the camera. Its eyes are the darkest thing on its
        // head, and its cheeks are pink: both colours are in the
        // picture exactly as they were painted and lit, not blurred
        // into the skin around them.
        let picture = sim.picture();
        let mut dark = 0;
        for y in 0..picture.height() as i32 {
            for x in 0..picture.width() as i32 {
                let pixel = picture.get_pixel(x, y).unwrap();
                if pixel.r < 60 && pixel.g < 50 && pixel.b < 70 {
                    dark += 1;
                }
            }
        }
        assert!(dark >= 4, "the eyes are drawn: {dark} dark pixels");
    }

    #[test]
    fn a_model_saved_again_is_taken_while_pip_walks() {
        // The game's own copy of the model, to save over: a test never
        // changes the example's files.
        let folder = std::env::temp_dir().join(format!("pip-walk-test-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("pip.glb");
        std::fs::copy(format!("{ASSETS}/pip.glb"), &file).unwrap();
        let mut game = PipWalk::new().unwrap();
        game.pip = AnimatedModel::load(&file).unwrap();
        let mut sim = Simulation::new(Config::default(), game);
        sim.press(Key::Down);
        sim.run(40);
        let steps = sim.number("steps");
        let triangles = sim.game().pip.mesh().triangles.len();

        // The crate saved over Pip's file: the game is told the file
        // changed, and takes it without starting again.
        std::fs::copy(format!("{ASSETS}/plank_crate.glb"), &file).unwrap();
        let name = file.to_string_lossy().replace('\\', "/");
        assert!(sim.change_file(&name, ""));
        assert_eq!(sim.game().pip.model().name, "plank_crate");
        assert_ne!(sim.game().pip.mesh().triangles.len(), triangles);
        sim.run(10);
        assert_eq!(sim.number("steps"), steps, "a crate has no walk");

        // And Pip again: it walks on.
        std::fs::copy(format!("{ASSETS}/pip.glb"), &file).unwrap();
        assert!(sim.change_file(&name, ""));
        sim.run(40);
        assert_eq!(sim.text("model"), "pip");
        assert_eq!(sim.text("animation"), "walk");
        assert!(sim.number("steps") > steps);
        // A file that is nobody's is not taken.
        assert!(!sim.change_file("examples/pip_walk/assets/other.glb", ""));
        let _ = std::fs::remove_dir_all(&folder);
    }
}
