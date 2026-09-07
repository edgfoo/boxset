//! Failed effects. Structured and matchable — prose lives in clients.

use std::path::PathBuf;

use thiserror::Error;

use crate::config::WhisperModel;
use crate::task::TaskId;

#[derive(Debug, Error)]
pub enum BoxsetError {
    #[error("encode failed")]
    EncodeFailed {
        task: TaskId,
        stage: Option<&'static str>,
        source: FfmpegError,
    },
    #[error("write failed: {path}")]
    WriteFailed {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("tool missing: {tool:?}")]
    ToolMissing { tool: Tool },
    #[error("model fetch failed: {model:?}")]
    ModelFetchFailed {
        model: WhisperModel,
        #[source]
        source: FetchError,
    },
    #[error("transcription failed")]
    TranscribeFailed {
        task: TaskId,
        #[source]
        source: TranscribeError,
    },
}

#[derive(Debug, Error)]
pub enum TranscribeError {
    /// The cached model file wouldn't load: absent, truncated, or not a GGML model
    #[error("couldn't load the {model:?} model from {}", path.display())]
    ModelLoad {
        model: WhisperModel,
        path: PathBuf,
        source: whisper_rs::WhisperError,
    },
    #[error("whisper failed while transcribing")]
    Inference(#[source] whisper_rs::WhisperError),
    /// Extracting 16kHz mono PCM for whisper is an ffmpeg call
    #[error("couldn't extract audio to transcribe")]
    AudioExtract(#[source] FfmpegError),
    #[error("the source has no audio track to transcribe")]
    NoAudioTrack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Ffmpeg,
    Ffprobe,
}

#[derive(Debug, Error)]
pub enum FetchError {
    #[error("request failed")]
    Request(#[source] Box<ureq::Error>),
    #[error("io error during download")]
    Io(#[from] std::io::Error),
    /// A model whose bytes don't match the recorded digest is a failed
    /// download, never a cache entry to be used anyway.
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
}

impl From<ureq::Error> for FetchError {
    fn from(e: ureq::Error) -> Self {
        FetchError::Request(Box::new(e))
    }
}

#[derive(Debug, Error)]
#[error("ffmpeg failed: {kind:?}")]
pub struct FfmpegError {
    pub kind: FfmpegErrorKind,
    /// Raw stderr, kept whole so a client can surface it unabridged.
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FfmpegErrorKind {
    EncoderMissing { encoder: String },
    NoSuchStream,
    BadFilter,
    InputNotFound,
    InvalidInput,
    Unclassified,
}

/// Matches the line naming the cause, never ffmpeg's final summary line:
/// distinct causes share the same "Invalid argument" ending. Every pattern
/// here comes from a real captured failure, never a guess.
pub fn classify_ffmpeg_failure(stderr: &str) -> FfmpegErrorKind {
    if let Some(encoder) = between(stderr, "Unknown encoder '", "'") {
        return FfmpegErrorKind::EncoderMissing { encoder };
    }
    if stderr.contains("matches no streams") {
        return FfmpegErrorKind::NoSuchStream;
    }
    if stderr.contains("Error initializing filters") {
        return FfmpegErrorKind::BadFilter;
    }
    if stderr.contains("Error opening input: No such file or directory") {
        return FfmpegErrorKind::InputNotFound;
    }
    if stderr.contains("Error opening input: Invalid data found when processing input") {
        return FfmpegErrorKind::InvalidInput;
    }
    FfmpegErrorKind::Unclassified
}

fn between(haystack: &str, prefix: &str, suffix: &str) -> Option<String> {
    let start = haystack.find(prefix)? + prefix.len();
    let rest = &haystack[start..];
    let end = rest.find(suffix)?;
    Some(rest[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ffmpeg-stderr/");
        std::fs::read_to_string(format!("{path}{name}.txt")).unwrap()
    }

    #[test]
    fn classifies_unknown_encoder_and_names_it() {
        assert_eq!(
            classify_ffmpeg_failure(&fixture("encoder-missing")),
            FfmpegErrorKind::EncoderMissing {
                encoder: "libx266".to_string()
            }
        );
    }

    #[test]
    fn classifies_missing_stream() {
        assert_eq!(
            classify_ffmpeg_failure(&fixture("no-such-stream")),
            FfmpegErrorKind::NoSuchStream
        );
    }

    #[test]
    fn classifies_bad_filter() {
        assert_eq!(
            classify_ffmpeg_failure(&fixture("bad-filter")),
            FfmpegErrorKind::BadFilter
        );
    }

    #[test]
    fn classifies_missing_input() {
        assert_eq!(
            classify_ffmpeg_failure(&fixture("input-not-found")),
            FfmpegErrorKind::InputNotFound
        );
    }

    #[test]
    fn classifies_undecodable_input() {
        assert_eq!(
            classify_ffmpeg_failure(&fixture("invalid-input")),
            FfmpegErrorKind::InvalidInput
        );
    }

    /// bad-filter and no-such-stream both end "Invalid argument"; a classifier
    /// reading the last line would collapse them.
    #[test]
    fn causes_sharing_a_summary_line_stay_distinct() {
        assert_ne!(
            classify_ffmpeg_failure(&fixture("bad-filter")),
            classify_ffmpeg_failure(&fixture("no-such-stream"))
        );
    }

    #[test]
    fn unrecognised_stderr_is_unclassified() {
        assert_eq!(
            classify_ffmpeg_failure("something nobody has seen before"),
            FfmpegErrorKind::Unclassified
        );
    }
}
