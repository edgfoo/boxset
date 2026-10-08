//! `--html`: a `<video>` snippet per target in one boxset.html, from the
//! outputs a run wrote.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

use boxset::problem::Severity;

use super::errors;
use super::style::{self, Note, bold};
use super::units::directory;

const HTML_FILE: &str = "boxset.html";

/// `root` is the config's directory, or the cwd.
pub fn html_file(root: &Path) -> PathBuf {
    boxset::config::resolve_against(root, Path::new(HTML_FILE))
}

/// Exits if `file` can't be written, so a bad path doesn't cost a full
/// encode. Leaves no empty file behind.
pub fn ensure_writable(file: &Path) {
    let existed = file.exists();
    match OpenOptions::new().append(true).create(true).open(file) {
        Ok(_) if !existed => {
            let _ = std::fs::remove_file(file);
        }
        Ok(_) => {}
        Err(e) => errors::fail_with_note(failure_note(file, e.to_string())),
    }
}

/// `names` is indexed by a task's target. The error is the cause, for
/// `print_result`.
pub fn write_html(
    plan: &boxset::Plan,
    names: &[String],
    file: &Path,
    base_url: Option<&str>,
) -> Result<(), String> {
    let html = boxset::html::render(plan, names, base_url)
        .map_err(|e| format!("ffprobe couldn't read {}", e.path.display()))?;
    std::fs::write(file, html).map_err(|e| e.to_string())
}

pub fn print_result(file: &Path, result: &Result<(), String>) {
    println!();
    match result {
        Ok(()) => println!("  Find HTML snippets in {}.", bold(&directory(file))),
        Err(cause) => style::print_notes(&[failure_note(file, cause.clone())]),
    }
}

fn failure_note(file: &Path, cause: String) -> Note {
    Note {
        severity: Severity::Error,
        locator: None,
        message: format!("couldn't write {}", directory(file)),
        detail: vec![],
        cause: Some(cause),
    }
}
