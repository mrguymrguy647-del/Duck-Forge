//! What was hit: Duck Mold models' hit shapes, tested in many poses.
//!
//! `robot`, `gnome`, `door` and `cart` in `tests/fixtures/duck_mold/`
//! have hit shapes (`colliders = "parts"` in their recipes), and each
//! `<name>.colliders.json` is what `duck-mold inspect --colliders --json`
//! said about them. These tests check that the file holds what Duck Mold
//! said, that every shape goes with its part through every pose and
//! animation, and that a ray, a segment or a ball finds the right part:
//! checked against a slow, simple search written here that walks along
//! the ray in tiny steps.

use std::f32::consts::PI;
use std::path::{Path, PathBuf};

mod common;
use common::TestFolder;

use duckforge::LoadError;
use duckforge::glb::AnimatedModel;
use duckforge::hits::{HitShape, HitVolume};
use duckforge::prelude::*;

/// The models with hit shapes.
const WITH_HITS: [&str; 4] = ["robot", "gnome", "door", "cart"];

fn fixtures(test: &str) -> (TestFolder, PathBuf) {
    let folder = TestFolder::new(test);
    let models = folder.fixture("duck_mold");
    (folder, models)
}

fn json(models: &Path, file: &str) -> serde_json::Value {
    let text = std::fs::read_to_string(models.join(file)).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn three(value: &serde_json::Value) -> Vec3 {
    let n = |at: usize| value[at].as_f64().unwrap() as f32;
    vec3(n(0), n(1), n(2))
}

/// How far `point` is outside a shape (0 inside): worked out here from
/// the shape's numbers alone, not with Duck Forge's own code.
fn outside(volume: &HitVolume, point: Vec3) -> f32 {
    match *volume {
        HitVolume::Ball { center, radius } => ((point - center).length() - radius).max(0.0),
        HitVolume::Box { center, axes, half } => {
            let from = point - center;
            let reach = [half.x, half.y, half.z];
            let mut squares = 0.0;
            for (axis, reach) in axes.iter().zip(reach) {
                let over = (from.dot(*axis).abs() - reach).max(0.0);
                squares += over * over;
            }
            f32::sqrt(squares)
        }
        HitVolume::Capsule { a, b, radius } => {
            // How far along the middle line the nearest point of it is,
            // as a share of its length, kept between its ends.
            let line = b - a;
            let share = ((point - a).dot(line) / line.dot(line)).clamp(0.0, 1.0);
            ((point - (a + line * share)).length() - radius).max(0.0)
        }
    }
}

/// The parts whose shapes hold `point`.
fn holding(shapes: &[HitShape], point: Vec3) -> Vec<&str> {
    shapes
        .iter()
        .filter(|shape| outside(&shape.volume, point) == 0.0)
        .map(|shape| shape.part.as_str())
        .collect()
}

fn middle(volume: &HitVolume) -> Vec3 {
    match *volume {
        HitVolume::Ball { center, .. } | HitVolume::Box { center, .. } => center,
        HitVolume::Capsule { a, b, .. } => (a + b) * 0.5,
    }
}

/// Fourteen ways to look at a part: from the six sides and the eight
/// corners.
fn directions() -> Vec<Vec3> {
    let mut all = vec![
        vec3(1.0, 0.0, 0.0),
        vec3(-1.0, 0.0, 0.0),
        vec3(0.0, 1.0, 0.0),
        vec3(0.0, -1.0, 0.0),
        vec3(0.0, 0.0, 1.0),
        vec3(0.0, 0.0, -1.0),
    ];
    for x in [-1.0, 1.0] {
        for y in [-1.0, 1.0] {
            for z in [-1.0, 1.0] {
                all.push(vec3(x, y, z).normalized());
            }
        }
    }
    all
}

/// Every moment a model is tested in: at rest, in each pose, and at
/// five moments of each animation. Each is a name and the model, posed.
fn moments(file: &Path) -> Result<Vec<(String, AnimatedModel)>, LoadError> {
    let mut all = vec![];
    let model = AnimatedModel::load(file)?;
    all.push(("rest".to_string(), model.clone()));
    for pose in model.poses() {
        let mut posed = model.clone();
        posed.set_blend(0.0);
        posed.set_pose(pose);
        all.push((format!("pose {pose}"), posed));
    }
    for (name, length) in model
        .model()
        .animations
        .iter()
        .map(|animation| (animation.name.clone(), animation.length))
    {
        for fifth in 0..5 {
            let mut playing = model.clone();
            playing.set_blend(0.0);
            playing.play(&name);
            playing.update(length * fifth as f32 / 4.5);
            all.push((format!("{name} at {fifth}/4.5"), playing));
        }
    }
    Ok(all)
}

#[test]
fn every_hit_shape_is_where_duck_mold_said() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-hit-shapes");
    for name in WITH_HITS {
        let model = AnimatedModel::load(models.join(format!("{name}.glb")))?;
        let report = json(&models, &format!("{name}.colliders.json"));
        let listed = report["colliders"].as_array().unwrap();
        let shapes = model.hit_shapes(&Transform::default());
        assert_eq!(shapes.len(), listed.len(), "{name}");
        assert_eq!(
            shapes.len() as u64,
            report["hit_shapes"]["count"].as_u64().unwrap(),
            "{name}"
        );
        for (shape, expected) in shapes.iter().zip(listed) {
            let part = expected["part"].as_str().unwrap();
            assert_eq!(shape.part, part, "{name}");
            // The file keeps 32-bit numbers: they agree to about 0.00001.
            let near = |found: Vec3, want: Vec3| {
                assert!(
                    (found - want).length() < 2e-5,
                    "{name} {part}: {found:?} {want:?}"
                );
            };
            let close = |found: f32, want: &serde_json::Value| {
                let want = want.as_f64().unwrap() as f32;
                assert!((found - want).abs() < 2e-5, "{name} {part}: {found} {want}");
            };
            match (shape.volume, expected["shape"].as_str().unwrap()) {
                (HitVolume::Box { center, axes, half }, "box") => {
                    near(center, three(&expected["center"]));
                    near(half * 2.0, three(&expected["size"]));
                    for (axis, want) in axes.iter().zip(expected["directions"].as_array().unwrap())
                    {
                        near(*axis, three(want));
                    }
                }
                (HitVolume::Ball { center, radius }, "sphere") => {
                    near(center, three(&expected["center"]));
                    close(radius, &expected["radius"]);
                }
                (HitVolume::Capsule { a, b, radius }, "capsule") => {
                    near(a, three(&expected["ends"][0]));
                    near(b, three(&expected["ends"][1]));
                    close(radius, &expected["radius"]);
                    close((b - a).length() + 2.0 * radius, &expected["length"]);
                }
                (found, want) => panic!("{name} {part}: {found:?} is not a {want}"),
            }
        }
        // A part that said it has none has none.
        let none = report["hit_shapes"]["none_by_their_own"].as_array();
        for part in none.into_iter().flatten() {
            let part = part.as_str().unwrap();
            assert!(
                shapes.iter().all(|shape| shape.part != part),
                "{name} {part}"
            );
        }
    }
    // The robot's head is a ball; its body a box; its eyes have none.
    let robot = AnimatedModel::load(models.join("robot.glb"))?;
    let shapes = robot.hit_shapes(&Transform::default());
    let kind = |part: &str| {
        shapes
            .iter()
            .find(|shape| shape.part == part)
            .map(|s| s.volume)
    };
    assert!(matches!(kind("head"), Some(HitVolume::Ball { .. })));
    assert!(matches!(kind("body"), Some(HitVolume::Box { .. })));
    assert!(matches!(kind("antenna"), Some(HitVolume::Capsule { .. })));
    assert_eq!(kind("left_eye"), None);
    Ok(())
}

