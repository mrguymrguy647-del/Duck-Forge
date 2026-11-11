//! Undo and Redo: this session's edits, each one step that names the files
//! it changed, with their text before and after.
//!
//! Everything the Studio edits is a plain text file (the level, the tune
//! file), so one kind of step covers them all: undoing writes the
//! "before" text back, redoing writes the "after". A paint stroke is one
//! step however many cells it covers. A change made outside the Studio
//! (an editor, an agent) becomes a step too, so Undo never skips over it
//! by surprise.

use std::path::PathBuf;

/// One file's text before and after a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEdit {
    pub path: PathBuf,
    pub before: String,
    pub after: String,
}

/// One step for Undo: what it was, in a few words, and the files it changed.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub title: String,
    pub files: Vec<FileEdit>,
    /// Edits with the same stroke (one press of the mouse) join one step.
    pub stroke: Option<u64>,
}

/// The steps that can be undone, and the ones undone that can be redone.
#[derive(Debug, Default)]
pub struct Undo {
    done: Vec<Step>,
    undone: Vec<Step>,
}

/// How many steps Undo keeps.
const MOST_STEPS: usize = 500;

impl Undo {
    /// Notes a step. One with the same stroke as the last joins it (its
    /// files keep their first "before"). Anything undone can't be redone
    /// after a new step.
    pub fn push(&mut self, step: Step) {
        if step.files.iter().all(|edit| edit.before == edit.after) {
            return;
        }
        self.undone.clear();
        if let (Some(stroke), Some(last)) = (step.stroke, self.done.last_mut())
            && last.stroke == Some(stroke)
        {
            for edit in step.files {
                match last.files.iter_mut().find(|e| e.path == edit.path) {
                    Some(earlier) => earlier.after = edit.after,
                    None => last.files.push(edit),
                }
            }
            last.title = step.title;
            return;
        }
        self.done.push(step);
        if self.done.len() > MOST_STEPS {
            self.done.remove(0);
        }
    }

    /// The step to undo: its files go back to their "before".
    pub fn undo(&mut self) -> Option<Step> {
        let step = self.done.pop()?;
        self.undone.push(step.clone());
        Some(step)
    }

    /// The step to redo: its files go to their "after" again.
    pub fn redo(&mut self) -> Option<Step> {
        let step = self.undone.pop()?;
        self.done.push(step.clone());
        Some(step)
    }

    /// What Undo would undo, and Redo redo, in words.
    pub fn next_undo(&self) -> Option<&str> {
        self.done.last().map(|step| step.title.as_str())
    }

    pub fn next_redo(&self) -> Option<&str> {
        self.undone.last().map(|step| step.title.as_str())
    }

    /// Every step that can be undone, newest last.
    pub fn steps(&self) -> &[Step] {
        &self.done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(title: &str, before: &str, after: &str, stroke: Option<u64>) -> Step {
        Step {
            title: title.to_string(),
            files: vec![FileEdit {
                path: PathBuf::from("level.txt"),
                before: before.to_string(),
                after: after.to_string(),
            }],
            stroke,
        }
    }

    #[test]
    fn a_stroke_is_one_step_and_redo_ends_with_a_new_edit() {
        let mut undo = Undo::default();
        undo.push(step("Painted 1 tile", "a", "b", Some(1)));
        undo.push(step("Painted 2 tiles", "b", "c", Some(1)));
        undo.push(step("Nothing", "c", "c", None));
        assert_eq!(undo.steps().len(), 1);
        assert_eq!(undo.next_undo(), Some("Painted 2 tiles"));

        let undone = undo.undo().unwrap();
        assert_eq!(
            (
                undone.files[0].before.as_str(),
                undone.files[0].after.as_str()
            ),
            ("a", "c")
        );
        assert_eq!(undo.next_redo(), Some("Painted 2 tiles"));
        assert_eq!(undo.redo().unwrap().files[0].after, "c");

        undo.undo();
        undo.push(step("Erased", "a", "d", None));
        assert_eq!(undo.next_redo(), None, "a new edit ends redo");
    }
}
