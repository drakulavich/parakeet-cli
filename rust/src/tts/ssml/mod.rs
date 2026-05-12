//! SSML → linear segment list for the TTS pipeline.
//!
//! Supported tags:
//! - `<speak>` — required root wrapper
//! - `<break time="...">` — silence of the given duration
//! - `<phoneme alphabet="ipa" ph="...">text</phoneme>` — bypass G2P and
//!   feed the IPA in `ph` directly to the synthesis tokenizer. Content
//!   text (`text` above) is suppressed. `alphabet` defaults to IPA when
//!   omitted; other values warn-strip with the inner text preserved.
//! - `<emphasis level="...">text</emphasis>` — stress hint; `level="none"` sets
//!   `suppress=true` (strip `+` markers); all other levels preserve them for Vosk
//! - `<prosody rate="...">text</prosody>` — speed multiplier; only supported when
//!   the prosody wraps the entire utterance (immediate child of `<speak>` with no
//!   other meaningful content). Mid-utterance prosody is warned and stripped.
//! - plain text inside/between elements — synthesized via G2P
//! - unknown tags — one stderr warning per name, contained text preserved

mod rate;
mod segment;
mod walker;
mod warnings;

use std::collections::HashSet;

use ssml_parser::elements::{EmphasisLevel, ParsedElement, PhonemeAlphabet};
use ssml_parser::parse_ssml;

pub use segment::Segment;

use rate::{find_relative_rate, has_structural_source_siblings, parse_rate_value};
use segment::DEFAULT_BREAK;
use walker::{parse_inner_spans, push_text_slice, span_priority};
use warnings::{WARN_PROSODY_MID_UTTERANCE, WARN_PROSODY_NO_SUPPORTED_ATTR};

