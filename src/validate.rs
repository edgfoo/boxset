//! `validate()`: everything wrong with the config and sources, reported at once.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use crate::config::{
    Anchor, AudioField, Codec, Crop, PosterField, SubtitlesField, TOP_LEVEL_FIELDS, TargetConfig,
};
use crate::outputs::{self, Naming};
use crate::problem::{Problem, ProblemKind, Severity};
use crate::sources::{ProbeErrorKind, SourceLookup, SourceState};

/// Fields a `[[target]]` entry may set. Used both for the top-level-key check
/// (a target field written at the top level) and for suggesting a spelling
/// when a target has an unknown field.
const TARGET_FIELDS: &[&str] = &[
    "src",
    "name",
    "quality",
    "codecs",
    "crop",
    "widths",
    "trim",
    "fps",
    "audio",
    "poster",
    "subtitles",
    "h264",
    "h265",
    "vp9",
    "av1",
];

/// Never short-circuits: one bad field must not hide the rest.
///
/// `configs` are the targets with defaults already applied. `top_level` is the
/// config file's leftover keys.
pub fn validate(
    configs: &[TargetConfig],
    defaults: &TargetConfig,
    top_level: &BTreeMap<String, toml::Value>,
    out_dir: &Path,
    sources: &impl SourceLookup,
) -> Vec<Problem> {
    let mut problems = Vec::new();

    check_top_level_fields(top_level, &mut problems);
    check_defaults(defaults, &mut problems);

    for (index, config) in configs.iter().enumerate() {
        check_src(index, config, sources, &mut problems);
        check_codec_overrides(index, config, &mut problems);
        check_audio_on_silent_source(index, config, sources, &mut problems);
        check_malformed_values(index, config, &mut problems);
        check_widths_against_source(index, config, sources, &mut problems);
        check_unknown_fields(index, config, &mut problems);
    }

    check_output_collisions(configs, out_dir, sources, &mut problems);

    problems
}

fn check_top_level_fields(top_level: &BTreeMap<String, toml::Value>, problems: &mut Vec<Problem>) {
    for name in top_level.keys() {
        let kind = if TARGET_FIELDS.contains(&name.as_str()) {
            ProblemKind::TargetFieldAtTopLevel { name: name.clone() }
        } else {
            ProblemKind::UnknownField {
                name: name.clone(),
                suggestion: closest_match(name, TOP_LEVEL_FIELDS),
            }
        };
        problems.push(Problem {
            severity: Severity::Warning,
            kind,
            target: None,
            field: None,
        });
    }
}

