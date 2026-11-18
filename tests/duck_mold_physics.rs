//! A body for Duck Forge's physics from a Duck Mold model's hit shapes:
//! a block round them as the model stands at rest.

use std::path::PathBuf;

mod common;
use common::TestFolder;

use duckforge::LoadError;
use duckforge::glb::GlbModel;
use duckforge::physics::{Body, PhysicsWorld, Shape};
use duckforge::prelude::*;

fn fixtures(test: &str) -> (TestFolder, PathBuf) {
    let folder = TestFolder::new(test);
    let models = folder.fixture("duck_mold");
    (folder, models)
}

fn half_size(body: &Body<Vec3>) -> Vec3 {
    match body.shape {
        Shape::Block { half_size } => half_size,
        Shape::Ball { .. } => panic!("a block"),
    }
}

#[test]
fn the_body_is_a_block_round_the_hit_shapes_standing_on_the_feet() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("physics_body");
    let robot = GlbModel::load(models.join("robot.glb"))?;
    let (low, high) = robot.hit_box().expect("the robot has hit shapes");
    let feet = vec3(1.0, 0.0, 3.0);
    let body = robot.physics_body(feet);
    let half = half_size(&body);
    assert!(((high - low) * 0.5 - half).length() < 1e-5);
    // Its bottom stands on the feet.
    assert!((body.position.y - half.y - feet.y).abs() < 1e-5);
    assert!((body.position.x - feet.x).abs() < 1e-5 && (body.position.z - feet.z).abs() < 1e-5);
    // The antenna's tip has no hit shape: the body is lower than the model.
    let tallest = robot
        .vertices
        .iter()
        .map(|corner| corner[1])
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(high.y < tallest, "{} < {tallest}", high.y);
    // Drawn where stands_at says, the hit shapes' box sits in the block.
    let at = robot.stands_at(&body);
    let drawn_low = low + at;
    let drawn_high = high + at;
    let block_low = body.position - half;
    let block_high = body.position + half;
    assert!(
        (drawn_low - block_low).length() < 1e-4,
        "{drawn_low:?} {block_low:?}"
    );
    assert!((drawn_high - block_high).length() < 1e-4);
    Ok(())
}

#[test]
fn a_model_without_hit_shapes_uses_its_own_box() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("physics_body_plain");
    let model = GlbModel::load(models.join("house.glb"))?;
    assert!(model.colliders.is_empty());
    let (low, high) = model.hit_box().unwrap();
    for corner in &model.vertices {
        assert!(corner[1] >= low.y - 1e-5 && corner[1] <= high.y + 1e-5);
    }
    let body = model.physics_body(Vec3::ZERO);
    assert!(((high - low) * 0.5 - half_size(&body)).length() < 1e-5);
    Ok(())
}

#[test]
fn the_robot_stands_on_a_floor_and_stops_at_a_wall() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("physics_body_world");
    let robot = GlbModel::load(models.join("robot.glb"))?;
    let mut world = PhysicsWorld::new(vec3(0.0, -9.8, 0.0));
    world.add(Body::block(vec3(0.0, -0.5, 0.0), vec3(20.0, 1.0, 20.0)).fixed());
    world.add(Body::block(vec3(2.0, 1.0, 0.0), vec3(0.2, 2.0, 4.0)).fixed());
    let me = world.add(robot.physics_body(vec3(0.0, 0.0, 0.0)).with_friction(0.0));
    // Walking towards the wall for two seconds.
    for _ in 0..120 {
        let body = world.get_mut(me).unwrap();
        body.velocity.x = 1.0;
        world.step(1.0 / 60.0);
    }
    let body = world.get(me).unwrap();
    let half = half_size(body);
    // Stopped by the wall, its side against it; standing on the floor.
    assert!(body.position.x + half.x < 1.9 + 0.02, "{}", body.position.x);
    assert!(body.position.x + half.x > 1.9 - 0.05, "{}", body.position.x);
    let feet = robot.stands_at(body);
    assert!(feet.y.abs() < 0.02, "{feet:?}");
    Ok(())
}