/// Parse an SSML string into a linear segment list.
/// Unknown tags emit a single stderr warning per name and are otherwise stripped
/// (their text content is still synthesized).
///
/// Hardening: requires a `<speak>` root element, rejects `<!DOCTYPE>` (XXE surface),
/// and upstream `ssml-parser` disallows external entities by construction.
pub fn parse(input: &str) -> anyhow::Result<Vec<Segment>> {
    let trimmed = input.trim_start();
    if trimmed.is_empty() {
        anyhow::bail!("SSML input is empty");
    }
    if !trimmed.starts_with("<speak") {
        anyhow::bail!(
            "SSML must start with a <speak> element (got '{}...')",
            &trimmed.chars().take(20).collect::<String>()
        );
    }
    // Reject DOCTYPE declarations anywhere in the document — defense in depth
    // against billion-laughs / XXE, even though ssml-parser doesn't currently
    // expand external entities. Input length is already bounded upstream
    // (`MAX_TEXT_CHARS`), so a full scan is cheap.
    if contains_doctype(trimmed) {
        anyhow::bail!("SSML DOCTYPE declarations are not supported");
    }
    // Reject relative-percent rate values (`+N%` / `-N%`) before handing off
    // to `ssml-parser`. The upstream crate strips the `+` sign during parse
    // — Display of `RateRange::Percentage(25)` is `"25%"` regardless of the
    // original `+25%` source — so the rest of our code path would silently
    // misinterpret `+25%` as the absolute 25% (0.25×) instead of relative
    // 1.25×. `-N%` would otherwise surface upstream's cryptic "Negative
    // percentage not allowed for rate" message. Tracked as a v2 follow-up
    // on #236.
    if let Some(rel) = find_relative_rate(trimmed) {
        anyhow::bail!(
            "SSML <prosody rate=\"{rel}\"> uses a relative percentage; \
             this is not yet supported. Use an absolute percentage (e.g. \
             \"125%\") or a named value (\"x-slow\"/\"slow\"/\"medium\"/\
             \"fast\"/\"x-fast\"/\"default\") instead."
        );
    }

    let ssml = parse_ssml(input)?;
    let text: Vec<char> = ssml.get_text().chars().collect();

    // Collect all spans + sort by start. The iterator order isn't guaranteed to be textual.
    // Secondary sort: when spans share the same `start` (nested tags all map to the same
    // character range), more-specific structural tags (Phoneme, SayAs) must sort BEFORE
    // Emphasis, which must sort before Speak. This ensures that when an <emphasis> wraps
    // a <say-as> or <phoneme>, the inner tag runs first, advances `cursor`, and the outer
    // Emphasis arm is then skipped by the cursor-guard below ("inner tag wins" spec rule).
    let mut spans: Vec<_> = ssml.tags().collect();
    spans.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then_with(|| span_priority(&a.element).cmp(&span_priority(&b.element)))
    });

    let mut segments: Vec<Segment> = Vec::new();
    let mut warned: HashSet<String> = HashSet::new();
    let mut cursor: usize = 0;

    for span in &spans {
        // If a higher-priority sibling span (sorted via span_priority) already
        // consumed this region — i.e. its arm advanced cursor past the current
        // span.start — skip the current span. This implements the spec's
        // "inner structural tag wins" rule: SayAs / Phoneme have priority 0 and
        // run before the enclosing Emphasis (priority 2) when they share the
        // same character range, so the outer Emphasis is silently absorbed.
        // See #233.
        if span.start < cursor {
            continue;
        }
        match &span.element {
            // `<speak>` covers the whole document; nothing to emit for the wrapper itself.
            ParsedElement::Speak(_) => {}
            ParsedElement::Break(attrs) => {
                push_text_slice(&mut segments, &text, cursor, span.start);
                let dur = attrs
                    .time
                    .as_ref()
                    .map(|t| t.duration())
                    .unwrap_or(DEFAULT_BREAK);
                segments.push(Segment::Break(dur));
                cursor = span.end;
            }
            ParsedElement::Phoneme(attrs) => {
                // IPA override bypasses G2P. Alphabets other than `ipa`
                // warn-strip (contained text still flows as a Text segment),
                // so we only consume the span when the alphabet is IPA or
                // absent (the spec's implementation-defined default, which
                // we choose to be IPA since that's the only alphabet both
                // Kokoro's tokenizer and Piper's phoneme-id map speak).
                let is_ipa = matches!(&attrs.alphabet, None | Some(PhonemeAlphabet::Ipa));
                if is_ipa {
                    push_text_slice(&mut segments, &text, cursor, span.start);
                    if !attrs.ph.is_empty() {
                        segments.push(Segment::Ipa(attrs.ph.clone()));
                    }
                    cursor = span.end;
                } else {
                    // `is_ipa` above already filtered `None` and `Some(Ipa)`,
                    // so the only remaining variant today is `Other(s)`. Future
                    // `ssml-parser` enum growth falls into the wildcard with a
                    // synthesized name — warn + strip, never panic on user input.
                    let alpha = match &attrs.alphabet {
                        Some(PhonemeAlphabet::Other(s)) => s.clone(),
                        other => format!("{other:?}"),
                    };
                    if warned.insert(format!("phoneme[alphabet={alpha}]")) {
                        eprintln!(
                            "warning: SSML <phoneme alphabet=\"{alpha}\"> not supported — only \"ipa\" is recognised; falling back to G2P on contained text"
                        );
                    }
                }
            }
            ParsedElement::SayAs(attrs) => {
                if attrs.interpret_as == "characters" {
                    // Emit any pending text up to the tag, then a Spell segment for
                    // the inner text. Cursor advances past the closing tag so we
                    // don't double-emit the inner content as a Text fall-through.
                    push_text_slice(&mut segments, &text, cursor, span.start);
                    if let Some(inner) = extract_inner_text(&text, span.start, span.end) {
                        segments.push(Segment::Spell(inner));
                    }
                    cursor = span.end;
                } else {
                    // Other interpret-as values (cardinal, ordinal, date, telephone, …)
                    // are out of scope for #232. Keep the established warn+strip
                    // behavior; the inner text falls through as a Text segment.
                    let key = format!("say-as[interpret-as={}]", attrs.interpret_as);
                    if warned.insert(key) {
                        eprintln!(
                            "warning: SSML <say-as interpret-as=\"{}\"> is not supported — only \"characters\" is recognised; falling back to plain text",
                            attrs.interpret_as
                        );
                    }
                }
            }
            ParsedElement::Emphasis(attrs) => {
                push_text_slice(&mut segments, &text, cursor, span.start);
                if let Some(content) = extract_inner_text(&text, span.start, span.end) {
                    // SSML 1.1: missing/empty level == "moderate" (default). Only
                    // `level="none"` triggers suppression — all other variants
                    // (Strong, Moderate, Reduced) collapse to "honor `+` markers".
                    let suppress = matches!(attrs.level, Some(EmphasisLevel::None));
                    segments.push(Segment::Emphasis { content, suppress });
                }
                // Cursor advances past the entire emphasis span. Any structural child
                // (e.g. <break/>, <say-as>, <phoneme>) whose `start` falls within
                // [span.start, span.end) will be skipped by the loop-top
                // `if span.start < cursor { continue; }` guard. For <say-as> /
                // <phoneme> this is the desired "inner tag wins" behavior (the inner
                // arm runs first via span_priority sort and consumes its own range);
                // for <break/> the silence is silently absorbed into the emphasis
                // content. Out of scope per the #233 spec; tracked separately if a
                // real user hits it.
                cursor = span.end;
            }
            ParsedElement::Prosody(attrs) => {
                // Whole-utterance detection: the prosody is whole-utterance when
                // (a) the text outside [span.start, span.end) within the <speak>
                // root is entirely whitespace, AND (b) the source between the
                // `<speak ...>` open tag and the `<prosody ...>` open tag, and
                // between `</prosody>` and `</speak>`, is whitespace-only. Check
                // (b) is needed because zero-width tags like `<break/>` have a
                // collapsed text offset (start == end) that coincides with the
                // prosody boundary in the linearised text, so a check over
                // ssml-parser's text-position spans alone cannot distinguish
                // `<speak><break/><prosody>x</prosody></speak>` (mid-utterance)
                // from `<speak><prosody><break/>x</prosody></speak>` (inside).
                let prefix: String = text[..span.start].iter().collect();
                let suffix: String = text[span.end..].iter().collect();
                let is_whole_utterance = prefix.trim().is_empty()
                    && suffix.trim().is_empty()
                    && !has_structural_source_siblings(input);

                if is_whole_utterance {
                    // Attempt to parse the rate attribute.
                    let rate_str = attrs.rate.as_ref().map(|r| r.to_string());
                    let parsed_rate = rate_str.as_deref().and_then(parse_rate_value);
                    if let Some(rate) = parsed_rate {
                        // Emit ProsodyRate with the inner content parsed recursively.
                        // The inner text of the prosody span is text[span.start..span.end].
                        // Recurse: collect the sub-spans that fall within this prosody's
                        // range and parse them as a nested segment list.
                        push_text_slice(&mut segments, &text, cursor, span.start);
                        let inner_segs =
                            parse_inner_spans(&spans, &text, span.start, span.end, &mut warned);
                        segments.push(Segment::ProsodyRate {
                            rate,
                            content: inner_segs,
                        });
                        cursor = span.end;
                    } else {
                        // Whole-utterance but unparseable rate attribute — warn+strip.
                        if warned.insert(WARN_PROSODY_NO_SUPPORTED_ATTR.to_string()) {
                            eprintln!(
                                "warning: SSML <prosody> without a parseable rate= attribute \
                                 is not supported (pitch/volume scoped to a follow-up); stripping"
                            );
                        }
                        // Leave cursor unchanged; inner text flows through as Text.
                    }
                } else {
                    // Mid-utterance prosody — warn+strip.
                    if warned.insert(WARN_PROSODY_MID_UTTERANCE.to_string()) {
                        eprintln!(
                            "warning: SSML <prosody> mid-utterance is not yet supported \
                             (whole-utterance only); stripping rate, pitch, and volume"
                        );
                    }
                    // Leave cursor unchanged; inner text falls through as Text.
                }
            }
            other => {
                let name = tag_name(other);
                if warned.insert(name.clone()) {
                    eprintln!("warning: SSML tag <{name}> is not supported — stripping");
                }
                // Preserve the text content; don't touch cursor.
            }
        }
    }
    // Trailing text after the last span.
    push_text_slice(&mut segments, &text, cursor, text.len());
    Ok(segments)
}

