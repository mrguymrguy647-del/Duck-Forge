//! Models with images (Duck Mold's Phase 7b, reader version 9).
//!
//! A Duck Mold recipe can put a picture on a part, or on one face of it.
//! The `.glb` then holds all of the model's images on one picture, and
//! says where each triangle is on it. `plank_crate.glb` in
//! `tests/fixtures/duck_mold/` is Duck Mold's own golden file: a crate
//! with planks on its four sides and a lid on its top and bottom, painted
//! with Duck Quill, 32 pixels to the metre.
//!
//! These tests load it and check that it comes with its picture, that
//! the picture is drawn the right way round, and that its pixels stay
//! sharp, near and far.

use std::path::{Path, PathBuf};

mod common;
use common::TestFolder;

use duckforge::LoadError;
use duckforge::glb::{AnimatedModel, GlbModel};
use duckforge::prelude::*;

/// A private copy of the Duck Mold fixtures, for one test.
fn fixtures(test: &str) -> (TestFolder, PathBuf) {
    let folder = TestFolder::new(test);
    let models = folder.fixture("duck_mold");
    (folder, models)
}

/// The colours the planks are painted in (`examples/textures/planks.json`
/// in Duck Mold): the wood, the lighter streaks, the gaps between the
/// planks, and the nails.
const WOOD: Color = Color::rgb(0xB0, 0x7A, 0x3F);
const LIGHT: Color = Color::rgb(0xC4, 0x91, 0x5A);
const GAP: Color = Color::rgb(0x7A, 0x4E, 0x24);
const NAIL: Color = Color::rgb(0x4A, 0x30, 0x18);
/// The lid's frame.
const FRAME: Color = Color::rgb(0x8A, 0x5A, 0x2B);

/// Draws a mesh standing at the origin, seen from `eye`, glowing (so
/// every pixel is exactly its picture's colour, whatever the light).
fn picture(mesh: &Mesh, eye: Vec3, size: (usize, usize)) -> Canvas {
    let mut canvas = Canvas::new(size.0, size.1);
    canvas.clear(Color::BLACK);
    let camera = Camera3D::looking_at(eye, vec3(0.0, 0.5, 0.0));
    canvas.draw_mesh(&mesh.clone().glowing(), &Transform::at(Vec3::ZERO), &camera);
    canvas
}

/// Every colour on a picture but the black behind, with how many pixels
/// have it.
fn colours(canvas: &Canvas) -> Vec<(Color, usize)> {
    let mut seen: Vec<(Color, usize)> = Vec::new();
    for y in 0..canvas.height() as i32 {
        for x in 0..canvas.width() as i32 {
            let Some(color) = canvas.get_pixel(x, y) else {
                continue;
            };
            if color == Color::BLACK {
                continue;
            }
            match seen.iter_mut().find(|(known, _)| *known == color) {
                Some((_, count)) => *count += 1,
                None => seen.push((color, 1)),
            }
        }
    }
    seen
}

/// The box round everything that isn't black: left, top, right, bottom.
fn covered(canvas: &Canvas) -> [i32; 4] {
    let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, 0, 0);
    for y in 0..canvas.height() as i32 {
        for x in 0..canvas.width() as i32 {
            if canvas.get_pixel(x, y) != Some(Color::BLACK) {
                left = left.min(x);
                top = top.min(y);
                right = right.max(x);
                bottom = bottom.max(y);
            }
        }
    }
    [left, top, right, bottom]
}

/// The stored picture `tests/fixtures/duck_mold/pictures/<name>`: checks
/// that `canvas` is that picture, pixel for pixel. Run the tests with
/// `DUCKFORGE_ACCEPT_PICTURES=1` to store what is drawn now instead.
fn check_picture(folder: &Path, name: &str, canvas: &Canvas) {
    let stored = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/duck_mold/pictures")
        .join(name);
    let drawn = folder.join(name);
    canvas.save_png(&drawn).unwrap();
    let drawn = std::fs::read(drawn).unwrap();
    if std::env::var_os("DUCKFORGE_ACCEPT_PICTURES").is_some() {
        std::fs::write(&stored, drawn).unwrap();
        return;
    }
    let wanted = std::fs::read(&stored).unwrap_or_else(|_| {
        panic!(
            "{} is missing: store it with DUCKFORGE_ACCEPT_PICTURES=1 cargo test --test duck_mold_images",
            stored.display()
        )
    });
    assert!(
        drawn == wanted,
        "{name} is no longer the stored picture. If the change is meant, store the new one with DUCKFORGE_ACCEPT_PICTURES=1 cargo test --test duck_mold_images"
    );
}

