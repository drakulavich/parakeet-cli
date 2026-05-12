//! Walker over `ssml-parser` spans that builds segment lists. Lives in its own
//! module so the top-level `parse()` and the recursive whole-utterance
//! `<prosody>` body share the same emit/skip logic without duplication.

use std::collections::HashSet;

use ssml_parser::elements::{EmphasisLevel, ParsedElement, PhonemeAlphabet};

use super::segment::{Segment, DEFAULT_BREAK};
use super::warnings::WARN_PROSODY_NESTED;
use super::{extract_inner_text, tag_name};

/// Sort key for span ordering: lower priority runs FIRST in the segment
/// loop. When two spans share `span.start` (e.g. an inner `<say-as>`
/// nested inside `<emphasis>`), the lower-priority arm runs first and
/// advances the cursor; the higher-priority arm is then skipped by the
/// loop-top `cursor` guard. This implements the spec's "inner structural
/// tag wins" rule for nested SSML.
///
/// Priority assignments (#233):
/// - 0: structural-leaf tags (Phoneme, SayAs) — run first, consume span
/// - 1: Break and other non-overlapping containers
/// - 2: Emphasis — run after inner leaves; otherwise wraps them
/// - 3: Speak root wrapper
pub(super) fn span_priority(el: &ParsedElement) -> u8 {
    match el {
        ParsedElement::Phoneme(_) | ParsedElement::SayAs(_) => 0,
        ParsedElement::Break(_) => 1,
        ParsedElement::Emphasis(_) | ParsedElement::Prosody(_) => 2,
        ParsedElement::Speak(_) => 3,
        _ => 1,
    }
}

pub(super) fn push_text_slice(out: &mut Vec<Segment>, text: &[char], start: usize, end: usize) {
    if start >= end {
        return;
    }
    let chunk: String = text[start..end].iter().collect();
    if !chunk.trim().is_empty() {
        out.push(Segment::Text(chunk));
    }
}

/// Parse the inner content of a whole-utterance `<prosody>` span into segments.
/// Iterates the sub-spans whose character range falls strictly within
/// `[prosody_start, prosody_end)`, applying the same rules as the top-level
/// walker (Break, Phoneme, SayAs, Emphasis). Unknown tags warn+strip. The
/// outer `warned` set is shared so each warning fires at most once per
/// document regardless of nesting.
pub(super) fn parse_inner_spans(
    all_spans: &[&ssml_parser::parser::Span],
    text: &[char],
    prosody_start: usize,
    prosody_end: usize,
    warned: &mut HashSet<String>,
) -> Vec<Segment> {
    let mut segments: Vec<Segment> = Vec::new();
    let mut cursor = prosody_start;

    // Filter to spans that are strictly children of the prosody span
    // (start >= prosody_start, end <= prosody_end) and are not the prosody
    // span itself (we skip the Prosody element and the Speak wrapper).
    for span in all_spans {
        if span.start < prosody_start || span.end > prosody_end {
            continue;
        }
        // Skip the outer prosody span itself — `all_spans` includes it
        // because its `(start, end)` matches the prosody range. Without this
        // guard, the Prosody match arm below would fire `prosody-nested`
        // for every whole-utterance prosody.
        if span.start == prosody_start && span.end == prosody_end {
            if let ParsedElement::Prosody(_) = &span.element {
                continue;
            }
        }
        if span.start < cursor {
            continue;
        }
        match &span.element {
            ParsedElement::Speak(_) => {
                // Skip the speak wrapper — we're already inside it.
            }
            ParsedElement::Prosody(_) => {
                // Nested <prosody> inside another <prosody>: not supported in v1.
                // Inner attributes are dropped; inner content flows at the outer
                // rate via the trailing push_text_slice plus any leaf spans below.
                if warned.insert(WARN_PROSODY_NESTED.to_string()) {
                    eprintln!(
                        "warning: SSML <prosody> nested inside another <prosody> is not \
                         supported; inner rate/pitch/volume attributes ignored"
                    );
                }
            }
            ParsedElement::Break(attrs) => {
                push_text_slice(&mut segments, text, cursor, span.start);
                let dur = attrs
                    .time
                    .as_ref()
                    .map(|t| t.duration())
                    .unwrap_or(DEFAULT_BREAK);
                segments.push(Segment::Break(dur));
                cursor = span.end;
            }
            ParsedElement::Phoneme(attrs) => {
                let is_ipa = matches!(&attrs.alphabet, None | Some(PhonemeAlphabet::Ipa));
                if is_ipa {
                    push_text_slice(&mut segments, text, cursor, span.start);
                    if !attrs.ph.is_empty() {
                        segments.push(Segment::Ipa(attrs.ph.clone()));
                    }
                    cursor = span.end;
                } else {
                    let alpha = match &attrs.alphabet {
                        Some(PhonemeAlphabet::Other(s)) => s.clone(),
                        other => format!("{other:?}"),
                    };
                    if warned.insert(format!("phoneme[alphabet={alpha}]")) {
                        eprintln!(
                            "warning: SSML <phoneme alphabet=\"{alpha}\"> not supported — \
                             only \"ipa\" is recognised; falling back to G2P on contained text"
                        );
                    }
                }
            }
            ParsedElement::SayAs(attrs) => {
                if attrs.interpret_as == "characters" {
                    push_text_slice(&mut segments, text, cursor, span.start);
                    if let Some(inner) = extract_inner_text(text, span.start, span.end) {
                        segments.push(Segment::Spell(inner));
                    }
                    cursor = span.end;
                } else {
                    let key = format!("say-as[interpret-as={}]", attrs.interpret_as);
                    if warned.insert(key) {
                        eprintln!(
                            "warning: SSML <say-as interpret-as=\"{}\"> is not supported — \
                             only \"characters\" is recognised; falling back to plain text",
                            attrs.interpret_as
                        );
                    }
                }
            }
            ParsedElement::Emphasis(attrs) => {
                push_text_slice(&mut segments, text, cursor, span.start);
                if let Some(content) = extract_inner_text(text, span.start, span.end) {
                    let suppress = matches!(attrs.level, Some(EmphasisLevel::None));
                    segments.push(Segment::Emphasis { content, suppress });
                }
                cursor = span.end;
            }
            other => {
                let name = tag_name(other);
                if warned.insert(name.clone()) {
                    eprintln!("warning: SSML tag <{name}> is not supported — stripping");
                }
            }
        }
    }
    // Trailing text inside the prosody span.
    push_text_slice(&mut segments, text, cursor, prosody_end);
    segments
}
