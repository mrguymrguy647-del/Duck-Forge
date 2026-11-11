//! Problems, said plainly: what happened, what to do next, and the raw
//! details for whoever wants them.

use serde::Serialize;

/// Something that went wrong, as the Studio tells a person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    /// What happened, in a sentence or two, with no jargon.
    pub what: String,
    /// What to do about it.
    pub next: String,
    /// The raw output (a compiler error, a command's last lines), shown
    /// only behind "Show details".
    pub details: String,
    /// A page that helps, with the words for its button: the official
    /// installer, say. The Studio opens it only when the button is pressed.
    pub link: Option<(String, String)>,
}

impl Problem {
    pub fn new(what: impl Into<String>, next: impl Into<String>) -> Problem {
        Problem {
            what: what.into(),
            next: next.into(),
            details: String::new(),
            link: None,
        }
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Problem {
        self.details = details.into();
        self
    }

    pub fn with_link(mut self, button: &str, address: &str) -> Problem {
        self.link = Some((button.to_string(), address.to_string()));
        self
    }
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.what, self.next)
    }
}

impl std::error::Error for Problem {}
