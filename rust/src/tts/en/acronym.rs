//! Tokenize English plain text and emit a segment list with two kinds of
//! pronunciation overrides:
//!
//! - **`IPA_LEXICON`** — case-sensitive token → IPA-phoneme map. Hits emit
//!   `Segment::Ipa(...)` which `synth_segments_kokoro_with` routes directly
//!   to `infer_ipa`, bypassing G2P. Covers all-caps acronyms with
//!   industry pronunciations (EPAM, JSON, JPEG, …) AND mixed-case proper
//!   nouns Kokoro mispronounces (Anthropic, Microsoft, Claude, …).
//! - **Letter-spell rule** — uppercase 2..=5 tokens NOT on `STOP_LIST` and
//!   NOT in `IPA_LEXICON` get expanded letter-by-letter via
//!   `letter_table::expand_chars` (still grapheme-level — Kokoro reads
//!   "ef bee eye" naturally). Gated by `auto_expand`.
//! - **`STOP_LIST` (30 entries)** — natural-English caps words that Kokoro
//!   handles via its training; passed through unchanged.
//!
//! IPA hits are intent-explicit (parallel to `<say-as>`); they fire even
//! when `auto_expand=false`. Letter-spelling is gated by `auto_expand`.
//!
//! Spec: `docs/superpowers/specs/2026-05-07-kokoro-en-acronym-expansion-design.md`.

use super::letter_table::expand_chars;
use crate::tts::ssml::Segment;

const STOP_LIST: &[&str] = &[
    // Emphatic length-2 caps
    "OK", "NO", "GO", "IT", "IS", "AS", "AT", "BY", "IN", "ON", "OR", "OF", "TO", "WE", "US", "MY",
    "ME", "HE", "BE", "DO", // Natural-English caps words
    "NASA", "NATO", "AIDS", "OPEC", "IKEA", "ASCII", "NAFTA", "LASER", "RADAR", "SCUBA",
];

/// Token → IPA phoneme map. Keys are case-sensitive. Values use IPA without
/// syllable separators (`.`) — the slash notation `/…/` and dot separators
/// in user-supplied IPA are presentation-only; Kokoro's `infer_ipa` accepts
/// stress marks (`ˈ` `ˌ`) and length marks (`ː`) but not separators.
const IPA_LEXICON: &[(&str, &str)] = &[
    // All-caps acronyms with industry pronunciations
    ("EPAM", "ˈiːpæm"),
    ("JSON", "ˈdʒeɪsən"),
    ("JPEG", "ˈdʒeɪpɛɡ"),
    ("GIF", "ɡɪf"),
    ("SQL", "ˈsiːkwəl"),
    ("ASAP", "ˈeɪsæp"),
    ("CRUD", "krʌd"),
    ("JWT", "ˌdʒeɪdʌbəljuːˈtiː"),
    // Mixed-case proper nouns
    ("OAuth", "ˈoʊɔːθ"),
    ("Microsoft", "ˈmaɪkroʊsɔːft"),
    ("Anthropic", "ænˈθrɒpɪk"),
    ("Claude", "klɔːd"),
    // NVIDIA: removed from lexicon — Kokoro renders it natively. None of A–N
    // IPA renderings reproduced the desired "en-VID-ee-ah" pronunciation
    // accurately enough; default G2P path is the cleanest fallback.
    ("Kubernetes", "ˌkuːbərˈnɛtiːz"),
    ("PostgreSQL", "ˈpoʊstɡrɛs"),
    ("GraphQL", "ˌɡræfˈkjuːɛl"),
    ("Linux", "ˈlɪnəks"),
    ("Tokio", "ˈtoʊkioʊ"),
    ("macOS", "ˌmækˈoʊɛs"),
    ("Granola", "ɡrəˈnoʊlə"),
];

const TRAILING_PUNCT: &[char] = &[
    '.', ',', ':', ';', '!', '?', '»', ')', '„', '"', '…', '—', '–', '-',
];

const LEADING_PUNCT: &[char] = &['«', '(', '"', '„'];

/// Returns true if `core` is a candidate for letter-by-letter spelling.
/// Pure structural check — does not consult the stop-list or lexicon.
fn is_acronym_token(core: &str) -> bool {
    let len = core.chars().count();
    if !(2..=5).contains(&len) {
        return false;
    }
    core.chars().all(|c| c.is_ascii_uppercase())
}