#[test]
fn a_model_without_hit_shapes_is_never_hit() -> Result<(), LoadError> {
    let (folder, models) = fixtures("duck-mold-no-hits");
    let old = folder.fixture("duck_mold_before_tree");
    for file in [
        models.join("crate.glb"),
        models.join("house.glb"),
        old.join("robot.glb"),
    ] {
        let model = AnimatedModel::load(&file)?;
        let place = Transform::default();
        assert!(model.hit_shapes(&place).is_empty(), "{file:?}");
        let ray = Ray::new(vec3(0.0, 0.5, 5.0), vec3(0.0, 0.0, -1.0));
        assert_eq!(model.hit_ray(ray, &place), None);
        assert!(model.hits_ball(Vec3::ZERO, 100.0, &place).is_empty());
    }
    Ok(())
}

#[test]
fn every_shape_goes_with_its_part_in_every_pose() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-hits-follow");
    let mut checked = 0;
    for name in WITH_HITS {
        for (moment, model) in moments(&models.join(format!("{name}.glb")))? {
            let mesh = model.mesh();
            for shape in model.hit_shapes(&Transform::default()) {
                let part = model.model().part(&shape.part).unwrap();
                // A capsule is as long as its part: the rim of a
                // cylinder's ends pokes out of its rounded ends by up to
                // (√2 − 1) of its radius.
                let slack = match shape.volume {
                    HitVolume::Capsule { radius, .. } => radius * 0.42 + 1e-4,
                    _ => 1e-4,
                };
                for triangle in &mesh.triangles[part.range()] {
                    for corner in triangle.corners {
                        let point = mesh.vertices[corner];
                        let off = outside(&shape.volume, point);
                        assert!(
                            off <= slack,
                            "{name} {moment}: {} pokes {off} out of its hit shape at {point:?}",
                            shape.part
                        );
                        checked += 1;
                    }
                }
            }
        }
    }
    assert!(checked > 50_000, "{checked}");
    Ok(())
}

