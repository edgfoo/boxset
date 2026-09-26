//! Turning a source's audio into a WebVTT track.

use std::fmt::Write as _;
use std::path::Path;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::config::WhisperModel;
use crate::error::TranscribeError;

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start_secs: f64,
    pub end_secs: f64,
    pub text: String,
}

/// Loads the cached model and transcribes 16kHz mono PCM. `language` is the
/// language spoken in the source, a hint for accuracy; `None` auto-detects.
/// boxset transcribes and never translates. `max_cue_chars` caps how long a
/// cue may run, `None` defaults to whisper's sentence-level segmentation.
pub fn transcribe(
    model: WhisperModel,
    model_path: &Path,
    samples: &[f32],
    language: Option<&str>,
    max_cue_chars: Option<u32>,
    mut on_progress: impl FnMut(f32) + 'static,
) -> Result<Vec<Cue>, TranscribeError> {
    // whisper.cpp and GGML log to stderr by default, which would wreck the
    // CLI's line output. With no logging backend enabled these go nowhere.
    // Safe to call repeatedly; only the first call has an effect.
    whisper_rs::install_logging_hooks();

    let context = WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
        .map_err(|source| TranscribeError::ModelLoad {
            model,
            path: model_path.to_path_buf(),
            source,
        })?;

    let mut state = context.create_state().map_err(TranscribeError::Inference)?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 5 });
    params.set_language(language);
    // boxset transcribes; translation would silently change the output language.
    params.set_translate(false);
    // Stops the model emitting sound annotations like `(upbeat music)`.
    params.set_suppress_nst(true);
    if let Some(max_chars) = max_cue_chars {
        // whisper derives the per-segment character count from token
        // timestamps, so max_len does nothing unless these are on.
        params.set_token_timestamps(true);
        params.set_max_len(max_chars as i32);
        // Otherwise a split lands mid-word.
        params.set_split_on_word(true);
    }
    // whisper.cpp prints to stdout by default, which would corrupt the CLI's output.
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_progress_callback_safe(move |percent: i32| on_progress(percent as f32 / 100.0));

    state
        .full(params, samples)
        .map_err(TranscribeError::Inference)?;

    let mut cues = Vec::new();
    for segment in state.as_iter() {
        // Lossy: a segment whose bytes aren't valid UTF-8 is worth keeping
        // approximately rather than failing the whole track over.
        let Ok(text) = segment.to_str_lossy() else {
            continue;
        };
        let text = text.trim().to_string();
        // `[BLANK_AUDIO]` is whisper's own marker for a silent segment, which
        // we don't want to show during silence.
        if text.is_empty() || text == "[BLANK_AUDIO]" {
            continue;
        }
        cues.push(Cue {
            // whisper.cpp reports centiseconds.
            start_secs: segment.start_timestamp() as f64 / 100.0,
            end_secs: segment.end_timestamp() as f64 / 100.0,
            text,
        });
    }

    Ok(cues)
}

/// WebVTT, which needs the `WEBVTT` header and `HH:MM:SS.mmm` timestamps.
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
/// floats whisper takes. A truncated trailing byte is dropped.
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

    /// The blank line between cues is what players parse on; without it the
    /// whole track reads as one malformed cue.
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

    /// Rounding has to carry into the next second rather than print `.1000`.
    #[test]
    fn a_millisecond_short_of_a_minute_rounds_up_to_it() {
        assert_eq!(timestamp(59.9999), "00:01:00.000");
        assert_eq!(timestamp(3661.5), "01:01:01.500");
    }
}
