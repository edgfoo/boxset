//! Live output build information, updated in-place as it runsa.
//!
//! Each target's rows are printed as a group. The whole group is printed
//! when any of the output's begins building, and is redrawn as each output
//! progresses. Without a terminal to redraw (in CI, etc), nothing is printed
//! until a group is done.

use std::io::Write;
use std::time::Duration;

use boxset::config::Codec;
use boxset::environment::Requirement;
use boxset::error::BoxsetError;
use boxset::plan::Plan;
use boxset::report::{Reporter, TaskOutcome, TaskReport};
use boxset::task::{TaskId, TaskKind};

use super::errors::explain;
use super::style::{bold, bold_green, dim, gray, icon, interactive, red};
use super::units::{directory, elapsed, filename, size};

const CURSOR_UP: &str = "\x1b[A";
const CLEAR_LINE: &str = "\x1b[2K";

/// The widest `elapsed` renders: `mm:ss` once a task passes a minute.
const TIME_WIDTH: usize = 5;

struct Row {
    task: TaskId,
    icon: String,
    name: String,
    progress: f32,
    result: Option<TaskReport>,
}

struct Group {
    target: usize,
    rows: Vec<Row>,
    remaining: usize,
}

impl Group {
    fn started(&self) -> bool {
        self.rows
            .iter()
            .any(|row| row.progress > 0.0 || row.result.is_some())
    }
}

pub struct LiveReporter {
    verbose: bool,
    groups: Vec<Group>,
    name_width: usize,
    /// Groups commit in plan order: one that finishes early waits for those
    /// before it, so the transcript reads in the order the config declares.
    next_to_commit: usize,
    /// Lines currently drawn at the bottom of the screen.
    drawn: usize,
}

impl LiveReporter {
    pub fn new(plan: &Plan, verbose: bool) -> Self {
        let mut groups: Vec<Group> = Vec::new();

        for task in &plan.tasks {
            let index = match groups.iter().position(|g| g.target == task.id.target) {
                Some(index) => index,
                None => {
                    groups.push(Group {
                        target: task.id.target,
                        rows: Vec::new(),
                        remaining: 0,
                    });
                    groups.len() - 1
                }
            };

            groups[index].rows.push(Row {
                task: task.id,
                icon: icon(task.id.kind),
                name: filename(&task.output_path),
                progress: 0.0,
                result: None,
            });

            groups[index].remaining += 1;
        }

        let name_width = groups
            .iter()
            .flat_map(|group| &group.rows)
            .map(|row| row.name.chars().count())
            .max()
            .unwrap_or(0);

        Self {
            verbose,
            groups,
            name_width,
            next_to_commit: 0,
            drawn: 0,
        }
    }

    fn find(&mut self, task: TaskId) -> Option<(usize, usize)> {
        self.groups.iter().enumerate().find_map(|(group, g)| {
            g.rows
                .iter()
                .position(|row| row.task == task)
                .map(|row| (group, row))
        })
    }

    fn erase(&mut self) {
        if self.drawn == 0 {
            return;
        }

        let mut out = std::io::stdout().lock();
        for _ in 0..self.drawn {
            let _ = write!(out, "{CURSOR_UP}{CLEAR_LINE}");
        }

        let _ = out.flush();
        self.drawn = 0;
    }

    /// Commits every finished group whose turn has come, then redraws what's
    /// still running beneath it.
    fn advance(&mut self) {
        self.erase();

        while self
            .groups
            .get(self.next_to_commit)
            .is_some_and(|group| group.remaining == 0)
        {
            if self.next_to_commit > 0 {
                println!();
            }

            for line in self.render(self.next_to_commit, false) {
                println!("{line}");
            }

            self.next_to_commit += 1;
        }

        self.redraw();
    }

    /// Draws each started-but-unfinished group at the bottom of the screen.
    /// The live region grows with `jobs` rather than with the plan, since
    /// only the groups actually in flight are drawn.
    fn redraw(&mut self) {
        if !interactive() {
            return;
        }

        let mut lines = Vec::new();
        for index in self.next_to_commit..self.groups.len() {
            if self.groups[index].started() {
                if self.next_to_commit > 0 || !lines.is_empty() {
                    lines.push(String::new());
                }
                lines.extend(self.render(index, true));
            }
        }

        for line in &lines {
            println!("{line}");
        }
        self.drawn = lines.len();
    }

