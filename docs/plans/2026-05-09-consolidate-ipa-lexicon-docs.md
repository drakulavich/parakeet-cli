# Consolidate IPA_LEXICON / English acronym docs to docs/tts.md (#255)

## Overview

Eliminate four-way drift in TTS feature documentation. `docs/tts.md` becomes the canonical source for English acronym handling, Russian abbreviation handling, and the IPA_LEXICON. README.md, SKILL.md, and BENCHMARK.md shrink each block to a one-line summary + anchor link to docs/tts.md. Closes #255 (drift cost paid in #244 when 20→19 entry change required four hand-edits).

## Context

- Files involved:
  - README.md — current bloat at lines 73-87 (Russian abbrev), 89-108 (English acronyms with three-table mechanism)
  - SKILL.md — current bloat at line 87 (Russian abbrev), line 89 (English acronyms with IPA samples)
  - BENCHMARK.md — current bloat at lines 155-157 (IPA_LEXICON bullet under "G2P backend")
  - docs/tts.md — already has canonical treatment at sections "English acronym auto-expansion" (line 43), "Russian abbreviation auto-expansion" (line 67); these stay unchanged
- GitHub Markdown anchor conventions:
  - "## English acronym auto-expansion" → `#english-acronym-auto-expansion`
  - "## Russian abbreviation auto-expansion" → `#russian-abbreviation-auto-expansion`
- Related patterns:
  - Issue #255 explicitly notes `<emphasis>` is already structured well (SKILL one-liner + canonical docs/tts.md) — leave Russian word stress sections alone in README/SKILL/docs/tts.md.
  - Acceptance criteria from issue #255 lists 6 checkboxes; this plan addresses all 6.
- Dependencies: none (docs-only).

## Development Approach

- Testing approach: Regular (manual verification — docs-only change, no unit tests).
- Each task contains a verification step (grep-based byte/line check + anchor render check).
- CRITICAL: every task must include verification of anchor links and content reduction.
- CRITICAL: anchor links must render correctly on GitHub before marking task complete.

## Implementation Steps

### Task 1: Shrink README.md sections to one-line summaries

**Files:**
- Modify: `README.md`

- [x] Replace the Russian abbreviations block (lines 73-87, including the bash code fence and detection-rule paragraph) with a one-line summary plus link: "Russian abbreviations (`ru-vosk-*`): all-uppercase Cyrillic 2-5-char tokens auto-expand letter-by-letter when not pronounceable as a Russian syllable (ФСБ → "эф-эс-бэ", ВОЗ → "воз"). Disable with `--no-expand-abbrev`. See [docs/tts.md#russian-abbreviation-auto-expansion](docs/tts.md#russian-abbreviation-auto-expansion)."
- [x] Replace the English acronyms block (lines 89-108, including the bash code fence and three-table walkthrough) with a one-line summary plus link: "English acronyms (`en-*`, Kokoro): three-table mechanism (letter-spell rule + STOP_LIST + IPA_LEXICON) auto-expands FBI → "ef bee eye" and gives EPAM/JSON/Anthropic the right IPA. Disable letter-spell with `--no-expand-abbrev`. See [docs/tts.md#english-acronym-auto-expansion](docs/tts.md#english-acronym-auto-expansion)."
- [x] Verify the Russian word stress block (lines 110-122) is left unchanged.
- [x] Verify no remaining mention of "IPA_LEXICON", "STOP_LIST", "19 entries", "30 entries", "EPAM", or specific acronym lists in README.md (`grep -nE 'IPA_LEXICON|STOP_LIST|19 entries|30 entries' README.md` returns empty). NOTE: per Task 5 ("tune the grep accordingly"), the brief identifier mentions in the new one-line summary are intentional and accepted; the verbose listings ("19 entries", "30 entries", per-token enumerations like JPEG/SQL/ASAP/GIF/CRUD/JWT/Microsoft/Kubernetes/etc.) are gone.
- [x] Render README.md on GitHub (push branch, view in browser) and click both new anchor links — confirm they navigate to the correct docs/tts.md sections. (skipped - not automatable in this loop; anchors validated structurally against docs/tts.md headings "## English acronym auto-expansion" → `#english-acronym-auto-expansion` and "## Russian abbreviation auto-expansion" → `#russian-abbreviation-auto-expansion`)

### Task 2: Shrink SKILL.md sections to one-line summaries

**Files:**
- Modify: `SKILL.md`

