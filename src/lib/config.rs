//! `boxset.toml` shape.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

pub const CONFIG_FILE: &str = "boxset.toml";

/// The whole config file: a flat list of targets plus the project-wide keys.
/// `targets` holds what each `[[target]]` literally says, unmerged.
#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_out_dir")]
    pub out_dir: PathBuf,
    pub jobs: Option<usize>,
    #[serde(default)]
    pub defaults: TargetConfig,
    #[serde(rename = "target", default)]
    pub targets: Vec<TargetConfig>,

    /// Leftover top-level keys, for the "target field written at top level" check
    #[serde(flatten)]
    pub unknown: BTreeMap<String, toml::Value>,
}

fn default_out_dir() -> PathBuf {
    PathBuf::from("export")
}

pub fn resolve_against(base: &Path, path: &Path) -> PathBuf {
    match path.is_absolute() {
        true => path.to_path_buf(),
        false => fold_dot_segments(&base.join(path)),
    }
}

pub fn fold_dot_segments(path: &Path) -> PathBuf {
    let mut out: Vec<Component> = Vec::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir if matches!(out.last(), Some(Component::Normal(_))) => {
                out.pop();
            }
            part => out.push(part),
        }
    }
    match out.is_empty() {
        true => PathBuf::from("."),
        false => out.iter().collect(),
    }
}

impl Config {
    /// Re-bases every path the file contains onto `dir`, the directory the config was read from, so
    /// a config describes the same build wherever boxset is run from.
    pub fn rebase(&mut self, dir: &Path) {
        self.out_dir = resolve_against(dir, &self.out_dir);
        for target in &mut self.targets {
            if let Some(src) = &target.src {
                target.src = Some(resolve_against(dir, src));
            }
        }
    }