#[test]
fn a_ray_finds_the_first_part_in_its_way_in_every_pose() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-rays");
    let mut rays = 0;
    for name in WITH_HITS {
        for (moment, model) in moments(&models.join(format!("{name}.glb")))? {
            let place = Transform::default();
            let shapes = model.hit_shapes(&place);
            for aimed in &shapes {
                let target = middle(&aimed.volume);
                let mut named = 0;
                for direction in directions() {
                    // From 3 metres away, straight at the part's middle.
                    let ray = Ray::new(target + direction * 3.0, -direction);
                    let hit = model
                        .hit_ray(ray, &place)
                        .unwrap_or_else(|| panic!("{name} {moment}: missed {}", aimed.part));
                    rays += 1;
                    // The point is on the part it names...
                    let shape = shapes.iter().find(|s| s.part == hit.part).unwrap();
                    assert!(outside(&shape.volume, hit.point) < 1e-4, "{name} {moment}");
                    assert!((ray.at(hit.distance) - hit.point).length() < 1e-5);
                    assert!((hit.normal.length() - 1.0).abs() < 1e-4);
                    assert!(hit.normal.dot(ray.direction) <= 1e-4, "it faces the ray");
                    // ...and walking along the ray, nothing was there
                    // before it. Each step goes exactly as far as the
                    // nearest shape is, so it can't jump over one.
                    let mut walked = 0.0;
                    // Where it only touched a shape's edge or corner and
                    // went on outside it: a hit there or none are both
                    // fair.
                    let mut grazed = vec![];
                    loop {
                        let point = ray.at(walked);
                        let gap = shapes
                            .iter()
                            .map(|shape| outside(&shape.volume, point))
                            .fold(f32::INFINITY, f32::min);
                        if gap < 1e-6 {
                            if holding(&shapes, ray.at(walked + 1e-4)).is_empty() {
                                grazed.push(walked);
                                walked += 1e-4;
                                continue;
                            }
                            break;
                        }
                        walked += gap;
                        assert!(walked < 10.0, "{name} {moment}: the walk found nothing");
                    }
                    let near = |at: f32| (at - hit.distance).abs() < 1e-3;
                    assert!(
                        near(walked) || grazed.iter().any(|&at| near(at)),
                        "{name} {moment}: aiming at {} from {direction:?}, the walk met {:?} at \
                         {walked}, not {} at {}",
                        aimed.part,
                        holding(&shapes, ray.at(walked + 1e-4)),
                        hit.part,
                        hit.distance
                    );
                    if hit.part == aimed.part {
                        named += 1;
                    }
                }
                // Something else can be in the way from some sides (an
                // arm before the body), but never from all fourteen.
                assert!(
                    named > 0,
                    "{name} {moment}: {} is always hidden",
                    aimed.part
                );
            }
        }
    }
    assert!(rays > 10_000, "{rays}");
    Ok(())
}

