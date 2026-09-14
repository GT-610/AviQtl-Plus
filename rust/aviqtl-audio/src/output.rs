use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// A default-device output stream fed with interleaved stereo floating-point samples.
pub struct AudioOutput {
    queue: Arc<Mutex<VecDeque<f32>>>,
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
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let stream = match sample_format {
            cpal::SampleFormat::I8 => build_stream::<i8>(&device, &config, channels, &queue),
            cpal::SampleFormat::I16 => build_stream::<i16>(&device, &config, channels, &queue),
            cpal::SampleFormat::I24 => {
                build_stream::<cpal::I24>(&device, &config, channels, &queue)
            }
            cpal::SampleFormat::I32 => build_stream::<i32>(&device, &config, channels, &queue),
            cpal::SampleFormat::I64 => build_stream::<i64>(&device, &config, channels, &queue),
            cpal::SampleFormat::U8 => build_stream::<u8>(&device, &config, channels, &queue),
            cpal::SampleFormat::U16 => build_stream::<u16>(&device, &config, channels, &queue),
            cpal::SampleFormat::U32 => build_stream::<u32>(&device, &config, channels, &queue),
            cpal::SampleFormat::U64 => build_stream::<u64>(&device, &config, channels, &queue),
            cpal::SampleFormat::F32 => build_stream::<f32>(&device, &config, channels, &queue),
            cpal::SampleFormat::F64 => build_stream::<f64>(&device, &config, channels, &queue),
            format => Err(format!("Unsupported audio output sample format: {format}")),
        }?;
        stream
            .play()
            .map_err(|error| format!("Could not start audio output: {error}"))?;
        Ok(Self {
            queue,
            _stream: stream,
            sample_rate,
        })
    }

    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn queued_frames(&self) -> usize {
        self.queue.lock().map_or(0, |queue| queue.len() / 2)
    }

    pub fn enqueue_stereo(&self, samples: &[f32]) {
        if !samples.len().is_multiple_of(2) {
            return;
        }
        let Ok(mut queue) = self.queue.lock() else {
            return;
        };
        let maximum_samples = self.sample_rate as usize * 4;
        let overflow = queue
            .len()
            .saturating_add(samples.len())
            .saturating_sub(maximum_samples);
        if overflow > 0 {
            let drain_count = overflow.min(queue.len());
            queue.drain(..drain_count);
        }
        queue.extend(samples.iter().map(|sample| sample.clamp(-1.0, 1.0)));
    }

    pub fn clear(&self) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.clear();
        }
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    queue: &Arc<Mutex<VecDeque<f32>>>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let queue = Arc::clone(queue);
    device
        .build_output_stream(
            config,
            move |output: &mut [T], _| write_output(output, channels, &queue),
            |error| eprintln!("Audio output error: {error}"),
            None,
        )
        .map_err(|error| format!("Could not open the default audio output: {error}"))
}

fn write_output<T>(output: &mut [T], channels: usize, queue: &Arc<Mutex<VecDeque<f32>>>)
where
    T: Sample + FromSample<f32>,
{
    let Ok(mut queue) = queue.lock() else {
        output.fill(T::from_sample(0.0));
        return;
    };
    for frame in output.chunks_mut(channels.max(1)) {
        let left = queue.pop_front().unwrap_or_default();
        let right = queue.pop_front().unwrap_or_default();
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
        let queue = Arc::new(Mutex::new(VecDeque::from([0.25, -0.5, 0.75, 1.0])));
        let mut output = [0.0_f32; 8];
        write_output(&mut output, 4, &queue);
        assert_eq!(
            output,
            [0.25, -0.5, -0.125, -0.125, 0.75, 1.0, 0.875, 0.875]
        );
        assert!(queue.lock().expect("queue remains available").is_empty());
    }

    #[test]
    fn output_callback_fills_underruns_with_silence() {
        let queue = Arc::new(Mutex::new(VecDeque::from([0.5, -0.5])));
        let mut output = [1.0_f32; 4];
        write_output(&mut output, 2, &queue);
        assert_eq!(output, [0.5, -0.5, 0.0, 0.0]);
    }

    #[test]
    fn output_callback_downmixes_stereo_for_a_mono_device() {
        let queue = Arc::new(Mutex::new(VecDeque::from([0.5, -0.25, -1.0, 0.5])));
        let mut output = [0.0_f32; 2];
        write_output(&mut output, 1, &queue);
        assert_eq!(output, [0.125, -0.25]);
    }
}
