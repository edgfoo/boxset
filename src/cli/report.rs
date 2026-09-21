//! Line output: a block per target, printed when that target's tasks have all
//! finished. Never assumes a terminal it can redraw — this is the client that
//! has to work in a CI log and in a pipe.

use std::path::Path;
use std::time::Duration;

use boxset::config::Codec;
use boxset::environment::Requirement;
use boxset::error::{BoxsetError, TranscribeError};
use boxset::plan::Plan;
use boxset::report::{Reporter, TaskOutcome, TaskReport};
use boxset::task::{TaskId, TaskKind};

const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

fn bold(text: &str) -> String {
    format!("{BOLD}{text}{RESET}")
}

/// A target's rows, held until every one of its tasks has finished so the
/// block prints together rather than interleaved with another target's.
struct Block {
    header: String,
    rows: Vec<Row>,
    remaining: usize,
}

struct Row {
    task: TaskId,
    name: String,
    result: Option<TaskReport>,
}

pub struct LineReporter {
    verbose: bool,
    blocks: Vec<Block>,
    /// Blocks print in plan order, so one that finishes early waits for those
    /// before it.
    next_to_print: usize,
}

impl LineReporter {
    pub fn new(plan: &Plan, verbose: bool) -> Self {
        let mut blocks: Vec<Block> = Vec::new();
        let mut targets: Vec<usize> = Vec::new();

        for task in &plan.tasks {
            let index = match targets.iter().position(|t| *t == task.id.target) {
                Some(index) => index,
                None => {
                    targets.push(task.id.target);
                    blocks.push(Block {
                        header: header(plan, task.id.target),
                        rows: Vec::new(),
                        remaining: 0,
                    });
                    blocks.len() - 1
                }
            };
            blocks[index].rows.push(Row {
                task: task.id,
                name: filename(&task.output_path),
                result: None,
            });
            blocks[index].remaining += 1;
        }

        Self {
            verbose,
            blocks,
            next_to_print: 0,
        }
    }

    fn flush(&mut self) {
        while let Some(block) = self.blocks.get(self.next_to_print) {
            if block.remaining > 0 {
                break;
            }
            print_block(block, self.verbose);
            self.next_to_print += 1;
        }
    }
}

impl Reporter for LineReporter {
    fn requirement_started(&mut self, req: &Requirement) {
        if let Requirement::Model(tier) = req {
            println!("  downloading {tier:?} model");
        }
    }

    fn task_finished(&mut self, task: TaskId, report: TaskReport) {
        for block in &mut self.blocks {
            let Some(row) = block.rows.iter_mut().find(|r| r.task == task) else {
                continue;
            };
            row.result = Some(report);
            block.remaining -= 1;
            break;
        }
        self.flush();
    }
}

/// `a.mp4, b.mp4 and c.mp4`
pub fn reading_line(sources: &[String]) -> String {
    let list = match sources.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    };
    bold(&format!("Reading {list}..."))
}

pub fn closing_line(succeeded: usize, failed: usize) -> String {
    let total = succeeded + failed;
    let text = match failed {
        0 => format!("all {total} outputs created successfully"),
        _ => format!("{failed} of {total} outputs failed"),
    };
    bold(&text)
}

