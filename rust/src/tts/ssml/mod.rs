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

use ssml_parser::elements::{EmphasisLevel, ParsedElement, PhonemeAlphabet};
use ssml_parser::parse_ssml;

pub use segment::Segment;

use super::warn::warn_once;
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
                    warn_once(
                        &format!("phoneme[alphabet={alpha}]"),
                        &format!(
                            "SSML <phoneme alphabet=\"{alpha}\"> not supported — only \"ipa\" is recognised; falling back to G2P on contained text"
                        ),
                    );
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
                    warn_once(
                        &format!("say-as[interpret-as={}]", attrs.interpret_as),
                        &format!(
                            "SSML <say-as interpret-as=\"{}\"> is not supported — only \"characters\" is recognised; falling back to plain text",
                            attrs.interpret_as
                        ),
                    );
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
                        let inner_segs = parse_inner_spans(&spans, &text, span.start, span.end);
                        segments.push(Segment::ProsodyRate {
                            rate,
                            content: inner_segs,
                        });
                        cursor = span.end;
                    } else {
                        // Whole-utterance but unparseable rate attribute — warn+strip.
                        warn_once(
                            WARN_PROSODY_NO_SUPPORTED_ATTR,
                            "SSML <prosody> without a parseable rate= attribute \
                             is not supported (pitch/volume scoped to a follow-up); stripping",
                        );
                        // Leave cursor unchanged; inner text flows through as Text.
                    }
                } else {
                    // Mid-utterance prosody — warn+strip.
                    warn_once(
                        WARN_PROSODY_MID_UTTERANCE,
                        "SSML <prosody> mid-utterance is not yet supported \
                         (whole-utterance only); stripping rate, pitch, and volume",
                    );
                    // Leave cursor unchanged; inner text falls through as Text.
                }
            }
            other => {
                let name = tag_name(other);
                warn_once(
                    &format!("unknown-tag-{name}"),
                    &format!("SSML tag <{name}> is not supported — stripping"),
                );
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

    // Integration scenarios for the public `parse()` API (full <speak>…</speak>
    // blobs → `Vec<Segment>` assertions) live in `rust/tests/ssml_integration.rs`
    // post-#267 F8. This in-crate test block keeps only unit tests for items
    // that are `pub(super)` and therefore unreachable from an external
    // integration test — currently just `tag_name`.

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
}
