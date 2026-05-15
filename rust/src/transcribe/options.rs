//! Type-state builder for [`TranscribeOptions`] (F18).
//!
//! The runtime guard in [`super::transcribe_with_options`] (`anyhow::ensure!`)
//! catches `with_speakers && !with_segments` at the API boundary. The
//! builder lifts that constraint into the type system: `with_speakers()`
//! is only callable in the `WithSegments` state, so the misuse becomes
//! a compile error at every call site that goes through the builder.
//!
//! The runtime guard stays in place as defence-in-depth — direct struct
//! construction (the public fields are still public) bypasses the
//! builder. Closes the type-state half of the #290 follow-up; the
//! `anyhow::ensure!` half remains.

use std::marker::PhantomData;

use super::{TranscribeOptions, VadMode};

pub mod marker {
    /// Builder state: segments not yet enabled. `with_speakers()` is unavailable.
    pub struct NoSegments;
    /// Builder state: segments enabled. `with_speakers()` is available.
    pub struct WithSegments;
}

/// Type-state builder for [`TranscribeOptions`]. Start with
/// [`TranscribeOptionsBuilder::new`] and chain `vad`, `with_segments`,
/// `with_speakers` in any order — `with_speakers` is only available
/// after the `with_segments` transition.
#[derive(Debug)]
pub struct TranscribeOptionsBuilder<S = marker::NoSegments> {
    mode: VadMode,
    with_speakers: bool,
    _state: PhantomData<S>,
}

impl Default for TranscribeOptionsBuilder<marker::NoSegments> {
    fn default() -> Self {
        Self::new()
    }
}

impl TranscribeOptionsBuilder<marker::NoSegments> {
    /// Start a new builder. Defaults match [`TranscribeOptions::default`]:
    /// `VadMode::Auto`, no segments, no speakers.
    pub fn new() -> Self {
        Self {
            mode: VadMode::Auto,
            with_speakers: false,
            _state: PhantomData,
        }
    }

    /// Override the VAD preprocessing mode.
    pub fn vad(mut self, mode: VadMode) -> Self {
        self.mode = mode;
        self
    }

    /// Transition to the `WithSegments` state: per-utterance segments
    /// will be populated. Required before `with_speakers` becomes available.
    pub fn with_segments(self) -> TranscribeOptionsBuilder<marker::WithSegments> {
        TranscribeOptionsBuilder {
            mode: self.mode,
            with_speakers: false,
            _state: PhantomData,
        }
    }

    /// Finalise into a [`TranscribeOptions`] with text-only output.
    pub fn build(self) -> TranscribeOptions {
        TranscribeOptions {
            mode: self.mode,
            with_segments: false,
            with_speakers: false,
        }
    }
}

impl TranscribeOptionsBuilder<marker::WithSegments> {
    /// Enable speaker diarization labels on each segment. Only callable
    /// in the `WithSegments` state — the type-state mirrors the runtime
    /// `anyhow::ensure!` guard in [`super::transcribe_with_options`].
    pub fn with_speakers(mut self) -> Self {
        self.with_speakers = true;
        self
    }

    /// Finalise into a [`TranscribeOptions`] with segments enabled
    /// (and speakers if [`Self::with_speakers`] was called).
    pub fn build(self) -> TranscribeOptions {
        TranscribeOptions {
            mode: self.mode,
            with_segments: true,
            with_speakers: self.with_speakers,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_new_matches_struct_default() {
        let from_builder = TranscribeOptionsBuilder::new().build();
        let from_default = TranscribeOptions::default();
        assert_eq!(from_builder.mode, from_default.mode);
        assert_eq!(from_builder.with_segments, from_default.with_segments);
        assert_eq!(from_builder.with_speakers, from_default.with_speakers);
    }

    #[test]
    fn no_segments_path_produces_text_only_options() {
        let opts = TranscribeOptionsBuilder::new().vad(VadMode::Off).build();
        assert_eq!(opts.mode, VadMode::Off);
        assert!(!opts.with_segments);
        assert!(!opts.with_speakers);
    }

    #[test]
    fn with_segments_alone_keeps_speakers_off() {
        let opts = TranscribeOptionsBuilder::new()
            .vad(VadMode::On)
            .with_segments()
            .build();
        assert_eq!(opts.mode, VadMode::On);
        assert!(opts.with_segments);
        assert!(!opts.with_speakers);
    }

    #[test]
    fn with_speakers_after_with_segments_enables_both() {
        let opts = TranscribeOptionsBuilder::new()
            .with_segments()
            .with_speakers()
            .build();
        assert!(opts.with_segments);
        assert!(opts.with_speakers);
    }

    #[test]
    fn default_impl_matches_new() {
        let from_default: TranscribeOptionsBuilder = TranscribeOptionsBuilder::default();
        let opts = from_default.build();
        assert_eq!(opts.mode, VadMode::Auto);
        assert!(!opts.with_segments);
        assert!(!opts.with_speakers);
    }
}
