//! Where the backend's errors become sentences

use boxset::error::{BoxsetError, TranscribeError};
use boxset::task::TaskKind;

use super::live::codec_name;

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

pub fn explain(error: &BoxsetError) -> (String, Vec<String>) {
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
