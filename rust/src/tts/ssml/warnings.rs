//! Warn-once bucket keys for the SSML walker. Hoisted to consts so the
//! three call sites in `parse()` and `walker.rs` can't drift on a typo —
//! `HashSet<String>::insert` swallows any mismatch silently and re-fires
//! the warning forever.

pub(super) const WARN_PROSODY_MID_UTTERANCE: &str = "prosody-mid-utterance";
pub(super) const WARN_PROSODY_NESTED: &str = "prosody-nested";
pub(super) const WARN_PROSODY_NO_SUPPORTED_ATTR: &str = "prosody-no-supported-attr";
