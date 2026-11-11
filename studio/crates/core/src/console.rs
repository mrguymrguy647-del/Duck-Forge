//! The console: every command the Studio runs, with its real output.
//!
//! Nothing the Studio does is hidden. Each button that runs a program
//! (cargo, a game, a tool script) starts it here, so the Console panel can
//! show the command as you'd type it and what it printed. Commands typed
//! into the panel run here too, in the games folder, exactly as they would
//! in a terminal.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

/// A program to run: what, with which words, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLine {
    pub program: OsString,
    pub args: Vec<OsString>,
    /// The folder it runs in.
    pub folder: PathBuf,
    /// The command as a person would type it in that folder.
    pub shown: String,
    /// Extra environment variables.
    pub env: Vec<(String, String)>,
}

impl CommandLine {
    /// `program` with `args`, shown as those words joined by spaces.
    pub fn new(program: impl Into<OsString>, args: &[&str], folder: impl Into<PathBuf>) -> Self {
        let program = program.into();
        let mut shown = shown_word(&program.to_string_lossy());
        for arg in args {
            shown.push(' ');
            shown += &shown_word(arg);
        }
        CommandLine {
            program,
            args: args.iter().map(OsString::from).collect(),
            folder: folder.into(),
            shown,
            env: Vec::new(),
        }
    }

    /// A line typed by a person, run by the system's shell, so `&&`,
    /// quotes and `.bat` files all work as they do in a terminal.
    pub fn typed(line: &str, folder: impl Into<PathBuf>) -> Self {
        let (program, flag) = if cfg!(windows) {
            ("cmd", "/C")
        } else {
            ("sh", "-c")
        };
        CommandLine {
            program: program.into(),
            args: vec![flag.into(), line.into()],
            folder: folder.into(),
            shown: line.to_string(),
            env: Vec::new(),
        }
    }

    pub fn with_env(mut self, name: &str, value: &str) -> Self {
        self.env.push((name.to_string(), value.to_string()));
        self
    }

    /// Shows it under other words (the program's short name instead of a
    /// long path, say).
    pub fn shown_as(mut self, shown: impl Into<String>) -> Self {
        self.shown = shown.into();
        self
    }
}

/// A word as it's typed: in quotes if it has a space.
fn shown_word(word: &str) -> String {
    if word.contains(' ') {
        format!("\"{word}\"")
    } else {
        word.to_string()
    }
}

/// Which job a console entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JobId(pub u64);

/// How a job stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobState {
    Running,
    /// It ended, with this exit code (0 is success; `None` if it was stopped).
    Ended(Option<i32>),
    /// It never started: the program is missing, say.
    CouldntStart(String),
}

/// One command and what it printed.
#[derive(Debug, Clone)]
pub struct Entry {
    pub id: JobId,
    /// What the Studio was doing, in a word or two: "Build", "Check".
    pub title: String,
    /// The command, as you'd type it.
    pub shown: String,
    pub lines: Vec<String>,
    pub state: JobState,
}

/// Where a line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Out,
    Err,
}

/// Something a job did, for whoever started it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobEvent {
    Line(Stream, String),
    Ended(Option<i32>),
    CouldntStart(String),
}

/// How much of a job's output the console keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// Both what it prints and its error lines.
    Everything,
    /// Only its error lines: what it prints is data for the Studio (cargo's
    /// JSON messages), not something to read.
    ErrorsOnly,
}

/// The most lines kept for one command (the oldest go first).
const MOST_LINES: usize = 4000;
/// The most commands kept.
const MOST_ENTRIES: usize = 200;

/// Every command the Studio has run, and the ones still running.
pub struct Console {
    entries: Vec<Entry>,
    keep: BTreeMap<JobId, Keep>,
    children: BTreeMap<JobId, Arc<Mutex<Child>>>,
    next: u64,
    sender: Sender<(JobId, JobEvent)>,
    receiver: Receiver<(JobId, JobEvent)>,
    /// A PATH for the programs it starts, when cargo was found somewhere
    /// the system's own PATH doesn't have.
    pub path: Option<OsString>,
}

impl Default for Console {
    fn default() -> Self {
        let (sender, receiver) = channel();
        Console {
            entries: Vec::new(),
            keep: BTreeMap::new(),
            children: BTreeMap::new(),
            next: 1,
            sender,
            receiver,
            path: None,
        }
    }
}

