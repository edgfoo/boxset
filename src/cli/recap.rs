//! The recap section: what the run produced, and what it suggests doing next.

use std::time::Duration;

use boxset::plan::Plan;
use boxset::task::{TaskId, TaskKind};

use super::style::{bold, red};
use super::units::{directory, elapsed, plural, size};

/// eg. `12 videos, 6 posters, 6 VTTs`
fn counts_by_kind(plan: &Plan, produced: &[TaskId]) -> Vec<String> {
    let count = |matches: fn(TaskKind) -> bool| {
        plan.tasks
            .iter()
            .filter(|task| produced.contains(&task.id) && matches(task.id.kind))
            .count()
    };

    [
        (count(|k| matches!(k, TaskKind::Rendition { .. })), "video"),
        (count(|k| matches!(k, TaskKind::Poster { .. })), "poster"),
        (count(|k| matches!(k, TaskKind::Subtitles)), "VTT"),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, word)| format!("{} {}", bold(&n.to_string()), plural(n, word)))
    .collect()
}

#[allow(clippy::too_many_arguments)]
pub fn recap_lines(
    plan: &Plan,
    produced: &[TaskId],
    failed: usize,
    stopped: bool,
    bytes: u64,
    wall: Duration,
    out_dir: &std::path::Path,
) -> Vec<String> {
    let mut lines = Vec::new();

    let made = counts_by_kind(plan, produced);
    if !made.is_empty() {
        lines.push(format!(
            "  {} created in {}.",
            made.join(", "),
            bold(&elapsed(wall)),
        ));
        lines.push(format!("  {} in total.", bold(&size(Some(bytes)))));
    }

    if stopped {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        let outstanding = plan.tasks.len() - produced.len() - failed;
        lines.push(format!(
            "  Stopped with {} of {} {} left to build.",
            bold(&outstanding.to_string()),
            bold(&plan.tasks.len().to_string()),
            plural(plan.tasks.len(), "output"),
        ));
    }

    if failed > 0 {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(format!(
            "  {} {} {} failed.",
            red("✗"),
            bold(&failed.to_string()),
            plural(failed, "output"),
        ));
    }

    if !made.is_empty() {
        lines.push(String::new());
        lines.push(format!("  Find them in {}.", bold(&directory(out_dir))));
    }

    lines
}

pub fn recap_section(succeeded: bool) -> &'static str {
    match succeeded {
        true => "That's a wrap",
        false => "Cut",
    }
}
