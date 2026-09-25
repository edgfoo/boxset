//! Where the backend's errors become sentences

use boxset::error::{BoxsetError, TranscribeError};
use boxset::problem::{Problem, ProblemKind, Severity};
use boxset::sources::ProbeErrorKind;
use boxset::task::TaskKind;

use super::live::codec_name;
use super::style::Note;
use super::units::{filename, plural};

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

pub fn problem_note(
    problem: &Problem,
    sources: &[Option<std::path::PathBuf>],
    config: Option<&std::path::Path>,
) -> Note {
    let names_its_own_source = matches!(
        problem.kind,
        ProblemKind::SrcMissing | ProblemKind::SrcUnprobeable { .. }
    );

    let locator = match (names_its_own_source, problem.target) {
        (true, _) => None,
        (false, Some(index)) => sources
            .get(index)
            .and_then(|src| src.as_ref())
            .map(|src| filename(src))
            .or_else(|| Some(format!("target {}", index + 1))),
        (false, None) => config.map(filename),
    };

    let (message, detail) = problem_wording(&problem.kind);
    Note {
        severity: problem.severity,
        locator,
        message,
        detail,
    }
}

fn problem_wording(kind: &ProblemKind) -> (String, Vec<String>) {
    match kind {
        ProblemKind::SrcMissing => (
            "no source file given".to_string(),
            vec!["Every target needs a src.".to_string()],
        ),
        ProblemKind::SrcUnprobeable { path, reason } => {
            let detail = match reason {
                ProbeErrorKind::NotFound => "There's no file at that path.",
                ProbeErrorKind::Unreadable => "boxset couldn't run ffprobe to inspect it.",
                ProbeErrorKind::Unparseable => {
                    "ffprobe couldn't make sense of it, so it may be corrupt or not a video at all."
                }
            };
            (
                format!("can't read {}", path.display()),
                vec![detail.to_string()],
            )
        }
        ProblemKind::OutputCollision { other, path } => (
            "two targets write the same file".to_string(),
            vec![
                format!(
                    "{} is also written by target {}.",
                    path.display(),
                    other + 1
                ),
                "Give one of them a distinct name.".to_string(),
            ],
        ),
        ProblemKind::CodecOverrideForExcludedCodec => (
            "settings for a codec this target doesn't use".to_string(),
            vec!["Add the codec to codecs, or drop its settings.".to_string()],
        ),
        ProblemKind::OutDirNotWritable { path } => (
            format!("boxset can't write to {}", path.display()),
            vec!["Check the directory's permissions.".to_string()],
        ),
        ProblemKind::AudioSettingOnSilentSource => (
            "audio settings on a video with no audio track".to_string(),
            vec!["They'll be ignored.".to_string()],
        ),
        ProblemKind::UnknownField { name, suggestion } => {
            let detail = match suggestion {
                Some(guess) => vec![format!("Did you mean `{guess}`?")],
                None => Vec::new(),
            };
            (format!("unknown setting `{name}`"), detail)
        }
        ProblemKind::MalformedValue { value, expected } => (
            format!("`{value}` isn't valid here"),
            vec![format!("Expected {expected}.")],
        ),
        ProblemKind::TargetFieldAtTopLevel { name } => (
            format!("`{name}` is a target setting, but it's at the top level"),
            vec![
                "Move it under a [[target]] table, or under [defaults] to set it for every target."
                    .to_string(),
            ],
        ),
        ProblemKind::FieldNotAllowedInDefaults { name } => (
            format!("`{name}` can't go in [defaults]"),
            vec!["Set it on each [[target]] instead.".to_string()],
        ),
        ProblemKind::WidthsExceedSource { widths, available } => {
            let list = widths
                .iter()
                .map(|w| w.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            (
                format!("the source is only {available}px wide, {list} would be upscaled"),
                vec!["Upscaling makes bigger files without adding detail.".to_string()],
            )
        }
    }
}

pub fn task_note(error: &BoxsetError) -> Note {
    let (message, detail) = error_wording(error);
    Note {
        severity: Severity::Error,
        locator: None,
        message,
        detail,
    }
}

fn error_wording(error: &BoxsetError) -> (String, Vec<String>) {
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
            detail.push("Try: --verbose   to see ffmpeg's own output".to_string());
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
                format!(
                    "ffmpeg is missing {} {}",
                    encoders.len(),
                    plural(encoders.len(), "encoder")
                ),
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

pub fn toml_note(path: &std::path::Path, text: &str, error: &toml::de::Error) -> Note {
    // toml's `Display` renders a caret diagram we don't print, so build the
    // note from `message()` and `span()` instead
    let locator = match error.span() {
        Some(span) => {
            let line = text[..span.start.min(text.len())].lines().count().max(1);
            format!("{}  line {line}", filename(path))
        }
        None => filename(path),
    };

    Note {
        severity: Severity::Error,
        locator: Some(locator),
        message: error.message().trim().to_string(),
        detail: Vec::new(),
    }
}

/// clap's parse failures, reworded so an unknown flag reads like
/// every other boxset error.
pub fn fail_parse(error: &clap::Error) -> ! {
    use clap::error::{ContextKind, ErrorKind};

    // clap reports a successful `--help` or `--version` as an error
    // too, which is why those exit 0 here.
    if matches!(
        error.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    ) {
        let _ = error.print();
        std::process::exit(0);
    }

    let offender = error
        .get(ContextKind::InvalidArg)
        .or_else(|| error.get(ContextKind::InvalidValue))
        .map(|value| value.to_string());

    let note = match (error.kind(), offender) {
        (ErrorKind::UnknownArgument, Some(flag)) => Note {
            severity: Severity::Error,
            locator: None,
            message: format!("boxset doesn't have a {flag} flag"),
            detail: vec!["Try: boxset help".to_string()],
        },
        (ErrorKind::InvalidValue, Some(value)) => Note {
            severity: Severity::Error,
            locator: None,
            message: format!("`{value}` isn't valid here"),
            detail: vec!["Try: boxset help".to_string()],
        },
        _ => Note {
            severity: Severity::Error,
            locator: None,
            message: clap_problem_line(error),
            detail: vec!["Try: boxset help".to_string()],
        },
    };

    fail_with_note(note);
}

/// clap renders a usage block and tips below the problem itself
fn clap_problem_line(error: &clap::Error) -> String {
    error
        .to_string()
        .lines()
        .next()
        .unwrap_or("that isn't a valid command")
        .trim_start_matches("error: ")
        .to_string()
}

pub fn fail_with_note(note: Note) -> ! {
    println!();
    super::style::print_notes(&[note]);
    println!();
    std::process::exit(1);
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
