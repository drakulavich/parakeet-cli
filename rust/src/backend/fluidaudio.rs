use std::io::{BufWriter, Write};
use std::os::fd::OwnedFd;

use anyhow::{Context, Result};
use fluidaudio_rs::FluidAudio;

use super::TranscribeBackend;

/// FluidAudio's CoreML ASR rejects clips shorter than ~1s (returns
/// `invalidAudioData` and prints the error to stdout — see #259).
/// VAD spans frequently produce sub-second segments at speech onsets /
/// offsets, so we pad them with trailing silence before handing to
/// `transcribe_file`. 1.5 s @ 16 kHz = 24 000 samples; well above the
/// observed failure threshold and small enough that the extra silence
/// doesn't cost meaningful ASR latency.
const MIN_SAMPLES: usize = 16_000 + 16_000 / 2; // 1.5 s @ 16 kHz

pub struct FluidAudioBackend {
    audio: FluidAudio,
    /// Pre-opened /dev/null reused across `transcribe_samples` calls so
    /// the per-segment hot path skips the open syscall (~10K saved on a
    /// 1 h meeting). `None` when the open at construction time failed,
    /// in which case `with_silenced_stdout` falls back to running the
    /// closure with stdout untouched — never worse than the pre-#259
    /// behaviour, just with the residual print risk back on the table.
    devnull: Option<OwnedFd>,
}

impl FluidAudioBackend {
    pub fn new() -> Result<Self> {
        let audio = FluidAudio::new().context("failed to initialize FluidAudio bridge")?;
        audio
            .init_asr()
            .context("failed to initialize FluidAudio ASR (first run compiles models for ANE)")?;
        let devnull = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/null")
            .ok()
            .map(OwnedFd::from);
        Ok(Self { audio, devnull })
    }
}

impl TranscribeBackend for FluidAudioBackend {
    fn transcribe(&mut self, audio_path: &str) -> Result<String> {
        let result = self
            .audio
            .transcribe_file(audio_path)
            .context("FluidAudio transcription failed")?;
        Ok(result.text)
    }

    /// `fluidaudio-rs 0.1.0` ships without `transcribe_samples` (available
    /// on main, not yet published), so this shim writes the slice to a
    /// temp WAV and calls `transcribe_file`. Temp I/O for a 16 kHz mono f32
    /// slice is negligible vs the ~50-200 ms ASR cost. Drop this shim and
    /// delegate to `transcribe_samples` directly once upstream cuts a
    /// release that exposes it.
    ///
    /// Sub-second VAD segments are padded to MIN_SAMPLES with trailing
    /// silence (#259); FluidAudio's transcribe_file otherwise emits
    /// `Transcribe error: invalidAudioData` to stdout and returns an Err.
    /// stdout is silenced for the duration of the call as belt-and-braces
    /// — even with padding, residual upstream prints would corrupt the
    /// engine's `--json` output by interleaving with our JSON write.
    fn transcribe_samples(&mut self, samples: &[f32]) -> Result<String> {
        let padded = pad_to_min(samples, MIN_SAMPLES);
        let tmp = tempfile::Builder::new()
            .prefix("kesha-vad-segment-")
            .suffix(".wav")
            .tempfile()
            .context("creating temp WAV for VAD segment")?;
        write_float_wav(tmp.path(), &padded, 16_000).context("writing temp WAV for VAD segment")?;
        let path_str = tmp.path().to_str().context("temp WAV path was non-UTF-8")?;
        let result = with_silenced_stdout(self.devnull.as_ref(), || {
            self.audio.transcribe_file(path_str)
        })
        .context("FluidAudio sample transcription failed")?;
        Ok(result.text)
    }
}

/// Pad `samples` to at least `min_len` with trailing zeros (silence).
/// Returns a borrowed `Cow` so already-long-enough inputs don't allocate.
fn pad_to_min(samples: &[f32], min_len: usize) -> std::borrow::Cow<'_, [f32]> {
    if samples.len() >= min_len {
        std::borrow::Cow::Borrowed(samples)
    } else {
        let mut padded = Vec::with_capacity(min_len);
        padded.extend_from_slice(samples);
        padded.resize(min_len, 0.0);
        std::borrow::Cow::Owned(padded)
    }
}

