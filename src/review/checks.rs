//! The checks: each looks at the recorded frames, the engine's truth and
//! the sound, and says what a viewer would notice, with times.

use super::audio::SoundStart;
use super::expect::{Expect, Mark, What, When};
use super::frames::Recorder;
use super::profiles::Profile;
use super::truth::{TextLook, Thing, clip, color_luminance, contrast};
use super::{Finding, Outcome, Run, Severity};
use crate::math::Rect;
use std::collections::BTreeMap;

/// What every check needs.
pub(crate) struct Checking<'a> {
    pub(crate) rec: &'a Recorder,
    pub(crate) run: &'a Run,
    pub(crate) profiles: &'a [Profile],
    /// Frame ranges whose camera motion is meant: start, end, what.
    intended: Vec<(u64, u64, String)>,
    /// Frames where a cut (or a change of place) is marked.
    cuts: Vec<u64>,
}

/// Seconds, as the review writes them.
pub(crate) fn time(frame: u64, ups: f64) -> String {
    format!("{:.2} s", frame as f64 / ups)
}

fn median(values: &mut [f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f32::total_cmp);
    values[values.len() / 2]
}

/// Runs of consecutive indexes where `bad` holds: (first, last).
fn stretches(count: usize, bad: impl Fn(usize) -> bool) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for i in 0..=count {
        let is_bad = i < count && bad(i);
        match (is_bad, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s, i - 1));
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// The angle from `a` to `b`, the short way round.
fn turn(a: f32, b: f32) -> f32 {
    let mut d = (b - a) % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    } else if d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    d
}

/// How readable a line of text is: its contrast with what's around its
/// letters, and what that was measured against.
pub(crate) fn text_contrast(look: &TextLook) -> (f32, &'static str) {
    let text = color_luminance(look.color);
    // What's behind: the box mixed over the pixels, or the pixels.
    let behind = |l: f32| match look.box_color {
        Some(b) => {
            let a = b.a as f32 / 255.0;
            color_luminance(b) * a + l * (1.0 - a)
        }
        None => l,
    };
    let worst = look
        .behind
        .iter()
        .map(|&l| contrast(text, behind(l)))
        .fold(f32::INFINITY, f32::min);
    let mut best = (
        worst,
        if look.box_color.is_some() {
            "its box"
        } else {
            "what's behind it"
        },
    );
    if let Some((outline, width)) = look.outline {
        if width >= 1.0 && outline.a >= 128 {
            let c = contrast(text, color_luminance(outline));
            if c > best.0 {
                best = (c, "its outline");
            }
        }
    }
    if let Some(shadow) = look.shadow {
        if shadow.a >= 128 {
            // A shadow helps, but less than an outline: it's on one side.
            let c = contrast(text, color_luminance(shadow)).min(4.5);
            if c > best.0 {
                best = (c, "its shadow");
            }
        }
    }
    best
}

/// How a line of text fares on one profile: too small (the size it
/// should be), and what covers it (name, paused only).
pub(crate) fn text_on_profile(
    rect: Rect,
    profile: &Profile,
    width: f32,
    height: f32,
) -> (Option<f32>, Vec<(String, bool)>) {
    let small = (rect.h < profile.min_text * height).then_some(profile.min_text * height);
    let area = (rect.w * rect.h).max(1.0);
    let covered = profile
        .covered_on(width, height)
        .into_iter()
        .filter(|(_, r, _)| {
            let x0 = rect.x.max(r.x);
            let y0 = rect.y.max(r.y);
            let x1 = (rect.x + rect.w).min(r.x + r.w);
            let y1 = (rect.y + rect.h).min(r.y + r.h);
            (x1 - x0).max(0.0) * (y1 - y0).max(0.0) / area > 0.15
        })
        .map(|(name, _, paused)| (name, paused))
        .collect();
    (small, covered)
}

