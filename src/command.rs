//! Turning a task's intent into ffmpeg arguments.

use std::path::Path;

use crate::config::{Codec, CodecOverrides};
use crate::settings::{AudioSettings, Crop, Fps, TimeRange, Timestamp};
use crate::sources::Probe;

/// vp9 encodes in two passes; every other codec in one.
pub fn stages(codec: Codec) -> &'static [&'static str] {
    match codec {
        Codec::Vp9 => &["pass 1", "pass 2"],
        _ => &["encode"],
    }
}

/// Pass 1 analyses and finishes far sooner than pass 2, so an even split would
/// look like a stall right after it.
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

/// Post-crop dimensions, which the ladder derives from.
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
        filters.push(format!("fps={}/{}", fps.num, fps.den));
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

fn audio_args(audio: Option<&AudioSettings>, codec: Codec) -> Vec<String> {
    let Some(audio) = audio else {
        return vec!["-an".to_string()];
    };

    // vp9 lives in webm, which takes opus rather than aac.
    let encoder = match codec {
        Codec::Vp9 => "libopus",
        _ => "aac",
    };

    let mut args = vec![
        "-c:a".to_string(),
        encoder.to_string(),
        "-b:a".to_string(),
        audio.bitrate.clone(),
    ];
    if audio.normalize {
        args.push("-af".to_string());
        args.push("loudnorm".to_string());
    }
    args
}

fn value<'a>(overrides: &'a CodecOverrides, expanded: &'a CodecOverrides) -> Resolved<'a> {
    Resolved {
        crf: overrides.crf.or(expanded.crf).unwrap_or(23),
        preset: overrides
            .preset
            .as_deref()
            .or(expanded.preset.as_deref())
            .unwrap_or("slow"),
        profile: overrides.profile.as_deref(),
        cpu_used: overrides.cpu_used.or(expanded.cpu_used).unwrap_or(2),
        row_mt: overrides.row_mt.or(expanded.row_mt).unwrap_or(true),
        extra_args: overrides.extra_args.as_deref().unwrap_or(&[]),
    }
}

struct Resolved<'a> {
    crf: u32,
    preset: &'a str,
    profile: Option<&'a str>,
    cpu_used: u32,
    row_mt: bool,
    extra_args: &'a [String],
}

pub struct RenditionArgs {
    /// One entry per stage; vp9 has two.
    pub stages: Vec<Vec<String>>,
    /// Removed once the task ends, whichever stage it ended on.
    pub passlog: Option<String>,
}

const KEYFRAME_SECONDS: f64 = 4.0;

