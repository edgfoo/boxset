//! `resolve()`: config plus probe in, total `Settings` out.

use crate::config::{
    self, Anchor, AudioField, Codec, CodecOverrides, PosterField, Quality, SubtitlesField,
    TargetConfig, WhisperModel,
};
use crate::settings::{
    AudioSettings, CodecOptions, Crop, Fps, PosterSettings, Settings, SubtitleSettings, TimeRange,
    Timestamp, derive_ladder, expand_quality,
};
use crate::sources::Probe;

/// Precedence, lowest to highest: defaults in code, quality expansion,
/// specified fields — applied field by field.
pub fn resolve(config: &TargetConfig, probe: &Probe, out_dir: &std::path::Path) -> Settings {
    let quality = config.quality.unwrap_or(Quality::Balanced);
    let codecs = config
        .codecs
        .clone()
        .unwrap_or_else(|| vec![Codec::H264, Codec::Vp9]);
    let crop = config.crop.as_ref().map(resolve_crop);
    let trim = config.trim.as_ref().map(resolve_trim);

    // Filter order is trim, then crop, then scale: the ladder derives from
    // the post-crop width, not the source's.
    let post_crop_width = crate::command::cropped_size(crop, probe).0;
    let widths = config
        .widths
        .clone()
        .unwrap_or_else(|| derive_ladder(post_crop_width));

    let audio = resolve_audio(config.audio.as_ref(), probe.has_audio);
    let poster = resolve_poster(config.poster.as_ref());
    let subtitles = resolve_subtitles(config.subtitles.as_ref());

    Settings {
        src: config.src.clone().expect("validated: src is present"),
        name: config.name.clone(),
        out_dir: out_dir.to_path_buf(),
        quality,
        h264: resolve_codec_options(quality, Codec::H264, config.h264.as_ref(), &codecs),
        h265: resolve_codec_options(quality, Codec::H265, config.h265.as_ref(), &codecs),
        vp9: resolve_codec_options(quality, Codec::Vp9, config.vp9.as_ref(), &codecs),
        av1: resolve_codec_options(quality, Codec::Av1, config.av1.as_ref(), &codecs),
        codecs,
        crop,
        widths,
        trim,
        fps: config.fps.map(|f| Fps {
            num: f.num,
            den: f.den,
        }),
        audio,
        poster,
        subtitles,
    }
}

fn resolve_codec_options(
    quality: Quality,
    codec: Codec,
    specified: Option<&CodecOverrides>,
    codecs: &[Codec],
) -> CodecOptions {
    if !codecs.contains(&codec) {
        return CodecOptions::default();
    }

    let mut options = expand_quality(quality, codec);
    let Some(specified) = specified else {
        return options;
    };

    // Field by field: anything the user didn't name keeps its expanded value.
    options.crf = specified.crf.or(options.crf);
    options.preset = specified.preset.clone().or(options.preset);
    options.profile = specified.profile.clone().or(options.profile);
    options.cpu_used = specified.cpu_used.or(options.cpu_used);
    options.row_mt = specified.row_mt.or(options.row_mt);
    if let Some(extra_args) = &specified.extra_args {
        options.extra_args = extra_args.clone();
    }
    options
}

fn resolve_crop(crop: &config::Crop) -> Crop {
    let (ratio, anchor) = match crop {
        config::Crop::Bare(ratio) => (ratio.as_str(), Anchor::Centre),
        config::Crop::Anchored { ratio, anchor } => (ratio.as_str(), *anchor),
    };
    Crop {
        ratio: ratio_or_panic(ratio),
        anchor,
    }
}

fn ratio_or_panic(raw: &str) -> (u32, u32) {
    parse_ratio(raw).unwrap_or_else(|| panic!("validated: crop ratio is well-formed, got {raw:?}"))
}

/// `W:H`, both sides non-zero.
pub fn parse_ratio(raw: &str) -> Option<(u32, u32)> {
    let (w, h) = raw.split_once(':')?;
    let w: u32 = w.trim().parse().ok()?;
    let h: u32 = h.trim().parse().ok()?;
    (w > 0 && h > 0).then_some((w, h))
}

fn resolve_trim(trim: &config::TimeRange) -> TimeRange {
    TimeRange {
        start_secs: trim.start.as_deref().map(timestamp_or_panic).unwrap_or(0.0),
        end_secs: trim.end.as_deref().map(timestamp_or_panic),
    }
}

fn timestamp_or_panic(raw: &str) -> f64 {
    parse_timestamp(raw)
        .unwrap_or_else(|| panic!("validated: timestamp is well-formed, got {raw:?}"))
}

/// `HH:MM:SS(.ms)`, `MM:SS(.ms)` or bare seconds.
pub fn parse_timestamp(raw: &str) -> Option<f64> {
    let raw = raw.trim();
    let Some((rest, secs)) = raw.rsplit_once(':') else {
        return non_negative(raw.parse().ok()?);
    };

    let secs: f64 = secs.parse().ok()?;
    let (hours, minutes): (u32, u32) = match rest.split_once(':') {
        Some((h, m)) => (h.trim().parse().ok()?, m.trim().parse().ok()?),
        None => (0, rest.trim().parse().ok()?),
    };
    non_negative(hours as f64 * 3600.0 + minutes as f64 * 60.0 + secs)
}

fn non_negative(secs: f64) -> Option<f64> {
    (secs.is_finite() && secs >= 0.0).then_some(secs)
}

