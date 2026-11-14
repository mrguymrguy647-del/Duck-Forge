//! Models made with Duck Mold: `Mesh::load_glb` and `GlbModel`.
//!
//! The six models in `tests/fixtures/duck_mold/` are Duck Mold's own
//! golden files, each with the report `duck-mold inspect --json` gave for
//! it. These tests load every one and check it against that report: the
//! triangles, the size, where it stands, its colours. Then they draw the
//! models, to check which way they face: the robot's eyes and the house's
//! door are on their fronts (+z), and the house's chimney on its own left
//! (-x).

use std::f32::consts::PI;
use std::path::{Path, PathBuf};

mod common;
use common::TestFolder;

use duckforge::LoadError;
use duckforge::glb::{AnimatedModel, GlbModel};
use duckforge::prelude::*;

const MODELS: [&str; 6] = ["crate", "table", "tree", "house", "robot", "snowman"];

/// A private copy of the Duck Mold fixtures, for one test.
fn fixtures(test: &str) -> (TestFolder, PathBuf) {
    let folder = TestFolder::new(test);
    let models = folder.fixture("duck_mold");
    (folder, models)
}

/// What `duck-mold inspect --json` said about a model.
fn report(models: &Path, name: &str) -> serde_json::Value {
    let text = std::fs::read_to_string(models.join(format!("{name}.inspect.json"))).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn three(value: &serde_json::Value) -> [f32; 3] {
    [0, 1, 2].map(|axis| value[axis].as_f64().unwrap() as f32)
}

/// A colour written `#RRGGBB`.
fn hex(text: &str) -> Color {
    Color::from_hex(u32::from_str_radix(text.trim_start_matches('#'), 16).unwrap())
}

#[test]
fn every_model_loads_with_the_triangles_size_and_colours_duck_mold_reported()
-> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-reports");
    for name in MODELS {
        let mesh = Mesh::load_glb(models.join(format!("{name}.glb")))?;
        let report = report(&models, name);

        // Every triangle, and every corner one of them uses.
        assert_eq!(
            mesh.triangles.len() as u64,
            report["triangles"].as_u64().unwrap(),
            "{name}"
        );
        // Duck Mold's file gives each face corners of its own; corners in
        // the same place are one vertex in the mesh, so there are fewer.
        assert!(
            (mesh.vertices.len() as u64) < report["vertices"].as_u64().unwrap(),
            "{name}"
        );

        // The size and the box around it, to well under a millimetre: the
        // file holds 32-bit numbers, the report 64-bit ones.
        let (low, high) = mesh.bounds();
        let close = |a: f32, b: f32| (a - b).abs() < 1e-5;
        let [min, max, size] = [
            three(&report["bounds"]["min"]),
            three(&report["bounds"]["max"]),
            three(&report["size"]),
        ];
        for (axis, (found, wanted)) in [
            (low.x, min[0]),
            (low.y, min[1]),
            (low.z, min[2]),
            (high.x, max[0]),
            (high.y, max[1]),
            (high.z, max[2]),
        ]
        .into_iter()
        .enumerate()
        {
            assert!(
                close(found, wanted),
                "{name}: bound {axis} is {found}, not {wanted}"
            );
        }
        let measured = high - low;
        for (found, wanted) in [measured.x, measured.y, measured.z].into_iter().zip(size) {
            assert!(close(found, wanted), "{name}: size {found}, not {wanted}");
        }
        // It stands on the ground.
        assert_eq!(low.y, 0.0, "{name}");

        // Each palette colour paints as many triangles as the report says,
        // with exactly the palette's colour, and no other colour is used.
        let mut counted = 0;
        for color in report["palette"].as_array().unwrap() {
            let wanted = hex(color["hex"].as_str().unwrap());
            let painted = mesh
                .triangles
                .iter()
                .filter(|triangle| triangle.color == wanted)
                .count();
            assert_eq!(
                painted as u64,
                color["triangles"].as_u64().unwrap(),
                "{name}: {}",
                color["name"]
            );
            counted += painted;
        }
        assert_eq!(
            counted,
            mesh.triangles.len(),
            "{name}: only palette colours"
        );
    }
    Ok(())
}

#[test]
fn the_crate_is_a_cube_like_duck_forges_own() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-crate");
    let crate_box = Mesh::load_glb(models.join("crate.glb"))?;
    // 8 corners and 12 triangles, like `Mesh::cube`.
    assert_eq!(
        (crate_box.vertices.len(), crate_box.triangles.len()),
        (8, 12)
    );
    // Duck Forge's own test for its meshes: every triangle's front (its
    // corners clockwise; `(b - a) x (c - a)` in numbers) faces away from
    // the middle of the shape. The crate stands on the ground, so its
    // middle is half a metre up.
    let middle = vec3(0.0, 0.5, 0.0);
    for triangle in &crate_box.triangles {
        let [a, b, c] = triangle.corners.map(|i| crate_box.vertices[i]);
        let normal = (b - a).cross(c - a);
        let centre = (a + b + c) * (1.0 / 3.0);
        assert!(
            normal.dot(centre - middle) > 0.0,
            "{triangle:?} faces inward"
        );
    }
    // Its top and bottom are the darker planks, its sides the wood.
    let plank = Color::from_hex(0x8A5A2B);
    for triangle in &crate_box.triangles {
        let [a, b, c] = triangle.corners.map(|i| crate_box.vertices[i]);
        let upright = (b - a).cross(c - a).y.abs() > 0.5;
        assert_eq!(triangle.color == plank, upright, "{triangle:?}");
    }
    Ok(())
}

/// Every part of every model is a closed solid with its triangles facing
/// out: each edge is walked once each way, and the volume inside comes out
/// positive (it would be negative if the corners went the wrong way round).
#[test]
fn every_part_is_a_closed_solid_facing_outwards() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-solids");
    for name in MODELS {
        let model = GlbModel::load(models.join(format!("{name}.glb")))?;
        let mesh = model.to_mesh();
        assert!(!model.parts.is_empty(), "{name}");
        for part in &model.parts {
            let triangles = &mesh.triangles[part.range()];
            let mut edges = std::collections::BTreeMap::new();
            let mut volume = 0.0;
            for triangle in triangles {
                let [a, b, c] = triangle.corners;
                for edge in [(a, b), (b, c), (c, a)] {
                    *edges.entry(edge).or_insert(0) += 1;
                }
                let [a, b, c] = triangle.corners.map(|i| mesh.vertices[i]);
                volume += a.dot(b.cross(c)) / 6.0;
            }
            for (&(from, to), &count) in &edges {
                assert_eq!(count, 1, "{name}/{}: an edge is walked twice", part.name);
                assert_eq!(
                    edges.get(&(to, from)),
                    Some(&1),
                    "{name}/{}: an edge has no partner",
                    part.name
                );
            }
            assert!(volume > 0.0, "{name}/{}: it faces inwards", part.name);
        }
    }
    // The crate is a 1-metre cube: exactly 1 cubic metre.
    let crate_box = Mesh::load_glb(models.join("crate.glb"))?;
    let volume: f32 = crate_box
        .triangles
        .iter()
        .map(|triangle| {
            let [a, b, c] = triangle.corners.map(|i| crate_box.vertices[i]);
            a.dot(b.cross(c)) / 6.0
        })
        .sum();
    assert!((volume - 1.0).abs() < 1e-5, "{volume}");
    Ok(())
}

