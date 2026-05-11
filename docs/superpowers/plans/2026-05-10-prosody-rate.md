# SSML `<prosody rate>` — conservative v1 implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Honor SSML `<prosody rate>` when it wraps the whole utterance, on Vosk-TTS (`ru-vosk-*`) and Kokoro (`en-*`) voices. Mid-utterance prosody warns+strips. AVSpeech (`macos-*`) warns+strips.

**Architecture:** Parser detects whole-utterance `<prosody rate>` and emits a new `Segment::ProsodyRate { rate, content }` variant. Dispatcher in `tts/mod.rs::say` multiplies the SSML rate by the existing CLI `--rate`, clamps to `0.5..=2.0`, and threads it into Vosk's `Synth::set_speech_rate` / Kokoro's `speed` parameter.

**Tech Stack:** Rust (existing engine), `ssml-parser` crate (already a dep), `vosk-tts` (vendored), Kokoro pipeline.

**Spec:** `docs/superpowers/specs/2026-05-10-prosody-rate-design.md` (commit `591239f`).

---

## Task 1: Spike (BLOCKING gate before any kesha code is committed)

Per CLAUDE.md "VERIFY THIRD-PARTY MODEL FORMATS WITH A SPIKE" + the spike requirements in spec Section 5. Records findings to `/tmp/kesha-236-evidence/T1-spike.notes` and amends the spec if the design needs to pivot.

**Files:** Spike-only, no code committed. Spec amended if any spike Q forces a pivot.

- [x] **Step 1.1: Q1 — Vosk `set_speech_rate` signature**

```bash
mkdir -p /tmp/kesha-236-evidence
{
  echo "## Q1: Vosk Synth::set_speech_rate signature"
  grep -n 'set_speech_rate\|pub fn\|impl Synth' rust/vendor/vosk-tts/src/lib.rs | head -30
} > /tmp/kesha-236-evidence/T1-spike.notes
cat /tmp/kesha-236-evidence/T1-spike.notes
```

Look for: `pub fn set_speech_rate(&mut self, rate: f32)` (or similar). Record the exact signature.

**Pivot trigger:** If `set_speech_rate` is missing OR not callable on `&mut self`, abort and replan to **Kokoro-only v1**. Spec capability flag changes to `tts.prosody_rate.kokoro_only`; Vosk goes into a v2 follow-up issue.

- [x] **Step 1.2: Q2/Q3 — Endpoint quality on Vosk + Kokoro**

```bash
cd /Users/anton/Personal/repos/kesha-voice-kit
SPIKE=/tmp/kesha-236-spike && rm -rf "$SPIKE" && mkdir -p "$SPIKE"
# Q2: Vosk at clamp endpoints
kesha say --voice ru-vosk-m02 --rate 0.5 'Привет, как дела сегодня вечером.' --out "$SPIKE/vosk-0.5x.wav"
kesha say --voice ru-vosk-m02 --rate 1.0 'Привет, как дела сегодня вечером.' --out "$SPIKE/vosk-1.0x.wav"
kesha say --voice ru-vosk-m02 --rate 2.0 'Привет, как дела сегодня вечером.' --out "$SPIKE/vosk-2.0x.wav"
# Q3: Kokoro at clamp endpoints
kesha say --voice en-am_michael --rate 0.5 'The quick brown fox jumps over the lazy dog.' --out "$SPIKE/kokoro-0.5x.wav"
kesha say --voice en-am_michael --rate 1.0 'The quick brown fox jumps over the lazy dog.' --out "$SPIKE/kokoro-1.0x.wav"
kesha say --voice en-am_michael --rate 2.0 'The quick brown fox jumps over the lazy dog.' --out "$SPIKE/kokoro-2.0x.wav"
ls -lh "$SPIKE"
```

Run the **audio-quality-check** agent on the 6 WAVs. Append to spike notes:

```bash
{
  echo ""
  echo "## Q2/Q3: Endpoint quality"
  for f in "$SPIKE"/*.wav; do
    sox "$f" -n stat 2>&1 | grep -E 'Length|RMS' | head -2
    echo "  ($f)"
  done
} >> /tmp/kesha-236-evidence/T1-spike.notes
```

