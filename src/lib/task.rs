//! A `Task` is one output: one rendition, one poster, or one target's subtitles.

use std::path::PathBuf;
use std::sync::Arc;

use crate::config::{Codec, CodecOverrides, Quality, WhisperModel};
use crate::settings::{AudioSettings, Crop, Fps, TimeRange, Timestamp};
use crate::sources::Probe;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId {
    pub target: usize,
    pub kind: TaskKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskKind {
    Rendition { width: u32, codec: Codec },
    Poster { width: u32 },
    Subtitles,
}

#[derive(Debug, Clone)]
pub struct Task {
    pub id: TaskId,
    pub probe: Arc<Probe>,
    pub output_path: PathBuf,
    pub exists: bool,
    pub work: TaskWork,
}

#[derive(Debug, Clone)]
pub enum TaskWork {
    Rendition {
        codec: Codec,
        width: u32,
        quality: Quality,
        overrides: CodecOverrides,
        trim: Option<TimeRange>,
        crop: Option<Crop>,
        fps: Option<Fps>,
        audio: Option<AudioSettings>,
    },
    Poster {
        width: u32,
        at: Timestamp,
        crop: Option<Crop>,
    },
    Subtitles {
        language: Option<String>,
        model: WhisperModel,
        /// Transcription covers the trimmed window, so cue timings line up
        /// with the renditions rather than the source.
        trim: Option<TimeRange>,
        extra_args: Vec<String>,
    },
}