#[test]
fn the_model_knows_its_name_palette_and_parts() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-parts");
    let robot = GlbModel::load(models.join("robot.glb"))?;
    assert_eq!(robot.name, "robot");
    let palette: Vec<(&str, [u8; 3])> = robot
        .palette
        .iter()
        .map(|color| (color.name.as_str(), color.rgb))
        .collect();
    assert_eq!(
        palette,
        [
            ("steel", [0x8A, 0x93, 0x9B]),
            ("dark", [0x3B, 0x41, 0x48]),
            ("red", [0xD0, 0x45, 0x3A]),
            ("glow", [0xFF, 0xE0, 0x8A]),
        ]
    );
    // Every part with a shape of its own, in order, owning every triangle
    // once; a part inside another is named with a slash.
    assert_eq!(robot.parts.len(), 19);
    let mut next = 0;
    for part in &robot.parts {
        assert_eq!(part.range().start, next, "{}", part.name);
        next = part.range().end;
    }
    assert_eq!(next, robot.triangles.len());
    assert!(robot.part("left_arm/hand").is_some());
    assert!(robot.part("tail").is_none());
    // A part made of parts (an assembly) is found by name too: its range
    // is the triangles of every part inside it.
    let arm = robot.part("left_arm").expect("the whole arm");
    assert_eq!(arm.triangles, 84);
    let inside: Vec<&str> = robot
        .parts
        .iter()
        .filter(|part| arm.range().contains(&part.first_triangle))
        .map(|part| part.name.as_str())
        .collect();
    assert_eq!(
        inside,
        [
            "left_arm/upper",
            "left_arm/elbow",
            "left_arm/forearm",
            "left_arm/hand"
        ]
    );

    // A part's range picks out its triangles in the mesh too: recolour
    // the hands gold, and exactly their triangles change.
    let mut mesh = robot.to_mesh();
    let gold = Color::rgb(240, 192, 48);
    for hand in ["left_arm/hand", "right_arm/hand"] {
        let part = robot.part(hand).unwrap();
        assert_eq!(part.triangles, 12, "a box");
        for triangle in &mut mesh.triangles[part.range()] {
            triangle.color = gold;
        }
    }
    let golden = mesh.triangles.iter().filter(|t| t.color == gold).count();
    assert_eq!(golden, 24);
    // The hands were red: the red that's left is the rest of the robot's
    // red (its chest panel and the tip of its antenna).
    let red = Color::from_hex(0xD0453A);
    assert_eq!(
        mesh.triangles.iter().filter(|t| t.color == red).count(),
        84 - 24
    );
    // A whole arm in one go: its upper arm, elbow, forearm and hand.
    for triangle in &mut mesh.triangles[arm.range()] {
        triangle.color = gold;
    }
    let golden = mesh.triangles.iter().filter(|t| t.color == gold).count();
    assert_eq!(golden, 84 + 12, "the left arm and the other hand");
    Ok(())
}

#[test]
fn every_assembly_duck_mold_reported_has_the_triangles_of_its_parts() -> Result<(), LoadError> {
    // The report lists the tree of parts; the file lists the assemblies
    // (parts made of parts), outer ones first, each with the triangles of
    // every part inside it, one after the other.
    fn assemblies(parts: &serde_json::Value, into: &mut Vec<(String, usize)>) {
        for part in parts.as_array().unwrap() {
            if part["kind"] == "assembly" {
                into.push((
                    part["name"].as_str().unwrap().to_owned(),
                    part["triangles"].as_u64().unwrap() as usize,
                ));
                assemblies(&part["parts"], into);
            }
        }
    }
    let (_folder, models) = fixtures("duck-mold-assemblies");
    let mut found_any = 0;
    for name in MODELS {
        let model = GlbModel::load(models.join(format!("{name}.glb")))?;
        let mut expected = Vec::new();
        assemblies(&report(&models, name)["parts"], &mut expected);
        let found: Vec<(String, usize)> = model
            .assemblies
            .iter()
            .map(|a| (a.name.clone(), a.triangles))
            .collect();
        assert_eq!(found, expected, "{name}");
        for assembly in &model.assemblies {
            let inside = format!("{}/", assembly.name);
            let parts: Vec<_> = model
                .parts
                .iter()
                .filter(|part| part.name.starts_with(&inside))
                .collect();
            assert_eq!(parts[0].first_triangle, assembly.first_triangle, "{name}");
            let triangles: usize = parts.iter().map(|part| part.triangles).sum();
            assert_eq!(triangles, assembly.triangles, "{name}: {}", assembly.name);
            assert_eq!(model.part(&assembly.name), Some(assembly));
        }
        found_any += model.assemblies.len();
    }
    // The house's roof, and the robot's arms and legs.
    assert_eq!(found_any, 5);
    Ok(())
}

// ---- which way a model faces ------------------------------------------------------

/// Draws a mesh at the origin, seen from `eye` looking at `target`, with
/// the mesh glowing (so every pixel is exactly its triangle's colour).
fn picture(mesh: &Mesh, turn: f32, eye: Vec3, target: Vec3) -> Canvas {
    let mut canvas = Canvas::new(160, 120);
    canvas.clear(Color::BLACK);
    let camera = Camera3D::looking_at(eye, target);
    let place = Transform::at(Vec3::ZERO).rotated(vec3(0.0, turn, 0.0));
    canvas.draw_mesh(&mesh.clone().glowing(), &place, &camera);
    canvas
}

/// Where a colour is on a picture: how many pixels have it, and the middle
/// of them across the picture (from 0 at its left edge to 1 at its right).
fn find(canvas: &Canvas, color: Color) -> (usize, f32) {
    let (mut count, mut sum) = (0, 0.0);
    for y in 0..canvas.height() as i32 {
        for x in 0..canvas.width() as i32 {
            if canvas.get_pixel(x, y) == Some(color) {
                count += 1;
                sum += x as f32;
            }
        }
    }
    let across = sum / count.max(1) as f32 / canvas.width() as f32;
    (count, across)
}

