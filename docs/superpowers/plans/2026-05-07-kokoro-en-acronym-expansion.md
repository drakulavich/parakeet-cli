# English acronym auto-expansion for Kokoro — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** For Kokoro voices (`en-*`), auto-expand all-uppercase Latin acronyms (length 2–5) letter-by-letter using a 26-entry US-canonical letter-name table, gated by the existing `--no-expand-abbrev` flag. Honor `<say-as interpret-as="characters">` regardless of the flag.

**Architecture:** New `rust/src/tts/en/` module mirroring the shipped `rust/src/tts/ru/` layout (3 files: `mod.rs`, `acronym.rs`, `letter_table.rs`). Plain-text path: `tts::say()` calls `en::expand_text` before G2P when `lang.starts_with("en")` and `opts.expand_abbrev`. SSML path: `synth_segments_kokoro` (private) calls `en::normalize_segments` before `synth_segments_kokoro_with`, mirroring the `ru::normalize_segments` insertion point at `synth_segments_vosk` (line 355). `Emphasis` is normalized into `Text` upstream (warn-once + `+` strip preserved); the existing arm in `synth_segments_kokoro_with` becomes a defensive fallback matching the Vosk pattern. New capability flag `tts.en_acronym_expansion`.

**Tech Stack:** Rust 2024, no new crate dependencies. TS-side: extend the capability gate that today only checks `tts.ru_acronym_expansion`. Engine release v1.10.0 (v1.9.0 reserved for #247).

**Spec:** `docs/superpowers/specs/2026-05-07-kokoro-en-acronym-expansion-design.md` (commit `205cfb3`).

---

## File Structure

| Path | Status | Responsibility |
|---|---|---|
| `rust/src/tts/en/mod.rs` | NEW | Public `expand_text(&str) -> String` (plain path) and `normalize_segments(Vec<Segment>, bool) -> Vec<Segment>` (SSML path). Routes `Spell` through `letter_table::expand_chars`; `Text` through `acronym::expand_acronyms` when `auto_expand`; `Emphasis` warn-once-on-non-ru-vosk + `+`-strip → `Text(content)`. |
| `rust/src/tts/en/acronym.rs` | NEW | `STOP_LIST: &[&str]` (30 entries), `is_acronym_token`, `expand_acronyms`, `split_punct`, `expand_token`. Ports the Russian module's structure with three differences: `[A-Z]`-only test (vs `[А-ЯЁ]`), no Ъ/Ь rejection, no same-type-pair gate (every length-2..=5 all-caps token spells unless on stop-list). |
| `rust/src/tts/en/letter_table.rs` | NEW | `LETTERS: &[(char, &str); 26]` (US-canonical), `expand_chars(&str) -> String`. No position-dependence (unlike Russian's С), no `phrase_override` table. |
| `rust/src/tts/mod.rs` | MODIFY | (a) `say()` plain Kokoro path (line ~163): insert `let normalized = if opts.expand_abbrev && opts.lang.starts_with("en") { Cow::Owned(en::expand_text(opts.text)) } else { Cow::Borrowed(opts.text) };` then pass `&normalized` to `g2p::text_to_ipa`. (b) `synth_segments_kokoro` (line 219–230): take `expand_abbrev: bool` parameter, call `en::normalize_segments(segments, expand_abbrev)` before `synth_segments_kokoro_with`. (c) `say_ssml` (line 187–217): thread `opts.expand_abbrev` into the new `synth_segments_kokoro` parameter. (d) `synth_segments_kokoro_with` Emphasis arm (line 264–289): rewrite as defensive fallback — comment "Defensive fallback: en::normalize_segments converts Emphasis→Text upstream", strip `+`, warn-once. |
| `rust/src/tts/mod.rs` | MODIFY | Add `pub(super) mod en;` declaration alongside `pub(super) mod ru;`. |
| `rust/src/capabilities.rs` | MODIFY | Append `features.push("tts.en_acronym_expansion");` under the existing `#[cfg(feature = "tts")]` block, immediately after the `tts.ru_acronym_expansion` push. |
| `rust/tests/tts_en_normalize.rs` | NEW | Integration test using the `LoopEngine` `--stdin-loop` harness pattern (mirror of `tts_ru_normalize.rs`): plain spell, stop-list pass-through, `<say-as>` SSML, `--no-expand-abbrev` round-trip. Asserts byte-length deltas vs baseline. |
| `src/cli/say.ts` | MODIFY | Update the help-text description for `--no-expand-abbrev` (line 89): "applies to Russian (ru-vosk-*) and English (en-*) voices." Extend the capability-gated forwarding check (line ~157 area) to OR `tts.en_acronym_expansion`. |
| `README.md` | MODIFY | Add an English acronym example block alongside the existing TTS examples. |
| `SKILL.md` | MODIFY | One paragraph + example mirroring the Russian acronym entry shipped in #232. |
| `package.json` | MODIFY | Lockstep bump `version` and `keshaEngine.version` to `1.10.0`. |
| `rust/Cargo.toml` | MODIFY | Bump `version` to `1.10.0`. |
| `rust/Cargo.lock` | MODIFY | `cargo check --no-default-features --features onnx,tts` updates the lockfile entry to `1.10.0`. |

> **Spec amendment captured here:** the spec's SSML interaction table claims `Emphasis` is "stripped, content passed through verbatim (no warn-once)". For symmetry with the Russian path (and to preserve the v1.8.1 #237/#238 warn-once UX), `en::normalize_segments` instead handles `Emphasis` exactly like `ru::normalize_segments` — strip `+`, warn-once on missing marker, emit `Text(content)`. The existing arm in `synth_segments_kokoro_with` becomes a defensive fallback. This is recorded in the spec via a follow-up edit in Task 3, Step 3.0.

---

## Pre-flight

Verify the working tree before starting:

- [ ] **Step 0.1: Confirm branch + spec**

Run:
```bash
cd /Users/anton/Personal/repos/kesha-voice-kit
git rev-parse --abbrev-ref HEAD
git log -1 --oneline -- docs/superpowers/specs/2026-05-07-kokoro-en-acronym-expansion-design.md
```
Expected: branch `feat/244-kokoro-en-acronym`; spec commit `205cfb3` reachable.

- [ ] **Step 0.2: Confirm baseline tests + clippy + fmt are green**

Run:
```bash
cd rust && cargo test --no-default-features --features onnx,tts --lib 2>&1 | tail -3
cargo clippy --all-targets --no-default-features --features onnx,tts -- -D warnings 2>&1 | tail -3
cargo fmt --check && echo fmt-clean
cd .. && bun test 2>&1 | tail -3
bunx tsc --noEmit && echo tsc-clean
```
Expected: rust tests pass, clippy clean, fmt clean, bun test pass, tsc clean. If anything fails, stop and investigate before adding new code on a broken base.

- [ ] **Step 0.3: Stage evidence directory**

Run:
```bash
mkdir -p /tmp/kesha-244-evidence
```
Used by Task 9 (audio quality check).

---

## Task 1: Letter-name table for English acronyms

Adds the smallest, most-isolated piece first. The 26-entry US-canonical alphabet table + `expand_chars` function. Tested in isolation; no integration with the rest of the engine yet.

**Files:**
- Create: `rust/src/tts/en/letter_table.rs`

- [ ] **Step 1.1: Create the file with `LETTERS` const + `expand_chars` + tests**

Write `rust/src/tts/en/letter_table.rs`:

```rust
//! US-canonical English letter-name table for spelling-out acronyms on the
//! Kokoro path. Joined with single spaces — misaki-rs handles letter-name
//! strings like "ay bee see" cleanly when fed plain text.
//!
//! No position-dependence (unlike Russian's С). No `phrase_override` table —
//! YAGNI per spec Q2 (closes #244, mirror of #232).

const LETTERS: &[(char, &str)] = &[
    ('a', "ay"),
    ('b', "bee"),
    ('c', "see"),
    ('d', "dee"),
    ('e', "ee"),
    ('f', "ef"),
    ('g', "jee"),
    ('h', "aitch"),
    ('i', "eye"),
    ('j', "jay"),
    ('k', "kay"),
    ('l', "el"),
    ('m', "em"),
    ('n', "en"),
    ('o', "oh"),
    ('p', "pee"),
    ('q', "kyu"),
    ('r', "ar"),
    ('s', "ess"),
    ('t', "tee"),
    ('u', "yoo"),
    ('v', "vee"),
    ('w', "double yoo"),
    ('x', "ex"),
    ('y', "why"),
    ('z', "zee"),
];

/// Expand `input` to space-separated English letter names. Non-`[a-zA-Z]`
/// characters pass through verbatim (defensive — the matcher upstream
/// filters them out, but `<say-as>` of mixed input must stay safe).
pub(super) fn expand_chars(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 4);
    let mut last_was_letter = false;
    for c in input.chars() {
        let lc = c.to_ascii_lowercase();
        let name = LETTERS.iter().find(|(k, _)| *k == lc).map(|(_, v)| *v);
        match name {
            Some(s) => {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(s);
                last_was_letter = true;
            }
            None => {
                if last_was_letter {
                    out.push(' ');
                }
                out.push(c);
                last_was_letter = false;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epam_expands_to_four_letter_names() {
        assert_eq!(expand_chars("EPAM"), "ee pee ay em");
    }

    #[test]
    fn fbi_expands_to_three_letter_names() {
        assert_eq!(expand_chars("FBI"), "ef bee eye");
    }

    #[test]
    fn http_uses_double_yoo_for_w_via_lowercase_mixin() {
        // Sanity: "double yoo" is one entry, not split. Spot-check W only here;
        // dedicated W test below.
        assert_eq!(expand_chars("HTTP"), "aitch tee tee pee");
    }

    #[test]
    fn w_renders_as_double_yoo() {
        assert_eq!(expand_chars("WWW"), "double yoo double yoo double yoo");
    }

    #[test]
    fn lowercase_input_works() {
        assert_eq!(expand_chars("epam"), "ee pee ay em");
    }

    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(expand_chars(""), "");
    }

    #[test]
    fn full_alphabet_round_trip() {
        let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let result = expand_chars(alphabet);
        // 26 audible tokens; "double yoo" counts as one (split by spaces gives 27 pieces).
        let pieces: Vec<&str> = result.split(' ').collect();
        assert_eq!(pieces.len(), 27, "got: {result}");
    }

    #[test]
    fn non_latin_passes_through() {
        // The matcher won't pass non-Latin to us; this is a sanity guard for
        // explicit <say-as> with mixed input.
        assert_eq!(expand_chars("AB1"), "ay bee 1");
    }

    #[test]
    fn pure_non_letter_passes_through_without_leading_space() {
        assert_eq!(expand_chars("---"), "---");
        assert_eq!(expand_chars("123"), "123");
    }
}
```

- [ ] **Step 1.2: Wire the new file in (still won't compile until parent module is created in Task 3)**

This file is `pub(super) mod letter_table` inside `rust/src/tts/en/mod.rs` (Task 3). For now it's an orphan.

- [ ] **Step 1.3: Skip-build until Task 3 lands the parent**

The file is unreachable yet. Move on; tests run in Task 3 after the parent compiles.

- [ ] **Step 1.4: Commit**

Run:
```bash
cd /Users/anton/Personal/repos/kesha-voice-kit
git add rust/src/tts/en/letter_table.rs
git commit -m "$(cat <<'EOF'
feat(#244,tts): add English letter-name table for acronym spell-out

US-canonical 26-entry table; no position-dependence (unlike Russian's С),
no phrase_override (YAGNI per spec Q2). Joined with single spaces — misaki
handles letter-name strings like "ay bee see" cleanly.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```
Expected: commit succeeds. Record SHA in evidence file:
```bash
git rev-parse HEAD > /tmp/kesha-244-evidence/T1-letter-table.sha
```

---

## Task 2: Acronym matcher (rule + STOP_LIST + split_punct)

The "is this a spell-able token?" decision and the "tokenize, peel punct, replace" loop. Uses `letter_table::expand_chars` from Task 1.

**Files:**
- Create: `rust/src/tts/en/acronym.rs`

- [ ] **Step 2.1: Create the file with rule + STOP_LIST + tests**

Write `rust/src/tts/en/acronym.rs`:

```rust
//! Auto-detect all-uppercase Latin acronyms in plain text and replace them
//! with letter-by-letter spellings via `letter_table::expand_chars`.
//!
//! Rule (see spec 2026-05-07 §"Spell rule" / Decisions Q1):
//! 1. Tokenize on Unicode whitespace, preserving the original spacing.
//! 2. Strip a leading run of `«("` (head) and a trailing run of
//!    `.,:;!?»)„"…—–-` (tail); the rest is `core`.
//! 3. `core` must be 2..=5 chars, all `[A-Z]`. Aggressive — every length-2..=5
//!    all-caps Latin token spells, unless on `STOP_LIST`. Closes #244.
//! 4. `core` must not be in `STOP_LIST` (covers emphatic short caps + natural-
//!    English caps words read as words).
//! 5. Otherwise, replace the token with `head + expand_chars(core) + tail`.

use std::borrow::Cow;

use super::letter_table::expand_chars;

/// 30-entry seed list. Half are emphatic length-2 caps (OK, IT, IS, …);
/// half are natural-English caps words read as words (NASA, NATO, AIDS, …).
/// Maintainer extends as users report mispronunciations — one-line edits.
const STOP_LIST: &[&str] = &[
    // Emphatic length-2 caps
    "OK", "NO", "GO", "IT", "IS", "AS", "AT", "BY", "IN", "ON", "OR", "OF", "TO", "WE", "US",
    "MY", "ME", "HE", "BE", "DO",
    // Natural-English caps words
    "NASA", "NATO", "AIDS", "OPEC", "IKEA", "ASCII", "NAFTA", "LASER", "RADAR", "SCUBA",
];

const TRAILING_PUNCT: &[char] = &[
    '.', ',', ':', ';', '!', '?', '»', ')', '„', '"', '…', '—', '–', '-',
];

const LEADING_PUNCT: &[char] = &['«', '(', '"', '„'];

/// Returns true if `core` is a candidate acronym worth expanding.
/// Pure structural check — does not consult the stop-list.
fn is_acronym_token(core: &str) -> bool {
    let len = core.chars().count();
    if !(2..=5).contains(&len) {
        return false;
    }
    core.chars().all(|c| c.is_ascii_uppercase())
}

/// Auto-expand all-uppercase Latin acronyms in `input`. Whitespace and
/// non-acronym tokens are preserved verbatim.
pub(super) fn expand_acronyms(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut buf = String::new();
    for c in input.chars() {
        if c.is_whitespace() {
            if !buf.is_empty() {
                out.push_str(expand_token(&buf).as_ref());
                buf.clear();
            }
            out.push(c);
        } else {
            buf.push(c);
        }
    }
    if !buf.is_empty() {
        out.push_str(expand_token(&buf).as_ref());
    }
    out
}

fn expand_token(token: &str) -> Cow<'_, str> {
    let (head, mid, tail) = split_punct(token);
    if !is_acronym_token(mid) {
        return Cow::Borrowed(token);
    }
    if STOP_LIST.contains(&mid) {
        return Cow::Borrowed(token);
    }
    let mut s = String::from(head);
    s.push_str(&expand_chars(mid));
    s.push_str(tail);
    Cow::Owned(s)
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

    fn cases() -> Vec<(&'static str, &'static str)> {
        vec![
            // Spell out — alternating CV (Russian rule would skip; English does not).
            ("EPAM", "ee pee ay em"),
            ("EPAM.", "ee pee ay em."),
            ("EPAM partners", "ee pee ay em partners"),
            // Spell out — 0 vowels.
            ("FBI", "ef bee eye"),
            ("SQL", "ess kyu el"),
            ("LLM", "el el em"),
            ("XML", "ex em el"),
            // Spell out — common length-3..5 acronyms.
            ("CEO", "see ee oh"),
            ("API", "ay pee eye"),
            ("HTTP", "aitch tee tee pee"),
            ("JSON", "jay ess oh en"),
            ("CSS", "see ess ess"),
            ("URL", "yoo ar el"),
            ("IBM", "eye bee em"),
            // Spell out — length 2.
            ("AI", "ay eye"),
            ("TV", "tee vee"),
            ("DC", "dee see"),
            // Stop-list pass-through.
            ("NASA", "NASA"),
            ("NATO", "NATO"),
            ("AIDS", "AIDS"),
            ("OPEC", "OPEC"),
            ("IKEA", "IKEA"),
            ("OK", "OK"),
            ("IT", "IT"),
            ("IS", "IS"),
            // Inflected / mixed case / digits / hyphens preserved.
            ("EPAMs", "EPAMs"),
            ("APIs", "APIs"),
            ("iPhone", "iPhone"),
            ("H2O", "H2O"),
            ("MP3", "MP3"),
            ("T-shirt", "T-shirt"),
            ("WiFi", "WiFi"),
            // Wrong shape preserved.
            ("hello", "hello"),
            ("A", "A"),
            ("ABCDEF", "ABCDEF"),
            // Punctuation around acronyms.
            ("«EPAM»", "«ee pee ay em»"),
            ("EPAM!", "ee pee ay em!"),
            ("CEO,", "see ee oh,"),
            ("FBI? CIA!", "ef bee eye? see eye ay!"),
            // Stop-list with punct preserved.
            ("NASA.", "NASA."),
            ("«NATO»", "«NATO»"),
            // Stop-list followed by other text — the stop-list entry must not consume neighbors.
            ("OK partners", "OK partners"),
            ("NASA briefed", "NASA briefed"),
        ]
    }

    #[test]
    fn matrix() {
        for (input, expected) in cases() {
            assert_eq!(expand_acronyms(input), expected, "input: {input:?}");
        }
    }

    #[test]
    fn every_stop_list_entry_round_trips() {
        for w in STOP_LIST {
            assert_eq!(expand_acronyms(w), *w, "stop-list entry escaped: {w}");
        }
    }

    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(expand_acronyms(""), "");
    }

    #[test]
    fn pure_whitespace_passes_through() {
        assert_eq!(expand_acronyms("   "), "   ");
        assert_eq!(expand_acronyms("\n"), "\n");
    }
}
```

- [ ] **Step 2.2: Skip-build until Task 3 lands the parent**

Same orphan situation — no `pub mod en::acronym;` declaration yet.

- [ ] **Step 2.3: Commit**

Run:
```bash
git add rust/src/tts/en/acronym.rs
git commit -m "$(cat <<'EOF'
feat(#244,tts): add English acronym matcher (rule + STOP_LIST)

Aggressive spell-by-default: every length-2..=5 all-caps Latin token spells
unless in STOP_LIST. 30-entry seed (emphatic shorts + natural-English caps
words). Mirror of rust/src/tts/ru/acronym.rs, simpler — no Cyrillic-soft-sign
rejection, no same-type-pair gate.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git rev-parse HEAD > /tmp/kesha-244-evidence/T2-acronym.sha
```

---

## Task 3: Module facade (`tts::en::mod`)

Now the parent compiles. Adds `expand_text` (plain path) and `normalize_segments` (SSML path) — the public surface that `tts::mod` calls. Includes `Emphasis` handling (warn-once + strip) for symmetry with `ru::normalize_segments` and to preserve the v1.8.1 #237/#238 UX.

**Files:**
- Create: `rust/src/tts/en/mod.rs`
- Modify: `rust/src/tts/mod.rs` (add `pub(super) mod en;` line)
- Modify: `rust/src/tts/en/letter_table.rs` (change `pub(super)` → confirmed already; lift `pub(super) fn expand_chars` if needed)
- Modify: `docs/superpowers/specs/2026-05-07-kokoro-en-acronym-expansion-design.md` (spec amendment for Emphasis handling)

- [ ] **Step 3.0: Amend the spec to reflect the Emphasis decision**

Open the spec and replace the SSML-interaction-table row for `<emphasis>` with the corrected behavior. The spec previously claimed "no warn-once"; we keep the v1.8.1 warn-once + `+`-strip in `en::normalize_segments` for symmetry with `ru::normalize_segments`.

Run:
```bash
sed -i '' 's|Kokoro has no stress mechanism. Tag stripped, content passed through verbatim (no warn-once — content is preserved, only the tag is unsupported).|Kokoro has no stress mechanism for `+`-marker stress placement. `en::normalize_segments` strips `+` from content, warns once per process (key: `emphasis-non-ru-vosk`), and emits `Text(content)`. Mirrors `ru::normalize_segments` Emphasis handling; preserves v1.8.1 (#237/#238) warn-once UX.|' \
  docs/superpowers/specs/2026-05-07-kokoro-en-acronym-expansion-design.md
```

Verify:
```bash
grep -n 'emphasis-non-ru-vosk' docs/superpowers/specs/2026-05-07-kokoro-en-acronym-expansion-design.md
```
Expected: one match in the SSML interaction table row.

- [ ] **Step 3.1: Create `rust/src/tts/en/mod.rs`**

```rust
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
        let out = normalize_segments(
            vec![Segment::Text("EPAM partners".to_string())],
            true,
        );
        assert_eq!(out, vec![Segment::Text("ee pee ay em partners".to_string())]);
    }

    #[test]
    fn text_passes_through_when_auto_expand_is_false() {
        let out = normalize_segments(
            vec![Segment::Text("EPAM partners".to_string())],
            false,
        );
        assert_eq!(out, vec![Segment::Text("EPAM partners".to_string())]);
    }

    #[test]
    fn spell_wins_even_when_auto_expand_is_false() {
        let out = normalize_segments(vec![Segment::Spell("OK".to_string())], false);
        assert_eq!(out, vec![Segment::Text("o kay".to_string())]);
        // OK letter-by-letter via expand_chars is "oh kay" — assert correct table.
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
```

> **Note on the `o kay` test in Step 3.1:** the assertion in `spell_wins_even_when_auto_expand_is_false` expects `"o kay"` — but `expand_chars("OK")` yields `"oh kay"` per the letter table (`O → "oh"`, `K → "kay"`). Fix the assertion to `"oh kay"` before running the tests below.

- [ ] **Step 3.2: Fix the test assertion**

```bash
sed -i '' 's|"o kay"|"oh kay"|' rust/src/tts/en/mod.rs
```

- [ ] **Step 3.3: Wire `pub(super) mod en;` in `rust/src/tts/mod.rs`**

Find the existing `pub(super) mod ru;` line:
```bash
grep -n 'pub(super) mod ru' rust/src/tts/mod.rs
```
Add `pub(super) mod en;` immediately after it. Use the Edit tool (matching one line is enough):

```rust
// Before:
pub(super) mod ru;

// After:
pub(super) mod ru;
pub(super) mod en;
```

- [ ] **Step 3.4: Run tests**

```bash
cd rust && cargo test --no-default-features --features onnx,tts --lib tts::en 2>&1 | tail -10
```
Expected: tests in `tts::en::letter_table::tests`, `tts::en::acronym::tests`, and `tts::en::tests` all pass.

- [ ] **Step 3.5: Run clippy + fmt**

```bash
cargo clippy --all-targets --no-default-features --features onnx,tts -- -D warnings 2>&1 | tail -5
cargo fmt --check && echo fmt-clean
```
Expected: clippy clean, fmt clean. Apply `cargo fmt` if not.

- [ ] **Step 3.6: Commit**

```bash
git add rust/src/tts/en/mod.rs rust/src/tts/mod.rs docs/superpowers/specs/2026-05-07-kokoro-en-acronym-expansion-design.md
git commit -m "$(cat <<'EOF'
feat(#244,tts): add tts::en module facade (expand_text + normalize_segments)

Mirrors tts::ru — Spell→Text via letter-table, Text→acronym-expand when
auto_expand, Emphasis→Text with `+`-strip + warn-once (preserves v1.8.1
#237/#238 UX). Spec amended to reflect Emphasis handling.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git rev-parse HEAD > /tmp/kesha-244-evidence/T3-mod.sha
```

---

## Task 4: Wire `en::expand_text` into the plain Kokoro path

The non-SSML branch of `tts::say()` currently sends raw text through `g2p::text_to_ipa` (line ~163 in `rust/src/tts/mod.rs`). Insert `en::expand_text` for English voices when `expand_abbrev` is true.

**Files:**
- Modify: `rust/src/tts/mod.rs` (line ~163 area)

- [ ] **Step 4.1: Locate the exact wiring site**

```bash
grep -n 'g2p::text_to_ipa(opts.text' rust/src/tts/mod.rs
```
Expected: one match around line 163.

- [ ] **Step 4.2: Apply the edit**

Use the Edit tool. Replace:

```rust
    let ipa = g2p::text_to_ipa(opts.text, opts.lang)
        .map_err(|e| TtsError::SynthesisFailed(format!("g2p: {e}")))?;
```

with:

```rust
    // Auto-expand English acronyms when the voice is en-* and the user
    // hasn't passed --no-expand-abbrev. <say-as> is honored on the SSML
    // path (synth_segments_kokoro) regardless. Closes #244 (mirror of #232).
    let normalized_text: Cow<'_, str> = if opts.expand_abbrev && opts.lang.starts_with("en") {
        Cow::Owned(en::expand_text(opts.text))
    } else {
        Cow::Borrowed(opts.text)
    };
    let ipa = g2p::text_to_ipa(normalized_text.as_ref(), opts.lang)
        .map_err(|e| TtsError::SynthesisFailed(format!("g2p: {e}")))?;
```

- [ ] **Step 4.3: Add the `use` import for `Cow` and `en` if not already present**

```bash
grep -n 'use std::borrow::Cow' rust/src/tts/mod.rs | head -1
grep -n 'use crate::tts::ru' rust/src/tts/mod.rs | head -1
```

`Cow` is likely already imported (the Vosk path uses it). `en` is reachable via the local `mod en;` — no extra `use` needed inside `tts/mod.rs`.

If `Cow` is not imported, add `use std::borrow::Cow;` near the top of the file alongside the other `use` statements.

- [ ] **Step 4.4: Run tests + clippy**

```bash
cd rust && cargo test --no-default-features --features onnx,tts --lib tts:: 2>&1 | tail -3
cargo clippy --all-targets --no-default-features --features onnx,tts -- -D warnings 2>&1 | tail -3
```
Expected: pass; the existing tests still hold (default `expand_abbrev=true`, English voices were already routing without expansion since `en::expand_text` didn't exist; the lookup of unknown caps tokens in misaki-rs returns the raw token unchanged, so adding the expansion only changes audio for true acronyms, not for fixture text).

- [ ] **Step 4.5: Commit**

```bash
git add rust/src/tts/mod.rs
git commit -m "$(cat <<'EOF'
feat(#244,tts): wire en::expand_text into Kokoro plain-text path

When voice is en-* and --no-expand-abbrev is not set, run text through
en::expand_text before G2P. Default behavior unchanged for non-English
voices and for users passing --no-expand-abbrev.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git rev-parse HEAD > /tmp/kesha-244-evidence/T4-plain-wiring.sha
```

---

## Task 5: Wire `en::normalize_segments` into the SSML Kokoro path

Mirrors the Vosk insertion at line 355: in `synth_segments_kokoro` (private wrapper), call `en::normalize_segments(segments, expand_abbrev)` before passing to `synth_segments_kokoro_with`. The existing `Emphasis` arm in `synth_segments_kokoro_with` becomes a defensive fallback (matches the Vosk pattern).

**Files:**
- Modify: `rust/src/tts/mod.rs` — `synth_segments_kokoro` signature + body, `say_ssml` call site, `synth_segments_kokoro_with` Emphasis arm comment.

- [ ] **Step 5.1: Update `synth_segments_kokoro` signature + body**

Use the Edit tool. Replace:

```rust
fn synth_segments_kokoro(
    segments: &[ssml::Segment],
    lang: &str,
    model_path: &Path,
    voice_path: &Path,
    speed: f32,
    format: OutputFormat,
) -> Result<Vec<u8>, TtsError> {
    let mut sess = sessions::KokoroSession::load(model_path)
        .map_err(|e| TtsError::SynthesisFailed(e.to_string()))?;
    synth_segments_kokoro_with(&mut sess, segments, lang, voice_path, speed, format)
}
```

with:

```rust
fn synth_segments_kokoro(
    segments: Vec<ssml::Segment>,
    lang: &str,
    model_path: &Path,
    voice_path: &Path,
    speed: f32,
    format: OutputFormat,
    expand_abbrev: bool,
) -> Result<Vec<u8>, TtsError> {
    // Run en::normalize_segments for en-* voices: maps Spell→Text via the
    // letter table, Text→acronym-expanded (when expand_abbrev), Emphasis→Text
    // with `+`-strip + warn-once. Mirror of synth_segments_vosk's call to
    // ru::normalize_segments at line 355. Closes #244.
    let segments = if lang.starts_with("en") {
        en::normalize_segments(segments, expand_abbrev)
    } else {
        segments
    };
    let mut sess = sessions::KokoroSession::load(model_path)
        .map_err(|e| TtsError::SynthesisFailed(e.to_string()))?;
    synth_segments_kokoro_with(&mut sess, &segments, lang, voice_path, speed, format)
}
```

> **Signature change:** `&[ssml::Segment]` → `Vec<ssml::Segment>` because `normalize_segments` consumes the vec by-value to avoid cloning. Update the caller in Step 5.2.

- [ ] **Step 5.2: Update the `say_ssml` Kokoro arm to pass owned `Vec` + `expand_abbrev`**

In `say_ssml` (around line 196-208), replace:

```rust
        EngineChoice::Kokoro {
            model_path,
            voice_path,
            speed,
        } => synth_segments_kokoro(
            &segments,
            opts.lang,
            model_path,
            voice_path,
            *speed,
            opts.format,
        ),
```

with:

```rust
        EngineChoice::Kokoro {
            model_path,
            voice_path,
            speed,
        } => synth_segments_kokoro(
            segments,
            opts.lang,
            model_path,
            voice_path,
            *speed,
            opts.format,
            opts.expand_abbrev,
        ),
```

(Drop the `&` — `segments` is now passed by-value. `opts.expand_abbrev` added as the trailing argument.)

This means the local `segments` binding is moved out of `say_ssml`. Confirm with:
```bash
grep -n 'let segments =' rust/src/tts/mod.rs | head -3
```
The Vosk arm in `say_ssml` doesn't reach the bottom of the function in the same way — verify the caller is not still using `segments` after the move. Looking at the source: `say_ssml`'s match expression is the entire return value, so moving `segments` into the Kokoro arm is fine; the Vosk arm is `unreachable!()` per line 210.

- [ ] **Step 5.3: Mark the `synth_segments_kokoro_with` Emphasis arm as defensive fallback**

Find the Emphasis arm (line ~264 in the current source). Replace the existing comment block:

```rust
            ssml::Segment::Emphasis { content, suppress } => {
                // <emphasis> stress markers are honored only on ru-vosk-* voices.
                // For Kokoro, strip `+` from content (G2P would otherwise choke on
                // the unfamiliar character) and warn the user once per process.
                // Skip the warning when suppress=true: the caller used level="none"
                // to explicitly opt out of stress markers — the warning would be
                // misleading. Closes #238.
```

with:

```rust
            ssml::Segment::Emphasis { content, suppress } => {
                // Defensive fallback: en::normalize_segments converts Emphasis→Text
                // upstream of synth_segments_kokoro_with's caller (synth_segments_kokoro).
                // The arm remains for `--stdin-loop` callers that bypass that wrapper
                // and feed segments directly. Mirrors synth_segments_vosk_with's
                // Emphasis fallback at line 386. Closes #238 (preserved).
```

The body (warn-once, `+`-strip, infer) stays unchanged.

- [ ] **Step 5.4: Run tests + clippy**

```bash
cd rust && cargo test --no-default-features --features onnx,tts --lib 2>&1 | tail -5
cargo clippy --all-targets --no-default-features --features onnx,tts -- -D warnings 2>&1 | tail -5
cargo fmt --check && echo fmt-clean
```
Expected: all unit tests pass, clippy clean, fmt clean.

- [ ] **Step 5.5: Commit**

```bash
git add rust/src/tts/mod.rs
git commit -m "$(cat <<'EOF'
feat(#244,tts): wire en::normalize_segments into Kokoro SSML path

synth_segments_kokoro (private) now calls en::normalize_segments before
delegating to synth_segments_kokoro_with — mirrors synth_segments_vosk's
ru::normalize_segments call at line 355. Existing Emphasis arm in
synth_segments_kokoro_with becomes a defensive fallback for --stdin-loop
callers. Closes #244 SSML path.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git rev-parse HEAD > /tmp/kesha-244-evidence/T5-ssml-wiring.sha
```

---

## Task 6: Capability flag + TS-side gate

Engine reports the new feature; TS CLI extends its capability check.

**Files:**
- Modify: `rust/src/capabilities.rs` (one line)
- Modify: `src/cli/say.ts` (description + capability gate)

- [ ] **Step 6.1: Add the engine-side feature flag**

In `rust/src/capabilities.rs`, find the `tts.ru_acronym_expansion` push:
```bash
grep -n 'tts.ru_acronym_expansion' rust/src/capabilities.rs
```
Expected: line 21.

Add immediately after:
```rust
        features.push("tts.en_acronym_expansion");
```

- [ ] **Step 6.2: Update the TS-side `--no-expand-abbrev` flag description**

In `src/cli/say.ts` line 89:

Old:
```ts
    "<say-as interpret-as='characters'> still works. No effect for non-ru-vosk voices.",
```

New:
```ts
    "<say-as interpret-as='characters'> still works. Applies to Russian (ru-vosk-*) and English (en-*) voices.",
```

- [ ] **Step 6.3: Extend the capability-gated forwarding**

Locate the spot where the CLI decides whether to forward `--no-expand-abbrev` to the engine subprocess:
```bash
grep -nE 'ru_acronym_expansion|noExpandAbbrev' src/cli/say.ts | head -10
```

If the CLI today checks `caps.features.includes("tts.ru_acronym_expansion")` before forwarding, change it to:
```ts
caps.features.some((f) => f === "tts.ru_acronym_expansion" || f === "tts.en_acronym_expansion")
```

(Exact location depends on the existing wiring; the principle is OR the new flag with the old one.)

If no such check exists today (the flag is forwarded unconditionally), no change is needed — note this in the commit message.

- [ ] **Step 6.4: Run tests**

```bash
cd rust && cargo test --no-default-features --features onnx,tts capabilities 2>&1 | tail -5
cd .. && bun test tests/unit 2>&1 | tail -5
bunx tsc --noEmit && echo tsc-clean
```
Expected: rust + bun unit tests pass, tsc clean.

- [ ] **Step 6.5: Verify the engine binary reports the flag**

```bash
cd rust && cargo build --no-default-features --features onnx,tts 2>&1 | tail -3
./target/debug/kesha-engine --capabilities-json | jq '.features | map(select(. | startswith("tts.")))'
```
Expected output includes `"tts.en_acronym_expansion"`.

- [ ] **Step 6.6: Commit**

```bash
cd /Users/anton/Personal/repos/kesha-voice-kit
git add rust/src/capabilities.rs src/cli/say.ts
git commit -m "$(cat <<'EOF'
feat(#244,tts): add tts.en_acronym_expansion capability flag

Engine reports the new capability; TS CLI extends its forwarding gate
for --no-expand-abbrev. Older engines (pre-1.10.0) won't have the flag
and the gate falls back to the existing ru_acronym_expansion path.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git rev-parse HEAD > /tmp/kesha-244-evidence/T6-capability.sha
```

---

## Task 7: Spike — letter-name alphabet listening test

Per CLAUDE.md "VERIFY THIRD-PARTY MODEL FORMATS WITH A SPIKE": the letter-name strings (`"ay bee see ... double yoo ex why zee"`) as misaki-rs G2P input are an unconfirmed shape. Synth + listen before locking the table.

**Files:** none (throwaway).

- [ ] **Step 7.1: Build the engine**

```bash
cd rust && cargo build --release --no-default-features --features onnx,tts 2>&1 | tail -3
```

- [ ] **Step 7.2: Synthesize the alphabet**

```bash
cd /Users/anton/Personal/repos/kesha-voice-kit
SPIKE=/tmp/kesha-244-spike && rm -rf "$SPIKE" && mkdir "$SPIKE"
echo "ay bee see dee ee ef jee aitch eye jay kay el em en oh pee kyu ar ess tee yoo vee double yoo ex why zee" \
  | rust/target/release/kesha-engine say --voice en-am_michael --out "$SPIKE/alphabet.wav" 2>&1 | tail -5
file "$SPIKE/alphabet.wav"
[[ $(stat -f%z "$SPIKE/alphabet.wav" 2>/dev/null || stat -c%s "$SPIKE/alphabet.wav") -gt 200000 ]] && echo "alphabet.wav size OK"
```
Expected: WAV file ~10s, mono float32 at 24 kHz (Kokoro), >200 KB.

- [ ] **Step 7.3: Listen and validate (manual)**

```bash
afplay "$SPIKE/alphabet.wav"
```

Listen for: are all 26 letter names recognizable? Any mush, drop-out, or clipping? Run this past the maintainer for sign-off if you're not a native US English speaker.

If problem letters surface, candidate swaps from spec:
- `H "aitch"` → `"aych"`
- `Q "kyu"` → `"queue"`
- `W "double yoo"` → `"dub-yoo"` or `"double-u"`

Each swap is a one-line edit in `rust/src/tts/en/letter_table.rs` plus one test-assertion update.

- [ ] **Step 7.4: Record the spike outcome**

```bash
echo "Spike outcome: PASS — all 26 letter names recognizable on en-am_michael." \
  > /tmp/kesha-244-evidence/T7-spike.notes
cp "$SPIKE/alphabet.wav" /tmp/kesha-244-evidence/T7-alphabet.wav
rm -rf "$SPIKE"
```

If the spike found problems, replace the notes with the swap decisions and apply them as a fix-up commit before continuing:
```bash
git commit -am "fix(#244,tts): swap letter-table entries (post-spike)"
```

- [ ] **Step 7.5: No code commit unless letter-table edits applied**

If letter table unchanged, no commit needed. Move on.

---

## Task 8: Integration test via `--stdin-loop`

End-to-end Rust integration test asserting the engine's behavior against the warm-session subprocess harness, mirror of `rust/tests/tts_ru_normalize.rs`. Tests text-norm, not audio quality (Task 9 covers that).

**Files:**
- Create: `rust/tests/tts_en_normalize.rs`

- [ ] **Step 8.1: Read the Russian harness for reference**

```bash
sed -n '1,120p' rust/tests/tts_ru_normalize.rs
```
Note the `LoopEngine` wrapper at lines ~73-150. The new file imports the same harness pattern; if `LoopEngine` is private to that file, lift it into a small shared `tests/common/mod.rs`. Otherwise duplicate per the existing convention.

- [ ] **Step 8.2: Create `rust/tests/tts_en_normalize.rs`**

Skeleton (full content depends on the `LoopEngine` shape — copy the working pattern from `tts_ru_normalize.rs` and adapt voice id + assertions):

```rust
//! Integration test for #244 — English acronym auto-expansion via Kokoro.
//! Uses the warm `--stdin-loop` subprocess to avoid model reload per case.
//! Mirrors rust/tests/tts_ru_normalize.rs.

mod common;
use common::LoopEngine;

#[test]
fn epam_spells_letter_by_letter_via_auto_expand() {
    let mut engine = LoopEngine::start_kokoro("en-am_michael").unwrap();

    // Baseline: --no-expand-abbrev → Kokoro fuses "EPAM" into one syllable.
    let raw = engine.synth_bytes("EPAM partners", &["--no-expand-abbrev"]).unwrap();
    let expanded = engine.synth_bytes("EPAM partners", &[]).unwrap();

    // Spell-mode WAV must be longer (more audio = letters spoken individually).
    assert!(
        expanded.len() > raw.len() + 5_000,
        "expanded={} raw={}, expected expanded > raw + 5KB",
        expanded.len(),
        raw.len()
    );
}

#[test]
fn nasa_passes_through_via_stop_list() {
    let mut engine = LoopEngine::start_kokoro("en-am_michael").unwrap();
    let with_flag = engine.synth_bytes("NASA briefed Congress", &[]).unwrap();
    let without_flag = engine.synth_bytes("NASA briefed Congress", &["--no-expand-abbrev"]).unwrap();
    // Stop-list entry — same audio either way (within ±5%).
    let delta = (with_flag.len() as i64 - without_flag.len() as i64).abs();
    assert!(
        delta < (without_flag.len() / 20) as i64,
        "NASA stop-list should produce identical audio; delta={delta}"
    );
}

#[test]
fn say_as_characters_overrides_no_expand_abbrev() {
    let mut engine = LoopEngine::start_kokoro("en-am_michael").unwrap();
    let raw_caps = engine
        .synth_bytes(
            r#"<speak>EPAM</speak>"#,
            &["--ssml", "--no-expand-abbrev"],
        )
        .unwrap();
    let say_as = engine
        .synth_bytes(
            r#"<speak><say-as interpret-as="characters">EPAM</say-as></speak>"#,
            &["--ssml", "--no-expand-abbrev"],
        )
        .unwrap();
    assert!(
        say_as.len() > raw_caps.len() + 5_000,
        "<say-as> should letter-spell even with --no-expand-abbrev; say_as={} raw_caps={}",
        say_as.len(),
        raw_caps.len()
    );
}
```

> **Adaptation note:** if `LoopEngine` does not expose `start_kokoro`, copy the construction from `tts_ru_normalize.rs::LoopEngine::start_vosk` and swap voice + engine subprocess args.

- [ ] **Step 8.3: Run the integration test**

```bash
cd rust && cargo test --no-default-features --features onnx,tts --test tts_en_normalize 2>&1 | tail -10
```
Expected: 3/3 passing.

- [ ] **Step 8.4: Commit**

```bash
cd /Users/anton/Personal/repos/kesha-voice-kit
git add rust/tests/tts_en_normalize.rs rust/tests/common/mod.rs 2>/dev/null
git commit -m "$(cat <<'EOF'
test(#244,tts): integration test for English acronym expansion

Three cases via warm --stdin-loop harness: EPAM spell vs raw byte-length,
NASA stop-list pass-through, <say-as> overrides --no-expand-abbrev.
Mirror of rust/tests/tts_ru_normalize.rs.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git rev-parse HEAD > /tmp/kesha-244-evidence/T8-integration.sha
```

---

## Task 9: Audio-quality-check on 15-phrase corpus

Per CLAUDE.md `audio-quality-check` agent (post-`rust/src/tts/**` commit gate). Replaces the human "послушай WAV" loop with deterministic stats.

**Files:** none (evidence files in `/tmp/kesha-244-evidence/`).

- [ ] **Step 9.1: Generate the 15-phrase corpus**

```bash
cd /Users/anton/Personal/repos/kesha-voice-kit
EV=/tmp/kesha-244-evidence
mkdir -p "$EV/corpus"

# 10 spell controls
declare -a SPELL=(
  "Our company EPAM partners with Anthropic."
  "AI is the future of software."
  "The CEO briefed the board."
  "The FBI is investigating the incident."
  "Send the request via HTTP."
  "The response is in JSON."
  "We use SQL for queries."
  "Modern apps run an LLM in the loop."
  "IBM and Microsoft are competitors."
  "Document the API for downstream callers."
)
# 5 word controls (stop-list)
declare -a WORDS=(
  "NASA briefed Congress."
  "NATO held a summit."
  "Wear the SCUBA gear."
  "It is OK."
  "Just GO home."
)

i=0
for phrase in "${SPELL[@]}"; do
  i=$((i + 1))
  echo "$phrase" | rust/target/release/kesha-engine say \
    --voice en-am_michael --out "$EV/corpus/spell-$i.wav" 2>/dev/null
done
i=0
for phrase in "${WORDS[@]}"; do
  i=$((i + 1))
  echo "$phrase" | rust/target/release/kesha-engine say \
    --voice en-am_michael --out "$EV/corpus/word-$i.wav" 2>/dev/null
done
ls "$EV/corpus" | wc -l
```
Expected: 15 files.

- [ ] **Step 9.2: Run the `audio-quality-check` agent**

Dispatch the `audio-quality-check` subagent (per CLAUDE.md, this agent is "Use after any commit touching rust/src/tts/**"):

> "Run audio-quality-check on `/tmp/kesha-244-evidence/corpus/*.wav` (15 files). Expected sample rate 24000 Hz, mono float32. Flag any file with RMS < −40 dB, silence ratio > 0.4, or length-vs-grapheme ratio outside the band the agent uses for English. Cross-reference against the source phrases in `/tmp/kesha-244-evidence/corpus/manifest.txt` (write this manifest first if absent). Report a one-line PASS/FAIL plus per-file deviations."

- [ ] **Step 9.3: Capture the agent report**

```bash
# After the agent finishes, paste its summary into the evidence dir:
echo "<agent report verbatim>" > /tmp/kesha-244-evidence/T9-audio-qa.report
```

- [ ] **Step 9.4: No code commit; evidence-only**

If the agent reports a failure, fix the offending case (likely a letter-table swap from Task 7's candidate list) and re-run `audio-quality-check` until clean. Each fix-up is its own commit.

---

## Task 10: Documentation — README + SKILL.md + CLI help text

User-facing copy. Mirror of the Russian section shipped in #232.

**Files:**
- Modify: `README.md`
- Modify: `SKILL.md`
- (`src/cli/say.ts` help text already updated in Task 6.)

- [ ] **Step 10.1: Find the existing Russian acronym example block**

```bash
grep -n 'ru-vosk\|acronym' README.md SKILL.md | head -20
```

- [ ] **Step 10.2: Add an English example block to `README.md`**

Append, near the existing TTS examples:

```markdown
**English acronyms** auto-expand on Kokoro voices (closes #244):

```bash
kesha say --voice en-am_michael 'EPAM partners with Anthropic'
# audible: "ee pee ay em partners with anthropic"

kesha say --voice en-am_michael --no-expand-abbrev 'EPAM partners with Anthropic'
# audible: "epam partners with anthropic"  (Kokoro fuses to one syllable)
```

NASA, NATO, AIDS, SCUBA, etc. read as words (stop-list). Use
`<say-as interpret-as="characters">` to force letter-spelling regardless.
```

- [ ] **Step 10.3: Add a paragraph to `SKILL.md`**

Append next to the Russian acronym section, ~one paragraph mirroring the Russian wording.

- [ ] **Step 10.4: Verify with bun test**

```bash
bun test 2>&1 | tail -3
bunx tsc --noEmit && echo tsc-clean
```
Expected: no test churn (docs only).

- [ ] **Step 10.5: Commit**

```bash
git add README.md SKILL.md
git commit -m "$(cat <<'EOF'
docs(#244): English acronym auto-expansion examples

README + SKILL.md mirror the Russian section shipped in #232. Documents
the aggressive spell-by-default rule, the 30-entry stop-list (NASA/NATO/
SCUBA/OK/IT/...), and the <say-as> override.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git rev-parse HEAD > /tmp/kesha-244-evidence/T10-docs.sha
```

---

## Task 11: Lockstep version bump to 1.10.0

Per CLAUDE.md "RELEASE PROCESS" — engine release because `rust/` changed.

**Files:**
- Modify: `package.json` (`version` and `keshaEngine.version`)
- Modify: `rust/Cargo.toml` (`version`)
- Modify: `rust/Cargo.lock` (auto-updated via `cargo check`)

- [ ] **Step 11.1: Bump `package.json`**

```bash
cd /Users/anton/Personal/repos/kesha-voice-kit
sed -i '' 's/"version": "1.9.0"/"version": "1.10.0"/' package.json
sed -i '' 's/"version": "1.9.0"/"version": "1.10.0"/' package.json  # second occurrence (keshaEngine.version)
grep '"version"' package.json
```
Expected: both `version` and `keshaEngine.version` show `1.10.0`.

- [ ] **Step 11.2: Bump `rust/Cargo.toml`**

```bash
sed -i '' 's/^version = "1.9.0"/version = "1.10.0"/' rust/Cargo.toml
grep '^version' rust/Cargo.toml
```
Expected: `version = "1.10.0"`.

- [ ] **Step 11.3: Update `Cargo.lock`**

```bash
cd rust && cargo check --no-default-features --features onnx,tts 2>&1 | tail -3
grep -A1 'name = "kesha-engine"' Cargo.lock | head -3
```
Expected: lock file shows `version = "1.10.0"` for the `kesha-engine` package.

- [ ] **Step 11.4: Verify the binary reports the new version**

```bash
cargo build --release --no-default-features --features onnx,tts 2>&1 | tail -3
./target/release/kesha-engine --version
```
Expected: `kesha-engine 1.10.0`.

- [ ] **Step 11.5: Run the full test suite once more**

```bash
cargo test --no-default-features --features onnx,tts 2>&1 | tail -5
cargo clippy --all-targets --no-default-features --features onnx,tts -- -D warnings 2>&1 | tail -3
cargo fmt --check && echo fmt-clean
cd .. && bun test 2>&1 | tail -3
bunx tsc --noEmit && echo tsc-clean
```
Expected: all green.

- [ ] **Step 11.6: Commit**

```bash
git add package.json rust/Cargo.toml rust/Cargo.lock
git commit -m "$(cat <<'EOF'
chore(release): bump engine + CLI to v1.10.0 for #244

Lockstep bump per CLAUDE.md RELEASE PROCESS — engine release because
rust/ changed. v1.9.0 reserved for #247 (timestamped transcript segments).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git rev-parse HEAD > /tmp/kesha-244-evidence/T11-bump.sha
```

---

## Task 12: STOP — manual release runbook (ask user before tagging)

Per the spec's verifiability gate and CLAUDE.md "RELEASE PROCESS — CLI AND ENGINE ARE VERSIONED INDEPENDENTLY", the actual tag push + draft publish + npm publish requires explicit user authorization. Do NOT auto-tag.

**Files:** none.

- [ ] **Step 12.1: Push the branch**

```bash
git push -u origin feat/244-kokoro-en-acronym 2>&1 | tail -3
```
Expected: branch pushed.

- [ ] **Step 12.2: Open a draft PR linked to #244**

```bash
gh pr create -R drakulavich/kesha-voice-kit \
  --title "feat(tts): English acronym auto-expansion for Kokoro — v1.10.0 release (closes #244)" \
  --body "$(cat <<'EOF'
Closes #244. Direct mirror of #232 architecture (shipped v1.7.0).

## What's new

- Auto-expand all-uppercase Latin acronyms (length 2-5) on `en-*` (Kokoro) voices, gated by the existing `--no-expand-abbrev` flag.
- 30-entry stop-list covers emphatic short caps (`OK`, `IT`, `IS`, …) and natural-English caps words (`NASA`, `NATO`, `AIDS`, `SCUBA`, …).
- `<say-as interpret-as="characters">` always wins over `--no-expand-abbrev`.
- New capability flag `tts.en_acronym_expansion`.

## Test evidence

Per-task SHAs at `/tmp/kesha-244-evidence/`. Audio QA: see `T9-audio-qa.report`. Spike: `T7-spike.notes` + `T7-alphabet.wav`.

## Verifiability

- Rust unit tests: `tts::en::*` modules — 30+ assertions.
- Rust integration test: `rust/tests/tts_en_normalize.rs` — 3 cases via `--stdin-loop`.
- Capability JSON: `kesha-engine --capabilities-json` includes `tts.en_acronym_expansion`.
- audio-quality-check: 15-phrase corpus (10 spell + 5 word controls) PASS.

## Release plan

After merge, follow CLAUDE.md "RELEASE PROCESS":
1. `git tag v1.10.0 && git push origin v1.10.0` (triggers `build-engine.yml`).
2. Author release notes BEFORE `gh release edit v1.10.0 --draft=false`.
3. Independent v1.10.0 validation: download `kesha-engine-darwin-arm64`, exercise `EPAM` end-to-end, confirm WAV byte-length distinguishable from `--no-expand-abbrev` baseline.
4. `npm publish --access public`.

EOF
)"
```

- [ ] **Step 12.3: STOP — ask the user**

Wait for explicit go-ahead from drakulavich before:
- Tagging `v1.10.0`
- Triggering the build-engine workflow
- Authoring release notes
- Publishing the draft (`--draft=false`)
- Running independent validation
- Running `npm publish --access public`

Do NOT execute any of those steps without confirmation. The user-facing prompt should look like:

> "PR opened, branch pushed. Ready to drive the v1.10.0 release runbook (tag → build-engine → release notes → publish draft → independent validation → npm publish). The npm publish step is irreversible. Want me to proceed?"

---

## Self-review

After completing tasks 1–11, before opening the PR (Task 12.2):

**1. Spec coverage:**

| Spec section | Implemented in |
|---|---|
| §Architecture / file layout | T1 (letter-table), T2 (acronym), T3 (mod facade), T4 (plain wiring), T5 (SSML wiring) |
| §Rule details — spell rule | T2 |
| §Rule details — stop-list (30 entries) | T2 |
| §Rule details — letter table | T1 |
| §Rule details — tokenization & punctuation | T2 |
| §SSML interaction (Spell, Emphasis, Break, Ipa, cardinal/ordinal) | T3 + spec amendment in T3.0 |
| §CLI surface — `--no-expand-abbrev` description | T6.2 |
| §CLI surface — TS capability gate | T6.3 |
| §Capability JSON | T6.1, T6.5 |
| §Testing — Rust unit | T1, T2, T3 |
| §Testing — Rust integration | T8 |
| §Testing — audio-quality-check | T9 |
| §Testing — capability JSON test | T6.5 (manual binary check; bun-side gap acknowledged) |
| §Release plan — lockstep bump | T11 |
| §Release plan — tag + build + notes + draft + validate + publish | T12 (manual; gated on user) |

**2. Placeholder scan:** No "TBD", "implement later", or "similar to Task N" without code. Each step shows actual code or actual command.

**3. Type consistency:** Every function name and type referenced across tasks is defined in an earlier task. `en::expand_text`, `en::normalize_segments`, `letter_table::expand_chars`, `acronym::expand_acronyms`, `Segment::Spell`, `Segment::Emphasis`, `Segment::Text`, `Segment::Break`, `Segment::Ipa` — all defined or pre-existing.

**4. CLAUDE.md gates verified:**
- ✓ `cargo clippy --all-targets -- -D warnings` runs at T3.5, T4.4, T5.4, T6.4, T11.5.
- ✓ `cargo fmt --check` runs at T3.5, T5.4, T11.5.
- ✓ `bun test && bunx tsc --noEmit` runs at T6.4, T11.5.
- ✓ `audio-quality-check` runs at T9 (post-`rust/src/tts/**` commits).
- ✓ Independent v1.10.0 validation gated behind user authorization at T12.

---

## Execution

Plan complete. Two execution options:

1. **Subagent-Driven** (recommended) — fresh subagent per task, two-stage review (spec compliance + code quality) between tasks. Use `superpowers:subagent-driven-development`.
2. **Inline Execution** — execute tasks in this session. Use `superpowers:executing-plans`.
