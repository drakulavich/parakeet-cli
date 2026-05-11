# SSML `<prosody rate>` — conservative v1 design

**Issue:** [#236](https://github.com/drakulavich/kesha-voice-kit/issues/236) (scoped subset).
**Status:** brainstorm complete; pre-spike.

## Problem

`rust/src/tts/ssml.rs` currently parses `<prosody>` and warns-once + strips. Callers cannot ask for slower / faster synthesis at the SSML layer. #236 proposes full prosody support (rate + pitch + volume) — this spec covers **rate only**, with conservative scoping that ships in days. Pitch and volume are deferred to a v2 (or never).

## Locked decisions (from brainstorming session 2026-05-10)

- **Conservative scope:** `<prosody rate>` honored only when it wraps the WHOLE utterance (immediate child of `<speak>`, no sibling content outside the prosody). Mid-utterance prosody warns-once + strips.
- **Engines:** Vosk-TTS (`ru-vosk-*`) + Kokoro (`en-*`) only. AVSpeech (`macos-*`) warns-once + strips.
- **Value forms:** SSML standard — named (`x-slow|slow|medium|fast|x-fast`), `N%`, `+N%`, `-N%`.
- **Mapping (W3C / Polly defaults):**

  | Named | Multiplier |
  |---|---|
  | `x-slow` | 0.5× |
  | `slow` | 0.75× |
  | `medium` | 1.0× |
  | `fast` | 1.25× |
  | `x-fast` | 1.5× |

  `N%` → `N/100`. `default` → 1.0× (alias for `medium`).

  Relative percent `+N%` / `-N%` was in the original scope but **deferred to v2**: `ssml-parser 0.1.4` strips the `+` sign before our code sees it (Display of `RateRange::Percentage(25)` is `"25%"`) and bails on `-N%` with a cryptic upstream message. Implementing relative percent requires either patching the upstream crate or pre-scanning the raw input for the rate attribute. v1 rejects relative-percent inputs at parse time with a clear error pointing users at absolute / named forms.
- **Clamp range:** `0.5..=2.0`. Values outside the range are clamped silently — past 2.0× both engines produce noticeably distorted output, below 0.5× prosody falls apart.
- **`--rate` × SSML interaction:** **multiply.** Final speed = `cli_rate * ssml_rate`, then clamp. Matches AWS Polly + Google TTS behaviour and composes naturally for the "I want everything 0.9× because the default is too fast" case.
- **Capability flag:** `tts.prosody_rate` (boolean), advertised whenever the `tts` cargo feature is on. Mirrors the existing `tts.ru_emphasis_marker` / `tts.en_acronym_expansion` shape.

## Architecture

### Pipeline

```
ssml::parse(input)
  └─ when ParsedElement::Prosody is direct child of <speak>
     and only has Text / Break / Spell siblings → Segment::ProsodyRate { rate, content }
     otherwise → warn_once("prosody-mid-utterance") + flatten content as Text segments
       (current behaviour for the warn+strip path is preserved)

tts::say(segments, voice, cli_rate)
  └─ for each Segment::ProsodyRate { rate, content }:
       effective_rate = clamp(cli_rate * rate, 0.5..=2.0)
       Vosk path:      Synth::set_speech_rate(effective_rate); synth_audio(content)
       Kokoro path:    pipeline.synth(content, speed: effective_rate)
       AVSpeech path:  warn_once("prosody-rate-non-vosk-kokoro") + flatten content
                       (sidecar protocol bump deferred to v2)
```

### Files touched

- **Modify** `rust/src/tts/ssml.rs`
  - Add `Segment::ProsodyRate { rate: f32, content: Vec<Segment> }` variant.
  - In the parser walker: detect whole-utterance `<prosody>` with a `rate` attribute, parse the value via a new `parse_rate_value(s: &str) -> Option<f32>` helper, emit `ProsodyRate` instead of warn+strip.
  - Mid-utterance `<prosody>` keeps the existing warn+strip path with a new bucket key `"prosody-mid-utterance"`.
- **Modify** `rust/src/tts/mod.rs::say` (and the per-engine dispatchers it calls)
  - Add a `ProsodyRate` arm that multiplies + clamps + dispatches per engine.
- **Modify** `rust/src/capabilities.rs`
  - Push `"tts.prosody_rate"` under `#[cfg(feature = "tts")]`.
- **Modify** `rust/src/tts/kokoro.rs` and `rust/src/tts/vosk.rs`
  - Surface a `set_rate(f32)` shim that the `say` dispatcher calls before synthesis. Vosk wraps `Synth::set_speech_rate`; Kokoro threads `speed` into the existing forward pass.
- **Tests** `rust/src/tts/ssml.rs` (unit) and `rust/tests/tts_prosody_rate.rs` (new integration)
  - Unit: rate-value parser (every named form, `N%` happy path, `+N%`/`-N%`, malformed input, edge clamping).
  - Unit: parser emits `ProsodyRate` for whole-utterance, warn+strip for mid-utterance.
  - Integration: round-trip via the warm `--stdin-loop` harness — synthesize the same fixture at `medium` and `fast`, assert the byte-length ratio is in `[0.7, 0.9]` (faster ≈ shorter audio).

## Output / public surface

- **No new CLI flag.** `<prosody rate>` ships through the existing `--ssml` path.
- **Capability flag** `tts.prosody_rate: true` advertised in `--capabilities-json` so the TS CLI gate can detect older engines and warn the user.
- **No public Rust API change.** `tts::say(segments, ...)` stays the same; `Segment::ProsodyRate` is a new variant in an existing public enum (additive — old code that exhaustively matches `Segment` will get a clippy `non_exhaustive` warning, which we ignore by using `_` arms in the dispatchers).

## Spike requirements (BLOCKING gate per CLAUDE.md)

Run before T2 implementation work commits to the design.

| Q | Question | Method |
|---|---|---|
| Q1 | Does Vosk's `Synth::set_speech_rate` mutate the synth across `synth_audio` calls? Is it `&mut self` or `&self`? | Read `vendor/vosk-tts/src/lib.rs` for the signature; if `&mut self`, our shim can use it directly. |
| Q2 | Does Vosk produce intelligible audio at the clamp endpoints (0.5×, 2.0×)? | Synthesize a 5-word fixture at each rate, listen via `audio-quality-check` agent (RMS + length sanity). |
| Q3 | Does Kokoro's `speed` parameter accept floats and produce audible-but-correct rate changes at endpoints? | Same as Q2, on Kokoro. |
| Q4 | Does Vosk's rate setting persist between calls on the same `Synth` instance, or reset per call? | Two-call test in the spike harness; affects whether we set rate per utterance defensively. |

**If Q1 fails (Vosk doesn't expose `set_speech_rate` on a callable shape):** ship Kokoro-only v1, add Vosk to a v2 follow-up; capability flag becomes `tts.prosody_rate.kokoro_only`. Decide at spike time.

**If Q2 / Q3 fails at endpoints:** narrow the clamp (e.g. 0.7–1.5×) and warn-once when SSML requests outside the safe range.

### Spike findings (2026-05-11)

Evidence: `/tmp/kesha-236-evidence/T1-spike.notes`. All gates pass; no pivots fired.

- **Q1 PASS, simplified.** Vosk's `Synth::synth_audio(..., speech_rate: Option<f32>, ...)` takes the rate AS A PER-CALL ARGUMENT (`rust/vendor/vosk-tts/src/synth.rs:125-156`); no stateful `set_speech_rate` method exists. Our existing wrapper `Vosk::infer(text, speaker_id, rate)` at `rust/src/tts/vosk.rs:48` already plumbs it. **Implication for T4:** drop the planned `Vosk::set_rate(&mut self, f32)` / `Kokoro::set_rate(&mut self, f32)` shims — multiply `cli_rate * ssml_rate` in `say()` and thread the result into the existing `rate` arg directly.
- **Q2/Q3 PASS.** Vosk and Kokoro both honor the rate within ~3-7% of theoretical at the clamp endpoints. Vosk: `0.5×→314,436 B`, `1.0×→157,764 B`, `2.0×→83,012 B` (ratios 1.99 / 0.526). Kokoro: `0.5×→722,468 B`, `1.0×→355,268 B`, `2.0×→189,668 B` (ratios 2.03 / 0.534). No silent no-op; clamps are safe.
- **Q4 MOOT.** Q1 confirmed rate is per-call, not stored on `Synth`. No reset-after-prosody hygiene needed.

## Out of scope (v2 candidates)

- Mid-utterance `<prosody>` (per-segment splitting + concat). Requires the boundary-cut spike from #236 to verify no audible click/pop.
- AVSpeech `<prosody rate>`. Sidecar protocol bump — pass `rate` over stdin / argv. Small but expands the test matrix.
- `<prosody pitch>`. Native on AVSpeech (`pitchMultiplier`); needs rubberband-rs (~2 MB binary + cross-compile risk) on Vosk + Kokoro. Probably warn+strip everywhere except AVSpeech.
- `<prosody volume>`. Free output-buffer gain, but rarely useful in TTS context (downstream consumers handle gain). Defer until someone asks.

## Acceptance criteria

- [ ] `<speak><prosody rate="slow">Hello</prosody></speak>` on `ru-vosk-m02` produces audibly slower output than `<speak>Hello</speak>` (RMS approximately equal, byte length ~33% longer).
- [ ] Same on `en-am_michael` for Kokoro.
- [ ] `<speak>Hi <prosody rate="fast">there</prosody></speak>` (mid-utterance) emits a single `prosody-mid-utterance` stderr warning and synthesizes the full text at the default rate.
- [ ] `<speak><prosody rate="fast">Hello</prosody></speak>` on a `macos-*` voice — DEFERRED. AVSpeech rejects all SSML up-front (`tts/mod.rs` early return) until the sidecar protocol bump in #141 lands, so the dedicated `prosody-rate-non-vosk-kokoro` warning bucket isn't implemented. Tracked alongside the AVSpeech SSML follow-up.
- [ ] `--rate 0.8` + `<prosody rate="slow">` produces audio with effective speed ~0.6× (clamped if outside 0.5–2.0×).
- [ ] `--capabilities-json` lists `tts.prosody_rate` when `tts` feature is on.
- [ ] `cargo clippy --all-targets -- -D warnings` clean for both `onnx,tts` and `coreml,tts,system_tts,system_diarize` matrices.

## CLAUDE.md applicability

- Touches `rust/` → engine release (v1.13.0).
- `cargo fmt` + `cargo clippy --all-targets -- -D warnings` mandatory.
- `audio-quality-check` agent on a 6-phrase corpus covering all 5 named rates + 2 percentage forms before publish.
- No model hash changes; no model fetches.
- Spike findings recorded in `/tmp/kesha-236-evidence/T1-spike.notes` and committed back into spec Section 5.
