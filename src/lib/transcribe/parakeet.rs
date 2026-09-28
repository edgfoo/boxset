//! The parakeet family of transcribe.cpp.

use super::ModelSource;
use crate::config::ParakeetTier;

pub fn model_source(tier: ParakeetTier) -> ModelSource {
    let (repo, sha256, size_bytes) = match tier {
        ParakeetTier::Fast => (
            "parakeet-tdt_ctc-110m",
            "1552e707fddb59e3741b66ea917fc91a9381e336a4730739b1d2d448cb013a2e",
            101_335_520,
        ),
        ParakeetTier::Multilingual => (
            "parakeet-tdt-0.6b-v3",
            "cc722e76adc1a629fc0b2535de879d99b8160d07ad4c0215e2ca7d7ea0ae4b8f",
            548_946_272,
        ),
    };

    ModelSource::quantized(repo, sha256, size_bytes)
}
