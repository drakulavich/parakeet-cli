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
    "OK", "NO", "GO", "IT", "IS", "AS", "AT", "BY", "IN", "ON", "OR", "OF", "TO", "WE", "US", "MY",
    "ME", "HE", "BE", "DO", // Natural-English caps words
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
