//! Field flags: one per `TargetConfig` field, table-shaped fields flattened
//! into one flag per sub-field. On `build` these are an error naming the
//! field, since only boxset.toml describes multiple targets.

use std::path::PathBuf;

use anyhow::bail;
use clap::Args;

use boxset::config::{
    Anchor, AudioField, Codec, CodecOverrides, Crop, Fps, PosterField, PosterSettings, Quality,
    SubtitleSettings, SubtitlesField, TargetConfig, TimeRange, WhisperModel,
};

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

impl FieldFlags {
    /// The first field flag given, named as the user spelled it. Every field
    /// flag is listed here: one missing is one `build` accepts and ignores.
    pub fn first_field_flag(&self) -> Option<&'static str> {
        let given: &[(&'static str, bool)] = &[
            ("--name", self.name.is_some()),
            ("--quality", self.quality.is_some()),
            ("--codecs", self.codecs.is_some()),
            ("--crop", self.crop.is_some()),
            ("--crop-anchor", self.crop_anchor.is_some()),
            ("--widths", self.widths.is_some()),
            ("--trim", self.trim.is_some()),
            ("--fps", self.fps.is_some()),
            ("--no-audio", self.no_audio),
            ("--no-poster", self.no_poster),
            ("--poster", self.poster.is_some()),
            ("--no-subs", self.no_subs),
            ("--subs-lang", self.subs_lang.is_some()),
            ("--subs-model", self.subs_model.is_some()),
            ("--h264-crf", self.h264_crf.is_some()),
            ("--h264-preset", self.h264_preset.is_some()),
            ("--h264-profile", self.h264_profile.is_some()),
            ("--h264-extra-args", self.h264_extra_args.is_some()),
            ("--h265-crf", self.h265_crf.is_some()),
            ("--h265-preset", self.h265_preset.is_some()),
            ("--h265-profile", self.h265_profile.is_some()),
            ("--h265-extra-args", self.h265_extra_args.is_some()),
            ("--vp9-crf", self.vp9_crf.is_some()),
            ("--vp9-cpu-used", self.vp9_cpu_used.is_some()),
            ("--vp9-row-mt", self.vp9_row_mt.is_some()),
            ("--vp9-extra-args", self.vp9_extra_args.is_some()),
            ("--av1-crf", self.av1_crf.is_some()),
            ("--av1-preset", self.av1_preset.is_some()),
            ("--av1-extra-args", self.av1_extra_args.is_some()),
        ];
        given
            .iter()
            .find(|&&(_, given)| given)
            .map(|&(flag, _)| flag)
    }

    /// Flags to a `TargetConfig`, so a single-shot run and a config entry
    /// become the same shape.
    pub fn to_target_config(&self, src: PathBuf) -> anyhow::Result<TargetConfig> {
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

fn crop_field(flags: &FieldFlags) -> anyhow::Result<Option<Crop>> {
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

fn subtitles_field(flags: &FieldFlags) -> anyhow::Result<Option<SubtitlesField>> {
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

fn parse_quality(raw: &str) -> anyhow::Result<Quality> {
    match raw {
        "low" => Ok(Quality::Low),
        "balanced" => Ok(Quality::Balanced),
        "high" => Ok(Quality::High),
        "max" => Ok(Quality::Max),
        other => bail!("unknown quality {other:?}: expected low, balanced, high or max"),
    }
}

fn parse_codec(raw: &str) -> anyhow::Result<Codec> {
    match raw {
        "h264" => Ok(Codec::H264),
        "h265" => Ok(Codec::H265),
        "vp9" => Ok(Codec::Vp9),
        "av1" => Ok(Codec::Av1),
        other => bail!("unknown codec {other:?}: expected h264, h265, vp9 or av1"),
    }
}

fn parse_model(raw: &str) -> anyhow::Result<WhisperModel> {
    match raw {
        "tiny" => Ok(WhisperModel::Tiny),
        "base" => Ok(WhisperModel::Base),
        "small" => Ok(WhisperModel::Small),
        "medium" => Ok(WhisperModel::Medium),
        "large" => Ok(WhisperModel::Large),
        other => bail!("unknown model {other:?}: expected tiny, base, small, medium or large"),
    }
}

fn parse_anchor(raw: &str) -> anyhow::Result<Anchor> {
    match raw {
        "centre" => Ok(Anchor::Centre),
        "top" => Ok(Anchor::Top),
        "bottom" => Ok(Anchor::Bottom),
        "left" => Ok(Anchor::Left),
        "right" => Ok(Anchor::Right),
        other => {
            bail!("unknown crop anchor {other:?}: expected centre, top, bottom, left or right")
        }
    }
}

/// `START-END`, either side omittable: `-30` trims only the tail, `5-` only
/// the head.
fn parse_trim(raw: &str) -> anyhow::Result<TimeRange> {
    let Some((start, end)) = raw.split_once('-') else {
        bail!("trim {raw:?} should look like START-END, e.g. 0:05-0:30");
    };
    let field = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_string());
    Ok(TimeRange {
        start: field(start),
        end: field(end),
    })
}

/// A decimal like `23.976` becomes an exact rational; ffmpeg is given the
/// ratio rather than a rounded float.
fn parse_fps(raw: &str) -> anyhow::Result<Fps> {
    if let Some((num, den)) = raw.split_once('/') {
        return Ok(Fps {
            num: num.trim().parse()?,
            den: den.trim().parse()?,
        });
    }
    let value: f64 = raw.trim().parse()?;
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
