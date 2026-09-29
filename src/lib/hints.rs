//! Hints: what a user should do to get better output or performance from boxset

use crate::task::TaskKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hint {
    /// Subtitles were written, and no model transcribes perfectly.
    ReviewSubtitles,
}

/// The hints a run has collected so far, in the order they first applied.
#[derive(Debug, Default)]
pub struct Hints {
    hints: Vec<Hint>,
}

impl Hints {
    pub fn new() -> Self {
        Self::default()
    }

    /// A hint that applies to several outputs is only held once, so a target
    /// with a rendition each per width doesn't repeat it.
    pub fn observe_completed_output(&mut self, kind: TaskKind) {
        for hint in hints_for_output(kind) {
            if !self.hints.contains(&hint) {
                self.hints.push(hint);
            }
        }
    }

    pub fn as_slice(&self) -> &[Hint] {
        &self.hints
    }

    pub fn is_empty(&self) -> bool {
        self.hints.is_empty()
    }
}

fn hints_for_output(kind: TaskKind) -> Vec<Hint> {
    match kind {
        TaskKind::Subtitles => vec![Hint::ReviewSubtitles],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Codec;

    const RENDITION: TaskKind = TaskKind::Rendition {
        width: 960,
        codec: Codec::H264,
    };

    #[test]
    fn subtitles_get_a_review_hint_and_renditions_do_not() {
        let mut hints = Hints::new();
        hints.observe_completed_output(RENDITION);
        assert!(hints.is_empty());

        hints.observe_completed_output(TaskKind::Subtitles);
        assert_eq!(hints.as_slice(), [Hint::ReviewSubtitles]);
    }

    /// Two targets each with subtitles are still one thing to go and review.
    #[test]
    fn a_hint_applying_to_two_outputs_is_held_once() {
        let mut hints = Hints::new();
        hints.observe_completed_output(TaskKind::Subtitles);
        hints.observe_completed_output(TaskKind::Subtitles);

        assert_eq!(hints.as_slice(), [Hint::ReviewSubtitles]);
    }
}
