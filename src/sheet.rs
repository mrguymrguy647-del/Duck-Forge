//! Choosing a contact sheet's frames by what happened (`--sheet-events`).
//!
//! An evenly spread sheet of a 70-second run shows a frame every 4.5
//! seconds, and can skip the moment the player lost three lives. With
//! `--sheet-events`, the frames are the ones where a reported value
//! changed: text and yes/no values first (like `state`), then numbers (like
//! `lives` or `score`), plus the first and the last frame, and the rest
//! filled evenly. A value that changes all the time (a position, a timer)
//! isn't an event, so after [`EVENT_LIMIT`] changes it's ignored.
//!
//! While the game runs, [`SheetPicker`] keeps a copy of each frame that
//! might be chosen; at the end, [`SheetPicker::choose`] picks the frames.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::canvas::Canvas;
use crate::pictures;
use crate::report::ReportValue;
use crate::scene::Scene;

/// A value that changes more often than this isn't an event.
pub(crate) const EVENT_LIMIT: usize = 20;

/// Why a frame might go on the sheet: a report changed to this value.
#[derive(Debug, Clone, PartialEq)]
struct Reason {
    name: String,
    value: String,
    /// Text or yes/no (like a state): these come first.
    text: bool,
}

/// A frame kept in case it's chosen.
struct Kept {
    picture: Canvas,
    scene: Scene,
    reasons: Vec<Reason>,
    /// One of the evenly spread frames, kept to fill gaps.
    spread: bool,
}

/// A frame chosen for the sheet, with what it shows.
pub(crate) struct SheetFrame {
    pub frame: u64,
    pub picture: Canvas,
    #[allow(dead_code)] // for following a label up close (Fix 5)
    pub scene: Scene,
    /// What made it worth showing, like `lives 2` or `state Lost`.
    pub caption: String,
}

/// Collects candidate frames during a run (see the module's notes).
#[derive(Default)]
pub(crate) struct SheetPicker {
    changes: HashMap<String, usize>,
    kept: BTreeMap<u64, Kept>,
    /// The reasons for the frame about to be kept.
    pending: Option<(u64, Vec<Reason>)>,
}

impl SheetPicker {
    /// The reasons this frame might be worth showing, from the values that
    /// changed on it. Frame 1 has none: every value is "new" there.
    /// Values that change too often are dropped, from this frame and from
    /// the frames already kept.
    pub(crate) fn reasons(&mut self, frame: u64, changed: &BTreeMap<String, ReportValue>) -> bool {
        if frame <= 1 {
            for name in changed.keys() {
                self.changes.entry(name.clone()).or_insert(0);
            }
            return false;
        }
        let mut reasons = Vec::new();
        for (name, value) in changed {
            let count = self.changes.entry(name.clone()).or_insert(0);
            *count += 1;
            let count = *count;
            if count == EVENT_LIMIT + 1 {
                self.forget(name);
            }
            if count <= EVENT_LIMIT {
                let (value, text) = match value {
                    ReportValue::Text(text) => (text.clone(), true),
                    ReportValue::Bool(b) => (b.to_string(), true),
                    number => (number.to_string(), false),
                };
                reasons.push(Reason {
                    name: name.clone(),
                    value,
                    text,
                });
            }
        }
        let wanted = !reasons.is_empty();
        if wanted {
            self.pending = Some((frame, reasons));
        }
        wanted
    }

    /// Keeps the picture (and scene) of `frame`: the one `reasons` just
    /// asked for, or an evenly spread one (`spread`).
    pub(crate) fn keep(&mut self, frame: u64, picture: Canvas, scene: Scene, spread: bool) {
        let reasons = match self.pending.take() {
            Some((f, reasons)) if f == frame => reasons,
            _ => Vec::new(),
        };
        self.kept.insert(
            frame,
            Kept {
                picture,
                scene,
                reasons,
                spread,
            },
        );
    }

    /// Drops a value that turned out to change all the time.
    fn forget(&mut self, name: &str) {
        for kept in self.kept.values_mut() {
            kept.reasons.retain(|r| r.name != name);
        }
        self.kept
            .retain(|_, kept| kept.spread || !kept.reasons.is_empty());
    }

