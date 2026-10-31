//! A 3D stress test: thousands of spinning shapes, to see how fast the
//! renderer is.
//!
//! Play with it:
//!     cargo run --release --example stress3d
//!
//! Benchmark it (runs for a fixed time, prints the average FPS, then quits):
//!     cargo run --release --example stress3d -- --objects 5000 --seconds 10 --renderer gpu
//!     cargo run --release --example stress3d -- --objects 5000 --seconds 10 --renderer cpu
//!
//! Options (everything after the `--` goes to this program, not to Cargo):
//!     --objects N        how many shapes to draw (default 2000)
//!     --seconds S        benchmark mode: measure for S seconds, print, then quit
//!     --renderer NAME    what draws 3D: gpu, cpu or auto (default auto)
//!
//! Controls: number keys 1-6 pick 500 / 1,000 / 2,000 / 5,000 / 10,000 /
//! 20,000 objects, O toggles the automatic orbiting camera (turn it off to
//! fly with WASD, Q/E and the arrows), Escape quits.

use std::f32::consts::TAU;
use std::time::Instant;

use duckforge::prelude::*;

const SPACING: f32 = 2.0;
const PALETTE: [Color; 6] = [
    Color::RED,
    Color::ORANGE,
    Color::YELLOW,
    Color::GREEN,
    Color::BLUE,
    Color::PURPLE,
];
const PRESETS: [(Key, usize); 6] = [
    (Key::Digit1, 500),
    (Key::Digit2, 1_000),
    (Key::Digit3, 2_000),
    (Key::Digit4, 5_000),
    (Key::Digit5, 10_000),
    (Key::Digit6, 20_000),
];
/// Benchmark mode ignores the first frames, while everything warms up.
const WARM_UP_SECONDS: f32 = 2.0;
/// More objects than this would mostly just run out of memory.
const MAX_OBJECTS: usize = 1_000_000;

/// Settings read from the command line.
struct Options {
    objects: usize,
    benchmark_seconds: Option<f32>,
    renderer: Renderer,
}

impl Options {
    /// Reads `--objects N`, `--seconds S` and `--renderer NAME` from the
    /// command line. `game_args` leaves out the engine's own options, like
    /// `--headless`, which `run` reads.
    fn from_args() -> Result<Options, String> {
        Options::parse(duckforge::game_args().into_iter())
    }

    /// Reads the options from a list of words (separate from `from_args`, so
    /// tests can try it without a real command line).
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Options, String> {
        let mut options = Options {
            objects: 2_000,
            benchmark_seconds: None,
            renderer: Renderer::Auto,
        };
        while let Some(arg) = args.next() {
            let mut value = || args.next().ok_or(format!("{arg} needs a value"));
            match arg.as_str() {
                "--objects" => {
                    options.objects = value()?
                        .parse()
                        .map_err(|_| "--objects needs a whole number")?;
                    if options.objects > MAX_OBJECTS {
                        return Err(format!("--objects can be at most {MAX_OBJECTS}"));
                    }
                }
                "--seconds" => {
                    let seconds: f32 = value()?.parse().map_err(|_| "--seconds needs a number")?;
                    // "NaN" and "inf" parse as numbers too, but would run forever.
                    if !(seconds.is_finite() && seconds > 0.0) {
                        return Err(String::from("--seconds must be more than 0"));
                    }
                    options.benchmark_seconds = Some(seconds);
                }
                "--renderer" => {
                    options.renderer = match value()?.as_str() {
                        "gpu" => Renderer::Gpu,
                        "cpu" => Renderer::Cpu,
                        "auto" => Renderer::Auto,
                        other => {
                            return Err(format!(
                                "unknown renderer {other:?}: use gpu, cpu or auto"
                            ));
                        }
                    };
                }
                _ => return Err(format!("unknown option: {arg}")),
            }
        }
        Ok(options)
    }
}

