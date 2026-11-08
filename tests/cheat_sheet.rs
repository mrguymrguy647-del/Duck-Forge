//! Keeps the AI guide's API cheat sheet (docs/AI_GUIDE.md, section 7) true.
//!
//! Every function the cheat sheet mentions is called here, the way the
//! cheat sheet says to call it, so renaming or removing one breaks this
//! file. And `every_function_in_the_cheat_sheet_is_used_here` reads the
//! cheat sheet itself, so a function added to it without an example here
//! fails too.

use std::collections::BTreeSet;

use duckforge::physics::{Body, BodyId, PhysicsWorld, Shape};
use duckforge::prelude::*;
use duckforge::{HeadlessOptions, LoadError, run_headless};

/// A small game that touches every part of `Context`, `Input`, `Rng`,
/// `Canvas`, sprites, the camera, sound and the debug overlay.
struct Everything {
    camera: Camera2D,
    duck: Sprite,
    walk: Animation,
    x: f32,
    inits: u32,
    resizes: u32,
}

impl Everything {
    fn new() -> Everything {
        let duck = Sprite::from_ascii(
            &["..yy", "yyyo"],
            &[('y', Color::YELLOW), ('o', Color::ORANGE)],
        );
        let sheet = Sprite::from_pixels(4, 2, vec![Color::WHITE; 8]);
        assert_eq!((duck.width(), duck.height()), (4, 2));
        assert_eq!(duck.get_pixel(0, 0).unwrap().a, 0, "'.' is see-through");
        let _cut = sheet.sub(0, 0, 2, 2);
        let walk = Animation::new(sheet.frames(2, 2), 8.0);
        assert_eq!(walk.index_at(0.2), 1);
        assert_eq!(walk.duration(), 0.25);
        Everything {
            camera: Camera2D::new(320.0, 240.0),
            duck,
            walk,
            x: 0.0,
            inits: 0,
            resizes: 0,
        }
    }
}

impl Game for Everything {
    fn init(&mut self, ctx: &mut Context) {
        self.inits += 1;
        ctx.report("width", ctx.width());
    }

    fn resize(&mut self, ctx: &mut Context, width: f32, height: f32) {
        self.resizes += 1;
        assert_eq!((ctx.width(), ctx.height()), (width, height));
    }

    fn update(&mut self, ctx: &mut Context) {
        // Context
        let _ = (ctx.dt(), ctx.time(), ctx.frame(), ctx.width(), ctx.height());
        ctx.report("jump", ctx.tune("jump_speed", 330.0));
        let level = Rect::new(0.0, 0.0, 320.0, 240.0);
        ctx.check(
            "player inside level",
            level.contains(vec2(self.x + 160.0, 120.0)),
        );
        ctx.check("frame 50 never comes", ctx.frame() != 50);
        if ctx.frame() > 1000 {
            ctx.idle_ok(); // waiting on purpose (a pause screen, say)
        }
        ctx.report("frame", ctx.frame());
        if ctx.frame() == 1 {
            ctx.lock_pointer(true);
        }
        ctx.report("locked", ctx.pointer_locked());
        ctx.report("state", format!("{:?}", Timestep::RealTime));
        // Input
        let input = &ctx.input;
        let _ = (
            input.is_down(Key::Space),
            input.was_pressed(Key::Space),
            input.was_released(Key::Space),
        );
        self.x += input.axis(Key::Left, Key::Right);
        let _ = (input.mouse_position(), input.mouse_delta(), input.scroll());
        let _ = input.is_mouse_down(MouseButton::Left)
            || input.was_mouse_pressed(MouseButton::Right)
            || input.was_mouse_released(MouseButton::Middle);
        // Rng
        let _ = (
            ctx.rng.range(0.0, 1.0),
            ctx.rng.range_i32(0, 10),
            ctx.rng.chance(0.25),
            ctx.rng.next_f32(),
        );
        // Camera
        self.camera.follow(vec2(self.x, 120.0), 0.2, ctx.dt());
        if ctx.frame() == 2 {
            self.camera.shake(3.0, 0.1);
            assert!(self.camera.is_shaking());
        }
        self.camera.update(ctx);
        let mouse = ctx.input.mouse_position().unwrap_or(Vec2::ZERO);
        let world = self.camera.screen_to_world(mouse);
        let _ = (
            self.camera.world_to_screen(world),
            self.camera.visible_area(),
        );
        let _ = (
            self.camera.position,
            self.camera.zoom,
            self.camera.screen_size,
        );
        // Sound
        if ctx.frame() == 3 {
            ctx.beep(440.0, 50.0);
            ctx.play(Sound::coin());
            ctx.play_panned(Sound::explosion(), -0.5); // from the left
            ctx.play(Sound::jump());
            ctx.play(Sound::hit());
            ctx.play(Sound::explosion());
            ctx.play(Sound {
                name: "laser",
                wave: Wave::Square,
                freq: 1200.0,
                slide_to: 200.0,
                ms: 150.0,
                volume: 0.3,
                ..Default::default()
            });
            for wave in [Wave::Sine, Wave::Triangle, Wave::Noise] {
                ctx.play(Sound {
                    wave,
                    attack: 1.0,
                    decay: 10.0,
                    ..Default::default()
                });
            }
        }
        // Debug overlay
        ctx.debug_rect(Rect::new(0.0, 0.0, 4.0, 4.0));
        ctx.debug_circle(vec2(2.0, 2.0), 2.0);
        if ctx.frame() == 1000 {
            ctx.quit();
        }
    }

