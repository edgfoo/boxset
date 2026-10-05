//! Turning a task's intent into ffmpeg arguments.

use std::path::Path;

use crate::config::Codec;
use crate::settings::{AudioSettings, CodecOptions, Crop, Fps, TimeRange, Timestamp};
use crate::sources::Probe;

pub const MIN_FFMPEG_VERSION: (u32, u32) = (7, 0);

pub fn video_encoder(codec: Codec) -> &'static str {
    match codec {
        Codec::H264 => "libx264",
        Codec::H265 => "libx265",
        Codec::Av1 => "libsvtav1",
        Codec::Vp9 => "libvpx-vp9",
    }
}

/// vp9 lives in webm, which takes opus rather than aac.
pub fn audio_encoder(codec: Codec) -> &'static str {
    match codec {
        Codec::Vp9 => "libopus",
        _ => "aac",
    }
}

pub const POSTER_ENCODER: &str = "mjpeg";
pub const AUDIO_EXTRACT_ENCODER: &str = "pcm_s16le";

/// vp9 encodes in two passes; every other codec in one.
pub fn stages(codec: Codec) -> &'static [&'static str] {
    match codec {
        Codec::Vp9 => &["pass 1", "pass 2"],
        _ => &["encode"],
    }
}

/// Estimate that pass 1 takes roughly 15% of the VP9 encoding work, pass 2
/// does the bulk of it, 85%.
const PASS_1_SHARE: f32 = 0.15;

pub fn overall_progress(codec: Codec, stage_index: u32, stage_done: f32) -> f32 {
    match (codec, stage_index) {
        (Codec::Vp9, 0) => stage_done * PASS_1_SHARE,
        (Codec::Vp9, _) => PASS_1_SHARE + stage_done * (1.0 - PASS_1_SHARE),
        _ => stage_done,
    }
}

/// The largest rectangle of the wanted ratio that fits the source, as an
/// ffmpeg `crop` filter. Cropping never scales.
fn crop_filter(crop: Crop, probe: &Probe) -> String {
    let (w, h) = cropped_size(Some(crop), probe);
    let (x, y) = crop
        .anchor
        .offset(probe.width as u64, probe.height as u64, w as u64, h as u64);
    format!("crop={w}:{h}:{x}:{y}")
}

pub fn cropped_size(crop: Option<Crop>, probe: &Probe) -> (u32, u32) {
    let Some(crop) = crop else {
        return (probe.width, probe.height);
    };
    let (rw, rh) = crop.ratio;
    let (sw, sh) = (probe.width as u64, probe.height as u64);

    let (mut w, mut h) = if sw * rh as u64 >= sh * rw as u64 {
        (sh * rw as u64 / rh as u64, sh)
    } else {
        (sw, sw * rh as u64 / rw as u64)
    };
    // Odd dimensions are rejected by yuv420p encoders.
    w -= w % 2;
    h -= h % 2;

    (w as u32, h as u32)
}

/// Crop, then scale, then fps. Trim is not here: it's applied with `-ss`/`-to`
/// on the input, which is faster than filtering every frame.
fn video_filters(width: u32, crop: Option<Crop>, fps: Option<Fps>, probe: &Probe) -> String {
    let mut filters = Vec::new();

    if let Some(crop) = crop {
        filters.push(crop_filter(crop, probe));
    }

    // -2 keeps the aspect ratio and rounds to an even height.
    filters.push(format!("scale={width}:-2"));

    if let Some(fps) = fps {
        filters.push(format!("fps={fps}"));
    }

    filters.join(",")
}

fn trim_args(trim: Option<TimeRange>) -> Vec<String> {
    let Some(trim) = trim else {
        return Vec::new();
    };
    let mut args = vec!["-ss".to_string(), format!("{}", trim.start_secs)];
    if let Some(end) = trim.end_secs {
        args.push("-to".to_string());
        args.push(format!("{end}"));
    }
    args
}

pub const LOUDNESS_TARGET: Loudness = Loudness {
    i: -14.0,
    tp: -1.0,
    lra: 11.0,
};

