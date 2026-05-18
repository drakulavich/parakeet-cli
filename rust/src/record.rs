#[cfg(target_os = "macos")]
use std::io::{self, IsTerminal, Read};
use std::path::Path;
#[cfg(target_os = "macos")]
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;
#[cfg(target_os = "macos")]
use std::time::Instant;

#[cfg(any(target_os = "macos", test))]
use anyhow::Context;
use anyhow::{bail, Result};
#[cfg(target_os = "macos")]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
#[cfg(target_os = "macos")]
use cpal::{SampleFormat, StreamConfig};

pub struct RecordSummary {
    pub path: std::path::PathBuf,
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u64,
}

#[cfg(not(target_os = "macos"))]
pub fn record_default_input_to_wav(_path: &Path, _max_duration: Duration) -> Result<RecordSummary> {
    bail!("microphone recording is currently supported on macOS only");
}

#[cfg(target_os = "macos")]
pub fn record_default_input_to_wav(path: &Path, max_duration: Duration) -> Result<RecordSummary> {
    if max_duration.is_zero() {
        bail!("--max-seconds must be greater than 0");
    }

    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .context("no default microphone input device found")?;
    let supported = device
        .default_input_config()
        .context("failed to read default microphone format")?;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let channels = config.channels;
    let sample_rate = config.sample_rate.0;

    let (sample_tx, sample_rx) = mpsc::channel::<Vec<f32>>();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    spawn_stdin_stop_thread(stop_tx);

    let err_fn = |err| eprintln!("recording stream error: {err}");
    let stream = match sample_format {
        SampleFormat::F32 => build_input_stream::<f32>(&device, &config, sample_tx, err_fn)?,
        SampleFormat::I16 => build_input_stream::<i16>(&device, &config, sample_tx, err_fn)?,
        SampleFormat::U16 => build_input_stream::<u16>(&device, &config, sample_tx, err_fn)?,
        other => bail!("unsupported microphone sample format: {other:?}"),
    };

    let mut writer = create_wav_writer(path, sample_rate, channels)?;
    stream
        .play()
        .context("failed to start microphone recording")?;

    let started = Instant::now();
    let mut sample_count = 0u64;
    'recording: loop {
        if stop_rx.try_recv().is_ok() || started.elapsed() >= max_duration {
            break;
        }
        match sample_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(samples) => {
                for (index, sample) in samples.into_iter().enumerate() {
                    if index % 1024 == 0
                        && (stop_rx.try_recv().is_ok() || started.elapsed() >= max_duration)
                    {
                        break 'recording;
                    }
                    writer
                        .write_sample(sample.clamp(-1.0, 1.0))
                        .context("failed to write microphone sample")?;
                    sample_count += 1;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    drop(stream);
    writer
        .finalize()
        .context("failed to finalize WAV recording")?;

    Ok(RecordSummary {
        path: path.to_path_buf(),
        sample_rate,
        channels,
        frames: sample_count / u64::from(channels),
    })
}

#[cfg(target_os = "macos")]
fn spawn_stdin_stop_thread(stop_tx: mpsc::Sender<()>) {
    if io::stdin().is_terminal() {
        return;
    }
    std::thread::spawn(move || {
        let mut buf = [0u8; 1];
        let _ = io::stdin().read(&mut buf);
        let _ = stop_tx.send(());
    });
}

#[cfg(target_os = "macos")]
fn build_input_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_tx: mpsc::Sender<Vec<f32>>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream>
where
    T: cpal::Sample + cpal::SizedSample + Copy + Send + 'static,
    f32: FromInputSample<T>,
{
    let channels = usize::from(config.channels);
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let mut samples = Vec::with_capacity(data.len());
                for frame in data.chunks(channels) {
                    for sample in frame {
                        samples.push(f32::from_input_sample(*sample));
                    }
                }
                let _ = sample_tx.send(samples);
            },
            err_fn,
            None,
        )
        .context("failed to build microphone input stream")
}

#[cfg(target_os = "macos")]
trait FromInputSample<T> {
    fn from_input_sample(sample: T) -> f32;
}

#[cfg(target_os = "macos")]
impl FromInputSample<f32> for f32 {
    fn from_input_sample(sample: f32) -> f32 {
        sample
    }
}

#[cfg(target_os = "macos")]
impl FromInputSample<i16> for f32 {
    fn from_input_sample(sample: i16) -> f32 {
        sample as f32 / i16::MAX as f32
    }
}

#[cfg(target_os = "macos")]
impl FromInputSample<u16> for f32 {
    fn from_input_sample(sample: u16) -> f32 {
        (sample as f32 - 32768.0) / 32768.0
    }
}

#[cfg(any(target_os = "macos", test))]
fn create_wav_writer(
    path: &Path,
    sample_rate: u32,
    channels: u16,
) -> Result<hound::WavWriter<std::io::BufWriter<std::fs::File>>> {
    if channels == 0 {
        bail!("microphone reported zero channels");
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory: {}", parent.display()))?;
    }
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    hound::WavWriter::create(path, spec)
        .with_context(|| format!("failed to create WAV recording: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_writer_finalizes_readable_float_wav() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mic.wav");
        let mut writer = create_wav_writer(&path, 16_000, 1).unwrap();
        writer.write_sample(0.0f32).unwrap();
        writer.write_sample(0.5f32).unwrap();
        writer.write_sample(-0.5f32).unwrap();
        writer.finalize().unwrap();

        let reader = hound::WavReader::open(&path).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 16_000);
        assert_eq!(spec.bits_per_sample, 32);
        assert_eq!(spec.sample_format, hound::SampleFormat::Float);
        let samples = reader
            .into_samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(samples, vec![0.0, 0.5, -0.5]);
    }
}
