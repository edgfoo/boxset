//! Turning a source's audio into a WebVTT track.

mod cues;
mod parakeet;
mod whisper;

use std::fmt::Write as _;
use std::path::Path;

use transcribe_cpp::{Model, RunOptions, TimestampKind};

use crate::config::TranscriptionModel;
use crate::error::TranscribeError;

const QUANT: &str = "Q5_K_M";
const BASE_URL: &str = "https://huggingface.co/handy-computer";

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start_secs: f64,
    pub end_secs: f64,
    pub text: String,
}

pub struct ModelSource {
    /// Cache filename, unique across engines
    pub file_name: String,
    pub url: String,
    pub sha256: &'static str,
    /// Only tells the user how large the download will be. Not necessarily accurate.
    pub size_bytes: u64,
}

impl ModelSource {
    /// A GGUF in `handy-computer/<repo>-gguf`, named `<repo>-<QUANT>.gguf`
    fn quantized(repo: &str, sha256: &'static str, size_bytes: u64) -> ModelSource {
        let file_name = format!("{repo}-{QUANT}.gguf");
        ModelSource {
            url: format!("{BASE_URL}/{repo}-gguf/resolve/main/{file_name}"),
            file_name,
            sha256,
            size_bytes,
        }
    }
}

pub fn model_source(model: TranscriptionModel) -> ModelSource {
    match model {
        TranscriptionModel::Whisper(tier) => whisper::model_source(tier),
        TranscriptionModel::Parakeet(tier) => parakeet::model_source(tier),
    }
}

/// Transcribes 16kHz mono PCM with the cached model at `model_path`.
/// The spoken language is detected by the model.
pub fn transcribe(
    model: TranscriptionModel,
    model_path: &Path,
    samples: &[f32],
) -> Result<Vec<Cue>, TranscribeError> {
    // The native library logs to stderr unless told otherwise, which would
    // scribble over the live display.
    transcribe_cpp::disable_logging();

    let loaded = Model::load(model_path).map_err(|e| TranscribeError::ModelLoad {
        model,
        path: model_path.to_path_buf(),
        detail: e.to_string(),
    })?;

    let mut session = loaded.session().map_err(|e| TranscribeError::Inference {
        model,
        detail: e.to_string(),
    })?;

    let options = RunOptions {
        timestamps: TimestampKind::Auto,
        ..RunOptions::default()
    };

    let transcript = session
        .run(samples, &options)
        .map_err(|e| TranscribeError::Inference {
            model,
            detail: e.to_string(),
        })?;

    Ok(cues::from_transcript(&transcript))
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
