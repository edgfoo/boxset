//! The whisper.cpp backend.

use std::path::Path;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use super::{Cue, ModelSource};
use crate::config::{TranscriptionModel, WhisperTier};
use crate::error::TranscribeError;

const BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

pub fn model_source(tier: WhisperTier) -> ModelSource {
    let (file_name, sha256, size_bytes) = match tier {
        WhisperTier::Tiny => (
            "ggml-tiny.bin",
            "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
            77_691_713,
        ),
        WhisperTier::Base => (
            "ggml-base.bin",
            "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
            147_951_465,
        ),
        WhisperTier::Small => (
            "ggml-small.bin",
            "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
            487_601_967,
        ),
        WhisperTier::Medium => (
            "ggml-medium.bin",
            "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208",
            1_533_763_059,
        ),
        WhisperTier::Large => (
            "ggml-large-v3-turbo.bin",
            "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69",
            1_624_555_275,
        ),
    };

    ModelSource {
        file_name,
        url: format!("{BASE_URL}/{file_name}"),
        sha256,
        size_bytes,
    }
}

/// `max_cue_chars` caps how long a cue may run.
/// `None` defaults to whisper's sentence-level segmentation.
pub fn transcribe(
    tier: WhisperTier,
    model_path: &Path,
    samples: &[f32],
    language: Option<&str>,
    max_cue_chars: Option<u32>,
    mut on_progress: impl FnMut(f32) + 'static,
) -> Result<Vec<Cue>, TranscribeError> {
    // Prevent whisper.cpp and GGML logging to stderr
    whisper_rs::install_logging_hooks();

    let model = TranscriptionModel::Whisper(tier);
    let context = WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
        .map_err(|e| TranscribeError::ModelLoad {
            model,
            path: model_path.to_path_buf(),
            detail: e.to_string(),
        })?;

    let mut state = context
        .create_state()
        .map_err(|e| TranscribeError::Inference {
            model,
            detail: e.to_string(),
        })?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 5 });

    params.set_language(language);
    params.set_translate(false);

    // Don't emit sound annotations like `(upbeat music)`.
    params.set_suppress_nst(true);

    if let Some(max_chars) = max_cue_chars {
        // whisper derives the per-segment character count from token timestamps,
        // max_len needs this to be turned on.
        params.set_token_timestamps(true);

        params.set_split_on_word(true);
        params.set_max_len(max_chars as i32);
    }

    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_progress_callback_safe(move |percent: i32| on_progress(percent as f32 / 100.0));

    state
        .full(params, samples)
        .map_err(|e| TranscribeError::Inference {
            model,
            detail: e.to_string(),
        })?;

    let mut cues = Vec::new();
    for segment in state.as_iter() {
        let Ok(text) = segment.to_str_lossy() else {
            continue;
        };

        let text = text.trim().to_string();

        if text.is_empty() {
            continue;
        }

        cues.push(Cue {
            // whipser.cpp gives timestamps in 100s of seconds, so divide by 100
            start_secs: segment.start_timestamp() as f64 / 100.0,
            end_secs: segment.end_timestamp() as f64 / 100.0,
            text,
        });
    }

    Ok(cues)
}