/// `src` and `name` identify one target, so sharing them across all of them is
/// always a mistake.
fn check_defaults(defaults: &TargetConfig, problems: &mut Vec<Problem>) {
    let named: [(&'static str, bool); 2] = [
        ("src", defaults.src.is_some()),
        ("name", defaults.name.is_some()),
    ];

    for (name, is_set) in named {
        if is_set {
            problems.push(Problem {
                severity: Severity::Error,
                kind: ProblemKind::FieldNotAllowedInDefaults { name },
                target: None,
                field: Some(name),
            });
        }
    }

    for name in defaults.unknown.keys() {
        problems.push(Problem {
            severity: Severity::Warning,
            kind: ProblemKind::UnknownField {
                name: name.clone(),
                suggestion: closest_match(name, TARGET_FIELDS),
            },
            target: None,
            field: None,
        });
    }
}

fn check_src(
    index: usize,
    config: &TargetConfig,
    sources: &impl SourceLookup,
    problems: &mut Vec<Problem>,
) {
    let Some(src) = &config.src else {
        problems.push(Problem {
            severity: Severity::Error,
            kind: ProblemKind::SrcMissing,
            target: Some(index),
            field: Some("src"),
        });
        return;
    };

    let reason = match sources.get(src) {
        Some(SourceState::Probed(_)) => return,
        Some(SourceState::Failed(kind)) => *kind,
        _ => ProbeErrorKind::NotFound,
    };
    problems.push(Problem {
        severity: Severity::Error,
        kind: ProblemKind::SrcUnprobeable {
            path: src.clone(),
            reason,
        },
        target: Some(index),
        field: Some("src"),
    });
}

fn check_codec_overrides(index: usize, config: &TargetConfig, problems: &mut Vec<Problem>) {
    let codecs = config
        .codecs
        .clone()
        .unwrap_or_else(|| vec![Codec::H264, Codec::Vp9]);

    let overrides: [(Codec, bool, &'static str); 4] = [
        (Codec::H264, config.h264.is_some(), "h264"),
        (Codec::H265, config.h265.is_some(), "h265"),
        (Codec::Vp9, config.vp9.is_some(), "vp9"),
        (Codec::Av1, config.av1.is_some(), "av1"),
    ];

    for (codec, is_set, field) in overrides {
        if is_set && !codecs.contains(&codec) {
            problems.push(Problem {
                severity: Severity::Error,
                kind: ProblemKind::CodecOverrideForExcludedCodec,
                target: Some(index),
                field: Some(field),
            });
        }
    }
}

fn check_audio_on_silent_source(
    index: usize,
    config: &TargetConfig,
    sources: &impl SourceLookup,
    problems: &mut Vec<Problem>,
) {
    let Some(AudioField::Settings(_)) = &config.audio else {
        return;
    };
    let Some(src) = &config.src else {
        return;
    };
    if let Some(SourceState::Probed(probe)) = sources.get(src)
        && !probe.has_audio
    {
        problems.push(Problem {
            severity: Severity::Warning,
            kind: ProblemKind::AudioSettingOnSilentSource,
            target: Some(index),
            field: Some("audio"),
        });
    }
}

fn check_malformed_values(index: usize, config: &TargetConfig, problems: &mut Vec<Problem>) {
    let mut malformed = |value: &str, expected, field| {
        problems.push(Problem {
            severity: Severity::Error,
            kind: ProblemKind::MalformedValue {
                value: value.to_string(),
                expected,
            },
            target: Some(index),
            field: Some(field),
        });
    };

    if let Some(crop) = &config.crop {
        let raw = match crop {
            Crop::Bare(ratio) => ratio,
            Crop::Anchored { ratio, .. } => ratio,
        };
        if crate::resolve::ratio(raw).is_none() {
            malformed(raw, "an aspect ratio like 16:9", "crop");
        }
    }

    if let Some(trim) = &config.trim {
        for raw in [trim.start.as_deref(), trim.end.as_deref()]
            .into_iter()
            .flatten()
        {
            if crate::resolve::timestamp(raw).is_none() {
                malformed(raw, "a timestamp like 00:00:04 or 4.5", "trim");
            }
        }
    }

    if let Some(PosterField::Settings(poster)) = &config.poster
        && let Some(raw) = poster.at.as_deref()
        && crate::resolve::timestamp(raw).is_none()
    {
        malformed(raw, "a timestamp like 00:00:04 or 4.5", "poster");
    }
}

fn check_widths_against_source(
    index: usize,
    config: &TargetConfig,
    sources: &impl SourceLookup,
    problems: &mut Vec<Problem>,
) {
    let Some(widths) = &config.widths else {
        return;
    };
    let Some(src) = &config.src else {
        return;
    };
    let Some(SourceState::Probed(probe)) = sources.get(src) else {
        return;
    };

    let crop = config.crop.as_ref().and_then(try_resolve_crop);
    let available = crate::command::cropped_size(crop, probe).0;

    let too_wide: Vec<u32> = widths.iter().copied().filter(|&w| w > available).collect();
    if !too_wide.is_empty() {
        problems.push(Problem {
            severity: Severity::Warning,
            kind: ProblemKind::WidthsExceedSource {
                widths: too_wide,
                available,
            },
            target: Some(index),
            field: Some("widths"),
        });
    }
}

fn check_unknown_fields(index: usize, config: &TargetConfig, problems: &mut Vec<Problem>) {
    for name in config.unknown.keys() {
        let suggestion = closest_match(name, TARGET_FIELDS);
        problems.push(Problem {
            severity: Severity::Warning,
            kind: ProblemKind::UnknownField {
                name: name.clone(),
                suggestion,
            },
            target: Some(index),
            field: None,
        });
    }
}

/// Every path a target claims: a rendition per width per codec, a poster per
/// width, and one subtitle track.
fn output_paths(
    config: &TargetConfig,
    out_dir: &Path,
    sources: &impl SourceLookup,
) -> Option<Vec<PathBuf>> {
    let src = config.src.as_ref()?;
    let Some(SourceState::Probed(probe)) = sources.get(src) else {
        return None;
    };

    let crop = config.crop.as_ref().and_then(try_resolve_crop);
    let post_crop_width = crate::command::cropped_size(crop, probe).0;
    let widths = config
        .widths
        .clone()
        .unwrap_or_else(|| crate::settings::derive_ladder(post_crop_width));
    let codecs = config
        .codecs
        .clone()
        .unwrap_or_else(|| vec![Codec::H264, Codec::Vp9]);

    let naming = Naming {
        out_dir,
        src,
        name: config.name.as_deref(),
    };

    let mut paths = Vec::new();
    for width in widths {
        for &codec in &codecs {
            paths.push(outputs::rendition_path(&naming, &codecs, width, codec));
        }
        if !matches!(config.poster, Some(PosterField::Off(false))) {
            paths.push(outputs::poster_path(&naming, width));
        }
    }
    if !matches!(config.subtitles, Some(SubtitlesField::Off(false))) {
        paths.push(outputs::subtitles_path(&naming));
    }
    Some(paths)
}

/// `None` for a malformed ratio, which is reported separately; here it just
/// means the ladder can't be narrowed, so the uncropped width stands.
fn try_resolve_crop(crop: &Crop) -> Option<crate::settings::Crop> {
    let (raw, anchor) = match crop {
        Crop::Bare(ratio) => (ratio.as_str(), Anchor::Centre),
        Crop::Anchored { ratio, anchor } => (ratio.as_str(), *anchor),
    };
    Some(crate::settings::Crop {
        ratio: crate::resolve::ratio(raw)?,
        anchor,
    })
}

fn check_output_collisions(
    configs: &[TargetConfig],
    out_dir: &Path,
    sources: &impl SourceLookup,
    problems: &mut Vec<Problem>,
) {
    let mut seen: HashMap<PathBuf, usize> = HashMap::new();

    for (index, config) in configs.iter().enumerate() {
        let Some(paths) = output_paths(config, out_dir, sources) else {
            continue;
        };

        for path in paths {
            if let Some(&other) = seen.get(&path) {
                problems.push(Problem {
                    severity: Severity::Error,
                    kind: ProblemKind::OutputCollision { other, path },
                    target: Some(index),
                    field: Some("name"),
                });
            } else {
                seen.insert(path, index);
            }
        }
    }
}

fn closest_match(name: &str, candidates: &[&'static str]) -> Option<String> {
    candidates
        .iter()
        .map(|candidate| (candidate, levenshtein(name, candidate)))
        .filter(|(_, distance)| *distance <= 2)
        .min_by_key(|(_, distance)| *distance)
        .map(|(candidate, _)| candidate.to_string())
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();

    for (i, &ca) in a.iter().enumerate() {
        let mut curr = vec![i + 1];
        for (j, &cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr.push((prev[j] + cost).min(prev[j + 1] + 1).min(curr[j] + 1));
        }
        prev = curr;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::Probe;

    fn config(src: &str) -> TargetConfig {
        TargetConfig {
            src: Some(PathBuf::from(src)),
            ..Default::default()
        }
    }

    fn out_dir() -> &'static Path {
        Path::new("assets/video")
    }

    fn sources_with(path: &str, mut probe: Probe) -> HashMap<PathBuf, SourceState> {
        probe.src = PathBuf::from(path);
        HashMap::from([(PathBuf::from(path), SourceState::Probed(probe))])
    }

    fn probe(width: u32, height: u32, has_audio: bool) -> Probe {
        Probe {
            src: PathBuf::new(),
            width,
            height,
            duration_secs: 10.0,
            frame_rate: (30, 1),
            has_audio,
        }
    }

    #[test]
    fn missing_src_is_an_error() {
        let problems = validate(
            &[TargetConfig::default()],
            &TargetConfig::default(),
            &BTreeMap::new(),
            out_dir(),
            &HashMap::new(),
        );
        assert!(matches!(
            problems.as_slice(),
            [Problem {
                kind: ProblemKind::SrcMissing,
                severity: Severity::Error,
                target: Some(0),
                ..
            }]
        ));
    }

    #[test]
    fn unprobed_src_is_an_error() {
        let configs = [config("missing.mp4")];
        let problems = validate(
            &configs,
            &TargetConfig::default(),
            &BTreeMap::new(),
            out_dir(),
            &HashMap::new(),
        );
        assert!(matches!(
            problems.as_slice(),
            [Problem {
                kind: ProblemKind::SrcUnprobeable { .. },
                ..
            }]
        ));
    }

    #[test]
    fn two_targets_same_stem_no_name_collide() {
        let configs = [config("video.mp4"), config("video.mp4")];
        let sources = sources_with("video.mp4", probe(1920, 1080, true));
        let problems = validate(
            &configs,
            &TargetConfig::default(),
            &BTreeMap::new(),
            out_dir(),
            &sources,
        );
        assert!(
            problems
                .iter()
                .any(|p| matches!(p.kind, ProblemKind::OutputCollision { .. }))
        );
    }

    #[test]
    fn two_targets_same_source_different_names_do_not_collide() {
        let mut wide = config("video.mp4");
        wide.name = Some("wide".to_string());
        let mut tall = config("video.mp4");
        tall.name = Some("tall".to_string());
        let sources = sources_with("video.mp4", probe(1920, 1080, true));
        let problems = validate(
            &[wide, tall],
            &TargetConfig::default(),
            &BTreeMap::new(),
            out_dir(),
            &sources,
        );
        assert!(
            !problems
                .iter()
                .any(|p| matches!(p.kind, ProblemKind::OutputCollision { .. }))
        );
    }

    #[test]
    fn codec_override_for_excluded_codec_is_an_error() {
        let mut cfg = config("video.mp4");
        cfg.codecs = Some(vec![Codec::H264]);
        cfg.vp9 = Some(Default::default());
        let problems = validate(
            &[cfg],
            &TargetConfig::default(),
            &BTreeMap::new(),
            out_dir(),
            &HashMap::new(),
        );
        assert!(problems.iter().any(|p| matches!(
            p.kind,
            ProblemKind::CodecOverrideForExcludedCodec
        ) && p.field == Some("vp9")));
    }

    #[test]
    fn explicit_audio_on_silent_source_warns() {
        let mut cfg = config("video.mp4");
        cfg.audio = Some(AudioField::Settings(Default::default()));
        let sources = sources_with("video.mp4", probe(640, 480, false));
        let problems = validate(
            &[cfg],
            &TargetConfig::default(),
            &BTreeMap::new(),
            out_dir(),
            &sources,
        );
        assert!(problems.iter().any(|p| matches!(
            p,
            Problem {
                kind: ProblemKind::AudioSettingOnSilentSource,
                severity: Severity::Warning,
                ..
            }
        )));
    }

    #[test]
    fn src_and_name_in_defaults_are_errors() {
        let defaults = TargetConfig {
            src: Some(PathBuf::from("video.mp4")),
            name: Some("everything".to_string()),
            ..Default::default()
        };
        let problems = validate(&[], &defaults, &BTreeMap::new(), out_dir(), &HashMap::new());
        let fields: Vec<_> = problems
            .iter()
            .filter(|p| {
                matches!(p.kind, ProblemKind::FieldNotAllowedInDefaults { .. })
                    && p.severity == Severity::Error
            })
            .map(|p| p.field)
            .collect();
        assert_eq!(fields, [Some("src"), Some("name")]);
    }

    /// One problem per target, not one per rung.
    #[test]
    fn widths_wider_than_the_source_warn_once() {
        let mut cfg = config("video.mp4");
        cfg.widths = Some(vec![480, 1280, 1920]);
        let sources = sources_with("video.mp4", probe(640, 480, true));
        let problems = validate(
            &[cfg],
            &TargetConfig::default(),
            &BTreeMap::new(),
            out_dir(),
            &sources,
        );
        let upscales: Vec<_> = problems
            .iter()
            .filter_map(|p| match &p.kind {
                ProblemKind::WidthsExceedSource { widths, available } => Some((widths, available)),
                _ => None,
            })
            .collect();
        assert_eq!(upscales.len(), 1);
        assert_eq!(upscales[0].0, &vec![1280, 1920]);
        assert_eq!(*upscales[0].1, 640);
    }

    #[test]
    fn unknown_field_suggests_a_close_spelling() {
        let mut cfg = config("video.mp4");
        cfg.unknown
            .insert("qualiy".to_string(), toml::Value::String("high".into()));
        let problems = validate(
            &[cfg],
            &TargetConfig::default(),
            &BTreeMap::new(),
            out_dir(),
            &HashMap::new(),
        );
        let unknown = problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::UnknownField { .. }))
            .unwrap();
        match &unknown.kind {
            ProblemKind::UnknownField { suggestion, .. } => {
                assert_eq!(suggestion.as_deref(), Some("quality"));
            }
            _ => unreachable!(),
        }
    }

    /// Resolution panics on these rather than returning a Result, so the
    /// report has to catch them first.
    #[test]
    fn malformed_values_are_errors_against_their_field() {
        let mut cfg = config("video.mp4");
        cfg.crop = Some(Crop::Bare("16x9".to_string()));
        cfg.trim = Some(crate::config::TimeRange {
            start: Some("0:05".to_string()),
            end: Some("half past".to_string()),
        });
        cfg.poster = Some(PosterField::Settings(crate::config::PosterSettings {
            at: Some("later".to_string()),
        }));

        let problems = validate(
            &[cfg],
            &TargetConfig::default(),
            &BTreeMap::new(),
            out_dir(),
            &HashMap::new(),
        );
        let malformed: Vec<_> = problems
            .iter()
            .filter(|p| matches!(p.kind, ProblemKind::MalformedValue { .. }))
            .collect();
        assert_eq!(
            malformed.iter().map(|p| p.field).collect::<Vec<_>>(),
            vec![Some("crop"), Some("trim"), Some("poster")]
        );
        assert!(malformed.iter().all(|p| p.severity == Severity::Error));
    }

    fn top_level(key: &str) -> BTreeMap<String, toml::Value> {
        BTreeMap::from([(key.to_string(), toml::Value::String("high".into()))])
    }

    /// A target field at the top level applies to nothing, so it is named as
    /// that rather than as an unknown key.
    #[test]
    fn target_field_at_top_level_is_named_as_such() {
        let problems = validate(
            &[],
            &TargetConfig::default(),
            &top_level("quality"),
            out_dir(),
            &HashMap::new(),
        );
        assert!(matches!(
            problems.as_slice(),
            [Problem {
                kind: ProblemKind::TargetFieldAtTopLevel { .. },
                severity: Severity::Warning,
                target: None,
                ..
            }]
        ));
    }

    #[test]
    fn unknown_top_level_key_suggests_a_top_level_spelling() {
        let problems = validate(
            &[],
            &TargetConfig::default(),
            &top_level("out_dr"),
            out_dir(),
            &HashMap::new(),
        );
        let [
            Problem {
                kind: ProblemKind::UnknownField { suggestion, .. },
                ..
            },
        ] = problems.as_slice()
        else {
            panic!("expected one unknown-field problem, got {problems:?}");
        };
        assert_eq!(suggestion.as_deref(), Some("out_dir"));
    }

    /// The project-wide keys deserialise into their own fields, so they never
    /// reach the leftover map the check reads.
    #[test]
    fn project_wide_keys_are_not_reported() {
        let config: crate::config::Config =
            toml::from_str("out_dir = \"assets/video\"\njobs = 4\n").unwrap();
        assert!(
            validate(
                &[],
                &TargetConfig::default(),
                &config.unknown,
                out_dir(),
                &HashMap::new()
            )
            .is_empty()
        );
    }
}
