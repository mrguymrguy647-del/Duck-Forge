//! `--diff OLD_DIR NEW_DIR`: what changed between two headless runs.
//!
//! It answers "did my fix change only what I meant to change?" in one
//! command, without looking at every picture:
//!
//! - for each picture saved by both runs (`last.png`, `frame-*.png`, ...):
//!   how many pixels changed, and where; changed pictures get a copy in
//!   `NEW_DIR/diff/` with the changed pixels in bright magenta;
//! - for each scene file in both (`last.scene.json`, ...): which labels
//!   moved, appeared or disappeared;
//! - from `log.json`: reported values that ended differently.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;

use serde_json::Value;

use crate::canvas::Canvas;
use crate::color::Color;

/// Compares the two folders, saves the highlighted pictures, and returns
/// the report to print.
pub(crate) fn diff_runs(old: &Path, new: &Path) -> Result<String, String> {
    for folder in [old, new] {
        if !folder.is_dir() {
            return Err(format!(
                "--diff: {} isn't a folder; give the --out folders of two headless runs",
                folder.display()
            ));
        }
    }
    let old_files = names(old)?;
    let new_files = names(new)?;
    let out = new.join("diff");
    let _ = fs::remove_dir_all(&out);

    let mut lines = vec![format!(
        "comparing {} with {}",
        old.display(),
        new.display()
    )];
    let mut unchanged = Vec::new();
    for name in old_files.intersection(&new_files) {
        if !name.ends_with(".png") {
            continue;
        }
        let a = read_png(&old.join(name))?;
        let b = read_png(&new.join(name))?;
        if (a.width(), a.height()) != (b.width(), b.height()) {
            lines.push(format!(
                "{name}: the size changed, {}x{} to {}x{}",
                a.width(),
                a.height(),
                b.width(),
                b.height()
            ));
            continue;
        }
        let changed = compare(&a, &b);
        if changed.count == 0 {
            unchanged.push(name.clone());
            continue;
        }
        fs::create_dir_all(&out)
            .map_err(|err| format!("could not create {}: {err}", out.display()))?;
        let path = out.join(name);
        highlight(&b, &changed.mask)
            .save_png(&path)
            .map_err(|err| format!("could not save {}: {err}", path.display()))?;
        let (x0, y0, x1, y1) = changed.area;
        lines.push(format!(
            "{name}: {} pixels changed ({:.1}%), in {x0},{y0} {}x{}; see diff/{name}",
            changed.count,
            100.0 * changed.count as f64 / (a.width() * a.height()).max(1) as f64,
            x1 - x0 + 1,
            y1 - y0 + 1
        ));
    }
    if !unchanged.is_empty() {
        lines.push(format!("unchanged: {}", short_list(&unchanged)));
    }
    for name in old_files.intersection(&new_files) {
        if name.ends_with(".scene.json") {
            let moves = compare_scenes(&old.join(name), &new.join(name))?;
            if !moves.is_empty() {
                lines.push(format!("{name}: {}", moves.join("; ")));
            }
        }
    }
    if old_files.contains("log.json") && new_files.contains("log.json") {
        let (old_log, new_log) = (
            read_json(&old.join("log.json"))?,
            read_json(&new.join("log.json"))?,
        );
        let changes = compare_reports(&old_log, &new_log);
        if !changes.is_empty() {
            lines.push(format!("reported at the end: {}", changes.join(", ")));
        }
        if old_log["drawn_on_pictures"] != new_log["drawn_on_pictures"] {
            lines.push(format!(
                "note: the runs drew different extras on their pictures ({} and {}), so the pictures differ \
                 because of that too; run both with the same --grid and --debug",
                old_log["drawn_on_pictures"], new_log["drawn_on_pictures"]
            ));
        }
    }
    let only = |a: &BTreeSet<String>, b: &BTreeSet<String>| -> Vec<String> {
        a.difference(b)
            .filter(|n| n.ends_with(".png"))
            .cloned()
            .collect()
    };
    let only_old = only(&old_files, &new_files);
    let only_new = only(&new_files, &old_files);
    if !only_old.is_empty() {
        lines.push(format!("only in the old run: {}", short_list(&only_old)));
    }
    if !only_new.is_empty() {
        lines.push(format!("only in the new run: {}", short_list(&only_new)));
    }
    if lines.len() == 1 {
        lines.push(String::from(
            "no pictures or scenes to compare in both folders",
        ));
    }
    Ok(lines.join("\n"))
}

/// The file names in a folder.
fn names(folder: &Path) -> Result<BTreeSet<String>, String> {
    let entries = fs::read_dir(folder)
        .map_err(|err| format!("could not read {}: {err}", folder.display()))?;
    Ok(entries
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect())
}

/// A long list of names, cut down to a few.
fn short_list(names: &[String]) -> String {
    if names.len() <= 6 {
        names.join(", ")
    } else {
        format!("{} and {} more", names[..5].join(", "), names.len() - 5)
    }
}