**Pivot trigger:** if 0.5× output on either engine sounds garbled (distortion, dropped phonemes), tighten the clamp lower bound (e.g. 0.7×) in the spec. If 2.0× output sounds garbled, tighten upper bound (e.g. 1.5×). Record the user's verdict.

- [x] **Step 1.3: Q4 — Vosk persistence across calls**

If Q1 confirmed `set_speech_rate(&mut self, ...)`, write a 5-line Rust test in a scratch file:

```rust
// /tmp/kesha-236-spike/persistence.rs
fn main() {
    use vosk_tts::{Synth, Model};
    let model = Model::new(Some("...path...")).unwrap();
    let synth = Synth::new(model).unwrap();
    let _audio_a = synth.synth_audio("привет"); // default rate
    synth.set_speech_rate(2.0);
    let audio_fast = synth.synth_audio("привет");
    let audio_again = synth.synth_audio("привет"); // does this stay at 2.0× or reset?
    println!("fast.len = {}, again.len = {}", audio_fast.len(), audio_again.len());
}
```

Or simpler: read the vosk-tts source for `set_speech_rate`'s implementation — does it mutate persistent state or set a per-call hint? Append finding.

**Implementation impact:** if rate is per-call, set it before every `synth_audio` (cheap). If persistent, set it on entry to `ProsodyRate` arm and reset to 1.0× on exit (so subsequent non-prosody segments aren't affected).

- [x] **Step 1.4: Amend spec Section 5 with findings**

```bash
cd /Users/anton/Personal/repos/kesha-voice-kit
# Append a "## Spike findings (2026-05-10)" subsection to spec Section 5,
# with Q1-Q4 verdicts inline. Commit the amendment.
git add docs/superpowers/specs/2026-05-10-prosody-rate-design.md
git commit -m "docs(#236): record prosody-rate spike findings (T1)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
git rev-parse HEAD > /tmp/kesha-236-evidence/T1-spike.sha
```

**STOP if any Q forces a pivot.** Replan affected tasks before proceeding.

---

## Task 2: SSML parser — `parse_rate_value` helper + unit tests

Pure-function helper. Test-first; no engine changes yet.

**Files:**
- Modify: `rust/src/tts/ssml.rs`

- [x] **Step 2.1: Write failing tests**

Add to the existing `#[cfg(test)] mod tests` block in `ssml.rs`:

```rust
#[test]
fn parse_rate_named_values() {
    assert_eq!(parse_rate_value("x-slow"), Some(0.5));
    assert_eq!(parse_rate_value("slow"), Some(0.75));
    assert_eq!(parse_rate_value("medium"), Some(1.0));
    assert_eq!(parse_rate_value("fast"), Some(1.25));
    assert_eq!(parse_rate_value("x-fast"), Some(1.5));
}

#[test]
fn parse_rate_percent_absolute() {
    assert_eq!(parse_rate_value("100%"), Some(1.0));
    assert_eq!(parse_rate_value("50%"), Some(0.5));
    assert_eq!(parse_rate_value("150%"), Some(1.5));
    assert_eq!(parse_rate_value("200%"), Some(2.0));
}

#[test]
fn parse_rate_percent_relative() {
    // +N% / -N% are relative to medium (1.0)
    assert_eq!(parse_rate_value("+25%"), Some(1.25));
    assert_eq!(parse_rate_value("-25%"), Some(0.75));
    assert_eq!(parse_rate_value("+0%"), Some(1.0));
}

#[test]
fn parse_rate_clamps_to_range() {
    // Spec clamp 0.5..=2.0
    assert_eq!(parse_rate_value("10%"), Some(0.5));   // clamped from 0.1
    assert_eq!(parse_rate_value("400%"), Some(2.0));  // clamped from 4.0
    assert_eq!(parse_rate_value("+500%"), Some(2.0));
    assert_eq!(parse_rate_value("-90%"), Some(0.5));
}

#[test]
fn parse_rate_malformed_returns_none() {
    assert_eq!(parse_rate_value(""), None);
    assert_eq!(parse_rate_value("abc"), None);
    assert_eq!(parse_rate_value("100"), None);   // missing %
    assert_eq!(parse_rate_value("--50%"), None); // double sign
    assert_eq!(parse_rate_value("xx-slow"), None);
}
```

```bash
cd /Users/anton/Personal/repos/kesha-voice-kit/rust
cargo test --no-default-features --features onnx,tts --bin kesha-engine parse_rate 2>&1 | tail -10
```
Expected: 5 fails (function not defined).

- [x] **Step 2.2: Implement `parse_rate_value`**

Add above `pub fn parse(input: &str) -> ...` in `ssml.rs`:

```rust
/// Parse an SSML `prosody rate` attribute value into a multiplier.
/// Supports W3C named values, absolute `N%`, and relative `+N%` / `-N%`.
/// Clamps the result to 0.5..=2.0; returns None on malformed input.
fn parse_rate_value(s: &str) -> Option<f32> {
    let s = s.trim();
    let mult = match s {
        "x-slow" => 0.5_f32,
        "slow" => 0.75,
        "medium" => 1.0,
        "fast" => 1.25,
        "x-fast" => 1.5,
        _ => {
            // Percent forms: "N%", "+N%", "-N%"
            let pct = s.strip_suffix('%')?;
            if let Some(rest) = pct.strip_prefix('+') {
                let n: f32 = rest.parse().ok()?;
                1.0 + n / 100.0
            } else if let Some(rest) = pct.strip_prefix('-') {
                let n: f32 = rest.parse().ok()?;
                1.0 - n / 100.0
            } else {
                let n: f32 = pct.parse().ok()?;
                n / 100.0
            }
        }
    };
    Some(mult.clamp(0.5, 2.0))
}
```

```bash
cargo test --no-default-features --features onnx,tts --bin kesha-engine parse_rate 2>&1 | tail -3
```
Expected: 5 pass.

- [x] **Step 2.3: Commit**

```bash
cd /Users/anton/Personal/repos/kesha-voice-kit
git add rust/src/tts/ssml.rs
git commit -m "feat(#236): parse_rate_value helper for SSML <prosody rate>

Pure function; W3C named values + N% absolute + +/-N% relative;
clamps result to 0.5..=2.0. Returns None on malformed input.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
git rev-parse HEAD > /tmp/kesha-236-evidence/T2-rate-parser.sha
```

---

## Task 3: Add `Segment::ProsodyRate` variant + parser dispatch

Extends the public `Segment` enum and rewires the SSML walker to emit `ProsodyRate` for whole-utterance `<prosody rate>`. Mid-utterance keeps the existing warn+strip path with a new bucket key.

**Files:**
- Modify: `rust/src/tts/ssml.rs`

- [x] **Step 3.1: Add the variant**

In the `Segment` enum (after the `Emphasis` variant):

```rust
/// SSML `<prosody rate>` content where the prosody wraps the entire
/// utterance (immediate child of `<speak>`, no sibling content). The
/// dispatcher multiplies `rate` by the CLI `--rate` and threads the
/// result into the per-engine speed knob. Mid-utterance prosody is
/// warned+stripped at parse time and never reaches a `ProsodyRate`
/// segment.
ProsodyRate { rate: f32, content: Vec<Segment> },
```

- [x] **Step 3.2: Walker — detect whole-utterance prosody**

The existing `<prosody>` arm in the walker currently emits a warn-once + flattens. Replace it with the whole-utterance check. Pseudocode (adjust to the actual ssml-parser API used in the file):

```rust
// In the walker's element-dispatch arm:
ParsedElement::Prosody(p) => {
    if is_whole_utterance(&context) {
        if let Some(rate) = p.rate.as_deref().and_then(parse_rate_value) {
            // Recursively walk the prosody's children into a Vec<Segment>.
            let inner = walk_children(&p.children);
            segments.push(Segment::ProsodyRate { rate, content: inner });
            return;
        }
        // Whole-utterance prosody but missing/unparseable rate → warn+strip
        warn::warn_once(
            "prosody-no-supported-attr",
            "<prosody> without a rate= attribute is not supported (pitch/volume \
             are scoped to a follow-up); stripping",
        );
    } else {
        warn::warn_once(
            "prosody-mid-utterance",
            "<prosody> mid-utterance is not yet supported (whole-utterance only); \
             stripping rate, pitch, and volume",
        );
    }
    // Flatten children as Text segments either way.
    walk_children_into(segments, &p.children);
}
```

`is_whole_utterance` is a new helper: returns true when the current ParsedElement is the immediate child of `<speak>` and the `<speak>` has no other sibling content (text, break, emphasis, etc.) outside this prosody. Implementation detail depends on ssml-parser's tree shape — read the existing walker for patterns.

- [x] **Step 3.3: Tests for whole-utterance vs mid-utterance**

```rust
#[test]
fn prosody_whole_utterance_emits_prosody_rate() {
    let segs = parse(r#"<speak><prosody rate="fast">Hello</prosody></speak>"#).unwrap();
    assert_eq!(segs.len(), 1);
    match &segs[0] {
        Segment::ProsodyRate { rate, content } => {
            assert!((rate - 1.25).abs() < 1e-6);
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
    // No ProsodyRate emitted; everything is plain Text.
    assert!(!segs.iter().any(|s| matches!(s, Segment::ProsodyRate { .. })));
    let combined: String = segs.iter()
        .filter_map(|s| if let Segment::Text(t) = s { Some(t.as_str()) } else { None })
        .collect::<Vec<_>>().join(" ");
    assert!(combined.contains("Hi"));
    assert!(combined.contains("there"));
    assert!(combined.contains("bye"));
}
```

```bash
cd /Users/anton/Personal/repos/kesha-voice-kit/rust
cargo test --no-default-features --features onnx,tts --bin kesha-engine prosody 2>&1 | tail -5
```
Expected: 2 pass.

Update existing `<prosody>` warn+strip test (line ~356 — `parse(r#"<speak>Hi <prosody rate="fast">there</prosody></speak>"#)`) to assert the new bucket key fires.

- [x] **Step 3.4: Run full ssml test suite + clippy**

```bash
cargo test --no-default-features --features onnx,tts --bin kesha-engine ssml 2>&1 | tail -3
cargo clippy --all-targets --no-default-features --features onnx,tts -- -D warnings 2>&1 | tail -3
```
Expected: all pass, clippy clean. (You'll get a `non_exhaustive` warning on every `match Segment` site that doesn't yet cover `ProsodyRate` — Task 4 fixes those, but for now add a `Segment::ProsodyRate { .. } => unimplemented!("Task 4")` arm so the code compiles.)

- [x] **Step 3.5: Commit**

```bash
cd /Users/anton/Personal/repos/kesha-voice-kit
git add rust/src/tts/ssml.rs
git commit -m "feat(#236): Segment::ProsodyRate + whole-utterance parser dispatch

Whole-utterance <prosody rate> emits Segment::ProsodyRate { rate, content };
mid-utterance warns 'prosody-mid-utterance' and flattens. Adds the new
variant to the public Segment enum (additive — exhaustive matches in
the synth dispatchers will get unimplemented! arms until T4 wires them).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
git rev-parse HEAD > /tmp/kesha-236-evidence/T3-segment-variant.sha
```

---

## Task 4: Engine dispatchers — Vosk + Kokoro + AVSpeech

Wire `ProsodyRate` through to `Synth::set_speech_rate` (Vosk) and the Kokoro speed parameter, with `--rate` multiplication and clamp. AVSpeech gets the warn+strip arm.

**Files:**
- Modify: `rust/src/tts/mod.rs` (the `say()` dispatcher)
- Modify: `rust/src/tts/vosk.rs` (add `set_rate(f32)` shim)
- Modify: `rust/src/tts/kokoro.rs` (thread `speed` into the synth call)

- [x] **Step 4.1: Vosk shim**

Add to `rust/src/tts/vosk.rs`:

```rust
impl Vosk {
    /// Set the per-utterance speech rate before the next `synth_audio` call.
    /// Wraps `vosk_tts::Synth::set_speech_rate`. The rate is clamped by the
    /// caller (tts/mod.rs::say) — this fn is a thin pass-through.
    pub fn set_rate(&mut self, rate: f32) {
        self.synth.set_speech_rate(rate);
    }
}
```

(If T1 spike Q1 found a different signature — e.g. the rate field needs setting on `Synth::synth_audio` directly — adjust the shim accordingly.)

- [x] **Step 4.2: Kokoro speed plumbing**

In `rust/src/tts/kokoro.rs`, locate the existing `speed` parameter handed into the ONNX session (post-#207 the Kokoro pipeline already accepts `speed: f32`; we just expose it). Add or repurpose a setter:

```rust
impl Kokoro {
    /// Set the per-utterance synthesis speed. Threaded into the ONNX
    /// `speed` input on the next `synthesize(...)` call.
    pub fn set_rate(&mut self, rate: f32) {
        self.next_speed = rate;
    }
}
```

(Adjust to whatever per-call state struct already exists. If Kokoro's synth fn takes `speed` as an arg, just plumb it through `say()`'s call site.)

- [x] **Step 4.3: `say()` dispatcher**

In `rust/src/tts/mod.rs::say`, where the segment match handles each `Segment` variant, add:

```rust
ssml::Segment::ProsodyRate { rate, content } => {
    let effective = (cli_rate * rate).clamp(0.5, 2.0);
    match voice_kind {
        VoiceKind::VoskRu => {
            vosk.set_rate(effective);
            // Recursively synthesize each inner segment with the rate applied.
            for inner in content {
                synth_segment(inner, ...);
            }
            // Reset to default for subsequent non-prosody segments. (Skip
            // this if T1 Q4 confirmed Vosk's rate is per-call, not persistent.)
            vosk.set_rate(cli_rate);
        }
        VoiceKind::KokoroEn => {
            kokoro.set_rate(effective);
            for inner in content {
                synth_segment(inner, ...);
            }
            kokoro.set_rate(cli_rate);
        }
        VoiceKind::AvSpeechMacOS => {
            warn::warn_once(
                "prosody-rate-non-vosk-kokoro",
                "<prosody rate> is not yet supported on macos-* voices; \
                 stripping rate, synthesizing content at default speed",
            );
            for inner in content {
                synth_segment(inner, ...);
            }
        }
    }
}
```

(Adapt to the actual VoiceKind / dispatcher shape in `tts/mod.rs`.)

- [x] **Step 4.4: Tests + clippy**

```bash
cd /Users/anton/Personal/repos/kesha-voice-kit/rust
cargo test --no-default-features --features onnx,tts --bin kesha-engine 2>&1 | tail -3
cargo clippy --all-targets --no-default-features --features onnx,tts -- -D warnings 2>&1 | tail -3
cargo fmt --check && echo fmt-clean
```

- [x] **Step 4.5: Commit**

```bash
cd /Users/anton/Personal/repos/kesha-voice-kit
git add rust/src/tts/mod.rs rust/src/tts/vosk.rs rust/src/tts/kokoro.rs
git commit -m "feat(#236): wire ProsodyRate through Vosk + Kokoro dispatchers

Vosk.set_rate / Kokoro.set_rate shims accept the multiplied + clamped
rate (cli_rate * ssml_rate, clamped to 0.5..=2.0). AVSpeech path
warns 'prosody-rate-non-vosk-kokoro' and flattens to default-rate
synth.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
git rev-parse HEAD > /tmp/kesha-236-evidence/T4-dispatchers.sha
```

---

## Task 5: Capability flag + integration test

**Files:**
- Modify: `rust/src/capabilities.rs`
- Create: `rust/tests/tts_prosody_rate.rs`

- [x] **Step 5.1: Capability flag**

In `capabilities.rs`, after the existing `tts.ru_emphasis_marker` push:

```rust
#[cfg(feature = "tts")]
features.push("tts.prosody_rate");
```

- [x] **Step 5.2: Integration test — duration ratio**

```rust
//! Closes #236. End-to-end check that `<prosody rate>` actually changes
//! synthesis duration on the Vosk + Kokoro paths. Uses the warm
//! --stdin-loop harness so we synthesize the same fixture at three
//! different rates and compare WAV durations.
//!
//! Skips at runtime when the relevant TTS voice isn't installed; the
//! installer flow is exercised by separate unit tests.

#![cfg(feature = "tts")]

use std::path::PathBuf;
use std::process::Command;

fn engine_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kesha-engine"))
}

fn say_ssml(voice: &str, ssml: &str, out: &PathBuf) -> bool {
    Command::new(engine_binary())
        .args([
            "say", "--voice", voice, "--ssml", "--out", out.to_str().unwrap(), ssml,
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn wav_byte_len(path: &PathBuf) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

#[test]
fn vosk_prosody_rate_fast_shorter_than_medium() {
    let tmp = tempfile::Builder::new().prefix("kesha-prosody-").tempdir().unwrap();
    let medium = tmp.path().join("medium.wav");
    let fast = tmp.path().join("fast.wav");
    let text_medium = r#"<speak>Привет, как дела сегодня вечером.</speak>"#;
    let text_fast = r#"<speak><prosody rate="fast">Привет, как дела сегодня вечером.</prosody></speak>"#;
    if !say_ssml("ru-vosk-m02", text_medium, &medium) {
        eprintln!("skipping: ru-vosk-m02 not installed");
        return;
    }
    say_ssml("ru-vosk-m02", text_fast, &fast);
    let m = wav_byte_len(&medium) as f32;
    let f = wav_byte_len(&fast) as f32;
    let ratio = f / m;
    // fast=1.25× → expect ~80% the byte length of medium, with slack
    // for header overhead and synth nondeterminism.
    assert!(
        (0.7..=0.9).contains(&ratio),
        "expected fast/medium byte ratio in 0.7..=0.9, got {ratio:.2}"
    );
}

#[test]
fn kokoro_prosody_rate_slow_longer_than_medium() {
    let tmp = tempfile::Builder::new().prefix("kesha-prosody-").tempdir().unwrap();
    let medium = tmp.path().join("medium.wav");
    let slow = tmp.path().join("slow.wav");
    let text_medium = r#"<speak>The quick brown fox jumps over the lazy dog.</speak>"#;
    let text_slow = r#"<speak><prosody rate="slow">The quick brown fox jumps over the lazy dog.</prosody></speak>"#;
    if !say_ssml("en-am_michael", text_medium, &medium) {
        eprintln!("skipping: en-am_michael not installed");
        return;
    }
    say_ssml("en-am_michael", text_slow, &slow);
    let m = wav_byte_len(&medium) as f32;
    let s = wav_byte_len(&slow) as f32;
    let ratio = s / m;
    // slow=0.75× → expect ~133% the byte length of medium.
    assert!(
        (1.2..=1.5).contains(&ratio),
        "expected slow/medium byte ratio in 1.2..=1.5, got {ratio:.2}"
    );
}

#[test]
fn macos_prosody_rate_warns_and_synthesizes() {
    if !cfg!(target_os = "macos") {
        return;
    }
    let tmp = tempfile::Builder::new().prefix("kesha-prosody-").tempdir().unwrap();
    let out = tmp.path().join("macos.wav");
    let text = r#"<speak><prosody rate="fast">Hello there.</prosody></speak>"#;
    let result = Command::new(engine_binary())
        .args(["say", "--voice", "macos-com.apple.voice.compact.en-US.Samantha", "--ssml",
               "--out", out.to_str().unwrap(), text])
        .output()
        .unwrap();
    if !result.status.success() {
        eprintln!("skipping: macos voice not available");
        return;
    }
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("prosody-rate-non-vosk-kokoro") || stderr.contains("not yet supported on macos-*"),
            "expected warn line; got stderr: {stderr}");
    assert!(wav_byte_len(&out) > 1024, "output WAV should be non-empty");
}
```

- [x] **Step 5.3: Run + commit**

```bash
cd /Users/anton/Personal/repos/kesha-voice-kit/rust
cargo test --release --no-default-features --features onnx,tts --test tts_prosody_rate 2>&1 | tail -10
```

Expected: 3 tests pass (or skip lines if voices not installed locally).

```bash
cd /Users/anton/Personal/repos/kesha-voice-kit
git add rust/src/capabilities.rs rust/tests/tts_prosody_rate.rs
git commit -m "feat(#236): tts.prosody_rate capability + integration test

Capability flag advertised when the tts feature is on. Integration
test self-fixtures: synthesizes the same SSML at medium vs fast
(Vosk) and medium vs slow (Kokoro), asserts byte-length ratios fall
within the expected ranges per the W3C named-rate multipliers.
macos-* path asserts the warn+strip behavior.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
git rev-parse HEAD > /tmp/kesha-236-evidence/T5-capability-tests.sha
```

---

## Task 6: Documentation

**Files:**
- Modify: `README.md`
- Modify: `SKILL.md`
- Modify: `docs/tts.md`
- Modify: `CHANGELOG.md`

- [x] **Step 6.1: README.md — add a `<prosody rate>` example**

After the existing Russian word stress block:

```markdown
**Speech rate via SSML** (`ru-vosk-*` and `en-*` voices):

```bash
kesha say --voice ru-vosk-m02 --ssml \
  '<speak><prosody rate="slow">Привет, как дела.</prosody></speak>' --out slow.wav

kesha say --voice en-am_michael --ssml \
  '<speak><prosody rate="x-fast">Read this fast.</prosody></speak>' --out fast.wav
```

Honored when `<prosody rate>` wraps the whole utterance. Mid-utterance prosody warns and synthesizes at default rate (whole-segment-only is a v1 limitation; mid-utterance support tracked in [#236](https://github.com/drakulavich/kesha-voice-kit/issues/236)). `--rate` and `<prosody rate>` compose multiplicatively. Range clamped to 0.5×–2.0×.
```

- [x] **Step 6.2: SKILL.md + docs/tts.md — same pattern**

Append a one-paragraph `<prosody rate>` block to both, mirroring the Russian-emphasis section.

- [x] **Step 6.3: CHANGELOG.md**

After `## [Unreleased]`:

```markdown
## [1.13.0] (unreleased)

Engine release. Adds SSML `<prosody rate>` support for Vosk + Kokoro voices.

### Added
- **SSML `<prosody rate>` honored on `ru-vosk-*` and `en-*` voices** when it wraps the whole utterance — `<speak><prosody rate="fast">…</prosody></speak>`. Supports W3C named values (`x-slow`/`slow`/`medium`/`fast`/`x-fast`), absolute `N%`, and relative `+N%`/`-N%`. Result is multiplied by the existing `--rate` flag and clamped to 0.5×–2.0×. New capability flag `tts.prosody_rate`. Closes [#236](https://github.com/drakulavich/kesha-voice-kit/issues/236) (rate-only conservative scope; pitch + volume deferred).

### Notes
- Mid-utterance `<prosody>` (anything not whole-utterance) emits a `prosody-mid-utterance` stderr warning and synthesizes the content at default rate. Per-segment splitting is a v2 follow-up — requires the boundary-cut spike from #236.
- AVSpeech (`macos-*`) voices warn `prosody-rate-non-vosk-kokoro` and synthesize at default rate; sidecar protocol bump for native AVSpeech rate is also a v2 follow-up.
```

- [x] **Step 6.4: Commit**

```bash
git add README.md SKILL.md docs/tts.md CHANGELOG.md
git commit -m "docs(#236): document <prosody rate> for Vosk + Kokoro voices

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
git rev-parse HEAD > /tmp/kesha-236-evidence/T6-docs.sha
```

---

## Task 7: Lockstep version bump to v1.13.0

**Files:**
- Modify: `package.json`
- Modify: `rust/Cargo.toml`
- Modify: `rust/Cargo.lock`

- [x] **Step 7.1: Bumps**

```bash
cd /Users/anton/Personal/repos/kesha-voice-kit
sed -i '' 's/"version": "1.12.0"/"version": "1.13.0"/g' package.json
sed -i '' 's/^version = "1.12.0"/version = "1.13.0"/' rust/Cargo.toml
cd rust && cargo check --no-default-features --features onnx,tts 2>&1 | tail -3
```

- [x] **Step 7.2: Verify version + tests**

```bash
cd rust
cargo build --release --no-default-features --features onnx,tts 2>&1 | tail -3
./target/release/kesha-engine --version  # → kesha-engine 1.13.0
cargo test --no-default-features --features onnx,tts --bin kesha-engine 2>&1 | tail -3
cargo clippy --all-targets --no-default-features --features onnx,tts -- -D warnings 2>&1 | tail -3
cargo fmt --check && echo fmt-clean
cd .. && bunx tsc --noEmit && echo tsc-clean
bun test --exclude tests/integration/ 2>&1 | tail -3
```

- [x] **Step 7.3: Commit**

```bash
git add package.json rust/Cargo.toml rust/Cargo.lock
git commit -m "chore(release): bump engine + CLI to v1.13.0 for #236

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
git rev-parse HEAD > /tmp/kesha-236-evidence/T7-bump.sha
```

---

## Task 8: STOP — manual release runbook (gate on user)

Per CLAUDE.md "RELEASE PROCESS". Do NOT auto-tag, auto-build, or auto-publish.

- [x] **Step 8.1: Push + open release PR**

Branch is named `release/v1.13.0` so `integration-tests` skips on the chicken-and-egg version. If currently on a different branch name, rename via `gh api -X POST '/repos/.../branches/<old>/rename' -f new_name=release/v1.13.0`.

```bash
gh pr create -R drakulavich/kesha-voice-kit \
  --base main --head release/v1.13.0 \
  --title "feat(tts): SSML <prosody rate> — v1.13.0 release (closes #236)" \
  --body "Closes #236 (rate-only conservative scope). ..."
```

- [x] **Step 8.2: STOP — wait for user authorization**

Do NOT auto-execute without explicit go-ahead:
- Squash-merge PR
- Tag `v1.13.0` and push
- Trigger / wait for build-engine.yml
- Author + apply release notes
- `gh release edit v1.13.0 --draft=false`
- Independent v1.13.0 validation (download released darwin + linux binaries, exercise `<prosody rate>` end-to-end)
- `npm publish --access public`

The user-facing prompt should look like:
> "PR opened, all gates green. Ready to drive the v1.13.0 release runbook (squash → tag → build → notes → publish → validate → npm publish). The npm publish step is irreversible. Want me to proceed?"

---

## Self-review

**1. Spec coverage:**

| Spec section | Implemented in |
|---|---|
| §Architecture / pipeline | T3 (parser dispatch), T4 (engine arms) |
| §Output / public surface | T3 (Segment::ProsodyRate variant), T5 (capability flag) |
| §Spike findings | T1 |
| §Acceptance criteria | T2 (parser tests), T3 (whole/mid-utterance tests), T5 (integration tests) |
| §Out of scope | T4 (AVSpeech warn arm) + T6 (docs flagging the v2 follow-up) |

**2. Placeholder scan:** No "TBD" / "implement later". T1 spike step is intentionally measurement-only. T8 release runbook intentionally gates on the user.

**3. Type consistency:** `Segment::ProsodyRate { rate: f32, content: Vec<Segment> }` defined in T3, consumed in T4, asserted in T3 + T5 tests. `parse_rate_value(&str) -> Option<f32>` defined in T2, used in T3.

**4. CLAUDE.md gates verified:**
- ✓ `cargo clippy --all-targets -- -D warnings` runs at T3.4, T4.4, T7.2.
- ✓ `cargo fmt --check` runs at T7.2.
- ✓ `bun test && bunx tsc --noEmit` runs at T7.2.
- ✓ Spike-mandatory before plan: T1 BLOCKS subsequent tasks; spec amendment commit (T1.4) records the findings.
- ✓ Independent v1.13.0 validation gated behind user authorization at T8.

---

## Execution

Plan complete. Two execution options:

1. **Subagent-Driven** (recommended) — fresh subagent per task, two-stage review (spec compliance + code quality) between tasks. Use `superpowers:subagent-driven-development`.
2. **Inline Execution** — execute tasks in this session. Use `superpowers:executing-plans`.

Which approach?
