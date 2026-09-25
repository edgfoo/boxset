//! `Settings`: every field of a target decided, nothing left optional.

use std::path::PathBuf;

pub use crate::config::{Anchor, Codec, CodecOverrides, Quality, WhisperModel};

#[derive(Debug, Clone)]
pub struct Settings {
    pub src: PathBuf,
    pub name: Option<String>,
    pub out_dir: PathBuf,
    pub quality: Quality,
    pub codecs: Vec<Codec>,
    pub crop: Option<Crop>,
    pub widths: Vec<u32>,
    pub trim: Option<TimeRange>,
    pub fps: Option<Fps>,
    pub audio: Option<AudioSettings>,
    pub poster: Option<PosterSettings>,
    pub subtitles: Option<SubtitleSettings>,
    pub h264: CodecOverrides,
    pub h265: CodecOverrides,
    pub vp9: CodecOverrides,
    pub av1: CodecOverrides,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crop {
    pub ratio: (u32, u32),
    pub anchor: Anchor,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeRange {
    pub start_secs: f64,
    pub end_secs: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fps {
    pub num: u32,
    pub den: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioSettings {
    pub normalize: bool,
    pub bitrate: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timestamp(pub f64);

#[derive(Debug, Clone)]
pub struct PosterSettings {
    /// `None` means the default: the first frame after trim, resolved once
    /// the task's trim window is known.
    pub at: Option<Timestamp>,
}

#[derive(Debug, Clone)]
pub struct SubtitleSettings {
    pub language: Option<String>,
    pub model: WhisperModel,
}

const RUNG_WIDTHS: [u32; 5] = [480, 640, 960, 1280, 1920];

/// The widths to encode for a source this wide, widest first, dropping any
/// rung less than twice as narrow as the last one kept: two rungs close in
/// width cost an encode each and give the page almost the same file.
pub fn derive_ladder(post_crop_width: u32) -> Vec<u32> {
    let candidates: Vec<u32> = RUNG_WIDTHS
        .into_iter()
        .filter(|&w| w <= post_crop_width)
        .collect();

    let Some(&largest) = candidates.last() else {
        // Source narrower than the smallest rung: never upscale.
        return vec![post_crop_width];
    };

    let mut kept = vec![largest];
    for &width in candidates.iter().rev().skip(1) {
        if width * 2 <= *kept.last().unwrap() {
            kept.push(width);
        }
    }
    kept.reverse();
    kept
}

/// Fixed per codec: `quality` varies compression, not effort. libvpx cpu-used
/// above 3 disables rate-distortion optimisation, so those values encode worse
/// at every CRF rather than merely faster.
fn effort(codec: Codec) -> &'static str {
    match codec {
        Codec::H264 | Codec::H265 => "veryslow",
        Codec::Vp9 => "0",
        Codec::Av1 => "4",
    }
}

pub fn expand_quality(quality: Quality, codec: Codec) -> CodecOverrides {
    use Codec::*;
    use Quality::*;

    // CRF values are not consistent between codecs. Each is tuned to match an h264 baseline on
    // measured quality, so one quality tier produces roughly the same quality across all codecs.
    // VP9 and AV1 require higher CRFs for the same quality.
    let crf: u32 = match (quality, codec) {
        (Low, H264) => 30,
        (Balanced, H264) => 26,
        (High, H264) => 23,
        (Max, H264) => 20,
        (Low, Vp9) => 47,
        (Balanced, Vp9) => 42,
        (High, Vp9) => 34,
        (Max, Vp9) => 29,
        (Low, H265) => 33,
        (Balanced, H265) => 29,
        (High, H265) => 26,
        (Max, H265) => 23,
        (Low, Av1) => 48,
        (Balanced, Av1) => 42,
        (High, Av1) => 34,
        (Max, Av1) => 28,
    };

    let effort = effort(codec);

    match codec {
        Vp9 => CodecOverrides {
            crf: Some(crf),
            cpu_used: effort.parse().ok(),
            row_mt: Some(true),
            ..Default::default()
        },
        _ => CodecOverrides {
            crf: Some(crf),
            preset: Some(effort.to_string()),
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ladder_is_usable() {
        for source in [400, 640, 1080, 1280, 1920, 3840] {
            let ladder = derive_ladder(source);

            assert!(!ladder.is_empty(), "{source} gave no rungs");
            assert!(
                ladder.iter().all(|&w| w <= source),
                "{source} upscales: {ladder:?}"
            );

            for pair in ladder.windows(2) {
                assert!(pair[0] * 2 <= pair[1], "{source} kept {pair:?}");
            }
        }
    }

    /// A source narrower than every rung still gets one, at its own width.
    #[test]
    fn a_source_below_the_smallest_rung_gets_itself() {
        assert_eq!(derive_ladder(400), vec![400]);
    }
}