pub struct Loudness {
    /// Integrated loudness, LUFS
    pub i: f64,
    /// True peak ceiling, dBTP
    pub tp: f64,
    /// Loudness range, LU
    pub lra: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoudnessMeasurement {
    pub i: f64,
    pub tp: f64,
    pub lra: f64,
    pub thresh: f64,
}

impl LoudnessMeasurement {
    /// loudnorm measures silence as -inf, then rejects -inf as a measured value.
    pub fn is_silent(&self) -> bool {
        !(self.i.is_finite() && self.tp.is_finite())
    }
}

pub fn loudness_measure_args(src: &Path, trim: Option<TimeRange>) -> Vec<String> {
    let Loudness { i, tp, lra } = LOUDNESS_TARGET;
    let mut args = vec!["-y".to_string()];

    args.extend(trim_args(trim));
    args.extend(["-i".to_string(), src.to_string_lossy().to_string()]);
    args.extend(["-vn".to_string()]);
    args.extend([
        "-af".to_string(),
        format!("loudnorm=I={i}:TP={tp}:LRA={lra}:print_format=json"),
    ]);
    args.extend(["-f".to_string(), "null".to_string(), "-".to_string()]);
    args
}

pub fn parse_loudness_measurement(stderr: &str) -> Option<LoudnessMeasurement> {
    let start = stderr.rfind('{')?;
    let block = &stderr[start..stderr[start..].find('}')? + start];

    let field = |name: &str| -> Option<f64> {
        let at = block.find(&format!("\"{name}\""))?;
        let rest = &block[at..];
        let open = rest.find(':')?;
        let value = rest[open + 1..].split(',').next()?;
        value.trim().trim_matches('"').parse().ok()
    };

    Some(LoudnessMeasurement {
        i: field("input_i")?,
        tp: field("input_tp")?,
        lra: field("input_lra")?,
        thresh: field("input_thresh")?,
    })
}

fn audio_args(
    audio: Option<&AudioSettings>,
    codec: Codec,
    measured: Option<&LoudnessMeasurement>,
) -> Vec<String> {
    let Some(audio) = audio else {
        return vec!["-an".to_string()];
    };

    let mut args = vec![
        "-c:a".to_string(),
        audio_encoder(codec).to_string(),
        "-b:a".to_string(),
        audio.encode_bitrate().to_string(),
        // The loudnorm step outputs 192kHz audio. We need to downsample this
        // to 48kHz. If we didn't, aac would try to match the 192kHz but hit its
        // limit of 96kHz, which is way beyond human perception and simply wastes bits.
        "-ar".to_string(),
        "48000".to_string(),
    ];
    if audio.normalize && !measured.is_some_and(LoudnessMeasurement::is_silent) {
        let Loudness { i, tp, lra } = LOUDNESS_TARGET;
        let mut filter = format!("loudnorm=I={i}:TP={tp}:LRA={lra}");
        if let Some(m) = measured {
            filter.push_str(&format!(
                ":measured_I={}:measured_TP={}:measured_LRA={}:measured_thresh={}:linear=true",
                m.i, m.tp, m.lra, m.thresh
            ));
        }
        args.push("-af".to_string());
        args.push(filter);
    }
    args
}

pub struct RenditionArgs {
    /// One entry per stage; vp9 has two.
    pub stages: Vec<Vec<String>>,
    /// Removed once the task ends, whichever stage it ended on.
    pub passlog: Option<String>,
}

const KEYFRAME_SECONDS: f64 = 4.0;

/// `None` when the clip fits in one interval anyway, or the frame rate is unknown.
fn keyframe_interval(fps: Option<Fps>, trim: Option<TimeRange>, probe: &Probe) -> Option<u32> {
    let rate = match fps {
        Some(fps) => fps.rate(),
        None if probe.frame_rate.1 > 0 => probe.frame_rate.0 as f64 / probe.frame_rate.1 as f64,
        None => return None,
    };
    if !(rate.is_finite() && rate > 0.0) {
        return None;
    }

    let full = probe.duration_secs;
    let duration = match trim {
        Some(t) => t.end_secs.unwrap_or(full) - t.start_secs,
        None => full,
    };

    let interval = (rate * KEYFRAME_SECONDS).round();
    if duration <= KEYFRAME_SECONDS || interval < 1.0 {
        return None;
    }
    Some(interval as u32)
}

#[allow(clippy::too_many_arguments)]
pub fn rendition_args(
    src: &Path,
    tmp: &Path,
    codec: Codec,
    width: u32,
    options: &CodecOptions,
    trim: Option<TimeRange>,
    crop: Option<Crop>,
    fps: Option<Fps>,
    audio: Option<&AudioSettings>,
    measured: Option<&LoudnessMeasurement>,
    probe: &Probe,
) -> RenditionArgs {
    let crf = options.crf.expect("resolved: quality expansion sets crf");
    let extra_args = &options.extra_args;
    let filters = video_filters(width, crop, fps, probe);
    let keyint = keyframe_interval(fps, trim, probe);
    let src = src.to_string_lossy().to_string();
    let tmp_str = tmp.to_string_lossy().to_string();

    let head = |args: &mut Vec<String>| {
        args.push("-y".to_string());
        args.extend(trim_args(trim));
        args.push("-i".to_string());
        args.push(src.clone());
        args.push("-vf".to_string());
        args.push(filters.clone());
    };

    match codec {
        Codec::Vp9 => {
            let cpu_used = options.cpu_used.unwrap_or(2);
            let row_mt = options.row_mt.unwrap_or(true);
            let passlog = format!("{tmp_str}.passlog");
            let common = |args: &mut Vec<String>| {
                args.extend([
                    "-c:v".to_string(),
                    video_encoder(codec).to_string(),
                    "-b:v".to_string(),
                    "0".to_string(),
                    "-crf".to_string(),
                    crf.to_string(),
                    "-deadline".to_string(),
                    "good".to_string(),
                    "-row-mt".to_string(),
                    if row_mt { "1" } else { "0" }.to_string(),
                    "-passlogfile".to_string(),
                    passlog.clone(),
                ]);
                if let Some(keyint) = keyint {
                    args.extend(["-g".to_string(), keyint.to_string()]);
                }
            };

            // Pass 1 takes no -cpu-used: libvpx ignores it below 5 and drops to
            // a single core above it, so naming it can only cost time.
            let mut one = Vec::new();
            head(&mut one);
            common(&mut one);
            one.extend([
                "-pass".to_string(),
                "1".to_string(),
                "-an".to_string(),
                "-f".to_string(),
                "null".to_string(),
                "-".to_string(),
            ]);
            one.extend(extra_args.iter().cloned());

            let mut two = Vec::new();
            head(&mut two);
            common(&mut two);
            two.extend([
                "-cpu-used".to_string(),
                cpu_used.to_string(),
                "-pix_fmt".to_string(),
                "yuv420p".to_string(),
                "-pass".to_string(),
                "2".to_string(),
            ]);
            two.extend(audio_args(audio, codec, measured));
            two.extend(extra_args.iter().cloned());
            two.push(tmp_str.clone());

            RenditionArgs {
                stages: vec![one, two],
                passlog: Some(passlog),
            }
        }
        _ => {
            let preset = options.preset.as_deref().unwrap_or("slow");
            let mut args = Vec::new();
            head(&mut args);

            args.extend(["-c:v".to_string(), video_encoder(codec).to_string()]);
            args.extend(["-crf".to_string(), crf.to_string()]);
            args.extend(["-preset".to_string(), preset.to_string()]);

            if let Some(profile) = options.profile.as_deref() {
                args.extend(["-profile:v".to_string(), profile.to_string()]);
            }
            if codec == Codec::H265 {
                // Without it, Apple players reject h265 in mp4.
                args.extend(["-tag:v".to_string(), "hvc1".to_string()]);
            }
            if let Some(keyint) = keyint {
                args.extend(["-g".to_string(), keyint.to_string()]);
            }

            args.extend(["-pix_fmt".to_string(), "yuv420p".to_string()]);
            args.extend(audio_args(audio, codec, measured));
            args.extend(["-movflags".to_string(), "+faststart".to_string()]);
            args.extend(extra_args.iter().cloned());
            args.push(tmp_str);

            RenditionArgs {
                stages: vec![args],
                passlog: None,
            }
        }
    }
}

pub fn poster_args(
    src: &Path,
    tmp: &Path,
    width: u32,
    at: Timestamp,
    crop: Option<Crop>,
    probe: &Probe,
) -> Vec<String> {
    let mut args = vec!["-y".to_string()];
    // Seeking before -i is the fast path. `at` is a timestamp into the source.
    args.extend(["-ss".to_string(), format!("{}", at.0)]);
    args.extend(["-i".to_string(), src.to_string_lossy().to_string()]);
    args.extend(["-vf".to_string(), video_filters(width, crop, None, probe)]);
    args.extend(["-frames:v".to_string(), "1".to_string()]);
    args.extend(["-c:v".to_string(), POSTER_ENCODER.to_string()]);
    args.extend(["-q:v".to_string(), "3".to_string()]);
    args.push(tmp.to_string_lossy().to_string());
    args
}

pub fn loudness_stages() -> &'static [&'static str] {
    &["measuring loudness"]
}