fn resolve_audio(field: Option<&AudioField>, has_audio: bool) -> Option<AudioSettings> {
    match field {
        Some(AudioField::Off(false)) => None,
        Some(AudioField::Off(true)) | None if !has_audio => None,
        Some(AudioField::Off(true)) | None => Some(AudioSettings {
            normalize: false,
            bitrate: default_audio_bitrate().to_string(),
        }),
        Some(AudioField::Settings(settings)) => Some(AudioSettings {
            normalize: settings.normalize.unwrap_or(false),
            bitrate: settings
                .bitrate
                .clone()
                .unwrap_or_else(|| default_audio_bitrate().to_string()),
        }),
    }
}

fn default_audio_bitrate() -> &'static str {
    "128k"
}

/// `None` means `poster = false`. `at: None` means the default: the first
/// frame after trim.
fn resolve_poster(field: Option<&PosterField>) -> Option<PosterSettings> {
    match field {
        Some(PosterField::Off(false)) => None,
        Some(PosterField::Settings(settings)) => Some(PosterSettings {
            at: settings
                .at
                .as_deref()
                .map(|raw| Timestamp(timestamp_or_panic(raw))),
        }),
        Some(PosterField::Off(true)) | None => Some(PosterSettings { at: None }),
    }
}

/// `None` means `subtitles = false`.
fn resolve_subtitles(field: Option<&SubtitlesField>) -> Option<SubtitleSettings> {
    match field {
        Some(SubtitlesField::Off(false)) => None,
        Some(SubtitlesField::Settings(settings)) => Some(SubtitleSettings {
            language: settings.language.clone(),
            model: settings.model.unwrap_or(WhisperModel::Base),
        }),
        Some(SubtitlesField::Off(true)) | None => Some(SubtitleSettings {
            language: None,
            model: WhisperModel::Base,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TargetConfig;
    use std::path::PathBuf;

    fn probe(width: u32, height: u32, has_audio: bool) -> Probe {
        Probe {
            src: PathBuf::from("video.mp4"),
            width,
            height,
            duration_secs: 10.0,
            frame_rate: (30, 1),
            has_audio,
            video_codec: "h264".to_string(),
            audio_codec: has_audio.then(|| "aac".to_string()),
            size_bytes: 1_000_000,
        }
    }

    fn config() -> TargetConfig {
        TargetConfig {
            src: Some(PathBuf::from("video.mp4")),
            ..Default::default()
        }
    }

    fn out_dir() -> PathBuf {
        PathBuf::from("assets/video")
    }

    #[test]
    fn ladder_derives_from_post_crop_width() {
        let mut cfg = config();
        cfg.crop = Some(config::Crop::Bare("9:16".to_string()));
        let settings = resolve(&cfg, &probe(1920, 1080, true), &out_dir());
        // 9:16 crop of 1920x1080 is height-limited: 1080 * 9 / 16 = 607, and
        // the crop filter rounds that down to an even 606.
        assert_eq!(settings.widths, derive_ladder(606));
        assert!(settings.widths.iter().all(|&w| w <= 606));
    }

    /// The ladder has to plan against the width the crop filter really
    /// produces, or a rung lands a pixel wider than the frame and upscales.
    #[test]
    fn post_crop_width_matches_the_crop_filter() {
        let mut cfg = config();
        cfg.crop = Some(config::Crop::Bare("9:16".to_string()));
        let probe = probe(1280, 720, true);
        let settings = resolve(&cfg, &probe, &out_dir());
        let (cropped, _) = crate::command::cropped_size(settings.crop, &probe);
        assert_eq!(cropped, 404);
        assert!(settings.widths.iter().all(|&w| w <= cropped));
    }

    #[test]
    fn no_audio_track_means_no_audio_settings() {
        let settings = resolve(&config(), &probe(640, 480, false), &out_dir());
        assert!(settings.audio.is_none());
    }

    #[test]
    fn audio_false_strips_the_track_even_when_present() {
        let mut cfg = config();
        cfg.audio = Some(AudioField::Off(false));
        let settings = resolve(&cfg, &probe(640, 480, true), &out_dir());
        assert!(settings.audio.is_none());
    }

    #[test]
    fn per_codec_override_wins_over_quality_expansion() {
        let mut cfg = config();
        cfg.h264 = Some(CodecOverrides {
            crf: Some(20),
            ..Default::default()
        });
        let settings = resolve(&cfg, &probe(1920, 1080, true), &out_dir());
        assert_eq!(settings.h264.crf, Some(20));

        let expanded = expand_quality(Quality::Balanced, Codec::H264);
        assert_ne!(expanded.crf, Some(20), "the override must differ to prove");
        assert_eq!(settings.h264.preset, expanded.preset);
    }

    #[test]
    fn excluded_codec_gets_no_expansion() {
        let mut cfg = config();
        cfg.codecs = Some(vec![Codec::H264]);
        let settings = resolve(&cfg, &probe(1920, 1080, true), &out_dir());
        assert_eq!(settings.vp9.crf, None);
    }

    #[test]
    fn timestamp_parses_hms_and_bare_seconds() {
        assert_eq!(timestamp_or_panic("00:00:04"), 4.0);
        assert_eq!(timestamp_or_panic("00:01:04.5"), 64.5);
        assert_eq!(timestamp_or_panic("4.5"), 4.5);
        assert_eq!(timestamp_or_panic("01:04"), 64.0);
    }

    #[test]
    fn malformed_timestamps_are_rejected() {
        for raw in ["", "abc", "00:aa:04", "-4", "1:2:3:4"] {
            assert_eq!(parse_timestamp(raw), None, "{raw:?} should not parse");
        }
    }

    #[test]
    fn malformed_ratios_are_rejected() {
        for raw in ["16x9", "16:", "16", "0:9", "16:0", "a:b"] {
            assert_eq!(parse_ratio(raw), None, "{raw:?} should not parse");
        }
        assert_eq!(parse_ratio("9:16"), Some((9, 16)));
    }
}
