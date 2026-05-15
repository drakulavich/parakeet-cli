use anyhow::Result;

use crate::transcribe::{self, TranscribeOptionsBuilder, VadMode};

pub fn run(audio_path: String, json: bool, vad: bool, no_vad: bool, speakers: bool) -> Result<()> {
    if speakers && !json {
        anyhow::bail!("--speakers requires --json");
    }
    let mode = VadMode::from_flags(vad, no_vad);
    let opts = if json {
        let mut b = TranscribeOptionsBuilder::new().vad(mode).with_segments();
        if speakers {
            b = b.with_speakers();
        }
        b.build()
    } else {
        TranscribeOptionsBuilder::new().vad(mode).build()
    };
    let output = transcribe::transcribe_with_options(&audio_path, &opts)?;
    if json {
        println!("{}", serde_json::to_string(&output)?);
    } else {
        println!("{}", output.text);
    }
    Ok(())
}
