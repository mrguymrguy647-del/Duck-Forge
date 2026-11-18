//! Far-away versions of a Duck Mold model (`lods` in its recipe): read
//! from the file, picked by distance, posed with the model, and drawn.

use std::path::PathBuf;

mod common;
use common::TestFolder;

use duckforge::LoadError;
use duckforge::glb::{AnimatedModel, GlbModel};
use duckforge::gltf::GltfModel;
use duckforge::prelude::*;

fn fixtures(test: &str) -> (TestFolder, PathBuf) {
    let folder = TestFolder::new(test);
    let models = folder.fixture("duck_mold");
    (folder, models)
}

/// What Duck Mold's `inspect` said of the robot's far versions:
/// (distance, triangles) for each.
fn reported(models: &std::path::Path) -> Vec<(f64, usize)> {
    let text = std::fs::read_to_string(models.join("robot.inspect.json")).unwrap();
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    report["far_versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|far| {
            (
                far["distance"].as_f64().unwrap(),
                far["triangles"].as_u64().unwrap() as usize,
            )
        })
        .collect()
}

#[test]
fn the_robots_far_versions_are_read_as_duck_mold_reported_them() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("far_read");
    let robot = GlbModel::load(models.join("robot.glb"))?;
    let wanted = reported(&models);
    assert_eq!(wanted.len(), 2);
    assert_eq!(robot.far.len(), 2);
    for (far, (distance, triangles)) in robot.far.iter().zip(&wanted) {
        assert_eq!(far.distance, *distance as f32);
        assert_eq!(far.model.triangles.len(), *triangles);
        // The same parts, joins and hit shapes: only triangles change.
        assert_eq!(far.model.joins.len(), robot.joins.len());
        assert_eq!(far.model.parts.len(), robot.parts.len());
        assert_eq!(far.model.colliders, robot.colliders);
        assert!(far.model.far.is_empty());
    }
    assert!(robot.far[0].model.triangles.len() < robot.triangles.len());
    // A model without them has none.
    assert!(GlbModel::load(models.join("house.glb"))?.far.is_empty());
    // A general glTF reader draws only the model itself: the far
    // versions are outside its scene.
    let general = GltfModel::load(models.join("robot.glb"))?;
    let general: usize = general
        .meshes()
        .iter()
        .map(|mesh| mesh.triangles.len())
        .sum();
    assert_eq!(general, robot.triangles.len());
    Ok(())
}

#[test]
fn the_version_drawn_goes_by_distance_and_scale() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("far_pick");
    let robot = AnimatedModel::load(models.join("robot.glb"))?;
    assert_eq!(robot.far_versions(), 2);
    let place = Transform::at(vec3(3.0, 0.0, -2.0));
    let camera_at = |distance: f32| {
        Camera3D::looking_at(place.position + vec3(0.0, 0.0, distance), place.position)
    };
    // Far 1 from 14.6 m, Far 2 from 36.6 m.
    for (distance, version) in [
        (5.0, 0),
        (14.5, 0),
        (14.7, 1),
        (30.0, 1),
        (36.7, 2),
        (200.0, 2),
    ] {
        assert_eq!(
            robot.version_at(&place, &camera_at(distance)),
            version,
            "at {distance} m"
        );
    }
    // Twice as big, it changes twice as far off.
    let big = place.scaled(vec3(2.0, 2.0, 2.0));
    assert_eq!(robot.version_at(&big, &camera_at(20.0)), 0);
    assert_eq!(robot.version_at(&big, &camera_at(40.0)), 1);
    // The mesh for each has that version's triangles.
    let wanted = reported(&models);
    assert_eq!(robot.mesh_for(&place, &camera_at(5.0)).triangles.len(), 384);
    assert_eq!(
        robot.mesh_for(&place, &camera_at(20.0)).triangles.len(),
        wanted[0].1
    );
    assert_eq!(
        robot.mesh_for(&place, &camera_at(50.0)).triangles.len(),
        wanted[1].1
    );
    // A model without far versions is always itself.
    let house = AnimatedModel::load(models.join("house.glb"))?;
    assert_eq!(house.far_versions(), 0);
    assert_eq!(house.version_at(&place, &camera_at(1000.0)), 0);
    Ok(())
}

#[test]
fn a_far_version_stands_as_the_model_does() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("far_pose");
    let mut robot = AnimatedModel::load(models.join("robot.glb"))?;
    let place = Transform::default();
    let far = Camera3D::looking_at(vec3(0.0, 1.0, 50.0), Vec3::ZERO);
    let height = |mesh: &Mesh| mesh.bounds().1.y;
    let at_rest = height(robot.mesh_for(&place, &far));
    // Arm up over the head: the far version's top rises with the
    // model's, to within a few millimetres (its elbow is a rounder
    // sphere with fewer sides).
    robot.set_blend(0.0);
    robot.set_pose("wave");
    robot.update(0.0);
    let model = height(robot.mesh());
    let version = height(robot.mesh_for(&place, &far));
    assert!(version > at_rest + 0.05, "{version} {at_rest}");
    assert!((version - model).abs() < 0.01, "{version} {model}");
    // Walking, frame by frame, its feet stay where the model's are.
    robot.play("walk");
    for _ in 0..20 {
        robot.update(1.0 / 30.0);
        let (low, _) = robot.mesh().bounds();
        let (far_low, _) = robot.mesh_for(&place, &far).bounds();
        assert!((low.y - far_low.y).abs() < 0.01, "{} {}", low.y, far_low.y);
    }
    Ok(())
}

#[test]
fn draw_mesh_draws_what_mesh_for_gives() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("far_draw");
    let mut robot = AnimatedModel::load(models.join("robot.glb"))?;
    robot.play("walk");
    robot.update(0.3);
    let place = Transform::at(vec3(0.0, 0.0, 0.0));
    for distance in [4.0, 20.0, 45.0] {
        let camera = Camera3D::looking_at(vec3(0.0, 1.0, distance), vec3(0.0, 0.9, 0.0));
        let mut drawn = Canvas::new(96, 96);
        robot.draw_mesh(&mut drawn, &place, &camera);
        let mut wanted = Canvas::new(96, 96);
        wanted.draw_mesh(robot.mesh_for(&place, &camera), &place, &camera);
        assert!(drawn.pixels() == wanted.pixels(), "at {distance} m");
    }
    // After the mesh is changed by hand, it is still drawn whole.
    robot.mesh_mut().vertices[0] += vec3(0.0, 5.0, 0.0);
    let camera = Camera3D::looking_at(vec3(0.0, 1.0, 4.0), vec3(0.0, 0.9, 0.0));
    let mut drawn = Canvas::new(96, 96);
    robot.draw_mesh(&mut drawn, &place, &camera);
    let mut wanted = Canvas::new(96, 96);
    wanted.draw_mesh(robot.mesh(), &place, &camera);
    assert!(drawn.pixels() == wanted.pixels());
    Ok(())
}
