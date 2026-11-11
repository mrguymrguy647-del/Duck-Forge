//! Building a game: finding Rust, running cargo behind a progress bar,
//! saying a mistake in the code plainly, and noticing when the code
//! changes so the next build can start by itself.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

use crate::console::{Console, JobEvent, JobId, Keep, Stream};
use crate::folder::GamesFolder;
use crate::problem::Problem;

/// The official pages the Studio points to. It opens them only when a
/// person presses the button, and never installs anything itself.
pub const RUST_PAGE: &str = "https://www.rust-lang.org/tools/install";
pub const BUILD_TOOLS_PAGE: &str = "https://visualstudio.microsoft.com/visual-cpp-build-tools/";

/// Where cargo is, and a PATH that finds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rust {
    pub cargo: PathBuf,
    /// Its version line, like `cargo 1.97.0 (...)`.
    pub version: String,
    /// A PATH with cargo's folder in front, when the system's own PATH
    /// doesn't have it (right after installing Rust, before signing out).
    pub path: Option<OsString>,
}

/// Looks for Rust: on the PATH, then in `.cargo/bin` in the user's folder
/// (where the official installer puts it).
pub fn find_rust() -> Result<Rust, Problem> {
    let name = if cfg!(windows) { "cargo.exe" } else { "cargo" };
    let mut tried = Vec::new();
    let mut places = vec![PathBuf::from("cargo")];
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(|home| PathBuf::from(home).join(".cargo"))
        });
    if let Some(home) = home {
        places.push(home.join("bin").join(name));
    }
    for cargo in places {
        let mut ask = std::process::Command::new(&cargo);
        ask.arg("--version");
        hide_window(&mut ask);
        match ask.output() {
            Ok(output) if output.status.success() => {
                let path = cargo
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .map(|bin| {
                        let mut all = vec![bin.to_path_buf()];
                        all.extend(std::env::split_paths(
                            &std::env::var_os("PATH").unwrap_or_default(),
                        ));
                        std::env::join_paths(all).unwrap_or_default()
                    });
                return Ok(Rust {
                    cargo,
                    version: String::from_utf8_lossy(&output.stdout).trim().to_string(),
                    path,
                });
            }
            Ok(output) => tried.push(format!(
                "{}: {}",
                cargo.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            )),
            Err(problem) => tried.push(format!("{}: {problem}", cargo.display())),
        }
    }
    Err(rust_is_missing(&tried.join("\n")))
}

/// The "Duck Forge needs Rust" problem.
pub fn rust_is_missing(details: &str) -> Problem {
    Problem::new(
        "Duck Forge needs Rust. Rust is the free tool that turns a game's code into a program. \
         It isn't on this computer yet, or Windows can't find it.",
        "Open the official install page and run the installer. Then come back and press Check \
         again.",
    )
    .with_link("Open the Rust install page", RUST_PAGE)
    .with_details(details)
}

fn hide_window(process: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        process.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    #[cfg(not(windows))]
    let _ = process;
}

/// A mistake in the code, as the compiler reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeMistake {
    /// The file, as cargo names it (from the games folder).
    pub file: String,
    pub line: u64,
    /// The compiler's own sentence.
    pub message: String,
    /// Its suggestion, if it made one.
    pub hint: Option<String>,
    /// The whole message as the compiler prints it.
    pub rendered: String,
}

/// How a build stands.
#[derive(Debug, Clone, PartialEq)]
pub enum BuildState {
    Running,
    /// Built: the program is here.
    Done {
        program: PathBuf,
    },
    Failed(Problem),
}

/// One build of one game, in progress or finished.
#[derive(Debug)]
pub struct Build {
    pub game: String,
    pub job: JobId,
    pub state: BuildState,
    /// Parts finished so far, and how many the last build had (0 if this
    /// is the first, when there's nothing to compare with).
    pub parts: u32,
    pub parts_expected: u32,
    pub started: Instant,
    program: Option<PathBuf>,
    mistakes: Vec<CodeMistake>,
    error_lines: Vec<String>,
}

impl Build {
    /// Starts building `game`.
    pub fn start(
        console: &mut Console,
        folder: &GamesFolder,
        rust: &Rust,
        game: &str,
        parts_expected: u32,
    ) -> Build {
        let command = folder.build_command(&rust.cargo, game);
        let job = console.start("Build", &command, Keep::ErrorsOnly);
        Build {
            game: game.to_string(),
            job,
            state: BuildState::Running,
            parts: 0,
            parts_expected,
            started: Instant::now(),
            program: None,
            mistakes: Vec::new(),
            error_lines: Vec::new(),
        }
    }

