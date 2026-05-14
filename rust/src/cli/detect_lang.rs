use anyhow::Result;

use crate::lang_id;

pub fn run(audio_path: String) -> Result<()> {
    let result = lang_id::detect_audio_language(&audio_path)?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}
