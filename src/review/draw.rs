//! The review's pictures: an overview of the whole run, filmstrips of
//! each flagged moment and event, what each app's viewer sees at each
//! caption, and the sound's spectrogram.

use super::checks::{text_contrast, text_on_profile, time};
use super::expect::Mark;
use super::frames::{Recorder, Small};
use super::profiles::Profile;
use super::truth::clip;
use super::{Finding, Run, Severity};
use crate::canvas::Canvas;
use crate::color::Color;
use crate::math::{Rect, vec2};
use std::path::Path;

const BACK: Color = Color::rgb(24, 24, 28);
const PLOT: Color = Color::rgb(40, 40, 46);
const INK: Color = Color::rgb(230, 230, 230);
const DIM: Color = Color::rgb(140, 140, 150);
const PLACE: Color = Color::rgb(80, 220, 240);
const EVENT: Color = Color::rgb(250, 220, 80);
const SOUND: Color = Color::rgb(255, 150, 60);
const FLAG: Color = Color::rgb(255, 70, 70);
const WARN: Color = Color::rgb(255, 170, 40);

/// `small` stretched to `width` x `height` (nearest pixel).
fn stretch(small: &Small, width: usize, height: usize) -> Canvas {
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        let sy = (y * small.height / height.max(1)).min(small.height.saturating_sub(1));
        for x in 0..width {
            let sx = (x * small.width / width.max(1)).min(small.width.saturating_sub(1));
            pixels.push(small.pixels[sy * small.width + sx]);
        }
    }
    Canvas::from_pixels(width, height, pixels)
}

/// A seconds step for time labels: at most about `most` of them.
fn tick_step(seconds: f64, most: f64) -> f64 {
    [
        0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0,
    ]
    .into_iter()
    .find(|s| seconds / s <= most)
    .unwrap_or(1200.0)
}

fn save(canvas: &Canvas, folder: &Path, name: &str, files: &mut Vec<String>) -> Result<(), String> {
    canvas
        .save_png(folder.join(name))
        .map_err(|e| format!("could not save {name}: {e}"))?;
    files.push(name.to_string());
    Ok(())
}