    /// `live` keeps unfinished rows as percentages; the committed copy drops
    /// them, since a failed group would otherwise keep a row at 0% forever.
    fn render(&self, index: usize, live: bool) -> Vec<String> {
        let group = &self.groups[index];
        let mut lines = Vec::new();

        for row in &group.rows {
            let width = self.name_width;
            let Some(report) = &row.result else {
                if live {
                    let percent = format!("{}%", (row.progress * 100.0) as u32);
                    lines.push(format!(
                        "  {} {:<width$}    {:>4}",
                        row.icon,
                        row.name,
                        dim(&percent),
                    ));
                }
                continue;
            };

            match &report.outcome {
                TaskOutcome::Succeeded => lines.push(format!(
                    "  {} {:<width$}    {} {} {}  {}",
                    row.icon,
                    row.name,
                    bold_green("✓"),
                    dim("in"),
                    gray(&format!("{:<TIME_WIDTH$}", elapsed(report.elapsed))),
                    gray(&size(report.bytes)),
                )),
                TaskOutcome::Failed(_) => lines.push(format!(
                    "  {} {:<width$}    {} {}",
                    row.icon,
                    row.name,
                    red("✗"),
                    dim("failed"),
                )),
            }
        }

        if !live {
            lines.extend(self.error_lines(group));
        }

        lines
    }

    /// One explanation per distinct cause.
    /// The same cause on two rows prints once.
    fn error_lines(&self, group: &Group) -> Vec<String> {
        let mut lines = Vec::new();
        let mut seen: Vec<String> = Vec::new();

        for row in &group.rows {
            let Some(report) = &row.result else {
                continue;
            };
            let TaskOutcome::Failed(error) = &report.outcome else {
                continue;
            };

            let (headline, detail) = explain(error);
            if seen.contains(&headline) {
                continue;
            }
            seen.push(headline.clone());

            lines.push(String::new());
            lines.push(format!("  {} {headline}", red("Error:")));

            for line in detail {
                lines.push(format!("    {}", dim(&line)));
            }

            if self.verbose
                && let BoxsetError::EncodeFailed { source, .. } = error
            {
                for line in source.stderr.lines() {
                    lines.push(format!("    {}", dim(line)));
                }
            }
        }

        lines
    }
}

impl Reporter for LiveReporter {
    fn requirement_started(&mut self, req: &Requirement) {
        if let Requirement::Model(tier) = req {
            self.erase();
            println!("  {}", dim(&format!("downloading {tier:?} model")));
        }
    }

    fn task_started(&mut self, task: TaskId) {
        // A group appears as soon as one of its tasks claims a worker, so the
        // run looks busy before ffmpeg's first progress line arrives.
        if self.find(task).is_some() {
            self.advance();
        }
    }

    fn task_progress(&mut self, task: TaskId, _stage: &'static str, overall: f32, _done: f32) {
        let Some((group, row)) = self.find(task) else {
            return;
        };

        let row = &mut self.groups[group].rows[row];
        // ffmpeg reports often; only a changed whole percent is worth a redraw.
        if (overall * 100.0) as u32 == (row.progress * 100.0) as u32 {
            return;
        }

        row.progress = overall;

        self.erase();
        self.redraw();
    }

    fn task_finished(&mut self, task: TaskId, report: TaskReport) {
        let Some((group, row)) = self.find(task) else {
            return;
        };

        let row = &mut self.groups[group].rows[row];

        row.result = Some(report);
        row.progress = 1.0;

        self.groups[group].remaining -= 1;
        self.advance();
    }
}

/// What the run made, broken down by kind: `12 videos, 6 posters, 6 VTTs`.
fn made(plan: &Plan, produced: &[TaskId]) -> Vec<String> {
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

pub fn closing_lines(
    plan: &Plan,
    produced: &[TaskId],
    failed: usize,
    bytes: u64,
    wall: Duration,
    out_dir: &std::path::Path,
) -> Vec<String> {
    let mut lines = Vec::new();

    let made = made(plan, produced);
    if !made.is_empty() {
        lines.push(format!(
            "  {} created in {}.",
            made.join(", "),
            bold(&elapsed(wall)),
        ));
        lines.push(format!("  {} in total.", bold(&size(Some(bytes)))));
    }

    if failed > 0 {
        lines.push(String::new());
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

/// The closing section's name, chosen by whether anything failed.
pub fn closing_section(failed: usize) -> &'static str {
    match failed {
        0 => "That's a wrap",
        _ => "Cut",
    }
}

fn plural(count: usize, word: &str) -> String {
    match count {
        1 => word.to_string(),
        _ => format!("{word}s"),
    }
}

pub fn codec_name(codec: Codec) -> &'static str {
    match codec {
        Codec::H264 => "H.264",
        Codec::H265 => "H.265",
        Codec::Vp9 => "VP9",
        Codec::Av1 => "AV1",
    }
}