/// One shape in the field.
struct Object {
    position: Vec3,
    /// Which mesh to draw: an index into `Stress::meshes`.
    mesh: usize,
    spin_speed: f32,
    phase: f32,
}

/// Measures the average frame rate over a fixed stretch of time.
struct Benchmark {
    seconds: f32,
    started: Option<Instant>,
    frames: u32,
}

struct Stress {
    objects: Vec<Object>,
    /// Every shape in every color: [cube, sphere, pyramid] x PALETTE.
    meshes: Vec<Mesh>,
    floor: Mesh,
    camera: Camera3D,
    orbit: bool,
    time: f32,
    triangles: usize,
    fps: f32,
    renderer_name: String,
    benchmark: Option<Benchmark>,
    launched: Instant,
}

impl Stress {
    fn new(options: &Options) -> Self {
        let mut meshes = Vec::new();
        for &color in &PALETTE {
            meshes.push(Mesh::cube(color));
            meshes.push(Mesh::sphere(color, 12));
            meshes.push(Mesh::pyramid(color));
        }
        let mut stress = Self {
            objects: Vec::new(),
            meshes,
            floor: Mesh::checkerboard(1, 1.0, Color::GRAY, Color::GRAY),
            camera: Camera3D::new(Vec3::ZERO),
            orbit: true,
            time: 0.0,
            triangles: 0,
            fps: 0.0,
            renderer_name: String::new(),
            benchmark: options.benchmark_seconds.map(|seconds| Benchmark {
                seconds,
                started: None,
                frames: 0,
            }),
            launched: Instant::now(),
        };
        stress.build(options.objects);
        stress
    }

    /// Lays `count` objects out on a square grid, centered on (0, 0, 0).
    fn build(&mut self, count: usize) {
        let mut rng = Rng::new(1234); // the same layout every run, so runs are comparable
        let side = (count as f32).sqrt().ceil() as usize;
        let half = side as f32 * SPACING / 2.0;

        self.objects = (0..count)
            .map(|i| {
                let (row, column) = (i / side, i % side);
                Object {
                    position: vec3(
                        column as f32 * SPACING - half,
                        0.6 + rng.range(0.0, 1.5),
                        row as f32 * SPACING - half,
                    ),
                    mesh: rng.range_i32(0, self.meshes.len() as i32) as usize,
                    spin_speed: rng.range(0.5, 2.0),
                    phase: rng.range(0.0, TAU),
                }
            })
            .collect();

        self.triangles = self
            .objects
            .iter()
            .map(|object| self.meshes[object.mesh].triangles.len())
            .sum();
        let floor_tiles = (side + 2).min(200);
        self.floor = Mesh::checkerboard(
            floor_tiles,
            (side + 2) as f32 * SPACING / floor_tiles as f32,
            Color::from_hex(0x3A_3F_4F),
            Color::from_hex(0x30_34_42),
        );
        self.place_camera();
    }

    /// Circles the camera around the field, far enough out to see all of it.
    fn place_camera(&mut self) {
        let size = (self.objects.len() as f32).sqrt() * SPACING;
        let radius = size * 0.75 + 8.0;
        let angle = self.time * 0.15;
        let position = vec3(
            angle.sin() * radius,
            size * 0.35 + 4.0,
            -angle.cos() * radius,
        );
        self.camera = Camera3D::looking_at(position, Vec3::ZERO);
    }

    /// In benchmark mode: count frames after the warm-up, then report and quit.
    fn update_benchmark(&mut self, ctx: &mut Context) {
        let Some(benchmark) = &mut self.benchmark else {
            return;
        };
        if self.launched.elapsed().as_secs_f32() < WARM_UP_SECONDS {
            return;
        }
        let started = *benchmark.started.get_or_insert_with(Instant::now);
        benchmark.frames += 1;

        let elapsed = started.elapsed().as_secs_f32();
        if elapsed >= benchmark.seconds {
            let fps = benchmark.frames as f32 / elapsed;
            println!(
                "stress3d: {} objects, {} triangles, {}: {:.1} FPS average ({:.2} ms per frame, {} frames in {:.1} s)",
                self.objects.len(),
                self.triangles,
                ctx.renderer_name(),
                fps,
                1000.0 / fps,
                benchmark.frames,
                elapsed,
            );
            ctx.quit();
        }
    }
}