/// `overview.png`: a slice of every frame, and the brightness, change and
/// loudness over the whole run, with places, events, sounds and problems.
pub(crate) fn overview(
    rec: &Recorder,
    run: &Run,
    findings: &[Finding],
    folder: &Path,
    files: &mut Vec<String>,
) -> Result<(), String> {
    let n = rec.stats.len();
    if n == 0 {
        return Ok(());
    }
    let width = n.clamp(400, 1200);
    let left = 0;
    let index_at = |x: usize| (x * n / width).min(n - 1);
    let first = rec.stats[0].frame;
    let last = rec.stats[n - 1].frame;
    let x_of = |frame: u64| -> usize {
        let span = (last - first).max(1) as f64;
        ((frame.saturating_sub(first)) as f64 / span * (width - 1) as f64).round() as usize
    };
    let band = 18;
    let slice_h = 150;
    let plot_h = 64;
    let height = 26 + 14 + (band + slice_h) * 2 + (band + plot_h) * 3 + 24;
    let mut c = Canvas::new(width + left, height);
    c.clear(BACK);
    let seconds = n as f64 / run.ups;
    c.draw_text(
        &format!(
            "{}  {:.1} S  {}X{}",
            run.title.to_uppercase(),
            seconds,
            rec.width,
            rec.height
        ),
        vec2(4.0, 4.0),
        2,
        INK,
    );
    let mut y = 26;
    // Problems along the top.
    for f in findings {
        let color = match f.severity {
            Severity::Flag => FLAG,
            Severity::Warn => WARN,
            Severity::Note => continue,
        };
        let (a, b) = (x_of(f.frame), x_of(f.end));
        c.fill_rect(
            Rect::new(a as f32, y as f32, (b - a + 1).max(2) as f32, 12.0),
            color,
        );
    }
    y += 14;
    let top_of_slices = y;
    // Middle column of each frame.
    c.draw_text(
        "MIDDLE COLUMN OF EACH FRAME",
        vec2(4.0, (y + 3) as f32),
        2,
        DIM,
    );
    y += band;
    for x in 0..width {
        let column = &rec.columns[index_at(x)];
        for row in 0..slice_h {
            let p = column[(row * column.len() / slice_h).min(column.len() - 1)];
            c.set_pixel(x as i32, (y + row) as i32, Color::from_hex(p));
        }
    }
    y += slice_h;
    c.draw_text(
        "MIDDLE ROW OF EACH FRAME (LEFT AT TOP)",
        vec2(4.0, (y + 3) as f32),
        2,
        DIM,
    );
    y += band;
    for x in 0..width {
        let row_pixels = &rec.rows[index_at(x)];
        for row in 0..slice_h {
            let p = row_pixels[(row * row_pixels.len() / slice_h).min(row_pixels.len() - 1)];
            c.set_pixel(x as i32, (y + row) as i32, Color::from_hex(p));
        }
    }
    y += slice_h;
    // The plots.
    let plot =
        |c: &mut Canvas, y: usize, title: &str, value: &dyn Fn(usize) -> f32, line: Option<f32>| {
            c.draw_text(title, vec2(4.0, (y + 3) as f32), 2, DIM);
            let top = y + band;
            c.fill_rect(
                Rect::new(0.0, top as f32, width as f32, plot_h as f32),
                PLOT,
            );
            if let Some(l) = line {
                let ly = top + ((1.0 - l.clamp(0.0, 1.0)) * (plot_h - 1) as f32) as usize;
                for x in (0..width).step_by(4) {
                    c.set_pixel(x as i32, ly as i32, DIM);
                }
            }
            let mut previous: Option<usize> = None;
            for x in 0..width {
                let v = value(x).clamp(0.0, 1.0);
                let py = top + ((1.0 - v) * (plot_h - 1) as f32) as usize;
                let (a, b) = match previous {
                    Some(p) => (p.min(py), p.max(py)),
                    None => (py, py),
                };
                for yy in a..=b {
                    c.set_pixel(x as i32, yy as i32, INK);
                }
                previous = Some(py);
            }
        };
    plot(
        &mut c,
        y,
        "BRIGHTNESS (DOTS: TOO DARK)",
        &|x| rec.stats[index_at(x)].mean,
        Some(0.06),
    );
    y += band + plot_h;
    plot(
        &mut c,
        y,
        "CHANGE PER FRAME",
        &|x| rec.stats[index_at(x)].changed.sqrt(),
        None,
    );
    y += band + plot_h;
    let curve = run
        .audio
        .as_ref()
        .map(|a| super::audio::loudness(&a.mono, a.rate));
    let loud_top = y + band;
    match &curve {
        Some(curve) if !curve.is_empty() => {
            let value = |x: usize| {
                let frame = rec.stats[index_at(x)].frame;
                let tenth = ((frame as f64 / run.ups) * 10.0) as usize;
                (curve[tenth.min(curve.len() - 1)] + 60.0) / 60.0
            };
            plot(
                &mut c,
                y,
                "LOUDNESS (-60 TO 0 DB; DOTS: -50)",
                &value,
                Some(10.0 / 60.0),
            );
        }
        _ => {
            c.draw_text("NO SOUND", vec2(4.0, (y + 3) as f32), 2, DIM);
        }
    }
    y += band + plot_h;
    // Sound starts on the loudness plot.
    for s in &run.sounds {
        let x = x_of(s.frame) as f32;
        c.fill_rect(Rect::new(x, loud_top as f32, 1.0, 10.0), SOUND);
    }
    // Places through everything; events as ticks over the plots.
    for (frame, mark) in &run.marks {
        if *frame < first || *frame > last {
            continue;
        }
        let x = x_of(*frame) as f32;
        match mark {
            Mark::Place(name) => {
                c.fill_rect(
                    Rect::new(x, top_of_slices as f32, 1.0, (y - top_of_slices) as f32),
                    PLACE,
                );
                c.draw_text(
                    &name.to_uppercase(),
                    vec2(x + 3.0, (top_of_slices + 3) as f32),
                    1,
                    PLACE,
                );
            }
            Mark::Cut => {
                c.fill_rect(Rect::new(x, top_of_slices as f32, 1.0, 8.0), PLACE);
            }
            Mark::Event(_) => {
                let plots_top = y - 3 * (band + plot_h) + band;
                c.fill_rect(Rect::new(x, plots_top as f32, 1.0, 6.0), EVENT);
            }
            Mark::Intend(..) => {}
        }
    }
    // Time along the bottom.
    let step = tick_step(seconds, 12.0);
    let mut t = (first as f64 / run.ups / step).ceil() * step;
    while t <= last as f64 / run.ups {
        let x = x_of((t * run.ups).round() as u64) as f32;
        c.fill_rect(Rect::new(x, y as f32, 1.0, 5.0), DIM);
        c.draw_text(&format!("{t}S"), vec2(x + 2.0, (y + 7) as f32), 2, DIM);
        t += step;
    }
    save(&c, folder, "overview.png", files)
}

