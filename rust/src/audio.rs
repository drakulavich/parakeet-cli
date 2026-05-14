use anyhow::{Context, Result};
use audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::{
    calculate_cutoff, Async, FixedAsync, Resampler, SincInterpolationParameters,
    SincInterpolationType, WindowFunction,
};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CodecParameters, CodecRegistry, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::{Hint, Probe};

const TARGET_SAMPLE_RATE: u32 = 16000;

/// Build a codec registry that includes the default codecs plus libopus.
fn get_codec_registry() -> CodecRegistry {
    let mut registry = CodecRegistry::new();
    // Register all default symphonia codecs
    symphonia::default::register_enabled_codecs(&mut registry);
    // Register libopus adapter for Opus decoding
    registry.register_all::<symphonia_adapter_libopus::OpusDecoder>();
    registry
}

/// Open `path`, probe format + select the first supported audio track.
/// Shared by `decode_audio` and `probe_duration_seconds` so container
/// detection + error messages live in one place.
fn open_format(path: &str) -> Result<(Box<dyn FormatReader>, u32, CodecParameters)> {
    let src = std::fs::File::open(path).with_context(|| format!("file not found: {path}"))?;
    let mss = MediaSourceStream::new(Box::new(src), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
    {
        hint.with_extension(ext);
    }

    let mut probe = Probe::default();
    symphonia::default::register_enabled_formats(&mut probe);

    let probed = probe
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .with_context(|| format!("unsupported audio format: {path}"))?;

    let track = probed
        .format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .with_context(|| format!("no supported audio tracks in: {path}"))?;

    let track_id = track.id;
    let codec_params = track.codec_params.clone();
    Ok((probed.format, track_id, codec_params))
}

/// Decode audio file to raw f32 mono samples at the native sample rate.
/// Returns (samples, sample_rate, channels).
fn decode_audio(path: &str) -> Result<(Vec<f32>, u32, usize)> {
    let (mut format, track_id, codec_params) = open_format(path)?;

    let sample_rate = codec_params
        .sample_rate
        .with_context(|| format!("unknown sample rate in: {path}"))?;
    let channels = codec_params.channels.map(|c| c.count()).unwrap_or(1);

    let dec_opts = DecoderOptions::default();
    let codec_registry = get_codec_registry();
    let mut decoder = codec_registry
        .make(&codec_params, &dec_opts)
        .with_context(|| format!("unsupported codec in: {path}"))?;

    let mut all_samples: Vec<f32> = Vec::new();
    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(_)) => break,
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(e).with_context(|| format!("decode error in: {path}")),
        };

        // Drain stale metadata
        while !format.metadata().is_latest() {
            format.metadata().pop();
        }

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymphoniaError::IoError(_)) | Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(e).with_context(|| format!("decode error in: {path}")),
        };

        // Initialise the sample buffer on first decoded frame
        let buf = sample_buf.get_or_insert_with(|| {
            SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec())
        });

        buf.copy_interleaved_ref(decoded);
        all_samples.extend_from_slice(buf.samples());
    }

    Ok((all_samples, sample_rate, channels))
}

/// Mix interleaved multi-channel samples to mono by averaging channels.
fn mix_to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels == 1 {
        return samples.to_vec();
    }
    samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Resample mono f32 samples from `src_rate` to `TARGET_SAMPLE_RATE` using
