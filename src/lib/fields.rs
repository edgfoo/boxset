//! The field table: one entry per `boxset.toml` key, and the single source for
//! the JSON schema, `--help`, and the unknown-key check in `validate`.

use std::sync::LazyLock;

/// The different "shapes" a config value may be
#[derive(Debug, Clone, Copy)]
pub enum Shape {
    Path,
    Text,
    Choice(&'static [&'static str]),
    ChoiceList(&'static [&'static str]),
    Integer {
        min: u32,
        max: u32,
    },
    IntegerList {
        min: u32,
        max: u32,
    },
    Boolean,
    /// A string matched against a regex, described in plain words by `hint`
    Pattern {
        regex: &'static str,
        hint: &'static str,
    },
    /// A positive number, or a `num/den` string for a rate that is not an
    /// exact decimal
    NumberOrRatio,
    Table(&'static [Field]),
    /// Like `Table`, but can also be `false`
    Toggle(&'static [Field]),
    /// A table of per-codec ffmpeg overrides
    CodecTable(&'static [Field]),
    TextList,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpGroup {
    General,
    Video,
    Poster,
    Subtitles,
    Encoder,
}

#[derive(Debug)]
pub struct Field {
    pub toml_key: &'static str,
    /// The CLI flag, where one exists.
    pub flag: Option<&'static str>,
    /// Empty for a flag that takes no value.
    pub value_name: &'static str,
    pub shape: Shape,
    pub help_note: &'static str,
    pub schema_doc: &'static str,
    pub default_note: &'static str,
    pub group: HelpGroup,
}

impl Field {
    const fn new(toml_key: &'static str, shape: Shape, schema_doc: &'static str) -> Field {
        Field {
            toml_key,
            flag: None,
            value_name: "",
            shape,
            help_note: "",
            schema_doc,
            default_note: "",
            group: HelpGroup::Video,
        }
    }
}

pub const QUALITY_TIERS: &[&str] = &["low", "balanced", "high", "max"];
pub const CODEC_NAMES: &[&str] = &["h264", "h265", "vp9", "av1"];
pub const ANCHOR_NAMES: &[&str] = &["centre", "top", "bottom", "left", "right"];

const RATIO: Shape = Shape::Pattern {
    regex: r"^\d+:\d+$",
    hint: "an aspect ratio like 16:9",
};

/// `4`, `4.5`, `0:04` or `00:00:04`, matching `resolve::parse_timestamp`.
const TIMESTAMP: Shape = Shape::Pattern {
    regex: r"^(\d+(\.\d+)?|(\d+:)?\d{1,2}:\d{2}(\.\d+)?)$",
    hint: "a timestamp like 00:00:04 or 4.5",
};

/// Videos below this size are rejected
const MIN_WIDTH: u32 = 16;
/// Videos above this size are rejected
const MAX_WIDTH: u32 = 16384;

const CRF_DOC: &str = "Constant rate factor. Lower is better quality and a larger file.";
const EFFORT_DOC: &str = "Encoder effort. Slower settings compress better.";
const EXTRA_ARGS_DOC: &str = "Extra ffmpeg arguments, passed through untouched.";

const X264_PRESETS: &[&str] = &[
    "ultrafast",
    "superfast",
    "veryfast",
    "faster",
    "fast",
    "medium",
    "slow",
    "slower",
    "veryslow",
];

const AUDIO_FIELDS: &[Field] = &[
    Field {
        default_note: "true",
        ..Field::new(
            "normalize",
            Shape::Boolean,
            "Even out loudness across the track.",
        )
    },
    Field {
        default_note: "128k",
        ..Field::new(
            "bitrate",
            Shape::Pattern {
                regex: r"^\d+k$",
                hint: "a bitrate like 128k",
            },
            "Audio bitrate, as ffmpeg spells it.",
        )
    },
];

const POSTER_FIELDS: &[Field] = &[Field {
    flag: Some("--poster"),
    value_name: "[<at>]",
    help_note: "timestamp of poster, eg. 0:04",
    default_note: "first frame",
    group: HelpGroup::Poster,
    ..Field::new("at", TIMESTAMP, "Timestamp of the frame to capture.")
}];

const SUBTITLE_FIELDS: &[Field] = &[Field {
    flag: Some("--subs-model"),
    value_name: "<model>",
    help_note: "which transcription model to run",
    default_note: "parakeet-0.6b",
    group: HelpGroup::Subtitles,
    ..Field::new(
        "model",
        Shape::Choice(MODEL_NAMES),
        "Which transcription model to run.",
    )
}];

const TRIM_FIELDS: &[Field] = &[
    Field {
        default_note: "the start of the source",
        ..Field::new("start", TIMESTAMP, "Where the output begins.")
    },
    Field {
        default_note: "the end of the source",
        ..Field::new("end", TIMESTAMP, "Where the output ends.")
    },
];

const CROP_FIELDS: &[Field] = &[
    Field {
        flag: Some("--crop"),
        value_name: "<w:h>",
        help_note: "crop outputs to aspect ratio",
        default_note: "none",
        ..Field::new("ratio", RATIO, "The aspect ratio to crop to.")
    },
    Field {
        flag: Some("--crop-anchor"),
        value_name: "<where>",
        help_note: "centre, top, bottom, left, right",
        default_note: "centre",
        ..Field::new(
            "anchor",
            Shape::Choice(ANCHOR_NAMES),
            "Which edge the crop window keeps.",
        )
    },
];

/// Each codec table lists only the keys its encoder understands,
/// eg. `cpu_used` is valid for VP9, not for h264.
const H264_FIELDS: &[Field] = &[
    Field {
        default_note: "set by quality",
        ..Field::new("crf", Shape::Integer { min: 0, max: 51 }, CRF_DOC)
    },
    Field {
        default_note: "veryslow",
        ..Field::new("preset", Shape::Choice(X264_PRESETS), EFFORT_DOC)
    },
    Field {
        default_note: "the encoder's choice",
        ..Field::new(
            "profile",
            Shape::Choice(&["baseline", "main", "high"]),
            "H.264 profile, for older device support.",
        )
    },
    Field {
        default_note: "none",
        ..Field::new("extra_args", Shape::TextList, EXTRA_ARGS_DOC)
    },
];

const H265_FIELDS: &[Field] = &[
    Field {
        default_note: "set by quality",
        ..Field::new("crf", Shape::Integer { min: 0, max: 51 }, CRF_DOC)
    },
    Field {
        default_note: "veryslow",
        ..Field::new("preset", Shape::Choice(X264_PRESETS), EFFORT_DOC)
    },
    Field {
        default_note: "the encoder's choice",
        ..Field::new(
            "profile",
            Shape::Choice(&["main", "main10"]),
            "H.265 profile.",
        )
    },
    Field {
        default_note: "none",
        ..Field::new("extra_args", Shape::TextList, EXTRA_ARGS_DOC)
    },
];

const VP9_FIELDS: &[Field] = &[
    Field {
        default_note: "set by quality",
        ..Field::new("crf", Shape::Integer { min: 0, max: 63 }, CRF_DOC)
    },
    Field {
        default_note: "0",
        ..Field::new(
            "cpu_used",
            Shape::Integer { min: 0, max: 8 },
            "Encoder effort, 0 slowest. Above 3 encodes worse at every CRF, not merely faster.",
        )
    },
    Field {
        default_note: "true",
        ..Field::new("row_mt", Shape::Boolean, "Row-based multithreading.")
    },
    Field {
        default_note: "none",
        ..Field::new("extra_args", Shape::TextList, EXTRA_ARGS_DOC)
    },
];

const AV1_FIELDS: &[Field] = &[
    Field {
        default_note: "set by quality",
        ..Field::new("crf", Shape::Integer { min: 0, max: 63 }, CRF_DOC)
    },
    Field {
        default_note: "4",
        ..Field::new(
            "preset",
            Shape::Integer { min: 0, max: 13 },
            "SVT-AV1 preset, 0 slowest.",
        )
    },
    Field {
        default_note: "none",
        ..Field::new("extra_args", Shape::TextList, EXTRA_ARGS_DOC)
    },
];

/// Every `[[target]]` key, in the order a generated config and the help
/// table present them.
pub const TARGET_FIELDS: &[Field] = &[
    Field {
        group: HelpGroup::General,
        ..Field::new(
            "src",
            Shape::Path,
            "The video to prepare, relative to this config.",
        )
    },
    Field {
        flag: Some("--name"),
        value_name: "<name>",
        help_note: "base name for outputs",
        default_note: "source filename",
        group: HelpGroup::General,
        ..Field::new("name", Shape::Text, "Base name for this target's outputs.")
    },
    Field {
        flag: Some("--quality"),
        value_name: "<tier>",
        help_note: "video quality: low, balanced, high, max",
        default_note: "balanced",
        ..Field::new(
            "quality",
            Shape::Choice(QUALITY_TIERS),
            "How hard to compress, trading file size against picture quality.",
        )
    },
    Field {
        flag: Some("--codecs"),
        value_name: "<list>",
        help_note: "transcode to: h264, h265, vp9, av1",
        default_note: "h264,vp9",
        ..Field::new(
            "codecs",
            Shape::ChoiceList(CODEC_NAMES),
            "Which formats to encode.",
        )
    },
    Field {
        help_note: "crop outputs to aspect ratio",
        default_note: "none",
        ..Field::new(
            "crop",
            Shape::Table(CROP_FIELDS),
            "Crop to an aspect ratio. A bare string is centred.",
        )
    },
    Field {
        flag: Some("--widths"),
        value_name: "<list>",
        help_note: "output width(s) in pixels",
        default_note: "derived from source",
        ..Field::new(
            "widths",
            Shape::IntegerList {
                min: MIN_WIDTH,
                max: MAX_WIDTH,
            },
            "Output widths in pixels.",
        )
    },
    Field {
        flag: Some("--trim"),
        value_name: "<start-end>",
        help_note: "eg 0:05-0:30, either side omittable",
        default_note: "none",
        ..Field::new(
            "trim",
            Shape::Table(TRIM_FIELDS),
            "Cut the output to a time range.",
        )
    },
    Field {
        flag: Some("--fps"),
        value_name: "<rate>",
        help_note: "output frames per second",
        default_note: "the source's",
        ..Field::new(
            "fps",
            Shape::NumberOrRatio,
            "Output frame rate, as a number or a num/den ratio.",
        )
    },
    Field {
        flag: Some("--no-audio"),
        help_note: "drop the audio track",
        default_note: "kept",
        ..Field::new(
            "audio",
            Shape::Toggle(AUDIO_FIELDS),
            "Audio settings, or false to drop the track.",
        )
    },
    Field {
        flag: Some("--no-poster"),
        help_note: "skip the poster image",
        default_note: "captured",
        group: HelpGroup::Poster,
        ..Field::new(
            "poster",
            Shape::Toggle(POSTER_FIELDS),
            "Poster image settings, or false to skip it.",
        )
    },
    Field {
        flag: Some("--no-subs"),
        help_note: "skip subtitles",
        default_note: "generated",
        group: HelpGroup::Subtitles,
        ..Field::new(
            "subtitles",
            Shape::Toggle(SUBTITLE_FIELDS),
            "Subtitle settings, or false to skip them.",
        )
    },
    Field {
        group: HelpGroup::Encoder,
        ..Field::new(
            "h264",
            Shape::CodecTable(H264_FIELDS),
            "ffmpeg overrides for h264.",
        )
    },
    Field {
        group: HelpGroup::Encoder,
        ..Field::new(
            "h265",
            Shape::CodecTable(H265_FIELDS),
            "ffmpeg overrides for h265.",
        )
    },
    Field {
        group: HelpGroup::Encoder,
        ..Field::new(
            "vp9",
            Shape::CodecTable(VP9_FIELDS),
            "ffmpeg overrides for vp9.",
        )
    },
    Field {
        group: HelpGroup::Encoder,
        ..Field::new(
            "av1",
            Shape::CodecTable(AV1_FIELDS),
            "ffmpeg overrides for av1.",
        )
    },
];

pub const TOP_LEVEL_FIELDS: &[Field] = &[
    Field {
        flag: Some("--out-dir"),
        value_name: "<dir>",
        help_note: "output folder",
        default_note: "./export",
        group: HelpGroup::General,
        ..Field::new(
            "out_dir",
            Shape::Path,
            "Where outputs are written, relative to this config.",
        )
    },
    Field {
        flag: Some("--jobs"),
        value_name: "<n>",
        help_note: "encodes in parallel",
        default_note: "2",
        group: HelpGroup::General,
        ..Field::new(
            "jobs",
            Shape::Integer { min: 1, max: 64 },
            "How many encodes run at once.",
        )
    },
];

/// Model names, kept in step with `config::ALL_MODELS` by the test below.
const MODEL_NAMES: &[&str] = &[
    "whisper-tiny",
    "whisper-base",
    "whisper-small",
    "whisper-medium",
    "whisper-large",
    "parakeet-110m",
    "parakeet-0.6b",
];

/// Top-level keys that refer to targets rather than settings
pub const CONTAINER_KEYS: &[&str] = &["target", "defaults"];

pub static TARGET_KEYS: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| TARGET_FIELDS.iter().map(|f| f.toml_key).collect());

/// Every key `boxset.toml` accepts at the top level
pub fn top_level_keys() -> Vec<&'static str> {
    TOP_LEVEL_FIELDS
        .iter()
        .map(|f| f.toml_key)
        .chain(CONTAINER_KEYS.iter().copied())
        .collect()
}

/// A codec table's flags are its codec and key. eg. `cpu_used` on vp9 becomes `--vp9-cpu-used`.
pub fn codec_flag(codec: &str, key: &str) -> String {
    format!("--{codec}-{}", key.replace('_', "-"))
}

fn flags_of(fields: &'static [Field]) -> Vec<String> {
    let mut flags = Vec::new();
    for field in fields {
        if let Some(flag) = field.flag {
            flags.push(flag.to_string());
        }
        match field.shape {
            Shape::CodecTable(inner) => {
                flags.extend(inner.iter().map(|f| codec_flag(field.toml_key, f.toml_key)));
            }
            Shape::Table(inner) | Shape::Toggle(inner) => {
                flags.extend(inner.iter().filter_map(|f| f.flag).map(str::to_string));
            }
            _ => {}
        }
    }
    flags.sort();
    flags.dedup();
    flags
}

pub fn target_flags() -> Vec<String> {
    flags_of(TARGET_FIELDS)
}

pub fn documented_flags() -> Vec<String> {
    let mut flags = flags_of(TOP_LEVEL_FIELDS);
    flags.extend(target_flags());
    flags.sort();
    flags.dedup();
    flags
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ALL_MODELS, TargetConfig};

    #[test]
    fn the_model_list_matches_the_models_that_exist() {
        let actual: Vec<&str> = ALL_MODELS.iter().map(|m| m.name()).collect();
        assert_eq!(MODEL_NAMES, actual.as_slice());
    }

    #[test]
    fn every_target_config_field_is_in_the_table() {
        let TargetConfig {
            src: _,
            name: _,
            quality: _,
            codecs: _,
            crop: _,
            widths: _,
            trim: _,
            fps: _,
            audio: _,
            poster: _,
            subtitles: _,
            h264: _,
            h265: _,
            vp9: _,
            av1: _,
            unknown: _,
        } = TargetConfig::default();

        assert_eq!(TARGET_KEYS.len(), 15, "the table lists {:?}", *TARGET_KEYS);
    }
}