#[test]
fn a_model_faces_forward_and_its_left_is_on_your_right_as_you_face_it() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-facing");
    let house = Mesh::load_glb(models.join("house.glb"))?;
    let (door, brick) = (Color::from_hex(0x6B4A2B), Color::from_hex(0x8A4B3A));
    let middle = vec3(0.0, 1.7, 0.0);

    // From in front: on its +z side, looking back at it.
    let front = picture(&house, 0.0, vec3(0.0, 1.7, 9.0), middle);
    let (door_pixels, _) = find(&front, door);
    assert!(
        door_pixels > 20,
        "the door is on the front ({door_pixels} pixels)"
    );
    // The chimney is on the house's own left (-x). Facing the house, its
    // left is on your right, as with a person facing you.
    let (brick_pixels, brick_across) = find(&front, brick);
    assert!(brick_pixels > 20);
    assert!(
        brick_across > 0.6,
        "the chimney is on the right of the picture ({brick_across})"
    );

    // From behind: no door, and the chimney on the left.
    let back = picture(&house, 0.0, vec3(0.0, 1.7, -9.0), middle);
    assert_eq!(find(&back, door).0, 0, "no door at the back");
    let (_, brick_across) = find(&back, brick);
    assert!(
        brick_across < 0.4,
        "from behind, the chimney is on the left ({brick_across})"
    );
    Ok(())
}

#[test]
fn a_default_camera_sees_a_model_from_behind_until_it_is_turned_round() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-turned");
    let robot = Mesh::load_glb(models.join("robot.glb"))?;
    let eyes = Color::from_hex(0xFFE08A);
    // Camera3D::new looks along +z, the way the robot faces: it sees the
    // robot's back.
    let behind = vec3(0.0, 0.9, -3.0);
    let target = vec3(0.0, 0.9, 0.0);
    assert_eq!(Camera3D::new(behind).forward().z, 1.0);
    let back = picture(&robot, 0.0, behind, target);
    assert_eq!(find(&back, eyes).0, 0, "no eyes from behind");
    assert!(
        find(&back, Color::from_hex(0x8A939B)).0 > 100,
        "but the robot is there"
    );
    // Turned half round, it faces that camera.
    let turned = picture(&robot, PI, behind, target);
    assert!(find(&turned, eyes).0 > 4, "eyes once turned round");
    // Or put the camera on its +z side, looking back: no turn needed.
    let front = picture(&robot, 0.0, vec3(0.0, 0.9, 3.0), target);
    assert!(find(&front, eyes).0 > 4, "eyes from in front");
    Ok(())
}

#[test]
fn a_loaded_model_is_lit_like_any_mesh() -> Result<(), LoadError> {
    // Not glowing: the sun shades it, so the top of the crate (facing the
    // default sun, which is above) is brighter than a side facing away.
    let (_folder, models) = fixtures("duck-mold-lit");
    let crate_box = Mesh::load_glb(models.join("crate.glb"))?;
    let mut canvas = Canvas::new(160, 120);
    canvas.clear(Color::BLACK);
    let camera = Camera3D::looking_at(vec3(1.5, 2.5, 3.0), vec3(0.0, 0.5, 0.0));
    canvas.draw_mesh(&crate_box, &Transform::at(Vec3::ZERO), &camera);
    let drawn = (0..120)
        .flat_map(|y| (0..160).map(move |x| (x, y)))
        .filter(|&(x, y)| canvas.get_pixel(x, y) != Some(Color::BLACK))
        .count();
    assert!(
        drawn > 1000,
        "the crate fills part of the picture ({drawn} pixels)"
    );
    // Lit, so not exactly the palette's colours any more.
    let exact =
        find(&canvas, Color::from_hex(0xB07A3F)).0 + find(&canvas, Color::from_hex(0x8A5A2B)).0;
    assert!(exact < drawn / 2, "{exact} of {drawn} pixels unlit");
    Ok(())
}

// ---- sockets -----------------------------------------------------------------------

#[test]
fn every_socket_is_where_duck_mold_said() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-sockets");
    let model = GlbModel::load(models.join("robot.glb"))?;
    let text = std::fs::read_to_string(models.join("robot.sockets.json")).unwrap();
    let listed: serde_json::Value = serde_json::from_str(&text).unwrap();
    let listed = listed["sockets"].as_array().unwrap();
    assert_eq!(model.sockets.len(), listed.len());
    assert_eq!(model.sockets.len(), 187);
    for expected in listed {
        let name = expected["name"].as_str().unwrap();
        let socket = model.socket(name).expect(name);
        // The file keeps 32-bit numbers: they agree to about 0.00001.
        for (found, want) in [
            (socket.position(), three(&expected["at"])),
            (socket.direction(), three(&expected["facing"])),
        ] {
            let off = (found - vec3(want[0], want[1], want[2])).length();
            assert!(off < 1e-5, "{name}: {found:?} is not {want:?}");
        }
    }
    assert!(model.socket("head.hat").is_none());

    // The top of the head is the top of the head's own triangles, facing
    // up; the left hand is on the model's left (-x), its grip facing down.
    let mesh = model.to_mesh();
    let head = model.part("head").unwrap();
    let highest = mesh.triangles[head.range()]
        .iter()
        .flat_map(|triangle| triangle.corners.map(|at| mesh.vertices[at].y))
        .fold(f32::NEG_INFINITY, f32::max);
    let top = model.socket("head.top").unwrap();
    assert_eq!(top.position().y, highest);
    assert_eq!(top.direction(), vec3(0.0, 1.0, 0.0));
    let grip = model.socket("left_arm.grip").unwrap();
    assert!(grip.position().x < 0.0);
    assert_eq!(grip.direction(), vec3(0.0, -1.0, 0.0));
    // A part inside an assembly is found by its path.
    assert_eq!(
        model.socket("left_arm/hand.bottom").unwrap().position(),
        grip.position()
    );
    Ok(())
}

#[test]
fn a_socket_moves_and_turns_with_the_model() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-socket-moves");
    let model = GlbModel::load(models.join("robot.glb"))?;
    let grip = model.socket("left_arm.grip").unwrap();
    // A quarter turn swings the robot's front (+z) round to +x, so its
    // left hand (-x) comes round to +z, as every corner of the mesh does.
    let place = Transform::at(vec3(3.0, 0.5, -2.0)).rotated(vec3(0.0, PI / 2.0, 0.0));
    let moved = place.apply(grip.position());
    assert!((moved.y - (grip.position().y + 0.5)).abs() < 1e-5);
    assert!(
        (moved.z - (-2.0 - grip.position().x)).abs() < 1e-4,
        "{moved:?}"
    );
    assert!(
        (moved.x - (3.0 + grip.position().z)).abs() < 1e-4,
        "{moved:?}"
    );
    Ok(())
}

#[test]
fn a_file_from_before_sockets_has_none() {
    let old = with_json(&crate_bytes(), |json| {
        let mut value: serde_json::Value = serde_json::from_str(json).unwrap();
        value["extras"]["duck_mold"]
            .as_object_mut()
            .unwrap()
            .remove("sockets");
        serde_json::to_string(&value).unwrap()
    });
    let model = duckforge::glb::read_duck_mold_glb(&old).unwrap();
    assert!(model.sockets.is_empty());
    assert!(model.socket("top").is_none());
    // And the crate as it is has the seven every model has, and its box's.
    let model = duckforge::glb::read_duck_mold_glb(&crate_bytes()).unwrap();
    assert_eq!(model.sockets.len(), 14);
    assert_eq!(model.socket("top").unwrap().position(), vec3(0.0, 1.0, 0.0));
}

