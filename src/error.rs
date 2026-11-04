//! [`LoadError`]: what went wrong loading a file, in one sentence.

use std::error::Error;
use std::fmt;
use std::io;

/// A file couldn't be loaded: it's missing, or it isn't what it should be.
///
/// It prints as one plain sentence saying what went wrong and how to fix
/// it, both with `{}` and with `{:?}`. That matters because when `main`
/// returns an error, Rust prints it with `{:?}`: a plain `io::Error` would
/// show up as `Custom { kind: NotFound, error: "..." }`, but a `LoadError`
/// shows up as just the sentence:
///
/// ```text
/// Error: couldn't find the image file assets/duk.png (looked for /home/me/game/assets/duk.png); ...
/// ```
///
/// It works with `?` in a `main` that returns `Result<(), Box<dyn Error>>`,
/// and turns into an `io::Error` for functions that return `io::Result`.
pub struct LoadError {
    kind: io::ErrorKind,
    message: String,
}

impl LoadError {
    pub(crate) fn new(kind: io::ErrorKind, message: String) -> LoadError {
        LoadError { kind, message }
    }

    /// What kind of problem it was: `NotFound` for a missing file,
    /// `InvalidData` for a file that isn't a picture or sound duckforge can read.
    pub fn kind(&self) -> io::ErrorKind {
        self.kind
    }
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl fmt::Debug for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message) // the sentence, not the struct
    }
}

impl Error for LoadError {}

impl From<LoadError> for io::Error {
    fn from(error: LoadError) -> io::Error {
        io::Error::new(error.kind, error.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prints_as_one_sentence_either_way() {
        let error = LoadError::new(io::ErrorKind::NotFound, String::from("couldn't find x.png"));
        assert_eq!(format!("{error}"), "couldn't find x.png");
        assert_eq!(format!("{error:?}"), "couldn't find x.png");
        let boxed: Box<dyn Error> = Box::new(error);
        assert_eq!(
            format!("{boxed:?}"),
            "couldn't find x.png",
            "as main would print it"
        );
        let io: io::Error = LoadError::new(io::ErrorKind::InvalidData, String::from("bad")).into();
        assert_eq!(io.kind(), io::ErrorKind::InvalidData);
    }
}