    /// Every target with `[defaults]` filled in
    pub fn merged_targets(&self) -> Vec<TargetConfig> {
        self.targets
            .iter()
            .map(|target| target.with_defaults(&self.defaults))
            .collect()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TargetConfig {
    pub src: Option<PathBuf>,
    pub name: Option<String>,
    pub quality: Option<Quality>,
    pub codecs: Option<Vec<Codec>>,
    pub crop: Option<Crop>,
    pub widths: Option<Vec<u32>>,
    pub trim: Option<TimeRange>,
    pub fps: Option<Fps>,
    pub audio: Option<AudioField>,
    pub poster: Option<PosterField>,
    pub subtitles: Option<SubtitlesField>,
    pub h264: Option<CodecOverrides>,
    pub h265: Option<CodecOverrides>,
    pub vp9: Option<CodecOverrides>,
    pub av1: Option<CodecOverrides>,

    /// Leftover keys on a target: unknown fields, and the top-level-key-on-a-target check
    #[serde(flatten)]
    pub unknown: BTreeMap<String, toml::Value>,
}

impl TargetConfig {
    /// Fill missing config values with `defaults`.
    /// Lists are replaced, not combined.
    pub fn with_defaults(&self, defaults: &TargetConfig) -> TargetConfig {
        TargetConfig {
            src: self.src.clone(),
            name: self.name.clone(),
            quality: self.quality.or(defaults.quality),
            codecs: or_clone(&self.codecs, &defaults.codecs),
            crop: or_clone(&self.crop, &defaults.crop),
            widths: or_clone(&self.widths, &defaults.widths),
            trim: or_clone(&self.trim, &defaults.trim),
            fps: self.fps.or(defaults.fps),
            audio: merge_toggle(&self.audio, &defaults.audio),
            poster: merge_toggle(&self.poster, &defaults.poster),
            subtitles: merge_toggle(&self.subtitles, &defaults.subtitles),
            h264: merge(&self.h264, &defaults.h264),
            h265: merge(&self.h265, &defaults.h265),
            vp9: merge(&self.vp9, &defaults.vp9),
            av1: merge(&self.av1, &defaults.av1),
            unknown: self.unknown.clone(),
        }
    }
}

fn or_clone<T: Clone>(target: &Option<T>, defaults: &Option<T>) -> Option<T> {
    target.clone().or_else(|| defaults.clone())
}

trait Mergeable: Clone {
    fn merge(&self, defaults: &Self) -> Self;
}

fn merge<T: Mergeable>(target: &Option<T>, defaults: &Option<T>) -> Option<T> {
    match (target, defaults) {
        (Some(target), Some(defaults)) => Some(target.merge(defaults)),
        _ => or_clone(target, defaults),
    }
}

/// Two tables merge. A `false` on either side switches the feature off or on
/// outright, so it never merges with a table.
fn merge_toggle<T: Mergeable>(
    target: &Option<Toggle<T>>,
    defaults: &Option<Toggle<T>>,
) -> Option<Toggle<T>> {
    match (target, defaults) {
        (Some(Toggle::Settings(target)), Some(Toggle::Settings(defaults))) => {
            Some(Toggle::Settings(target.merge(defaults)))
        }
        (Some(field), _) => Some(field.clone()),
        (None, defaults) => defaults.clone(),
    }
}

impl Mergeable for AudioSettings {
    fn merge(&self, defaults: &Self) -> Self {
        AudioSettings {
            normalize: self.normalize.or(defaults.normalize),
            bitrate: or_clone(&self.bitrate, &defaults.bitrate),
        }
    }
}

impl Mergeable for PosterSettings {
    fn merge(&self, defaults: &Self) -> Self {
        PosterSettings {
            at: or_clone(&self.at, &defaults.at),
        }
    }
}

impl Mergeable for SubtitleSettings {
    fn merge(&self, defaults: &Self) -> Self {
        SubtitleSettings {
            model: self.model.or(defaults.model),
        }
    }
}

impl Mergeable for CodecOverrides {
    fn merge(&self, defaults: &Self) -> Self {
        CodecOverrides {
            crf: self.crf.or(defaults.crf),
            preset: or_clone(&self.preset, &defaults.preset),
            profile: or_clone(&self.profile, &defaults.profile),
            cpu_used: self.cpu_used.or(defaults.cpu_used),
            row_mt: self.row_mt.or(defaults.row_mt),
            extra_args: or_clone(&self.extra_args, &defaults.extra_args),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    Low,
    Balanced,
    High,
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    H264,
    H265,
    Vp9,
    Av1,
}

pub const ALL_CODECS: [Codec; 4] = [Codec::H264, Codec::H265, Codec::Vp9, Codec::Av1];

/// What a target encodes when `codecs` is unset: h264 for reach, vp9 for size.
pub const DEFAULT_CODECS: [Codec; 2] = [Codec::H264, Codec::Vp9];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Anchor {
    #[default]
    Centre,
    Top,
    Bottom,
    Left,
    Right,
}

impl Anchor {
    /// Top-left corner of a `w`×`h` window in a `sw`×`sh` source. An anchor
    /// only cuts on the axis the crop shrinks; the other centres.
    pub fn offset(self, sw: u64, sh: u64, w: u64, h: u64) -> (u64, u64) {
        let centre_x = (sw - w) / 2;
        let centre_y = (sh - h) / 2;
        match self {
            Anchor::Centre => (centre_x, centre_y),
            Anchor::Top => (centre_x, 0),
            Anchor::Bottom => (centre_x, sh - h),
            Anchor::Left => (0, centre_y),
            Anchor::Right => (sw - w, centre_y),
        }
    }
}

/// An aspect ratio, either bare (centred) or with an anchor.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Crop {
    Bare(String),
    Anchored {
        ratio: String,
        #[serde(default)]
        anchor: Anchor,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct TimeRange {
    pub start: Option<String>,
    pub end: Option<String>,
}

/// A frame rate, written as a number or fraction string.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fps {
    Decimal(f64),
    Ratio { num: u32, den: u32 },
}

impl<'de> Deserialize<'de> for Fps {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Fps, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Written {
            Number(f64),
            Text(String),
        }

        match Written::deserialize(deserializer)? {
            Written::Number(value) => match value.is_finite() && value > 0.0 {
                true => Ok(Fps::Decimal(value)),
                false => Err(serde::de::Error::custom(FPS_HINT)),
            },
            Written::Text(text) => {
                parse_fps_ratio(&text).ok_or_else(|| serde::de::Error::custom(FPS_HINT))
            }
        }
    }
}

pub const FPS_HINT: &str = "a frame rate like 25, 23.976 or \"30000/1001\"";

/// Only the `num/den` string lands here; a bare number is already a rate.
pub fn parse_fps_ratio(text: &str) -> Option<Fps> {
    let (num, den) = text.split_once('/')?;
    let num: u32 = num.trim().parse().ok()?;
    let den: u32 = den.trim().parse().ok()?;
    match num > 0 && den > 0 {
        true => Some(Fps::Ratio { num, den }),
        false => None,
    }
}

/// `false` to switch off, or a table to configure.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Toggle<T> {
    Off(bool),
    Settings(T),
}

pub type AudioField = Toggle<AudioSettings>;
pub type PosterField = Toggle<PosterSettings>;
pub type SubtitlesField = Toggle<SubtitleSettings>;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AudioSettings {
    pub normalize: Option<bool>,
    pub bitrate: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PosterSettings {
    pub at: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SubtitleSettings {
    pub model: Option<TranscriptionModel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhisperTier {
    Tiny,
    Base,
    Small,
    Medium,
    Large,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParakeetTier {
    /// English only, fast.
    Fast,
    /// 25 European languages.
    Multilingual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptionModel {
    Whisper(WhisperTier),
    Parakeet(ParakeetTier),
}

impl TranscriptionModel {
    pub fn name(self) -> &'static str {
        match self {
            TranscriptionModel::Whisper(WhisperTier::Tiny) => "whisper-tiny",
            TranscriptionModel::Whisper(WhisperTier::Base) => "whisper-base",
            TranscriptionModel::Whisper(WhisperTier::Small) => "whisper-small",
            TranscriptionModel::Whisper(WhisperTier::Medium) => "whisper-medium",
            TranscriptionModel::Whisper(WhisperTier::Large) => "whisper-large",
            TranscriptionModel::Parakeet(ParakeetTier::Fast) => "parakeet-110m",
            TranscriptionModel::Parakeet(ParakeetTier::Multilingual) => "parakeet-0.6b",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        ALL_MODELS.iter().copied().find(|m| m.name() == raw)
    }
}

pub const ALL_MODELS: [TranscriptionModel; 7] = [
    TranscriptionModel::Whisper(WhisperTier::Tiny),
    TranscriptionModel::Whisper(WhisperTier::Base),
    TranscriptionModel::Whisper(WhisperTier::Small),
    TranscriptionModel::Whisper(WhisperTier::Medium),
    TranscriptionModel::Whisper(WhisperTier::Large),
    TranscriptionModel::Parakeet(ParakeetTier::Fast),
    TranscriptionModel::Parakeet(ParakeetTier::Multilingual),
];

impl std::fmt::Display for TranscriptionModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl<'de> Deserialize<'de> for TranscriptionModel {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{Error, Unexpected};

        let raw = String::deserialize(deserializer)?;
        match TranscriptionModel::parse(&raw) {
            Some(model) => Ok(model),
            None => Err(D::Error::invalid_value(
                Unexpected::Str(&raw),
                &model_names().as_str(),
            )),
        }
    }
}

pub fn model_names() -> String {
    ALL_MODELS
        .iter()
        .map(|m| m.name())
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CodecOverrides {
    pub crf: Option<u32>,
    pub preset: Option<String>,
    pub profile: Option<String>,
    pub cpu_used: Option<u32>,
    pub row_mt: Option<bool>,
    pub extra_args: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Config {
        toml::from_str(text).expect("test config parses")
    }

    #[test]
    fn fps_takes_a_number_or_a_ratio() {
        let fps = |text: &str| {
            parse(&format!("[[target]]\nsrc = \"a.mp4\"\nfps = {text}\n")).targets[0]
                .fps
                .expect("fps parses")
        };
        assert_eq!(fps("25"), Fps::Decimal(25.0));
        assert_eq!(fps("23.976"), Fps::Decimal(23.976));
        assert_eq!(
            fps("\"30000/1001\""),
            Fps::Ratio {
                num: 30000,
                den: 1001
            }
        );
    }

    #[test]
    fn fps_rejects_a_rate_that_is_not_a_number_or_a_ratio() {
        for text in ["\"bogus\"", "0", "-25", "\"30000/0\"", "\"/1001\""] {
            let toml = format!("[[target]]\nsrc = \"a.mp4\"\nfps = {text}\n");
            assert!(
                toml::from_str::<Config>(&toml).is_err(),
                "{text} should not parse"
            );
        }
    }

    #[test]
    fn a_target_field_wins_over_the_same_field_in_defaults() {
        let config = parse(
            r#"
            [defaults]
            quality = "low"
            widths = [400]

            [[target]]
            src = "a.mp4"

            [[target]]
            src = "b.mp4"
            quality = "high"
            "#,
        );
        let merged = config.merged_targets();
        assert_eq!(merged[0].quality, Some(Quality::Low));
        assert_eq!(merged[1].quality, Some(Quality::High));
        assert_eq!(merged[1].widths, Some(vec![400]));
    }

    #[test]
    fn settings_tables_merge_field_by_field() {
        let config = parse(
            r#"
            [defaults]
            audio = { normalize = true, bitrate = "128k" }

            [[target]]
            src = "a.mp4"
            audio = { bitrate = "96k" }
            "#,
        );
        let Some(AudioField::Settings(audio)) = &config.merged_targets()[0].audio else {
            panic!("expected merged audio settings");
        };
        assert_eq!(audio.normalize, Some(true));
        assert_eq!(audio.bitrate.as_deref(), Some("96k"));
    }

    #[test]
    fn switching_a_feature_off_on_a_target_beats_settings_in_defaults() {
        let config = parse(
            r#"
            [defaults]
            subtitles = { model = "whisper-small" }

            [[target]]
            src = "a.mp4"
            subtitles = false
            "#,
        );
        assert!(matches!(
            config.merged_targets()[0].subtitles,
            Some(SubtitlesField::Off(false))
        ));
    }

    #[test]
    fn settings_on_a_target_beat_a_feature_switched_off_in_defaults() {
        let config = parse(
            r#"
            [defaults]
            poster = false

            [[target]]
            src = "a.mp4"
            poster = { at = "4" }
            "#,
        );
        let Some(PosterField::Settings(poster)) = &config.merged_targets()[0].poster else {
            panic!("expected the target's poster settings to survive");
        };
        assert_eq!(poster.at.as_deref(), Some("4"));
    }

    #[test]
    fn resolving_folds_away_dot_segments() {
        let dir = Path::new("videos");
        assert_eq!(
            resolve_against(dir, Path::new("../src/assets")),
            PathBuf::from("src/assets")
        );
        assert_eq!(
            resolve_against(dir, Path::new("./clip.mp4")),
            PathBuf::from("videos/clip.mp4")
        );
        assert_eq!(
            resolve_against(Path::new(""), Path::new("clip.mp4")),
            PathBuf::from("clip.mp4")
        );
    }

    #[test]
    fn a_leading_parent_segment_is_kept() {
        assert_eq!(
            resolve_against(Path::new("."), Path::new("../out")),
            PathBuf::from("../out")
        );
        assert_eq!(
            resolve_against(Path::new("videos"), Path::new("../../out")),
            PathBuf::from("../out")
        );
    }

    #[test]
    fn an_absolute_path_ignores_the_base() {
        assert_eq!(
            resolve_against(Path::new("videos"), Path::new("/tmp/out")),
            PathBuf::from("/tmp/out")
        );
    }
}
