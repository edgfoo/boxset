//! Field flags: one per `TargetConfig` field, table-shaped fields flattened
//! into one flag per sub-field. On `build` these are an error naming the
//! field, since only boxset.toml describes multiple targets.

use std::path::PathBuf;

use clap::Args;

use boxset::config::{
    Anchor, AudioField, Codec, CodecOverrides, Crop, Fps, PosterField, PosterSettings, Quality,
    SubtitleSettings, SubtitlesField, TargetConfig, TimeRange, WhisperModel,
};
use boxset::problem::Severity;

use super::style::Note;

#[derive(Args, Debug, Default, Clone)]
pub struct FieldFlags {
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub quality: Option<String>,
    #[arg(long, value_delimiter = ',')]
    pub codecs: Option<Vec<String>>,
    #[arg(long)]
    pub crop: Option<String>,
    #[arg(long = "crop-anchor")]
    pub crop_anchor: Option<String>,
    #[arg(long, value_delimiter = ',')]
    pub widths: Option<Vec<u32>>,
    #[arg(long)]
    pub trim: Option<String>,
    #[arg(long)]
    pub fps: Option<String>,
    #[arg(long = "no-audio")]
    pub no_audio: bool,
    #[arg(long = "no-poster")]
    pub no_poster: bool,
    #[arg(long = "poster", num_args = 0..=1)]
    pub poster: Option<Option<String>>,
    #[arg(long = "no-subs")]
    pub no_subs: bool,
    #[arg(long = "subs-lang")]
    pub subs_lang: Option<String>,
    #[arg(long = "subs-model")]
    pub subs_model: Option<String>,

    #[arg(long = "h264-crf")]
    pub h264_crf: Option<u32>,
    #[arg(long = "h264-preset")]
    pub h264_preset: Option<String>,
    #[arg(long = "h264-profile")]
    pub h264_profile: Option<String>,
    #[arg(long = "h264-extra-args")]
    pub h264_extra_args: Option<String>,

    #[arg(long = "h265-crf")]
    pub h265_crf: Option<u32>,
    #[arg(long = "h265-preset")]
    pub h265_preset: Option<String>,
    #[arg(long = "h265-profile")]
    pub h265_profile: Option<String>,
    #[arg(long = "h265-extra-args")]
    pub h265_extra_args: Option<String>,

    #[arg(long = "vp9-crf")]
    pub vp9_crf: Option<u32>,
    #[arg(long = "vp9-cpu-used")]
    pub vp9_cpu_used: Option<u32>,
    #[arg(long = "vp9-row-mt")]
    pub vp9_row_mt: Option<bool>,
    #[arg(long = "vp9-extra-args")]
    pub vp9_extra_args: Option<String>,

    #[arg(long = "av1-crf")]
    pub av1_crf: Option<u32>,
    #[arg(long = "av1-preset")]
    pub av1_preset: Option<String>,
    #[arg(long = "av1-extra-args")]
    pub av1_extra_args: Option<String>,

    // Run-level flags: apply to whichever targets the run covers.
    #[arg(long = "out-dir", global = true)]
    pub out_dir: Option<std::path::PathBuf>,
    #[arg(long = "dry-run", global = true)]
    pub dry_run: bool,
    #[arg(long = "verbose", global = true)]
    pub verbose: bool,
    #[arg(long = "jobs", global = true)]
    pub jobs: Option<usize>,
    /// Auto-accepts the "proceed" prompt
    #[arg(short = 'y', long = "yes", global = true)]
    pub yes: bool,
}

#[cfg(test)]
pub const RUN_FLAGS: &[&str] = &[
    "--out-dir",
    "--dry-run",
    "--verbose",
    "--jobs",
    "-y",
    "--yes",
    "--help",
    "-h",
    "-V",
    "--version",
];

#[cfg(test)]
pub const BUILD_FLAGS: &[&str] = &["--target", "--config", "-c"];