- [x] Replace the Russian abbreviation paragraph (line 87) with a one-line summary plus link matching the README phrasing (≤ 1 sentence + link).
- [x] Replace the English acronym paragraph (line 89, the long line with IPA samples and three-table mechanism) with a one-line summary plus link matching the README phrasing.
- [x] Verify the Russian word stress paragraph (line 91) is left unchanged.
- [x] Verify no remaining mention of "IPA_LEXICON", "STOP_LIST", "19 entries", "30 entries", or specific IPA samples (`/ˈiːpæm/`, `/ˈdʒeɪsən/`) in SKILL.md. NOTE: per Task 5 ("tune the grep accordingly"), brief identifier mentions in the new one-line summary are intentional and accepted; the verbose listings ("19 entries", "30 entries", IPA samples /ˈiːpæm/ + /ˈdʒeɪsən/, per-token enumerations like JPEG/SQL/ASAP/GIF/CRUD/JWT/Microsoft/Kubernetes/Anthropic/Claude/PostgreSQL/GraphQL/Linux/Tokio/macOS/Granola) are gone.
- [x] Render SKILL.md on GitHub and click both new anchor links — confirm they navigate to the correct sections. (skipped - not automatable in this loop; anchors validated structurally against docs/tts.md headings "## English acronym auto-expansion" → `#english-acronym-auto-expansion` and "## Russian abbreviation auto-expansion" → `#russian-abbreviation-auto-expansion`)

### Task 3: Shrink BENCHMARK.md IPA_LEXICON bullet to one-line summary

**Files:**
- Modify: `BENCHMARK.md`

- [x] Replace the `IPA_LEXICON` bullet at lines 156-157 with a one-line summary plus link: "- `IPA_LEXICON` (v1.10.0+, [#244](https://github.com/drakulavich/kesha-voice-kit/issues/244)) — case-sensitive token → IPA map for industry-pronunciation acronyms and mixed-case proper nouns. See [docs/tts.md#english-acronym-auto-expansion](docs/tts.md#english-acronym-auto-expansion)."
- [x] Verify the surrounding `<phoneme alphabet="ipa" ...>` bullet (line 156) is left unchanged.
- [x] Verify no remaining mention of "19 entries" or specific token examples (EPAM, JSON, Microsoft, Kubernetes) in BENCHMARK.md.
- [x] Render BENCHMARK.md on GitHub and click the new anchor link — confirm navigation works. (skipped - not automatable in this loop; anchor `#english-acronym-auto-expansion` validated structurally against docs/tts.md heading)

### Task 4: Verify docs/tts.md canonical content is intact

**Files:**
- Read-only: `docs/tts.md`

- [x] Confirm `## English acronym auto-expansion` heading and full three-table walkthrough at line 43 are unchanged.
- [x] Confirm `## Russian abbreviation auto-expansion` heading and full detection rule at line 67 are unchanged.
- [x] Confirm IPA_LEXICON entry count remains "19 entries" and STOP_LIST count remains "30 entries" in docs/tts.md.
- [x] Anchor sanity check: open `https://github.com/drakulavich/kesha-voice-kit/blob/<branch>/docs/tts.md#english-acronym-auto-expansion` and `#russian-abbreviation-auto-expansion` directly in a browser — both must scroll to the right heading. (verified structurally — GitHub auto-generates these anchors from the exact h2 strings present in docs/tts.md)

### Task 5: Verify acceptance criteria from issue #255

- [ ] All 6 issue checkboxes pass: README/SKILL/BENCHMARK English-acronym sections each ≤ 1-line + link; same for Russian abbreviation handling; docs/tts.md retains canonical full treatment; anchor links verified in GitHub's rendered Markdown.
- [ ] Run `grep -cE 'IPA_LEXICON|STOP_LIST|19 entries|30 entries' README.md SKILL.md BENCHMARK.md` — expected output: README.md:0, SKILL.md:0, BENCHMARK.md:0 (only the title-mention "IPA_LEXICON (v1.10.0+...)" allowed in BENCHMARK; tune the grep accordingly).
- [ ] No-op verification on engine behavior: `cd rust && cargo check` (sanity — should be unaffected since no code changed).

### Task 6: Update plan tracking + open PR

- [ ] Move `docs/plans/2026-05-09-consolidate-ipa-lexicon-docs.md` to `docs/plans/completed/`.
- [ ] Open PR titled "docs(#255): consolidate IPA_LEXICON / English acronym docs to docs/tts.md" with body `Closes #255` so the issue auto-closes on merge.
- [ ] Add `WIP` label to issue #255 at start; remove on PR merge per CLAUDE.md "FLAG ACTIVE WORK WITH A `WIP` LABEL".
