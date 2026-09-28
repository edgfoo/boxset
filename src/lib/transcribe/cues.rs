//! Turning a `Transcript` into cues.
//!
//! For engines that return word-level timestamps (like Parakeet), we split transcripts up according
//! to cue length and/or long pauses. For engines that only return segments (like Whisper), we just
//! return the transcript - these engines often do some segmenting themselves, at least.

use transcribe_cpp::{Transcript, Word};

use super::Cue;

/// A silence at least this long between two words is a break in speech rather
/// than the ordinary gap between them, and always ends a cue.
const PAUSE_MS: i64 = 800;

/// Two 42-character lines, the conventional maximum for a subtitle. A cue that
/// would run past this is broken up, since the point of splitting at all is to
/// fit on screen.
const MAX_CUE_CHARS: usize = 84;

pub fn from_transcript(transcript: &Transcript) -> Vec<Cue> {
    let mut cues = Vec::new();

    for (index, segment) in transcript.segments.iter().enumerate() {
        let words: Vec<&Word> = transcript
            .words
            .iter()
            .filter(|word| word.seg_index == index as i32)
            .collect();

        match words.is_empty() {
            true => push(segment.t0_ms, segment.t1_ms, segment.text.trim(), &mut cues),
            false => split_words(&words, &mut cues),
        }
    }

    cues
}

fn split_words(words: &[&Word], cues: &mut Vec<Cue>) {
    let words: Vec<&Word> = words
        .iter()
        .copied()
        .filter(|word| !word.text.trim().is_empty())
        .collect();

    let mut rest = words.as_slice();

    while !rest.is_empty() {
        let take = break_after(rest);
        let (cue, remainder) = rest.split_at(take + 1);

        let text = cue
            .iter()
            .map(|word| word.text.trim())
            .collect::<Vec<_>>()
            .join(" ");

        push(cue[0].t0_ms, cue[cue.len() - 1].t1_ms, &text, cues);
        rest = remainder;
    }
}

/// The index of the last word to keep in the next cue.
///
/// A real pause always wins: it is the model telling us where speech stopped.
/// Failing that, take as many words as fit and break at the most natural point
/// among them — the end of a sentence for preference, otherwise the longest
/// silence, otherwise simply the last word that fits.
fn break_after(words: &[&Word]) -> usize {
    let mut chars = 0;
    let mut last_fitting = 0;

    for (index, word) in words.iter().enumerate() {
        if index > 0 {
            let gap = word.t0_ms - words[index - 1].t1_ms;

            if gap >= PAUSE_MS {
                return index - 1;
            }
        }

        // The space before every word but the first.
        chars += word.text.trim().chars().count() + usize::from(index > 0);

        // Always keep the first word, however long it is: a cue has to hold it
        // somewhere, and an empty cue would drop it.
        if chars > MAX_CUE_CHARS && index > 0 {
            break;
        }

        last_fitting = index;
    }

    if last_fitting == words.len() - 1 {
        return last_fitting;
    }

    let fitting = &words[..=last_fitting];

    sentence_end(fitting)
        .or_else(|| longest_gap(fitting))
        .unwrap_or(last_fitting)
}

/// The last word in `words` ending a sentence, if any.
fn sentence_end(words: &[&Word]) -> Option<usize> {
    words
        .iter()
        .rposition(|word| word.text.trim_end().ends_with(['.', '?', '!', '…']))
        .filter(|&index| index < words.len() - 1)
}

/// The word before the longest silence in `words`. Gaps here are all below
/// `PAUSE_MS`, so this is the best of a set of unremarkable breaks, and is
/// only worth taking when something actually separates the words.
fn longest_gap(words: &[&Word]) -> Option<usize> {
    (1..words.len())
        .max_by_key(|&index| words[index].t0_ms - words[index - 1].t1_ms)
        .filter(|&index| words[index].t0_ms - words[index - 1].t1_ms > 0)
        .map(|index| index - 1)
}