/// Collect the inner text of a structural span and trim whitespace.
/// Returns `None` for empty/whitespace-only content. Used by tags that
/// emit a single segment carrying their inner content (SayAs, Emphasis).
pub(super) fn extract_inner_text(text: &[char], start: usize, end: usize) -> Option<String> {
    let raw: String = text[start..end].iter().collect();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub(super) fn tag_name(el: &ParsedElement) -> String {
    // Explicit map to the canonical SSML element name. Using Debug would produce
    // `sayas` for `<say-as>` and `description` for `<desc>` — user-facing warnings
    // need to match the tag the user typed.
    let name = match el {
        ParsedElement::Speak(_) => "speak",
        ParsedElement::Lexicon(_) => "lexicon",
        ParsedElement::Lookup(_) => "lookup",
        ParsedElement::Meta(_) => "meta",
        ParsedElement::Metadata => "metadata",
        ParsedElement::Paragraph => "p",
        ParsedElement::Sentence => "s",
        ParsedElement::Token(_) => "token",
        ParsedElement::Word(_) => "w",
        ParsedElement::SayAs(_) => "say-as",
        // Canonical name kept for exhaustiveness; `parse()` handles Phoneme directly.
        ParsedElement::Phoneme(_) => "phoneme",
        ParsedElement::Sub(_) => "sub",
        ParsedElement::Lang(_) => "lang",
        ParsedElement::Voice(_) => "voice",
        ParsedElement::Emphasis(_) => "emphasis",
        ParsedElement::Break(_) => "break",
        ParsedElement::Prosody(_) => "prosody",
        ParsedElement::Audio(_) => "audio",
        ParsedElement::Mark(_) => "mark",
        ParsedElement::Description(_) => "desc",
        ParsedElement::Custom((name, _)) => return name.to_ascii_lowercase(),
    };
    name.to_string()
}

/// Case-insensitive search for `<!DOCTYPE` anywhere in the input.
fn contains_doctype(input: &str) -> bool {
    const NEEDLE: &[u8] = b"<!DOCTYPE";
    let bytes = input.as_bytes();
    if bytes.len() < NEEDLE.len() {
        return false;
    }
    bytes
        .windows(NEEDLE.len())
        .any(|w| w.eq_ignore_ascii_case(NEEDLE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn plain_text_in_speak_produces_single_text_segment() {
        let segs = parse("<speak>Hello, world</speak>").unwrap();
        assert_eq!(segs.len(), 1);
        match &segs[0] {
            Segment::Text(s) => assert!(s.contains("Hello"), "got {s:?}"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn break_with_time_produces_silence_segment() {
        let segs = parse(r#"<speak>Hello <break time="500ms"/> world</speak>"#).unwrap();
        let mut text_chunks = 0;
        let mut breaks = 0;
        for s in &segs {
            match s {
                Segment::Text(_) => text_chunks += 1,
                Segment::Ipa(_) => panic!("unexpected Ipa segment"),
                Segment::Spell(_) => unreachable!("parser does not emit Spell in this fixture"),
                Segment::Emphasis { .. } => {
                    unreachable!("parser does not emit Emphasis in this fixture")
                }
                Segment::ProsodyRate { .. } => {
                    unreachable!("parser does not emit ProsodyRate in this fixture")
                }
                Segment::Break(d) => {
                    assert_eq!(*d, Duration::from_millis(500));
                    breaks += 1;
                }
            }
        }
        assert_eq!(text_chunks, 2, "expected two text chunks, got {segs:?}");
        assert_eq!(breaks, 1);
    }

    #[test]
    fn break_with_seconds_parses_correctly() {
        let segs = parse(r#"<speak>A <break time="2s"/> B</speak>"#).unwrap();
        let break_durs: Vec<Duration> = segs
            .iter()
            .filter_map(|s| match s {
                Segment::Break(d) => Some(*d),
                _ => None,
            })
            .collect();
        assert_eq!(break_durs, vec![Duration::from_secs(2)]);
    }

    #[test]
    fn break_without_time_uses_default() {
        let segs = parse(r#"<speak>A <break/> B</speak>"#).unwrap();
        let has_default = segs
            .iter()
            .any(|s| matches!(s, Segment::Break(d) if *d == DEFAULT_BREAK));
        assert!(has_default, "expected default break, got {segs:?}");
    }

    #[test]
    fn unknown_tag_is_stripped_with_warning() {
        // <prosody> is not supported — should warn + strip, preserve text.
        let segs = parse(r#"<speak>Hi <prosody rate="fast">there</prosody></speak>"#).unwrap();
        let all_text: String = segs
            .iter()
            .filter_map(|s| match s {
                Segment::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert!(all_text.contains("Hi"));
        assert!(all_text.contains("there"));
    }

    #[test]
    fn input_without_speak_root_errors() {
        let err = parse("not xml").unwrap_err();
        assert!(err.to_string().contains("<speak>"), "msg: {err}");
    }

    #[test]
    fn empty_input_errors() {
        assert!(parse("").unwrap_err().to_string().contains("empty"));
        assert!(parse("   \n  ").unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn doctype_is_rejected() {
        let input = r#"<!DOCTYPE speak SYSTEM "foo"><speak>Hi</speak>"#;
        // DOCTYPE before <speak> → fails the <speak> root check first (still rejected)
        assert!(parse(input).is_err());
    }

    #[test]
    fn doctype_inside_speak_is_rejected() {
        let input = "<speak><!DOCTYPE foo>Hi</speak>";
        let err = parse(input).unwrap_err();
        assert!(err.to_string().contains("DOCTYPE"), "msg: {err}");
    }

    #[test]
    fn malformed_break_attribute_errors() {
        // Invalid time designation (not "Ns" or "Nms") → upstream parser rejects.
        let input = r#"<speak><break time="abc"/></speak>"#;
        assert!(parse(input).is_err());
    }

    #[test]
    fn doctype_deep_in_document_is_rejected() {
        // DOCTYPE past a 256-char prefix — earlier implementation had a scan window.
        let filler = "a ".repeat(400);
        let input = format!("<speak>{filler}<!DOCTYPE evil>tail</speak>");
        let err = parse(&input).unwrap_err();
        assert!(err.to_string().contains("DOCTYPE"), "msg: {err}");
    }

    #[test]
    fn say_as_tag_warning_uses_hyphenated_name() {
        // Regression: earlier Debug-based tag_name() produced `sayas`.
        use ssml_parser::elements::SayAsAttributes;
        let el = ParsedElement::SayAs(SayAsAttributes {
            interpret_as: "characters".to_string(),
            format: None,
            detail: None,
        });
        assert_eq!(tag_name(&el), "say-as");
    }

    #[test]
    fn phoneme_with_ipa_alphabet_emits_ipa_segment_and_suppresses_inner_text() {
        let segs = parse(
            r#"<speak>He said <phoneme alphabet="ipa" ph="nuˈmoʊniə">pneumonia</phoneme>.</speak>"#,
        )
        .unwrap();
        let ipas: Vec<&str> = segs
            .iter()
            .filter_map(|s| match s {
                Segment::Ipa(p) => Some(p.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(ipas, vec!["nuˈmoʊniə"]);
        // The inner "pneumonia" text must NOT leak into a Text segment —
        // that would double-speak the word (Kokoro would G2P it too).
        let all_text: String = segs
            .iter()
            .filter_map(|s| match s {
                Segment::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("|");
        assert!(
            !all_text.contains("pneumonia"),
            "inner text leaked: {all_text:?}"
        );
        assert!(
            all_text.contains("He said"),
            "outer text missing: {all_text:?}"
        );
    }

    #[test]
    fn phoneme_without_alphabet_defaults_to_ipa() {
        let segs = parse(r#"<speak><phoneme ph="həˈloʊ">hello</phoneme></speak>"#).unwrap();
        assert!(segs
            .iter()
            .any(|s| matches!(s, Segment::Ipa(p) if p == "həˈloʊ")));
    }

    #[test]
    fn phoneme_with_non_ipa_alphabet_falls_back_to_text() {
        let segs =
            parse(r#"<speak><phoneme alphabet="x-sampa" ph="h@_'low">hello</phoneme></speak>"#)
                .unwrap();
        // Non-IPA warn-strips: inner text flows as a Text segment so the
        // content still gets synthesized via G2P rather than dropped.
        assert!(segs.iter().all(|s| !matches!(s, Segment::Ipa(_))));
        assert!(segs
            .iter()
            .any(|s| matches!(s, Segment::Text(t) if t.contains("hello"))));
    }

    #[test]
    fn phoneme_with_empty_ph_is_dropped_silently() {
        let segs = parse(r#"<speak>pre <phoneme ph="">hello</phoneme> post</speak>"#).unwrap();
        assert!(segs.iter().all(|s| !matches!(s, Segment::Ipa(_))));
        let all_text: String = segs
            .iter()
            .filter_map(|s| match s {
                Segment::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("|");
        assert!(
            !all_text.contains("hello"),
            "inner text leaked when ph is empty: {all_text:?}"
        );
    }

    #[test]
    fn multiple_breaks_produce_multiple_silence_segments() {
        let segs =
            parse(r#"<speak>A <break time="100ms"/> B <break time="200ms"/> C</speak>"#).unwrap();
        let break_ms: Vec<u64> = segs
            .iter()
            .filter_map(|s| match s {
                Segment::Break(d) => Some(d.as_millis() as u64),
                _ => None,
            })
            .collect();
        // ssml-parser converts via f32 → Duration, so allow ±1ms slop per break.
        assert_eq!(break_ms.len(), 2, "got {break_ms:?}");
        assert!(
            (99..=101).contains(&break_ms[0]) && (199..=201).contains(&break_ms[1]),
            "breaks out of tolerance: {break_ms:?}"
        );
    }

    #[test]
    fn say_as_characters_emits_spell_segment() {
        let segs =
            parse(r#"<speak><say-as interpret-as="characters">ВОЗ</say-as></speak>"#).unwrap();
        let spell_chunks: Vec<&str> = segs
            .iter()
            .filter_map(|s| match s {
                Segment::Spell(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(spell_chunks, vec!["ВОЗ"]);
        // No stray text segments either side.
        let text_chunks = segs
            .iter()
            .filter(|s| matches!(s, Segment::Text(t) if !t.trim().is_empty()))
            .count();
        assert_eq!(text_chunks, 0);
    }

    #[test]
    fn say_as_cardinal_continues_warn_strip() {
        // interpret-as="cardinal" is not in scope for #232; keep the current
        // warn + strip behavior so the inner text is still synthesized.
        let segs = parse(r#"<speak><say-as interpret-as="cardinal">123</say-as></speak>"#).unwrap();
        assert!(matches!(segs.first(), Some(Segment::Text(t)) if t.contains("123")));
        assert!(!segs.iter().any(|s| matches!(s, Segment::Spell(_))));
    }

    #[test]
    fn say_as_without_interpret_as_continues_warn_strip() {
        // ssml-parser 0.1.4 treats `interpret-as` as a required attribute and returns
        // an Err when it is absent. Either an Err OR a successful parse without a Spell
        // segment is acceptable — no Spell must be emitted in the absent-attribute path.
        match parse(r#"<speak><say-as>literal</say-as></speak>"#) {
            Err(_) => {} // upstream parser rejects the malformed tag — acceptable
            Ok(segs) => {
                assert!(!segs.iter().any(|s| matches!(s, Segment::Spell(_))));
            }
        }
    }

    #[test]
    fn emphasis_default_level_emits_unsuppressed_segment() {
        let segs = parse(r#"<speak><emphasis>д+ома</emphasis></speak>"#).unwrap();
        let emphases: Vec<(&str, bool)> = segs
            .iter()
            .filter_map(|s| match s {
                Segment::Emphasis { content, suppress } => Some((content.as_str(), *suppress)),
                _ => None,
            })
            .collect();
        assert_eq!(emphases, vec![("д+ома", false)]);
        let text_chunks = segs
            .iter()
            .filter(|s| matches!(s, Segment::Text(t) if !t.trim().is_empty()))
            .count();
        assert_eq!(text_chunks, 0);
    }

    #[test]
    fn emphasis_level_none_sets_suppress_true() {
        let segs = parse(r#"<speak><emphasis level="none">д+ома</emphasis></speak>"#).unwrap();
        assert!(matches!(
            segs.first(),
            Some(Segment::Emphasis { content, suppress: true }) if content == "д+ома"
        ));
    }

    #[test]
    fn emphasis_level_strong_keeps_suppress_false() {
        let segs = parse(r#"<speak><emphasis level="strong">д+ома</emphasis></speak>"#).unwrap();
        assert!(matches!(
            segs.first(),
            Some(Segment::Emphasis {
                suppress: false,
                ..
            })
        ));
    }

    #[test]
    fn emphasis_level_reduced_keeps_suppress_false() {
        let segs = parse(r#"<speak><emphasis level="reduced">тест</emphasis></speak>"#).unwrap();
        assert!(matches!(
            segs.first(),
            Some(Segment::Emphasis {
                suppress: false,
                ..
            })
        ));
    }

    #[test]
    fn empty_emphasis_emits_no_segment() {
        let segs = parse(r#"<speak><emphasis></emphasis></speak>"#).unwrap();
        assert!(!segs.iter().any(|s| matches!(s, Segment::Emphasis { .. })));
    }

    #[test]
    fn emphasis_wrapping_say_as_does_not_double_emit() {
        // <emphasis><say-as interpret-as="characters">ВОЗ</say-as></emphasis>
        // — spec says "inner say-as wins". The parser should produce a single
        // segment for the inner content; the outer <emphasis> must NOT also
        // emit an Emphasis segment carrying the same text (would synth twice).
        let segs = parse(
            r#"<speak><emphasis><say-as interpret-as="characters">ВОЗ</say-as></emphasis></speak>"#,
        )
        .unwrap();

        let emphasis_count = segs
            .iter()
            .filter(|s| matches!(s, Segment::Emphasis { .. }))
            .count();
        let spell_count = segs
            .iter()
            .filter(|s| matches!(s, Segment::Spell(_)))
            .count();

        assert_eq!(
            spell_count, 1,
            "exactly one Spell segment expected, got: {segs:?}"
        );
        assert_eq!(
            emphasis_count, 0,
            "no Emphasis segment expected — inner say-as consumed the span, got: {segs:?}",
        );
    }

    #[test]
    fn emphasis_wrapping_phoneme_does_not_double_emit() {
        // Defensive: same nesting principle for <phoneme> inside <emphasis>.
        let segs = parse(
            r#"<speak><emphasis><phoneme alphabet="ipa" ph="dʌm">дом</phoneme></emphasis></speak>"#,
        )
        .unwrap();

        let emphasis_count = segs
            .iter()
            .filter(|s| matches!(s, Segment::Emphasis { .. }))
            .count();
        let ipa_count = segs.iter().filter(|s| matches!(s, Segment::Ipa(_))).count();

        assert_eq!(
            ipa_count, 1,
            "exactly one Ipa segment expected, got: {segs:?}"
        );
        assert_eq!(
            emphasis_count, 0,
            "no Emphasis when <phoneme> nested inside, got: {segs:?}"
        );
    }

    #[test]
    fn prosody_whole_utterance_emits_prosody_rate() {
        let segs = parse(r#"<speak><prosody rate="fast">Hello</prosody></speak>"#).unwrap();
        assert_eq!(segs.len(), 1);
        match &segs[0] {
            Segment::ProsodyRate { rate, content } => {
                assert!((*rate - 1.25).abs() < 1e-6, "expected 1.25, got {rate}");
                assert_eq!(content.len(), 1);
                assert!(matches!(&content[0], Segment::Text(t) if t.contains("Hello")));
            }
            other => panic!("expected ProsodyRate, got {other:?}"),
        }
    }

    #[test]
    fn prosody_mid_utterance_warns_and_flattens() {
        // Surrounding text outside the prosody → not whole-utterance.
        let segs = parse(r#"<speak>Hi <prosody rate="fast">there</prosody> bye</speak>"#).unwrap();
        assert!(!segs
            .iter()
            .any(|s| matches!(s, Segment::ProsodyRate { .. })));
        let combined: String = segs
            .iter()
            .filter_map(|s| {
                if let Segment::Text(t) = s {
                    Some(t.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            combined.contains("Hi"),
            "missing 'Hi' in flattened text: {combined}"
        );
        assert!(
            combined.contains("there"),
            "missing 'there' in flattened text: {combined}"
        );
        assert!(
            combined.contains("bye"),
            "missing 'bye' in flattened text: {combined}"
        );
    }

    #[test]
    fn prosody_whole_utterance_named_values() {
        for (name, expected) in [
            ("x-slow", 0.5_f32),
            ("slow", 0.75),
            ("medium", 1.0),
            ("fast", 1.25),
            ("x-fast", 1.5),
        ] {
            let xml = format!(r#"<speak><prosody rate="{name}">Hi</prosody></speak>"#);
            let segs = parse(&xml).unwrap();
            match &segs[0] {
                Segment::ProsodyRate { rate, .. } => {
                    assert!(
                        (*rate - expected).abs() < 1e-6,
                        "rate={name}: got {rate}, expected {expected}"
                    );
                }
                other => panic!("rate={name}: expected ProsodyRate, got {other:?}"),
            }
        }
    }

    #[test]
    fn prosody_with_sibling_break_is_not_whole_utterance() {
        // Leading <break/> outside the prosody is a structural sibling that
        // doesn't appear in the linearised text — without the sibling-span
        // check, is_whole_utterance would pass and the break would be
        // silently absorbed (parse_inner_spans filters it out as
        // out-of-range). The break must survive as its own segment and the
        // prosody must fall through to mid-utterance warn+strip.
        let segs =
            parse(r#"<speak><break time="500ms"/><prosody rate="fast">Hi</prosody></speak>"#)
                .unwrap();
        assert!(
            !segs
                .iter()
                .any(|s| matches!(s, Segment::ProsodyRate { .. })),
            "expected mid-utterance warn+strip, got: {segs:?}"
        );
        let has_break = segs
            .iter()
            .any(|s| matches!(s, Segment::Break(d) if *d == Duration::from_millis(500)));
        assert!(has_break, "leading <break/> dropped: {segs:?}");
        let has_text = segs
            .iter()
            .any(|s| matches!(s, Segment::Text(t) if t.contains("Hi")));
        assert!(has_text, "prosody content lost: {segs:?}");
    }

    #[test]
    fn prosody_relative_percent_is_rejected_at_input_scan() {
        // ssml-parser 0.1.4 silently strips the `+` from `+25%` during parse;
        // our `parse()` rejects relative percent at input pre-scan to avoid
        // emitting 0.25× audio for a user that asked for 1.25×.
        let err = parse(r#"<speak><prosody rate="+25%">Hi</prosody></speak>"#).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("relative percentage") && msg.contains("+25%"),
            "expected clear relative-percent message, got: {msg}"
        );

        // Negative form: upstream would bail with "Negative percentage not
        // allowed for rate"; our pre-scan catches it first with a clearer
        // user-facing message.
        let err = parse(r#"<speak><prosody rate="-25%">Hi</prosody></speak>"#).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("relative percentage") && msg.contains("-25%"),
            "expected clear relative-percent message, got: {msg}"
        );

        // Absolute percent is unaffected.
        assert!(parse(r#"<speak><prosody rate="125%">Hi</prosody></speak>"#).is_ok());
    }

    #[test]
    fn prosody_zero_percent_warn_strips_via_malformed_path() {
        // `<prosody rate="0%">` is finite + non-negative for ssml-parser, so
        // it reaches `parse_rate_value` which now rejects (mult <= 0). The
        // span is whole-utterance but the rate doesn't parse → warn+strip,
        // text falls through at the engine default rate.
        let segs = parse(r#"<speak><prosody rate="0%">Hi</prosody></speak>"#).unwrap();
        assert!(
            !segs
                .iter()
                .any(|s| matches!(s, Segment::ProsodyRate { .. })),
            "0% should warn+strip, not emit ProsodyRate; got: {segs:?}"
        );
        assert!(segs
            .iter()
            .any(|s| matches!(s, Segment::Text(t) if t.contains("Hi"))));
    }

    #[test]
    fn prosody_default_keyword_emits_unit_rate() {
        // SSML 1.1 `rate="default"` maps to 1.0×; the no-op rate is still a
        // valid request and should produce a ProsodyRate segment so the
        // engine speed is left untouched and the inner content is honored.
        let segs = parse(r#"<speak><prosody rate="default">Hi</prosody></speak>"#).unwrap();
        let rate = segs.iter().find_map(|s| match s {
            Segment::ProsodyRate { rate, .. } => Some(*rate),
            _ => None,
        });
        assert_eq!(rate, Some(1.0), "expected ProsodyRate(1.0); got {segs:?}");
    }

    #[test]
    fn nested_prosody_emits_warning_and_drops_inner_attributes() {
        // Inner <prosody> is silently dropped today; the warning surfaces the
        // behavior so users notice that nested rates don't compose.
        let segs = parse(
            r#"<speak><prosody rate="slow"><prosody rate="fast">Hi</prosody></prosody></speak>"#,
        )
        .unwrap();
        // Outer prosody still emits ProsodyRate; inner is flattened to text.
        let prosody = segs.iter().find_map(|s| match s {
            Segment::ProsodyRate { rate, content } => Some((rate, content)),
            _ => None,
        });
        let (rate, content) = prosody.expect("expected outer ProsodyRate");
        assert!((rate - 0.75).abs() < 1e-6, "outer rate wrong: got {rate}");
        let inner_text: String = content
            .iter()
            .filter_map(|s| match s {
                Segment::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("|");
        assert!(inner_text.contains("Hi"), "inner text lost: {inner_text}");
    }
}
