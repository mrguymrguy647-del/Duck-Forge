//! The panels round the game: a problem said plainly, the Check list, and
//! the Console.

use eframe::egui::{self, Color32, RichText, ScrollArea};
use studio_core::check::{CheckResult, CheckRow, RowStatus};
use studio_core::console::{Console, JobState};
use studio_core::problem::Problem;

use crate::pictures::Pictures;
use crate::theme;

/// What a person chose in a problem panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProblemChoice {
    /// A button the caller offered, by its words.
    Button(&'static str),
    /// The link's button: open the page.
    OpenLink(String),
}

/// Shows a problem: what happened, what to do next, the buttons, and the
/// raw details only behind "Show details".
pub fn problem(
    ui: &mut egui::Ui,
    title: &str,
    problem: &Problem,
    buttons: &[&'static str],
    show_details: &mut bool,
) -> Option<ProblemChoice> {
    let mut choice = None;
    ui.heading(title);
    ui.add_space(4.0);
    ui.label(&problem.what);
    ui.add_space(4.0);
    ui.label(RichText::new(&problem.next).strong());
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        if let Some((words, address)) = &problem.link
            && ui
                .add(theme::primary_button(words))
                .on_hover_text(address)
                .clicked()
        {
            choice = Some(ProblemChoice::OpenLink(address.clone()));
        }
        for (index, words) in buttons.iter().enumerate() {
            // The first button is the main one, unless a link already is.
            let clicked = if index == 0 && problem.link.is_none() {
                ui.add(theme::primary_button(words)).clicked()
            } else {
                ui.button(*words).clicked()
            };
            if clicked {
                choice = Some(ProblemChoice::Button(words));
            }
        }
    });
    if !problem.details.is_empty() {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let words = if *show_details {
                "Hide details"
            } else {
                "Show details"
            };
            if ui.link(words).clicked() {
                *show_details = !*show_details;
            }
            if ui
                .link("Copy this for an agent")
                .on_hover_text("Copies the problem and its details, to paste to an agent")
                .clicked()
            {
                ui.ctx().copy_text(format!(
                    "{}\n{}\n\n{}",
                    problem.what, problem.next, problem.details
                ));
            }
        });
        if *show_details {
            ScrollArea::vertical()
                .id_salt("problem details")
                .max_height(180.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(&problem.details).monospace().small())
                            .wrap(),
                    );
                });
        }
    }
    choice
}

fn mark(status: RowStatus) -> (&'static str, Color32) {
    match status {
        RowStatus::Ok => ("ok", theme::GOOD),
        RowStatus::Skipped => ("--", Color32::GRAY),
        RowStatus::Warning => ("!", theme::LOOK),
        RowStatus::Failed => ("X", theme::BAD),
    }
}

/// What might help with a line of the check, in a sentence.
fn what_to_try(row: &CheckRow) -> Option<&'static str> {
    match (row.status, row.step.as_str()) {
        (RowStatus::Failed, "build") => Some(
            "What to try: fix the line the details point at. The game builds again by itself \
             when you save.",
        ),
        (RowStatus::Failed, "play tests") => Some(
            "What to try: the details name each test that failed and the line. Either the game \
             changed on purpose (then the test needs updating) or something broke.",
        ),
        (RowStatus::Warning, _) => Some(
            "What to try: if the game is meant to end differently now, record the playthrough \
             again. If not, something changed how it plays.",
        ),
        (RowStatus::Failed, step) if step.starts_with("fuzz") => Some(
            "What to try: each problem has a replay in the check's fuzz folder. Replaying it \
             shows the moment it went wrong.",
        ),
        (RowStatus::Failed, _) => Some(
            "What to try: the picture shows the frames where things changed. Look for the first \
             one that isn't what you expect.",
        ),
        _ => None,
    }
}

/// What a person chose in the Check panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckChoice {
    CheckAgain,
    Close,
    Show(Option<usize>),
    RecordAgain,
}