#[test]
fn a_model_with_images_loads_with_its_picture() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-images-load");
    let model = GlbModel::load(models.join("plank_crate.glb"))?;
    let report: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(models.join("plank_crate.inspect.json")).unwrap(),
    )
    .unwrap();

    // The picture is the size Duck Mold's report says its images are
    // together, and it is a PNG that reads.
    let picture = model.picture.as_ref().expect("a picture");
    assert_eq!(
        serde_json::json!([picture.width, picture.height]),
        report["images"]["sheet"]
    );
    let sprite = model.picture_sprite().expect("a picture that reads");
    assert_eq!(
        (sprite.width() as u32, sprite.height() as u32),
        (picture.width, picture.height)
    );

    // Every one of the crate's 12 triangles has an image, and is white
    // under it.
    assert_eq!(model.triangles.len(), 12);
    for triangle in &model.triangles {
        let on = triangle.on_picture.expect("a place on the picture");
        assert_eq!(triangle.color, [255, 255, 255]);
        for [across, down] in on {
            assert!((0.0..=1.0).contains(&across) && (0.0..=1.0).contains(&down));
        }
    }

    // The mesh comes with them.
    let mesh = model.to_mesh();
    assert!(mesh.texture.is_some());
    assert_eq!(mesh.triangles.len(), 12);
    assert!((0..12).all(|triangle| mesh.uvs.get(triangle).is_some_and(Option::is_some)));
    assert!(
        mesh.triangles
            .iter()
            .all(|triangle| triangle.color == Color::WHITE)
    );
    // `Mesh::load_glb` is the same mesh.
    let loaded = Mesh::load_glb(models.join("plank_crate.glb"))?;
    assert_eq!(loaded.uvs, mesh.uvs);
    assert!(loaded.texture.is_some());
    Ok(())
}

#[test]
fn a_model_without_images_has_no_picture() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-images-none");
    for name in ["crate", "robot"] {
        let model = GlbModel::load(models.join(format!("{name}.glb")))?;
        assert!(model.picture.is_none(), "{name}");
        assert!(model.picture_sprite().is_none(), "{name}");
        assert!(
            model
                .triangles
                .iter()
                .all(|triangle| triangle.on_picture.is_none())
        );
        let mesh = model.to_mesh();
        assert!(mesh.texture.is_none(), "{name}");
        assert!(mesh.uvs.iter().all(Option::is_none), "{name}");
    }
    Ok(())
}

#[test]
fn the_planks_are_upright_on_the_sides_and_the_lid_is_on_top() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-images-sides");
    let mesh = Mesh::load_glb(models.join("plank_crate.glb"))?;

    // Each of the four sides, seen straight on: only the planks' four
    // colours. The gaps between the planks run across (each row of the
    // picture that has the gap's colour has it nearly all the way), and
    // the lowest row of the crate is a gap: the picture is upright.
    for (side, eye) in [
        ("front", vec3(0.0, 0.5, 6.0)),
        ("back", vec3(0.0, 0.5, -6.0)),
        ("right", vec3(6.0, 0.5, 0.0)),
        ("left", vec3(-6.0, 0.5, 0.0)),
    ] {
        let canvas = picture(&mesh, eye, (240, 240));
        let mut seen: Vec<Color> = colours(&canvas)
            .into_iter()
            .map(|(color, _)| color)
            .collect();
        seen.sort_by_key(|color| color.to_u32());
        let mut planks = vec![WOOD, LIGHT, GAP, NAIL];
        planks.sort_by_key(|color| color.to_u32());
        assert_eq!(seen, planks, "{side}");

        let [left, top, right, bottom] = covered(&canvas);
        let wide = right - left + 1;
        let gap_rows: Vec<i32> = (top..=bottom)
            .filter(|&y| {
                let gaps = (left..=right)
                    .filter(|&x| canvas.get_pixel(x, y) == Some(GAP))
                    .count() as i32;
                gaps * 10 >= wide * 9
            })
            .collect();
        // Four gaps, each a 32nd of the crate's height.
        assert!(
            gap_rows.contains(&bottom),
            "{side}: the lowest row is a gap"
        );
        assert!(!gap_rows.contains(&top), "{side}: the top row is wood");
        let tall = bottom - top + 1;
        let expected = tall * 4 / 32;
        assert!(
            (gap_rows.len() as i32 - expected).abs() <= 4,
            "{side}: {} rows of gap, about {expected} expected",
            gap_rows.len()
        );
    }

    // From straight above: the lid, with its darker frame, and none of
    // the planks' nails' rows.
    let above = picture(&mesh, vec3(0.0, 8.0, 0.01), (240, 240));
    let seen: Vec<Color> = colours(&above)
        .into_iter()
        .map(|(color, _)| color)
        .collect();
    assert!(seen.contains(&FRAME), "{seen:?}");
    let [left, top, right, bottom] = covered(&above);
    // The frame runs round the edge: the corners of the lid are frame.
    for (x, y) in [
        (left + 2, top + 2),
        (right - 2, top + 2),
        (left + 2, bottom - 2),
        (right - 2, bottom - 2),
    ] {
        let at = above.get_pixel(x, y).unwrap();
        assert!(at == FRAME || at == NAIL, "{x},{y}: {at:?}");
    }
    Ok(())
}