    /// How far along it is, from 0 to 1, or `None` on a first build (when
    /// the Studio says "a few minutes" instead).
    pub fn progress(&self) -> Option<f32> {
        if self.parts_expected == 0 {
            return None;
        }
        Some((self.parts as f32 / self.parts_expected as f32).min(1.0))
    }

    /// Takes in one thing the build's job did. Returns true when the
    /// build has just finished, either way.
    pub fn take(&mut self, event: &JobEvent) -> bool {
        match event {
            JobEvent::Line(Stream::Out, line) => {
                let Ok(message) = serde_json::from_str::<Value>(line) else {
                    return false;
                };
                match message["reason"].as_str() {
                    Some("compiler-artifact") => {
                        self.parts += 1;
                        let is_game = message["target"]["name"].as_str() == Some(&self.game)
                            && message["target"]["kind"]
                                .as_array()
                                .is_some_and(|kinds| kinds.iter().any(|k| k == "example"));
                        if let (true, Some(program)) = (is_game, message["executable"].as_str()) {
                            self.program = Some(PathBuf::from(program));
                        }
                    }
                    Some("compiler-message") => {
                        if let Some(mistake) = mistake_from(&message["message"]) {
                            self.mistakes.push(mistake);
                        }
                    }
                    _ => {}
                }
                false
            }
            JobEvent::Line(Stream::Err, line) => {
                self.error_lines.push(line.clone());
                false
            }
            JobEvent::Ended(code) => {
                self.state = match (code, self.program.take()) {
                    (Some(0), Some(program)) => BuildState::Done { program },
                    _ => BuildState::Failed(self.what_went_wrong()),
                };
                true
            }
            JobEvent::CouldntStart(why) => {
                self.state = BuildState::Failed(rust_is_missing(why));
                true
            }
        }
    }

    /// The reason the build failed, said plainly.
    fn what_went_wrong(&self) -> Problem {
        let output = self.error_lines.join("\n");
        if let Some(problem) = setup_problem(&output) {
            return problem;
        }
        match self.mistakes.first() {
            Some(mistake) => {
                let more = match self.mistakes.len() - 1 {
                    0 => String::new(),
                    1 => String::from(" (and one more after it)"),
                    n => format!(" (and {n} more after it)"),
                };
                let hint = mistake.hint.as_ref().map_or(String::new(), |hint| {
                    format!(" The compiler suggests: {hint}")
                });
                Problem::new(
                    format!(
                        "There's a mistake in the code{more}. {}, line {}: {}.{hint}",
                        mistake.file, mistake.line, mistake.message
                    ),
                    "Open the file at that line and fix it; the game builds again by itself \
                     when you save. Or copy this for an agent.",
                )
                .with_details(
                    self.mistakes
                        .iter()
                        .map(|m| m.rendered.trim_end())
                        .collect::<Vec<_>>()
                        .join("\n\n"),
                )
            }
            None => Problem::new(
                "The game couldn't be built, and the build didn't say which line is wrong.",
                "Look at the details, or copy them for an agent.",
            )
            .with_details(output),
        }
    }
}

/// A problem with the computer's setup rather than the game's code,
/// recognised from what cargo printed.
pub fn setup_problem(output: &str) -> Option<Problem> {
    let has = |text: &str| output.contains(text);
    if has("linker `link.exe` not found") || has("link.exe` not found") {
        return Some(
            Problem::new(
                "Rust needs Microsoft's free C++ Build Tools to finish a build on Windows, and \
                 they aren't installed. (The Rust installer usually offers to add them.)",
                "Open Microsoft's download page, install \"Desktop development with C++\", \
                 then press Check again.",
            )
            .with_link("Open Microsoft's download page", BUILD_TOOLS_PAGE)
            .with_details(output),
        );
    }
    if has("requires rustc") || has("is not supported by the following package") {
        return Some(
            Problem::new(
                "This computer's Rust is older than Duck Forge needs.",
                "Update Rust: type `rustup update` in the Console panel and press Enter, then \
                 build again.",
            )
            .with_details(output),
        );
    }
    if has("failed to download")
        || has("failed to get `")
        || has("Could not resolve host")
        || has("failed to query replaced source")
        || has("network failure")
        || has("spurious network error")
    {
        return Some(
            Problem::new(
                "Couldn't download the parts Duck Forge is built from. That only happens on \
                 the first build, and it needs the internet.",
                "Check the internet connection, then press Try again.",
            )
            .with_details(output),
        );
    }
    None
}