#[test]
fn a_ray_aimed_past_the_model_misses_it() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-misses");
    let robot = AnimatedModel::load(models.join("robot.glb"))?;
    let place = Transform::default();
    // Over its head, beside it, and away from it.
    for ray in [
        Ray::new(vec3(0.0, 2.5, 5.0), vec3(0.0, 0.0, -1.0)),
        Ray::new(vec3(1.0, 1.0, 5.0), vec3(0.0, 0.0, -1.0)),
        Ray::new(vec3(0.0, 1.0, 5.0), vec3(0.0, 0.0, 1.0)),
    ] {
        assert_eq!(robot.hit_ray(ray, &place), None, "{ray:?}");
    }
    // From in front, at the height of its head: the head.
    let ray = Ray::new(vec3(0.0, 1.39, 5.0), vec3(0.0, 0.0, -1.0));
    let hit = robot.hit_ray(ray, &place).unwrap();
    assert_eq!(hit.part, "head");
    assert!((hit.normal - vec3(0.0, 0.0, 1.0)).length() < 1e-4);
    Ok(())
}

#[test]
fn the_shapes_go_where_the_model_is_drawn() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-placed");
    let robot = AnimatedModel::load(models.join("robot.glb"))?;
    let here = Transform::default();
    // Moved, turned a quarter round and twice as big.
    let there = Transform::at(vec3(4.0, 1.0, -3.0))
        .rotated(vec3(0.0, PI / 2.0, 0.0))
        .sized(2.0);
    let at_home = robot.hit_shapes(&here);
    let placed = robot.hit_shapes(&there);
    for (home, away) in at_home.iter().zip(&placed) {
        assert_eq!(home.part, away.part);
        assert!((there.apply(middle(&home.volume)) - middle(&away.volume)).length() < 1e-4);
    }
    // A ray at where the head is drawn finds the head; its point is the
    // one found at home, placed the same way.
    let head = middle(&placed.iter().find(|s| s.part == "head").unwrap().volume);
    let ray = Ray::new(head + vec3(6.0, 0.0, 0.0), vec3(-1.0, 0.0, 0.0));
    let hit = robot.hit_ray(ray, &there).unwrap();
    assert_eq!(hit.part, "head");
    // Turned a quarter round, the robot's front (+z) faces +x; its head's
    // radius is twice as big.
    let radius = match placed.iter().find(|s| s.part == "head").unwrap().volume {
        HitVolume::Ball { radius, .. } => radius,
        other => panic!("{other:?}"),
    };
    assert!((hit.distance - (6.0 - radius)).abs() < 1e-4);
    assert!((hit.point.x - (head.x + radius)).abs() < 1e-4);
    Ok(())
}

#[test]
fn a_segment_stops_where_it_ends() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-segments");
    let robot = AnimatedModel::load(models.join("robot.glb"))?;
    let place = Transform::default();
    let from = vec3(0.0, 1.39, 2.0);
    // Long enough: the same as the ray.
    let to = vec3(0.0, 1.39, -2.0);
    let hit = robot.hit_segment(from, to, &place).unwrap();
    let by_ray = robot.hit_ray(Ray::new(from, to - from), &place).unwrap();
    assert_eq!(hit, by_ray);
    // Stopping short of the head: nothing.
    assert_eq!(robot.hit_segment(from, vec3(0.0, 1.39, 0.5), &place), None);
    // No length at all: whatever is at that point.
    let inside = robot.hit_segment(vec3(0.0, 1.39, 0.0), vec3(0.0, 1.39, 0.0), &place);
    assert_eq!(inside.unwrap().part, "head");
    Ok(())
}