// ---- files that can't be loaded ----------------------------------------------------

/// The bytes of the crate model.
fn crate_bytes() -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/duck_mold/crate.glb"))
        .unwrap()
}

/// A `.glb` with its JSON block changed by `change`, put back together
/// with the right lengths, so that only what `change` did is wrong.
fn with_json(glb: &[u8], change: impl Fn(&str) -> String) -> Vec<u8> {
    let length = |at: usize| u32::from_le_bytes(glb[at..at + 4].try_into().unwrap()) as usize;
    let json_end = 20 + length(12);
    let json = std::str::from_utf8(&glb[20..json_end]).unwrap();
    let mut json = change(json.trim_end()).into_bytes();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let numbers = &glb[json_end..]; // its length, "BIN\0", the numbers
    let total = 12 + 8 + json.len() + numbers.len();
    let mut out = Vec::new();
    out.extend(b"glTF");
    out.extend(2u32.to_le_bytes());
    out.extend((total as u32).to_le_bytes());
    out.extend((json.len() as u32).to_le_bytes());
    out.extend(b"JSON");
    out.extend(json);
    out.extend(numbers);
    out
}

#[test]
fn a_missing_or_wrong_file_is_one_sentence() {
    let folder = TestFolder::new("duck-mold-errors");

    let missing = Mesh::load_glb(folder.join("nowhere.glb")).unwrap_err();
    assert_eq!(missing.kind(), std::io::ErrorKind::NotFound);
    let text = missing.to_string();
    assert!(text.starts_with("couldn't find the model file"), "{text}");
    assert_eq!(format!("{missing:?}"), text, "Debug is the sentence too");

    // A file that isn't a .glb at all.
    std::fs::write(folder.join("notes.glb"), "not a model").unwrap();
    let wrong = Mesh::load_glb(folder.join("notes.glb")).unwrap_err();
    assert_eq!(wrong.kind(), std::io::ErrorKind::InvalidData);
    let text = wrong.to_string();
    assert!(
        text.contains("notes.glb isn't a model Duck Forge can load"),
        "{text}"
    );
    assert!(text.contains("doesn't start with \"glTF\""), "{text}");
    assert!(text.contains("duck-mold export"), "{text}");

    // A real .glb that some other program made: here, Duck Mold's crate
    // with its own mark ("duck_mold" under "extras") renamed.
    let crate_box = crate_bytes();
    // The helper itself changes nothing when asked to change nothing.
    assert_eq!(with_json(&crate_box, str::to_owned), crate_box);
    let foreign = with_json(&crate_box, |json| json.replace("duck_mold", "duck_mole"));
    std::fs::write(folder.join("foreign.glb"), &foreign).unwrap();
    let error = GlbModel::load(folder.join("foreign.glb")).unwrap_err();
    assert!(
        error.to_string().contains("wasn't written by Duck Mold"),
        "{error}"
    );

    // A file cut short.
    std::fs::write(folder.join("cut.glb"), &crate_box[..crate_box.len() - 40]).unwrap();
    let error = Mesh::load_glb(folder.join("cut.glb")).unwrap_err();
    assert!(
        error.to_string().contains("cut.glb isn't a model"),
        "{error}"
    );
}

#[test]
fn a_damaged_file_that_claims_too_many_numbers_is_refused_not_a_crash() {
    // The crate, with the count of its corner positions made bigger than
    // the file, and then bigger than any number of bytes. The reader must
    // refuse it with a sentence, not overflow while working out where
    // the numbers end.
    let folder = TestFolder::new("duck-mold-huge");
    let crate_box = crate_bytes();
    for huge in ["99", "4611686018427387904", "18446744073709551615"] {
        let damaged = with_json(&crate_box, |json| {
            let changed = json.replacen("\"count\":24", &format!("\"count\":{huge}"), 1);
            assert_ne!(changed, json, "the count was found");
            changed
        });
        std::fs::write(folder.join("huge.glb"), &damaged).unwrap();
        let error = Mesh::load_glb(folder.join("huge.glb")).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData, "{error}");
        assert!(
            error.to_string().contains("runs past the end of the file"),
            "{error}"
        );
    }
    // A part that claims triangles beyond the last.
    let damaged = with_json(&crate_box, |json| {
        json.replacen("\"triangles\":12", "\"triangles\":18446744073709551615", 1)
    });
    std::fs::write(folder.join("parts.glb"), &damaged).unwrap();
    let error = GlbModel::load(folder.join("parts.glb")).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("names triangles that aren't there"),
        "{error}"
    );
}

// ---- files laid out as a tree (Duck Mold's Phase 5), and files from before ---------

/// The models kept as Duck Mold wrote them before its Phase 5: one
/// object with one list of triangles.
const BEFORE_THE_TREE: [&str; 3] = ["crate", "house", "robot"];

#[test]
fn a_file_from_before_the_tree_loads_as_the_same_model_and_draws_the_same_picture()
-> Result<(), LoadError> {
    let folder = TestFolder::new("duck-mold-before-the-tree");
    let (old_files, new_files) = (
        folder.fixture("duck_mold_before_tree"),
        folder.fixture("duck_mold"),
    );
    for name in BEFORE_THE_TREE {
        let old = GlbModel::load(old_files.join(format!("{name}.glb")))?;
        let new = GlbModel::load(new_files.join(format!("{name}.glb")))?;
        // The same triangles in the same order with the same colours. A
        // corner of a new file is its part's pivot plus its place in the
        // part, added up on loading, so it can differ in its last digits.
        assert_eq!(old.triangles.len(), new.triangles.len(), "{name}");
        for (before, now) in old.triangles.iter().zip(&new.triangles) {
            assert_eq!(before.color, now.color, "{name}");
            for corner in 0..3 {
                let (a, b) = (
                    old.vertices[before.corners[corner]],
                    new.vertices[now.corners[corner]],
                );
                assert!(
                    (0..3).all(|axis| (a[axis] - b[axis]).abs() < 1e-6),
                    "{name}: {a:?} {b:?}"
                );
            }
        }
        assert_eq!(old.name, new.name);
        assert_eq!(old.palette, new.palette);
        assert_eq!(old.parts, new.parts, "{name}");
        assert_eq!(old.assemblies, new.assemblies, "{name}");
        let names = |model: &GlbModel| -> Vec<String> {
            model.sockets.iter().map(|s| s.name.clone()).collect()
        };
        assert_eq!(names(&old), names(&new), "{name}");
        // It still stands on the ground, exactly.
        for model in [&old, &new] {
            let lowest = model
                .vertices
                .iter()
                .map(|v| v[1])
                .fold(f32::INFINITY, f32::min);
            assert_eq!(lowest, 0.0, "{name}");
        }
        // An old file has no tree: it can be drawn, not posed.
        assert!(old.joins.is_empty() && !new.joins.is_empty(), "{name}");
        assert!(old.sockets.iter().all(|socket| socket.join.is_none()));

        // And the picture is the same, pixel for pixel, from the front
        // and from behind.
        let (old, new) = (old.to_mesh(), new.to_mesh());
        let (low, high) = new.bounds();
        let middle = (low + high) * 0.5;
        let away = (high - low).length() * 1.4;
        for side in [1.0, -1.0] {
            let eye = middle + vec3(away * 0.3, away * 0.2, away * side);
            let (before, now) = (
                picture(&old, 0.4, eye, middle),
                picture(&new, 0.4, eye, middle),
            );
            assert!(before.pixels() == now.pixels(), "{name} looks different");
            assert!(
                before
                    .pixels()
                    .iter()
                    .any(|pixel| *pixel != before.pixels()[0]),
                "{name} is in the picture"
            );
        }
    }
    Ok(())
}