/// Which pixels differ between two pictures of the same size.
struct Changed {
    count: usize,
    mask: Vec<bool>,
    /// The smallest box around every change: left, top, right, bottom.
    area: (usize, usize, usize, usize),
}

fn compare(a: &Canvas, b: &Canvas) -> Changed {
    let width = a.width();
    let mut changed = Changed {
        count: 0,
        mask: vec![false; a.pixels().len()],
        area: (usize::MAX, usize::MAX, 0, 0),
    };
    for (i, (pa, pb)) in a.pixels().iter().zip(b.pixels()).enumerate() {
        if pa != pb {
            let (x, y) = (i % width, i / width);
            changed.count += 1;
            changed.mask[i] = true;
            let area = &mut changed.area;
            *area = (area.0.min(x), area.1.min(y), area.2.max(x), area.3.max(y));
        }
    }
    changed
}

/// The new picture, darkened and gray, with the changed pixels bright
/// magenta, so they stand out at a glance.
fn highlight(picture: &Canvas, mask: &[bool]) -> Canvas {
    let pixels = picture
        .pixels()
        .iter()
        .zip(mask)
        .map(|(&p, &changed)| {
            if changed {
                Color::rgb(255, 0, 255).to_u32()
            } else {
                let c = Color::from_hex(p);
                let gray = ((c.r as u32 + c.g as u32 + c.b as u32) / 3 / 3) as u8; // a third as bright
                Color::rgb(gray, gray, gray).to_u32()
            }
        })
        .collect();
    Canvas::from_pixels(picture.width(), picture.height(), pixels)
}

/// Reads a PNG saved by a headless run (8-bit RGB or RGBA).
pub(crate) fn read_png(path: &Path) -> Result<Canvas, String> {
    let bad = |problem: &dyn std::fmt::Display| {
        format!("couldn't read {} as a PNG ({problem})", path.display())
    };
    let file = File::open(path).map_err(|e| bad(&e))?;
    let mut decoder = png::Decoder::new(BufReader::new(file));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| bad(&e))?;
    let mut bytes = vec![0; reader.output_buffer_size().ok_or_else(|| bad(&"too big"))?];
    let info = reader.next_frame(&mut bytes).map_err(|e| bad(&e))?;
    let channels = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Indexed => return Err(bad(&"a palette image")),
    };
    let (width, height) = (info.width as usize, info.height as usize);
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        for p in bytes[y * info.line_size..][..width * channels].chunks_exact(channels) {
            let (r, g, b) = if channels >= 3 {
                (p[0], p[1], p[2])
            } else {
                (p[0], p[0], p[0])
            };
            pixels.push(Color::rgb(r, g, b).to_u32());
        }
    }
    Ok(Canvas::from_pixels(width, height, pixels))
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path)
        .map_err(|err| format!("could not read {}: {err}", path.display()))?;
    serde_json::from_str(&text).map_err(|err| format!("{} isn't valid JSON: {err}", path.display()))
}

/// Labels that moved, appeared or disappeared between two scene files.
/// Labels with the same name are matched in the order they were drawn.
/// A move of less than one pixel on screen (the world move times the
/// camera's zoom) doesn't count: the pictures can't show it.
fn compare_scenes(old: &Path, new: &Path) -> Result<Vec<String>, String> {
    let labels = |scene: &Value| -> BTreeMap<String, Vec<(f64, f64)>> {
        let mut by_name: BTreeMap<String, Vec<(f64, f64)>> = BTreeMap::new();
        for label in scene["labels"].as_array().into_iter().flatten() {
            let name = label["name"].as_str().unwrap_or("?").to_string();
            let x = label["world"][0].as_f64().unwrap_or(0.0);
            let y = label["world"][1].as_f64().unwrap_or(0.0);
            by_name.entry(name).or_default().push((x, y));
        }
        by_name
    };
    let new_scene = read_json(new)?;
    let zoom = new_scene["camera"]["zoom"]
        .as_f64()
        .unwrap_or(1.0)
        .abs()
        .max(0.0001);
    let (a, b) = (labels(&read_json(old)?), labels(&new_scene));
    let mut notes = Vec::new();
    let names: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    for name in names {
        let empty = Vec::new();
        let (before, after) = (a.get(name).unwrap_or(&empty), b.get(name).unwrap_or(&empty));
        if before.len() != after.len() {
            notes.push(format!("{name} x{} to x{}", before.len(), after.len()));
            continue;
        }
        let moved: Vec<String> = before
            .iter()
            .zip(after)
            .filter(|(p, q)| (p.0 - q.0).abs() * zoom >= 1.0 || (p.1 - q.1).abs() * zoom >= 1.0)
            .map(|(p, q)| format!("{:.0},{:.0} to {:.0},{:.0}", p.0, p.1, q.0, q.1))
            .collect();
        match moved.len() {
            0 => {}
            1 if before.len() == 1 => notes.push(format!("{name} moved {}", moved[0])),
            n => notes.push(format!("{n} of {} {name} moved", before.len())),
        }
    }
    Ok(notes)
}

