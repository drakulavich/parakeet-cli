# English abbreviation handling for Kokoro

**Date:** 2026-05-07
**Status:** Approved (sections 1-4, brainstormed with maintainer)
**Issue:** #244 (this spec)
**Mirror of:** #232 (Russian, shipped v1.7.0)
**Related:** #212 (multi-language Kokoro G2P, independent)
**Branch:** `feat/244-kokoro-en-acronym`
**Engine release:** v1.10.0 (v1.9.0 reserved for #247 — timestamped transcript segments)

## Problem

Kokoro reads English acronyms as fused single syllables instead of letter-by-letter:

```
$ kesha say --voice en-am_michael 'EPAM partners with Anthropic'
# audible: "epam partners with anthropic"   ← made-up syllable
```

Expected: `"ee pee ay em partners with anthropic"`. Same problem with `AI`, `FBI`, `IBM`, `CEO`, `API`, `HTTP`, `JSON`, `XML`, `URL`, `LLM`, `SQL`, `CSS` — every all-caps Latin token misaki-rs / Kokoro doesn't recognize as a word.

The user can wrap such tokens in `<say-as interpret-as="characters">…</say-as>`, but the SSML parser routes `Spell` segments through the engine's pass-through path on the Kokoro side. SSML hints never reach Kokoro as letter spellings.

This is the English mirror of #232 (Russian Vosk acronym auto-expansion, shipped v1.7.0). Same rule shape with one deliberate difference (see §Decisions), different letter-name table, different stop-list.

## Goal

For voice id prefix `en-*` (Kokoro):

1. Honor `<say-as interpret-as="characters">…</say-as>` as a deterministic letter-by-letter expansion using a US-canonical English letter-name table.
2. Auto-detect English acronyms (all-uppercase Latin, length 2–5) in plain text and apply the same expansion. Opt-out via the existing `--no-expand-abbrev` flag. Stop-list of ~30 entries covers both emphatic short caps words (`OK`, `IT`, `IS`, …) and natural-English caps words read as words (`NASA`, `NATO`, `AIDS`, …).

Out of scope (this spec):

- AVSpeech (`macos-*`) acronym handling — Apple's TTS already does this reasonably; no demand.
- Mixed-script Cyrillic+Latin in the same token (e.g. transliterated brand names).
- Numeric `<say-as interpret-as="cardinal|ordinal|date|telephone|...">` — separate concern.
- Inflected English (`EPAMs`, `APIs`, `URLs`) — only fully-uppercase tokens match; track separately if real cases surface.
- Per-acronym custom-pronunciation overrides table (`KNOWN_PRONUNCIATIONS`) — YAGNI; design leaves a clean seam at `tts::en::acronym::expand_acronyms` to add later if a brand-pronunciation case appears in real usage.
- UK-canonical letter names (`Z="zed"`, `H="haitch"`) — re-evaluate when British Kokoro voices land.
- Stress / `<emphasis>` on Kokoro path — Kokoro has no `+`-marker analog of Vosk; out of scope, #236-adjacent.

## Decisions (from brainstorm)

| Question | Decision | Rationale |
|---|---|---|
| Q1 — rule shape | **Aggressive spell-by-default** (option A) | Most all-caps Latin tokens are acronyms with no natural English reading; spelling is the safe default. The Russian rule's CV/CVC alternation pass-through doesn't transfer (Kokoro has no Vosk-style natural-syllable behavior to lean on). |
| Q2 — overrides table | **Single stop-list, no per-acronym overrides** | YAGNI. The `tts::en::acronym::expand_acronyms` function leaves a clean seam to add `KNOWN_PRONUNCIATIONS` later if a brand-pronunciation case surfaces. |
| Q3 — stop-list contents | **30-entry seed**: 20 emphatic length-2 caps + 10 natural-English caps words | Minimum viable; one-line edits to extend; misses are user-reportable. |
| Q4 — structural rules | Mirror Russian: tokenize on whitespace, peel `«("` / `.,:;!?"...`, require all `[A-Z]`, length 2–5, case-sensitive stop-list lookup | Proven pattern; predictable. Digits, hyphens, possessives, mixed case all pass through unchanged. |
| Q5 — capability flag | `tts.en_acronym_expansion: true` reported in `--capabilities-json` | TS CLI capability gate extends to OR the new flag; avoids breaking against pre-1.10.0 engines. |
| Q6 — letter-table spike | Required: throwaway Kokoro+misaki synth of full alphabet before committing the table | CLAUDE.md "VERIFY THIRD-PARTY MODEL FORMATS WITH A SPIKE" — letter-name strings as misaki input are an unconfirmed shape. Swap candidates if needed: `H "aych"`, `Q "queue"`, `W "dub-yoo"`. |
| Architecture | **Direct mirror of `tts/ru/`** (Approach 1) — new `tts/en/` module, called from `say_with_kokoro` exactly as `tts/ru/` is called from `say_with_vosk` | YAGNI on the `LangNormalizer` trait; #212 (multi-language Kokoro) is its own scope and would re-cut a premature trait. |
| Voice routing | **No prefix-string dispatch** — engine choice = language choice (Kokoro = English, Vosk = Russian) | Mirrors Russian wiring. |
| `<emphasis>` on Kokoro path | **Strip `+` from content + warn-once on first occurrence per process** (key `emphasis-non-ru-vosk`); emit `Text(content)` | Kokoro has no `+`-marker stress mechanism analog of Vosk. Mirrors `ru::normalize_segments` Emphasis handling; preserves v1.8.1 (#237/#238) warn-once UX. |
| Letter-table position-dependence | **None** (unlike Russian's С) | English letter names are uniform regardless of position. |

## Architecture

### Pipeline

```
input text  +  optional --ssml flag  +  voice id (en-*)
    │
    ▼
┌─────────────────────────────────────────────────────────┐
│  1. SSML parse (existing, ssml::parse) — UNCHANGED      │
│     "<speak>...</speak>"  →  Vec<Segment>               │
│       • Text(String)                                    │
│       • Break(Duration)                                 │
│       • Spell(String)        ← from <say-as            │
│            interpret-as="characters">                   │
│       • Emphasis { content, suppress }                  │
│       • Ipa(String)                                     │
└─────────────────────────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────────────────────────┐
│  2. EN normalization (NEW, tts::en::normalize_segments) │
│     for each Spell segment:                             │
│       letter_table::expand_chars(text)                  │
│     for each Text segment, if expand_abbrev:            │
│       acronym::expand_acronyms(text)                    │
│     for each Emphasis segment:                          │
│       drop tag, emit Text(content) verbatim             │
│     Break / Ipa pass through                            │
└─────────────────────────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────────────────────────┐
│  3. Kokoro synth (existing) — concatenate per-segment   │
│     audio frames with break-as-silence                  │
└─────────────────────────────────────────────────────────┘
```

### File layout

```
rust/src/tts/
├── en/                              ← new
│   ├── mod.rs                       ← expand_text, normalize_segments
│   ├── acronym.rs                   ← STOP_LIST, is_acronym_token, expand_acronyms, split_punct
│   └── letter_table.rs              ← LETTERS const, expand_chars
├── ru/                              ← unchanged
└── mod.rs                           ← +1 wiring change in say_with_kokoro
```

`rust/src/capabilities.rs` — add `en_acronym_expansion: true` alongside `ru_acronym_expansion`.
`src/cli/say.ts` — extend the capability-gated forwarding for `--no-expand-abbrev`; update help text.

### Module responsibilities

**`tts::en::letter_table`** — pure text-in / text-out; lowercases input, looks up each `[a-z]` in the table, joins with single spaces. Non-Latin characters pass through verbatim (defensive; matcher upstream filters them out, but keeps `<say-as>` of mixed input safe). No `phrase_override` (Q2).

**`tts::en::acronym`** — pure text-in / text-out; tokenizes on Unicode whitespace, peels leading/trailing punctuation, applies `is_acronym_token` rule, consults `STOP_LIST`, calls `letter_table::expand_chars` on matches.

**`tts::en::mod`** — two public functions:
- `expand_text(&str) -> String` — used by the non-SSML Kokoro path; runs `acronym::expand_acronyms`.
- `normalize_segments(Vec<Segment>, bool) -> Vec<Segment>` — used by the SSML Kokoro path; routes `Spell` / `Text` / `Emphasis` per the table above.

## Rule details

### Spell rule

```rust
fn is_acronym_token(core: &str) -> bool {
    let len = core.chars().count();
    if !(2..=5).contains(&len) { return false; }
    core.chars().all(|c| c.is_ascii_uppercase())
}
```

`expand_token` then checks `STOP_LIST.contains(&core)` (case-sensitive); if hit, the token passes through verbatim. Otherwise, replace `core` with `expand_chars(core)`, re-attach head/tail punctuation.

### Stop-list (30 entries)

```rust
const STOP_LIST: &[&str] = &[
    // Emphatic length-2 caps (~20)
    "OK", "NO", "GO", "IT", "IS", "AS", "AT", "BY", "IN", "ON",
    "OR", "OF", "TO", "WE", "US", "MY", "ME", "HE", "BE", "DO",
    // Natural-English caps words (~10)
    "NASA", "NATO", "AIDS", "OPEC", "IKEA",
    "ASCII", "NAFTA", "LASER", "RADAR", "SCUBA",
];
```

Deliberately NOT on the list (so they spell):
- `AM`, `PM` — clock times → "ay em" / "pee em"
- `TV`, `DC`, `AC`, `CD`, `DVD`, `JPG`, `WWE` — typically read letter-by-letter
- `YES` — when written all-caps almost always emphatic shouting; letter-spell sounds OK
- Length-3+ short words (`WHO`, `BUT`, `AND`) — too rare in caps; if shouted, letter-spell is acceptable

Maintainer extends as users report mispronunciations. One-line edits.

### Letter table (US-canonical, 26 entries)

```rust
const LETTERS: &[(char, &str)] = &[
    ('a',"ay"),  ('b',"bee"), ('c',"see"), ('d',"dee"),  ('e',"ee"),
    ('f',"ef"),  ('g',"jee"), ('h',"aitch"),('i',"eye"), ('j',"jay"),
    ('k',"kay"), ('l',"el"),  ('m',"em"),  ('n',"en"),   ('o',"oh"),
    ('p',"pee"), ('q',"kyu"), ('r',"ar"),  ('s',"ess"),  ('t',"tee"),
    ('u',"yoo"), ('v',"vee"), ('w',"double yoo"),('x',"ex"),('y',"why"),
    ('z',"zee"),
];
```

`expand_chars` lowercases, looks up, joins with single spaces. No position-dependence (unlike Russian's С). No `phrase_override` table.

### Tokenization & punctuation

Identical to Russian: tokenize on Unicode whitespace; peel a leading run of `«("` (head) and a trailing run of `.,:;!?"...—–-` (tail); the rest is `core`. Examples:

- `"EPAM"` → head `""`, core `"EPAM"`, tail `""` → `"ee pee ay em"`
- `"«EPAM»"` → head `"«"`, core `"EPAM"`, tail `"»"` → `"«ee pee ay em»"`
- `"EPAM."` → head `""`, core `"EPAM"`, tail `"."` → `"ee pee ay em."`
- `"H2O"` → core `"H2O"`, structural check (digit) fails → unchanged
- `"EPAMs"` → core `"EPAMs"`, structural check (lowercase `s`) fails → unchanged
- `"T-shirt"` → core `"T-shirt"`, structural check fails → unchanged
- `"iPhone"` → core `"iPhone"`, structural check fails → unchanged

## SSML interaction

| Tag | Kokoro path behavior |
|---|---|
| `<say-as interpret-as="characters">FBI</say-as>` | `Segment::Spell("FBI")` → `letter_table::expand_chars` → "ef bee eye". Always wins; not gated by `--no-expand-abbrev`. |
| `<say-as interpret-as="cardinal" / "ordinal" / "date" / ...>` | Existing behavior unchanged: warn + strip + pass content through. |
| `<emphasis>...</emphasis>` | Kokoro has no stress mechanism for `+`-marker stress placement. `en::normalize_segments` strips `+` from content, warns once per process (key: `emphasis-non-ru-vosk`), and emits `Text(content)`. Mirrors `ru::normalize_segments` Emphasis handling; preserves v1.8.1 (#237/#238) warn-once UX. |
| `<break time="...">` | `Segment::Break(Duration)` → silence frame in synth. Unchanged. |
| `<phoneme alphabet="ipa" ph="...">` | `Segment::Ipa(String)` → existing engine handling. Unchanged. |

## CLI surface

- **`--no-expand-abbrev`** — flag already exists from #232. Today the flag's `expand_abbrev: bool` already threads to `say_with_kokoro` but is unused on that path; we start using it. Update help text from "still works. No effect for non-ru-vosk voices." to "applies to Russian (ru-vosk-*) and English (en-*) voices."
- **TS-side capability gate** — `getEngineCapabilities()` already returns the JSON; the forwarding check that decides whether to pass `--no-expand-abbrev` to the subprocess extends from `caps.tts?.ru_acronym_expansion` to `caps.tts?.ru_acronym_expansion || caps.tts?.en_acronym_expansion`.
- **Help text** — one example line in `kesha say --help`.
- **README / SKILL.md** — one example block, mirroring the Russian one shipped in #232.

## Capability JSON

```jsonc
{
  "tts": {
    "engines": ["kokoro", "vosk", "avspeech"],
    "ru_acronym_expansion": true,
    "en_acronym_expansion": true,        // new
    "ssml_emphasis_ru": true,
    // ...
  }
}
```

## Testing

### Rust unit tests

- **`tts/en/acronym.rs::tests`** — table-driven matrix mirroring Russian's `cases()`:
  - Spell cases: `EPAM`, `AI`, `CEO`, `FBI`, `HTTP`, `JSON`, `SQL`, `LLM`, `IBM`, `API`, `URL`, `CSS`, `XML`, `IEEE`, `STT`, `TTS` — including length-2, no-vowel, alternating-CV.
  - Stop-list round-trips: every entry preserved.
  - Punctuation: `«EPAM»`, `EPAM!`, `CEO,`, `FBI?`.
  - Inflected / mixed / digits: `EPAMs`, `iPhone`, `H2O`, `MP3`, `T-shirt`, `WiFi` → unchanged.
  - Empty input, whitespace-only input.

- **`tts/en/letter_table.rs::tests`** — full alphabet round-trip (26 audible tokens), lowercase input, mixed input passthrough, `<say-as>EPAM</say-as>` → `"ee pee ay em"`.

- **`tts/en/mod.rs::tests`** — `Spell` → letter-table; `Text` with/without `auto_expand`; `Emphasis` strips tag preserves content; `Break`/`Ipa` pass through.

### Rust integration test

New `rust/tests/tts_en_normalize.rs` mirroring `tts_ru_normalize.rs`. Uses the `--stdin-loop` subprocess harness. Cases:
- 5 spell phrases (`EPAM partners with Anthropic`, `AI is the future`, `The CEO of NASA briefed the FBI`, `JSON over HTTP`, `XML and CSS`).
- 2 stop-list (`NASA briefed Congress`, `It is OK`).
- 1 SSML `<say-as>` round-trip.
- 1 `--no-expand-abbrev` round-trip.

Asserts the engine's normalized text on stderr (not the WAV — that's `audio-quality-check`'s job).

### Capability JSON test

Extend `tests/integration/capabilities.test.ts` to assert `tts.en_acronym_expansion === true`.

### Audio-quality-check (subagent)

15-phrase corpus at `/tmp/kesha-244-evidence/`:
- 10 spell controls in sentence context: `EPAM`, `AI`, `CEO`, `FBI`, `HTTP`, `JSON`, `SQL`, `LLM`, `IBM`, `API`.
- 5 word controls: `NASA briefed Congress.`, `NATO held a summit.`, `Wear the SCUBA gear.`, `It is OK.`, `Just GO home.`

Per-WAV checks: RMS ≥ −40 dB, silence ratio ≤ 0.4, sample rate 24000, mono, length-vs-grapheme ratio in band. The agent flags suspicious files; subjective listening is the final gate before release.

### Pre-merge gate (CLAUDE.md)

```bash
cd rust && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
bun test && bunx tsc --noEmit
# Plus audio-quality-check agent on the 15-phrase corpus.
```

### Independent v1.10.0 validation (CLAUDE.md "make smoke-test ALONE DOES NOT VALIDATE")

After `gh release edit v1.10.0 --draft=false`:

```bash
SMOKE=/tmp/kesha-v1.10.0-smoke && rm -rf "$SMOKE" && mkdir "$SMOKE" && cd "$SMOKE"
curl -sLfo kesha-engine \
  "https://github.com/drakulavich/kesha-voice-kit/releases/download/v1.10.0/kesha-engine-darwin-arm64"
chmod +x kesha-engine && xattr -d com.apple.quarantine kesha-engine 2>/dev/null

./kesha-engine --version  # must print 1.10.0
./kesha-engine --capabilities-json | jq '.features.tts.en_acronym_expansion'  # must be true

KESHA_CACHE_DIR="$SMOKE/cache" ./kesha-engine install --tts
echo "EPAM partners with Anthropic" | KESHA_CACHE_DIR="$SMOKE/cache" \
  ./kesha-engine say --voice en-am_michael --out "$SMOKE/spell.wav"
echo "EPAM partners with Anthropic" | KESHA_CACHE_DIR="$SMOKE/cache" \
  ./kesha-engine say --voice en-am_michael --no-expand-abbrev --out "$SMOKE/raw.wav"
# spell.wav byte-length must exceed raw.wav (more audio = letter-spell took effect).
[[ $(stat -f%z "$SMOKE/spell.wav") -gt $(stat -f%z "$SMOKE/raw.wav") ]] \
  || { echo "ERROR: spell-mode WAV is not longer than raw mode"; exit 1; }
```

Repeat for `kesha-engine-linux-x64` (Docker if not on Linux).

If any step fails: do NOT `npm publish`. Either yank the GitHub release (`gh release delete v1.10.0 --yes`, delete the tag, bump patch, retry) or push a fix and rebuild via `gh workflow run "🔨 Build Engine"`.

## Release plan (engine release v1.10.0)

Per CLAUDE.md "RELEASE PROCESS — CLI AND ENGINE ARE VERSIONED INDEPENDENTLY". Engine release because `rust/` changes:

1. Lockstep bump: `rust/Cargo.toml`, `rust/Cargo.lock` (via `cargo check`), `package.json#keshaEngine.version`, `package.json#version` → all `1.10.0`.
2. Merge to main.
3. `git tag v1.10.0 && git push origin v1.10.0` — triggers `build-engine.yml`.
4. Author release notes BEFORE publishing the draft (`gh release edit v1.10.0 --notes "..."`).
5. `gh release edit v1.10.0 --draft=false`.
6. **Independent v1.10.0 validation** per the script above. Behavior testing is the human-in-the-loop pre-publish gate.
7. `npm publish --access public`.
8. Verify `gh issue view 244` is auto-closed; if still open, close manually.

## Acceptance criteria

- [ ] `kesha say --voice en-am_michael 'EPAM partners with Anthropic. AI is the future. The CEO of NASA briefed the FBI.'` audibly says letters for `EPAM`, `AI`, `CEO`, `FBI`; reads `NASA` as a word.
- [ ] `kesha say --voice en-am_michael --no-expand-abbrev 'EPAM ...'` → unchanged (Kokoro reads "epam" as one syllable, current behavior).
- [ ] `kesha say --voice en-am_michael --ssml '<speak><say-as interpret-as="characters">EPAM</say-as></speak>'` → "ee pee ay em" via letter-table (always wins over `--no-expand-abbrev`).
- [ ] `kesha say --voice ru-vosk-m02 'NASA в России'` — unchanged (English-rule does not apply on ru-vosk-* path; Latin chars structurally rejected by Russian matcher).
- [ ] All Rust unit + integration tests green; `cargo clippy --all-targets -- -D warnings` clean.
- [ ] `bun test && bunx tsc --noEmit` clean.
- [ ] `kesha-engine --capabilities-json` reports `tts.en_acronym_expansion: true`.
- [ ] CHANGELOG / README / SKILL.md examples for English acronym handling.
- [ ] Independent v1.10.0 validation passes (spell.wav > raw.wav byte length).