impl Game for Stress {
    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.was_pressed(Key::Escape) {
            ctx.quit();
        }
        for (key, count) in PRESETS {
            if ctx.input.was_pressed(key) {
                self.build(count);
            }
        }
        if ctx.input.was_pressed(Key::O) {
            self.orbit = !self.orbit;
        }

        self.time += ctx.dt();
        if self.orbit {
            self.place_camera();
        } else {
            self.camera.fly(ctx, 10.0);
        }
        self.fps = ctx.fps();
        self.renderer_name = ctx.renderer_name().to_string();
        self.update_benchmark(ctx);
    }

    fn draw(&self, canvas: &mut Canvas) {
        canvas.clear(Color::from_hex(0x10_12_1C));
        canvas.draw_mesh(&self.floor, &Transform::default(), &self.camera);

        for object in &self.objects {
            let angle = self.time * object.spin_speed + object.phase;
            let transform = Transform::at(object.position)
                .rotated(vec3(angle * 0.7, angle, 0.0))
                .sized(1.2);
            canvas.draw_mesh(&self.meshes[object.mesh], &transform, &self.camera);
        }

        let info = format!(
            "{} OBJECTS  {} TRIANGLES  FPS {:.0}",
            self.objects.len(),
            self.triangles,
            self.fps
        );
        canvas.fill_rect(Rect::new(0.0, 0.0, 320.0, 25.0), Color::BLACK);
        canvas.draw_text(&info, vec2(4.0, 3.0), 1, Color::YELLOW);
        canvas.draw_text(&self.renderer_name, vec2(4.0, 10.0), 1, Color::WHITE);
        canvas.draw_text(
            "1-6: OBJECT COUNT   O: ORBIT/FLY",
            vec2(4.0, 17.0),
            1,
            Color::GRAY,
        );
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = match Options::from_args() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            eprintln!("usage: stress3d [--objects N] [--seconds S] [--renderer gpu|cpu|auto]");
            eprintln!("(plus the options every duckforge game has: see --help)");
            std::process::exit(2);
        }
    };

    let config = Config {
        title: String::from("3D stress test - duckforge"),
        target_fps: 0, // no frame-rate cap: run as fast as possible
        // One update per drawn frame, so counting updates counts frames.
        timestep: Timestep::RealTime,
        renderer: options.renderer,
        ..Config::default()
    };
    duckforge::run(config, Stress::new(&options))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Result<Options, String> {
        Options::parse(line.split_whitespace().map(String::from))
    }

    #[test]
    fn good_options_are_read() {
        let options = parse("--objects 5000 --seconds 10 --renderer cpu").unwrap();
        assert_eq!(options.objects, 5000);
        assert_eq!(options.benchmark_seconds, Some(10.0));
        assert_eq!(options.renderer, Renderer::Cpu);
        assert_eq!(parse("").unwrap().objects, 2_000);
    }

    #[test]
    fn nonsense_options_are_refused() {
        // A NaN or infinite benchmark would never finish; zero or less makes no sense.
        for seconds in ["NaN", "inf", "0", "-5"] {
            assert!(parse(&format!("--seconds {seconds}")).is_err(), "{seconds}");
        }
        assert!(parse("--objects 50000000").is_err()); // gigabytes of objects
        assert!(parse("--objects many").is_err());
        assert!(parse("--renderer quantum").is_err());
        assert!(parse("--seconds").is_err()); // missing value
        assert!(parse("--speed 3").is_err());
    }
}
