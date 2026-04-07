use objc2_avf_audio::{AVAudioCommonFormat, AVAudioPCMBuffer};

pub(crate) fn audio_common_format_name(format: AVAudioCommonFormat) -> &'static str {
    match format {
        AVAudioCommonFormat::PCMFormatFloat32 => "float32",
        AVAudioCommonFormat::PCMFormatFloat64 => "float64",
        AVAudioCommonFormat::PCMFormatInt16 => "int16",
        AVAudioCommonFormat::PCMFormatInt32 => "int32",
        _ => "other",
    }
}

fn sample_vector_from_ptr<T: Copy>(
    ptr: *const T,
    frames: usize,
    stride: usize,
    normalize: impl Fn(T) -> f32,
) -> Vec<f32> {
    let mut samples = Vec::with_capacity(frames);
    for i in 0..frames {
        let sample = unsafe { *ptr.add(i.saturating_mul(stride)) };
        samples.push(normalize(sample));
    }
    samples
}

pub(crate) fn pcm_buffer_to_mono_f32(buffer: &AVAudioPCMBuffer) -> Option<Vec<f32>> {
    let frames = unsafe { buffer.frameLength() as usize };
    if frames == 0 {
        return Some(Vec::new());
    }
    let stride = unsafe { buffer.stride() }.max(1);
    let format = unsafe { buffer.format() };
    let common = unsafe { format.commonFormat() };

    match common {
        AVAudioCommonFormat::PCMFormatFloat32 => {
            let channels = unsafe { buffer.floatChannelData() };
            if channels.is_null() {
                return None;
            }
            let ch0 = unsafe { *channels };
            Some(sample_vector_from_ptr(
                ch0.as_ptr(),
                frames,
                stride,
                |sample| sample,
            ))
        }
        AVAudioCommonFormat::PCMFormatInt16 => {
            let channels = unsafe { buffer.int16ChannelData() };
            if channels.is_null() {
                return None;
            }
            let ch0 = unsafe { *channels };
            Some(sample_vector_from_ptr(
                ch0.as_ptr(),
                frames,
                stride,
                |sample| sample as f32 / i16::MAX as f32,
            ))
        }
        AVAudioCommonFormat::PCMFormatInt32 => {
            let channels = unsafe { buffer.int32ChannelData() };
            if channels.is_null() {
                return None;
            }
            let ch0 = unsafe { *channels };
            Some(sample_vector_from_ptr(
                ch0.as_ptr(),
                frames,
                stride,
                |sample| sample as f32 / i32::MAX as f32,
            ))
        }
        _ => None,
    }
}

pub(crate) fn float_to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|sample| (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16)
        .collect()
}

pub(crate) fn audio_levels(samples: &[f32]) -> (f32, f32) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }

    let mut peak = 0.0f32;
    let mut power = 0.0f32;
    for sample in samples {
        let abs = sample.abs();
        if abs > peak {
            peak = abs;
        }
        power += sample * sample;
    }
    let rms = (power / samples.len() as f32).sqrt();
    (peak, rms)
}

pub(crate) fn resample_linear(samples: &[f32], source_hz: u32, target_hz: u32) -> Vec<f32> {
    if samples.is_empty() || source_hz == 0 || source_hz == target_hz {
        return samples.to_vec();
    }

    let ratio = source_hz as f64 / target_hz as f64;
    let out_len = ((samples.len() as f64) / ratio).max(1.0) as usize;
    (0..out_len)
        .map(|i| {
            let src = i as f64 * ratio;
            let idx = src as usize;
            let frac = src - idx as f64;
            let a = samples.get(idx).copied().unwrap_or(0.0);
            let b = samples.get(idx + 1).copied().unwrap_or(a);
            a + (b - a) * frac as f32
        })
        .collect()
}
