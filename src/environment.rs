//! What a plan needs beyond the tasks themselves: ffmpeg, ffprobe, and any
//! Whisper model tier not yet cached.

use std::io::{Read, Write};
use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::config::WhisperModel;
use crate::error::{BoxsetError, FetchError, Tool};
use crate::plan::Plan;
use crate::report::{Phase, Reporter};
use crate::task::TaskWork;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Requirement {
    Tool(Tool),
    Model(WhisperModel),
}

/// Inspect only: touches the filesystem for cache presence but runs no
/// subprocess and downloads nothing, so a dry run can call it freely.
pub fn check_environment(plan: &Plan) -> Vec<Requirement> {
    let mut requirements = Vec::new();

    for tool in [Tool::Ffprobe, Tool::Ffmpeg] {
        if resolve_tool_path(tool).is_none() {
            requirements.push(Requirement::Tool(tool));
        }
    }

    // Distinct tiers only: several targets asking for `small` share one download.
    for task in &plan.tasks {
        let TaskWork::Subtitles { model, .. } = task.work else {
            continue;
        };
        let requirement = Requirement::Model(model);
        if !requirements.contains(&requirement) && !is_cached(model) {
            requirements.push(requirement);
        }
    }

    requirements
}

/// Acquire what's listed. A missing tool is fatal here: boxset ships ffmpeg
/// and ffprobe, so their absence means a broken install, not something to fetch.
pub fn ensure(
    requirements: &[Requirement],
    reporter: &mut dyn Reporter,
) -> Result<(), BoxsetError> {
    if let Some(Requirement::Tool(tool)) = requirements
        .iter()
        .find(|r| matches!(r, Requirement::Tool(_)))
    {
        return Err(BoxsetError::ToolMissing { tool: *tool });
    }

    let models: Vec<WhisperModel> = requirements
        .iter()
        .filter_map(|r| match r {
            Requirement::Model(m) => Some(*m),
            Requirement::Tool(_) => None,
        })
        .collect();

    if models.is_empty() {
        return Ok(());
    }

    reporter.phase(Phase::FetchingModels);

    for model in models {
        let requirement = Requirement::Model(model);
        reporter.requirement_started(&requirement);
        let outcome = install_model(model, reporter);
        reporter.requirement_finished(&requirement, &outcome);
        outcome?;
    }

    Ok(())
}

/// Resolves a tool's path: a `bin/` directory next to boxset's own
/// executable first, falling back to `PATH`.
pub fn resolve_tool_path(tool: Tool) -> Option<PathBuf> {
    let name = match tool {
        Tool::Ffmpeg => "ffmpeg",
        Tool::Ffprobe => "ffprobe",
    };

    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let bundled = dir.join("bin").join(name);
        if bundled.is_file() {
            return Some(bundled);
        }
    }

    which_on_path(name)
}

fn which_on_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub tier: WhisperModel,
    pub cached: bool,
    pub path: PathBuf,
    pub size_bytes: Option<u64>,
}

pub const ALL_MODELS: [WhisperModel; 5] = [
    WhisperModel::Tiny,
    WhisperModel::Base,
    WhisperModel::Small,
    WhisperModel::Medium,
    WhisperModel::Large,
];

const MODEL_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// The GGML file for a tier. `large` has no unversioned file in the upstream
/// repo, so it pins a version: turbo, which is half the size of large-v3 for
/// a slight accuracy cost.
fn model_file_name(tier: WhisperModel) -> &'static str {
    match tier {
        WhisperModel::Tiny => "ggml-tiny.bin",
        WhisperModel::Base => "ggml-base.bin",
        WhisperModel::Small => "ggml-small.bin",
        WhisperModel::Medium => "ggml-medium.bin",
        WhisperModel::Large => "ggml-large-v3-turbo.bin",
    }
}

/// SHA-256 per tier, read from the upstream repo's git-lfs metadata. They are
/// upstream's record of the bytes, not a digest boxset computed itself — the
/// first real download of a tier is what confirms one end to end.
fn model_sha256(tier: WhisperModel) -> &'static str {
    match tier {
        WhisperModel::Tiny => "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
        WhisperModel::Base => "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
        WhisperModel::Small => "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        WhisperModel::Medium => "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208",
        WhisperModel::Large => "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69",
    }
}

/// Expected size, used only to report download progress before any bytes
/// arrive; verification is by digest.
fn model_size_bytes(tier: WhisperModel) -> u64 {
    match tier {
        WhisperModel::Tiny => 77_691_713,
        WhisperModel::Base => 147_951_465,
        WhisperModel::Small => 487_601_967,
        WhisperModel::Medium => 1_533_763_059,
        WhisperModel::Large => 1_624_555_275,
    }
}

pub fn model_path(tier: WhisperModel) -> PathBuf {
    cache_dir().join(model_file_name(tier))
}

fn is_cached(tier: WhisperModel) -> bool {
    model_path(tier).is_file()
}

pub fn list_models() -> Vec<ModelInfo> {
    ALL_MODELS
        .iter()
        .map(|&tier| {
            let path = model_path(tier);
            let size_bytes = std::fs::metadata(&path).ok().map(|m| m.len());
            ModelInfo {
                tier,
                cached: size_bytes.is_some(),
                path,
                size_bytes,
            }
        })
        .collect()
}

