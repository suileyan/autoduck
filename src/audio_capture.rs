use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::{Producer, Consumer};
use rubato::{FftFixedInOut, Resampler};

const VAD_SAMPLE_RATE: u32 = 16000;
const VAD_FRAME_SIZE: usize = 256;

/// Audio capture that writes raw mono f32 samples (at native sample rate)
/// into an rtrb ring buffer from the audio callback thread.
pub struct AudioCapture {
    stream: Option<cpal::Stream>,
}

impl AudioCapture {
    pub fn new() -> Self {
        Self {
            stream: None,
        }
    }

    pub fn start(&mut self, producer: Producer<f32>) -> anyhow::Result<()> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow::anyhow!("No default input device available"))?;

        let supported_config = device
            .supported_input_configs()?
            .filter(|c| c.channels() <= 2 || c.min_sample_rate().0 <= 48000)
            .find(|c| c.sample_format() == cpal::SampleFormat::F32)
            .or_else(|| {
                device
                    .supported_input_configs()
                    .ok()?
                    .find(|c| c.sample_format() == cpal::SampleFormat::I16)
            })
            .ok_or_else(|| anyhow::anyhow!("No suitable input config found"))?;

        let sample_format = supported_config.sample_format();
        let config = supported_config.with_max_sample_rate().config();
        let channels = config.channels as usize;

        let mut producer = producer;

        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mono = downmix_to_mono_f32(data, channels);
                    push_samples(&mut producer, &mono);
                },
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let f32_data: Vec<f32> = data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                    let mono = downmix_to_mono_f32(&f32_data, channels);
                    push_samples(&mut producer, &mono);
                },
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )?,
            _ => return Err(anyhow::anyhow!("Unsupported sample format")),
        };

        stream.play()?;
        self.stream = Some(stream);
        Ok(())
    }

    pub fn stop(&mut self) {
        self.stream = None;
    }

    pub fn create_ring_buffer() -> (Producer<f32>, Consumer<f32>) {
        rtrb::RingBuffer::new(VAD_SAMPLE_RATE as usize)
    }
}

/// Downmix multi-channel interleaved f32 samples to mono by averaging channels.
fn downmix_to_mono_f32(data: &[f32], channels: usize) -> Vec<f32> {
    if channels == 1 {
        return data.to_vec();
    }
    let frame_count = data.len() / channels;
    let mut mono = Vec::with_capacity(frame_count);
    for frame_idx in 0..frame_count {
        let offset = frame_idx * channels;
        let sum: f32 = (0..channels).map(|ch| data[offset + ch]).sum();
        mono.push(sum / channels as f32);
    }
    mono
}

/// Push samples into the ring buffer, dropping any that don't fit.
fn push_samples(producer: &mut Producer<f32>, samples: &[f32]) {
    for &s in samples {
        if producer.push(s).is_err() {
            // Ring buffer full — drop oldest is not possible with rtrb,
            // so we just drop the incoming sample.
        }
    }
}

// ---------------------------------------------------------------------------
// Resampling state
// ---------------------------------------------------------------------------

/// Resampling strategy used by `FrameReader`.
enum ResampleState {
    /// Native rate equals 16 kHz — pass through unchanged.
    Passthrough,
    /// Integer ratio decimation (e.g., 48 kHz → 16 kHz, ratio = 3).
    IntegerDecimation { ratio: usize },
    /// Non-integer ratio — use rubato FftFixedInOut resampler.
    Rubato {
        resampler: FftFixedInOut<f32>,
        input_buffer: Vec<f32>,
        leftover: Vec<f32>,
    },
}

/// Reads raw mono samples from the ring buffer consumer, resamples to 16 kHz,
/// and yields fixed-size frames of `VAD_FRAME_SIZE` samples.
pub struct FrameReader {
    consumer: Consumer<f32>,
    resample_state: ResampleState,
    frame_buffer: Vec<f32>,
}

impl FrameReader {
    pub fn new(consumer: Consumer<f32>, native_sample_rate: u32) -> Self {
        let resample_state = if native_sample_rate == VAD_SAMPLE_RATE {
            ResampleState::Passthrough
        } else if native_sample_rate % VAD_SAMPLE_RATE == 0 {
            let ratio = (native_sample_rate / VAD_SAMPLE_RATE) as usize;
            ResampleState::IntegerDecimation { ratio }
        } else {
            let resampler = FftFixedInOut::<f32>::new(
                native_sample_rate as usize,
                VAD_SAMPLE_RATE as usize,
                1024, // chunk size — reasonable default
                1,    // channels
            )
            .expect("Failed to create rubato resampler");
            ResampleState::Rubato {
                resampler,
                input_buffer: Vec::new(),
                leftover: Vec::new(),
            }
        };

        Self {
            consumer,
            resample_state,
            frame_buffer: Vec::with_capacity(VAD_FRAME_SIZE * 2),
        }
    }

    /// Attempt to return the next VAD frame (256 samples at 16 kHz).
    /// Returns `None` if not enough resampled samples are available yet.
    pub fn next_frame(&mut self) -> Option<Vec<f32>> {
        // Drain all available samples from the ring buffer.
        let mut raw = Vec::new();
        while let Ok(s) = self.consumer.pop() {
            raw.push(s);
        }
        if raw.is_empty() && self.frame_buffer.len() < VAD_FRAME_SIZE {
            return None;
        }

        // Resample the newly arrived samples.
        let resampled = self.resample(&raw);
        self.frame_buffer.extend_from_slice(&resampled);

        if self.frame_buffer.len() >= VAD_FRAME_SIZE {
            let frame: Vec<f32> = self.frame_buffer.drain(..VAD_FRAME_SIZE).collect();
            Some(frame)
        } else {
            None
        }
    }

    fn resample(&mut self, raw: &[f32]) -> Vec<f32> {
        match &mut self.resample_state {
            ResampleState::Passthrough => raw.to_vec(),
            ResampleState::IntegerDecimation { ratio } => decimate_integer(raw, *ratio),
            ResampleState::Rubato {
                resampler,
                input_buffer,
                leftover,
            } => {
                input_buffer.extend_from_slice(raw);
                let mut output = Vec::new();

                let chunk_size = resampler.input_frames_next();
                while input_buffer.len() >= chunk_size {
                    let chunk: Vec<f32> = input_buffer.drain(..chunk_size).collect();
                    // rubato expects &[Vec<f32>] — one Vec per channel.
                    let input_channels = vec![chunk];
                    let resampled_channels = resampler.process(&input_channels, None).unwrap();
                    output.extend_from_slice(&resampled_channels[0]);
                }

                // Any remaining partial chunk stays in input_buffer for next call.
                // Prepend any leftover from previous process call.
                if !leftover.is_empty() {
                    let combined: Vec<f32> = leftover.drain(..).chain(output.iter().copied()).collect();
                    leftover.clear();
                    combined
                } else {
                    output
                }
            }
        }
    }
}

/// Simple integer-ratio decimation with averaging low-pass filter.
/// For ratio R, every R consecutive samples are averaged to produce 1 output sample.
fn decimate_integer(samples: &[f32], ratio: usize) -> Vec<f32> {
    let frame_count = samples.len() / ratio;
    let mut out = Vec::with_capacity(frame_count);
    for i in 0..frame_count {
        let offset = i * ratio;
        let sum: f32 = (0..ratio).map(|j| samples[offset + j]).sum();
        out.push(sum / ratio as f32);
    }
    out
}