impl Console {
    /// Starts a command. Its output arrives through [`poll`](Console::poll).
    pub fn start(&mut self, title: &str, command: &CommandLine, keep: Keep) -> JobId {
        let id = JobId(self.next);
        self.next += 1;
        let mut entry = Entry {
            id,
            title: title.to_string(),
            shown: command.shown.clone(),
            lines: Vec::new(),
            state: JobState::Running,
        };
        let mut process = Command::new(&command.program);
        process
            .args(&command.args)
            .current_dir(&command.folder)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(path) = &self.path {
            process.env("PATH", path);
        }
        for (name, value) in &command.env {
            process.env(name, value);
        }
        hide_console_window(&mut process);
        match process.spawn() {
            Ok(mut child) => {
                let out = child.stdout.take();
                let err = child.stderr.take();
                let child = Arc::new(Mutex::new(child));
                self.children.insert(id, Arc::clone(&child));
                let sender = self.sender.clone();
                std::thread::spawn(move || {
                    let errors = err.map(|err| {
                        let sender = sender.clone();
                        std::thread::spawn(move || read_lines(err, id, Stream::Err, &sender))
                    });
                    if let Some(out) = out {
                        read_lines(out, id, Stream::Out, &sender);
                    }
                    if let Some(errors) = errors {
                        let _ = errors.join();
                    }
                    let code = child
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .wait()
                        .ok()
                        .and_then(|status| status.code());
                    let _ = sender.send((id, JobEvent::Ended(code)));
                });
            }
            Err(problem) => {
                let why = problem.to_string();
                entry.state = JobState::CouldntStart(why.clone());
                let _ = self.sender.send((id, JobEvent::CouldntStart(why)));
            }
        }
        self.keep.insert(id, keep);
        self.entries.push(entry);
        if self.entries.len() > MOST_ENTRIES {
            let gone = self.entries.remove(0).id;
            self.keep.remove(&gone);
        }
        id
    }

    /// Runs a line typed into the Console panel, in `folder`.
    pub fn run_typed(&mut self, line: &str, folder: impl Into<PathBuf>) -> JobId {
        self.start(
            "You typed",
            &CommandLine::typed(line, folder),
            Keep::Everything,
        )
    }

