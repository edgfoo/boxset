//! What a plan needs beyond the tasks themselves: ffmpeg, ffprobe, and any
//! Whisper model tier not yet cached.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::config::WhisperModel;
use crate::error::{BoxsetError, FetchError, Tool};
use crate::plan::Plan;
use crate::report::{Phase, Reporter};
use crate::task::TaskWork;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Requirement {
    Tool(Tool),
    /// An encoder this plan's ffmpeg calls name, absent from `-encoders`.
    Encoder(&'static str),
    /// An ffmpeg at least `MIN_FFMPEG_VERSION`, when the one found is older.
    FfmpegVersion {
        found: String,
        minimum: (u32, u32),
    },
    Model(WhisperModel),
}

/// Inspect only: interrogates ffmpeg and reads the model cache, but writes
/// nothing and downloads nothing, so a dry run can call it freely.
pub fn check_environment(plan: &Plan) -> Vec<Requirement> {
    let mut requirements = Vec::new();

    for tool in [Tool::Ffprobe, Tool::Ffmpeg] {
        if resolve_tool_path(tool).is_none() {
            requirements.push(Requirement::Tool(tool));
        }
    }

    if let Some(ffmpeg) = resolve_tool_path(Tool::Ffmpeg) {
        check_ffmpeg_version(&ffmpeg, &mut requirements);
        check_encoders(&ffmpeg, plan, &mut requirements);
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

fn check_ffmpeg_version(ffmpeg: &Path, requirements: &mut Vec<Requirement>) {
    let found = crate::lock::tool_version(ffmpeg);
    let Some(version) = parse_version(&found) else {
        return;
    };
    let minimum = crate::command::MIN_FFMPEG_VERSION;
    if version < minimum {
        requirements.push(Requirement::FfmpegVersion { found, minimum });
    }
}

/// An ffmpeg whose version won't parse is not an ffmpeg that's too old.
fn parse_version(version: &str) -> Option<(u32, u32)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    Some((major, minor))
}

/// First occurrence wins, so the order an install is reported in stays stable.
fn deduplicated(names: impl Iterator<Item = &'static str>) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for name in names {
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

fn plan_encoders(plan: &Plan) -> Vec<&'static str> {
    deduplicated(plan.tasks.iter().flat_map(|task| match &task.work {
        TaskWork::Rendition { codec, audio, .. } => {
            let mut wanted = vec![crate::command::video_encoder(*codec)];
            if audio.is_some() {
                wanted.push(crate::command::audio_encoder(*codec));
            }
            wanted
        }
        TaskWork::Poster { .. } => vec![crate::command::POSTER_ENCODER],
        TaskWork::Subtitles { .. } => vec![crate::command::AUDIO_EXTRACT_ENCODER],
    }))
}

/// Every encoder boxset can name, whatever any one plan needs.
pub fn all_encoders() -> Vec<&'static str> {
    deduplicated(
        crate::config::ALL_CODECS
            .iter()
            .flat_map(|&codec| {
                [
                    crate::command::video_encoder(codec),
                    crate::command::audio_encoder(codec),
                ]
            })
            .chain([
                crate::command::POSTER_ENCODER,
                crate::command::AUDIO_EXTRACT_ENCODER,
            ]),
    )
}

/// Which of `all_encoders` this ffmpeg has, or `None` if it wouldn't say.
pub fn encoder_availability(ffmpeg: &Path) -> Option<Vec<(&'static str, bool)>> {
    let available = available_encoders(ffmpeg)?;
    Some(
        all_encoders()
            .into_iter()
            .map(|name| (name, available.contains(name)))
            .collect(),
    )
}

fn check_encoders(ffmpeg: &Path, plan: &Plan, requirements: &mut Vec<Requirement>) {
    let wanted = plan_encoders(plan);
    if wanted.is_empty() {
        return;
    }
    let Some(available) = available_encoders(ffmpeg) else {
        return;
    };
    for encoder in wanted {
        if !available.contains(encoder) {
            requirements.push(Requirement::Encoder(encoder));
        }
    }
}

/// `None` when ffmpeg wouldn't run or said nothing
fn available_encoders(ffmpeg: &Path) -> Option<HashSet<String>> {
    let output = std::process::Command::new(ffmpeg)
        .args(["-hide_banner", "-encoders"])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;

    // " V....D libx264   libx264 H.264 ...": a six-character flags column,
    // then the name. The legend above the `------` separator has neither.
    let names: HashSet<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let flags = fields.next()?;
            let name = fields.next()?;
            (flags.len() == 6 && flags.chars().all(|c| c == '.' || c.is_ascii_alphabetic()))
                .then(|| name.to_string())
        })
        .collect();

    (!names.is_empty()).then_some(names)
}

