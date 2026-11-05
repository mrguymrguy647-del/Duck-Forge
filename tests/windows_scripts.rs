//! The Windows scripts can't run here, so these checks catch the mistakes
//! that are easiest to make in them without running them: a bracket left
//! open (cmd then reads the rest of the file as one block), a `goto` or
//! `call` to a label that doesn't exist, a PowerShell parameter the helper
//! doesn't have, and Windows line endings (cmd needs them for labels).

const SCRIPTS: [(&str, &str); 2] = [
    ("tools/verify.bat", include_str!("../tools/verify.bat")),
    ("tools/new_game.bat", include_str!("../tools/new_game.bat")),
];

/// The lines that are commands: not comments.
fn commands(text: &str) -> impl Iterator<Item = &str> {
    text.lines().map(str::trim).filter(|l| {
        let lower = l.to_ascii_lowercase();
        !(lower == "rem" || lower.starts_with("rem ") || l.starts_with("::"))
    })
}

#[test]
fn brackets_balance_outside_quotes() {
    for (name, text) in SCRIPTS {
        let mut depth: i32 = 0;
        for (number, line) in commands(text).enumerate() {
            let mut quoted = false;
            let mut escaped = false;
            for c in line.chars() {
                match c {
                    _ if escaped => escaped = false,
                    '^' if !quoted => escaped = true,
                    '"' => quoted = !quoted,
                    '(' if !quoted => depth += 1,
                    ')' if !quoted => depth -= 1,
                    _ => {}
                }
            }
            assert!(
                depth >= 0,
                "{name}: a ) with no ( by command line {}: {line}",
                number + 1
            );
        }
        assert_eq!(depth, 0, "{name}: a ( is never closed");
    }
}

#[test]
fn every_goto_and_call_has_its_label() {
    for (name, text) in SCRIPTS {
        let labels: Vec<String> = text
            .lines()
            .filter_map(|l| l.trim().strip_prefix(':'))
            .filter(|l| !l.starts_with(':'))
            .map(|l| l.trim().to_ascii_lowercase())
            .collect();
        for line in commands(text) {
            let words: Vec<&str> = line.split_whitespace().collect();
            for pair in words.windows(2) {
                let target = match pair[0].to_ascii_lowercase().as_str() {
                    "goto" => pair[1].trim_start_matches(':'),
                    "call" if pair[1].starts_with(':') => &pair[1][1..],
                    _ => continue,
                };
                let target = target.to_ascii_lowercase();
                assert!(
                    target == "eof" || labels.contains(&target),
                    "{name}: `{line}` goes to :{target}, which isn't there"
                );
            }
        }
    }
}

#[test]
fn new_game_passes_only_parameters_the_helper_has() {
    let helper = include_str!("../tools/new_game.ps1");
    let bat = include_str!("../tools/new_game.bat");
    let call = bat
        .lines()
        .find(|l| l.contains("tools\\new_game.ps1"))
        .expect("new_game.bat calls new_game.ps1");
    let mut passed: Vec<&str> = call
        .split_whitespace()
        .filter(|w| w.starts_with('-') && w.len() > 2 && w.as_bytes()[1].is_ascii_uppercase())
        .collect();
    // -Title is passed through %titlearg%.
    assert!(bat.contains("set titlearg=-Title "));
    passed.push("-Title");
    for parameter in passed {
        let declared = format!("${}", &parameter[1..]);
        assert!(
            helper.contains(&declared),
            "new_game.bat passes {parameter}, but new_game.ps1 has no {declared}"
        );
    }
}

#[test]
fn batch_files_get_windows_line_endings() {
    let attributes = include_str!("../.gitattributes");
    assert!(
        attributes
            .lines()
            .any(|l| l.split_whitespace().collect::<Vec<_>>() == ["*.bat", "text", "eol=crlf"]),
        ".gitattributes must keep `*.bat text eol=crlf`"
    );
}
