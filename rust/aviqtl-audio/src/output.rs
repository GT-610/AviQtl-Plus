use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const QUEUE_GENERATION_SHIFT: u32 = 32;
const QUEUED_FRAMES_MASK: u64 = u32::MAX as u64;

#[derive(Debug, Clone, Copy)]
struct QueuedStereoFrame {
    generation: u32,
    left: f32,
    right: f32,
}

#[derive(Debug, Default)]
struct QueueStatus {
    state: AtomicU64,
}

impl QueueStatus {
    fn generation(&self) -> u32 {
        (self.state.load(Ordering::Acquire) >> QUEUE_GENERATION_SHIFT) as u32
    }

    fn queued_frames(&self) -> usize {
        (self.state.load(Ordering::Acquire) & QUEUED_FRAMES_MASK) as usize
    }

    fn enqueue(&self, generation: u32) -> bool {
        self.state
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                if (state >> QUEUE_GENERATION_SHIFT) as u32 != generation {
                    return None;
                }
                let queued_frames = (state & QUEUED_FRAMES_MASK) as u32;
                queued_frames
                    .checked_add(1)
                    .map(|queued_frames| (state & !QUEUED_FRAMES_MASK) | u64::from(queued_frames))
            })
            .is_ok()
    }

    fn dequeue(&self, generation: u32) -> bool {
        self.state
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                if (state >> QUEUE_GENERATION_SHIFT) as u32 != generation
                    || state & QUEUED_FRAMES_MASK == 0
                {
                    None
                } else {
                    Some(state - 1)
                }
            })
            .is_ok()
    }

    fn clear(&self) {
        let _ = self
            .state
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                let generation = ((state >> QUEUE_GENERATION_SHIFT) as u32).wrapping_add(1);
                Some(u64::from(generation) << QUEUE_GENERATION_SHIFT)
            });
    }
}

/// A default-device output stream fed with interleaved stereo floating-point samples.
pub struct AudioOutput {
    producer: Producer<QueuedStereoFrame>,
    queue_status: Arc<QueueStatus>,
    _stream: cpal::Stream,
    sample_rate: u32,
}

impl AudioOutput {
    pub fn open_default() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "No default audio output device is available.".to_owned())?;
        let supported = device
            .default_output_config()
            .map_err(|error| format!("Could not query the default audio output: {error}"))?;
        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let sample_rate = config.sample_rate.0;
        let channels = usize::from(config.channels).max(1);
        let queue_capacity = (sample_rate as usize)
            .saturating_mul(2)
            .min(u32::MAX as usize)
            .max(1);
        let (producer, consumer) = RingBuffer::new(queue_capacity);
        let queue_status = Arc::new(QueueStatus::default());
        let stream = match sample_format {
            cpal::SampleFormat::I8 => {
                build_stream::<i8>(&device, &config, channels, consumer, &queue_status)
            }
            cpal::SampleFormat::I16 => {
                build_stream::<i16>(&device, &config, channels, consumer, &queue_status)
            }
            cpal::SampleFormat::I24 => {
                build_stream::<cpal::I24>(&device, &config, channels, consumer, &queue_status)
            }
            cpal::SampleFormat::I32 => {
                build_stream::<i32>(&device, &config, channels, consumer, &queue_status)
            }
            cpal::SampleFormat::I64 => {
                build_stream::<i64>(&device, &config, channels, consumer, &queue_status)
            }
            cpal::SampleFormat::U8 => {
                build_stream::<u8>(&device, &config, channels, consumer, &queue_status)
            }
            cpal::SampleFormat::U16 => {
                build_stream::<u16>(&device, &config, channels, consumer, &queue_status)
            }
            cpal::SampleFormat::U32 => {
                build_stream::<u32>(&device, &config, channels, consumer, &queue_status)
            }
            cpal::SampleFormat::U64 => {
                build_stream::<u64>(&device, &config, channels, consumer, &queue_status)
            }
            cpal::SampleFormat::F32 => {
                build_stream::<f32>(&device, &config, channels, consumer, &queue_status)
            }
            cpal::SampleFormat::F64 => {
                build_stream::<f64>(&device, &config, channels, consumer, &queue_status)
            }
            format => Err(format!("Unsupported audio output sample format: {format}")),
        }?;
        stream
            .play()
            .map_err(|error| format!("Could not start audio output: {error}"))?;
        Ok(Self {
            producer,
            queue_status,
            _stream: stream,
            sample_rate,
        })
    }

    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn queued_frames(&self) -> usize {
        self.queue_status.queued_frames()
    }

    pub fn enqueue_stereo(&mut self, samples: &[f32]) {
        enqueue_stereo_frames(&mut self.producer, &self.queue_status, samples);
    }

    pub fn clear(&mut self) {
        self.queue_status.clear();
    }
}

