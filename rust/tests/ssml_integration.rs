//! End-to-end scenarios for `tts::ssml::parse` — full `<speak>…</speak>` strings
//! parsed into `Vec<Segment>` and asserted on shape. Moved out of
//! `tts/ssml/mod.rs` per #267 F8 so the production module is not 70% test code.
//!
//! Unit-level tests for individual helpers stay colocated with their helpers
//! (e.g. `parse_rate_value` in `ssml/rate.rs`, segment-trim invariants in
//! `ssml/segment.rs`, the `tag_name(ParsedElement)` regression next to its
//! `pub(super)` definition in `ssml/mod.rs`).

#![cfg(feature = "tts")]

use std::time::Duration;

use kesha_engine::tts::ssml::{parse, Segment};

// Mirrors `tts::ssml::segment::DEFAULT_BREAK` (pub(super)). Hardcoded here so
// the integration test treats it as part of the external contract — if the
// default changes upstream, this assertion is the alarm.
const DEFAULT_BREAK: Duration = Duration::from_millis(250);

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
    let segs = parse(r#"<speak><phoneme alphabet="x-sampa" ph="h@_'low">hello</phoneme></speak>"#)
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
    let segs = parse(r#"<speak><say-as interpret-as="characters">ВОЗ</say-as></speak>"#).unwrap();
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
        parse(r#"<speak><break time="500ms"/><prosody rate="fast">Hi</prosody></speak>"#).unwrap();
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
    let segs =
        parse(r#"<speak><prosody rate="slow"><prosody rate="fast">Hi</prosody></prosody></speak>"#)
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