    fn draw(&self, canvas: &mut Canvas) {
        canvas.clear(Color::BLACK);
        let glass = Color::rgba(0, 0, 255, 128);
        canvas.fill_rect(Rect::new(1.0, 1.0, 4.0, 4.0), Color::RED);
        canvas.draw_rect(Rect::new(1.0, 1.0, 4.0, 4.0), Color::GREEN.with_alpha(200));
        canvas.fill_circle(vec2(10.0, 10.0), 3.0, glass);
        canvas.draw_line(vec2(0.0, 0.0), vec2(9.0, 9.0), Color::from_hex(0x123456));
        canvas.fill_triangle(
            vec2(0.0, 0.0),
            vec2(5.0, 0.0),
            vec2(0.0, 5.0),
            Color::BLUE.lerp(Color::RED, 0.5),
        );
        canvas.set_pixel(1, 1, Color::WHITE);
        assert_eq!(canvas.get_pixel(1, 1), Some(Color::WHITE));
        canvas.push_camera(&self.camera);
        canvas.push_offset(vec2(1.0, 1.0));
        canvas.blit_sprite(&self.duck, vec2(20.0, 20.0), Blit::default());
        canvas.label("duck", Rect::new(20.0, 20.0, 4.0, 2.0));
        let blit = Blit {
            flip_x: true,
            flip_y: true,
            scale: 2.0,
            opacity: 0.5,
            ..Default::default()
        };
        let _tinted = Blit {
            tint: Color::RED,
            ..blit
        };
        canvas.blit_sprite(self.walk.frame_at(0.3), vec2(30.0, 20.0), blit);
        canvas.pop_offset();
        canvas.pop_camera();
        canvas.draw_text("Score 1\nLives 3", vec2(2.0, 2.0), 1, Color::WHITE);
        canvas.draw_text_centered("PAUSED", 100.0, 2, Color::GRAY);
        let _ = Canvas::text_width("PAUSED", 2) + Canvas::text_height(2) + Canvas::line_height(2);
        let _ = (canvas.width(), canvas.height());
        let _ = (Color::ORANGE, Color::YELLOW, Color::PURPLE);
    }
}

fn config() -> Config {
    Config {
        title: String::from("Cheat sheet"),
        width: 320,
        height: 240,
        scale: 3,
        target_fps: 60,
        show_fps: false,
        renderer: Renderer::Cpu,
        timestep: Timestep::Fixed {
            updates_per_second: 60,
        },
        seed: None,
        resizable: false,
        ..Config::default()
    }
}