pub fn subtitle_stages() -> &'static [&'static str] {
    &["extracting audio", "transcribing"]
}

/// 16kHz mono PCM, the input format transcribe.cpp requires. `-f s16le` forces
/// headerless output: the samples are read back raw, so a WAV header would
/// be decoded as audio.
pub fn audio_extract_args(src: &Path, tmp: &Path, trim: Option<TimeRange>) -> Vec<String> {
    let mut args = vec!["-y".to_string()];
    args.extend(trim_args(trim));
    args.extend(["-i".to_string(), src.to_string_lossy().to_string()]);
    args.extend(["-vn".to_string()]);
    args.extend(["-ac".to_string(), "1".to_string()]);
    args.extend(["-ar".to_string(), "16000".to_string()]);
    args.extend(["-c:a".to_string(), AUDIO_EXTRACT_ENCODER.to_string()]);
    args.extend(["-f".to_string(), "s16le".to_string()]);
    args.push(tmp.to_string_lossy().to_string());
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Quality;

    fn probe(frame_rate: (u32, u32), duration_secs: f64) -> Probe {
        Probe {
            src: std::path::PathBuf::from("in.mp4"),
            width: 1920,
            height: 1080,
            duration_secs,
            frame_rate,
            has_audio: true,
            video_codec: "h264".to_string(),
            audio_codec: Some("aac".to_string()),
            size_bytes: 1_000_000,
        }
    }

    #[test]
    fn a_clip_shorter_than_the_interval_gets_none() {
        assert_eq!(keyframe_interval(None, None, &probe((25, 1), 4.0)), None);
        assert_eq!(
            keyframe_interval(None, None, &probe((25, 1), 4.5)),
            Some(100)
        );
    }

    #[test]
    fn a_real_measurement_parses_and_a_failed_pass_yields_none() {
        let stderr = include_str!("../../tests/fixtures/ffmpeg-stderr/loudnorm-measure.txt");
        assert_eq!(
            parse_loudness_measurement(stderr),
            Some(LoudnessMeasurement {
                i: -38.88,
                tp: -25.82,
                lra: 0.00,
                thresh: -48.88,
            })
        );

        let failed = include_str!("../../tests/fixtures/ffmpeg-stderr/no-such-stream.txt");
        assert_eq!(parse_loudness_measurement(failed), None);
    }

    #[test]
    fn a_silent_track_skips_loudnorm() {
        let stderr = include_str!("../../tests/fixtures/ffmpeg-stderr/loudnorm-measure-silent.txt");
        let measured = parse_loudness_measurement(stderr).expect("silence still parses");
        assert!(measured.is_silent());

        let audio = AudioSettings {
            normalize: true,
            bitrate: None,
            quality: Quality::Balanced,
        };
        let args = audio_args(Some(&audio), Codec::H264, Some(&measured)).join(" ");
        assert!(!args.contains("loudnorm"), "{args}");
        assert!(args.contains("-c:a aac"), "{args}");
    }

    #[test]
    fn audio_is_always_48khz() {
        for normalize in [true, false] {
            let audio = AudioSettings {
                normalize,
                bitrate: None,
                quality: Quality::Balanced,
            };
            for codec in [Codec::H264, Codec::Vp9] {
                let args = audio_args(Some(&audio), codec, None).join(" ");
                assert!(args.contains("-ar 48000"), "{args}");
            }
        }
    }

    #[test]
    fn the_tier_sets_the_bitrate_unless_one_is_given() {
        let bitrate = |quality, bitrate: Option<&str>| {
            let audio = AudioSettings {
                normalize: false,
                bitrate: bitrate.map(str::to_string),
                quality,
            };
            [Codec::H264, Codec::Vp9].map(|codec| audio_args(Some(&audio), codec, None).join(" "))
        };
        for args in bitrate(Quality::Low, None) {
            assert!(args.contains("-b:a 64k"), "{args}");
        }
        for args in bitrate(Quality::Low, Some("192k")) {
            assert!(args.contains("-b:a 192k"), "{args}");
        }
    }

    #[test]
    fn a_measurement_reaches_the_filter() {
        let audio = AudioSettings {
            normalize: true,
            bitrate: None,
            quality: Quality::Balanced,
        };
        let measured = LoudnessMeasurement {
            i: -38.88,
            tp: -25.82,
            lra: 0.0,
            thresh: -48.88,
        };

        let args = audio_args(Some(&audio), Codec::H264, Some(&measured)).join(" ");
        for key in [
            "measured_I=-38.88",
            "measured_TP=-25.82",
            "measured_LRA=0",
            "measured_thresh=-48.88",
            "linear=true",
        ] {
            assert!(args.contains(key), "{key} missing from {args}");
        }

        let unmeasured = audio_args(Some(&audio), Codec::H264, None).join(" ");
        assert!(unmeasured.contains("loudnorm=I=-14"));
        assert!(!unmeasured.contains("measured_I"));
    }

    #[test]
    fn the_interval_follows_the_output_not_the_source() {
        let trim = Some(TimeRange {
            start_secs: 10.0,
            end_secs: Some(13.0),
        });
        assert_eq!(keyframe_interval(None, trim, &probe((25, 1), 60.0)), None);

        let fps = Some(Fps::Ratio { num: 25, den: 1 });
        assert_eq!(
            keyframe_interval(fps, None, &probe((50, 1), 30.0)),
            Some(100)
        );
    }
}
