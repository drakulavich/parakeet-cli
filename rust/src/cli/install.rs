use anyhow::Result;

use crate::{backend, models};

pub fn run(
    no_cache: bool,
    #[cfg(feature = "tts")] tts: bool,
    vad: bool,
    #[cfg(feature = "system_diarize")] diarize: bool,
    no_warmup: bool,
) -> Result<()> {
    // Emit the "Model mirror active" banner once at the start of the
    // install run, regardless of which subset of models the flags
    // request. Push-down to `download_*` is more "magic" — each fn
    // hides a stderr write behind its Ok(()) return.
    models::init_mirror_logging();
    models::install(no_cache)?;
    #[cfg(feature = "tts")]
    if tts {
        models::download_tts(no_cache)?;
        eprintln!("TTS models installed.");
    }
    if vad {
        models::download_vad(no_cache)?;
        eprintln!("VAD model installed.");
    }
    #[cfg(feature = "system_diarize")]
    if diarize {
        models::download_diarize(no_cache)?;
        eprintln!("Diarization model installed.");
    }
    // ASR backend warm-up: instantiate the backend once so the
    // expensive cold-start work — Apple Neural Engine model-compile
    // on CoreML (~20-30 s for Parakeet TDT 0.6B), ORT session init
    // on the ONNX path (~500 ms) — happens HERE, during the install
    // step where the user is already waiting on multi-GB downloads.
    // After this, the first real `kesha audio.ogg` is fast because
    // the macOS CoreML cache is keyed by (model bytes, signing
    // identity); the identity is stable across runs of the same
    // binary, so the warm cache survives until the next
    // `kesha install` re-signs (#295).
    //
    // Drop the backend handle immediately — no need to keep it
    // alive past install; the warm cache lives in the OS, not in
    // this process.
    if !no_warmup {
        let asr_dir = models::model_dir(models::ModelKind::Asr)
            .to_string_lossy()
            .into_owned();
        // Honest cost estimate per backend so the user knows what
        // to expect during the pause. CoreML (macOS) pays the ANE
        // compile (~20-30 s); ONNX (Linux/Windows + macOS without
        // `coreml` feature) just loads an ORT session (~500 ms).
        let cost_hint = if cfg!(feature = "coreml") {
            "one-time, ~20-30 s for the ANE compile on first install"
        } else {
            "~500 ms for the ORT session init"
        };
        eprintln!("Warming up ASR backend ({cost_hint})...");
        let t = std::time::Instant::now();
        // Warm-up failures are NON-FATAL (Greptile P1 on #298).
        // All models are already on disk; the install succeeded
        // and the user can still run `kesha audio.ogg`. The first
        // real invocation will pay the cold-start cost we were
        // trying to hide, but that's strictly no-worse than the
        // pre-#298 behavior. Surface the cause on stderr so the
        // user can investigate (typically: ANE permission glitch,
        // CoreML cache directory unwritable, transient ORT init
        // hiccup).
        match backend::create_backend(&asr_dir) {
            Ok(_) => eprintln!("ASR backend warmed up (dt={}ms).", t.elapsed().as_millis()),
            Err(e) => eprintln!(
                "warning: ASR backend warm-up failed ({e}); install \
                 still complete but the first `kesha audio.ogg` will \
                 pay the cold-start cost."
            ),
        }
    }
    eprintln!("Install complete.");
    Ok(())
}