#[test]
fn a_new_file_says_what_every_part_turns_about_and_hangs_from() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-joins");
    for name in MODELS {
        let model = GlbModel::load(models.join(format!("{name}.glb")))?;
        // One join for every part and every assembly, each after the one
        // it hangs from.
        assert_eq!(
            model.joins.len(),
            model.parts.len() + model.assemblies.len(),
            "{name}"
        );
        for (at, join) in model.joins.iter().enumerate() {
            assert!(model.part(&join.name).is_some(), "{name}: {}", join.name);
            assert!(join.parent.is_none_or(|parent| parent < at), "{name}");
        }
        // A part's corners are its own, next to each other, and the parts
        // have them all between them.
        let mut owner = vec![None; model.vertices.len()];
        for (at, join) in model.joins.iter().enumerate() {
            for owned in &mut owner[join.first_vertex..join.first_vertex + join.vertices] {
                assert!(owned.replace(at).is_none(), "{name}: a shared corner");
            }
        }
        assert!(owner.iter().all(Option::is_some), "{name}");
        // And its triangles use only its own corners.
        for part in &model.parts {
            let join = model
                .joins
                .iter()
                .position(|join| join.name == part.name)
                .unwrap();
            for triangle in &model.triangles[part.range()] {
                assert!(
                    triangle
                        .corners
                        .iter()
                        .all(|&corner| owner[corner] == Some(join))
                );
            }
        }
    }

    // The robot: its body hangs from the model; its head from its body,
    // turning about the neck; a hand from the forearm of its arm.
    let robot = GlbModel::load(models.join("robot.glb"))?;
    let join = |name: &str| {
        let at = robot
            .joins
            .iter()
            .position(|join| join.name == name)
            .unwrap();
        (at, &robot.joins[at])
    };
    let (body, _) = join("body");
    assert_eq!(robot.joins[body].parent, None);
    let (_, head) = join("head");
    assert_eq!(head.parent, Some(body));
    assert_eq!(head.pivot, robot.socket("body.neck").unwrap().at);
    let (arm, shoulder) = join("left_arm");
    assert_eq!(shoulder.parent, Some(body));
    assert_eq!(shoulder.pivot, robot.socket("body.left_arm").unwrap().at);
    assert_eq!(shoulder.vertices, 0, "an arm's corners are its parts'");
    let (upper, _) = join("left_arm/upper");
    assert_eq!(robot.joins[upper].parent, Some(arm));
    let (elbow, _) = join("left_arm/elbow");
    assert_eq!(robot.joins[elbow].parent, Some(upper));
    let (forearm, _) = join("left_arm/forearm");
    let (_, hand) = join("left_arm/hand");
    assert_eq!(hand.parent, Some(forearm));
    assert_eq!(
        hand.pivot,
        robot.socket("left_arm/forearm.bottom").unwrap().at
    );

    // A socket says which part carries it: a hat on `head.top` goes with
    // the head; the arm's `grip` was passed on from its hand, so it goes
    // with the hand; the model's own `top` with nothing.
    let carrier = |socket: &str| {
        robot
            .socket(socket)
            .unwrap()
            .join
            .map(|join| robot.joins[join].name.as_str())
    };
    assert_eq!(carrier("head.top"), Some("head"));
    assert_eq!(carrier("left_arm.grip"), Some("left_arm/hand"));
    assert_eq!(carrier("left_arm.top"), Some("left_arm"));
    assert_eq!(carrier("top"), None);
    Ok(())
}

#[test]
fn a_file_from_a_newer_duck_mold_says_to_update_duck_forge() {
    let folder = TestFolder::new("duck-mold-newer");
    let newer = with_json(&crate_bytes(), |json| {
        let changed = json.replacen("\"version\":1,\"name\"", "\"version\":2,\"name\"", 1);
        assert_ne!(changed, json, "the version was found");
        changed
    });
    std::fs::write(folder.join("newer.glb"), &newer).unwrap();
    let error = Mesh::load_glb(folder.join("newer.glb")).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("newer Duck Mold than this reader knows: update Duck Forge"),
        "{error}"
    );
}

// ---- models that move (Duck Mold's Phase 5): animations and poses ------------------

/// The robot's animations, each with whether it loops and how long it
/// lasts, as its recipe wrote them.
const ANIMATIONS: [(&str, bool, f32); 3] = [
    ("idle", true, 2.0),
    ("walk", true, 0.8),
    ("wave", false, 1.5),
];

fn robot(test: &str) -> Result<(TestFolder, PathBuf, AnimatedModel), LoadError> {
    let (folder, models) = fixtures(test);
    let robot = AnimatedModel::load(models.join("robot.glb"))?;
    Ok((folder, models, robot))
}

/// The lowest and highest corner of a mesh.
fn low_and_high(mesh: &Mesh) -> (f32, f32) {
    let (low, high) = mesh.bounds();
    (low.y, high.y)
}

