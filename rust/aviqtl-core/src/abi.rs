use std::mem::{align_of, size_of};

pub const ABI_VERSION: u32 = 1;
pub const CAPABILITY_EASING: u64 = 1 << 0;
pub const CAPABILITY_AUDIO_DSP: u64 = 1 << 1;
pub const CAPABILITIES: u64 = CAPABILITY_EASING | CAPABILITY_AUDIO_DSP;

pub const STATUS_OK: u32 = 0;
pub const STATUS_INVALID_ARGUMENT: u32 = 1;
pub const STATUS_OVERLAPPING_BUFFERS: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AviQtlEasingParameters {
    pub amplitude: f64,
    pub period: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AviQtlAudioMixParameters {
    pub relative_time: f64,
    pub duration: f64,
    pub fade_in_seconds: f32,
    pub fade_out_seconds: f32,
    pub volume: f32,
    pub master_volume: f32,
    pub pan: f32,
    pub limiter: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AviQtlAudioMeter {
    pub peak_left: f32,
    pub peak_right: f32,
    pub rms_left: f32,
    pub rms_right: f32,
}

pub fn pointer_is_valid<T>(pointer: *const T, length: usize) -> bool {
    length == 0 || (!pointer.is_null() && (pointer as usize) & (align_of::<T>() - 1) == 0)
}

fn byte_range<T>(pointer: *const T, length: usize) -> Option<(usize, usize)> {
    let byte_length = length.checked_mul(size_of::<T>())?;
    let start = pointer as usize;
    Some((start, start.checked_add(byte_length)?))
}

pub fn ranges_overlap<T, U>(
    first: *const T,
    first_length: usize,
    second: *const U,
    second_length: usize,
) -> Option<bool> {
    if first_length == 0 || second_length == 0 {
        return Some(false);
    }
    let (first_start, first_end) = byte_range(first, first_length)?;
    let (second_start, second_end) = byte_range(second, second_length)?;
    Some(first_start < second_end && second_start < first_end)
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_core_abi_version() -> u32 {
    ABI_VERSION
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_core_capabilities() -> u64 {
    CAPABILITIES
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};

    #[test]
    fn exposed_layouts_match_the_c_abi_contract() {
        assert_eq!(size_of::<AviQtlEasingParameters>(), 16);
        assert_eq!(align_of::<AviQtlEasingParameters>(), 8);
        assert_eq!(offset_of!(AviQtlEasingParameters, amplitude), 0);
        assert_eq!(offset_of!(AviQtlEasingParameters, period), 8);

        assert_eq!(size_of::<AviQtlAudioMixParameters>(), 40);
        assert_eq!(align_of::<AviQtlAudioMixParameters>(), 8);
        assert_eq!(offset_of!(AviQtlAudioMixParameters, relative_time), 0);
        assert_eq!(offset_of!(AviQtlAudioMixParameters, duration), 8);
        assert_eq!(offset_of!(AviQtlAudioMixParameters, fade_in_seconds), 16);
        assert_eq!(offset_of!(AviQtlAudioMixParameters, limiter), 36);

        assert_eq!(size_of::<AviQtlAudioMeter>(), 16);
        assert_eq!(align_of::<AviQtlAudioMeter>(), 4);
        assert_eq!(offset_of!(AviQtlAudioMeter, peak_left), 0);
        assert_eq!(offset_of!(AviQtlAudioMeter, rms_right), 12);
    }

    #[test]
    fn reports_the_declared_version_and_capabilities() {
        assert_eq!(aviqtl_core_abi_version(), ABI_VERSION);
        assert_eq!(aviqtl_core_capabilities(), CAPABILITIES);
    }
}
