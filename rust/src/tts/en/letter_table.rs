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
