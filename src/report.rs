//! Reports: values a game shares about itself, like its score, so a test or
//! an AI can check what happened without looking at pixels.
//!
//! A game calls [`Context::report`](crate::engine::Context::report) in its
//! `update`. In a window, reports go nowhere (they cost almost nothing). In a
//! [`Simulation`](crate::Simulation) they're remembered: the latest value of
//! each, and every change, frame by frame. A headless run saves them in
//! `log.json`.

use std::collections::BTreeMap;
use std::fmt;

use serde_json::Value;

/// A reported value: a number, a yes/no, or some text.
#[derive(Debug, Clone, PartialEq)]
pub enum ReportValue {
    Number(f64),
    Bool(bool),
    Text(String),
}

impl ReportValue {
    /// The number, if this is one.
    pub fn as_number(&self) -> Option<f64> {
        match self {
            ReportValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// The yes/no, if this is one.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ReportValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The text, if this is text.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ReportValue::Text(text) => Some(text),
            _ => None,
        }
    }

    /// The value as JSON.
    pub(crate) fn to_json(&self) -> Value {
        match self {
            ReportValue::Number(n) if n.fract() == 0.0 && n.abs() < 1e15 => Value::from(*n as i64),
            ReportValue::Number(n) => serde_json::Number::from_f64(*n)
                .map(Value::Number)
                .unwrap_or(Value::Null), // JSON has no NaN or infinity
            ReportValue::Bool(b) => Value::Bool(*b),
            ReportValue::Text(text) => Value::from(text.as_str()),
        }
    }
}

impl fmt::Display for ReportValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReportValue::Text(text) => write!(f, "{text:?}"),
            other => write!(f, "{}", other.to_json()),
        }
    }
}

// `From` conversions let games write `ctx.report("score", self.score)`
// with whatever number type they use. A macro writes the same `impl` for
// each whole-number type.
macro_rules! from_whole_number {
    ($($t:ty),*) => {
        $(impl From<$t> for ReportValue {
            fn from(n: $t) -> Self {
                ReportValue::Number(n as f64)
            }
        })*
    };
}

from_whole_number!(i8, i16, i32, i64, u8, u16, u32, u64, usize, isize);

impl From<f32> for ReportValue {
    fn from(n: f32) -> Self {
        // Through text, so 0.1_f32 is reported as 0.1 and not as
        // 0.10000000149011612 (the f64 closest to that f32).
        ReportValue::Number(n.to_string().parse().unwrap_or(n as f64))
    }
}

impl From<f64> for ReportValue {
    fn from(n: f64) -> Self {
        ReportValue::Number(n)
    }
}

impl From<bool> for ReportValue {
    fn from(b: bool) -> Self {
        ReportValue::Bool(b)
    }
}

impl From<&str> for ReportValue {
    fn from(text: &str) -> Self {
        ReportValue::Text(text.to_string())
    }
}

impl From<String> for ReportValue {
    fn from(text: String) -> Self {
        ReportValue::Text(text)
    }
}

/// The values that changed on one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameReport {
    pub frame: u64,
    /// Each name with its new value, in alphabetical order.
    pub changes: BTreeMap<String, ReportValue>,
}

/// Everything a game reported during a run: the latest value of each name,
/// and a log of when each changed.
#[derive(Debug, Clone, Default)]
pub(crate) struct ReportLog {
    pub latest: BTreeMap<String, ReportValue>,
    pub changes: Vec<FrameReport>,
    /// Off in a window: the log of changes would only grow.
    pub keep_changes: bool,
}

impl ReportLog {
    /// Takes in the values reported during `frame`. Only the ones that
    /// changed (or are new) go in the log, which keeps it short enough to read.
    pub fn record(&mut self, frame: u64, reported: Vec<(String, ReportValue)>) {
        let mut changes = BTreeMap::new();
        for (name, value) in reported {
            if self.latest.get(&name) != Some(&value) {
                self.latest.insert(name.clone(), value.clone());
                changes.insert(name, value);
            }
        }
        if self.keep_changes && !changes.is_empty() {
            match self.changes.last_mut() {
                // More from the same frame (`Simulation::with_context`
                // between frames): one entry for the frame.
                Some(last) if last.frame == frame => last.changes.extend(changes),
                _ => self.changes.push(FrameReport { frame, changes }),
            }
        }
    }

    /// The value `name` had at the end of `frame`, if it had been reported by then.
    pub fn value_at(&self, frame: u64, name: &str) -> Option<&ReportValue> {
        self.changes
            .iter()
            .take_while(|report| report.frame <= frame)
            .filter_map(|report| report.changes.get(name))
            .last()
    }
}

/// One frame's changes as a line of JSON: `{"frame": 12, "score": 10}`.
pub(crate) fn frame_json(report: &FrameReport) -> String {
    let mut line = format!("{{\"frame\": {}", report.frame);
    for (name, value) in &report.changes {
        line += &format!(", {}: {}", Value::from(name.as_str()), value.to_json());
    }
    line + "}"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_convert_from_ordinary_types() {
        assert_eq!(ReportValue::from(3_u32), ReportValue::Number(3.0));
        assert_eq!(ReportValue::from(-2_i64), ReportValue::Number(-2.0));
        assert_eq!(ReportValue::from(0.1_f32), ReportValue::Number(0.1));
        assert_eq!(ReportValue::from(true).as_bool(), Some(true));
        assert_eq!(ReportValue::from("Won").as_text(), Some("Won"));
        assert_eq!(ReportValue::from(String::from("x")).as_number(), None);
    }

    #[test]
    fn only_changes_are_logged() {
        let mut log = ReportLog {
            keep_changes: true,
            ..ReportLog::default()
        };
        let score = |n: u32| (String::from("score"), ReportValue::from(n));
        log.record(
            1,
            vec![score(0), (String::from("lives"), ReportValue::from(3))],
        );
        log.record(2, vec![score(0)]);
        log.record(3, vec![score(10)]);
        log.record(4, vec![]);
        assert_eq!(log.changes.len(), 2);
        assert_eq!(log.latest["score"], ReportValue::Number(10.0));
        assert_eq!(log.value_at(2, "score"), Some(&ReportValue::Number(0.0)));
        assert_eq!(log.value_at(9, "score"), Some(&ReportValue::Number(10.0)));
        assert_eq!(log.value_at(9, "lives"), Some(&ReportValue::Number(3.0)));
        assert_eq!(log.value_at(9, "nothing"), None);
        assert_eq!(
            frame_json(&log.changes[0]),
            r#"{"frame": 1, "lives": 3, "score": 0}"#
        );
    }

    #[test]
    fn a_window_keeps_only_the_latest_values() {
        let mut log = ReportLog::default();
        log.record(1, vec![(String::from("score"), ReportValue::from(5))]);
        assert!(log.changes.is_empty());
        assert_eq!(log.latest["score"], ReportValue::Number(5.0));
    }

    #[test]
    fn values_print_as_json() {
        assert_eq!(ReportValue::Number(2.5).to_string(), "2.5");
        assert_eq!(ReportValue::Number(f64::NAN).to_json(), Value::Null);
        assert_eq!(ReportValue::from("Won").to_string(), "\"Won\"");
    }
}
