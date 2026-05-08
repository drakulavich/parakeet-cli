//! English-specific text normalization for the Kokoro path.
//!
//! Mirrors `tts::ru` but tuned for misaki-rs G2P + Kokoro:
//! - `letter_table::expand_chars` — letter-by-letter spelling for
//!   `<say-as interpret-as="characters">`.
//! - `acronym::expand_acronyms` — auto-detect all-uppercase Latin acronyms.
//! - `normalize_segments` — routes `Spell`/`Text`/`Emphasis` segments.
//!
//! Closes #244.

pub(super) mod acronym;
pub(super) mod letter_table;

use crate::tts::ssml::Segment;

/// Auto-expand all-uppercase Latin acronyms in plain text. Used by the
/// non-SSML Kokoro path; the SSML path goes through `normalize_segments`
/// instead so it can also handle `Segment::Spell` and `Segment::Emphasis`.
pub fn expand_text(text: &str) -> String {
    acronym::expand_acronyms(text)
}

/// Normalize a segment list for the Kokoro path:
/// - `Spell(t)` → `Text(letter_table::expand_chars(t))` — always (not gated by `auto_expand`).
/// - `Emphasis { content, suppress }` → `Text(content_stripped_of_plus)`. If
///   `!suppress`, emit a once-per-process warning that `<emphasis>` stress
///   markers are honored only on `ru-vosk-*` voices.
/// - `Text(t)` → `Text(acronym::expand_acronyms(t))` if `auto_expand`; else unchanged.
/// - `Ipa(_)`, `Break(_)` → unchanged.
pub fn normalize_segments(segs: Vec<Segment>, auto_expand: bool) -> Vec<Segment> {
    segs.into_iter()
        .map(|s| match s {
            Segment::Spell(t) => Segment::Text(letter_table::expand_chars(&t)),
            Segment::Emphasis { content, suppress } => {
                if !suppress {
                    crate::tts::warn::warn_once(
                        "emphasis-non-ru-vosk",
                        "<emphasis> stress markers are honored only on ru-vosk-* voices; \
                         stripping `+` from content for non-Vosk path",
                    );
                }
                let stripped = if content.contains('+') {
                    content.replace('+', "")
                } else {
                    content
                };
                Segment::Text(stripped)
            }
            Segment::Text(t) if auto_expand => Segment::Text(acronym::expand_acronyms(&t)),
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn spell_segment_becomes_text_via_letter_table() {
        let out = normalize_segments(vec![Segment::Spell("EPAM".to_string())], false);
        assert_eq!(out, vec![Segment::Text("ee pee ay em".to_string())]);
    }

    #[test]
    fn text_runs_acronym_expansion_when_auto_expand_is_true() {
        let out = normalize_segments(vec![Segment::Text("EPAM partners".to_string())], true);
        assert_eq!(
            out,
            vec![Segment::Text("ee pee ay em partners".to_string())]
        );
    }

    #[test]
    fn text_passes_through_when_auto_expand_is_false() {
        let out = normalize_segments(vec![Segment::Text("EPAM partners".to_string())], false);
        assert_eq!(out, vec![Segment::Text("EPAM partners".to_string())]);
    }

    #[test]
    fn spell_wins_even_when_auto_expand_is_false() {
        // expand_chars("OK") = "oh kay" (O→"oh", K→"kay").
        let out = normalize_segments(vec![Segment::Spell("OK".to_string())], false);
        assert_eq!(out, vec![Segment::Text("oh kay".to_string())]);
    }

    #[test]
    fn break_and_ipa_pass_through() {
        let segs = vec![
            Segment::Break(Duration::from_millis(500)),
            Segment::Ipa("əˈpæm".to_string()),
        ];
        assert_eq!(normalize_segments(segs.clone(), true), segs);
    }

    #[test]
    fn emphasis_strips_plus_marker_and_yields_text() {
        let out = normalize_segments(
            vec![Segment::Emphasis {
                content: "д+ома".to_string(),
                suppress: false,
            }],
            false,
        );
        assert_eq!(out, vec![Segment::Text("дома".to_string())]);
    }

    #[test]
    fn emphasis_without_plus_still_yields_text() {
        let out = normalize_segments(
            vec![Segment::Emphasis {
                content: "regular text".to_string(),
                suppress: false,
            }],
            false,
        );
        assert_eq!(out, vec![Segment::Text("regular text".to_string())]);
    }

    #[test]
    fn emphasis_suppress_strips_plus_without_warning() {
        let out = normalize_segments(
            vec![Segment::Emphasis {
                content: "д+ома".to_string(),
                suppress: true,
            }],
            false,
        );
        assert_eq!(out, vec![Segment::Text("дома".to_string())]);
    }
}