/// Filmstrips: every frame of each flagged moment (a few either side),
/// and of each marked event. Each finding gets the name of its strip.
pub(crate) fn filmstrips(
    rec: &Recorder,
    run: &Run,
    findings: &mut [Finding],
    folder: &Path,
    files: &mut Vec<String>,
) -> Result<(), String> {
    let ups = run.ups as f32;
    let strip = |from: usize, to: usize, caption: &dyn Fn(usize) -> String| -> Canvas {
        let frames: Vec<(u64, Canvas, String)> = (from..=to)
            .map(|i| (rec.stats[i].frame, rec.thumbs[i].to_canvas(), caption(i)))
            .collect();
        crate::pictures::contact_sheet_with_captions(&frames, ups)
    };
    let n = rec.stats.len();
    if n == 0 {
        return Ok(());
    }
    let mut made = 0;
    let mut by_frame: Vec<(u64, String)> = Vec::new();
    for f in findings.iter_mut() {
        if f.severity == Severity::Note && f.check != "speed" || made >= 16 {
            continue;
        }
        if let Some((_, name)) = by_frame.iter().find(|(frame, _)| *frame == f.frame) {
            f.pictures.push(name.clone());
            continue;
        }
        let (Some(a), Some(b)) = (rec.nearest(f.frame), rec.nearest(f.end)) else {
            continue;
        };
        let b = b.min(a + 23);
        let (from, to) = (a.saturating_sub(6), (b + 6).min(n - 1));
        let check = f.check.to_uppercase();
        let sheet = if f.check == "speed" {
            continue;
        } else {
            strip(from, to, &|i| {
                if i >= a && i <= b {
                    check.clone()
                } else {
                    String::new()
                }
            })
        };
        made += 1;
        let name = format!(
            "film-{made:02}-{}-{:.1}s.png",
            f.check.replace(' ', "-"),
            f.frame as f64 / run.ups
        );
        save(&sheet, folder, &name, files)?;
        f.pictures.push(name.clone());
        by_frame.push((f.frame, name));
    }
    // Events.
    let mut events = 0;
    for (frame, mark) in &run.marks {
        let Mark::Event(event) = mark else { continue };
        if events >= 16 {
            break;
        }
        let Some(i) = rec.index_of(*frame) else {
            continue;
        };
        events += 1;
        let (from, to) = (i.saturating_sub(8), (i + 8).min(n - 1));
        let sounds: Vec<(u64, &str)> = run
            .sounds
            .iter()
            .map(|s| (s.frame, s.name.as_str()))
            .collect();
        let sheet = strip(from, to, &|j| {
            let f = rec.stats[j].frame;
            let mut words = Vec::new();
            if f == *frame {
                words.push(event.clone());
            }
            words.extend(
                sounds
                    .iter()
                    .filter(|(sf, _)| *sf == f)
                    .map(|(_, n)| format!("+{n}")),
            );
            words.join(" ")
        });
        let safe: String = event
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let name = format!(
            "event-{events:02}-{safe}-{:.1}s.png",
            *frame as f64 / run.ups
        );
        save(&sheet, folder, &name, files)?;
    }
    // The slowest frames.
    if let Some(f) = findings
        .iter_mut()
        .find(|f| f.check == "speed" && f.severity == Severity::Note)
    {
        let mut costs: Vec<(usize, f32)> = (0..n)
            .map(|i| {
                let frame = rec.stats[i].frame as usize;
                let update = run
                    .update_ms
                    .get(frame.wrapping_sub(1))
                    .copied()
                    .unwrap_or(0.0);
                let draw = rec.truths[i].as_ref().map_or(0.0, |t| t.draw_ms);
                (i, update + draw)
            })
            .collect();
        costs.sort_by(|a, b| b.1.total_cmp(&a.1));
        let frames: Vec<(u64, Canvas, String)> = costs
            .iter()
            .take(6)
            .map(|&(i, ms)| {
                (
                    rec.stats[i].frame,
                    rec.thumbs[i].to_canvas(),
                    format!("{ms:.1} MS"),
                )
            })
            .collect();
        let sheet = crate::pictures::contact_sheet_with_captions(&frames, ups);
        save(&sheet, folder, "slowest.png", files)?;
        f.pictures.push(String::from("slowest.png"));
    }
    Ok(())
}

