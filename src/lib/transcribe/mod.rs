//! Turning a source's audio into a WebVTT track.

mod whisper;

use std::fmt::Write as _;
use std::path::Path;

use crate::config::TranscriptionModel;
use crate::error::TranscribeError;

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start_secs: f64,
    pub end_secs: f64,
    pub text: String,
}

pub struct ModelSource {
    /// Cache filename, unique across engines
    pub file_name: &'static str,
    pub url: String,
    pub sha256: &'static str,
    /// Only tells the user how large the download will be. Not necessarily accurate.
    pub size_bytes: u64,
}

pub fn model_source(model: TranscriptionModel) -> ModelSource {
    match model {
        TranscriptionModel::Whisper(tier) => whisper::model_source(tier),
    }
}

/// Transcribes 16kHz mono PCM with the cached model at `model_path`.
/// `language` is a language hint, `None` auto-detects.
/// `max_cue_chars` caps how long a cue may run.
pub fn transcribe(
    model: TranscriptionModel,
    model_path: &Path,
    samples: &[f32],
    language: Option<&str>,
    max_cue_chars: Option<u32>,
    on_progress: impl FnMut(f32) + 'static,
) -> Result<Vec<Cue>, TranscribeError> {
    match model {
        TranscriptionModel::Whisper(tier) => whisper::transcribe(
            tier,
            model_path,
            samples,
            language,
            max_cue_chars,
            on_progress,
        ),
    }
}

pub fn to_webvtt(cues: &[Cue]) -> String {
    let mut out = String::from("WEBVTT\n");
    for cue in cues {
        out.push('\n');

        let _ = writeln!(
            out,
            "{} --> {}",
            timestamp(cue.start_secs),
            timestamp(cue.end_secs)
        );

        out.push_str(&cue.text);
        out.push('\n');
    }
    out
}

fn timestamp(secs: f64) -> String {
    let secs = secs.max(0.0);
    let total_ms = (secs * 1000.0).round() as u64;
    let ms = total_ms % 1000;
    let total_secs = total_ms / 1000;

    format!(
        "{:02}:{:02}:{:02}.{ms:03}",
        total_secs / 3600,
        (total_secs / 60) % 60,
        total_secs % 60
    )
}

/// 16-bit little-endian mono PCM, as ffmpeg writes it, to the normalised
/// floats the engines take. A truncated trailing byte is dropped.
pub fn pcm_s16le_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| i16::from_le_bytes(pair) as f32 / 32768.0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cues_are_separated_by_a_blank_line() {
        let cue = |start, end, text: &str| Cue {
            start_secs: start,
            end_secs: end,
            text: text.to_string(),
        };
        let vtt = to_webvtt(&[cue(0.0, 1.5, "Hello."), cue(1.5, 3.0, "Goodbye.")]);
        assert_eq!(
            vtt,
            "WEBVTT\n\n00:00:00.000 --> 00:00:01.500\nHello.\n\n00:00:01.500 --> 00:00:03.000\nGoodbye.\n"
        );
    }
}