/// The Check panel: the friendly list, and the chosen line's picture.
pub fn check(
    ui: &mut egui::Ui,
    title: &str,
    running: Option<&str>,
    result: Option<&CheckResult>,
    shown: Option<usize>,
    pictures: &mut Pictures,
    show_details: &mut bool,
) -> Option<CheckChoice> {
    let mut choice = None;
    ui.horizontal(|ui| {
        ui.heading(format!("Check: {title}"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                choice = Some(CheckChoice::Close);
            }
            if ui
                .add_enabled(running.is_none(), theme::primary_button("Check again"))
                .clicked()
            {
                choice = Some(CheckChoice::CheckAgain);
            }
        });
    });
    ui.separator();
    if let Some(doing) = running {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(format!("{doing}..."));
        });
        ui.label(
            RichText::new(
                "A check builds the game, runs its play tests, replays its recorded \
                           playthroughs and plays 50 random runs.",
            )
            .weak(),
        );
        return choice;
    }
    let Some(result) = result else {
        ui.label("This game hasn't been checked yet.");
        return choice;
    };
    ScrollArea::vertical().id_salt("check").show(ui, |ui| {
        for (index, row) in result.rows.iter().enumerate() {
            let (text, color) = mark(row.status);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{text:>2}"))
                        .monospace()
                        .strong()
                        .color(color),
                );
                if ui
                    .selectable_label(shown == Some(index), &row.say)
                    .clicked()
                {
                    choice = Some(CheckChoice::Show((shown != Some(index)).then_some(index)));
                }
            });
        }
        ui.separator();
        let Some(row) = shown.and_then(|index| result.rows.get(index)) else {
            let summary = if result.passed && result.warnings == 0 {
                String::from("Everything passed.")
            } else if result.passed {
                format!("It passed, with {} thing(s) worth a look.", result.warnings)
            } else {
                format!(
                    "{} line(s) need fixing. Click one to see more.",
                    result.problems
                )
            };
            ui.label(RichText::new(summary).strong());
            ui.label(RichText::new("Click a line to see its picture and details.").weak());
            return;
        };
        ui.label(RichText::new(&row.say).strong());
        ui.label(&row.text);
        if row.action == Some(studio_core::check::RowAction::RecordAgain)
            && ui
                .add(crate::theme::primary_button("Record it again"))
                .on_hover_text("The game's bot plays the level as it is now, and that's kept as its playthrough")
                .clicked()
        {
            choice = Some(CheckChoice::RecordAgain);
        }
        if let Some(advice) = what_to_try(row) {
            ui.add_space(4.0);
            ui.label(advice);
        }
        if let Some(texture) = row.picture.as_ref().and_then(|p| pictures.get(ui.ctx(), p)) {
            ui.add_space(6.0);
            // As wide as the panel, but never blown up past its own size.
            let size = texture.size_vec2();
            let fit = (ui.available_width() / size.x).min(1.0);
            ui.image((texture.id(), size * fit));
        }
        if !row.details.is_empty() {
            ui.add_space(4.0);
            let words = if *show_details {
                "Hide details"
            } else {
                "Show details"
            };
            if ui.link(words).clicked() {
                *show_details = !*show_details;
            }
            if *show_details {
                ui.label(RichText::new(row.details.join("\n")).monospace().small());
            }
        }
    });
    choice
}

/// The most lines of output the Console panel shows at once.
const SHOWN_LINES: usize = 400;

/// The Console panel: every command the Studio ran, as you'd type it,
/// with what it printed, and a line to type commands of your own. Returns
/// a line to run, if Enter (or Run) was pressed.
pub fn console(ui: &mut egui::Ui, console: &Console, line: &mut String) -> Option<String> {
    let mut run = None;
    ui.set_min_height(200.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Console").strong());
        ui.label(
            RichText::new(
                "Every command the Studio runs, with its real output. They all work in a \
                 terminal too.",
            )
            .weak(),
        );
    });
    let below = 30.0;
    ScrollArea::vertical()
        .id_salt("console")
        .auto_shrink([false, false])
        .max_height((ui.available_height() - below).max(40.0))
        .stick_to_bottom(true)
        .show(ui, |ui| {
            // Only the newest lines: a long build prints thousands.
            let mut budget = SHOWN_LINES;
            let mut from = console.entries().len();
            for (index, entry) in console.entries().iter().enumerate().rev() {
                from = index;
                budget = budget.saturating_sub(entry.lines.len() + 1);
                if budget == 0 {
                    break;
                }
            }
            for entry in &console.entries()[from..] {
                let state = match &entry.state {
                    JobState::Running => String::from("running"),
                    JobState::Ended(Some(0)) => String::from("done"),
                    JobState::Ended(Some(code)) => format!("ended with code {code}"),
                    JobState::Ended(None) => String::from("stopped"),
                    JobState::CouldntStart(why) => format!("couldn't start: {why}"),
                };
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&entry.title).strong());
                    if !entry.shown.is_empty() {
                        ui.label(RichText::new(format!("> {}", entry.shown)).monospace());
                    }
                    ui.label(RichText::new(format!("({state})")).weak());
                });
                let skip = entry.lines.len().saturating_sub(SHOWN_LINES);
                if !entry.lines[skip..].is_empty() {
                    ui.label(RichText::new(entry.lines[skip..].join("\n")).monospace());
                }
            }
        });
    ui.horizontal(|ui| {
        ui.label(RichText::new(">").monospace());
        let typed = ui.add(
            egui::TextEdit::singleline(line)
                .hint_text("Type a command, like: cargo test --example dodge")
                .font(egui::TextStyle::Monospace)
                .desired_width(ui.available_width() - 60.0),
        );
        let entered = typed.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button("Run").clicked() || entered) && !line.trim().is_empty() {
            run = Some(std::mem::take(line));
            typed.request_focus();
        }
    });
    run
}