/// Tokenize `text` and emit a segment list:
/// - Tokens hit by `IPA_LEXICON` (case-sensitive on the punct-stripped core)
///   become `Segment::Ipa(...)`; surrounding head/tail punct rejoin the
///   adjacent text segment so sentence shape is preserved.
/// - Uppercase 2..=5 tokens not in `STOP_LIST` and not in `IPA_LEXICON` get
///   letter-spelled into the surrounding text segment, gated by `auto_expand`.
/// - Everything else passes through verbatim.
///
/// Returns a list with at most one Text segment between any two Ipa
/// segments. Empty input returns an empty list.
pub fn expand_to_segments(text: &str, auto_expand: bool) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    let mut buf = String::new();
    let mut tok = String::new();

    for c in text.chars() {
        if c.is_whitespace() {
            if !tok.is_empty() {
                process_token(&tok, auto_expand, &mut buf, &mut out);
                tok.clear();
            }
            buf.push(c);
        } else {
            tok.push(c);
        }
    }
    if !tok.is_empty() {
        process_token(&tok, auto_expand, &mut buf, &mut out);
    }
    flush_buf(&mut buf, &mut out);
    out
}

/// Drain `buf` into `out` as a `Segment::Text` if non-empty. No-op otherwise.
fn flush_buf(buf: &mut String, out: &mut Vec<Segment>) {
    if !buf.is_empty() {
        out.push(Segment::Text(std::mem::take(buf)));
    }
}

fn process_token(token: &str, auto_expand: bool, buf: &mut String, out: &mut Vec<Segment>) {
    let (head, mid, tail) = split_punct(token);

    if let Some(ipa) = IPA_LEXICON.iter().find(|(k, _)| *k == mid).map(|(_, v)| *v) {
        // Flush accumulated text + leading punct, emit Ipa, start new buf with tail.
        buf.push_str(head);
        flush_buf(buf, out);
        out.push(Segment::Ipa(ipa.to_string()));
        buf.push_str(tail);
        return;
    }

    if auto_expand && is_acronym_token(mid) && !STOP_LIST.contains(&mid) {
        buf.push_str(head);
        buf.push_str(&expand_chars(mid));
        buf.push_str(tail);
        return;
    }

    buf.push_str(token);
}

