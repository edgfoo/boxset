//! `boxset.lock`: what produced each output, and with what.
//!
//! Machine-local and uncommitted.

use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{BoxsetError, Tool};
use crate::task::TaskWork;

pub const LOCK_FILE: &str = "boxset.lock";

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Lockfile {
    /// Keyed by output path, so a later run replaces an entry by writing the
    /// same key. Ordered, to keep the file stable across runs.
    #[serde(default, rename = "output")]
    pub outputs: BTreeMap<String, LockEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockEntry {
    pub source_hash: String,
    pub output_hash: String,
    pub args_hash: String,
    pub boxset_version: String,
    pub ffmpeg_version: String,
    /// Exact ffmpeg commands that generated the output, for reference only.
    /// These can vary between identical runs with the same config.
    #[serde(default)]
    pub commands: Vec<String>,
}

impl Lockfile {
    /// A missing or unparseable lockfile reads as an empty one, so a file from
    /// another version can't fail a build.
    pub fn read(dir: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(dir.join(LOCK_FILE)) else {
            return Self::default();
        };
        toml::from_str(&text).unwrap_or_default()
    }

    pub fn write(&self, dir: &Path) -> Result<(), BoxsetError> {
        let path = dir.join(LOCK_FILE);
        let text = toml::to_string_pretty(self).map_err(|e| BoxsetError::WriteFailed {
            path: path.clone(),
            source: std::io::Error::other(e),
        })?;
        std::fs::write(&path, text).map_err(|source| BoxsetError::WriteFailed { path, source })
    }

    /// Adds this run's entries, dropping any whose path is not in
    /// `all_outputs`. A filtered run keeps the entries for outputs it skipped.
    pub fn absorb(
        &mut self,
        entries: impl IntoIterator<Item = (PathBuf, LockEntry)>,
        all_outputs: &[PathBuf],
    ) {
        let keep: HashSet<&str> = all_outputs.iter().filter_map(|p| p.to_str()).collect();
        self.outputs.retain(|path, _| keep.contains(path.as_str()));

        for (path, entry) in entries {
            self.outputs
                .insert(path.to_string_lossy().into_owned(), entry);
        }
    }
}

/// Hash of a file's bytes. `None` when the file can't be read.
pub fn hash_file(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let read = file.read(&mut buf).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Some(hex(&hasher.finalize()))
}

/// Hashes the task's intent, not the command it ran, so two encodes match
/// only when their settings do.
pub fn args_hash(work: &TaskWork) -> String {
    let mut hasher = Sha256::new();
    hasher.update(describe_work(work).as_bytes());
    hex(&hasher.finalize())
}

/// Changing any of these renderings invalidates every existing lockfile entry.
fn describe_crop(crop: Option<crate::settings::Crop>) -> String {
    crop.map(|c| format!("{}:{}:{:?}", c.ratio.0, c.ratio.1, c.anchor))
        .unwrap_or_else(|| "none".to_string())
}

fn describe_trim(trim: Option<crate::settings::TimeRange>) -> String {
    trim.map(|t| format!("{}-{:?}", t.start_secs, t.end_secs))
        .unwrap_or_else(|| "none".to_string())
}

/// A stable rendering of a `TaskWork`. Written out by hand rather than derived
/// from `Debug`, whose output is explicitly not a stable format.
fn describe_work(work: &TaskWork) -> String {
    match work {
        TaskWork::Rendition {
            codec,
            width,
            options,
            trim,
            crop,
            fps,
            audio,
        } => {
            let crop = describe_crop(*crop);
            let trim = describe_trim(*trim);
            let fps = fps
                .map(|f| format!("{}/{}", f.num, f.den))
                .unwrap_or_else(|| "source".to_string());
            let audio = audio
                .as_ref()
                .map(|a| format!("{}:{}", a.bitrate, a.normalize))
                .unwrap_or_else(|| "none".to_string());
            format!(
                "rendition codec={codec:?} width={width} \
                 crf={:?} preset={:?} profile={:?} cpu_used={:?} row_mt={:?} \
                 extra={:?} trim={trim} crop={crop} fps={fps} audio={audio}",
                options.crf,
                options.preset,
                options.profile,
                options.cpu_used,
                options.row_mt,
                options.extra_args,
            )
        }
        TaskWork::Poster { width, at, crop } => {
            let crop = describe_crop(*crop);
            format!("poster width={width} at={} crop={crop}", at.0)
        }
        TaskWork::Subtitles {
            language,
            model,
            max_cue_chars,
            trim,
            extra_args,
        } => {
            let trim = describe_trim(*trim);
            format!(
                "subtitles language={language:?} model={model:?} cue_chars={max_cue_chars:?} trim={trim} extra={extra_args:?}"
            )
        }
    }
}

/// The version string ffmpeg reports, or `unknown` when it can't be read.
pub fn ffmpeg_version() -> String {
    match crate::environment::resolve_tool_path(Tool::Ffmpeg) {
        Some(path) => tool_version(&path),
        None => "unknown".to_string(),
    }
}

pub fn tool_version(path: &Path) -> String {
    let Ok(output) = std::process::Command::new(path)
        .arg("-version")
        .stdin(std::process::Stdio::null())
        .output()
    else {
        return "unknown".to_string();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    // "ffmpeg version 9.0.1 Copyright (c) ..." — the token after "version".
    text.split_whitespace()
        .nth(2)
        .unwrap_or("unknown")
        .to_string()
}

pub fn boxset_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Codec;
    use crate::settings::{CodecOptions, Crop, Timestamp};

    fn rendition(width: u32) -> TaskWork {
        TaskWork::Rendition {
            codec: Codec::H264,
            width,
            options: CodecOptions {
                crf: Some(23),
                ..Default::default()
            },
            trim: None,
            crop: None,
            fps: None,
            audio: None,
        }
    }

    fn entry(args_hash: &str) -> LockEntry {
        LockEntry {
            source_hash: "src".to_string(),
            output_hash: "out".to_string(),
            args_hash: args_hash.to_string(),
            boxset_version: "0.1.0".to_string(),
            ffmpeg_version: "9.0.1".to_string(),
            commands: vec!["ffmpeg -i in.mp4 out.mp4".to_string()],
        }
    }

    #[test]
    fn args_hash_changes_with_the_settings_that_shape_an_encode() {
        assert_ne!(args_hash(&rendition(480)), args_hash(&rendition(960)));

        let mut crf_20 = rendition(480);
        if let TaskWork::Rendition { options, .. } = &mut crf_20 {
            options.crf = Some(20);
        }
        assert_ne!(args_hash(&rendition(480)), args_hash(&crf_20));
    }

    /// A poster and a rendition of the same width are different work, and the
    /// hash has to say so even though both are "480".
    #[test]
    fn task_kinds_do_not_collide() {
        let poster = TaskWork::Poster {
            width: 480,
            at: Timestamp(0.0),
            crop: None,
        };
        assert_ne!(args_hash(&rendition(480)), args_hash(&poster));
    }

    /// A crop is part of what produced an output, so two otherwise identical
    /// renditions differing only in anchor must not share an entry.
    #[test]
    fn crop_anchor_is_part_of_the_hash() {
        let mut centred = rendition(480);
        let mut topped = rendition(480);
        if let TaskWork::Rendition { crop, .. } = &mut centred {
            *crop = Some(Crop {
                ratio: (9, 16),
                anchor: crate::config::Anchor::Centre,
            });
        }
        if let TaskWork::Rendition { crop, .. } = &mut topped {
            *crop = Some(Crop {
                ratio: (9, 16),
                anchor: crate::config::Anchor::Top,
            });
        }
        assert_ne!(args_hash(&centred), args_hash(&topped));
    }

    /// A filtered run rebuilds one output and skips another. The skipped one
    /// keeps its entry, while an output the config has dropped loses its.
    #[test]
    fn absorb_keeps_skipped_outputs_and_drops_unnamed_ones() {
        let (built, skipped, gone) = (
            PathBuf::from("a.mp4"),
            PathBuf::from("b.mp4"),
            PathBuf::from("c.mp4"),
        );

        let mut lock = Lockfile::default();
        let all = [built.clone(), skipped.clone(), gone.clone()];
        lock.absorb(
            [
                (built.clone(), entry("old")),
                (skipped.clone(), entry("untouched")),
                (gone.clone(), entry("dropped")),
            ],
            &all,
        );

        lock.absorb([(built.clone(), entry("new"))], &[built, skipped]);

        assert_eq!(lock.outputs["a.mp4"].args_hash, "new");
        assert_eq!(lock.outputs["b.mp4"].args_hash, "untouched");
        assert!(!lock.outputs.contains_key("c.mp4"));
    }

    #[test]
    fn round_trips_through_toml() {
        let path = PathBuf::from("assets/video/a.mp4");
        let mut lock = Lockfile::default();
        lock.absorb([(path.clone(), entry("h"))], &[path]);

        let text = toml::to_string_pretty(&lock).unwrap();
        let read: Lockfile = toml::from_str(&text).unwrap();
        assert_eq!(read.outputs, lock.outputs);
    }

    /// A lockfile from a future version, or one someone edited badly, must not
    /// fail a build.
    #[test]
    fn a_corrupt_lockfile_reads_as_empty() {
        let dir = std::env::temp_dir().join("boxset-lock-corrupt-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(LOCK_FILE), "this is not toml {{{").unwrap();
        assert!(Lockfile::read(&dir).outputs.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hashing_a_file_reads_its_bytes() {
        let dir = std::env::temp_dir().join("boxset-lock-hash-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.bin");
        std::fs::write(&path, b"hello").unwrap();

        // sha256("hello")
        assert_eq!(
            hash_file(&path).unwrap(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(hash_file(&dir.join("absent.bin")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
