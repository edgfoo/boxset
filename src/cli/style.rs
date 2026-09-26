use std::io::IsTerminal;
use std::sync::OnceLock;

use boxset::problem::Severity;
use boxset::task::TaskKind;

const VIDEO: &str = "⏵";
const POSTER: &str = "◪";
const SUBTITLES: &str = "┅";
const SECTION: &str = "◆";
const WARNING: &str = "▲";
const ERROR: &str = "█";

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const MAGENTA: &str = "\x1b[35m";
const BLUE: &str = "\x1b[34m";
const CYAN: &str = "\x1b[36m";
const BOLD_GREEN: &str = "\x1b[1;32m";
const GRAY: &str = "\x1b[90m";
const DIM_GRAY: &str = "\x1b[2;90m";
const DIM_CYAN: &str = "\x1b[2;36m";
const BOLD_DIM: &str = "\x1b[1;2m";
const RESET: &str = "\x1b[0m";

/// Decided once: a pipe, a CI log and `NO_COLOR` all get plain text.
pub fn styled() -> bool {
    static STYLED: OnceLock<bool> = OnceLock::new();
    *STYLED
        .get_or_init(|| std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal())
}

pub fn interactive() -> bool {
    static INTERACTIVE: OnceLock<bool> = OnceLock::new();
    *INTERACTIVE.get_or_init(|| {
        std::io::stdout().is_terminal()
            && std::env::var("TERM").map(|t| t != "dumb").unwrap_or(true)
    })
}

fn wrap(code: &str, text: &str) -> String {
    match styled() {
        true => format!("{code}{text}{RESET}"),
        false => text.to_string(),
    }
}

pub fn bold(text: &str) -> String {
    wrap(BOLD, text)
}

pub fn dim(text: &str) -> String {
    wrap(DIM, text)
}

pub fn red(text: &str) -> String {
    wrap(RED, text)
}

pub fn yellow(text: &str) -> String {
    wrap(YELLOW, text)
}

pub fn magenta(text: &str) -> String {
    wrap(MAGENTA, text)
}

pub fn blue(text: &str) -> String {
    wrap(BLUE, text)
}

pub fn cyan(text: &str) -> String {
    wrap(CYAN, text)
}

pub fn bold_green(text: &str) -> String {
    wrap(BOLD_GREEN, text)
}

/// Brighter than `dim`, for values beside dimmed labels.
pub fn gray(text: &str) -> String {
    wrap(GRAY, text)
}

pub fn dim_gray(text: &str) -> String {
    wrap(DIM_GRAY, text)
}

pub fn bold_dim(text: &str) -> String {
    wrap(BOLD_DIM, text)
}

pub fn section(name: &str) {
    println!();
    println!("{} {}", dim(SECTION), bold(name));
    println!();
}

pub struct Note {
    pub severity: Severity,
    /// A file, a flag, a target, etc
    pub locator: Option<String>,
    pub message: String,
    pub detail: Vec<String>,
}

pub fn note_lines(note: &Note) -> Vec<String> {
    let glyph = match note.severity {
        Severity::Error => red(ERROR),
        Severity::Warning => yellow(WARNING),
    };
    let head = match &note.locator {
        Some(locator) => format!("{glyph} {}  {}", dim(locator), note.message),
        None => format!("{glyph} {}", note.message),
    };

    let mut lines = vec![format!("  {head}")];
    for line in &note.detail {
        lines.push(format!("    {}", dim(line)));
    }
    lines
}

pub fn print_notes(notes: &[Note]) {
    let spaced = notes.iter().any(|note| !note.detail.is_empty());
    let ordered = notes
        .iter()
        .filter(|note| note.severity == Severity::Error)
        .chain(
            notes
                .iter()
                .filter(|note| note.severity == Severity::Warning),
        );

    for (index, note) in ordered.enumerate() {
        if spaced && index > 0 {
            println!();
        }
        for line in note_lines(note) {
            println!("{line}");
        }
    }
}

/// Pads to a visible width, ignoring the escapes the text may carry.
pub fn pad(text: &str, width: usize) -> String {
    let visible = visible_len(text);
    match visible >= width {
        true => text.to_string(),
        false => format!("{text}{}", " ".repeat(width - visible)),
    }
}

pub fn visible_len(text: &str) -> usize {
    let mut len = 0;
    let mut in_escape = false;

    for ch in text.chars() {
        if in_escape {
            in_escape = ch != 'm';
            continue;
        }

        if ch == '\x1b' {
            in_escape = true;
            continue;
        }

        len += 1;
    }

    len
}

pub fn icon(kind: TaskKind) -> String {
    match kind {
        TaskKind::Rendition { .. } => magenta(VIDEO),
        TaskKind::Poster { .. } => blue(POSTER),
        TaskKind::Subtitles => cyan(SUBTITLES),
    }
}

/// The subtitle icon, dimmed: the model fetch is work towards the subtitles
/// rather than an output of its own.
pub fn dim_subtitles_icon() -> String {
    wrap(DIM_CYAN, SUBTITLES)
}