#[test]
fn a_ball_finds_every_part_it_touches_nearest_first() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-balls");
    let mut robot = AnimatedModel::load(models.join("robot.glb"))?;
    robot.set_blend(0.0);
    let place = Transform::default();
    // Far away: nothing.
    assert!(robot.hits_ball(vec3(0.0, 1.0, 5.0), 0.5, &place).is_empty());
    // Big enough for the whole robot: every part with a shape, nearest
    // first, the ones it holds the middle of at 0.
    let all = robot.hits_ball(vec3(0.0, 0.9, 0.0), 3.0, &place);
    assert_eq!(all.len(), robot.hit_shapes(&place).len());
    assert!(all.windows(2).all(|two| two[0].distance <= two[1].distance));
    assert_eq!(all[0].part, "body");
    assert_eq!(all[0].distance, 0.0);
    // Just under the hand, waving: the hand. Each point is on the part's
    // shape, the nearest point of it to the ball's middle.
    robot.set_pose("wave");
    let shapes = robot.hit_shapes(&place);
    let hand = shapes.iter().find(|s| s.part == "left_arm/hand").unwrap();
    let under = middle(&hand.volume) + vec3(0.0, -0.2, 0.0);
    let hits = robot.hits_ball(under, 0.2, &place);
    assert!(
        hits.iter().any(|hit| hit.part == "left_arm/hand"),
        "{hits:?}"
    );
    for hit in &hits {
        let shape = shapes.iter().find(|s| s.part == hit.part).unwrap();
        assert!(outside(&shape.volume, hit.point) < 1e-4);
        assert!(((hit.point - under).length() - hit.distance).abs() < 1e-5);
        assert!(hit.distance <= 0.2);
    }
    // The same ball at rest, the arm down: not the hand.
    robot.rest();
    let hits = robot.hits_ball(under, 0.2, &place);
    assert!(
        hits.iter().all(|hit| hit.part != "left_arm/hand"),
        "{hits:?}"
    );
    Ok(())
}

#[test]
fn a_click_finds_the_part_drawn_under_the_mouse() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-clicks");
    let canvas = Canvas::new(320, 240);
    // The camera's ray goes back through every pixel exactly: turned,
    // tipped and zoomed.
    let mut camera = Camera3D::looking_at(vec3(1.5, 2.0, 4.0), vec3(0.0, 1.0, 0.0));
    camera.fov = 0.9;
    for (x, y) in [(0.0, 0.0), (160.0, 120.0), (319.0, 10.0), (37.5, 201.25)] {
        let ray = camera.ray_through(vec2(x, y), &canvas);
        for distance in [1.0, 4.0, 20.0] {
            let back = camera.world_to_screen(ray.at(distance), &canvas).unwrap();
            assert!((back - vec2(x, y)).length() < 1e-2, "{x} {y}: {back:?}");
        }
    }
    // Every part of the walking robot: a click on where its middle is
    // drawn names it, or a part drawn in front of it.
    let mut robot = AnimatedModel::load(models.join("robot.glb"))?;
    robot.set_blend(0.0);
    robot.play("walk");
    robot.update(0.3);
    let place = Transform::default();
    let camera = Camera3D::looking_at(vec3(0.0, 1.2, 4.0), vec3(0.0, 1.0, 0.0));
    let mut named = 0;
    let shapes = robot.hit_shapes(&place);
    for shape in &shapes {
        let middle = middle(&shape.volume);
        let pixel = camera.world_to_screen(middle, &canvas).unwrap();
        let hit = robot
            .hit_ray(camera.ray_through(pixel, &canvas), &place)
            .unwrap();
        if hit.part == shape.part {
            named += 1;
        } else {
            // What it found is nearer the camera.
            assert!(hit.distance < (middle - camera.position).length());
        }
    }
    assert!(named >= shapes.len() / 2, "{named} of {}", shapes.len());
    // The head, in the middle of the picture's top half.
    let head = camera
        .world_to_screen(vec3(0.0, 1.39, 0.0), &canvas)
        .unwrap();
    let hit = robot
        .hit_ray(camera.ray_through(head, &canvas), &place)
        .unwrap();
    assert_eq!(hit.part, "head");
    Ok(())
}
