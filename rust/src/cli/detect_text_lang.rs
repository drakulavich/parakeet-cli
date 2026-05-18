use anyhow::Result;

use crate::text_lang;

pub fn run(text: String) -> Result<()> {
    if text.trim().is_empty() {
        anyhow::bail!("detect-text-lang requires non-empty text");
    }
    let result = text_lang::detect_text_language(&text)?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}
