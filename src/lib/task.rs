//! A `Task` is one unit of a target's work.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::{Codec, TranscriptionModel};
use crate::settings::{AudioSettings, CodecOptions, Crop, Fps, TimeRange, Timestamp};
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
    Loudness,
}

#[derive(Debug, Clone)]
pub struct Task {
    pub id: TaskId,
    pub probe: Arc<Probe>,
    pub work: TaskWork,
}

impl Task {
    pub fn output(&self) -> Option<&Output> {
        match &self.work {
            TaskWork::Rendition { output, .. } => Some(output),
            TaskWork::Poster { output, .. } => Some(output),
            TaskWork::Subtitles { output, .. } => Some(output),
            TaskWork::Loudness { .. } => None,
        }
    }

    pub fn output_path(&self) -> Option<&Path> {
        self.output().map(|output| output.path.as_path())
    }

    /// False for work that writes no file
    pub fn exists(&self) -> bool {
        self.output().is_some_and(|output| output.exists)
    }
}

#[derive(Debug, Clone)]
pub struct Output {
    pub path: PathBuf,
    pub exists: bool,
}

#[derive(Debug, Clone)]
pub enum TaskWork {
    Rendition {
        output: Output,
        codec: Codec,
        width: u32,
        options: CodecOptions,
        trim: Option<TimeRange>,
        crop: Option<Crop>,
        fps: Option<Fps>,
        audio: Option<AudioSettings>,
    },
    Poster {
        output: Output,
        width: u32,
        at: Timestamp,
        crop: Option<Crop>,
    },
    Subtitles {
        output: Output,
        model: TranscriptionModel,
        trim: Option<TimeRange>,
        extra_args: Vec<String>,
    },
    Loudness {
        trim: Option<TimeRange>,
    },
}
