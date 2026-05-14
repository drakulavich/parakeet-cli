use anyhow::Result;

use crate::transcribe;

pub fn run(audio_path: String, json: bool, vad: bool, no_vad: bool, speakers: bool) -> Result<()> {
    if speakers && !json {
        anyhow::bail!("--speakers requires --json");
    }
    let opts = transcribe::TranscribeOptions {
        mode: transcribe::VadMode::from_flags(vad, no_vad),
        with_segments: json,
        with_speakers: speakers,
    };
    let output = transcribe::transcribe_with_options(&audio_path, &opts)?;
    if json {
        println!("{}", serde_json::to_string(&output)?);
    } else {
        println!("{}", output.text);
    }
    Ok(())
}
