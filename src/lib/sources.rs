//! Probing: `Sources` is the store of what ffprobe said about each path.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;

use serde::Deserialize;

use crate::environment::resolve_tool_path;
use crate::error::Tool;

const WORKER_COUNT: usize = 5;

#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    pub src: PathBuf,
    pub width: u32,
    pub height: u32,
    pub duration_secs: f64,
    pub frame_rate: (u32, u32),
    pub has_audio: bool,
    /// ffprobe's `codec_name` for the video stream, e.g. `h264`.
    pub video_codec: String,
    /// `None` when the source is silent.
    pub audio_codec: Option<String>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeErrorKind {
    NotFound,
    Unreadable,
    Unparseable,
}

#[derive(Debug, Clone)]
pub enum SourceState {
    Pending,
    Probed(Probe),
    Failed(ProbeErrorKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceHash(pub String);

enum WorkerMessage {
    Path(PathBuf),
    Shutdown,
}

/// Fills incrementally: `request` starts work and returns immediately,
/// `absorb`/`wait` drain results as they land. Owns a fixed pool of worker
/// threads for its lifetime, so one path never spawns one thread — probing a
/// large directory with unbounded threads thrashes the disk instead of
/// saturating it.
pub struct Sources {
    states: HashMap<PathBuf, SourceState>,
    work_tx: Sender<WorkerMessage>,
    result_rx: Receiver<(PathBuf, SourceState)>,
    workers: Vec<JoinHandle<()>>,
    in_flight: usize,
}

impl Sources {
    pub fn new() -> Self {
        let (work_tx, work_rx) = mpsc::channel::<WorkerMessage>();
        let (result_tx, result_rx) = mpsc::channel();
        let work_rx = std::sync::Arc::new(std::sync::Mutex::new(work_rx));

        let workers = (0..WORKER_COUNT)
            .map(|_| {
                let work_rx = work_rx.clone();
                let result_tx: Sender<(PathBuf, SourceState)> = result_tx.clone();
                std::thread::spawn(move || {
                    loop {
                        let message = { work_rx.lock().unwrap().recv() };
                        match message {
                            Ok(WorkerMessage::Path(path)) => {
                                let state = probe_path(&path);
                                let _ = result_tx.send((path, state));
                            }
                            Ok(WorkerMessage::Shutdown) | Err(_) => break,
                        }
                    }
                })
            })
            .collect();

        Self {
            states: HashMap::new(),
            work_tx,
            result_rx,
            workers,
            in_flight: 0,
        }
    }

    /// Idempotent per path: one already known or in flight is skipped.
    pub fn request(&mut self, paths: &[PathBuf]) {
        for path in paths {
            if self.states.contains_key(path) {
                continue;
            }
            self.states.insert(path.clone(), SourceState::Pending);
            self.in_flight += 1;
            let _ = self.work_tx.send(WorkerMessage::Path(path.clone()));
        }
    }

    pub fn absorb(&mut self) {
        while let Ok((path, state)) = self.result_rx.try_recv() {
            self.in_flight -= 1;
            self.states.insert(path, state);
        }
    }

    pub fn wait(&mut self) {
        while self.in_flight > 0 {
            match self.result_rx.recv() {
                Ok((path, state)) => {
                    self.in_flight -= 1;
                    self.states.insert(path, state);
                }
                Err(_) => break,
            }
        }
    }

    pub fn settled(&self) -> bool {
        self.in_flight == 0
    }

    pub fn get(&self, path: &Path) -> Option<&SourceState> {
        self.states.get(path)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Path, &SourceState)> {
        self.states.iter().map(|(p, s)| (p.as_path(), s))
    }
}

pub trait SourceLookup {
    fn get(&self, path: &Path) -> Option<&SourceState>;
}

impl SourceLookup for Sources {
    fn get(&self, path: &Path) -> Option<&SourceState> {
        Sources::get(self, path)
    }
}

impl SourceLookup for HashMap<PathBuf, SourceState> {
    fn get(&self, path: &Path) -> Option<&SourceState> {
        HashMap::get(self, path)
    }
}

impl Default for Sources {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Sources {
    fn drop(&mut self) {
        for _ in &self.workers {
            let _ = self.work_tx.send(WorkerMessage::Shutdown);
        }
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    streams: Vec<FfprobeStream>,
    format: FfprobeFormat,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    codec_type: String,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    r_frame_rate: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FfprobeFormat {
    duration: Option<String>,
    size: Option<String>,
}

fn probe_path(path: &Path) -> SourceState {
    if !path.is_file() {
        return SourceState::Failed(ProbeErrorKind::NotFound);
    }

    let Some(ffprobe) = resolve_tool_path(Tool::Ffprobe) else {
        return SourceState::Failed(ProbeErrorKind::Unreadable);
    };

    let output = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .output();

    let Ok(output) = output else {
        return SourceState::Failed(ProbeErrorKind::Unreadable);
    };

    // ffprobe ran and rejected the file: it read the bytes and they weren't video.
    if !output.status.success() {
        return SourceState::Failed(ProbeErrorKind::Unparseable);
    }

    let Ok(parsed) = serde_json::from_slice::<FfprobeOutput>(&output.stdout) else {
        return SourceState::Failed(ProbeErrorKind::Unparseable);
    };

    let Some(video) = parsed.streams.iter().find(|s| s.codec_type == "video") else {
        return SourceState::Failed(ProbeErrorKind::Unparseable);
    };

    let (Some(width), Some(height)) = (video.width, video.height) else {
        return SourceState::Failed(ProbeErrorKind::Unparseable);
    };

    let frame_rate = video
        .r_frame_rate
        .as_deref()
        .and_then(parse_frame_rate)
        .unwrap_or((0, 1));

    let Some(duration_secs) = parsed
        .format
        .duration
        .as_deref()
        .and_then(|d| d.parse().ok())
    else {
        return SourceState::Failed(ProbeErrorKind::Unparseable);
    };

    let audio_codec = parsed
        .streams
        .iter()
        .find(|s| s.codec_type == "audio")
        .map(|s| {
            s.codec_name
                .clone()
                .unwrap_or_else(|| "unknown".to_string())
        });

    let size_bytes = parsed
        .format
        .size
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    SourceState::Probed(Probe {
        src: path.to_path_buf(),
        width,
        height,
        duration_secs,
        frame_rate,
        has_audio: audio_codec.is_some(),
        video_codec: video
            .codec_name
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        audio_codec,
        size_bytes,
    })
}

/// ffprobe reports frame rate as a "num/den" string.
fn parse_frame_rate(raw: &str) -> Option<(u32, u32)> {
    let (num, den) = raw.split_once('/')?;
    Some((num.parse().ok()?, den.parse().ok()?))
}

#[cfg(test)]
mod probe_tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn probes_a_video_with_audio() {
        let state = probe_path(&fixture("bear.mp4"));
        let SourceState::Probed(probe) = state else {
            panic!("expected Probed, got {state:?}");
        };
        assert_eq!(probe.width, 320);
        assert_eq!(probe.height, 180);
        assert!(probe.has_audio);
        assert!(probe.duration_secs > 0.0);
        assert_eq!(probe.video_codec, "h264");
        assert_eq!(probe.audio_codec.as_deref(), Some("aac"));
        assert!(probe.size_bytes > 0);
    }

    #[test]
    fn probes_a_silent_video() {
        let state = probe_path(&fixture("bear_silent.mp4"));
        let SourceState::Probed(probe) = state else {
            panic!("expected Probed, got {state:?}");
        };
        assert!(!probe.has_audio);
        assert_eq!(probe.audio_codec, None);
    }

    #[test]
    fn missing_file_is_not_found() {
        let state = probe_path(&fixture("does-not-exist.mp4"));
        assert_eq!(state_kind(&state), Some(ProbeErrorKind::NotFound));
    }

    fn state_kind(state: &SourceState) -> Option<ProbeErrorKind> {
        match state {
            SourceState::Failed(kind) => Some(*kind),
            _ => None,
        }
    }
}