    /// Picks up to `count` frames: the first and the last, then frames
    /// where a text value changed, then where a number did (spread out if
    /// there are too many), then evenly spread ones to fill the sheet.
    pub(crate) fn choose(mut self, last: u64, count: usize) -> Vec<SheetFrame> {
        let mut chosen: BTreeSet<u64> = BTreeSet::new();
        let ends: Vec<u64> = [1, last]
            .into_iter()
            .filter(|f| self.kept.contains_key(f))
            .collect();
        chosen.extend(&ends);
        let room = count.saturating_sub(chosen.len());
        let with = |text: bool| -> Vec<u64> {
            self.kept
                .iter()
                .filter(|(f, k)| !ends.contains(f) && k.reasons.iter().any(|r| r.text == text))
                .map(|(f, _)| *f)
                .collect()
        };
        let texts = with(true);
        let numbers: Vec<u64> = with(false)
            .into_iter()
            .filter(|f| !texts.contains(f))
            .collect();
        chosen.extend(spread_pick(&texts, room));
        let room = count.saturating_sub(chosen.len());
        chosen.extend(spread_pick(&numbers, room));
        // Fill up with the evenly spread frames furthest from those chosen.
        while chosen.len() < count {
            let gap = |f: &u64| {
                chosen
                    .iter()
                    .map(|c| c.abs_diff(*f))
                    .min()
                    .unwrap_or(u64::MAX)
            };
            let Some(next) = self
                .kept
                .iter()
                .filter(|(f, k)| k.spread && !chosen.contains(f))
                .map(|(f, _)| *f)
                .max_by_key(gap)
            else {
                break;
            };
            chosen.insert(next);
        }
        chosen
            .into_iter()
            .filter_map(|frame| {
                let kept = self.kept.remove(&frame)?;
                let caption = if frame == 1 {
                    String::from("start")
                } else if kept.reasons.is_empty() && frame == last {
                    String::from("end")
                } else {
                    caption(&kept.reasons)
                };
                Some(SheetFrame {
                    frame,
                    picture: kept.picture,
                    scene: kept.scene,
                    caption,
                })
            })
            .collect()
    }
}

/// `state Lost, lives 2`: text values first.
fn caption(reasons: &[Reason]) -> String {
    let mut sorted: Vec<&Reason> = reasons.iter().collect();
    sorted.sort_by_key(|r| !r.text);
    let parts: Vec<String> = sorted
        .iter()
        .map(|r| format!("{} {}", r.name, r.value))
        .collect();
    parts.join(", ")
}

/// Up to `count` of `frames`, spread evenly, keeping the first and last.
fn spread_pick(frames: &[u64], count: usize) -> Vec<u64> {
    if frames.len() <= count {
        return frames.to_vec();
    }
    pictures::spread_frames(frames.len() as u64, count)
        .into_iter()
        .map(|i| frames[i as usize - 1])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changed(pairs: &[(&str, ReportValue)]) -> BTreeMap<String, ReportValue> {
        pairs
            .iter()
            .map(|(n, v)| (n.to_string(), v.clone()))
            .collect()
    }

    fn picker_for(run: &[(u64, Vec<(&str, ReportValue)>)], last: u64) -> SheetPicker {
        let mut picker = SheetPicker::default();
        let spread: BTreeSet<u64> = pictures::spread_frames(last, 16).into_iter().collect();
        let mut events = run.iter().peekable();
        for frame in 1..=last {
            let mut wanted = false;
            if let Some((_, values)) = events.next_if(|(f, _)| *f == frame) {
                wanted = picker.reasons(frame, &changed(values));
            }
            if wanted || spread.contains(&frame) {
                picker.keep(
                    frame,
                    Canvas::new(4, 4),
                    Scene::default(),
                    spread.contains(&frame),
                );
            }
        }
        picker
    }

    #[test]
    fn the_moments_things_change_are_picked_and_captioned() {
        let lives = |n: u32| ("lives", ReportValue::from(n));
        // Over 4200 frames: three lives lost close together near the end,
        // the state changing at the very end, and x changing every frame.
        let mut run: Vec<(u64, Vec<(&str, ReportValue)>)> = vec![(
            1,
            vec![
                lives(3),
                ("state", ReportValue::from("Playing")),
                ("x", 0.into()),
            ],
        )];
        for frame in 2..=4200u64 {
            let mut values = vec![("x", ReportValue::from(frame))];
            match frame {
                3900 => values.push(lives(2)),
                3990 => values.push(lives(1)),
                4100 => {
                    values.push(lives(0));
                    values.push(("state", ReportValue::from("Lost")));
                }
                _ => {}
            }
            run.push((frame, values));
        }
        let chosen = picker_for(&run, 4200).choose(4200, 16);
        assert_eq!(chosen.len(), 16);
        let frames: Vec<u64> = chosen.iter().map(|f| f.frame).collect();
        for moment in [1, 3900, 3990, 4100, 4200] {
            assert!(frames.contains(&moment), "{moment} in {frames:?}");
        }
        let caption = |frame| &chosen.iter().find(|f| f.frame == frame).unwrap().caption;
        assert_eq!(caption(1), "start");
        assert_eq!(caption(3900), "lives 2");
        assert_eq!(caption(4100), "state Lost, lives 0");
        assert_eq!(caption(4200), "end");
        // x changes every frame: never a reason.
        assert!(
            chosen.iter().all(|f| !f.caption.contains('x')),
            "{frames:?}"
        );
    }

    #[test]
    fn too_many_events_are_spread_out() {
        let run: Vec<(u64, Vec<(&str, ReportValue)>)> = (2..=40u64)
            .map(|f| {
                let state = if f % 2 == 0 { "A" } else { "B" };
                (f * 10, vec![("state", ReportValue::from(state))])
            })
            .collect();
        // "state" changes 39 times: more than EVENT_LIMIT, so it's churn.
        let chosen = picker_for(&run, 400).choose(400, 16);
        assert!(
            chosen
                .iter()
                .all(|f| f.caption == "start" || f.caption == "end" || f.caption.is_empty())
        );
        assert_eq!(chosen.len(), 16, "filled evenly");
    }
}
