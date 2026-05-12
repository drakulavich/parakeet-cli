use std::time::Duration;

/// A linearized slice of an SSML document.
#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    /// Plain text to feed into the G2P → engine pipeline.
    Text(String),
    /// Pre-phonemized IPA (from a `<phoneme>` override). Bypasses G2P —
    /// the tokenizer receives the `ph` string verbatim.
    Ipa(String),
    /// Silence of the given duration.
    Break(Duration),
    /// Letter-by-letter spelling request from `<say-as interpret-as="characters">`.
    /// The Russian-Vosk normalization step expands this to a `Text` segment via
    /// `tts::ru::letter_table::expand_chars`. Other engines pass it through as text
    /// (their G2P will read the cyrillic word verbatim — acceptable until per-engine
    /// support lands).
    Spell(String),
    /// SSML `<emphasis>` content. The Russian-Vosk normalization step honors
    /// any `+` markers in `content` (passing them through to Vosk, which
    /// interprets `+vowel` as a stress hint per the #233 spike). On non-
    /// `ru-vosk-*` voices the `+` markers are stripped before reaching G2P.
    /// `suppress` is set when the source tag had `level="none"` — strip `+`
    /// markers regardless of voice (SSML composition: a
    /// `<emphasis level="none">` overrides an inherited emphasis).
    Emphasis { content: String, suppress: bool },
    /// SSML `<prosody rate>` content where the prosody wraps the entire
    /// utterance (immediate child of `<speak>`, no sibling content). The
    /// dispatcher multiplies `rate` by the CLI `--rate` and threads the
    /// result into the per-engine speed knob. Mid-utterance prosody is
    /// warned+stripped at parse time and never reaches a `ProsodyRate`
    /// segment.
    ProsodyRate { rate: f32, content: Vec<Segment> },
}

/// Default `<break/>` duration when the `time` attribute is omitted.
/// Matches SSML 1.1's "medium" strength interpretation in most engines.
pub(super) const DEFAULT_BREAK: Duration = Duration::from_millis(250);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_has_spell_variant() {
        // Ensure the variant exists and is constructible.
        let s = Segment::Spell("ВОЗ".to_string());
        match s {
            Segment::Spell(t) => assert_eq!(t, "ВОЗ"),
            _ => panic!("expected Segment::Spell"),
        }
    }

    #[test]
    fn segment_has_emphasis_variant() {
        let s = Segment::Emphasis {
            content: "д+ома".to_string(),
            suppress: false,
        };
        match s {
            Segment::Emphasis { content, suppress } => {
                assert_eq!(content, "д+ома");
                assert!(!suppress);
            }
            _ => panic!("expected Segment::Emphasis"),
        }
    }
}