/// `viewer.png`: each caption moment as each matching app shows it, with
/// what the app covers shaded and each text outlined (green fine, red not).
pub(crate) fn viewer<'a>(
    rec: &'a Recorder,
    run: &Run,
    profiles: &[Profile],
    folder: &Path,
    files: &mut Vec<String>,
) -> Result<(), String> {
    if rec.kept.is_empty() || profiles.is_empty() {
        return Ok(());
    }
    let (w, h) = (rec.width as f32, rec.height as f32);
    // Moments with a problem first, then spread over the rest.
    let problem = |k: &super::frames::Kept| -> bool {
        let Some(truth) = &rec.truths[k.truth_index] else {
            return false;
        };
        truth.texts.iter().any(|t| {
            text_contrast(t).0 < 4.5
                || profiles.iter().any(|p| {
                    let (small, covered) = text_on_profile(clip(t.rect, w, h), p, w, h);
                    small.is_some() || !covered.is_empty()
                })
        })
    };
    // Up to 6, spread over the run: moments with a problem first.
    let spread =
        |from: Vec<&'a super::frames::Kept>, count: usize| -> Vec<&'a super::frames::Kept> {
            if from.len() <= count {
                return from;
            }
            (0..count)
                .map(|i| from[i * (from.len() - 1) / (count - 1).max(1)])
                .collect()
        };
    let mut chosen = spread(rec.kept.iter().filter(|k| problem(k)).collect(), 6);
    if chosen.len() < 6 {
        let rest: Vec<_> = rec
            .kept
            .iter()
            .filter(|k| !chosen.iter().any(|c| c.frame == k.frame))
            .collect();
        let more = spread(rest, 6 - chosen.len());
        chosen.extend(more);
    }
    chosen.sort_by_key(|k| k.frame);
    let profiles = &profiles[..profiles.len().min(4)];
    let tall = h >= w;
    let (pw, ph) = if tall {
        ((360.0 * w / h).round() as usize, 360)
    } else {
        (480, (480.0 * h / w).round() as usize)
    };
    let label = 30;
    let gap = 10;
    let width = gap + profiles.len() * (pw + gap);
    let height = gap + chosen.len() * (ph + label + gap);
    let mut c = Canvas::new(width, height);
    c.clear(BACK);
    let (sx, sy) = (pw as f32 / w, ph as f32 / h);
    for (row, kept) in chosen.iter().enumerate() {
        let Some(truth) = &rec.truths[kept.truth_index] else {
            continue;
        };
        for (col, profile) in profiles.iter().enumerate() {
            let left = gap + col * (pw + gap);
            let top = gap + row * (ph + label + gap);
            let name: String = profile.name.to_uppercase().chars().take(pw / 6).collect();
            c.draw_text(&name, vec2(left as f32, top as f32), 1, INK);
            c.draw_text(
                &time(kept.frame, run.ups).to_uppercase(),
                vec2(left as f32, (top + 12) as f32),
                1,
                DIM,
            );
            let picture_top = top + label;
            c.blit(&stretch(&kept.picture, pw, ph), left, picture_top);
            let place = |r: Rect| {
                Rect::new(
                    left as f32 + r.x * sx,
                    picture_top as f32 + r.y * sy,
                    r.w * sx,
                    r.h * sy,
                )
            };
            for (_, area, paused) in profile.covered_on(w, h) {
                let tint = if paused {
                    Color::rgba(255, 200, 0, 60)
                } else {
                    Color::rgba(255, 40, 40, 90)
                };
                c.fill_rect(place(area), tint);
            }
            for look in &truth.texts {
                if clip(look.rect, w, h).w < 1.0 || clip(look.rect, w, h).h < 1.0 {
                    continue; // off the picture
                }
                let (small, covered) = text_on_profile(clip(look.rect, w, h), profile, w, h);
                let fine = text_contrast(look).0 >= 4.5
                    && small.is_none()
                    && covered.iter().all(|(_, p)| *p);
                let r = place(clip(look.rect, w, h));
                let color = if fine { Color::rgb(60, 230, 90) } else { FLAG };
                c.draw_rect(Rect::new(r.x - 3.0, r.y - 3.0, r.w + 6.0, r.h + 6.0), color);
            }
        }
    }
    save(&c, folder, "viewer.png", files)
}