/// `None` when the clip fits in one interval anyway, or the frame rate is
/// unknown.
fn keyframe_interval(fps: Option<Fps>, trim: Option<TimeRange>, probe: &Probe) -> Option<u32> {
    let rate = match fps {
        Some(fps) if fps.den > 0 => fps.num as f64 / fps.den as f64,
        Some(_) => return None,
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
    overrides: &CodecOverrides,
    expanded: &CodecOverrides,
    trim: Option<TimeRange>,
    crop: Option<Crop>,
    fps: Option<Fps>,
    audio: Option<&AudioSettings>,
    probe: &Probe,
) -> RenditionArgs {
    let v = value(overrides, expanded);
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
            let passlog = format!("{tmp_str}.passlog");
            let common = |args: &mut Vec<String>| {
                args.extend([
                    "-c:v".to_string(),
                    "libvpx-vp9".to_string(),
                    "-b:v".to_string(),
                    "0".to_string(),
                    "-crf".to_string(),
                    v.crf.to_string(),
                    "-deadline".to_string(),
                    "good".to_string(),
                    "-row-mt".to_string(),
                    if v.row_mt { "1" } else { "0" }.to_string(),
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
            one.extend(v.extra_args.iter().cloned());

            let mut two = Vec::new();
            head(&mut two);
            common(&mut two);
            two.extend([
                "-cpu-used".to_string(),
                v.cpu_used.to_string(),
                "-pix_fmt".to_string(),
                "yuv420p".to_string(),
                "-pass".to_string(),
                "2".to_string(),
            ]);
            two.extend(audio_args(audio, codec));
            two.extend(v.extra_args.iter().cloned());
            two.push(tmp_str.clone());

            RenditionArgs {
                stages: vec![one, two],
                passlog: Some(passlog),
            }
        }
        _ => {
            let mut args = Vec::new();
            head(&mut args);

            let encoder = match codec {
                Codec::H264 => "libx264",
                Codec::H265 => "libx265",
                Codec::Av1 => "libsvtav1",
                Codec::Vp9 => unreachable!("vp9 is handled above"),
            };
            args.extend(["-c:v".to_string(), encoder.to_string()]);
            args.extend(["-crf".to_string(), v.crf.to_string()]);
            args.extend(["-preset".to_string(), v.preset.to_string()]);

            if let Some(profile) = v.profile {
                args.extend(["-profile:v".to_string(), profile.to_string()]);
            }
            if codec == Codec::Av1 {
                // Required for CRF mode on ffmpeg before 4.3, harmless after.
                args.extend(["-b:v".to_string(), "0".to_string()]);
            }
            if codec == Codec::H265 {
                // Without it, Apple players reject h265 in mp4.
                args.extend(["-tag:v".to_string(), "hvc1".to_string()]);
            }
            if let Some(keyint) = keyint {
                args.extend(["-g".to_string(), keyint.to_string()]);
            }

            args.extend(["-pix_fmt".to_string(), "yuv420p".to_string()]);
            args.extend(audio_args(audio, codec));
            args.extend(["-movflags".to_string(), "+faststart".to_string()]);
            args.extend(v.extra_args.iter().cloned());
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
    // Seeking before -i is the fast path; `at` is a timestamp into the source.
    args.extend(["-ss".to_string(), format!("{}", at.0)]);
    args.extend(["-i".to_string(), src.to_string_lossy().to_string()]);
    args.extend(["-vf".to_string(), video_filters(width, crop, None, probe)]);
    args.extend(["-frames:v".to_string(), "1".to_string()]);
    args.extend(["-q:v".to_string(), "3".to_string()]);
    args.push(tmp.to_string_lossy().to_string());
    args
}

/// Extraction, then inference. Both are long enough on a real source to be
/// worth naming separately.
pub fn subtitle_stages() -> &'static [&'static str] {
    &["extracting audio", "transcribing"]
}

/// 16kHz mono PCM, the input format whisper.cpp requires. `-f s16le` forces
/// headerless output: the samples are read back raw, so a WAV header would
/// be decoded as audio.
pub fn audio_extract_args(src: &Path, tmp: &Path, trim: Option<TimeRange>) -> Vec<String> {
    let mut args = vec!["-y".to_string()];
    args.extend(trim_args(trim));
    args.extend(["-i".to_string(), src.to_string_lossy().to_string()]);
    args.extend(["-vn".to_string()]);
    args.extend(["-ac".to_string(), "1".to_string()]);
    args.extend(["-ar".to_string(), "16000".to_string()]);
    args.extend(["-c:a".to_string(), "pcm_s16le".to_string()]);
    args.extend(["-f".to_string(), "s16le".to_string()]);
    args.push(tmp.to_string_lossy().to_string());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(frame_rate: (u32, u32), duration_secs: f64) -> Probe {
        Probe {
            src: std::path::PathBuf::from("in.mp4"),
            width: 1920,
            height: 1080,
            duration_secs,
            frame_rate,
            has_audio: true,
        }
    }

    /// A clip fitting in one interval already has only its opening keyframe,
    /// so forcing one in would just add bytes.
    #[test]
    fn a_clip_shorter_than_the_interval_gets_none() {
        assert_eq!(keyframe_interval(None, None, &probe((25, 1), 4.0)), None);
        assert_eq!(
            keyframe_interval(None, None, &probe((25, 1), 4.5)),
            Some(100)
        );
    }

    /// The interval follows the output: trim sets its length, `--fps` its
    /// rate, neither of which the source's own figures give.
    #[test]
    fn the_interval_follows_the_output_not_the_source() {
        let trim = Some(TimeRange {
            start_secs: 10.0,
            end_secs: Some(13.0),
        });
        assert_eq!(keyframe_interval(None, trim, &probe((25, 1), 60.0)), None);

        let fps = Some(Fps { num: 25, den: 1 });
        assert_eq!(
            keyframe_interval(fps, None, &probe((50, 1), 30.0)),
            Some(100)
        );
    }
}