impl<'a> Checking<'a> {
    pub(crate) fn new(rec: &'a Recorder, run: &'a Run, profiles: &'a [Profile]) -> Checking<'a> {
        let ups = run.ups;
        let mut intended = Vec::new();
        let mut cuts = Vec::new();
        for (frame, mark) in &run.marks {
            match mark {
                Mark::Intend(what, seconds) => {
                    let end = frame + (f64::from(*seconds) * ups).ceil() as u64;
                    intended.push((*frame, end, what.clone()));
                }
                Mark::Cut | Mark::Place(_) => cuts.push(*frame),
                Mark::Event(_) => {}
            }
        }
        Checking {
            rec,
            run,
            profiles,
            intended,
            cuts,
        }
    }

    fn t(&self, frame: u64) -> String {
        time(frame, self.run.ups)
    }

    fn frame(&self, i: usize) -> u64 {
        self.rec.stats[i].frame
    }

    fn is_intended(&self, frame: u64) -> Option<&str> {
        self.intended
            .iter()
            .find(|(s, e, _)| frame >= *s && frame <= *e)
            .map(|(.., w)| w.as_str())
    }

    fn near_cut(&self, frame: u64) -> bool {
        self.cuts.iter().any(|&c| c.abs_diff(frame) <= 1)
    }

    fn finding(
        &self,
        severity: Severity,
        check: &'static str,
        first: usize,
        last: usize,
        text: String,
    ) -> Finding {
        Finding {
            severity,
            check,
            frame: self.frame(first),
            end: self.frame(last),
            text,
            pictures: Vec::new(),
            replay: None,
        }
    }

    /// Every check, then every expectation.
    pub(crate) fn all(&self) -> (Vec<Finding>, Vec<Outcome>) {
        let mut found = Vec::new();
        self.text(&mut found);
        self.visibility(&mut found);
        self.motion(&mut found);
        self.light(&mut found);
        self.sound(&mut found);
        if self.run.game {
            self.response(&mut found);
            self.speed(&mut found);
            self.stuck(&mut found);
        }
        let outcomes = self
            .run
            .expectations
            .iter()
            .map(|e| self.expectation(e))
            .collect();
        (found, outcomes)
    }

    // ---- text ---------------------------------------------------------

    fn text(&self, found: &mut Vec<Finding>) {
        let (w, h) = (self.rec.width as f32, self.rec.height as f32);
        // Each text, from when it appears until it goes: how it looked on
        // each of its frames.
        #[derive(Default)]
        struct Seen {
            first: usize,
            last: usize,
            frames: usize,
            contrasts: Vec<(f32, &'static str)>,
            heights: Vec<f32>,
            /// (screen, wanted px): frames too small.
            small: BTreeMap<String, (usize, f32)>,
            /// (profile, area): frames covered, first, paused only.
            covered: BTreeMap<(String, String), (usize, usize, bool)>,
        }
        let mut open: BTreeMap<String, Seen> = BTreeMap::new();
        let mut done: Vec<(String, Seen)> = Vec::new();
        for (i, truth) in self.rec.truths.iter().enumerate() {
            let texts: &[TextLook] = truth.as_ref().map_or(&[], |t| &t.texts);
            let words: Vec<&str> = texts.iter().map(|t| t.text.as_str()).collect();
            let gone: Vec<String> = open
                .keys()
                .filter(|k| !words.contains(&k.as_str()))
                .cloned()
                .collect();
            for key in gone {
                let seen = open.remove(&key).expect("open");
                done.push((key, seen));
            }
            for look in texts {
                let seen = open.entry(look.text.clone()).or_insert_with(|| Seen {
                    first: i,
                    ..Seen::default()
                });
                if seen.last == i && seen.frames > 0 {
                    continue; // the same words twice in a frame: judged once
                }
                seen.last = i;
                seen.frames += 1;
                seen.contrasts.push(text_contrast(look));
                seen.heights.push(look.rect.h);
                let rect = clip(look.rect, w, h);
                if rect.w * rect.h <= 0.0 {
                    continue; // off screen (another check's business)
                }
                for profile in self.profiles {
                    let (small, covered) = text_on_profile(rect, profile, w, h);
                    if let Some(want) = small {
                        let e = seen
                            .small
                            .entry(profile.screen.word().to_string())
                            .or_insert((0, want));
                        e.0 += 1;
                    }
                    for (area, paused) in covered {
                        let e = seen
                            .covered
                            .entry((profile.name.clone(), area))
                            .or_insert((0, i, paused));
                        e.0 += 1;
                    }
                }
            }
        }
        done.extend(open);
        // A problem counts when it lasts a third of the text's time on
        // screen, or a second: not while the text flies in.
        let second = self.run.ups.round() as usize;
        let lasting = |frames: usize, total: usize| frames * 3 >= total || frames >= second;
        for (words, seen) in done {
            let quoted = shorten(&words);
            let total = seen.frames.max(1);
            let mut bad: Vec<(f32, &'static str)> = seen
                .contrasts
                .iter()
                .copied()
                .filter(|c| c.0 < 4.5)
                .collect();
            if !bad.is_empty() && lasting(bad.len(), total) {
                bad.sort_by(|a, b| a.0.total_cmp(&b.0));
                let (c, basis) = bad[bad.len() / 2];
                let severity = if c < 3.0 {
                    Severity::Flag
                } else {
                    Severity::Warn
                };
                found.push(self.finding(
                    severity,
                    "text contrast",
                    seen.first,
                    seen.last,
                    format!(
                        "{quoted}: contrast {c:.1}:1 against {basis} (needs 4.5:1, at least 3:1); \
                         give it an outline or a box"
                    ),
                ));
            }
            let mut heights = seen.heights.clone();
            let px = median(&mut heights);
            for (screen, (frames, want)) in seen.small {
                if lasting(frames, total) && px < want {
                    let severity = if px < want * 0.7 {
                        Severity::Flag
                    } else {
                        Severity::Warn
                    };
                    found.push(self.finding(
                        severity,
                        "text size",
                        seen.first,
                        seen.last,
                        format!("{quoted} is {px:.0} px tall: too small on a {screen} (at least {want:.0} px here)"),
                    ));
                }
            }
            // Group the apps that cover it the same way.
            let mut by_area: BTreeMap<(String, bool), (usize, Vec<String>)> = BTreeMap::new();
            for ((profile, area), (frames, first, paused)) in seen.covered {
                if !lasting(frames, total) {
                    continue;
                }
                let entry = by_area.entry((area, paused)).or_insert((first, Vec::new()));
                entry.0 = entry.0.min(first);
                entry.1.push(profile);
            }
            for ((area, paused), (i, apps)) in by_area {
                let severity = if paused {
                    Severity::Warn
                } else {
                    Severity::Flag
                };
                found.push(self.finding(
                    severity,
                    "text covered",
                    i,
                    seen.last.max(i),
                    format!("{quoted} is under {} {area}", possessive(&apps)),
                ));
            }
        }
    }

    // ---- visibility ---------------------------------------------------

    fn visibility(&self, found: &mut Vec<Finding>) {
        let h = self.rec.height as f32;
        let expected: Vec<&str> = self
            .run
            .expectations
            .iter()
            .filter_map(|e| match &e.what {
                What::Thing { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        let mut names: Vec<String> = Vec::new();
        for truth in self.rec.truths.iter().flatten() {
            for thing in &truth.things {
                if (thing.world_box.is_some() || expected.contains(&thing.name.as_str()))
                    && !names.contains(&thing.name)
                {
                    names.push(thing.name.clone());
                }
            }
        }
        let long = (self.run.ups as usize).max(1); // a second
        for name in names {
            let thing_at = |i: usize| -> Option<&Thing> {
                self.rec.truths[i]
                    .as_ref()?
                    .things
                    .iter()
                    .find(|t| t.name == name)
            };
            let count = self.rec.truths.len();
            let tiny = |i: usize| {
                thing_at(i).is_some_and(|t| t.on_screen() && t.visible.h < (0.03 * h).max(12.0))
            };
            for (a, b) in stretches(count, tiny) {
                if b - a + 1 >= long {
                    let px = thing_at(a).map_or(0.0, |t| t.visible.h);
                    found.push(self.finding(
                        Severity::Warn,
                        "visibility",
                        a,
                        b,
                        format!(
                            "\"{name}\" is tiny ({px:.0} px tall) for {:.1} s",
                            (b - a + 1) as f64 / self.run.ups
                        ),
                    ));
                }
            }
            let dark = |i: usize| {
                thing_at(i).is_some_and(|t| t.on_screen() && t.visible.h >= 4.0 && t.light < 0.06)
            };
            for (a, b) in stretches(count, dark) {
                if b - a + 1 >= long {
                    let light = thing_at(a).map_or(0.0, |t| t.light);
                    found.push(self.finding(
                        Severity::Warn,
                        "visibility",
                        a,
                        b,
                        format!(
                            "\"{name}\" is too dark to see (brightness {light:.2}) for {:.1} s",
                            (b - a + 1) as f64 / self.run.ups
                        ),
                    ));
                }
            }
        }
    }

    // ---- motion -------------------------------------------------------

    fn motion(&self, found: &mut Vec<Finding>) {
        let stats = &self.rec.stats;
        let n = stats.len();
        let cameras: Vec<Option<crate::render3d::Camera3D>> = self
            .rec
            .truths
            .iter()
            .map(|t| t.as_ref().and_then(|t| t.camera))
            .collect();
        let has_cameras = cameras.iter().filter(|c| c.is_some()).count() > n / 2;
        if has_cameras {
            // Each frame's move: x, y, z, yaw, pitch, and the distance.
            let delta = |i: usize| -> Option<[f32; 6]> {
                let (a, b) = (cameras[i.checked_sub(1)?]?, cameras[i]?);
                let d = b.position - a.position;
                Some([
                    d.x,
                    d.y,
                    d.z,
                    turn(a.yaw, b.yaw),
                    b.pitch - a.pitch,
                    d.length(),
                ])
            };
            let deltas: Vec<Option<[f32; 6]>> = (0..n).map(delta).collect();
            // Sudden jumps that aren't cuts.
            for i in 1..n {
                let Some(d) = deltas[i] else { continue };
                let frame = self.frame(i);
                if self.near_cut(frame) || self.is_intended(frame).is_some() {
                    continue;
                }
                let before: Vec<[f32; 6]> = deltas[i.saturating_sub(30)..i]
                    .iter()
                    .flatten()
                    .copied()
                    .collect();
                let mut moves: Vec<f32> = before.iter().map(|d| d[5]).collect();
                let mut turns: Vec<f32> =
                    before.iter().map(|d| d[3].abs().max(d[4].abs())).collect();
                let (usual_move, usual_turn) = (median(&mut moves), median(&mut turns));
                let turned = d[3].abs().max(d[4].abs());
                if d[5] > 8.0 * usual_move + 0.25 && d[5] > 0.5 {
                    found.push(self.finding(
                        Severity::Flag,
                        "camera jump",
                        i,
                        i,
                        format!(
                            "the camera jumps {:.2} in one frame (it usually moves {usual_move:.3}), and no cut is marked \
                             (ctx.cut() if it's meant)",
                            d[5]
                        ),
                    ));
                } else if turned > 8.0 * usual_turn + 0.05 && turned > 0.35 {
                    found.push(self.finding(
                        Severity::Flag,
                        "camera jump",
                        i,
                        i,
                        format!(
                            "the camera turns {:.0} degrees in one frame, and no cut is marked",
                            turned.to_degrees()
                        ),
                    ));
                }
            }
            // Jitter: the direction flipping almost every frame.
            const WINDOW: usize = 12;
            let shaky = |i: usize| -> bool {
                if i < WINDOW {
                    return false;
                }
                let window: Vec<[f32; 6]> = deltas[i + 1 - WINDOW..=i]
                    .iter()
                    .flatten()
                    .copied()
                    .collect();
                if window.len() < WINDOW {
                    return false;
                }
                (0..5).any(|axis| {
                    let eps = 1e-4;
                    window
                        .windows(2)
                        .filter(|p| {
                            p[0][axis].abs() > eps
                                && p[1][axis].abs() > eps
                                && (p[0][axis] > 0.0) != (p[1][axis] > 0.0)
                        })
                        .count()
                        >= 8
                })
            };
            for (a, b) in stretches(n, shaky) {
                let first = a.saturating_sub(WINDOW - 1);
                // Meant (ctx.intend) for most of it: not a mistake.
                let meant = (a..=b)
                    .filter(|&i| self.is_intended(self.frame(i)).is_some())
                    .count();
                if meant * 2 > b - a {
                    continue;
                }
                found.push(self.finding(
                    Severity::Flag,
                    "camera jitter",
                    first,
                    b,
                    format!(
                        "the camera shakes (its direction flips almost every frame) for {:.2} s; \
                         smooth it, or mark it with ctx.intend(\"shake\", seconds)",
                        (b - first + 1) as f64 / self.run.ups
                    ),
                ));
            }
        } else {
            // A video from elsewhere: sudden changes are cuts or jumps.
            for i in 1..n {
                let mut around: Vec<f32> = (i.saturating_sub(5)..(i + 6).min(n))
                    .filter(|&j| j != i)
                    .map(|j| stats[j].change)
                    .collect();
                let usual = median(&mut around);
                if stats[i].change > 0.15 && stats[i].change > 5.0 * usual + 0.02 {
                    found.push(self.finding(
                        Severity::Note,
                        "cut or jump",
                        i,
                        i,
                        format!(
                            "the picture changes all at once ({:.0}% of it): a cut or a jump",
                            stats[i].changed * 100.0
                        ),
                    ));
                }
            }
        }
        // Frozen stretches.
        let still = |i: usize| i > 0 && stats[i].changed < 0.001;
        for (a, b) in stretches(n, still) {
            let seconds = (b - a + 1) as f64 / self.run.ups;
            if seconds >= 2.0 && self.is_intended(self.frame(a)).is_none() {
                let severity = if seconds > 5.0 {
                    Severity::Warn
                } else {
                    Severity::Note
                };
                found.push(self.finding(
                    severity,
                    "frozen",
                    a,
                    b,
                    format!("nothing moves for {seconds:.1} s"),
                ));
            }
        }
        // Flicker: brightness going up and down every frame.
        let flips = |i: usize| -> bool {
            if i < 2 {
                return false;
            }
            let (d0, d1) = (
                stats[i - 1].mean - stats[i - 2].mean,
                stats[i].mean - stats[i - 1].mean,
            );
            d0.abs() > 0.03 && d1.abs() > 0.03 && (d0 > 0.0) != (d1 > 0.0)
        };
        for (a, b) in stretches(n, flips) {
            if b - a + 1 >= 4 && self.is_intended(self.frame(a)).is_none() {
                found.push(self.finding(
                    Severity::Flag,
                    "flicker",
                    a.saturating_sub(2),
                    b,
                    format!(
                        "the brightness flickers up and down every frame for {} frames (hard to watch, and a risk \
                         for people sensitive to flashing)",
                        b - a + 3
                    ),
                ));
            }
        }
        // The camera inside something.
        let truths = &self.rec.truths;
        let inside_block = |i: usize| truths[i].as_ref().is_some_and(|t| !t.inside.is_empty());
        for (a, b) in stretches(n, inside_block) {
            let what = truths[a]
                .as_ref()
                .map(|t| t.inside[0].clone())
                .unwrap_or_default();
            found.push(self.finding(
                Severity::Flag,
                "camera inside",
                a,
                b,
                format!("the camera is inside {what} ({} frames)", b - a + 1),
            ));
        }
        let inside_model = |i: usize| -> Option<String> {
            let t = truths[i].as_ref()?;
            let eye = t.camera?.position;
            let flat = stats[i].spread < 0.04 || stats[i].dark > 0.85;
            if !flat {
                return None;
            }
            t.things.iter().find_map(|thing| {
                let (lo, hi) = thing.world_box?;
                (eye.x > lo.x
                    && eye.x < hi.x
                    && eye.y > lo.y
                    && eye.y < hi.y
                    && eye.z > lo.z
                    && eye.z < hi.z)
                    .then(|| thing.name.clone())
            })
        };
        for (a, b) in stretches(n, |i| inside_model(i).is_some()) {
            let name = inside_model(a).unwrap_or_default();
            found.push(self.finding(
                Severity::Flag,
                "camera inside",
                a,
                b,
                format!("the camera is inside the model \"{name}\": the picture is almost one flat colour"),
            ));
        }
        // Almost black for a moment, with nothing known to explain it.
        let black = |i: usize| stats[i].dark > 0.92;
        for (a, b) in stretches(n, black) {
            let short = ((b - a + 1) as f64) < 1.5 * self.run.ups;
            let lit_before = a > 0 && stats[a.saturating_sub(10)].dark < 0.5;
            let lit_after = b + 1 < n && stats[(b + 10).min(n - 1)].dark < 0.5;
            let explained = (a..=b).any(|i| inside_block(i) || inside_model(i).is_some())
                || self.near_cut(self.frame(a))
                || self.is_intended(self.frame(a)).is_some();
            if short && lit_before && lit_after && !explained {
                found.push(self.finding(
                    Severity::Warn,
                    "almost black",
                    a,
                    b,
                    format!(
                        "the picture goes almost black for {} frames: the camera may be inside something",
                        b - a + 1
                    ),
                ));
            }
        }
    }

    // ---- light --------------------------------------------------------

    fn light(&self, found: &mut Vec<Finding>) {
        let stats = &self.rec.stats;
        let n = stats.len();
        let long = 2.0 * self.run.ups;
        for (a, b) in stretches(n, |i| stats[i].mean < 0.06) {
            if (b - a + 1) as f64 > long && self.is_intended(self.frame(a)).is_none() {
                found.push(self.finding(
                    Severity::Warn,
                    "too dark",
                    a,
                    b,
                    format!(
                        "very dark for {:.1} s (average brightness {:.2}): on a phone outdoors it's black",
                        (b - a + 1) as f64 / self.run.ups,
                        stats[a..=b].iter().map(|s| s.mean).sum::<f32>() / (b - a + 1) as f32
                    ),
                ));
            }
        }
        for (a, b) in stretches(n, |i| stats[i].bright > 0.5) {
            if (b - a + 1) as f64 > long && self.is_intended(self.frame(a)).is_none() {
                found.push(self.finding(
                    Severity::Warn,
                    "blown out",
                    a,
                    b,
                    format!(
                        "blown out (over half the picture pure white) for {:.1} s",
                        (b - a + 1) as f64 / self.run.ups
                    ),
                ));
            }
        }
    }

    // ---- sound --------------------------------------------------------

    /// The frame `sound` is first heard on.
    fn onset(&self, sound: &SoundStart) -> f64 {
        sound.frame as f64 + f64::from(sound.lead) * self.run.ups
    }

    fn event_frames(&self, event: &str) -> Vec<u64> {
        self.run
            .marks
            .iter()
            .filter(|(_, m)| matches!(m, Mark::Event(e) if e == event))
            .map(|(f, _)| *f)
            .collect()
    }

    fn sound(&self, found: &mut Vec<Finding>) {
        let ups = self.run.ups;
        let at = |seconds: f64| -> usize {
            self.rec
                .nearest((seconds * ups).round() as u64)
                .unwrap_or(0)
        };
        if let Some(audio) = &self.run.audio {
            for peak in &audio.peaks {
                let names: Vec<String> = peak
                    .sounds
                    .iter()
                    .take(4)
                    .map(|(n, looped, v, _)| {
                        format!("{n}{} {v:.2}", if *looped { " (loop)" } else { "" })
                    })
                    .collect();
                let fix: Vec<String> = peak
                    .suggestion
                    .iter()
                    .map(|(n, _, _, to)| format!("{n} at {to:.2}"))
                    .collect();
                let i = at(f64::from(peak.seconds));
                found.push(self.finding(
                    Severity::Flag,
                    "too loud",
                    i,
                    i,
                    format!(
                        "the mix reached {:.0}% before the limiter: {}; try {}",
                        peak.level * 100.0,
                        names.join(", "),
                        fix.join(", ")
                    ),
                ));
            }
            let curve = super::audio::loudness(&audio.mono, audio.rate);
            if !self.run.sounds.is_empty() || audio.mono.iter().any(|s| s.abs() > 0.01) {
                for (s, e) in super::audio::silences(&curve, -50.0, 3.0) {
                    let (a, b) = (at(f64::from(s)), at(f64::from(e)));
                    found.push(self.finding(
                        Severity::Warn,
                        "silence",
                        a,
                        b,
                        format!("silent for {:.1} s", e - s),
                    ));
                }
            }
        }
        // Each sound against the event it was played for.
        for sound in &self.run.sounds {
            let Some(event) = &sound.event else { continue };
            let events = self.event_frames(event);
            let onset = self.onset(sound);
            let Some(&nearest) = events.iter().min_by(|a, b| {
                (**a as f64 - onset)
                    .abs()
                    .total_cmp(&(**b as f64 - onset).abs())
            }) else {
                let i = at(sound.frame as f64 / ups);
                found.push(self.finding(
                    Severity::Warn,
                    "sound timing",
                    i,
                    i,
                    format!(
                        "\"{}\" was played for \"{event}\", which is never marked (ctx.event)",
                        sound.name
                    ),
                ));
                continue;
            };
            let off = onset - nearest as f64;
            if off.abs() > 2.0 {
                let i = at(nearest as f64 / ups);
                let why = if sound.lead > 0.0 {
                    format!(" ({:.2} s of silence at its start)", sound.lead)
                } else {
                    String::new()
                };
                found.push(self.finding(
                    Severity::Flag,
                    "sound timing",
                    i,
                    at(onset / ups),
                    format!(
                        "\"{}\" is heard {:.0} frames {} \"{event}\"{why}",
                        sound.name,
                        off.abs(),
                        if off > 0.0 { "after" } else { "before" }
                    ),
                ));
            }
        }
    }

    // ---- games --------------------------------------------------------

    fn response(&self, found: &mut Vec<Finding>) {
        let stats = &self.rec.stats;
        let mut slow = Vec::new();
        let mut unseen = 0;
        for (frame, key) in &self.run.presses {
            let Some(i) = self.rec.index_of(*frame) else {
                continue;
            };
            let before: Vec<f32> = stats[i.saturating_sub(10)..i]
                .iter()
                .map(|s| s.changed)
                .collect();
            // Only a press made while the screen was settled can be timed
            // from the pixels: in a busy scene, other things change too.
            let usual = before.iter().copied().fold(0.0f32, f32::max);
            if before.is_empty() || usual > 0.003 {
                continue;
            }
            let reacted =
                (i..stats.len().min(i + 31)).find(|&j| stats[j].changed > usual * 1.5 + 0.002);
            match reacted {
                Some(j) => {
                    let delay = self.frame(j) - frame;
                    if delay > 3 {
                        slow.push((i, j, delay, key.clone()));
                    }
                }
                None => unseen += 1,
            }
        }
        slow.sort_by_key(|s| std::cmp::Reverse(s.2));
        let more = slow.len().saturating_sub(5);
        for (n, (i, j, delay, key)) in slow.into_iter().take(5).enumerate() {
            let tail = if n == 0 && more > 0 {
                format!(" ({more} more presses were slow too)")
            } else {
                String::new()
            };
            found.push(self.finding(
                Severity::Flag,
                "response",
                i,
                j,
                format!(
                    "after pressing {key}, the screen changes {delay} frames later ({:.0} ms; more than 3 feels sluggish){tail}",
                    delay as f64 * 1000.0 / self.run.ups
                ),
            ));
        }
        if unseen > 0 {
            let first = self.run.presses.first().map(|p| p.0).unwrap_or(0);
            let i = self.rec.nearest(first).unwrap_or(0);
            found.push(self.finding(
                Severity::Note,
                "response",
                i,
                i,
                format!("{unseen} key presses changed nothing visible within half a second"),
            ));
        }
    }

    fn speed(&self, found: &mut Vec<Finding>) {
        let budget = 1000.0 / self.run.ups as f32;
        let mut costs: Vec<(usize, f32, f32)> = (0..self.rec.stats.len())
            .map(|i| {
                let frame = self.frame(i) as usize;
                let update = self
                    .run
                    .update_ms
                    .get(frame.wrapping_sub(1))
                    .copied()
                    .unwrap_or(0.0);
                let draw = self.rec.truths[i].as_ref().map_or(0.0, |t| t.draw_ms);
                (i, update, draw)
            })
            .collect();
        let slowest = costs.iter().map(|c| c.1 + c.2).fold(0.0f32, f32::max);
        if costs.is_empty() || slowest < budget / 2.0 {
            return; // never close to slow: nothing to say
        }
        for &(i, update, _) in &costs {
            if update > budget / 2.0 {
                found.push(self.finding(
                    Severity::Warn,
                    "speed",
                    i,
                    i,
                    format!("the update took {update:.1} ms, over half a frame ({budget:.1} ms): it may stutter in a window"),
                ));
            }
        }
        costs.sort_by(|a, b| (b.1 + b.2).total_cmp(&(a.1 + a.2)));
        let slowest: Vec<String> = costs
            .iter()
            .take(5)
            .map(|&(i, u, d)| {
                format!(
                    "{} (update {u:.1} ms, draw {d:.1} ms)",
                    self.t(self.frame(i))
                )
            })
            .collect();
        let i = costs[0].0;
        found.push(self.finding(
            Severity::Note,
            "speed",
            i,
            i,
            format!("slowest frames: {}", slowest.join("; ")),
        ));
    }

    fn stuck(&self, found: &mut Vec<Finding>) {
        let truths = &self.rec.truths;
        let n = truths.len();
        let where_is = |i: usize| -> Option<(f32, f32)> {
            let thing = truths[i]
                .as_ref()?
                .things
                .iter()
                .find(|t| t.name.to_lowercase().contains("player"))?;
            Some(match thing.world_box {
                Some((lo, hi)) => (
                    (lo.x + hi.x) / 2.0,
                    (lo.z + hi.z) / 2.0 + (lo.y + hi.y) / 2.0 * 1000.0,
                ),
                None => (thing.world.x, thing.world.y),
            })
        };
        let held = |i: usize| {
            let frame = self.frame(i) as usize;
            self.run
                .held
                .get(frame.wrapping_sub(1))
                .is_some_and(|&k| k > 0)
        };
        let still = |i: usize| -> bool {
            if i == 0 || !held(i) {
                return false;
            }
            match (where_is(i - 1), where_is(i)) {
                (Some(a), Some(b)) => (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3,
                _ => false,
            }
        };
        for (a, b) in stretches(n, still) {
            let seconds = (b - a + 1) as f64 / self.run.ups;
            if seconds > 5.0 {
                found.push(self.finding(
                    Severity::Warn,
                    "stuck",
                    a,
                    b,
                    format!(
                        "the player doesn't move for {seconds:.1} s while keys are held: stuck?"
                    ),
                ));
            }
        }
    }

    // ---- expectations -------------------------------------------------

    fn expectation(&self, expect: &Expect) -> Outcome {
        let ups = self.run.ups;
        let frames: Vec<u64> = match &expect.when {
            When::At(s) => vec![((f64::from(*s) * ups).round() as u64).max(1)],
            When::On(event) => self.event_frames(event),
        };
        let mut outcome = Outcome {
            expectation: expect.words(),
            passed: true,
            frames: frames.clone(),
            why: String::new(),
        };
        if frames.is_empty() {
            outcome.passed = false;
            outcome.why = String::from("the event was never marked (ctx.event)");
            return outcome;
        }
        let (w, h) = (self.rec.width as f32, self.rec.height as f32);
        for &frame in &frames {
            let problem = match &expect.what {
                What::Sound {
                    name,
                    within_frames,
                } => {
                    let heard: Vec<f64> = self
                        .run
                        .sounds
                        .iter()
                        .filter(|s| s.name == *name)
                        .map(|s| self.onset(s) - frame as f64)
                        .collect();
                    match heard
                        .iter()
                        .copied()
                        .min_by(|a, b| a.abs().total_cmp(&b.abs()))
                    {
                        None => Some(format!("\"{name}\" is never played")),
                        Some(off) if off.abs() > *within_frames as f64 => Some(format!(
                            "the nearest \"{name}\" is heard {:.0} frames {}",
                            off.abs(),
                            if off > 0.0 { "after" } else { "before" }
                        )),
                        Some(_) => None,
                    }
                }
                _ => match self.rec.index_of(frame).or_else(|| self.rec.nearest(frame)) {
                    None => Some(String::from("that moment wasn't drawn")),
                    Some(i) => match &self.rec.truths[i] {
                        None => Some(String::from("nothing is known about that frame")),
                        Some(truth) => match &expect.what {
                            What::Thing {
                                name,
                                on_screen,
                                min_height_share,
                                min_px,
                                min_light,
                            } => match truth.things.iter().find(|t| t.name == *name) {
                                None => Some(format!("nothing is labelled \"{name}\" then")),
                                Some(t) => {
                                    let mut wrong = Vec::new();
                                    if *on_screen && !t.on_screen() {
                                        wrong.push(String::from("it's off screen"));
                                    }
                                    if let Some(share) = min_height_share {
                                        if t.visible.h < share * h {
                                            wrong.push(format!(
                                                "it's {:.1}% of the height",
                                                t.visible.h / h * 100.0
                                            ));
                                        }
                                    }
                                    if let Some(px) = min_px {
                                        if t.visible.h < *px {
                                            wrong.push(format!("it's {:.0} px tall", t.visible.h));
                                        }
                                    }
                                    if let Some(light) = min_light {
                                        if t.light < *light {
                                            wrong.push(format!("its brightness is {:.2}", t.light));
                                        }
                                    }
                                    (!wrong.is_empty()).then(|| wrong.join(", "))
                                }
                            },
                            What::CaptionReadable => {
                                if truth.texts.is_empty() {
                                    Some(String::from("there's no text on screen"))
                                } else {
                                    // Each text's problems, the apps named once.
                                    let mut wrong: Vec<String> = Vec::new();
                                    for look in &truth.texts {
                                        let quoted = shorten(&look.text);
                                        let (c, basis) = text_contrast(look);
                                        if c < 4.5 {
                                            wrong.push(format!(
                                                "{quoted} has contrast {c:.1}:1 against {basis}"
                                            ));
                                        }
                                        let mut small_on = Vec::new();
                                        let mut under: BTreeMap<String, Vec<String>> =
                                            BTreeMap::new();
                                        for p in self.profiles {
                                            let (small, covered) =
                                                text_on_profile(clip(look.rect, w, h), p, w, h);
                                            if small.is_some() {
                                                small_on.push(p.name.clone());
                                            }
                                            for (area, paused) in covered {
                                                if !paused {
                                                    under
                                                        .entry(area)
                                                        .or_default()
                                                        .push(p.name.clone());
                                                }
                                            }
                                        }
                                        if !small_on.is_empty() {
                                            wrong.push(format!(
                                                "{quoted} is too small ({:.0} px) for {}",
                                                look.rect.h,
                                                small_on.join(", ")
                                            ));
                                        }
                                        for (area, apps) in under {
                                            wrong.push(format!(
                                                "{quoted} is under {} {area}",
                                                possessive(&apps)
                                            ));
                                        }
                                    }
                                    let more = wrong.len().saturating_sub(3);
                                    wrong.truncate(3);
                                    if more > 0 {
                                        wrong.push(format!("and {more} more"));
                                    }
                                    (!wrong.is_empty()).then(|| wrong.join("; "))
                                }
                            }
                            What::Sound { .. } => None,
                        },
                    },
                },
            };
            if let Some(why) = problem {
                outcome.passed = false;
                outcome.why = format!("at {}: {why}", self.t(frame));
                break;
            }
        }
        outcome
    }
}

/// "TikTok's", "TikTok's and Reels'": whose area it is.
fn possessive(apps: &[String]) -> String {
    let own = |a: &String| {
        if a.ends_with('s') {
            format!("{a}'")
        } else {
            format!("{a}'s")
        }
    };
    match apps {
        [] => String::new(),
        [one] => own(one),
        [rest @ .., last] => format!(
            "{} and {}",
            rest.iter().map(own).collect::<Vec<_>>().join(", "),
            own(last)
        ),
    }
}

/// Text in quotes, cut short if long.
fn shorten(text: &str) -> String {
    let t: String = text.chars().take(40).collect();
    if t.len() < text.len() {
        format!("\"{t}...\"")
    } else {
        format!("\"{t}\"")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;

    fn look(color: Color, behind: f32) -> TextLook {
        TextLook {
            text: String::from("hi"),
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            color,
            outline: None,
            shadow: None,
            box_color: None,
            behind: [behind; 3],
        }
    }

    #[test]
    fn contrast_counts_outlines_and_boxes() {
        // White on white: 1:1. With a black outline: 21:1.
        let mut l = look(Color::WHITE, 1.0);
        assert!((text_contrast(&l).0 - 1.0).abs() < 1e-3);
        l.outline = Some((Color::BLACK, 2.0));
        assert_eq!(text_contrast(&l).1, "its outline");
        assert!(text_contrast(&l).0 > 20.0);
        // A half see-through black box over white: in between.
        let mut l = look(Color::WHITE, 1.0);
        l.box_color = Some(Color::rgba(0, 0, 0, 128));
        let (c, basis) = text_contrast(&l);
        assert_eq!(basis, "its box");
        assert!(c > 1.5 && c < 3.0, "{c}");
    }

    #[test]
    fn stretches_and_turns() {
        let bad = [false, true, true, false, true];
        assert_eq!(stretches(5, |i| bad[i]), vec![(1, 2), (4, 4)]);
        assert!((turn(3.0, -3.0) - (std::f32::consts::TAU - 6.0)).abs() < 1e-5);
    }
}