fn enqueue_stereo_frames(
    producer: &mut Producer<QueuedStereoFrame>,
    queue_status: &QueueStatus,
    samples: &[f32],
) {
    if !samples.len().is_multiple_of(2) {
        return;
    }
    let generation = queue_status.generation();
    for frame in samples.chunks_exact(2) {
        if producer.is_full() || !queue_status.enqueue(generation) {
            break;
        }
        let queued = QueuedStereoFrame {
            generation,
            left: frame[0].clamp(-1.0, 1.0),
            right: frame[1].clamp(-1.0, 1.0),
        };
        if producer.push(queued).is_err() {
            let _ = queue_status.dequeue(generation);
            break;
        }
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    mut consumer: Consumer<QueuedStereoFrame>,
    queue_status: &Arc<QueueStatus>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let queue_status = Arc::clone(queue_status);
    device
        .build_output_stream(
            config,
            move |output: &mut [T], _| write_output(output, channels, &mut consumer, &queue_status),
            |error| eprintln!("Audio output error: {error}"),
            None,
        )
        .map_err(|error| format!("Could not open the default audio output: {error}"))
}

fn write_output<T>(
    output: &mut [T],
    channels: usize,
    consumer: &mut Consumer<QueuedStereoFrame>,
    queue_status: &QueueStatus,
) where
    T: Sample + FromSample<f32>,
{
    for frame in output.chunks_mut(channels.max(1)) {
        let (left, right) = loop {
            match consumer.pop() {
                Ok(queued) if queue_status.dequeue(queued.generation) => {
                    break (queued.left, queued.right);
                }
                Ok(_) => {}
                Err(_) => break (0.0, 0.0),
            }
        };
        for (channel, sample) in frame.iter_mut().enumerate() {
            let value = match (channels, channel) {
                (1, _) => (left + right) * 0.5,
                (_, 0) => left,
                (_, 1) => right,
                _ => (left + right) * 0.5,
            };
            *sample = T::from_sample(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_callback_maps_stereo_to_the_device_channel_count() {
        let (mut producer, mut consumer) = RingBuffer::new(2);
        let queue_status = QueueStatus::default();
        enqueue_stereo_frames(&mut producer, &queue_status, &[0.25, -0.5, 0.75, 1.0]);
        let mut output = [0.0_f32; 8];
        write_output(&mut output, 4, &mut consumer, &queue_status);
        assert_eq!(
            output,
            [0.25, -0.5, -0.125, -0.125, 0.75, 1.0, 0.875, 0.875]
        );
        assert_eq!(queue_status.queued_frames(), 0);
    }

    #[test]
    fn output_callback_fills_underruns_with_silence() {
        let (mut producer, mut consumer) = RingBuffer::new(1);
        let queue_status = QueueStatus::default();
        enqueue_stereo_frames(&mut producer, &queue_status, &[0.5, -0.5]);
        let mut output = [1.0_f32; 4];
        write_output(&mut output, 2, &mut consumer, &queue_status);
        assert_eq!(output, [0.5, -0.5, 0.0, 0.0]);
    }

    #[test]
    fn output_callback_downmixes_stereo_for_a_mono_device() {
        let (mut producer, mut consumer) = RingBuffer::new(2);
        let queue_status = QueueStatus::default();
        enqueue_stereo_frames(&mut producer, &queue_status, &[0.5, -0.25, -1.0, 0.5]);
        let mut output = [0.0_f32; 2];
        write_output(&mut output, 1, &mut consumer, &queue_status);
        assert_eq!(output, [0.125, -0.25]);
    }

    #[test]
    fn cleared_frames_are_skipped_without_splitting_stereo_pairs() {
        let (mut producer, mut consumer) = RingBuffer::new(2);
        let queue_status = QueueStatus::default();
        enqueue_stereo_frames(&mut producer, &queue_status, &[0.25, -0.25]);
        queue_status.clear();
        enqueue_stereo_frames(&mut producer, &queue_status, &[0.75, -0.5]);

        let mut output = [0.0_f32; 2];
        write_output(&mut output, 2, &mut consumer, &queue_status);

        assert_eq!(output, [0.75, -0.5]);
        assert_eq!(queue_status.queued_frames(), 0);
    }

    #[test]
    fn bounded_queue_drops_only_complete_stereo_frames() {
        let (mut producer, mut consumer) = RingBuffer::new(1);
        let queue_status = QueueStatus::default();
        enqueue_stereo_frames(&mut producer, &queue_status, &[0.25, -0.5, 0.75, 1.0]);

        let mut output = [0.0_f32; 4];
        write_output(&mut output, 2, &mut consumer, &queue_status);

        assert_eq!(output, [0.25, -0.5, 0.0, 0.0]);
        assert_eq!(queue_status.queued_frames(), 0);
    }
}