/// Fails on the requirements boxset can't acquire for itself. Downloads
/// nothing, so a dry run can call it.
pub fn ensure_available(requirements: &[Requirement]) -> Result<(), BoxsetError> {
    if let Some(Requirement::Tool(tool)) = requirements
        .iter()
        .find(|r| matches!(r, Requirement::Tool(_)))
    {
        return Err(BoxsetError::ToolMissing { tool: *tool });
    }

    if let Some(Requirement::FfmpegVersion { found, minimum }) = requirements
        .iter()
        .find(|r| matches!(r, Requirement::FfmpegVersion { .. }))
    {
        return Err(BoxsetError::FfmpegTooOld {
            found: found.clone(),
            minimum: *minimum,
        });
    }

    // One error naming all of them, rather than failing on the first and
    // hiding the rest.
    let encoders: Vec<&'static str> = requirements
        .iter()
        .filter_map(|r| match r {
            Requirement::Encoder(name) => Some(*name),
            _ => None,
        })
        .collect();
    if !encoders.is_empty() {
        return Err(BoxsetError::EncodersMissing { encoders });
    }

    Ok(())
}

pub fn ensure_met(
    requirements: &[Requirement],
    reporter: &mut dyn Reporter,
) -> Result<(), BoxsetError> {
    ensure_available(requirements)?;

    let models: Vec<WhisperModel> = requirements
        .iter()
        .filter_map(|r| match r {
            Requirement::Model(m) => Some(*m),
            _ => None,
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

    // Homebrew symlinks bin/boxset to libexec/boxset, so canonicalize to
    // resolve these links to absolute paths
    if let Ok(exe) = std::env::current_exe()
        && let Ok(exe) = exe.canonicalize()
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

    let actual = crate::lock::hex(&hasher.finalize());
    let expected = model_sha256(tier);
    if actual != expected {
        return Err(FetchError::ChecksumMismatch {
            expected: expected.to_string(),
            actual,
        });
    }

    Ok(())
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
    use crate::config::Codec;
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
                video_codec: "h264".to_string(),
                audio_codec: Some("aac".to_string()),
                size_bytes: 1_000_000,
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
                _ => None,
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

    fn rendition_task(codec: Codec, audio: bool) -> Task {
        let Task { probe, .. } = subtitles_task(0, WhisperModel::Tiny);
        Task {
            id: TaskId {
                target: 0,
                kind: TaskKind::Rendition { width: 640, codec },
            },
            probe,
            output_path: PathBuf::from("out.mp4"),
            exists: false,
            work: TaskWork::Rendition {
                codec,
                width: 640,
                options: Default::default(),
                trim: None,
                crop: None,
                fps: None,
                audio: audio.then(|| crate::settings::AudioSettings {
                    normalize: false,
                    bitrate: "128k".to_string(),
                }),
            },
        }
    }

    /// The point of scoping to the plan: an ffmpeg without libsvtav1 is only a
    /// problem for a run that asked for av1.
    #[test]
    fn only_the_codecs_in_the_plan_are_wanted() {
        let plan = Plan {
            tasks: vec![
                rendition_task(Codec::H264, true),
                rendition_task(Codec::H265, true),
            ],
        };
        assert_eq!(plan_encoders(&plan), ["libx264", "aac", "libx265"]);
    }

    #[test]
    fn a_silent_rendition_wants_no_audio_encoder() {
        let plan = Plan {
            tasks: vec![rendition_task(Codec::Vp9, false)],
        };
        assert_eq!(plan_encoders(&plan), ["libvpx-vp9"]);
    }

    /// An ffmpeg boxset couldn't interrogate reports `unknown`, which must not
    /// read as a version older than the floor.
    #[test]
    fn an_unreadable_version_is_not_too_old() {
        assert_eq!(parse_version("unknown"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn missing_tool_is_fatal_and_skips_downloads() {
        struct Silent;
        impl Reporter for Silent {}

        let requirements = vec![
            Requirement::Model(WhisperModel::Tiny),
            Requirement::Tool(Tool::Ffmpeg),
        ];
        let err = ensure_met(&requirements, &mut Silent).unwrap_err();
        assert!(matches!(
            err,
            BoxsetError::ToolMissing { tool: Tool::Ffmpeg }
        ));
    }

    #[test]
    fn ensure_with_nothing_missing_is_ok() {
        struct Silent;
        impl Reporter for Silent {}
        assert!(ensure_met(&[], &mut Silent).is_ok());
    }

    /// The cache is keyed by filename, so two tiers sharing one would serve
    /// the wrong weights; a shared digest would pass verification for both.
    #[test]
    fn each_tier_has_a_distinct_file_and_digest() {
        for tier in ALL_MODELS {
            let files = ALL_MODELS
                .iter()
                .filter(|&&t| model_file_name(t) == model_file_name(tier))
                .count();
            assert_eq!(files, 1, "{tier:?} shares a file with another tier");

            let digests = ALL_MODELS
                .iter()
                .filter(|&&t| model_sha256(t) == model_sha256(tier))
                .count();
            assert_eq!(digests, 1, "{tier:?} shares a digest with another tier");
        }
    }
}
