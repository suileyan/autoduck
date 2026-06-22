use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::{Producer, Consumer};
use rubato::{FftFixedInOut, Resampler};

const VAD_SAMPLE_RATE: u32 = 16000;
const VAD_FRAME_SIZE: usize = 256;

/// 每帧时长（毫秒）= VAD_FRAME_SIZE / VAD_SAMPLE_RATE * 1000
pub const VAD_FRAME_DURATION_MS: u32 = VAD_FRAME_SIZE as u32 * 1000 / VAD_SAMPLE_RATE;

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

    pub fn start(&mut self, producer: Producer<f32>) -> anyhow::Result<u32> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow::anyhow!("No default input device available"))?;

        let supported_config = device
            .supported_input_configs()?
            .filter(|c| c.channels() <= 2 && c.max_sample_rate().0 >= VAD_SAMPLE_RATE)
            .find(|c| c.sample_format() == cpal::SampleFormat::F32)
            .or_else(|| {
                device
                    .supported_input_configs()
                    .ok()?
                    .find(|c| c.sample_format() == cpal::SampleFormat::I16)
            })
            .ok_or_else(|| anyhow::anyhow!("No suitable input config found"))?;

        let sample_format = supported_config.sample_format();
        // Select the sample rate closest to 48000Hz to avoid unnecessary resampling
        let min_rate = supported_config.min_sample_rate();
        let max_rate = supported_config.max_sample_rate();
        let target_rate = cpal::SampleRate(48000);
        let best_rate = if target_rate.0 >= min_rate.0 && target_rate.0 <= max_rate.0 {
            target_rate
        } else if (min_rate.0 as i64 - target_rate.0 as i64).abs()
            < (max_rate.0 as i64 - target_rate.0 as i64).abs()
        {
            min_rate
        } else {
            max_rate
        };
        let config = supported_config
            .try_with_sample_rate(best_rate)
            .unwrap_or_else(|| supported_config.with_max_sample_rate())
            .config();
        let channels = config.channels as usize;

        let mut producer = producer;

        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if channels == 1 {
                        push_samples(&mut producer, data);
                    } else {
                        let frame_count = data.len() / channels;
                        for frame_idx in 0..frame_count {
                            let offset = frame_idx * channels;
                            let sum: f32 = (0..channels).map(|ch| data[offset + ch]).sum();
                            push_sample(&mut producer, sum / channels as f32);
                        }
                    }
                },
                |err| crate::dbg_output(&format!("Audio stream error: {}", err)),
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if channels == 1 {
                        for &s in data {
                            push_sample(&mut producer, s as f32 / i16::MAX as f32);
                        }
                    } else {
                        let frame_count = data.len() / channels;
                        for frame_idx in 0..frame_count {
                            let offset = frame_idx * channels;
                            let sum: f32 = (0..channels).map(|ch| data[offset + ch] as f32 / i16::MAX as f32).sum();
                            push_sample(&mut producer, sum / channels as f32);
                        }
                    }
                },
                |err| crate::dbg_output(&format!("Audio stream error: {}", err)),
                None,
            )?,
            _ => return Err(anyhow::anyhow!("Unsupported sample format")),
        };

        stream.play()?;
        self.stream = Some(stream);
        Ok(config.sample_rate.0)
    }

    pub fn stop(&mut self) {
        self.stream = None;
    }
}

/// Conservative ring buffer capacity: 192000 * 2 = 384000 samples (2 seconds at 192kHz).
/// For 48kHz devices this is ~8 seconds of buffer — only ~1.1MB extra memory.
const RING_BUFFER_CAPACITY: usize = 384000;

impl AudioCapture {
    pub fn create_ring_buffer() -> (Producer<f32>, Consumer<f32>) {
        rtrb::RingBuffer::new(RING_BUFFER_CAPACITY)
    }
}

/// Push a single sample into the ring buffer, dropping if full.
fn push_sample(producer: &mut Producer<f32>, s: f32) {
    if producer.push(s).is_err() {
        // Ring buffer full — drop the incoming sample.
    }
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
        resampler: Box<FftFixedInOut<f32>>,
        input_buffer: Vec<f32>,
    },
}