#[test]
fn the_game_parts_of_the_cheat_sheet_work() {
    let mut sim = Simulation::new(config(), Everything::new());
    sim.press(Key::Right);
    sim.step();
    sim.release(Key::Right);
    sim.move_mouse(vec2(10.0, 10.0));
    sim.move_mouse_by(vec2(3.0, -1.0));
    sim.press_mouse(MouseButton::Left);
    sim.release_mouse(MouseButton::Left);
    sim.scroll(1.0);
    sim.run(5);
    let scene = sim.scene();
    let duck = scene.labels_named("duck").next().unwrap();
    let _ = (
        duck.name.as_str(),
        duck.world,
        duck.screen,
        duck.on_screen,
        scene.frame,
    );
    let _ = (
        scene.camera.map(|c| (c.position, c.zoom)),
        scene.texts.len(),
    );
    assert!(
        scene.describe().contains("duck at 20,20"),
        "{}",
        scene.describe()
    );
    sim.resize(200, 100);
    sim.set_debug_overlay(true);
    let _ = sim.picture();
    assert_eq!(sim.game().inits, 1);
    assert_eq!(sim.game().resizes, 1);
    assert_eq!(sim.sound_frames("coin"), [3]);
    assert_eq!(sim.sounds().len(), 10);
    sim.assert_sound("laser", 3, 0);
    // One of the game's own methods, with the real ctx, between frames.
    let frame = sim.with_context(|game, ctx| {
        game.x += 0.0;
        ctx.frame()
    });
    assert_eq!(frame, sim.context().frame());
    let replay = sim.recording();
    let file = std::env::temp_dir().join(format!("cheat-sheet-{}.json", std::process::id()));
    replay.save(&file).unwrap();
    let loaded = InputScript::load(&file).unwrap();
    assert_eq!(InputScript::from_file(&file).unwrap(), loaded);
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(InputScript::parse(&text).unwrap(), loaded);
    assert_eq!(InputScript::from_json(&text).unwrap(), loaded);
    let _ = std::fs::remove_file(file);
    let mut again = Simulation::new(config(), Everything::new());
    again.follow(loaded);
    again.run(6);
    assert_eq!(again.game().x, sim.game().x);
}