#[test]
fn an_animated_model_names_its_animations_and_poses() -> Result<(), LoadError> {
    let (_folder, models, robot) = robot("duck-mold-animations")?;
    assert_eq!(robot.animations(), ["idle", "walk", "wave"]);
    assert_eq!(
        robot.poses(),
        [
            "step_left",
            "step_right",
            "wave",
            "wave_out",
            "breathe_in",
            "breathe_out"
        ]
    );
    // Duck Mold's report on the model names the same ones.
    let report = report(&models, "robot");
    let reported: Vec<&str> = report["animations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|animation| animation["name"].as_str().unwrap())
        .collect();
    assert_eq!(robot.animations(), reported);
    let reported: Vec<&str> = report["poses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pose| pose.as_str().unwrap())
        .collect();
    assert_eq!(robot.poses(), reported);
    for (animation, (name, looping, length)) in robot.model().animations.iter().zip(ANIMATIONS) {
        assert_eq!(animation.name, name);
        assert_eq!(animation.looping, looping, "{name}");
        assert_eq!(animation.length, length, "{name}");
    }

    // Before anything is played it stands at rest: the very mesh
    // `Mesh::load_glb` gives.
    let still = Mesh::load_glb(models.join("robot.glb"))?;
    assert_eq!(robot.mesh().vertices, still.vertices);
    assert_eq!(robot.mesh().triangles.len(), still.triangles.len());
    assert_eq!(robot.playing(), None);
    assert!(!robot.is_playing() && !robot.finished());

    // A model with no animations loads too, and stands there.
    let mut plain = AnimatedModel::load(models.join("crate.glb"))?;
    assert!(plain.animations().is_empty() && plain.poses().is_empty());
    let before = plain.mesh().vertices.clone();
    plain.play("walk"); // said once on the terminal; nothing changes
    plain.update(0.5);
    assert_eq!(plain.mesh().vertices, before);
    assert_eq!(plain.playing(), None);
    Ok(())
}

#[test]
fn every_animation_plays_as_duck_mold_reported() -> Result<(), LoadError> {
    let (_folder, models, _) = robot("duck-mold-plays")?;
    for (name, looping, length) in ANIMATIONS {
        let text =
            std::fs::read_to_string(models.join(format!("robot.{name}.inspect.json"))).unwrap();
        let report: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(report["loops"].as_bool(), Some(looping), "{name}");
        assert_eq!(report["length"].as_f64().unwrap() as f32, length, "{name}");

        let mut robot = AnimatedModel::load(models.join("robot.glb"))?;
        robot.set_blend(0.0); // straight into it, to compare frame by frame
        robot.play(name);
        assert_eq!(robot.playing(), Some(name));
        assert!(robot.is_playing());
        let start = robot.mesh().vertices.clone();

        // Duck Mold looked at every frame, thirty a second, and wrote
        // down how far the robot's lowest point goes below the ground
        // and above it. Played here, frame by frame, it is the same.
        let frames = report["frames"].as_u64().unwrap();
        let (mut lowest, mut highest) = (f32::INFINITY, f32::NEG_INFINITY);
        let mut moved = false;
        for frame in 0..frames {
            let (low, _) = low_and_high(robot.mesh());
            (lowest, highest) = (lowest.min(low), highest.max(low));
            moved |= robot.mesh().vertices != start;
            assert_eq!(robot.playing(), Some(name));
            if frame + 1 < frames {
                robot.update(1.0 / 30.0);
            }
        }
        assert!(moved, "{name} moves the robot");
        let ground = &report["ground"];
        let close = |found: f32, wanted: &serde_json::Value| {
            (f64::from(found) - wanted.as_f64().unwrap()).abs() < 2e-4
        };
        assert!(close(lowest, &ground["lowest_y"]), "{name}: {lowest}");
        assert!(close(highest, &ground["highest_y"]), "{name}: {highest}");

        // On to its end, and a little past it.
        robot.update(length);
        if looping {
            // Round again: still playing, somewhere inside it; and a
            // whole number of rounds later it stands as it did.
            assert!(robot.is_playing() && !robot.finished(), "{name}");
            assert!(robot.time() >= 0.0 && robot.time() < length, "{name}");
            let mut again = AnimatedModel::load(models.join("robot.glb"))?;
            again.set_blend(0.0);
            again.play(name);
            again.update(0.25);
            let early = again.mesh().vertices.clone();
            for _ in 0..3 {
                again.update(length);
            }
            for (now, then) in again.mesh().vertices.iter().zip(&early) {
                assert!((*now - *then).length() < 1e-4, "{name}: {now:?} {then:?}");
            }
        } else {
            // Played once: it stops on its last pose and says so.
            assert!(robot.finished() && !robot.is_playing(), "{name}");
            assert_eq!(robot.playing(), Some(name));
            assert_eq!(robot.time(), length);
            let end = robot.mesh().vertices.clone();
            robot.update(1.0);
            assert_eq!(robot.mesh().vertices, end, "{name} stays on its last pose");
        }
    }
    Ok(())
}

/// A mesh turned about the model's upright: half round (to see its
/// front from a camera looking along +z) or a quarter (to see its right
/// side). Done by swapping numbers, so that every computer gets exactly
/// the same picture.
fn turned_for_the_camera(mesh: &Mesh, side: bool) -> Mesh {
    let mut turned = mesh.clone();
    for vertex in &mut turned.vertices {
        *vertex = if side {
            vec3(vertex.z, vertex.y, -vertex.x)
        } else {
            vec3(-vertex.x, vertex.y, -vertex.z)
        };
    }
    turned
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
        std::fs::create_dir_all(stored.parent().unwrap()).unwrap();
        std::fs::write(&stored, drawn).unwrap();
        return;
    }
    let wanted = std::fs::read(&stored).unwrap_or_else(|_| {
        panic!(
            "{} is missing: store it with DUCKFORGE_ACCEPT_PICTURES=1 cargo test --test duck_mold_models",
            stored.display()
        )
    });
    assert!(
        drawn == wanted,
        "{name} is no longer the stored picture. If the change is meant, store the new one with DUCKFORGE_ACCEPT_PICTURES=1 cargo test --test duck_mold_models"
    );
}

#[test]
fn every_animation_draws_its_stored_pictures() -> Result<(), LoadError> {
    let (folder, models, _) = robot("duck-mold-pictures")?;
    // A camera that looks straight along +z, level, from 2.6 metres:
    // nothing in it is worked out with sines, so every computer draws
    // the same pixels.
    let camera = Camera3D {
        position: vec3(0.0, 0.95, -2.6),
        yaw: 0.0,
        pitch: 0.0,
        ..Camera3D::new(Vec3::ZERO)
    };
    let draw = |mesh: &Mesh, side: bool| {
        let mut canvas = Canvas::new(160, 120);
        canvas.clear(Color::rgb(0x2a, 0x2f, 0x3a));
        canvas.draw_mesh(
            &turned_for_the_camera(mesh, side),
            &Transform::default(),
            &camera,
        );
        canvas
    };
    for (name, _, length) in ANIMATIONS {
        // The walk shows best from the side; the others from the front.
        let side = name == "walk";
        let mut robot = AnimatedModel::load(models.join("robot.glb"))?;
        robot.set_blend(0.0);
        robot.play(name);
        let mut pictures: Vec<Canvas> = Vec::new();
        // Four moments: a tenth of the way in, then three tenths, a
        // little past half and eight tenths. (Not evenly spread: a loop
        // that goes out and back the same way would give two pairs of
        // the same picture.)
        for (moment, step) in [0.1, 0.2, 0.25, 0.25].into_iter().enumerate() {
            robot.update(length * step);
            let canvas = draw(robot.mesh(), side);
            check_picture(
                &folder,
                &format!("robot.{name}.{}.png", moment + 1),
                &canvas,
            );
            pictures.push(canvas);
        }
        // They are four different pictures: it moves.
        for (at, one) in pictures.iter().enumerate() {
            for other in &pictures[at + 1..] {
                assert!(one.pixels() != other.pixels(), "{name}: two moments alike");
            }
        }
    }
    // And the robot at rest, from the front and from the side.
    let robot = AnimatedModel::load(models.join("robot.glb"))?;
    check_picture(&folder, "robot.rest.front.png", &draw(robot.mesh(), false));
    check_picture(&folder, "robot.rest.side.png", &draw(robot.mesh(), true));
    Ok(())
}

#[test]
fn play_stop_loop_and_speed() -> Result<(), LoadError> {
    let (_folder, _models, mut robot) = robot("duck-mold-play")?;
    robot.set_blend(0.0);

    // Asking for what is already playing changes nothing: safe to call
    // every frame.
    robot.play("walk");
    robot.update(0.3);
    let time = robot.time();
    robot.play("walk");
    assert_eq!(robot.time(), time);
    // `restart` plays it from its start again.
    robot.restart();
    assert_eq!(robot.time(), 0.0);

    // Stopped, it freezes as it stands; resumed, it carries on.
    robot.update(0.3);
    let frozen = robot.mesh().vertices.clone();
    robot.stop();
    assert!(!robot.is_playing());
    robot.update(1.0);
    assert_eq!(robot.mesh().vertices, frozen);
    assert_eq!(robot.playing(), Some("walk"));
    robot.resume();
    assert!(robot.is_playing());
    robot.update(0.1);
    assert_ne!(robot.mesh().vertices, frozen);

    // Twice as fast is twice as far on; a speed of 0 is still.
    robot.restart();
    robot.set_speed(2.0);
    robot.update(0.1);
    assert!((robot.time() - 0.2).abs() < 1e-6);
    robot.set_speed(0.0);
    robot.update(5.0);
    assert!((robot.time() - 0.2).abs() < 1e-6);
    robot.set_speed(1.0);

    // An animation that plays once can be told to loop, and one that
    // loops to play once.
    robot.play_looping("wave");
    robot.update(1.6);
    assert!(robot.is_playing() && !robot.finished());
    assert!((robot.time() - 0.1).abs() < 1e-4, "{}", robot.time());
    robot.play_once("walk");
    robot.update(0.5);
    assert!(robot.is_playing());
    robot.update(0.5);
    assert!(robot.finished() && robot.time() == 0.8);

    // Backwards: it starts from its end and finishes at its start. (A
    // door's `open`, played backwards, closes it.)
    robot.rest();
    robot.set_speed(-1.0);
    robot.play("wave");
    assert_eq!(robot.time(), 1.5);
    robot.update(0.5);
    assert!((robot.time() - 1.0).abs() < 1e-6 && robot.is_playing());
    robot.update(2.0);
    assert!(robot.finished() && robot.time() == 0.0);
    // The wave starts at rest, so backwards it ends there.
    let still = robot.model().to_mesh();
    assert_eq!(robot.mesh().vertices, still.vertices);

    // A name it doesn't have changes nothing.
    robot.set_speed(1.0);
    robot.play("idle");
    robot.update(0.4);
    let before = (robot.time(), robot.mesh().vertices.clone());
    robot.play("wlak");
    robot.play("wlak");
    assert_eq!(robot.playing(), Some("idle"));
    assert_eq!((robot.time(), robot.mesh().vertices.clone()), before);
    Ok(())
}

#[test]
fn a_pose_is_held_and_rest_is_the_model_as_built() -> Result<(), LoadError> {
    let (_folder, _models, mut robot) = robot("duck-mold-poses")?;
    robot.set_blend(0.0);
    let still = robot.mesh().vertices.clone();
    let (_, at_rest) = low_and_high(robot.mesh());

    // Its arm goes up, and stays up: a pose plays nothing.
    robot.set_pose("wave");
    let (_, waving) = low_and_high(robot.mesh());
    assert!(waving > at_rest + 0.05, "{waving} against {at_rest}");
    let held = robot.mesh().vertices.clone();
    robot.update(3.0);
    assert_eq!(robot.mesh().vertices, held);
    assert_eq!(robot.playing(), None);
    assert!(!robot.is_playing());

    // An animation takes over from a pose, and a pose from an animation.
    robot.play("walk");
    robot.update(0.2);
    assert_eq!(robot.playing(), Some("walk"));
    robot.set_pose("step_left");
    assert_eq!(robot.playing(), None);
    // The step sinks the body 4 cm, with its feet still on the ground.
    let (low, high) = low_and_high(robot.mesh());
    assert!(low.abs() < 0.005, "{low}");
    assert!((high - (at_rest - 0.04)).abs() < 1e-4, "{high}");

    // `rest`, by its own name or as a pose, is the model as built.
    robot.rest();
    assert_eq!(robot.mesh().vertices, still);
    robot.set_pose("wave");
    robot.set_pose("rest");
    assert_eq!(robot.mesh().vertices, still);

    // A pose it doesn't have changes nothing.
    robot.set_pose("wave");
    robot.set_pose("waev");
    assert_eq!(robot.mesh().vertices, held);
    Ok(())
}

#[test]
fn changing_animation_blends_instead_of_snapping() -> Result<(), LoadError> {
    let (_folder, _models, mut robot) = robot("duck-mold-blend")?;
    let (_, at_rest) = low_and_high(robot.mesh());
    // From rest to the wave pose: 0.15 seconds, unless told otherwise.
    robot.set_pose("wave");
    let (_, at_once) = low_and_high(robot.mesh());
    assert_eq!(at_once, at_rest, "nothing has moved yet");
    let mut before = at_rest;
    for step in 1..=9 {
        robot.update(1.0 / 60.0);
        let (_, now) = low_and_high(robot.mesh());
        assert!(now >= before, "step {step}: it only goes up");
        before = now;
    }
    // After 9 sixtieths (0.15 s) it is there, and stays.
    let arrived = robot.mesh().vertices.clone();
    let mut snapped = AnimatedModel::new(robot.model().clone());
    snapped.set_blend(0.0);
    snapped.set_pose("wave");
    for (now, wanted) in arrived.iter().zip(&snapped.mesh().vertices) {
        assert!((*now - *wanted).length() < 1e-5);
    }
    robot.update(1.0);
    assert_eq!(robot.mesh().vertices, arrived);

    // Half way through the change back, its hand is between up and
    // down.
    let hand = |robot: &AnimatedModel| robot.socket("left_arm.grip").unwrap().position().y;
    let up = hand(&robot);
    robot.set_blend(0.2);
    robot.rest();
    robot.update(0.1);
    let half = hand(&robot);
    robot.update(0.1);
    let down = hand(&robot);
    assert!(up > down + 0.8, "{up} and {down}");
    assert!(half > down + 0.2 && half < up - 0.2, "{half}");

    // A walking robot that stops doesn't snap to standing either: its
    // corners move a little each frame, never far.
    let mut robot = AnimatedModel::new(robot.model().clone());
    robot.play("walk");
    robot.update(0.4);
    robot.play("idle");
    let mut last = robot.mesh().vertices.clone();
    for _ in 0..30 {
        robot.update(1.0 / 60.0);
        let now = robot.mesh().vertices.clone();
        let furthest = now
            .iter()
            .zip(&last)
            .map(|(a, b)| (*a - *b).length())
            .fold(0.0, f32::max);
        assert!(furthest < 0.06, "a corner jumped {furthest} in one frame");
        last = now;
    }
    Ok(())
}

#[test]
fn game_code_turns_a_join_and_gives_it_back() -> Result<(), LoadError> {
    let (_folder, _models, mut robot) = robot("duck-mold-turn")?;
    robot.set_blend(0.0);
    let still = robot.mesh().vertices.clone();
    let head = robot.model().part("head").unwrap().clone();
    let eye = robot.model().socket("head.left_eye").unwrap().position();
    // Which corners are the head's own, and its eyes' and antenna's: the
    // corners its triangles and theirs use.
    let on_head: Vec<usize> = robot
        .model()
        .parts
        .iter()
        .filter(|part| {
            ["head", "left_eye", "right_eye", "antenna", "antenna_tip"]
                .contains(&part.name.as_str())
        })
        .flat_map(|part| robot.mesh().triangles[part.range()].iter())
        .flat_map(|triangle| triangle.corners)
        .collect();
    assert!(head.triangles == 12 && on_head.len() > 100);

    // The head looks 90 degrees to its right: a positive turn around y
    // swings its front (+z) towards +x. Its eye, which was in front, is
    // now at its side; nothing below the neck has moved.
    // It turns about its neck: the point the head is joined at.
    let [neck_x, _, neck_z] = robot
        .model()
        .joins
        .iter()
        .find(|join| join.name == "head")
        .unwrap()
        .pivot;
    robot.turn("head", vec3(0.0, 90.0, 0.0));
    let now = robot.socket("head.left_eye").unwrap().position();
    assert!(
        (now.x - (neck_x + (eye.z - neck_z))).abs() < 1e-4
            && (now.z - (neck_z - (eye.x - neck_x))).abs() < 1e-4,
        "{now:?}, from {eye:?}"
    );
    assert!(
        now.x > 0.1,
        "its left eye is on its right side now: {now:?}"
    );
    assert!((now.y - eye.y).abs() < 1e-5);
    for (at, (now, was)) in robot.mesh().vertices.iter().zip(&still).enumerate() {
        if !on_head.contains(&at) {
            assert_eq!(now, was, "corner {at} isn't on the head");
        }
    }
    assert_ne!(robot.mesh().vertices, still);

    // The turn stays while an animation plays the rest of the robot.
    robot.play("walk");
    robot.update(0.1);
    let walking = robot.socket("head.left_eye").unwrap().position();
    // (The walk sinks the body a little; the head still looks right.)
    assert!((walking.x - now.x).abs() < 1e-4, "{walking:?}");

    // Given back, the head is the animation's again; at rest, as built.
    robot.release("head");
    robot.rest();
    assert_eq!(robot.mesh().vertices, still);

    // A part it doesn't have changes nothing.
    robot.turn("tail", vec3(0.0, 90.0, 0.0));
    robot.release("tail");
    assert_eq!(robot.mesh().vertices, still);

    // A part inside a mirror image turns as a mirror image: both
    // elbows, bent by the same numbers, lift both hands the same.
    robot.turn("left_arm/elbow", vec3(90.0, 0.0, 0.0));
    robot.turn("right_arm/elbow", vec3(90.0, 0.0, 0.0));
    let left = robot.socket("left_arm.grip").unwrap().position();
    let right = robot.socket("right_arm.grip").unwrap().position();
    assert!((left.x + right.x).abs() < 1e-4, "{left:?} {right:?}");
    assert!((left.y - right.y).abs() < 1e-4 && (left.z - right.z).abs() < 1e-4);
    let at_rest = robot.model().socket("left_arm.grip").unwrap().position();
    assert!(
        left.y > at_rest.y + 0.2 && left.z > at_rest.z + 0.2,
        "{left:?}"
    );
    Ok(())
}

#[test]
fn a_socket_goes_with_the_part_that_carries_it() -> Result<(), LoadError> {
    let (_folder, _models, mut robot) = robot("duck-mold-carried")?;
    robot.set_blend(0.0);
    let at_rest = robot.socket("head.top").unwrap();
    assert_eq!(
        at_rest.position(),
        robot.model().socket("head.top").unwrap().position()
    );
    assert_eq!(at_rest.direction(), vec3(0.0, 1.0, 0.0));
    assert!(robot.socket("head.bottom_of_nothing").is_none());

    // A step sinks the body 4 cm: the top of the head goes down with
    // it, still facing up.
    robot.set_pose("step_left");
    let stepping = robot.socket("head.top").unwrap();
    assert!((stepping.position().y - (at_rest.position().y - 0.04)).abs() < 1e-5);
    assert!((stepping.direction() - vec3(0.0, 1.0, 0.0)).length() < 1e-6);

    // A nod tips it: the top of the head leans, and the way it faces
    // leans with it, so a hat put there leans too.
    robot.rest();
    robot.turn("head", vec3(30.0, 0.0, 0.0));
    let nodding = robot.socket("head.top").unwrap();
    let up = nodding.direction();
    assert!((up.length() - 1.0).abs() < 1e-5);
    assert!((up.y - 30f32.to_radians().cos()).abs() < 1e-4, "{up:?}");
    assert!(up.z.abs() > 0.49 && up.x.abs() < 1e-5, "{up:?}");
    // The hand's grip is carried by the hand: waving lifts it over the
    // head.
    robot.release("head");
    robot.set_pose("wave");
    let grip = robot.socket("left_arm.grip").unwrap().position();
    assert!(grip.y > at_rest.position().y, "{grip:?}");
    // The whole model's own sockets stay put.
    assert_eq!(
        robot.socket("bottom").unwrap().position(),
        robot.model().socket("bottom").unwrap().position()
    );
    Ok(())
}

#[test]
fn a_file_from_before_the_tree_still_loads_and_stands_there() -> Result<(), LoadError> {
    let folder = TestFolder::new("duck-mold-animated-before");
    let old_files = folder.fixture("duck_mold_before_tree");
    let mut robot = AnimatedModel::load(old_files.join("robot.glb"))?;
    assert!(robot.animations().is_empty() && robot.poses().is_empty());
    let still = robot.mesh().vertices.clone();
    robot.play("walk");
    robot.turn("head", vec3(0.0, 90.0, 0.0)); // it has no joins to turn
    robot.update(1.0);
    assert_eq!(robot.mesh().vertices, still);
    assert!(robot.socket("head.top").is_some());
    Ok(())
}