fn push(start_ms: i64, end_ms: i64, text: &str, cues: &mut Vec<Cue>) {
    let text = text.trim();

    if text.is_empty() {
        return;
    }

    cues.push(Cue {
        start_secs: start_ms as f64 / 1000.0,
        end_secs: end_ms as f64 / 1000.0,
        text: text.to_string(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use transcribe_cpp::Segment;

    fn segment(t0_ms: i64, t1_ms: i64, text: &str) -> Segment {
        Segment {
            t0_ms,
            t1_ms,
            text: text.to_string(),
            ..Segment::default()
        }
    }

    fn word(t0_ms: i64, t1_ms: i64, text: &str) -> Word {
        Word {
            t0_ms,
            t1_ms,
            text: text.to_string(),
            ..Word::default()
        }
    }

    /// Whisper's shape: segments, no words.
    #[test]
    fn segments_without_words_are_cues_as_they_stand() {
        let transcript = Transcript {
            segments: vec![segment(0, 4000, "one two"), segment(4000, 8000, "three")],
            ..Transcript::default()
        };

        let cues = from_transcript(&transcript);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "one two");
        assert_eq!(cues[1].start_secs, 4.0);
    }

    /// Parakeet's shape: one segment, timed words. The pause after "there"
    /// breaks the cue; the short gap inside each phrase does not.
    #[test]
    fn a_pause_between_words_starts_a_new_cue() {
        let transcript = Transcript {
            segments: vec![segment(0, 3000, "hello there goodbye now")],
            words: vec![
                word(0, 500, "hello"),
                word(560, 1000, "there"),
                word(2000, 2400, "goodbye"),
                word(2460, 3000, "now"),
            ],
            ..Transcript::default()
        };

        let cues = from_transcript(&transcript);
        let texts: Vec<&str> = cues.iter().map(|cue| cue.text.as_str()).collect();
        assert_eq!(texts, vec!["hello there", "goodbye now"]);

        // Each cue is timed from its own words.
        assert_eq!(cues[0].end_secs, 1.0);
        assert_eq!(cues[1].start_secs, 2.0);
    }

    #[test]
    fn unbroken_speech_is_split_before_it_runs_too_long() {
        // 30 six-character words, no gaps and no punctuation: nothing to
        // prefer, so it falls back to the last word that fits.
        let words: Vec<Word> = (0..30)
            .map(|i| word(i * 300, i * 300 + 300, "abcdef"))
            .collect();

        let cues = from_transcript(&Transcript {
            segments: vec![segment(0, 9000, "unbroken")],
            words,
            ..Transcript::default()
        });

        assert!(cues.len() > 1, "expected a long run to be split");

        for cue in &cues {
            assert!(
                cue.text.chars().count() <= MAX_CUE_CHARS,
                "cue ran {} chars: {:?}",
                cue.text.chars().count(),
                cue.text
            );
        }
    }

    #[test]
    fn a_long_run_breaks_at_a_sentence_end_rather_than_where_it_stops_fitting() {
        // "end." sits well inside the limit; without the preference the break
        // would fall at the last word that fits instead.
        let mut words = vec![
            word(0, 300, "one"),
            word(300, 600, "two"),
            word(600, 900, "end."),
        ];
        words.extend((3..30).map(|i| word(i * 300, i * 300 + 300, "filler")));

        let cues = from_transcript(&Transcript {
            segments: vec![segment(0, 9000, "sentences")],
            words,
            ..Transcript::default()
        });

        assert_eq!(cues[0].text, "one two end.");
    }

    #[test]
    fn a_long_run_without_punctuation_breaks_at_the_widest_gap() {
        // A 300ms gap after "three" — below PAUSE_MS, but the widest available.
        let mut words = vec![
            word(0, 100, "one"),
            word(100, 200, "two"),
            word(200, 300, "three"),
            word(600, 700, "four"),
        ];
        words.extend((0..30).map(|i| word(700 + i * 100, 800 + i * 100, "filler")));

        let cues = from_transcript(&Transcript {
            segments: vec![segment(0, 4000, "gaps")],
            words,
            ..Transcript::default()
        });

        assert_eq!(cues[0].text, "one two three");
    }

    #[test]
    fn a_word_longer_than_the_limit_still_gets_a_cue() {
        let long = "x".repeat(MAX_CUE_CHARS + 20);

        let cues = from_transcript(&Transcript {
            segments: vec![segment(0, 1000, "long")],
            words: vec![word(0, 500, &long), word(500, 1000, "after")],
            ..Transcript::default()
        });

        assert_eq!(cues[0].text, long);
        assert_eq!(cues[1].text, "after");
    }
}
