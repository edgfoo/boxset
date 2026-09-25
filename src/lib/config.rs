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

    /// Leftover top-level keys, for the "target field written at top level" check.
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

/// Purely textual, so a `..` that would step out of a symlinked directory is
/// left alone.
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
    /// Re-bases every path the file contains onto `dir`, the directory the
    /// config was read from, so a config describes the same build wherever
    /// boxset is run from.
    pub fn rebase(&mut self, dir: &Path) {
        self.out_dir = resolve_against(dir, &self.out_dir);
        for target in &mut self.targets {
            if let Some(src) = &target.src {
                target.src = Some(resolve_against(dir, src));
            }
        }
    }

    /// Every target with `[defaults]` filled in. The rest of the pipeline
    /// runs on these, not on `targets`.
    pub fn merged_targets(&self) -> Vec<TargetConfig> {
        self.targets
            .iter()
            .map(|target| target.with_defaults(&self.defaults))
            .collect()
    }
}

/// Keys that belong at the top level, for suggesting a spelling.
pub const TOP_LEVEL_FIELDS: &[&str] = &["out_dir", "jobs", "target", "defaults"];

/// One `[[target]]` entry, or the flags of a single-shot run. Every field is
/// optional: what the user actually specified, nothing derived.
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

    /// Leftover keys on a target: unknown fields, and the top-level-key-on-a-target check.
    #[serde(flatten)]
    pub unknown: BTreeMap<String, toml::Value>,
}

impl TargetConfig {
    /// Anything this target sets wins; anything it leaves out comes from
    /// `defaults`. Lists replace rather than concatenate.
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
            audio: merge_audio(&self.audio, &defaults.audio),
            poster: merge_poster(&self.poster, &defaults.poster),
            subtitles: merge_subtitles(&self.subtitles, &defaults.subtitles),
            h264: merge_overrides(&self.h264, &defaults.h264),
            h265: merge_overrides(&self.h265, &defaults.h265),
            vp9: merge_overrides(&self.vp9, &defaults.vp9),
            av1: merge_overrides(&self.av1, &defaults.av1),
            unknown: self.unknown.clone(),
        }
    }
}

fn or_clone<T: Clone>(target: &Option<T>, defaults: &Option<T>) -> Option<T> {
    target.clone().or_else(|| defaults.clone())
}

/// Two tables merge field by field. A `false` on either side switches the
/// feature off or on outright, so it never merges with a table.
fn merge_audio(target: &Option<AudioField>, defaults: &Option<AudioField>) -> Option<AudioField> {
    match (target, defaults) {
        (Some(AudioField::Settings(target)), Some(AudioField::Settings(defaults))) => {
            Some(AudioField::Settings(AudioSettings {
                normalize: target.normalize.or(defaults.normalize),
                bitrate: or_clone(&target.bitrate, &defaults.bitrate),
            }))
        }
        (Some(field), _) => Some(field.clone()),
        (None, defaults) => defaults.clone(),
    }
}

fn merge_poster(
    target: &Option<PosterField>,
    defaults: &Option<PosterField>,
) -> Option<PosterField> {
    match (target, defaults) {
        (Some(PosterField::Settings(target)), Some(PosterField::Settings(defaults))) => {
            Some(PosterField::Settings(PosterSettings {
                at: or_clone(&target.at, &defaults.at),
            }))
        }
        (Some(field), _) => Some(field.clone()),
        (None, defaults) => defaults.clone(),
    }
}

fn merge_subtitles(
    target: &Option<SubtitlesField>,
    defaults: &Option<SubtitlesField>,
) -> Option<SubtitlesField> {
    match (target, defaults) {
        (Some(SubtitlesField::Settings(target)), Some(SubtitlesField::Settings(defaults))) => {
            Some(SubtitlesField::Settings(SubtitleSettings {
                language: or_clone(&target.language, &defaults.language),
                model: target.model.or(defaults.model),
            }))
        }
        (Some(field), _) => Some(field.clone()),
        (None, defaults) => defaults.clone(),
    }
}

fn merge_overrides(
    target: &Option<CodecOverrides>,
    defaults: &Option<CodecOverrides>,
) -> Option<CodecOverrides> {
    let (Some(target), Some(defaults)) = (target, defaults) else {
        return or_clone(target, defaults);
    };
    Some(CodecOverrides {
        crf: target.crf.or(defaults.crf),
        preset: or_clone(&target.preset, &defaults.preset),
        profile: or_clone(&target.profile, &defaults.profile),
        cpu_used: target.cpu_used.or(defaults.cpu_used),
        row_mt: target.row_mt.or(defaults.row_mt),
        extra_args: or_clone(&target.extra_args, &defaults.extra_args),
    })
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Anchor {
    Centre,
    Top,
    Bottom,
    Left,
    Right,
}

impl Anchor {
    /// Top-left corner of a `w`×`h` window in a `sw`×`sh` source. An anchor
    /// only bites on the axis the crop shrinks; the other centres.
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
    Anchored { ratio: String, anchor: Anchor },
}

#[derive(Debug, Clone, Deserialize)]
pub struct TimeRange {
    pub start: Option<String>,
    pub end: Option<String>,
}

/// A rational frame rate, so 23.976 / 59.94 survive resolution intact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Fps {
    pub num: u32,
    pub den: u32,
}

/// `false` to switch off, or a table to configure.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum AudioField {
    Off(bool),
    Settings(AudioSettings),
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AudioSettings {
    pub normalize: Option<bool>,
    pub bitrate: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum PosterField {
    Off(bool),
    Settings(PosterSettings),
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PosterSettings {
    pub at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum SubtitlesField {
    Off(bool),
    Settings(SubtitleSettings),
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SubtitleSettings {
    pub language: Option<String>,
    pub model: Option<WhisperModel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WhisperModel {
    Tiny,
    Base,
    Small,
    Medium,
    Large,
}

/// Per-codec overrides: an inline table, since `[target.h264]` in a TOML array
/// of tables binds to the last element.
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
            subtitles = { model = "small" }

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

    /// A `..` with nothing to cancel has to survive, or a path pointing above
    /// the config's directory would silently become one inside it.
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