/// Split `token` into (leading_punct, core, trailing_punct).
fn split_punct(token: &str) -> (&str, &str, &str) {
    let start = token
        .char_indices()
        .find(|(_, c)| !LEADING_PUNCT.contains(c))
        .map(|(i, _)| i)
        .unwrap_or(token.len());
    let rest = &token[start..];
    let mut end = rest.len();
    for (idx, c) in rest.char_indices().rev() {
        if TRAILING_PUNCT.contains(&c) {
            end = idx;
        } else {
            break;
        }
    }
    let head = &token[..start];
    let core = &rest[..end];
    let tail = &rest[end..];
    (head, core, tail)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: collapse a segment list into a "[ipa]"-tagged string for
    /// readable test assertions. Unrelated to the production flow.
    fn flatten(segs: &[Segment]) -> String {
        let mut s = String::new();
        for seg in segs {
            match seg {
                Segment::Text(t) => s.push_str(t),
                Segment::Ipa(p) => {
                    s.push('[');
                    s.push_str(p);
                    s.push(']');
                }
                _ => panic!("unexpected variant in flatten test helper"),
            }
        }
        s
    }

    // --- Letter-spell cases (no IPA hit) ---

    #[test]
    fn fbi_letter_spells_when_auto_expand() {
        let segs = expand_to_segments("FBI investigation", true);
        assert_eq!(flatten(&segs), "ef bee eye investigation");
    }

    #[test]
    fn fbi_passes_through_when_no_auto_expand() {
        let segs = expand_to_segments("FBI investigation", false);
        assert_eq!(flatten(&segs), "FBI investigation");
    }

    #[test]
    fn http_json_mixed_letter_and_ipa() {
        // HTTP letter-spells; JSON hits IPA_LEXICON.
        let segs = expand_to_segments("HTTP and JSON", true);
        assert_eq!(flatten(&segs), "aitch tee tee pee and [ˈdʒeɪsən]");
    }

    // --- IPA lexicon hits ---

    #[test]
    fn epam_emits_ipa_segment() {
        let segs = expand_to_segments("EPAM partners", true);
        assert_eq!(flatten(&segs), "[ˈiːpæm] partners");
    }

    #[test]
    fn json_lexicon_hit() {
        let segs = expand_to_segments("a JSON file", true);
        assert_eq!(flatten(&segs), "a [ˈdʒeɪsən] file");
    }

    #[test]
    fn microsoft_mixed_case_lexicon_hit() {
        let segs = expand_to_segments("IBM and Microsoft are competitors", true);
        // IBM letter-spells (no IPA, is_acronym_token=true, not stop-listed).
        // Microsoft IPA hit — separates Text segments around it.
        assert_eq!(
            flatten(&segs),
            "eye bee em and [ˈmaɪkroʊsɔːft] are competitors"
        );
    }

    #[test]
    fn ipa_fires_even_without_auto_expand() {
        // Lexicon overrides are intent-explicit; not gated by auto_expand.
        let segs = expand_to_segments("EPAM partners", false);
        assert_eq!(flatten(&segs), "[ˈiːpæm] partners");
    }

    #[test]
    fn kubernetes_long_token_lexicon_hit() {
        // 10 chars; matcher rejects (length > 5), but IPA_LEXICON is checked first.
        let segs = expand_to_segments("deploy on Kubernetes", true);
        assert_eq!(flatten(&segs), "deploy on [ˌkuːbərˈnɛtiːz]");
    }

    // --- Punctuation handling ---

    #[test]
    fn epam_with_trailing_punct() {
        let segs = expand_to_segments("EPAM.", true);
        assert_eq!(flatten(&segs), "[ˈiːpæm].");
    }

    #[test]
    fn epam_with_quotes_around_it() {
        let segs = expand_to_segments("«EPAM»", true);
        assert_eq!(flatten(&segs), "«[ˈiːpæm]»");
    }

    #[test]
    fn fbi_with_punct_letter_spells_with_punct_preserved() {
        let segs = expand_to_segments("«FBI»", true);
        assert_eq!(flatten(&segs), "«ef bee eye»");
    }

    #[test]
    fn multiple_lexicon_hits_in_one_input() {
        let segs = expand_to_segments("Anthropic builds Claude", true);
        assert_eq!(flatten(&segs), "[ænˈθrɒpɪk] builds [klɔːd]");
    }

    // --- Stop-list / non-acronym pass-through ---

    #[test]
    fn nasa_stop_list_passes_through() {
        let segs = expand_to_segments("NASA briefed Congress", true);
        assert_eq!(flatten(&segs), "NASA briefed Congress");
    }

    #[test]
    fn lowercase_word_passes_through() {
        let segs = expand_to_segments("hello world", true);
        assert_eq!(flatten(&segs), "hello world");
    }

    #[test]
    fn inflected_token_passes_through() {
        // EPAMs is mixed case — not in IPA_LEXICON, not all-caps, no expansion.
        let segs = expand_to_segments("EPAMs are growing", true);
        assert_eq!(flatten(&segs), "EPAMs are growing");
    }

    #[test]
    fn empty_input_returns_empty_list() {
        assert!(expand_to_segments("", true).is_empty());
    }

    #[test]
    fn whitespace_only_input_returns_single_text_segment() {
        let segs = expand_to_segments("   ", true);
        assert_eq!(flatten(&segs), "   ");
    }

    #[test]
    fn every_stop_list_entry_round_trips() {
        for w in STOP_LIST {
            let segs = expand_to_segments(w, true);
            assert_eq!(flatten(&segs), *w, "stop-list entry escaped: {w}");
        }
    }

    /// Sanity check that every IPA_LEXICON value contains only characters
    /// from the IPA / stress-mark / length-mark / separator alphabet that
    /// Kokoro's `infer_ipa` accepts. Catches typos like a Latin "e" in place
    /// of an IPA "ɛ" or a smart-quote sneaking in. A failure here points to
    /// a maintainer-side data error, not a code bug.
    #[test]
    fn ipa_lexicon_values_are_well_formed() {
        for (key, ipa) in IPA_LEXICON {
            assert!(!ipa.is_empty(), "IPA_LEXICON entry {key:?} has empty value");
            for ch in ipa.chars() {
                let ok = ch.is_ascii_lowercase()           // a-z (rare in IPA but ok)
                    || ch.is_whitespace()                  // space-separated phoneme groups
                    || matches!(
                        ch,
                        // Stress + length + syllable separator
                        'ˈ' | 'ˌ' | 'ː' | '.' |
                        // Vowel symbols
                        'ə' | 'ɛ' | 'ɪ' | 'ɒ' | 'ɔ' | 'ʌ' | 'ʊ' | 'æ' | 'ɑ' |
                        // Consonant symbols
                        'ʃ' | 'ʒ' | 'ʧ' | 'ʤ' | 'ŋ' | 'θ' | 'ð' | 'ɹ' | 'ɾ' | 'ɫ' |
                        // Velar g (IPA U+0261, distinct from ASCII 'g') + tie / glottal
                        'ɡ' | 'ʔ' | '͡'
                    );
                assert!(
                    ok,
                    "IPA_LEXICON[{key:?}] contains unexpected char {ch:?} (U+{:04X})",
                    ch as u32
                );
            }
        }
    }
}