/// The first place an error points at, from one of the compiler's messages.
fn mistake_from(message: &Value) -> Option<CodeMistake> {
    if message["level"].as_str() != Some("error") {
        return None;
    }
    let text = message["message"].as_str()?;
    if text.starts_with("aborting due to") || text.starts_with("could not compile") {
        return None;
    }
    let spans = message["spans"].as_array()?;
    let span = spans
        .iter()
        .find(|span| span["is_primary"].as_bool() == Some(true))
        .or_else(|| spans.first())?;
    let hint = message["children"].as_array().and_then(|children| {
        children
            .iter()
            .find(|child| child["level"].as_str() == Some("help"))
            .and_then(|child| child["message"].as_str())
            .map(String::from)
    });
    Some(CodeMistake {
        file: span["file_name"].as_str()?.to_string(),
        line: span["line_start"].as_u64()?,
        message: text.to_string(),
        hint,
        rendered: message["rendered"].as_str().unwrap_or(text).to_string(),
    })
}

/// Copies a built game to the folder the Studio plays games from, under a
/// new name each time. Windows won't let cargo replace a program while it
/// runs, and the next build must work while you're still playing.
pub fn copy_to_play(folder: &GamesFolder, program: &Path) -> Result<PathBuf, Problem> {
    let couldnt = |problem: std::io::Error| {
        Problem::new(
            "The Studio couldn't get the built game ready to play.",
            "Try again. If it keeps happening, check that the disk isn't full.",
        )
        .with_details(format!("copying {}: {problem}", program.display()))
    };
    let run = folder.run_folder();
    std::fs::create_dir_all(&run).map_err(couldnt)?;
    let stem = program.file_stem().unwrap_or_default().to_string_lossy();
    let end = program
        .extension()
        .map_or(String::new(), |e| format!(".{}", e.to_string_lossy()));
    // Old copies of this game that aren't running any more can go. (One
    // that is still running can't be deleted on Windows: it's left alone.)
    if let Ok(entries) = std::fs::read_dir(&run) {
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with(&format!("{stem}-"))
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis());
    let copy = run.join(format!("{stem}-{stamp}{end}"));
    std::fs::copy(program, &copy).map_err(couldnt)?;
    Ok(copy)
}

/// How long the watched files must stay unchanged before a build starts,
/// so a half-saved edit (or several saves in a row) never starts one.
pub const SETTLE: Duration = Duration::from_millis(1000);

/// Watches a game's code and says when to rebuild.
#[derive(Debug, Default)]
pub struct Watcher {
    /// What each file looked like at the last look: when it changed, and
    /// how long it is.
    seen: BTreeMap<PathBuf, (Option<SystemTime>, u64)>,
    /// When a change was last noticed, if a build is owed.
    changed_at: Option<Instant>,
}

impl Watcher {
    /// Starts watching: whatever the files look like now is "unchanged".
    pub fn new(files: &[PathBuf]) -> Watcher {
        Watcher {
            seen: look(files),
            changed_at: None,
        }
    }

    /// Looks at the files. Returns true once, when something has changed
    /// and then stayed the same for [`SETTLE`].
    pub fn look(&mut self, files: &[PathBuf], now: Instant) -> bool {
        self.take(look(files), now)
    }

    /// [`look`](Watcher::look), given what the files look like now.
    fn take(
        &mut self,
        now_seen: BTreeMap<PathBuf, (Option<SystemTime>, u64)>,
        now: Instant,
    ) -> bool {
        if now_seen != self.seen {
            self.seen = now_seen;
            self.changed_at = Some(now); // changed again: start waiting afresh
            return false;
        }
        match self.changed_at {
            Some(at) if now.duration_since(at) >= SETTLE => {
                self.changed_at = None;
                true
            }
            _ => false,
        }
    }

    /// Whether a change has been seen that hasn't settled yet.
    pub fn is_waiting(&self) -> bool {
        self.changed_at.is_some()
    }
}

