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
use duckforge::glb::GlbModel;
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