/// Reads raw mono samples from the ring buffer consumer, resamples to 16 kHz,
/// and yields fixed-size frames of `VAD_FRAME_SIZE` samples.
pub struct FrameReader {
    consumer: Consumer<f32>,
    resample_state: ResampleState,
    frame_buffer: Vec<f32>,
    raw_buffer: Vec<f32>,
    resample_output: Vec<f32>,
    output_frame: Vec<f32>,
}

impl FrameReader {
    pub fn new(consumer: Consumer<f32>, native_sample_rate: u32) -> Self {
        let resample_state = if native_sample_rate == VAD_SAMPLE_RATE {
            ResampleState::Passthrough
        } else if native_sample_rate.is_multiple_of(VAD_SAMPLE_RATE) {
            let ratio = (native_sample_rate / VAD_SAMPLE_RATE) as usize;
            ResampleState::IntegerDecimation { ratio }
        } else {
            ResampleState::Rubato {
                resampler: Box::new(FftFixedInOut::<f32>::new(
                    native_sample_rate as usize,
                    VAD_SAMPLE_RATE as usize,
                    1024,
                    1,
                ).expect("Failed to create rubato resampler")),
                input_buffer: Vec::new(),
            }
        };

        Self {
            consumer,
            resample_state,
            frame_buffer: Vec::with_capacity(VAD_FRAME_SIZE * 2),
            raw_buffer: Vec::with_capacity(4096),
            resample_output: Vec::with_capacity(4096),
            output_frame: vec![0.0f32; VAD_FRAME_SIZE],
        }
    }

    /// Attempt to return the next VAD frame (256 samples at 16 kHz).
    /// Returns `None` if not enough resampled samples are available yet.
    pub fn next_frame(&mut self) -> Option<&[f32]> {
        // Drain all available samples from the ring buffer.
        self.raw_buffer.clear();
        while let Ok(s) = self.consumer.pop() {
            self.raw_buffer.push(s);
        }
        if self.raw_buffer.is_empty() && self.frame_buffer.len() < VAD_FRAME_SIZE {
            return None;
        }

        // Resample the newly arrived samples.
        let raw = std::mem::take(&mut self.raw_buffer);
        self.resample(&raw);
        self.raw_buffer = raw;

        if self.frame_buffer.len() >= VAD_FRAME_SIZE {
            self.output_frame.copy_from_slice(&self.frame_buffer[..VAD_FRAME_SIZE]);
            self.frame_buffer.drain(..VAD_FRAME_SIZE);
            Some(&self.output_frame)
        } else {
            None
        }
    }

    fn resample(&mut self, raw: &[f32]) {
        match &mut self.resample_state {
            ResampleState::Passthrough => {
                self.frame_buffer.extend_from_slice(raw);
            }
            ResampleState::IntegerDecimation { ratio } => {
                self.resample_output.clear();
                decimate_integer(raw, *ratio, &mut self.resample_output);
                self.frame_buffer.extend_from_slice(&self.resample_output);
            }
            ResampleState::Rubato {
                resampler,
                input_buffer,
            } => {
                input_buffer.extend_from_slice(raw);
                let chunk_size = resampler.input_frames_next();
                while input_buffer.len() >= chunk_size {
                    let chunk: Vec<f32> = input_buffer.drain(..chunk_size).collect();
                    let input_channels = vec![chunk];
                    let resampled_channels = resampler.process(&input_channels, None).unwrap();
                    self.frame_buffer.extend_from_slice(&resampled_channels[0]);
                }
            }
        }
    }
}

/// Simple integer-ratio decimation with averaging low-pass filter.
/// For ratio R, every R consecutive samples are averaged to produce 1 output sample.
fn decimate_integer(samples: &[f32], ratio: usize, out: &mut Vec<f32>) {
    let frame_count = samples.len() / ratio;
    out.reserve(frame_count);
    for i in 0..frame_count {
        let offset = i * ratio;
        let sum: f32 = (0..ratio).map(|j| samples[offset + j]).sum();
        out.push(sum / ratio as f32);
    }
}