/// Run `f` with the process's stdout temporarily redirected to `devnull`.
/// FluidAudio's CoreML pipeline writes diagnostic strings (`Transcribe
/// error: invalidAudioData`) to stdout via Swift's `print(...)` — when
/// `kesha-engine transcribe --json` is the caller, that noise interleaves
/// with our JSON serialization and breaks downstream `jq` parsers (#259).
/// Restoring stdout in a `Drop` impl keeps the redirect short-lived even
/// if `f` panics.
///
/// `devnull` is the long-lived fd cached on `FluidAudioBackend`; passing
/// `None` runs `f` with stdout untouched (best-effort fallback for the
/// pathological case where opening /dev/null at backend init failed).
fn with_silenced_stdout<R>(devnull: Option<&OwnedFd>, f: impl FnOnce() -> R) -> R {
    use std::os::fd::{AsRawFd, FromRawFd};

    struct StdoutGuard {
        saved: Option<OwnedFd>,
    }
    impl Drop for StdoutGuard {
        fn drop(&mut self) {
            if let Some(saved) = self.saved.take() {
                // SAFETY: saved is a dup'd stdout fd we own. as_raw_fd
                // borrows it for the dup2 call (atomic in the kernel);
                // `saved` is then dropped at end of this block, closing
                // the duplicate. dup2 retains its own reference on fd 1.
                let rc = unsafe { libc::dup2(saved.as_raw_fd(), libc::STDOUT_FILENO) };
                if rc < 0 {
                    // Restore failed — fd 1 stays pointed at /dev/null and
                    // every subsequent `println!` (including our final JSON)
                    // silently vanishes. Surface the OS error on stderr so the
                    // caller has any chance of noticing the broken pipe.
                    // Rare path (fd exhaustion mid-run); we can't do better
                    // than warn from a Drop impl.
                    let errno = std::io::Error::last_os_error();
                    let _ = writeln!(
                        std::io::stderr(),
                        "warning: failed to restore stdout after FluidAudio call: {errno}"
                    );
                }
            }
        }
    }

    // SAFETY: dup(STDOUT) returns a fresh fd we own; OwnedFd takes
    // responsibility for closing it on drop. dup failure is best-effort —
    // we just run f without a guard, never worse than the pre-#259
    // behaviour.
    let saved: Option<OwnedFd> = unsafe {
        let raw = libc::dup(libc::STDOUT_FILENO);
        if raw < 0 {
            None
        } else {
            Some(OwnedFd::from_raw_fd(raw))
        }
    };
    let _guard = StdoutGuard { saved };

    if let Some(devnull) = devnull {
        // SAFETY: devnull is the long-lived fd cached on FluidAudioBackend;
        // dup2 atomically replaces fd 1 with a duplicate of devnull, and
        // the cached fd remains valid for subsequent calls.
        unsafe {
            libc::dup2(devnull.as_raw_fd(), libc::STDOUT_FILENO);
        }
    }
    f()
}

/// Write a 16 kHz mono IEEE float32 WAV. FluidAudio loads it via Apple's
/// `AVAudioFile`, which accepts format tag 3 (IEEE_FLOAT). We can't use
/// `hound` here because the `coreml` feature must build cleanly without
/// the `tts` feature that pulls it in.
fn write_float_wav(path: &std::path::Path, samples: &[f32], sample_rate: u32) -> Result<()> {
    let file = std::fs::File::create(path)?;
    let mut w = BufWriter::new(file);
    let channels: u16 = 1;
    let bits_per_sample: u16 = 32;
    let byte_rate = sample_rate * channels as u32 * (bits_per_sample as u32 / 8);
    let block_align = channels * (bits_per_sample / 8);
    let data_bytes = (samples.len() * 4) as u32;
    let fmt_chunk_size: u32 = 16;
    let riff_size = 4 + (8 + fmt_chunk_size) + (8 + data_bytes);

    w.write_all(b"RIFF")?;
    w.write_all(&riff_size.to_le_bytes())?;
    w.write_all(b"WAVE")?;

    w.write_all(b"fmt ")?;
    w.write_all(&fmt_chunk_size.to_le_bytes())?;
    w.write_all(&3u16.to_le_bytes())?; // format code 3 = IEEE_FLOAT
    w.write_all(&channels.to_le_bytes())?;
    w.write_all(&sample_rate.to_le_bytes())?;
    w.write_all(&byte_rate.to_le_bytes())?;
    w.write_all(&block_align.to_le_bytes())?;
    w.write_all(&bits_per_sample.to_le_bytes())?;

    w.write_all(b"data")?;
    w.write_all(&data_bytes.to_le_bytes())?;
    for &s in samples {
        w.write_all(&s.to_le_bytes())?;
    }
    w.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_to_min_borrows_when_already_long_enough() {
        let s = vec![0.5_f32; MIN_SAMPLES];
        let out = pad_to_min(&s, MIN_SAMPLES);
        assert!(matches!(out, std::borrow::Cow::Borrowed(_)));
        assert_eq!(out.len(), MIN_SAMPLES);
    }

    #[test]
    fn pad_to_min_pads_short_clip_with_trailing_silence() {
        let original = vec![0.5_f32; 6_400]; // 0.4 s @ 16 kHz — the failing case from #259
        let out = pad_to_min(&original, MIN_SAMPLES);
        assert_eq!(out.len(), MIN_SAMPLES);
        // Original samples preserved at the head, silence at the tail.
        assert_eq!(&out[..6_400], original.as_slice());
        assert!(out[6_400..].iter().all(|&v| v == 0.0));
    }

    #[test]
    fn pad_to_min_handles_empty_input() {
        let out = pad_to_min(&[], MIN_SAMPLES);
        assert_eq!(out.len(), MIN_SAMPLES);
        assert!(out.iter().all(|&v| v == 0.0));
    }
}