/// Every field flag, paired with whether this invocation gave it. One missing
/// is one `build` accepts and silently ignores.
fn field_flags(fields: &FieldFlags) -> [(&'static str, bool); 29] {
    [
        ("--name", fields.name.is_some()),
        ("--quality", fields.quality.is_some()),
        ("--codecs", fields.codecs.is_some()),
        ("--crop", fields.crop.is_some()),
        ("--crop-anchor", fields.crop_anchor.is_some()),
        ("--widths", fields.widths.is_some()),
        ("--trim", fields.trim.is_some()),
        ("--fps", fields.fps.is_some()),
        ("--no-audio", fields.no_audio),
        ("--no-poster", fields.no_poster),
        ("--poster", fields.poster.is_some()),
        ("--no-subs", fields.no_subs),
        ("--subs-lang", fields.subs_lang.is_some()),
        ("--subs-model", fields.subs_model.is_some()),
        ("--h264-crf", fields.h264_crf.is_some()),
        ("--h264-preset", fields.h264_preset.is_some()),
        ("--h264-profile", fields.h264_profile.is_some()),
        ("--h264-extra-args", fields.h264_extra_args.is_some()),
        ("--h265-crf", fields.h265_crf.is_some()),
        ("--h265-preset", fields.h265_preset.is_some()),
        ("--h265-profile", fields.h265_profile.is_some()),
        ("--h265-extra-args", fields.h265_extra_args.is_some()),
        ("--vp9-crf", fields.vp9_crf.is_some()),
        ("--vp9-cpu-used", fields.vp9_cpu_used.is_some()),
        ("--vp9-row-mt", fields.vp9_row_mt.is_some()),
        ("--vp9-extra-args", fields.vp9_extra_args.is_some()),
        ("--av1-crf", fields.av1_crf.is_some()),
        ("--av1-preset", fields.av1_preset.is_some()),
        ("--av1-extra-args", fields.av1_extra_args.is_some()),
    ]
}

#[cfg(test)]
pub fn field_flag_names() -> impl Iterator<Item = &'static str> {
    field_flags(&FieldFlags::default())
        .into_iter()
        .map(|(flag, _)| flag)
}

impl FieldFlags {
    /// The first field flag given, named as the user spelled it.
    pub fn first_field_flag(&self) -> Option<&'static str> {
        field_flags(self)
            .into_iter()
            .find(|(_, given)| *given)
            .map(|(flag, _)| flag)
    }

    /// Flags to a `TargetConfig`, so a single-shot run and a config entry
    /// become the same shape.
    pub fn to_target_config(&self, src: PathBuf) -> Result<TargetConfig, Note> {
        Ok(TargetConfig {
            src: Some(src),
            name: self.name.clone(),
            quality: self.quality.as_deref().map(parse_quality).transpose()?,
            codecs: self
                .codecs
                .as_ref()
                .map(|list| list.iter().map(|c| parse_codec(c)).collect())
                .transpose()?,
            crop: crop_field(self)?,
            widths: self.widths.clone(),
            trim: self.trim.as_deref().map(parse_trim).transpose()?,
            fps: self.fps.as_deref().map(parse_fps).transpose()?,
            audio: self.no_audio.then_some(AudioField::Off(false)),
            poster: poster_field(self),
            subtitles: subtitles_field(self)?,
            h264: codec_overrides(
                self.h264_crf,
                self.h264_preset.clone(),
                self.h264_profile.clone(),
                None,
                None,
                self.h264_extra_args.as_deref(),
            ),
            h265: codec_overrides(
                self.h265_crf,
                self.h265_preset.clone(),
                self.h265_profile.clone(),
                None,
                None,
                self.h265_extra_args.as_deref(),
            ),
            vp9: codec_overrides(
                self.vp9_crf,
                None,
                None,
                self.vp9_cpu_used,
                self.vp9_row_mt,
                self.vp9_extra_args.as_deref(),
            ),
            av1: codec_overrides(
                self.av1_crf,
                self.av1_preset.clone(),
                None,
                None,
                None,
                self.av1_extra_args.as_deref(),
            ),
            unknown: Default::default(),
        })
    }
}

fn unrecognised_value(given: &str, what: &str, expected: &str) -> Note {
    Note {
        severity: Severity::Error,
        locator: None,
        message: format!("`{given}` isn't {what}"),
        detail: vec![format!("Expected {expected}.")],
    }
}

fn crop_field(flags: &FieldFlags) -> Result<Option<Crop>, Note> {
    let Some(ratio) = flags.crop.clone() else {
        return Ok(None);
    };
    Ok(Some(match &flags.crop_anchor {
        Some(anchor) => Crop::Anchored {
            ratio,
            anchor: parse_anchor(anchor)?,
        },
        None => Crop::Bare(ratio),
    }))
}

fn poster_field(flags: &FieldFlags) -> Option<PosterField> {
    if flags.no_poster {
        return Some(PosterField::Off(false));
    }
    match &flags.poster {
        Some(Some(at)) => Some(PosterField::Settings(PosterSettings {
            at: Some(at.clone()),
        })),
        Some(None) => Some(PosterField::Settings(PosterSettings { at: None })),
        None => None,
    }
}