/// rubato's asynchronous sinc resampler.
fn resample(samples: Vec<f32>, src_rate: u32) -> Result<Vec<f32>> {
    if src_rate == TARGET_SAMPLE_RATE {
        return Ok(samples);
    }

    let ratio = TARGET_SAMPLE_RATE as f64 / src_rate as f64;

    let sinc_len = 128;
    let window = WindowFunction::BlackmanHarris2;
    let params = SincInterpolationParameters {
        sinc_len,
        f_cutoff: calculate_cutoff(sinc_len, window),
        interpolation: SincInterpolationType::Cubic,
        oversampling_factor: 256,
        window,
    };

    let chunk_size = 1024usize;
    let channels = 1usize;

    let mut resampler =
        Async::<f32>::new_sinc(ratio, 1.1, &params, chunk_size, channels, FixedAsync::Input)
            .context("failed to create resampler")?;

    let input_data = [samples];
    let total_frames = input_data[0].len();

    let mut output_mono: Vec<f32> =
        Vec::with_capacity((total_frames as f64 * ratio * 1.1) as usize);

    let mut frame_offset = 0usize;

    while frame_offset + chunk_size <= total_frames {
        let frames_needed = resampler.input_frames_next();
        if frame_offset + frames_needed > total_frames {
            break;
        }

        let chunk: Vec<Vec<f32>> = input_data
            .iter()
            .map(|ch| ch[frame_offset..frame_offset + frames_needed].to_vec())
            .collect();

        let input_adapter = SequentialSliceOfVecs::new(&chunk, channels, frames_needed)
            .context("failed to create input adapter")?;

        let out_max = resampler.output_frames_max();
        let mut out_data: Vec<Vec<f32>> = vec![vec![0.0f32; out_max]; channels];
        let mut output_adapter = SequentialSliceOfVecs::new_mut(&mut out_data, channels, out_max)
            .context("failed to create output adapter")?;

        let (_frames_in, frames_out) = resampler
            .process_into_buffer(&input_adapter, &mut output_adapter, None)
            .context("resampling failed")?;

        output_mono.extend_from_slice(&out_data[0][..frames_out]);
        frame_offset += frames_needed;
    }

    // Flush remaining samples with zero-padding
    if frame_offset < total_frames {
        let remaining = total_frames - frame_offset;
        let frames_needed = resampler.input_frames_next();
        let mut last_chunk: Vec<f32> = input_data[0][frame_offset..].to_vec();
        last_chunk.resize(frames_needed, 0.0);

        let chunk: Vec<Vec<f32>> = vec![last_chunk];
        let input_adapter = SequentialSliceOfVecs::new(&chunk, channels, frames_needed)
            .context("failed to create input adapter")?;

        let out_max = resampler.output_frames_max();
        let mut out_data: Vec<Vec<f32>> = vec![vec![0.0f32; out_max]; channels];
        let mut output_adapter = SequentialSliceOfVecs::new_mut(&mut out_data, channels, out_max)
            .context("failed to create output adapter")?;

        let (_frames_in, frames_out) = resampler
            .process_into_buffer(&input_adapter, &mut output_adapter, None)
            .context("resampling failed")?;

        // Only keep output proportional to remaining real input
        let real_out = ((remaining as f64 * ratio) as usize).min(frames_out);
        output_mono.extend_from_slice(&out_data[0][..real_out]);
    }

    Ok(output_mono)
}

/// Load an audio file and return 16 kHz mono f32 samples.
pub fn load_audio(path: &str) -> Result<Vec<f32>> {
    let (interleaved, sample_rate, channels) = decode_audio(path)?;
    let mono = mix_to_mono(&interleaved, channels);
    resample(mono, sample_rate)
}

/// Load an audio file, return 16 kHz mono f32 samples truncated to `max_seconds`.
pub fn load_audio_truncated(path: &str, max_seconds: f32) -> Result<Vec<f32>> {
    let samples = load_audio(path)?;
    let max_samples = (max_seconds * TARGET_SAMPLE_RATE as f32) as usize;
    Ok(samples.into_iter().take(max_samples).collect())
}

/// Probe audio duration in seconds without decoding. Returns `None` when the
/// container doesn't report a frame count (some streaming Ogg/Opus files);
/// callers should treat `None` as "unknown — skip auto-trigger" rather than
/// falling back to a decode-and-measure, which would defeat the purpose of a
/// cheap probe.
pub fn probe_duration_seconds(path: &str) -> Result<Option<f32>> {
    let (_format, _track_id, codec_params) = open_format(path)?;
    match (codec_params.n_frames, codec_params.sample_rate) {
        (Some(n), Some(sr)) if sr > 0 => Ok(Some(n as f32 / sr as f32)),
        _ => Ok(None),
    }
}

/// Validate that the file is a supported audio container with at least one
/// audio track. Reads container headers only — never decodes frames or
/// scans for `n_frames` (so it stays cheap even for the Xing-less CBR MP3
/// worst case). Errors on:
///
/// - `"unsupported audio format: ..."` — no symphonia demuxer for the
///   container, e.g. m4a without the `isomp4` feature.
/// - `"no supported audio tracks in: ..."` — video-only / corrupted
///   container, e.g. a webm carrying only a VP8 video track.
///
/// Designed to be called once at the top of every transcribe entry point
/// so the user sees a clean symphonia error instead of a cryptic
/// downstream backend failure (FluidAudio's "Swift bridge error:
/// Transcription failed" is the worst offender, since it lands ~25 s into
/// the ASR cold-load on a file we knew at the top was unusable).
pub fn ensure_audio_track(path: &str) -> Result<()> {
    open_format(path).map(|_| ())
}
