use anyhow::Result;
use clap::{Parser, Subcommand};

mod audio;
mod backend;
mod capabilities;
mod cli;
mod debug;
mod lang_id;
mod models;
#[cfg(feature = "tts")]
mod say_loop;
mod text_lang;
mod transcribe;
#[cfg(feature = "tts")]
mod tts;
mod util;
mod vad;

#[derive(Parser)]
#[command(name = "kesha-engine", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Print capabilities as JSON
    #[arg(long = "capabilities-json")]
    capabilities_json: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Transcribe an audio file
    Transcribe {
        /// Path to audio file
        audio_path: String,
        /// Output structured JSON with text and timestamped segments.
        #[arg(long)]
        json: bool,
        /// Force Silero VAD preprocessing. Requires the VAD model to be
        /// installed (`kesha install --vad`). Mutually exclusive with
        /// `--no-vad`. Without either flag, VAD auto-engages on audio
        /// ≥ 120 s when the model is installed (#187).
        #[arg(long, conflicts_with = "no_vad")]
        vad: bool,
        /// Disable VAD preprocessing regardless of duration or install state.
        #[arg(long = "no-vad")]
        no_vad: bool,
        /// Include speaker labels in transcript segments. Requires --json.
        /// Currently darwin-arm64 only (#199).
        #[arg(long)]
        speakers: bool,
    },
    /// Detect spoken language from audio
    DetectLang {
        /// Path to audio file
        audio_path: String,
    },
    /// Detect language of text (macOS only)
    DetectTextLang {
        /// Text to analyze
        text: String,
    },
    /// Download models
    Install {
        /// Re-download even if cached
        #[arg(long)]
        no_cache: bool,
        /// Also install TTS models (Kokoro EN + Vosk RU, ~990MB).
        #[cfg(feature = "tts")]
        #[arg(long)]
        tts: bool,
        /// Also install Silero VAD (~2.3MB) for long-audio preprocessing.
        #[arg(long)]
        vad: bool,
        /// Also install the Sortformer streaming-diarization model (~245MB,
        /// darwin-arm64 only, #199).
        #[cfg(feature = "system_diarize")]
        #[arg(long)]
        diarize: bool,
        /// Skip the ASR-backend warm-up step at the end of install. On macOS
        /// (CoreML) the warm-up triggers the ~20-30 s Apple Neural Engine
        /// model-compile so the first `kesha audio.ogg` invocation is fast.
        /// On the ONNX path (Linux/Windows) warm-up is ~500 ms — still worth
        /// running since it surfaces missing-dep crashes at install time.
        /// Use this flag in scripted installs where the cold-start cost
        /// belongs on the first real run, or to debug install-time issues
        /// without the backend in the loop.
        #[arg(long = "no-warmup")]
        no_warmup: bool,
    },
    /// Synthesize speech from text (TTS)
    #[cfg(feature = "tts")]
    Say {
        /// Text to synthesize (omit to read from stdin)
        text: Option<String>,
        /// Voice id, e.g. `en-am_michael`
        #[arg(long)]
        voice: Option<String>,
        /// Override the voice's default BCP 47 language code, e.g. `en-gb`
        #[arg(long)]
        lang: Option<String>,
        /// Output file (default: stdout)
        #[arg(long)]
        out: Option<std::path::PathBuf>,
        /// Speaking rate (0.5–2.0)
        #[arg(long, default_value_t = 1.0)]
        rate: f32,
        /// List installed voices and exit
        #[arg(long)]
        list_voices: bool,
        /// Parse the input as SSML (supports <speak>, <break>; strips unknown tags).
        /// See issue #122 for the v1 tag matrix.
        #[arg(long)]
        ssml: bool,
        /// Output audio format. Defaults to `wav` (or inferred from `--out`
        /// extension when omitted). Supported: `wav`, `ogg-opus`. See #223.
        #[arg(long, value_name = "FORMAT")]
        format: Option<String>,
        /// Opus bitrate in bits/second (e.g. 16000, 32000, 64000). Only valid
        /// with `--format ogg-opus`. Default 32000 (Telegram-grade).
        #[arg(long, value_name = "BPS")]
        bitrate: Option<i32>,
        /// Encoder sample rate. Only valid with `--format ogg-opus`. Must be
        /// one of 8000/12000/16000/24000/48000. Default 24000.
        #[arg(long = "sample-rate", value_name = "HZ")]
        sample_rate: Option<u32>,
        /// Explicit model path (testing override)
        #[arg(long, hide = true)]
        model: Option<std::path::PathBuf>,
        /// Explicit voice embedding file (testing override)
        #[arg(long = "voice-file", hide = true)]
        voice_file: Option<std::path::PathBuf>,
        /// Long-lived loop: read newline-delimited JSON requests on stdin,
        /// reuse loaded engines across calls, write framed binary responses
        /// on stdout. See `docs/tts-stdin-loop.md`. Issue #213.
        #[arg(long = "stdin-loop", hide = true)]
        stdin_loop: bool,
        /// Disable auto-expansion of Russian acronyms (e.g. ВОЗ → "вэ о зэ").
        /// `<say-as interpret-as="characters">` in SSML remains honored.
        /// No effect for non-`ru-vosk-*` voices.
        #[arg(long = "no-expand-abbrev", default_value_t = false)]
        no_expand_abbrev: bool,
    },
}

fn main() -> Result<()> {
    // Anchor the `KESHA_DEBUG=1` `+Nms` timeline before `Cli::parse()` so
    // clap parsing + env probes are counted toward the first `dtrace!`'s
    // prefix (Greptile P2 on #293). No-op when debug is off.
    debug::init();
    let cli = Cli::parse();

    if cli.capabilities_json {
        let caps = capabilities::get_capabilities();
        println!("{}", serde_json::to_string(&caps)?);
        return Ok(());
    }

    match cli.command {
        Some(Commands::Transcribe {
            audio_path,
            json,
            vad,
            no_vad,
            speakers,
        }) => cli::transcribe::run(audio_path, json, vad, no_vad, speakers)?,
        Some(Commands::DetectLang { audio_path }) => cli::detect_lang::run(audio_path)?,
        Some(Commands::DetectTextLang { text }) => cli::detect_text_lang::run(text)?,
        Some(Commands::Install {
            no_cache,
            #[cfg(feature = "tts")]
            tts,
            vad,
            #[cfg(feature = "system_diarize")]
            diarize,
            no_warmup,
        }) => cli::install::run(
            no_cache,
            #[cfg(feature = "tts")]
            tts,
            vad,
            #[cfg(feature = "system_diarize")]
            diarize,
            no_warmup,
        )?,
        #[cfg(feature = "tts")]
        Some(Commands::Say {
            text,
            voice,
            lang,
            out,
            rate,
            list_voices,
            ssml,
            format,
            bitrate,
            sample_rate,
            model,
            voice_file,
            stdin_loop,
            no_expand_abbrev,
        }) => {
            std::process::exit(cli::say::run(cli::say::SayArgs {
                text,
                voice,
                lang,
                out,
                rate,
                list_voices,
                ssml,
                format,
                bitrate,
                sample_rate,
                model,
                voice_file,
                stdin_loop,
                no_expand_abbrev,
            }));
        }
        None => {
            eprintln!("Usage: kesha-engine <command>");
            eprintln!("Run --help for usage information");
            std::process::exit(1);
        }
    }

    Ok(())
}