    /// Takes in what the running commands have printed since last time,
    /// and hands it on for whoever is waiting on a job.
    pub fn poll(&mut self) -> Vec<(JobId, JobEvent)> {
        let events: Vec<(JobId, JobEvent)> = self.receiver.try_iter().collect();
        for (id, event) in &events {
            let keep = self.keep.get(id).copied().unwrap_or(Keep::Everything);
            let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == *id) else {
                continue;
            };
            match event {
                JobEvent::Line(stream, line) => {
                    if *stream == Stream::Err || keep == Keep::Everything {
                        entry.lines.push(line.clone());
                        if entry.lines.len() > MOST_LINES {
                            entry.lines.remove(0);
                        }
                    }
                }
                JobEvent::Ended(code) => {
                    entry.state = JobState::Ended(*code);
                    self.children.remove(id);
                }
                JobEvent::CouldntStart(why) => entry.state = JobState::CouldntStart(why.clone()),
            }
        }
        events
    }

    /// Stops a running command.
    pub fn stop(&mut self, id: JobId) {
        if let Some(child) = self.children.get(&id) {
            let _ = child.lock().unwrap_or_else(|e| e.into_inner()).kill();
        }
    }

    pub fn is_running(&self, id: JobId) -> bool {
        self.children.contains_key(&id)
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn entry(&self, id: JobId) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// The last `count` lines a command printed, joined, for "Show details".
    pub fn tail(&self, id: JobId, count: usize) -> String {
        match self.entry(id) {
            Some(entry) => {
                let from = entry.lines.len().saturating_sub(count);
                entry.lines[from..].join("\n")
            }
            None => String::new(),
        }
    }

    /// Adds a note that isn't a command: something the Studio did itself
    /// (copied a file, saved a session), said in the same place.
    pub fn note(&mut self, title: &str, text: &str) {
        let id = JobId(self.next);
        self.next += 1;
        self.entries.push(Entry {
            id,
            title: title.to_string(),
            shown: String::new(),
            lines: text.lines().map(String::from).collect(),
            state: JobState::Ended(Some(0)),
        });
    }

    /// Waits for a job to end, taking in output meanwhile. For tests and
    /// scripts: the app polls instead, so its window never waits.
    pub fn wait(&mut self, id: JobId) -> Vec<JobEvent> {
        let mut mine = Vec::new();
        loop {
            for (job, event) in self.poll() {
                if job == id {
                    mine.push(event);
                }
            }
            if !matches!(self.entry(id).map(|e| &e.state), Some(JobState::Running)) {
                // The end can arrive a moment before the last lines are read.
                for (job, event) in self.poll() {
                    if job == id {
                        mine.push(event);
                    }
                }
                return mine;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

impl Drop for Console {
    /// Closing the Studio stops whatever it started.
    fn drop(&mut self) {
        for child in self.children.values() {
            let _ = child.lock().unwrap_or_else(|e| e.into_inner()).kill();
        }
    }
}

fn read_lines(from: impl Read, id: JobId, stream: Stream, to: &Sender<(JobId, JobEvent)>) {
    let mut reader = BufReader::new(from);
    let mut bytes = Vec::new();
    // Read as bytes: a program may print something that isn't UTF-8.
    while matches!(reader.read_until(b'\n', &mut bytes), Ok(n) if n > 0) {
        let line = String::from_utf8_lossy(&bytes);
        let line = line.trim_end_matches(['\n', '\r']).to_string();
        if to.send((id, JobEvent::Line(stream, line))).is_err() {
            break;
        }
        bytes.clear();
    }
}

/// On Windows, a program started by a window app would flash a black
/// console window of its own. This keeps it hidden.
fn hide_console_window(process: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        process.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = process;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn here() -> PathBuf {
        std::env::current_dir().unwrap()
    }

    #[test]
    fn a_typed_command_runs_and_its_output_is_kept() {
        let mut console = Console::default();
        let line = if cfg!(windows) {
            "echo one&& echo two"
        } else {
            "echo one && echo two && echo oops >&2"
        };
        let id = console.run_typed(line, here());
        let events = console.wait(id);
        let entry = console.entry(id).unwrap();
        assert_eq!(entry.shown, line, "shown exactly as it was typed");
        assert_eq!(entry.state, JobState::Ended(Some(0)));
        assert!(entry.lines.iter().any(|l| l.trim() == "one"), "{entry:?}");
        assert!(entry.lines.iter().any(|l| l.trim() == "two"), "{entry:?}");
        assert!(events.contains(&JobEvent::Ended(Some(0))));
    }

    #[test]
    fn a_failing_command_keeps_its_exit_code() {
        let mut console = Console::default();
        let id = console.run_typed("exit 3", here());
        console.wait(id);
        assert_eq!(console.entry(id).unwrap().state, JobState::Ended(Some(3)));
    }

    #[test]
    fn a_missing_program_says_so_instead_of_running() {
        let mut console = Console::default();
        let command = CommandLine::new("no-such-program-anywhere", &["--version"], here());
        let id = console.start("Test", &command, Keep::Everything);
        let events = console.wait(id);
        assert!(matches!(events[0], JobEvent::CouldntStart(_)), "{events:?}");
        assert!(matches!(
            console.entry(id).unwrap().state,
            JobState::CouldntStart(_)
        ));
    }

    #[test]
    fn data_output_can_be_kept_out_of_the_console() {
        let mut console = Console::default();
        let line = if cfg!(windows) {
            "echo data& echo words 1>&2"
        } else {
            "echo data; echo words >&2"
        };
        let id = console.start("Build", &CommandLine::typed(line, here()), Keep::ErrorsOnly);
        let events = console.wait(id);
        let lines: Vec<&str> = console
            .entry(id)
            .unwrap()
            .lines
            .iter()
            .map(|l| l.trim())
            .collect();
        assert_eq!(lines, ["words"], "only the error lines are shown");
        assert!(
            events
                .iter()
                .any(|e| matches!(e, JobEvent::Line(Stream::Out, l) if l.trim() == "data")),
            "but the starter still gets the data: {events:?}"
        );
    }

    #[test]
    fn commands_are_shown_as_typed() {
        let command = CommandLine::new("cargo", &["build", "--example", "bee maze"], here());
        assert_eq!(command.shown, "cargo build --example \"bee maze\"");
    }

    #[test]
    fn a_running_command_can_be_stopped() {
        let mut console = Console::default();
        let line = if cfg!(windows) {
            "ping -n 30 127.0.0.1 > nul"
        } else {
            "sleep 30"
        };
        let id = console.run_typed(line, here());
        assert!(console.is_running(id));
        console.stop(id);
        console.wait(id);
        assert!(!console.is_running(id));
        assert!(matches!(
            console.entry(id).unwrap().state,
            JobState::Ended(_)
        ));
    }
}