fn subtitles_field(flags: &FieldFlags) -> Result<Option<SubtitlesField>, Note> {
    if flags.no_subs {
        return Ok(Some(SubtitlesField::Off(false)));
    }
    if flags.subs_lang.is_none() && flags.subs_model.is_none() {
        return Ok(None);
    }
    Ok(Some(SubtitlesField::Settings(SubtitleSettings {
        language: flags.subs_lang.clone(),
        model: flags.subs_model.as_deref().map(parse_model).transpose()?,
    })))
}

fn codec_overrides(
    crf: Option<u32>,
    preset: Option<String>,
    profile: Option<String>,
    cpu_used: Option<u32>,
    row_mt: Option<bool>,
    extra_args: Option<&str>,
) -> Option<CodecOverrides> {
    let extra_args = extra_args.map(split_args);
    if crf.is_none()
        && preset.is_none()
        && profile.is_none()
        && cpu_used.is_none()
        && row_mt.is_none()
        && extra_args.is_none()
    {
        return None;
    }
    Some(CodecOverrides {
        crf,
        preset,
        profile,
        cpu_used,
        row_mt,
        extra_args,
    })
}

/// Whitespace-separated, which is enough for the flags people actually pass
/// and keeps the shell as the thing that handles quoting.
fn split_args(raw: &str) -> Vec<String> {
    raw.split_whitespace().map(str::to_string).collect()
}

fn parse_quality(raw: &str) -> Result<Quality, Note> {
    match raw {
        "low" => Ok(Quality::Low),
        "balanced" => Ok(Quality::Balanced),
        "high" => Ok(Quality::High),
        "max" => Ok(Quality::Max),
        other => Err(unrecognised_value(
            other,
            "a quality",
            "low, balanced, high or max",
        )),
    }
}

fn parse_codec(raw: &str) -> Result<Codec, Note> {
    match raw {
        "h264" => Ok(Codec::H264),
        "h265" => Ok(Codec::H265),
        "vp9" => Ok(Codec::Vp9),
        "av1" => Ok(Codec::Av1),
        other => Err(unrecognised_value(
            other,
            "a codec",
            "h264, h265, vp9 or av1",
        )),
    }
}

fn parse_model(raw: &str) -> Result<WhisperModel, Note> {
    match raw {
        "tiny" => Ok(WhisperModel::Tiny),
        "base" => Ok(WhisperModel::Base),
        "small" => Ok(WhisperModel::Small),
        "medium" => Ok(WhisperModel::Medium),
        "large" => Ok(WhisperModel::Large),
        other => Err(unrecognised_value(
            other,
            "a subtitle model",
            "tiny, base, small, medium or large",
        )),
    }
}

fn parse_anchor(raw: &str) -> Result<Anchor, Note> {
    match raw {
        "centre" => Ok(Anchor::Centre),
        "top" => Ok(Anchor::Top),
        "bottom" => Ok(Anchor::Bottom),
        "left" => Ok(Anchor::Left),
        "right" => Ok(Anchor::Right),
        other => Err(unrecognised_value(
            other,
            "a crop anchor",
            "centre, top, bottom, left or right",
        )),
    }
}

/// `START-END`, either side omittable: `-30` trims only the tail, `5-` only
/// the head.
fn parse_trim(raw: &str) -> Result<TimeRange, Note> {
    let Some((start, end)) = raw.split_once('-') else {
        return Err(Note {
            severity: Severity::Error,
            locator: None,
            message: format!("`{raw}` isn't a trim range"),
            detail: vec!["Expected START-END, like 0:05-0:30.".to_string()],
        });
    };
    let field = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_string());
    Ok(TimeRange {
        start: field(start),
        end: field(end),
    })
}

/// A decimal like `23.976` becomes an exact rational; ffmpeg is given the
/// ratio rather than a rounded float.
fn parse_fps(raw: &str) -> Result<Fps, Note> {
    let unrecognised = || {
        unrecognised_value(
            raw,
            "a frame rate",
            "a number like 25, 23.976 or 30000/1001",
        )
    };

    if let Some((num, den)) = raw.split_once('/') {
        return Ok(Fps {
            num: num.trim().parse().map_err(|_| unrecognised())?,
            den: den.trim().parse().map_err(|_| unrecognised())?,
        });
    }
    let value: f64 = raw.trim().parse().map_err(|_| unrecognised())?;
    // 23.976 and 29.97 are 24000/1001 and 30000/1001; recover the exact form.
    let rounded = (value * 1001.0 / 1000.0).round();
    if ((rounded * 1000.0 / 1001.0) - value).abs() < 0.001 && value.fract() != 0.0 {
        return Ok(Fps {
            num: (rounded * 1000.0) as u32,
            den: 1001,
        });
    }
    Ok(Fps {
        num: value.round() as u32,
        den: 1,
    })
}
