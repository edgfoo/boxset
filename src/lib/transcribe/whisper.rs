//! The whisper family of transcribe.cpp.

use super::ModelSource;
use crate::config::WhisperTier;

pub fn model_source(tier: WhisperTier) -> ModelSource {
    let (repo, sha256, size_bytes) = match tier {
        WhisperTier::Tiny => (
            "whisper-tiny",
            "72cfa8ee436a635a5b6fb373cc056a828b9efe96d32d6eb8769ed3cc5b429719",
            44_211_616,
        ),
        WhisperTier::Base => (
            "whisper-base",
            "8e0feb7bc35780353cf31821018e601bb7b7cff6c9a0e17ada5a5db23f4db867",
            63_786_048,
        ),
        WhisperTier::Small => (
            "whisper-small",
            "326cd00c3e7217c751667c7c1600eaf7e0de174e186ca2c16b4bf590251c3c3b",
            193_749_056,
        ),
        WhisperTier::Medium => (
            "whisper-medium",
            "4e2a8904a866b3aa7ef70d7640ec6abc5f0a05524cd950ea4b66ace12122bf53",
            582_746_048,
        ),
        WhisperTier::Large => (
            "whisper-large-v3-turbo",
            "977b5db4e004349dffd1ab9caa10ba5aaba3fc3edd3ba72cadb84328a3203e36",
            619_628_128,
        ),
    };

    ModelSource::quantized(repo, sha256, size_bytes)
}