#[test]
fn headless_and_play_tests() {
    let folder = std::env::temp_dir().join(format!("duckforge-cheat-sheet-{}", std::process::id()));
    let options = HeadlessOptions {
        frames: 10,
        out: Some(folder.clone()),
        inputs: None,
        sheet: true,
        gif: true,
        debug: true,
        ..Default::default()
    };
    let sim = run_headless(config(), Everything::new(), &options).unwrap();
    assert_eq!(sim.number("frame"), 10.0);
    assert!(folder.join("audio.wav").exists());
    let failed = PlayTest::new(config(), Everything::new())
        .frames(60)
        .ignore_checks()
        .run();
    let check = &failed.failed_checks()[0];
    assert_eq!(
        (check.name.as_str(), check.frame),
        ("frame 50 never comes", 50)
    );
    let played = PlayTest::new(config(), Everything::new())
        .script(r#"{"events": [{"frame": 2, "press": "Right"}]}"#)
        .frames(30)
        .tune("jump_speed", 400.0)
        .run();
    played.assert_sound("coin", 3, 1);
    assert_eq!(played.number("jump"), 400.0);
    let tune_file =
        std::env::temp_dir().join(format!("cheat-sheet-{}.tune.json", std::process::id()));
    std::fs::write(&tune_file, r#"{"jump_speed": 350}"#).unwrap();
    let played = PlayTest::new(config(), Everything::new())
        .tune_file(&tune_file)
        .frames(2)
        .run();
    assert_eq!(played.number("jump"), 350.0);
    let _ = std::fs::remove_file(tune_file);
    let _ = std::fs::remove_dir_all(folder);
}

/// Plays a looping hum and a looping file, changes one's volume and stops it.
struct Looper {
    music: Option<LoopId>,
    theme: Option<Wav>,
}

impl Game for Looper {
    fn update(&mut self, ctx: &mut Context) {
        match ctx.frame() {
            1 => {
                let hum = Sound {
                    name: "hum",
                    wave: Wave::Sine,
                    freq: 80.0,
                    ms: 500.0,
                    ..Default::default()
                };
                let music: LoopId = ctx.play_loop(hum, 0.6);
                self.music = Some(music);
                if let Some(theme) = &self.theme {
                    ctx.play_loop(theme, 0.5);
                }
            }
            10 => ctx.set_loop_volume(self.music.unwrap(), 0.3),
            20 => ctx.stop_loop(self.music.unwrap()),
            _ => {}
        }
    }

    fn draw(&self, _: &mut Canvas) {}
}

#[test]
fn looping_sounds() {
    let played = PlayTest::new(
        config(),
        Looper {
            music: None,
            theme: None,
        },
    )
    .frames(30)
    .run();
    let hum = &played.loops()[0];
    assert_eq!(
        (hum.name.as_str(), hum.start, hum.stop),
        ("hum", 1, Some(20))
    );
    assert_eq!(hum.volumes, [(1, 0.6), (10, 0.3)]);
    assert_eq!(hum.volume_at(15), 0.3);
    assert!(!hum.is_playing());
}

#[test]
fn sprite_sheets_from_duck_quill() -> Result<(), LoadError> {
    let sheet = SpriteSheet::load("tests/fixtures/quill/scale1/duck.json")?;
    let names: Vec<&str> = sheet.names().collect();
    assert_eq!(names, ["idle", "jump"]);
    let jump: &Animation = sheet.animation("jump");
    assert!(
        !jump.looping,
        "jump plays once, then stays on its last frame"
    );
    assert!(sheet.get("swim").is_none());
    assert_eq!(
        (sheet.pivot(), sheet.frame_size(), sheet.scale()),
        (vec2(8.0, 16.0), (16, 16), 1)
    );
    assert_eq!(sheet.image().width(), 48);
    let mut canvas = Canvas::new(64, 64);
    let feet = vec2(32.0, 40.0);
    sheet.draw(&mut canvas, "idle", 0.0, feet, Blit::default());
    let mut by_hand = Canvas::new(64, 64);
    by_hand.draw_at_pivot(
        &sheet.animation("idle").frames[0],
        feet,
        sheet.pivot(),
        Blit::default(),
    );
    assert_eq!(canvas.pixels(), by_hand.pixels());
    let named = Config {
        tune_file: Some("examples/mygame.tune.json".into()),
        ..Default::default()
    };
    assert!(named.tune_file.is_some());
    Ok(())
}

#[test]
fn tile_maps_tilesets_and_backgrounds() -> Result<(), LoadError> {
    const LEVEL: &str = "
p.....
...==.
~~####";
    let legend = Legend::new()
        .solid('#', "grass")
        .one_way('=', "plank")
        .decoration('~', "water")
        .spawn('p', "player");
    let mut map = TileMap::parse(LEVEL, 16.0, &legend)?;
    assert_eq!(map.spawn("player"), vec2(8.0, 16.0));
    assert_eq!(map.spawns("player").len(), 1);
    assert_eq!((map.columns(), map.rows(), map.tile_size()), (6, 3, 16.0));
    assert_eq!(map.bounds(), Rect::new(0.0, 0.0, 96.0, 48.0));
    assert_eq!(map.cell_at(vec2(40.0, 20.0)), (2, 1));
    assert_eq!(map.tile_rect(2, 1), Rect::new(32.0, 16.0, 16.0, 16.0));
    assert_eq!(map.tile_at(3, 1), Some(("plank", TileKind::OneWay)));
    assert_eq!(
        map.kind_at(vec2(1.0, 40.0)),
        TileKind::Empty,
        "water is only drawn"
    );
    let mut body = Rect::new(66.0, 0.0, 8.0, 8.0);
    let velocity = vec2(0.0, 600.0);
    let touched: Contacts = map.move_and_slide(&mut body, velocity * (1.0 / 60.0));
    assert!(touched.ground && !touched.ceiling && !touched.wall_left && !touched.wall_right);
    assert_eq!(body.bottom(), 16.0, "on the plank");
    map.clear_tile(4, 1);
    map.set_tile(0, 1, "grass", TileKind::Solid);
    assert_eq!(map.tile_at(4, 1), None);

    let sheet = SpriteSheet::load("tests/fixtures/quill/scale1/duck.json")?;
    let mut tiles = Tileset::from_sheet(&sheet);
    let green = Sprite::from_pixels(16, 16, vec![Color::GREEN; 256]);
    let grid = Tileset::grid(&green, 16, 16, &["grass"]);
    tiles.insert("grass", grid.get("grass").unwrap().clone());
    for name in ["plank", "water"] {
        tiles.insert(name, Animation::new(vec![green.clone()], 1.0));
    }
    let mut canvas = Canvas::new(96, 48);
    let sky = Background::new(green.clone())
        .parallax(0.2)
        .parallax_xy(0.2, 0.1)
        .offset(vec2(0.0, 0.0))
        .repeat_x(true)
        .repeat_y(true);
    let camera = Camera2D::new(96.0, 48.0);
    canvas.draw_background(&sky, &camera);
    canvas.push_camera(&camera);
    assert_eq!(canvas.visible_area(), Rect::new(0.0, 0.0, 96.0, 48.0));
    map.draw(&mut canvas, &tiles, 0.0);
    canvas.pop_camera();
    assert_eq!(canvas.get_pixel(40, 40), Some(Color::GREEN));
    Ok(())
}

#[test]
fn math_and_collisions() {
    let v = vec2(3.0, 4.0);
    assert_eq!(v.length(), 5.0);
    let _ = (
        v.normalized(),
        v.distance(Vec2::ZERO),
        v.dot(v),
        v + v - v * 2.0,
    );
    let r = Rect::new(0.0, 0.0, 10.0, 10.0);
    let c = Rect::from_center(vec2(5.0, 5.0), 4.0, 4.0);
    let _ = (r.left(), r.right(), r.top(), r.bottom(), r.center());
    assert_eq!(
        (
            r.top_left(),
            r.top_right(),
            r.bottom_left(),
            r.bottom_right()
        ),
        (
            vec2(0.0, 0.0),
            vec2(10.0, 0.0),
            vec2(0.0, 10.0),
            vec2(10.0, 10.0)
        )
    );
    assert!(r.overlaps(&c) && r.contains(vec2(1.0, 1.0)));
    assert_eq!(r.inset(1.0), Rect::new(1.0, 1.0, 8.0, 8.0));
    assert_eq!(r.expand(1.0), Rect::new(-1.0, -1.0, 12.0, 12.0));
    assert_eq!(r.intersection(&c), Some(c));
    assert!(circle_vs_rect(vec2(5.0, -1.0), 2.0, r));
    assert!(circle_vs_circle(vec2(0.0, 0.0), 1.0, vec2(1.5, 0.0), 1.0));
    let hit: Option<Hit> = swept_aabb(Rect::new(-5.0, 0.0, 2.0, 2.0), vec2(10.0, 0.0), r);
    let hit = hit.unwrap();
    assert_eq!((hit.time, hit.normal), (0.3, vec2(-1.0, 0.0)));
    assert_eq!(approach(0.0, 10.0, 3.0), 3.0);
    assert_eq!(remap(5.0, (0.0, 10.0), (0.0, 1.0)), 0.5);
    let _ = vec3(0.0, 1.0, 0.0);
}

#[test]
fn loading_files_gives_one_sentence_errors() {
    let png: Result<Sprite, LoadError> = Sprite::load_png("no-such-file.png");
    assert!(
        png.unwrap_err()
            .to_string()
            .starts_with("couldn't find the image file")
    );
    let wav: Result<Wav, LoadError> = Wav::load("no-such-file.wav");
    let error = wav.unwrap_err();
    assert_eq!(
        format!("{error:?}"),
        error.to_string(),
        "Debug is the sentence too"
    );
    // play_wav with a real file is tested in src/headless.rs; here it only
    // needs to exist with the cheat sheet's signature.
    let _: fn(&mut Context, &Wav) = Context::play_wav;
    let _: fn(&mut Context, &Wav, f32) = Context::play_wav_panned;
    let _: fn(&Wav) -> bool = Wav::is_stereo;
}

#[test]
fn physics() {
    let mut world = PhysicsWorld::new(vec2(0.0, 400.0));
    let floor: BodyId = world.add(Body::block(vec2(160.0, 230.0), vec2(320.0, 20.0)).fixed());
    let ball = world.add(
        Body::ball(vec2(160.0, 100.0), 8.0)
            .with_velocity(vec2(0.0, 10.0))
            .with_bounce(0.5)
            .with_friction(0.6)
            .with_mass(2.0),
    );
    for _ in 0..600 {
        world.step(1.0 / 60.0); // ten seconds: long enough to settle on the floor
    }
    let body = world.get(ball).unwrap();
    let _ = (body.position, body.velocity);
    assert!(matches!(body.shape, Shape::Ball { .. }));
    assert_eq!(world.bodies().count(), 2);
    let _ = world.contacts();
    assert!(world.touching(ball, floor));
    assert!(world.remove(ball).is_some());
}

#[test]
fn three_d() {
    struct Scene {
        camera: Camera3D,
        meshes: Vec<Mesh>,
    }
    impl Game for Scene {
        fn update(&mut self, ctx: &mut Context) {
            self.camera.fly(ctx, 3.0);
        }
        fn draw(&self, canvas: &mut Canvas) {
            canvas.clear(Color::BLACK);
            for (i, mesh) in self.meshes.iter().enumerate() {
                let at = Transform::at(vec3(i as f32 - 1.5, 0.0, 5.0))
                    .rotated(vec3(0.3, 0.6, 0.0))
                    .sized(0.8);
                canvas.draw_mesh(mesh, &at, &self.camera);
            }
        }
    }
    let scene = Scene {
        camera: Camera3D::new(vec3(0.0, 0.0, 0.0)),
        meshes: vec![
            Mesh::cube(Color::RED),
            Mesh::sphere(Color::GREEN, 8),
            Mesh::pyramid(Color::BLUE),
            Mesh::checkerboard(4, 0.5, Color::WHITE, Color::GRAY),
        ],
    };
    let mut sim = Simulation::new(config(), scene);
    sim.step();
    let picture = sim.picture();
    let drawn = (0..240).any(|y| (0..320).any(|x| picture.get_pixel(x, y) != Some(Color::BLACK)));
    assert!(drawn, "the meshes were drawn");
    let _ = Camera3D::looking_at(vec3(0.0, 2.0, -5.0), Vec3::ZERO);
}

#[test]
fn three_d_textures_light_and_billboards() -> Result<(), LoadError> {
    let camera = Camera3D::looking_at(vec3(0.0, 2.0, -5.0), Vec3::ZERO);
    let mut canvas = Canvas::new(320, 240);
    let (screen, depth) = camera.project(Vec3::ZERO, &canvas).unwrap();
    assert_eq!(Some(screen), camera.world_to_screen(Vec3::ZERO, &canvas));
    assert!(depth > 5.0);

    // Shapes, and one built by hand with texture coordinates.
    let picture = Sprite::from_pixels(
        2,
        2,
        vec![Color::RED, Color::GREEN, Color::BLUE, Color::WHITE],
    );
    let mut sign = Mesh::new();
    let corners = [
        vec3(0.0, 0.0, 0.0),
        vec3(0.0, 1.0, 0.0),
        vec3(1.0, 1.0, 0.0),
        vec3(1.0, 0.0, 0.0),
    ];
    let [a, b, c, d] = corners.map(|p| sign.add_vertex(p));
    let uv = [
        vec2(0.0, 1.0),
        vec2(0.0, 0.0),
        vec2(1.0, 0.0),
        vec2(1.0, 1.0),
    ];
    sign.add_quad_uv(a, b, c, d, uv, Color::WHITE);
    let e = sign.add_vertex(vec3(0.5, 1.5, 0.0));
    sign.add_triangle_uv(b, e, c, [uv[1], vec2(0.5, 0.0), uv[2]], Color::WHITE);
    sign.add_triangle(a, b, d, Color::GRAY);
    sign.add_quad(a, b, c, d, Color::GRAY);
    let sign = sign.with_texture(picture.clone()).with_color(Color::WHITE);
    let shapes = [
        Mesh::cylinder(Color::RED, 12),
        Mesh::cone(Color::GREEN, 12).glowing(),
        Mesh::quad(1.0, 2.0).with_texture(picture.clone()),
        sign,
    ];
    let atlas = Tileset::grid(
        &Sprite::from_pixels(32, 16, vec![Color::GREEN; 512]),
        16,
        16,
        &["grass_top", "dirt"],
    );
    let block = Mesh::block(&atlas, ["grass_top", "dirt", "dirt"]);
    let mut chunk = Mesh::new();
    for x in 0..4 {
        chunk.merge(&block, &Transform::at(vec3(x as f32, 0.0, 0.0)));
    }
    assert_eq!(
        chunk.bounds(),
        (vec3(-0.5, -0.5, -0.5), vec3(3.5, 0.5, 0.5))
    );
    assert_eq!(atlas.uv_rect("dirt"), Rect::new(0.5, 0.0, 0.5, 1.0));
    let floor = Mesh::quad(4.0, 4.0).with_texture(atlas.tile("dirt"));
    assert!(floor.texture.is_some());
    let dirt = atlas.frame("dirt", 0.0);
    assert_eq!(dirt.place_in_picture(), [16, 0, 16, 16]);
    assert_eq!(dirt.whole_picture().width(), 32);
    let palette = BlockPalette::new(
        &atlas,
        &[
            BlockKind::new("stone", ["dirt"; 3]),
            BlockKind::new("glass", ["grass_top"; 3]).see_through(),
            BlockKind::new("leaves", ["grass_top"; 3]).inner_faces(),
            BlockKind::new("lamp", ["dirt"; 3]).glowing(),
        ],
    );
    let stone = palette.id("stone");
    let mut world = BlockWorld::new(8, 4, 8);
    world.set_below(stone);
    world.set(1, 0, 1, stone);
    assert_eq!(world.get(1, 0, 1), stone);
    assert_eq!(world.get(1, 1, 1), AIR);
    world.update_meshes(&palette);
    world.update_meshes_shaded(&palette, |_, _, y, _| if y > 0 { 1.0 } else { 0.5 });
    world.update_meshes_shaded(&palette, |_, x, _, _| FaceLight {
        shade: 1.0,
        glow: if x == 2 { 0.6 } else { 0.0 },
    });
    let mut glowing_corner = Mesh::cube(Color::GRAY);
    glowing_corner.set_glow(0, 1.0);
    assert_eq!(glowing_corner.glow_of(0), 1.0);
    canvas.set_fog(Some(Fog {
        color: Color::from_hex(0x86c4f2),
        near: 40.0,
        far: 120.0,
    }));
    assert!(canvas.fog().is_some());
    assert_eq!(world.chunk_mesh(0, 0).triangles.len(), 10);

    canvas.set_lighting(Lighting {
        sun_direction: vec3(0.0, 0.3, -1.0),
        sun_color: Color::from_hex(0xfff2d0),
        sun_strength: 1.0,
        ambient: 0.6,
    });
    assert_eq!(canvas.lighting().ambient, 0.6);
    canvas.set_lighting(Lighting {
        ambient: 0.5,
        ..Lighting::default()
    });
    canvas.clear(Color::BLACK);
    for (i, mesh) in shapes.iter().enumerate() {
        let at = Transform::at(vec3(i as f32 - 1.5, 0.5, 0.0)).scaled(vec3(0.8, 0.8, 0.8));
        canvas.draw_mesh(mesh, &at, &camera);
    }
    canvas.draw_mesh(&chunk, &Transform::at(vec3(-1.5, -1.0, 1.0)), &camera);
    world.draw(&mut canvas, &camera);
    canvas.set_fog(None);
    let picture = std::env::temp_dir().join(format!("cheat-sheet-{}.png", std::process::id()));
    canvas.save_png(&picture).expect("the picture is saved");
    let _ = std::fs::remove_file(&picture);
    let ghost = Color::rgba(160, 200, 255, 100);
    canvas.draw_mesh_tinted(
        &Mesh::cube(Color::WHITE),
        &Transform::default(),
        &camera,
        ghost,
    );
    let held = Transform::at(vec3(0.3, -0.3, 0.6)).sized(0.2);
    canvas.draw_mesh_in_view(&Mesh::cube(Color::RED), &held, &camera);
    let duck = SpriteSheet::load("tests/fixtures/quill/scale1/duck.json")?;
    let frame = duck.animation("idle").frame_at(0.0);
    canvas.draw_billboard(frame, vec3(0.0, 0.0, -1.0), 1.0, &camera);
    let look = Billboard {
        pivot: Some(duck.pivot()),
        flip_x: true,
        face_camera: true,
    };
    canvas.draw_billboard_with(frame, vec3(1.0, 0.0, -1.0), 1.0, &camera, look);
    let drawn = canvas.pixels().iter().filter(|&&p| p != 0).count();
    assert!(drawn > 5_000, "{drawn} pixels drawn");

    // Comparing renderers: without a graphics card, Gpu falls back to the CPU.
    let gpu = Simulation::with_renderer(config(), Everything::new(), Renderer::Gpu);
    assert_eq!(gpu.context().frame(), 0);
    Ok(())
}

/// The guide says which way each rotation turns (Duck Craft's arm first
/// pointed behind the camera for want of it). Keep the words and the
/// maths together.
#[test]
fn the_guide_says_which_way_rotations_turn_and_it_is_true() {
    let guide = include_str!("../docs/AI_GUIDE.md");
    for sentence in [
        "`rotate_y(a)`: forward (+z) towards right (+x)",
        "`rotate_x(a)`: forward (+z) towards up (+y)",
        "`rotate_z(a)`: right (+x) towards up (+y)",
        "`vec3(0.0, -1.0, 0.0).rotate_x(0.5)`",
        "has a positive z",
    ] {
        assert!(guide.contains(sentence), "the guide should say: {sentence}");
    }
    let forward = vec3(0.0, 0.0, 1.0);
    assert!(forward.rotate_y(0.5).x > 0.0, "y: forward turns right");
    assert!(forward.rotate_x(0.5).y > 0.0, "x: forward tips up");
    assert!(
        vec3(0.0, -1.0, 0.0).rotate_x(0.5).z > 0.0,
        "x: below swings forward"
    );
    assert!(
        vec3(1.0, 0.0, 0.0).rotate_z(0.5).y > 0.0,
        "z: right rolls up"
    );
    // The camera agrees: positive yaw looks right, positive pitch looks up.
    let mut camera = Camera3D::new(Vec3::ZERO);
    camera.yaw = 0.5;
    assert!(camera.forward().x > 0.0);
    camera.pitch = 0.5;
    assert!(camera.forward().y > 0.0);
}

/// The function names the cheat sheet mentions: every `name(` inside a
/// `code span` in section 7 of the guide.
fn cheat_sheet_functions() -> BTreeSet<String> {
    let guide = include_str!("../docs/AI_GUIDE.md");
    let start = guide
        .find("## 7. API cheat sheet")
        .expect("the cheat sheet section");
    let end = start
        + guide[start..]
            .find("\n## 8.")
            .expect("the section after it");
    let section = &guide[start..end];
    let mut names = BTreeSet::new();
    for (i, span) in section.split('`').enumerate() {
        if i % 2 == 0 {
            continue; // outside backticks
        }
        let bytes = span.as_bytes();
        let mut word_start = None;
        for (j, &b) in bytes.iter().enumerate() {
            let is_word = b.is_ascii_alphanumeric() || b == b'_';
            match (is_word, word_start) {
                (true, None) => word_start = Some(j),
                (false, Some(s)) => {
                    if b == b'(' {
                        names.insert(span[s..j].to_string());
                    }
                    word_start = None;
                }
                _ => {}
            }
        }
    }
    names
}

#[test]
fn every_function_in_the_cheat_sheet_is_used_here() {
    // Words that look like calls but are Rust itself, or example names.
    let not_engine = ["fn", "format", "Some", "Ok", "vec", "matches"];
    let this_file = include_str!("cheat_sheet.rs");
    let missing: Vec<String> = cheat_sheet_functions()
        .into_iter()
        .filter(|name| !not_engine.contains(&name.as_str()))
        .filter(|name| {
            !this_file.contains(&format!("{name}(")) && !this_file.contains(&format!("::{name};"))
        })
        .collect();
    assert!(
        missing.is_empty(),
        "the cheat sheet mentions these, but tests/cheat_sheet.rs never calls them \
         (add an example, or fix the cheat sheet): {missing:?}"
    );
    assert!(
        cheat_sheet_functions().len() > 100,
        "the cheat sheet was found and read"
    );
}