/// `spectrogram.png`: the sound's frequencies over time (low at the
/// bottom), with each sound's start marked and named.
pub(crate) fn spectrogram(run: &Run, folder: &Path, files: &mut Vec<String>) -> Result<(), String> {
    let Some(audio) = &run.audio else {
        return Ok(());
    };
    if audio.mono.is_empty() {
        return Ok(());
    }
    let seconds = audio.mono.len() as f64 / f64::from(audio.rate);
    let width = ((seconds * 20.0) as usize).clamp(300, 1200);
    let rows = 160;
    let picture = super::audio::spectrogram(&audio.mono, audio.rate, width, rows);
    let names_h = 60;
    let top = 22 + names_h;
    let mut c = Canvas::new(width, top + rows + 24);
    c.clear(BACK);
    c.draw_text(
        "SPECTROGRAM, 40 HZ (BOTTOM) TO 16 KHZ (TOP)",
        vec2(4.0, 6.0),
        1,
        DIM,
    );
    for (x, column) in picture.iter().enumerate() {
        for (r, &db) in column.iter().enumerate() {
            let v = ((db + 90.0) / 90.0).clamp(0.0, 1.0);
            // Dark blue, through purple and orange, to yellow.
            let color = Color::rgb(
                (255.0 * v.powf(0.8)) as u8,
                (255.0 * v.powf(2.2)) as u8,
                (120.0 * (1.0 - v) * v * 4.0).min(255.0) as u8 + (40.0 * (1.0 - v)) as u8,
            );
            c.set_pixel(x as i32, (top + rows - 1 - r) as i32, color);
        }
    }
    let x_of = |frame: u64| -> f32 {
        let t = frame.saturating_sub(1) as f64 / run.ups;
        (t / seconds * width as f64) as f32
    };
    let mut lanes = [f32::MIN; 6];
    for s in &run.sounds {
        let x = x_of(s.frame);
        c.fill_rect(Rect::new(x, top as f32, 1.0, 8.0), Color::WHITE);
        let words = s.name.to_uppercase();
        let w = Canvas::text_width(&words, 1);
        if let Some(lane) = lanes.iter().position(|&end| end < x) {
            c.draw_text(&words, vec2(x + 2.0, (22 + lane * 9) as f32), 1, SOUND);
            c.fill_rect(
                Rect::new(x, (22 + lane * 9) as f32, 1.0, (top - 22 - lane * 9) as f32),
                Color::rgba(255, 150, 60, 120),
            );
            lanes[lane] = x + w + 6.0;
        }
    }
    for (frame, mark) in &run.marks {
        if let Mark::Event(_) | Mark::Place(_) = mark {
            let color = if matches!(mark, Mark::Place(_)) {
                PLACE
            } else {
                EVENT
            };
            c.fill_rect(
                Rect::new(x_of(*frame), (top + rows - 8) as f32, 1.0, 8.0),
                color,
            );
        }
    }
    let step = tick_step(seconds, 12.0);
    let mut t = 0.0;
    while t <= seconds {
        let x = (t / seconds * width as f64) as f32;
        c.fill_rect(Rect::new(x, (top + rows) as f32, 1.0, 5.0), DIM);
        c.draw_text(
            &format!("{t}S"),
            vec2(x + 2.0, (top + rows + 8) as f32),
            2,
            DIM,
        );
        t += step;
    }
    save(&c, folder, "spectrogram.png", files)
}