pub fn install_model(tier: WhisperModel, reporter: &mut dyn Reporter) -> Result<(), BoxsetError> {
    if is_cached(tier) {
        return Ok(());
    }

    let dir = cache_dir();
    let path = model_path(tier);
    std::fs::create_dir_all(&dir).map_err(|source| BoxsetError::WriteFailed {
        path: dir.clone(),
        source,
    })?;

    // Downloaded beside the final path and renamed on success, so an
    // interrupted fetch never leaves a half file where a valid model was.
    let tmp = path.with_extension("bin.partial");
    let result = download_verified(tier, &tmp, reporter);

    match result {
        Ok(()) => std::fs::rename(&tmp, &path).map_err(|source| BoxsetError::WriteFailed {
            path: path.clone(),
            source,
        }),
        Err(source) => {
            let _ = std::fs::remove_file(&tmp);
            Err(BoxsetError::ModelFetchFailed {
                model: tier,
                source,
            })
        }
    }
}

fn download_verified(
    tier: WhisperModel,
    tmp: &std::path::Path,
    reporter: &mut dyn Reporter,
) -> Result<(), FetchError> {
    let url = format!("{}/{}", MODEL_BASE_URL, model_file_name(tier));
    let mut body = ureq::get(&url).call()?.into_body();
    let total = body.content_length().unwrap_or(model_size_bytes(tier));

    let mut reader = body.as_reader();
    let mut file = std::fs::File::create(tmp)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut done: u64 = 0;
    let requirement = Requirement::Model(tier);

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])?;
        done += n as u64;
        reporter.requirement_progress(&requirement, done as f32 / total.max(1) as f32);
    }
    file.flush()?;

    let actual = hex(&hasher.finalize());
    let expected = model_sha256(tier);
    if actual != expected {
        return Err(FetchError::ChecksumMismatch {
            expected: expected.to_string(),
            actual,
        });
    }

    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn remove_model(tier: WhisperModel) -> Result<(), BoxsetError> {
    let path = model_path(tier);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(BoxsetError::WriteFailed { path, source }),
    }
}

/// Not configurable: the cache is keyed by tier and shared across projects,
/// so every target on the machine asking for `small` reuses one download.
fn cache_dir() -> PathBuf {
    let base = directories::BaseDirs::new()
        .map(|d| d.cache_dir().to_path_buf())
        .unwrap_or_else(std::env::temp_dir);
    base.join("boxset").join("models")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::sources::Probe;
    use crate::task::{Task, TaskId, TaskKind};

    fn subtitles_task(target: usize, model: WhisperModel) -> Task {
        Task {
            id: TaskId {
                target,
                kind: TaskKind::Subtitles,
            },
            probe: Arc::new(Probe {
                src: PathBuf::from("in.mp4"),
                width: 1920,
                height: 1080,
                duration_secs: 10.0,
                frame_rate: (25, 1),
                has_audio: true,
            }),
            output_path: PathBuf::from("out.vtt"),
            exists: false,
            work: TaskWork::Subtitles {
                language: None,
                model,
                trim: None,
                extra_args: Vec::new(),
            },
        }
    }

    fn models_of(plan: &Plan) -> Vec<WhisperModel> {
        check_environment(plan)
            .into_iter()
            .filter_map(|r| match r {
                Requirement::Model(m) => Some(m),
                Requirement::Tool(_) => None,
            })
            .collect()
    }

    #[test]
    fn repeated_tier_is_required_once() {
        let plan = Plan {
            tasks: vec![
                subtitles_task(0, WhisperModel::Small),
                subtitles_task(1, WhisperModel::Small),
                subtitles_task(2, WhisperModel::Medium),
            ],
        };

        let models = models_of(&plan);
        let uncached: Vec<_> = [WhisperModel::Small, WhisperModel::Medium]
            .into_iter()
            .filter(|&t| !is_cached(t))
            .collect();
        assert_eq!(models, uncached);
    }

    #[test]
    fn plan_without_subtitles_needs_no_model() {
        let plan = Plan { tasks: Vec::new() };
        assert!(models_of(&plan).is_empty());
    }

    #[test]
    fn missing_tool_is_fatal_and_skips_downloads() {
        struct Silent;
        impl Reporter for Silent {}

        let requirements = vec![
            Requirement::Model(WhisperModel::Tiny),
            Requirement::Tool(Tool::Ffmpeg),
        ];
        let err = ensure(&requirements, &mut Silent).unwrap_err();
        assert!(matches!(
            err,
            BoxsetError::ToolMissing { tool: Tool::Ffmpeg }
        ));
    }

    #[test]
    fn ensure_with_nothing_missing_is_ok() {
        struct Silent;
        impl Reporter for Silent {}
        assert!(ensure(&[], &mut Silent).is_ok());
    }

    #[test]
    fn each_tier_has_a_distinct_file_and_digest() {
        for tier in ALL_MODELS {
            assert_eq!(model_sha256(tier).len(), 64);
            let same_file = ALL_MODELS
                .iter()
                .filter(|&&t| model_file_name(t) == model_file_name(tier))
                .count();
            assert_eq!(same_file, 1, "{tier:?} shares a file with another tier");
        }
    }
}