#[test]
fn an_images_pixels_stay_sharp_near_and_far() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-images-sharp");
    let mesh = Mesh::load_glb(models.join("plank_crate.glb"))?;
    // Near (the crate fills the picture), further off, and far (the
    // crate is a few pixels): never a colour that isn't one of the four
    // the planks were painted with. A blur would make in-between ones.
    for distance in [1.6, 4.0, 12.0, 40.0] {
        let canvas = picture(&mesh, vec3(0.0, 0.5, distance), (320, 240));
        let seen = colours(&canvas);
        assert!(!seen.is_empty(), "{distance}");
        for (color, count) in &seen {
            assert!(
                [WOOD, LIGHT, GAP, NAIL].contains(color),
                "at {distance} m: {count} pixels of {color:?}, which nobody painted"
            );
        }
    }
    // Near, one pixel of the image is many pixels of the screen, and the
    // edge between a plank and a gap is one clean step: straight down
    // the middle of the crate, the colour only ever changes between
    // the planks' colours.
    let canvas = picture(&mesh, vec3(0.0, 0.5, 1.6), (320, 240));
    let column: Vec<Color> = (0..240).filter_map(|y| canvas.get_pixel(160, y)).collect();
    assert!(column.iter().filter(|color| **color == GAP).count() > 10);
    assert!(column.iter().filter(|color| **color == WOOD).count() > 100);
    Ok(())
}

#[test]
fn the_crate_draws_its_stored_pictures() -> Result<(), LoadError> {
    let (folder, models) = fixtures("duck-mold-images-pictures");
    let mesh = Mesh::load_glb(models.join("plank_crate.glb"))?;
    // Lit as a game lights it, from the front and from above a corner.
    let draw = |eye: Vec3| {
        let mut canvas = Canvas::new(200, 200);
        canvas.clear(Color::rgb(0x2a, 0x2f, 0x3a));
        let camera = Camera3D::looking_at(eye, vec3(0.0, 0.5, 0.0));
        canvas.draw_mesh(&mesh, &Transform::at(Vec3::ZERO), &camera);
        canvas
    };
    check_picture(
        &folder.join(""),
        "plank_crate.front.png",
        &draw(vec3(0.0, 0.5, 2.6)),
    );
    check_picture(
        &folder.join(""),
        "plank_crate.corner.png",
        &draw(vec3(1.8, 1.9, 1.8)),
    );
    Ok(())
}

#[test]
fn an_animated_model_keeps_its_images() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-images-animated");
    let mut crate_ = AnimatedModel::load(models.join("plank_crate.glb"))?;
    crate_.update(0.1);
    assert!(crate_.mesh().texture.is_some());
    assert!(crate_.mesh().uvs.iter().all(Option::is_some));
    // Drawn through the model, it is the same picture as its mesh.
    let camera = Camera3D::looking_at(vec3(0.0, 0.5, 3.0), vec3(0.0, 0.5, 0.0));
    let place = Transform::at(Vec3::ZERO);
    let mut by_model = Canvas::new(120, 120);
    crate_.draw_mesh(&mut by_model, &place, &camera);
    let mut by_mesh = Canvas::new(120, 120);
    by_mesh.draw_mesh(crate_.mesh(), &place, &camera);
    assert!(by_model.pixels() == by_mesh.pixels());
    assert!(colours(&by_model).len() >= 3);
    Ok(())
}

#[test]
fn a_picture_that_cannot_be_read_is_one_sentence() {
    let (folder, models) = fixtures("duck-mold-images-damaged");
    let mut bytes = std::fs::read(models.join("plank_crate.glb")).unwrap();
    // Spoil the PNG inside the file: the bytes just after its first
    // eight, which say how big it is.
    let start = bytes
        .windows(8)
        .position(|eight| eight == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
        .expect("a PNG inside the file");
    for byte in &mut bytes[start + 12..start + 24] {
        *byte = 0xFF;
    }
    let path = folder.join("damaged.glb");
    std::fs::write(&path, bytes).unwrap();
    let error = Mesh::load_glb(&path).unwrap_err().to_string();
    assert!(
        error
            .contains("isn't a model Duck Forge can load (the picture of its images can't be read"),
        "{error}"
    );
}
