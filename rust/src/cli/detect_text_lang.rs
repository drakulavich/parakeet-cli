use anyhow::Result;

use crate::text_lang;

pub fn run(text: String) -> Result<()> {
    let result = text_lang::detect_text_language(&text)?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}
