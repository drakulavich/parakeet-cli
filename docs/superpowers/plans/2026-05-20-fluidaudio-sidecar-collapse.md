# FluidAudio Sidecar Collapse Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the `kesha-kokoro` and `kesha-diarize` Swift sidecars with native calls into a forked `fluidaudio-rs` crate, deleting both SwiftPM packages, their `build.rs` blocks, and their release artifacts.

**Architecture:** Fork `fluidaudio-rs` (already linked for CoreML ASR) to add a native Kokoro TTS binding and a model-path diarization variant; kesha consumes the fork via a git dependency and calls Rust functions instead of spawning subprocesses. Diarization keeps kesha's pinned-hash model governance by passing the pre-staged model dir; Kokoro keeps its existing FluidAudio auto-download.

**Tech Stack:** Rust (kesha-engine + the fork's Rust FFI), Swift 6 (`@_cdecl` bridge + FluidAudio framework), SwiftPM, `cargo nextest`, `hound` (WAV decode).

**Companion spec:** `docs/superpowers/specs/2026-05-20-fluidaudio-sidecar-collapse-design.md`

**Prerequisite (separate, in flight):** PR 1 on branch `chore/cargo-dep-bumps` bumps `fluidaudio-rs 0.1 → 0.14.1` (crates.io) for ASR and drops the temp-WAV shim. This plan (PR 2) supersedes that dep by pointing at the fork's git rev. Land PR 1 first so the 0.14 ASR API is already de-risked.

---

## File Structure

**Fork repo (`drakulavich/fluidaudio-rs`, branch off `v0.14.1` tag or `main`):**
- Modify: `Package.swift` — bump `FluidAudio` pin `0.14.1 → 0.14.5`.
- Modify: `swift/FluidAudioBridge.swift` — add Kokoro `@_cdecl` fns + a model-path diarize `@_cdecl` fn.
- Modify: `src/ffi/bridge.rs` — add `extern "C"` decls + safe wrappers.
- Modify: `src/lib.rs` — add `init_kokoro`, `synthesize_kokoro`, `diarize_file_with_models` public methods + `KokoroAudio` result type.
- Create: `examples/kokoro.rs` — runnable smoke for the new TTS path.

**kesha repo (`drakulavich/kesha-voice-kit`, branch `feat/fluidaudio-native`):**
- Create: `rust/src/fluid_stdout.rs` — shared `with_silenced_stdout` (moved from `backend/fluidaudio.rs`) + a one-shot variant for the Kokoro/diarize call sites.
- Modify: `rust/Cargo.toml` — `fluidaudio-rs` → fork git rev; `system_kokoro`/`system_diarize` features pull `dep:fluidaudio-rs` + `dep:libc`.
- Rewrite: `rust/src/tts/fluid_kokoro.rs` — crate call instead of `Command`.
- Modify: `rust/src/transcribe/diarize.rs` — swap the `Command`-spawn (`run()`, ~lines 88–199) for `diarize_file_with_models`; keep coverage-validation + speaker-merge.
- Modify: `rust/build.rs` — delete the `system_kokoro` (~46–90) and `system_diarize` (~133–184) swift-build blocks.
- Delete: `swift/kesha-kokoro/`, `swift/kesha-diarize/`.
- Modify: `.github/workflows/build-engine.yml` — delete the `kesha-kokoro-darwin-arm64` / `kesha-diarize-darwin-arm64` build+upload steps.
- Tests: `rust/src/tts/fluid_kokoro.rs` (#[cfg(test)]), `rust/tests/diarize_*.rs`, the macOS smoke in `build-engine.yml`.

---

## Phase 0 — Validation spikes (GATE: all three must pass before any Phase 1+ commit)

Throwaway. Artifacts in `/tmp/<name>-spike/`, deleted after findings recorded in this plan's "Spike results" appendix. If a spike fails, STOP and revise the spec — do not proceed.

### Task 0.1: Spike — single-binary Swift link coexistence (highest risk)

**Files:** throwaway clone of the fork at `/tmp/fluidaudio-fork-spike/`.

- [ ] **Step 1: Clone + branch the fork**

```bash
gh repo fork FluidInference/fluidaudio-rs --clone=true --fork-name fluidaudio-rs 2>/dev/null || true
git clone https://github.com/drakulavich/fluidaudio-rs /tmp/fluidaudio-fork-spike
cd /tmp/fluidaudio-fork-spike && git checkout -b spike/link v0.14.1 2>/dev/null || git checkout -b spike/link
```

- [ ] **Step 2: Build the unmodified crate with all features on macos-14 arm64**

Run: `cd /tmp/fluidaudio-fork-spike && cargo build --features asr,diarization,tts`
Expected: PASS — confirms baseline `swift build` + framework linking works on this toolchain (needs Xcode + swift). If it fails for missing toolchain, that's an environment fix, not a design failure.

- [ ] **Step 3: Confirm our coreml ASR usage links against it**

In `/tmp/fluidaudio-fork-spike`, run `cargo run --example transcribe -- <a 16kHz wav from kesha rust/tests/fixtures>`.
Expected: prints a transcript (downloads ASR model on first run). Confirms ASR + the crate's bridge link & run in one binary.

- [ ] **Step 4: Record finding** in the "Spike results" appendix: does `cargo build --features asr,diarization,tts` link cleanly in one binary? (Pass criterion for the whole effort.)

### Task 0.2: Spike — KokoroTtsManager returns WAV bytes usable over FFI

**Files:** `/tmp/fluidaudio-fork-spike/swift/probe_kokoro.swift` (throwaway).

- [ ] **Step 1: Confirm the Swift API matches our sidecar's usage**

Our `swift/kesha-kokoro/Sources/kesha-kokoro/main.swift` already calls (against `FluidAudio@0.14.5`):
```swift
let manager = KokoroTtsManager(defaultVoice: voice)
try await manager.initialize(preloadVoices: [voice])
let wav: Data = try await manager.synthesize(text: trimmed, voice: voice, voiceSpeed: clampedSpeed)
```
- [ ] **Step 2: Verify `synthesize` exists with this signature in FluidAudio 0.14.5** (the fork's bumped pin):

Run: `cd /tmp/fluidaudio-fork-spike && grep -rn "func synthesize\|class KokoroTtsManager\|func initialize" .build/checkouts/FluidAudio/Sources 2>/dev/null | head`
Expected: finds `KokoroTtsManager.synthesize(text:voice:voiceSpeed:) -> Data` (or close). Record the exact signature.

- [ ] **Step 3: Record finding** — exact `KokoroTtsManager` method names/signatures in 0.14.5, and confirm `synthesize` returns a complete WAV `Data` (24 kHz mono f32). This is the source of truth for Task 1.2's Swift code.

### Task 0.3: Spike — DiarizerManager loads from an explicit model dir (no download)

**Files:** reference only — `swift/kesha-diarize/Sources/kesha-diarize/main.swift`.

- [ ] **Step 1: Extract the model-loading call our sidecar already uses**

Run: `grep -n "Sortformer\|DiarizerManager\|modelPath\|loadModels\|init(config" swift/kesha-diarize/Sources/kesha-diarize/main.swift`
Expected: shows how we instantiate the diarizer from a local model path with `SortformerConfig.balancedV2`. Record the exact init + diarize calls.

- [ ] **Step 2: Confirm the same calls exist in FluidAudio 0.14.5**

Run: `grep -rn "DiarizerManager\|SortformerConfig\|func diarize" /tmp/fluidaudio-fork-spike/.build/checkouts/FluidAudio/Sources 2>/dev/null | head`
Expected: the model-path init path exists. Record the exact signature — source of truth for Task 2.1's Swift code.

- [ ] **Step 3: Record finding** + delete spikes: `rm -rf /tmp/fluidaudio-fork-spike` (after recording).

---

## Phase 1 — Fork: native Kokoro TTS binding

Work in a clone of `drakulavich/fluidaudio-rs` on branch `feat/kokoro-binding`. Mirror the existing `fluidaudio_diarize_file` bridge triple. Signatures below are the **plan baseline**; reconcile against Task 0.2's recorded findings before committing each task.

### Task 1.1: Bump FluidAudio pin to 0.14.5

**Files:** Modify `Package.swift`, `Package.resolved`.

- [ ] **Step 1: Edit the pin**

In `Package.swift`: `.package(url: "https://github.com/FluidInference/FluidAudio.git", exact: "0.14.1")` → `exact: "0.14.5"`.

- [ ] **Step 2: Resolve + build**

Run: `swift package resolve && cargo build --features asr,diarization,tts`
Expected: PASS against FluidAudio 0.14.5.

- [ ] **Step 3: Commit**

```bash
git add Package.swift Package.resolved Cargo.toml
git commit -m "deps: bump FluidAudio 0.14.1 -> 0.14.5"
```

### Task 1.2: Swift bridge — Kokoro `@_cdecl` functions

**Files:** Modify `swift/FluidAudioBridge.swift`.

- [ ] **Step 1: Add the bridge functions** (port from `kesha-kokoro/main.swift`; the bridge holds a `KokoroTtsManager?` on the bridge object alongside the existing managers). Append, mirroring `fluidaudio_initialize_diarization` / `fluidaudio_diarize_file`:

```swift
@_cdecl("fluidaudio_initialize_kokoro")
public func fluidaudio_initialize_kokoro(_ ptr: UnsafeMutableRawPointer?, _ defaultVoice: UnsafePointer<CChar>?) -> Int32 {
    guard let ptr = ptr, let bridge = Unmanaged<FluidAudioBridge>.fromOpaque(ptr).takeUnretainedValue() as FluidAudioBridge? else { return -1 }
    let voice = defaultVoice.map { String(cString: $0) } ?? "am_michael"
    let sem = DispatchSemaphore(value: 0)
    var rc: Int32 = 0
    Task {
        do {
            let mgr = KokoroTtsManager(defaultVoice: voice)
            try await mgr.initialize(preloadVoices: [voice])
            bridge.kokoro = mgr
        } catch { rc = 1 }
        sem.signal()
    }
    sem.wait()
    return rc
}

/// Synthesize `text` with `voice` at `speed`; returns a complete WAV byte buffer
/// (24 kHz mono f32) via out-params. Caller frees with `fluidaudio_free_bytes`.
@_cdecl("fluidaudio_kokoro_synthesize")
public func fluidaudio_kokoro_synthesize(
    _ ptr: UnsafeMutableRawPointer?,
    _ text: UnsafePointer<CChar>?,
    _ voice: UnsafePointer<CChar>?,
    _ speed: Float,
    _ outBytes: UnsafeMutablePointer<UnsafeMutablePointer<UInt8>?>?,
    _ outLen: UnsafeMutablePointer<UInt>?
) -> Int32 {
    guard let ptr = ptr,
          let bridge = Unmanaged<FluidAudioBridge>.fromOpaque(ptr).takeUnretainedValue() as FluidAudioBridge?,
          let mgr = bridge.kokoro,
          let text = text else { return -1 }
    let t = String(cString: text)
    let v = voice.map { String(cString: $0) } ?? "am_michael"
    let s = min(max(speed, 0.5), 2.0)
    let sem = DispatchSemaphore(value: 0)
    var rc: Int32 = 0
    Task {
        do {
            let wav: Data = try await mgr.synthesize(text: t, voice: v, voiceSpeed: s)
            let count = wav.count
            let buf = UnsafeMutablePointer<UInt8>.allocate(capacity: count)
            wav.copyBytes(to: buf, count: count)
            outBytes?.pointee = buf
            outLen?.pointee = UInt(count)
        } catch { rc = 1 }
        sem.signal()
    }
    sem.wait()
    return rc
}

@_cdecl("fluidaudio_free_bytes")
public func fluidaudio_free_bytes(_ p: UnsafeMutablePointer<UInt8>?) { p?.deallocate() }
```
Also add `var kokoro: KokoroTtsManager?` to the `FluidAudioBridge` class.

- [ ] **Step 2: Build**

Run: `cargo build --features tts`
Expected: PASS (Swift compiles).

- [ ] **Step 3: Commit**

```bash
git add swift/FluidAudioBridge.swift
git commit -m "swift: add Kokoro TTS @_cdecl bridge (init + synthesize -> WAV bytes)"
```

### Task 1.3: Rust FFI wrapper

**Files:** Modify `src/ffi/bridge.rs`.

- [ ] **Step 1: Add extern decls** (in the `extern "C"` block):

```rust
fn fluidaudio_initialize_kokoro(ptr: *mut c_void, default_voice: *const c_char) -> i32;
fn fluidaudio_kokoro_synthesize(
    ptr: *mut c_void,
    text: *const c_char,
    voice: *const c_char,
    speed: f32,
    out_bytes: *mut *mut u8,
    out_len: *mut usize,
) -> i32;
fn fluidaudio_free_bytes(p: *mut u8);
```

- [ ] **Step 2: Add safe wrappers** on `FluidAudioBridge` (mirror `diarize_file`):

```rust
pub fn initialize_kokoro(&self, default_voice: &str) -> Result<(), String> {
    let v = CString::new(default_voice).map_err(|_| "invalid voice")?;
    let rc = unsafe { fluidaudio_initialize_kokoro(self.ptr, v.as_ptr()) };
    if rc == 0 { Ok(()) } else { Err("Failed to initialize Kokoro".into()) }
}

pub fn kokoro_synthesize(&self, text: &str, voice: &str, speed: f32) -> Result<Vec<u8>, String> {
    let t = CString::new(text).map_err(|_| "invalid text")?;
    let v = CString::new(voice).map_err(|_| "invalid voice")?;
    let mut out_bytes: *mut u8 = std::ptr::null_mut();
    let mut out_len: usize = 0;
    let rc = unsafe {
        fluidaudio_kokoro_synthesize(self.ptr, t.as_ptr(), v.as_ptr(), speed, &mut out_bytes, &mut out_len)
    };
    if rc != 0 { return Err("Kokoro synthesis failed".into()); }
    if out_bytes.is_null() || out_len == 0 { return Err("Kokoro returned no audio".into()); }
    let wav = unsafe { std::slice::from_raw_parts(out_bytes, out_len) }.to_vec();
    unsafe { fluidaudio_free_bytes(out_bytes) };
    Ok(wav)
}
```

- [ ] **Step 3: Build + commit**

Run: `cargo build --features tts` → PASS.
```bash
git add src/ffi/bridge.rs && git commit -m "ffi: Rust wrappers for Kokoro init + synthesize"
```

### Task 1.4: Public API in `lib.rs` + example

**Files:** Modify `src/lib.rs`; Create `examples/kokoro.rs`.

- [ ] **Step 1: Add public methods** (mirror `init_diarization`/`diarize_file`):

```rust
/// Initialize the Kokoro TTS engine with a default voice (downloads model on first run).
pub fn init_kokoro(&self, default_voice: &str) -> Result<(), FluidAudioError> {
    self.bridge.initialize_kokoro(default_voice).map_err(FluidAudioError::Backend)
}

/// Synthesize `text` as a complete WAV byte buffer (24 kHz mono f32).
pub fn synthesize_kokoro(&self, text: &str, voice: &str, speed: f32) -> Result<Vec<u8>, FluidAudioError> {
    self.bridge.kokoro_synthesize(text, voice, speed).map_err(FluidAudioError::Backend)
}
```
(Use whatever `FluidAudioError` variant the crate already uses for backend strings — match the `diarize_file` mapping.)

- [ ] **Step 2: Add `examples/kokoro.rs`**:

```rust
//! Example: synthesize English TTS via FluidAudio Kokoro.
//! Usage: cargo run --example kokoro --features tts -- "Hello world" am_michael
use fluidaudio_rs::FluidAudio;
use std::io::Write;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let text = args.get(1).map(String::as_str).unwrap_or("Hello world");
    let voice = args.get(2).map(String::as_str).unwrap_or("am_michael");
    let audio = FluidAudio::new()?;
    audio.init_kokoro(voice)?;
    let wav = audio.synthesize_kokoro(text, voice, 1.0)?;
    eprintln!("WAV bytes: {}", wav.len());
    std::io::stdout().write_all(&wav)?;
    Ok(())
}
```

- [ ] **Step 3: Run the example** (the real Kokoro smoke):

Run: `cargo run --example kokoro --features tts -- "Hello world" am_michael > /tmp/k.wav && file /tmp/k.wav`
Expected: `/tmp/k.wav` is a valid RIFF/WAVE, > 50 KB.

- [ ] **Step 4: Commit**

```bash
git add src/lib.rs examples/kokoro.rs && git commit -m "feat: public Kokoro TTS API (init_kokoro + synthesize_kokoro)"
```

---

## Phase 2 — Fork: model-path diarization variant

### Task 2.1: Swift bridge — `fluidaudio_diarize_file_with_models`

**Files:** Modify `swift/FluidAudioBridge.swift`.

- [ ] **Step 1: Add the model-path variant** next to `fluidaudio_diarize_file`, porting the explicit-model-path init from `kesha-diarize/main.swift` (Task 0.3 finding). Same out-param shape as `fluidaudio_diarize_file` (speaker_ids/start/end/quality/count), but the initializer loads from `modelDir` with `SortformerConfig.balancedV2` instead of `downloadAndLoad()`. **The `.balancedV2` config is mandatory, not optional:** it must match the shipped `SortformerNvidiaLow_v2.mlpackage` (`fifoLen=188`) or CoreML fails with a hard tensor-shape mismatch at runtime (Greptile #427). Hardcode it in the bridge — we ship one model, so it is not a parameter.

```swift
@_cdecl("fluidaudio_diarize_file_with_models")
public func fluidaudio_diarize_file_with_models(
    _ ptr: UnsafeMutableRawPointer?,
    _ audioPath: UnsafePointer<CChar>?,
    _ modelDir: UnsafePointer<CChar>?,
    _ outSpeakerIds: UnsafeMutablePointer<UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?>?,
    _ outStart: UnsafeMutablePointer<UnsafeMutablePointer<Float>?>?,
    _ outEnd: UnsafeMutablePointer<UnsafeMutablePointer<Float>?>?,
    _ outQuality: UnsafeMutablePointer<UnsafeMutablePointer<Float>?>?,
    _ outCount: UnsafeMutablePointer<UInt32>?
) -> Int32 {
    // 1. String(cString:) the two paths.
    // 2. Instantiate DiarizerManager from modelDir + SortformerConfig.balancedV2
    //    (EXACT init copied from kesha-diarize/main.swift per Task 0.3).
    // 3. Run diarization on audioPath, marshal spans into the out-params exactly
    //    like fluidaudio_diarize_file does (reuse its marshalling helper).
    // Return 0 on success.
}
```
(Reuse the existing `fluidaudio_free_diarization_result` for cleanup — same allocation shape.)

- [ ] **Step 2: Build + commit**

Run: `cargo build --features diarization` → PASS.
```bash
git add swift/FluidAudioBridge.swift && git commit -m "swift: add diarize_file_with_models (explicit model dir, no download)"
```

### Task 2.2: Rust FFI + public API

**Files:** Modify `src/ffi/bridge.rs`, `src/lib.rs`; Modify `examples/diarize.rs`.

- [ ] **Step 1: extern decl + safe wrapper** in `bridge.rs` — clone `diarize_file`'s body, add a `model_dir: &str` param, call `fluidaudio_diarize_file_with_models`. Returns `Vec<DiarizationSegment>` (identical marshalling).

- [ ] **Step 2: public method** in `lib.rs`:

```rust
/// Diarize using a pre-staged model directory (no network download).
pub fn diarize_file_with_models<P: AsRef<Path>, Q: AsRef<Path>>(
    &self, audio: P, model_dir: Q,
) -> Result<Vec<DiarizationSegment>, FluidAudioError> {
    self.bridge
        .diarize_file_with_models(audio.as_ref().to_str().ok_or(FluidAudioError::InvalidPath)?,
                                  model_dir.as_ref().to_str().ok_or(FluidAudioError::InvalidPath)?)
        .map_err(FluidAudioError::Backend)
}
```

- [ ] **Step 3: smoke via example**

Run: `cargo run --example diarize -- <multi-speaker.wav>` after adding an optional 3rd arg `model_dir` that routes to `diarize_file_with_models` when present.
Expected: prints speaker spans using the local model, no download.

- [ ] **Step 4: Commit**

```bash
git add src/ffi/bridge.rs src/lib.rs examples/diarize.rs
git commit -m "feat: diarize_file_with_models (pinned/explicit model dir)"
```

### Task 2.3: Tag the fork rev + open upstream PR

- [ ] **Step 1: Push branch + record rev**

```bash
git push -u origin feat/kokoro-binding   # or merge to fork main first
git rev-parse HEAD   # record <FORK_SHA> for kesha Cargo.toml
```

- [ ] **Step 2: Open upstream PR** to FluidInference/fluidaudio-rs with the same commits (Kokoro binding + diarize_file_with_models). Title: "Add native Kokoro TTS binding + model-path diarization". This starts the upstream-and-retire path.

---

## Phase 3 — kesha: consume the fork + migrate Kokoro

Work in a kesha worktree off `origin/main`: `git worktree add .worktrees/feat-fluidaudio-native -b feat/fluidaudio-native origin/main`.

### Task 3.0: Promote `with_silenced_stdout` to a shared module (Greptile #427)

**Files:** Create `rust/src/fluid_stdout.rs`; Modify `rust/src/backend/fluidaudio.rs` (remove the local copy), `rust/src/lib.rs` + `rust/src/main.rs` (`mod fluid_stdout;`), `rust/Cargo.toml` (libc under the new features).

FluidAudio's Swift layer prints diagnostics to stdout (#259); the native Kokoro + diarize calls hit the same layer, and Kokoro's WAV goes to stdout, so all three call sites need the guard. The guard uses `libc::dup`/`dup2`, so every feature with a FluidAudio call site (`coreml`, `system_kokoro`, `system_diarize`) must pull `dep:libc`.

- [ ] **Step 1: Create `rust/src/fluid_stdout.rs`** — move `with_silenced_stdout(devnull, f)` here verbatim and add a one-shot variant:

```rust
use std::os::fd::OwnedFd;

/// One-shot stdout silencer for non-hot-path FluidAudio calls (Kokoro synth,
/// diarization). Opens /dev/null itself; a failed open falls back to running
/// `f` with stdout untouched (best-effort — never worse than no guard).
pub fn with_silenced_stdout_oneshot<R>(f: impl FnOnce() -> R) -> R {
    let devnull: Option<OwnedFd> = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/null")
        .ok()
        .map(OwnedFd::from);
    with_silenced_stdout(devnull.as_ref(), f)
}
```

- [ ] **Step 2:** In `backend/fluidaudio.rs`, replace the local `with_silenced_stdout` with `use crate::fluid_stdout::with_silenced_stdout;`. Add `dep:libc` to the `system_kokoro` and `system_diarize` feature lists in `Cargo.toml` (it is already under `coreml`).

- [ ] **Step 3: Build every feature set that compiles a call site**

Run: `cargo check --features coreml --no-default-features`, `cargo check --features onnx,tts,system_kokoro,system_diarize --no-default-features` → both PASS.

- [ ] **Step 4: Commit**

```bash
git add rust/src/fluid_stdout.rs rust/src/backend/fluidaudio.rs rust/src/lib.rs rust/src/main.rs rust/Cargo.toml
git commit -m "refactor: shared with_silenced_stdout + one-shot variant for FluidAudio calls"
```

### Task 3.1: Point the dependency at the fork

**Files:** Modify `rust/Cargo.toml`, `rust/Cargo.lock`.

- [ ] **Step 1: Swap the dep**

```toml
fluidaudio-rs = { git = "https://github.com/drakulavich/fluidaudio-rs", rev = "<FORK_SHA>", optional = true }
```
Enable `diarization` + `tts` features where the crate is used. Keep `optional = true` (gated by `coreml`/`system_*`).

- [ ] **Step 2: Resolve**

Run: `cd rust && cargo update -p fluidaudio-rs && cargo check --features coreml,system_kokoro,system_diarize --no-default-features`
Expected: PASS (links the fork in one binary — this is spike 0.1 reproduced in kesha).

- [ ] **Step 3: Commit**

```bash
git add rust/Cargo.toml rust/Cargo.lock
git commit -m "deps: point fluidaudio-rs at fork with Kokoro + diarize-model-path"
```

### Task 3.2: Rewrite `fluid_kokoro.rs` to call the crate (TDD)

**Files:** Rewrite `rust/src/tts/fluid_kokoro.rs`; Test: same file `#[cfg(test)]`.

- [ ] **Step 1: Write failing test** — a unit test that the synth entry returns a non-empty f32 buffer for a known voice. (Gate behind `#[cfg(all(test, feature = "system_kokoro"))]`; mark `#[ignore]` if it needs the model download in CI — run locally.)

```rust
#[cfg(all(test, feature = "system_kokoro"))]
#[test]
#[ignore = "requires FluidAudio Kokoro model download; run locally on darwin-arm64"]
fn synthesize_returns_audio() {
    let pcm = super::synthesize("Hello world", "am_michael", 1.0).expect("synth");
    assert!(pcm.len() > 16_000, "expected > ~0.5s of samples, got {}", pcm.len());
}
```

- [ ] **Step 2: Run → fails** (old subprocess `synthesize` signature differs / helper missing).

Run: `cargo test --features system_kokoro fluid_kokoro -- --ignored synthesize_returns_audio`
Expected: FAIL.

- [ ] **Step 3: Replace the subprocess body** — delete `helper_path()`, the `Command` spawn, and the WAV-over-stdout read. New body decodes the crate's WAV bytes to f32 via `hound` (already a dep):

```rust
use anyhow::{Context, Result};
use fluidaudio_rs::FluidAudio;

/// Synthesize English text via FluidAudio Kokoro (CoreML/ANE). Returns f32 PCM
/// at the engine's TTS sample rate (24 kHz). Replaces the kesha-kokoro sidecar.
pub fn synthesize(text: &str, voice_id: &str, speed: f32) -> Result<Vec<f32>> {
    // FluidAudio prints diagnostics to stdout; `kesha say` writes the WAV bytes to
    // stdout, so a stray print corrupts the audio stream. Silence stdout for the
    // whole call via the shared one-shot guard (Task 3.0 / Greptile #427).
    let wav = crate::fluid_stdout::with_silenced_stdout_oneshot(|| {
        let audio = FluidAudio::new().context("init FluidAudio bridge")?;
        audio.init_kokoro(voice_id).context("init Kokoro (first run downloads model)")?;
        audio.synthesize_kokoro(text, voice_id, speed).context("Kokoro synthesis")
    })?;
    decode_wav_f32(&wav)
}

/// Decode a complete WAV byte buffer into mono f32 samples.
fn decode_wav_f32(wav: &[u8]) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::new(std::io::Cursor::new(wav)).context("parse Kokoro WAV")?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => reader
            .samples::<i32>()
            .map(|s| s.map(|v| v as f32 / i32::MAX as f32))
            .collect::<Result<_, _>>()?,
    };
    Ok(samples)
}
```
Keep `available_voice_ids()` (the hardcoded `en-<voice>` list) as-is. Update the caller in `tts/say.rs` if the `synthesize` signature changed (it returns `Vec<f32>` now, not raw WAV — confirm `say.rs` already expects f32 from this path, matching the ONNX Kokoro path).

- [ ] **Step 4: Run → passes** (locally on darwin-arm64).

Run: `cargo test --features system_kokoro fluid_kokoro -- --ignored synthesize_returns_audio`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add rust/src/tts/fluid_kokoro.rs rust/src/tts/say.rs
git commit -m "feat(tts): native FluidAudio Kokoro via fluidaudio-rs (drop kesha-kokoro sidecar)"
```

---

## Phase 4 — kesha: migrate diarization

### Task 4.1: Swap the diarize subprocess for the crate call (TDD)

**Files:** Modify `rust/src/transcribe/diarize.rs`; Test: `rust/tests/` (existing diarize integration test, gated/`#[ignore]` for model).

- [ ] **Step 1: Write/locate failing test** — a test that `run(audio, model_dir, ...)` returns labeled spans for a 2-speaker fixture using the pinned model dir. If an existing diarize test spawns the sidecar, repoint it.

```rust
#[test]
#[ignore = "requires diarize .mlpackage; run locally on darwin-arm64 after `kesha install --diarize`"]
fn diarize_labels_two_speakers() {
    let spans = diarize::run(FIXTURE_WAV, &pinned_model_dir(), &asr_segments(), DURATION).unwrap();
    assert!(spans.iter().map(|s| s.speaker).collect::<HashSet<_>>().len() >= 2);
}
```

- [ ] **Step 2: Run → fails.**

Run: `cargo test --features system_diarize diarize_labels_two_speakers -- --ignored`
Expected: FAIL.

- [ ] **Step 3: Replace `Command`-spawn in `run()`** (~lines 88–199) — keep the adaptive-timeout, coverage-validation (≥95% midpoint), and speaker-merge code; replace ONLY the subprocess+JSON-parse with:

```rust
// Same FluidAudio Swift layer as ASR/Kokoro -> silence stdout for the call so
// diagnostics can't corrupt the engine's --json output (Task 3.0 / Greptile #427).
let segments = crate::fluid_stdout::with_silenced_stdout_oneshot(|| {
    let audio = FluidAudio::new().context("init FluidAudio bridge")?;
    audio.init_diarization(0.6).context("init diarization")?; // keep the current threshold constant
    audio
        .diarize_file_with_models(audio_path, model_path)
        .context("FluidAudio diarization failed")
})?;
// map fluidaudio_rs::DiarizationSegment { speaker_id: String, start_time, end_time, .. }
// into our existing span type (parse "SPEAKER_NN" -> u32, or carry the label).
```
Map `speaker_id: "SPEAKER_03"` → `3u32` via suffix parse, preserving the existing merge contract. Delete `sidecar_path()` and the JSON `Deserialize` structs.

- [ ] **Step 4: Run → passes** (locally).

Run: `cargo test --features system_diarize diarize_labels_two_speakers -- --ignored`
Expected: PASS, ≥2 speakers.

- [ ] **Step 5: Commit**

```bash
git add rust/src/transcribe/diarize.rs
git commit -m "feat(diarize): native FluidAudio diarization via fluidaudio-rs (drop kesha-diarize sidecar)"
```

---

## Phase 5 — kesha: delete sidecars + build/CI machinery

### Task 5.1: Delete SwiftPM packages + build.rs blocks

**Files:** Delete `swift/kesha-kokoro/`, `swift/kesha-diarize/`; Modify `rust/build.rs`.

- [ ] **Step 1: Delete the packages**

```bash
git rm -r swift/kesha-kokoro swift/kesha-diarize
```

- [ ] **Step 2: Remove the build.rs blocks** — delete the `#[cfg(all(feature = "system_kokoro", ...))]` block (~46–90) and the `#[cfg(all(feature = "system_diarize", ...))]` block (~133–184), plus their `KESHA_KOKORO_HELPER` / `KESHA_DIARIZE_SIDECAR` env exports. Leave `system_tts` (AVSpeech) and `system_text_lang` blocks untouched.

- [ ] **Step 3: Build all features → confirm no dangling refs**

Run: `cd rust && cargo build --features coreml,tts,system_tts,system_kokoro,system_diarize,system_text_lang --no-default-features`
Expected: PASS (no references to the deleted env vars / helper paths).

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "chore: delete kesha-kokoro + kesha-diarize SwiftPM packages and build.rs blocks"
```

### Task 5.2: Drop the release artifacts from build-engine.yml

**Files:** Modify `.github/workflows/build-engine.yml`.

- [ ] **Step 1: Delete the build+upload steps** for `kesha-kokoro-darwin-arm64` and `kesha-diarize-darwin-arm64` (the `find ... -name kesha-kokoro` / `kesha-diarize` staging + their upload). Keep `say-avspeech-darwin-arm64` and `kesha-textlang-darwin-arm64`. Keep the darwin `features` row unchanged.

- [ ] **Step 2: Lint the workflow**

Run: `yamllint .github/workflows/build-engine.yml 2>/dev/null || true` and visually confirm the matrix + smoke steps still reference only the surviving artifacts.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/build-engine.yml
git commit -m "ci: drop kesha-kokoro/kesha-diarize sidecar artifacts (now in fluidaudio-rs)"
```

---

## Phase 6 — kesha: full verification

### Task 6.1: Rust gate

- [ ] **Step 1:** `cd rust && cargo fmt && cargo clippy --all-targets --features coreml,tts,system_tts,system_kokoro,system_diarize,system_text_lang --no-default-features -- -D warnings` → no warnings.
- [ ] **Step 2:** `cargo nextest run --features tts` → green (the non-darwin-gated tests; the `#[ignore]` model tests run locally).
- [ ] **Step 3:** `cargo check --features onnx,tts` (Linux/Windows feature set) → PASS (confirms the fluidaudio-rs deletion doesn't break the non-coreml build; `fluidaudio-rs` stays optional).

### Task 6.2: Behavior smoke (darwin-arm64, local)

- [ ] **Step 1: Kokoro** — `echo "Hello world" | KESHA_CACHE_DIR=/tmp/kc cargo run --features ... -- say --voice en-am_michael --out /tmp/en.wav` → valid WAV > 50 KB.
- [ ] **Step 2:** Run the `audio-quality-check` agent on `/tmp/en.wav` (RMS, silence ratio, rate, length-vs-text) vs. a sidecar-produced reference — no regression flags.
- [ ] **Step 3: Diarization** — `kesha install --diarize` then `transcribe --with-speakers` on a 2-speaker fixture → ≥2 speakers, coverage validation passes, no network download (verify with the model pre-staged).

### Task 6.3: PR

- [ ] **Step 1:** Push `feat/fluidaudio-native`, open PR. Body: link the spec + this plan + the fork PR + the upstream PR; note Greptile auto-review; verify the build-engine matrix still mirrors Cargo defaults (`diff <(grep 'features = ' .github/workflows/build-engine.yml) <(grep default rust/Cargo.toml)`).
- [ ] **Step 2:** After CI + Greptile green on the latest SHA, merge.

---

## Spike results (fill during Phase 0)

- **0.1 link coexistence:** _<pass/fail + notes>_
- **0.2 Kokoro Swift signature (0.14.5):** _<exact `KokoroTtsManager` API>_
- **0.3 diarizer model-dir init:** _<exact `DiarizerManager`/`SortformerConfig` init from a path>_

## Notes
- Engine release: any change under `rust/` is an **engine release** — bump `rust/Cargo.toml` + `package.json#keshaEngine.version` + `package.json#version` per CLAUDE.md before tagging.
- The fork git-dep means CI builds it from source (runs `swift build`); macOS CI build time rises. Acceptable (was true for the sidecars).
- Retire the fork: once the upstream PR merges + releases, switch `rust/Cargo.toml` back to the crates.io version and delete the fork dependency.