/// Reported values whose final value changed, like `score 10 to 12`.
fn compare_reports(a: &Value, b: &Value) -> Vec<String> {
    let empty = serde_json::Map::new();
    let (a, b) = (
        a["final"].as_object().unwrap_or(&empty),
        b["final"].as_object().unwrap_or(&empty),
    );
    let names: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    names
        .into_iter()
        .filter(|name| a.get(*name) != b.get(*name))
        .map(|name| {
            let show = |v: Option<&Value>| v.map_or(String::from("(none)"), |v| v.to_string());
            format!("{name} {} to {}", show(a.get(name)), show(b.get(name)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Rect;

    fn folder(name: &str) -> crate::test_folder::TestFolder {
        crate::test_folder::TestFolder::new(&format!("diff-{name}"))
    }

    fn picture(square_x: f32) -> Canvas {
        let mut canvas = Canvas::new(40, 20);
        canvas.fill_rect(Rect::new(square_x, 5.0, 4.0, 4.0), Color::WHITE);
        canvas
    }

    #[test]
    fn diffs_pictures_scenes_and_reports() {
        let (old, new) = (folder("old"), folder("new"));
        picture(5.0).save_png(old.join("last.png")).unwrap();
        picture(9.0).save_png(new.join("last.png")).unwrap();
        picture(5.0).save_png(old.join("frame-000010.png")).unwrap();
        picture(5.0).save_png(new.join("frame-000010.png")).unwrap();
        picture(5.0).save_png(old.join("frame-000020.png")).unwrap();
        let scene = |x: f32, rocks: usize| {
            let mut labels = vec![format!(r#"{{"name": "duck", "world": [{x}, 5, 4, 4]}}"#)];
            labels.extend(
                (0..rocks).map(|i| format!(r#"{{"name": "rock", "world": [{i}, 0, 1, 1]}}"#)),
            );
            format!(r#"{{"frame": 30, "labels": [{}]}}"#, labels.join(", "))
        };
        fs::write(old.join("last.scene.json"), scene(5.0, 2)).unwrap();
        fs::write(new.join("last.scene.json"), scene(9.0, 3)).unwrap();
        fs::write(
            old.join("log.json"),
            r#"{"final": {"score": 10, "lives": 3}, "drawn_on_pictures": {"grid": null}}"#,
        )
        .unwrap();
        fs::write(
            new.join("log.json"),
            r#"{"final": {"score": 12, "lives": 3}, "drawn_on_pictures": {"grid": 32}}"#,
        )
        .unwrap();

        let report = diff_runs(&old, &new).unwrap();
        let lines: Vec<&str> = report.lines().collect();
        assert_eq!(
            &lines[1..],
            [
                // The square moved 4 pixels: 16 pixels left, 16 arrived, minus none overlapping.
                "last.png: 32 pixels changed (4.0%), in 5,5 8x4; see diff/last.png",
                "unchanged: frame-000010.png",
                "last.scene.json: duck moved 5,5 to 9,5; rock x2 to x3",
                "reported at the end: score 10 to 12",
                "note: the runs drew different extras on their pictures ({\"grid\":null} and {\"grid\":32}), so the pictures differ because of that too; run both with the same --grid and --debug",
                "only in the old run: frame-000020.png",
            ],
            "{report}"
        );
        let highlighted = read_png(&new.join("diff/last.png")).unwrap();
        assert_eq!(
            highlighted.get_pixel(5, 5),
            Some(Color::rgb(255, 0, 255)),
            "changed: magenta"
        );
        assert_eq!(
            highlighted.get_pixel(0, 0),
            Some(Color::BLACK),
            "unchanged: dimmed"
        );
        assert!(
            !new.join("diff/frame-000010.png").exists(),
            "no picture for an unchanged one"
        );
    }

    #[test]
    fn moves_smaller_than_a_pixel_are_ignored() {
        let (old, new) = (folder("sub-old"), folder("sub-new"));
        let scene = |frog_x: f32, zoom: f32| {
            format!(
                r#"{{"frame": 150, "camera": {{"x": 0, "y": 0, "zoom": {zoom}}}, "labels": [
                    {{"name": "frog", "world": [{frog_x}, 5, 4, 4]}}, {{"name": "frog", "world": [50, 5, 4, 4]}}]}}"#
            )
        };
        fs::write(old.join("frame-000150.scene.json"), scene(10.0, 1.0)).unwrap();
        fs::write(new.join("frame-000150.scene.json"), scene(10.4, 1.0)).unwrap();
        assert!(
            !diff_runs(&old, &new).unwrap().contains("frog"),
            "0.4 pixels: not a move"
        );
        fs::write(new.join("frame-000150.scene.json"), scene(10.4, 4.0)).unwrap();
        let report = diff_runs(&old, &new).unwrap();
        assert!(
            report.contains("1 of 2 frog moved"),
            "0.4 world units at zoom 4 is 1.6 pixels: {report}"
        );
    }

    #[test]
    fn a_missing_folder_is_one_sentence() {
        let error = diff_runs(Path::new("no/such/run"), Path::new(".")).unwrap_err();
        assert!(error.contains("no/such/run isn't a folder"), "{error}");
    }
}