fn look(files: &[PathBuf]) -> BTreeMap<PathBuf, (Option<SystemTime>, u64)> {
    files
        .iter()
        .map(|file| {
            let seen = std::fs::metadata(file)
                .map(|data| (data.modified().ok(), data.len()))
                .unwrap_or((None, 0));
            (file.clone(), seen)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_is_found_here_and_says_its_version() {
        let rust = find_rust().expect("these tests are run by cargo, so it's here");
        assert!(rust.version.starts_with("cargo "), "{rust:?}");
    }

    #[test]
    fn a_build_waits_until_the_files_have_been_still_for_a_second() {
        let file = PathBuf::from("main.rs");
        let at = |length: u64| -> BTreeMap<PathBuf, (Option<SystemTime>, u64)> {
            [(file.clone(), (None, length))].into_iter().collect()
        };
        let start = Instant::now();
        let ms = |n: u64| start + Duration::from_millis(n);
        let mut watcher = Watcher {
            seen: at(10),
            changed_at: None,
        };
        assert!(!watcher.take(at(10), ms(0)), "nothing changed");
        assert!(!watcher.take(at(11), ms(500)), "a change: start waiting");
        assert!(watcher.is_waiting());
        assert!(!watcher.take(at(11), ms(1000)), "only half a second so far");
        assert!(!watcher.take(at(12), ms(1400)), "saved again: wait afresh");
        assert!(!watcher.take(at(12), ms(2300)), "0.9 seconds since");
        assert!(watcher.take(at(12), ms(2400)), "a full second: build");
        assert!(!watcher.take(at(12), ms(5000)), "and only once");
        assert!(!watcher.is_waiting());
    }

    fn build_of(game: &str) -> Build {
        Build {
            game: game.to_string(),
            job: JobId(1),
            state: BuildState::Running,
            parts: 0,
            parts_expected: 4,
            started: Instant::now(),
            program: None,
            mistakes: Vec::new(),
            error_lines: Vec::new(),
        }
    }

    fn out(line: &str) -> JobEvent {
        JobEvent::Line(Stream::Out, line.to_string())
    }

    #[test]
    fn a_good_build_counts_its_parts_and_finds_the_program() {
        let mut build = build_of("dodge");
        build.take(&out(r#"{"reason":"compiler-artifact","target":{"name":"png","kind":["lib"]},"executable":null}"#));
        build.take(&out(r#"{"reason":"compiler-artifact","target":{"name":"duckforge","kind":["lib"]},"executable":null}"#));
        assert_eq!(build.progress(), Some(0.5));
        build.take(&out(r#"{"reason":"compiler-artifact","target":{"name":"dodge","kind":["example"]},"executable":"/t/debug/examples/dodge"}"#));
        build.take(&out("not json at all"));
        assert!(build.take(&JobEvent::Ended(Some(0))));
        assert_eq!(
            build.state,
            BuildState::Done {
                program: PathBuf::from("/t/debug/examples/dodge")
            }
        );
    }

    #[test]
    fn a_mistake_in_the_code_is_said_in_one_sentence_with_the_line() {
        let mut build = build_of("dodge");
        build.take(&out(
            r#"{"reason":"compiler-message","message":{"level":"error","message":"cannot find value `jump_sped` in this scope","spans":[{"file_name":"examples/dodge.rs","line_start":42,"is_primary":true}],"children":[{"level":"help","message":"a local variable with a similar name exists: `jump_speed`"}],"rendered":"error[E0425]: cannot find value `jump_sped`\n --> examples/dodge.rs:42:9\n"}}"#,
        ));
        build.take(&out(
            r#"{"reason":"compiler-message","message":{"level":"error","message":"aborting due to 1 previous error","spans":[],"children":[],"rendered":"error: aborting"}}"#,
        ));
        build.take(&out(
            r#"{"reason":"compiler-message","message":{"level":"warning","message":"unused variable","spans":[{"file_name":"examples/dodge.rs","line_start":7,"is_primary":true}],"children":[],"rendered":"warning"}}"#,
        ));
        assert!(build.take(&JobEvent::Ended(Some(101))));
        let BuildState::Failed(problem) = &build.state else {
            panic!("it failed: {:?}", build.state);
        };
        assert_eq!(
            problem.what,
            "There's a mistake in the code. examples/dodge.rs, line 42: cannot find value \
             `jump_sped` in this scope. The compiler suggests: a local variable with a similar \
             name exists: `jump_speed`"
        );
        assert!(
            problem.details.contains("error[E0425]"),
            "the raw text is in the details"
        );
        assert!(!problem.what.contains("E0425"), "and not in the sentence");
    }

    #[test]
    fn setup_problems_are_told_apart_from_code_mistakes() {
        let tools = setup_problem("error: linker `link.exe` not found\n  note: ...").unwrap();
        assert!(tools.what.contains("C++ Build Tools"));
        assert_eq!(tools.link.unwrap().1, BUILD_TOOLS_PAGE);
        let old = setup_problem("error: package `x` requires rustc 1.95 or newer").unwrap();
        assert!(old.what.contains("older than Duck Forge needs"));
        let offline = setup_problem("error: failed to download from `https://...`").unwrap();
        assert!(offline.next.contains("internet connection"));
        assert_eq!(setup_problem("error[E0425]: cannot find value"), None);
        let missing = rust_is_missing("cargo: not found");
        assert_eq!(missing.link.unwrap().1, RUST_PAGE);
        // A build that can't even start cargo is the same problem.
        let mut build = build_of("dodge");
        build.take(&JobEvent::CouldntStart(String::from("program not found")));
        assert!(matches!(&build.state, BuildState::Failed(p) if p.what.contains("needs Rust")));
    }
}