fn filename(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `interview.mp4 (4 versions, 1 poster, 1 subs)` — a kind with no outputs is
/// left out rather than shown as zero.
fn header(plan: &Plan, target: usize) -> String {
    let tasks = plan.tasks.iter().filter(|t| t.id.target == target);

    let mut src = String::new();
    let (mut versions, mut posters, mut subs) = (0, 0, 0);
    for task in tasks {
        src = filename(&task.probe.src);
        match task.id.kind {
            TaskKind::Rendition { .. } => versions += 1,
            TaskKind::Poster { .. } => posters += 1,
            TaskKind::Subtitles => subs += 1,
        }
    }

    let mut parts = Vec::new();
    if versions > 0 {
        parts.push(format!("{versions} {}", plural(versions, "version")));
    }
    if posters > 0 {
        parts.push(format!("{posters} {}", plural(posters, "poster")));
    }
    if subs > 0 {
        parts.push(format!("{subs} subs"));
    }

    bold(&format!("{src} ({})", parts.join(", ")))
}

fn plural(count: usize, word: &str) -> String {
    match count {
        1 => word.to_string(),
        _ => format!("{word}s"),
    }
}

fn print_block(block: &Block, verbose: bool) {
    println!();
    println!("{}", block.header);
    println!();

    let width = block.rows.iter().map(|r| r.name.len()).max().unwrap_or(0);
    for row in &block.rows {
        let Some(report) = &row.result else {
            continue;
        };
        match &report.outcome {
            TaskOutcome::Succeeded => println!(
                "    {:<width$}  ✓  {}  {}",
                row.name,
                elapsed(report.elapsed),
                size(report.bytes),
            ),
            TaskOutcome::Failed(_) => println!("    {:<width$}  ✗  failed", row.name),
        }
    }

    print_errors(block, verbose);
}

/// One block per distinct cause. The same cause on two targets prints in both:
/// an error belongs with the rows it explains.
fn print_errors(block: &Block, verbose: bool) {
    let mut seen: Vec<String> = Vec::new();

    for row in &block.rows {
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

        println!();
        println!("    Error: {headline}");
        for line in detail {
            println!("      {line}");
        }

        if verbose && let BoxsetError::EncodeFailed { source, .. } = error {
            for line in source.stderr.lines() {
                println!("      {line}");
            }
        }
    }
}

fn elapsed(duration: Duration) -> String {
    let secs = duration.as_secs();
    format!("{}:{:02}", secs / 60, secs % 60)
}

fn size(bytes: Option<u64>) -> String {
    let Some(bytes) = bytes else {
        return String::new();
    };
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;

    let bytes = bytes as f64;
    if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else {
        format!("{:.0} KB", bytes / KB)
    }
}

fn ffmpeg_cause(kind: &boxset::error::FfmpegErrorKind) -> String {
    use boxset::error::FfmpegErrorKind::*;

    match kind {
        EncoderMissing { encoder } => format!("this ffmpeg has no {encoder} encoder"),
        NoSuchStream => "the source has no such stream".to_string(),
        BadFilter => "the crop or scale settings didn't make a valid filter".to_string(),
        InputNotFound => "the source file is missing".to_string(),
        InvalidInput => "the source isn't readable video".to_string(),
        Unclassified => "ffmpeg failed; re-run with --verbose for its output".to_string(),
    }
}

fn explain(error: &BoxsetError) -> (String, Vec<String>) {
    match error {
        BoxsetError::EncodeFailed {
            source,
            stage,
            task,
        } => {
            let codec = match task.kind {
                TaskKind::Rendition { codec, .. } => codec_name(codec),
                TaskKind::Poster { .. } => "the poster",
                TaskKind::Subtitles => "subtitles",
            };
            let mut detail = vec![capitalise(&ffmpeg_cause(&source.kind))];
            if let Some(stage) = stage {
                detail.push(format!("It failed during the {stage} stage."));
            }
            detail.push("Full ffmpeg output: boxset build --verbose".to_string());
            (format!("boxset couldn't encode {codec}"), detail)
        }
        BoxsetError::WriteFailed { path, source } => (
            format!("boxset couldn't write {}", path.display()),
            vec![capitalise(&source.to_string())],
        ),
        BoxsetError::ToolMissing { tool } => (
            format!("boxset couldn't find {}", tool_name(*tool)),
            vec!["It isn't beside boxset or on your PATH.".to_string()],
        ),
        BoxsetError::FfmpegTooOld { found, minimum } => (
            format!("this ffmpeg is too old: {found}"),
            vec![format!(
                "boxset expects version {}.{} and newer.",
                minimum.0, minimum.1
            )],
        ),
        BoxsetError::EncodersMissing { encoders } => {
            let mut detail = vec![
                "The installed ffmpeg was built without the encoders this build needs:".to_string(),
            ];
            for encoder in encoders {
                detail.push(format!("  {encoder} — {}", encoder_purpose(encoder)));
            }
            (
                format!("ffmpeg is missing {} encoder(s)", encoders.len()),
                detail,
            )
        }
        BoxsetError::ModelFetchFailed { model, source } => (
            format!("boxset couldn't fetch the {model:?} model"),
            vec![capitalise(&source.to_string())],
        ),
        BoxsetError::TranscribeFailed { source, .. } => {
            let detail = match source {
                TranscribeError::NoAudioTrack => {
                    vec![
                        "The source has no audio to transcribe.".to_string(),
                        "Try: --no-subs".to_string(),
                    ]
                }
                TranscribeError::ModelLoad { model, .. } => {
                    vec![format!(
                        "The {model:?} model wouldn't load; it may be a bad download."
                    )]
                }
                TranscribeError::AudioExtract(e) => vec![format!(
                    "Couldn't extract audio to transcribe: {}",
                    ffmpeg_cause(&e.kind)
                )],
                TranscribeError::Inference(_) => {
                    vec!["Whisper failed while transcribing.".to_string()]
                }
            };
            ("boxset couldn't make subtitles".to_string(), detail)
        }
    }
}

fn codec_name(codec: Codec) -> &'static str {
    match codec {
        Codec::H264 => "H.264",
        Codec::H265 => "H.265",
        Codec::Vp9 => "VP9",
        Codec::Av1 => "AV1",
    }
}

fn tool_name(tool: boxset::error::Tool) -> &'static str {
    match tool {
        boxset::error::Tool::Ffmpeg => "ffmpeg",
        boxset::error::Tool::Ffprobe => "ffprobe",
    }
}

fn encoder_purpose(encoder: &str) -> &'static str {
    match encoder {
        "libx264" => "H.264 video",
        "libx265" => "H.265 video",
        "libvpx-vp9" => "VP9 video",
        "libsvtav1" => "AV1 video",
        "libopus" => "audio in webm, which VP9 uses",
        "aac" => "audio in mp4",
        "mjpeg" => "poster images",
        "pcm_s16le" => "the audio subtitles are transcribed from",
        _ => "part of this build",
    }
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    let sentence = match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    match sentence.ends_with('.') {
        true => sentence,
        false => sentence + ".",
    }
}
