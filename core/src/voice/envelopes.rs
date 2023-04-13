use simdeez::prelude::*;

use crate::soundfont::SoundfontInitOptions;
use crate::voice::{EnvelopeControlData, ReleaseType, VoiceControlData};

use super::{SIMDSampleMono, SIMDVoiceGenerator, VoiceGeneratorBase};

/// The stages in envelopes as a numbered enum
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EnvelopeStage {
    Delay = 0,
    Attack = 1,
    Hold = 2,
    Decay = 3,
    Sustain = 4,
    Release = 5, // Goes to this stage as soon as the voice is released
    Finished = 6,
}

impl EnvelopeStage {
    pub fn as_usize(&self) -> usize {
        *self as usize
    }

    pub fn next_stage(&self) -> EnvelopeStage {
        match self {
            EnvelopeStage::Delay => EnvelopeStage::Attack,
            EnvelopeStage::Attack => EnvelopeStage::Hold,
            EnvelopeStage::Hold => EnvelopeStage::Decay,
            EnvelopeStage::Decay => EnvelopeStage::Sustain,
            EnvelopeStage::Sustain => EnvelopeStage::Release,
            EnvelopeStage::Release => EnvelopeStage::Finished,
            EnvelopeStage::Finished => EnvelopeStage::Finished,
        }
    }
}

// The lerp equation is `start + (end - start) * factor`
// We store: start, length (= end - start)
struct SIMDLerper<T: Simd> {
    start_simd: T::Vf32,
    length_simd: T::Vf32,
    start: f32,
    length: f32,
}

impl<T: Simd> SIMDLerper<T> {
    fn new(start: f32, end: f32) -> Self {
        simd_invoke!(T, unsafe {
            SIMDLerper {
                start_simd: T::Vf32::set1(start),
                length_simd: T::Vf32::set1(end - start),
                start,
                length: end - start,
            }
        })
    }

    fn lerp(&self, factor: f32) -> f32 {
        self.start + self.length * factor
    }

    fn lerp_simd(&self, factor: T::Vf32) -> T::Vf32 {
        simd_invoke!(T, self.start_simd + self.length_simd * factor)
    }
}

struct SIMDLerpToZeroCurve<T: Simd> {
    start_simd: T::Vf32,
    start: f32,
}

impl<T: Simd> SIMDLerpToZeroCurve<T> {
    fn new(start: f32) -> Self {
        simd_invoke!(T, unsafe {
            SIMDLerpToZeroCurve {
                start_simd: T::Vf32::set1(start),
                start,
            }
        })
    }

    fn lerp(&self, factor: f32) -> f32 {
        let mult = 1.0 - factor;
        mult.powi(8) * self.start
    }

    fn lerp_simd(&self, factor: T::Vf32) -> T::Vf32 {
        simd_invoke!(T, {
            let one = unsafe { T::Vf32::set1(1.0) };
            let r1 = one - factor;
            let r2 = r1 * r1;
            let r3 = r2 * r2;
            let mult = r3 * r3;
            self.start_simd * mult
        })
    }
}

struct StageTime<T: Simd> {
    stage_time_simd: T::Vf32,
    stage_end_time_f32: f32,
    increment_simd: T::Vf32,      // The SIMD width as a SIMD float
    stage_end_time_simd: T::Vf32, // The stage end time as a SIMD float
}

impl<T: Simd> StageTime<T> {
    fn new(start_offset: u32, stage_end_time: u32) -> Self {
        simd_invoke!(T, unsafe {
            let mut stage_time_simd = T::Vf32::set1(start_offset as f32);
            for i in 0..T::Vf32::WIDTH {
                stage_time_simd[i] += i as f32;
            }

            StageTime {
                stage_time_simd,
                stage_end_time_f32: stage_end_time as f32,
                increment_simd: T::Vf32::set1(T::Vf32::WIDTH as f32),
                stage_end_time_simd: T::Vf32::set1(stage_end_time as f32),
            }
        })
    }

    #[inline(always)]
    fn increment(&mut self) {
        simd_invoke!(T, self.stage_time_simd += self.increment_simd);
    }

    #[inline(always)]
    fn increment_by(&mut self, by: u32) {
        simd_invoke!(
            T,
            self.stage_time_simd += unsafe { T::Vf32::set1(by as f32) }
        );
    }

    #[inline(always)]
    /// Is the upper most value in the SIMD array past the end?
    pub fn is_ending(&self) -> bool {
        self.simd_array_end_f32() >= self.stage_end_time_f32
    }

    #[inline(always)]
    /// Is the SIMD array intersecting the end? Or has it completely passed the end
    pub fn is_intersecting_end(&self) -> bool {
        self.is_ending() && self.simd_array_start_f32() < self.stage_end_time_f32
    }

    #[inline(always)]
    fn raw_simd_array(&self) -> &T::Vf32 {
        &self.stage_time_simd
    }

    #[inline(always)]
    pub fn progress_simd_array(&self) -> T::Vf32 {
        simd_invoke!(T, *self.raw_simd_array() / self.stage_end_time_simd)
    }

    #[inline(always)]
    pub fn simd_array_start_f32(&self) -> f32 {
        self.stage_time_simd[0]
    }

    #[inline(always)]
    pub fn simd_array_end_f32(&self) -> f32 {
        self.stage_time_simd[T::Vf32::WIDTH - 1]
    }

    #[allow(unused)]
    #[inline(always)]
    pub fn simd_array_start(&self) -> u32 {
        self.simd_array_start_f32() as u32
    }

    #[allow(unused)]
    #[inline(always)]
    pub fn simd_array_end(&self) -> u32 {
        self.simd_array_end_f32() as u32
    }
}

#[derive(Debug, Clone, Copy)]
pub enum EnvelopePart {
    Lerp {
        target: f32,   // Target value by the end of the envelope part
        duration: u32, // Duration in samples
    },
    LerpToZeroCurve {
        duration: u32,
    },
    Hold(f32),
}

impl EnvelopePart {
    pub fn lerp(target: f32, duration: u32) -> EnvelopePart {
        EnvelopePart::Lerp { target, duration }
    }

    pub fn lerp_to_zero_curve(duration: u32) -> EnvelopePart {
        EnvelopePart::LerpToZeroCurve { duration }
    }

    pub fn hold(value: f32) -> EnvelopePart {
        EnvelopePart::Hold(value)
    }
}

/// The original envelope descriptor
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EnvelopeDescriptor {
    pub start_percent: f32,   // % (0-1)
    pub delay: f32,           // Seconds
    pub attack: f32,          // Seconds
    pub hold: f32,            // Seconds
    pub decay: f32,           // Seconds
    pub sustain_percent: f32, // % (0-1)
    pub release: f32,         // Seconds
}

impl EnvelopeDescriptor {
    pub fn to_envelope_params(
        &self,
        samplerate: u32,
        options: SoundfontInitOptions,
    ) -> EnvelopeParameters {
        let samplerate = samplerate as f32;

        let release = if options.linear_release {
            EnvelopePart::lerp(0.0, (self.release * samplerate) as u32)
        } else {
            EnvelopePart::lerp_to_zero_curve((self.release * samplerate) as u32)
        };

        EnvelopeParameters {
            start: self.start_percent,
            parts: [
                // Delay
                EnvelopePart::lerp(self.start_percent, (self.delay * samplerate) as u32),
                // Attack
                EnvelopePart::lerp(1.0, (self.attack * samplerate) as u32),
                // Hold
                EnvelopePart::lerp(1.0, (self.hold * samplerate) as u32),
                // Decay
                EnvelopePart::lerp(self.sustain_percent, (self.decay * samplerate) as u32),
                // Sustain
                EnvelopePart::hold(self.sustain_percent),
                // Release
                release,
                // Finished
                EnvelopePart::hold(0.0),
            ],
        }
    }
}

/// The raw envelope parameters used to generate the envelope.
/// Is a separate struct to EnvelopeDescriptor for performance reasons.
/// Use EnvelopeDescriptor to generate the EnvelopeParameters struct.
#[derive(Debug, Clone, Copy)]
pub struct EnvelopeParameters {
    start: f32,
    pub parts: [EnvelopePart; 7],
}

impl EnvelopeParameters {
    fn get_stage_data<T: Simd>(
        &self,
        stage: EnvelopeStage,
        start_amp: f32,
    ) -> VoiceEnvelopeState<T> {
        simd_invoke!(T, {
            let stage_info = &self.parts[stage.as_usize()];
            match stage_info {
                EnvelopePart::Lerp { target, duration } => {
                    let duration = *duration;
                    let target = *target;
                    if duration == 0 {
                        self.get_stage_data(stage.next_stage(), target)
                    } else {
                        let data = StageData::Lerp(
                            SIMDLerper::new(start_amp, target),
                            StageTime::new(0, duration),
                        );
                        VoiceEnvelopeState {
                            current_stage: stage,
                            stage_data: data,
                        }
                    }
                }
                EnvelopePart::LerpToZeroCurve { duration } => {
                    let duration = *duration;
                    if duration == 0 {
                        self.get_stage_data(stage.next_stage(), 0.0)
                    } else {
                        let data = StageData::LerpToZeroCurve(
                            SIMDLerpToZeroCurve::new(start_amp),
                            StageTime::new(0, duration),
                        );
                        VoiceEnvelopeState {
                            current_stage: stage,
                            stage_data: data,
                        }
                    }
                }
                EnvelopePart::Hold(value) => {
                    let data = StageData::Constant(unsafe { T::Vf32::set1(*value) });
                    VoiceEnvelopeState {
                        current_stage: stage,
                        stage_data: data,
                    }
                }
            }
        })
    }

    pub fn get_stage_duration<T: Simd>(&self, stage: EnvelopeStage) -> u32 {
        let stage_info = &self.parts[stage.as_usize()];
        match stage_info {
            EnvelopePart::Lerp {
                target: _,
                duration,
            } => *duration,
            EnvelopePart::LerpToZeroCurve { duration } => *duration,
            EnvelopePart::Hold(_) => 0,
        }
    }

    pub fn modify_stage_data<T: Simd>(&mut self, part: usize, data: EnvelopePart) {
        self.parts[part] = data;
    }
}

enum StageData<T: Simd> {
    Lerp(SIMDLerper<T>, StageTime<T>),
    LerpToZeroCurve(SIMDLerpToZeroCurve<T>, StageTime<T>),
    Constant(T::Vf32),
}

struct VoiceEnvelopeState<T: Simd> {
    current_stage: EnvelopeStage,
    stage_data: StageData<T>,
}

pub struct SIMDVoiceEnvelope<T: Simd> {
    original_params: EnvelopeParameters,
    params: EnvelopeParameters,
    allow_release: bool,
    state: VoiceEnvelopeState<T>,
    sample_rate: f32,
    killed: bool,
}

impl<T: Simd> SIMDVoiceEnvelope<T> {
    pub fn new(
        original_params: EnvelopeParameters,
        params: EnvelopeParameters,
        allow_release: bool,
        sample_rate: f32,
    ) -> Self {
        let state = params.get_stage_data(EnvelopeStage::Delay, params.start);

        SIMDVoiceEnvelope {
            original_params,
            params,
            allow_release,
            state,
            sample_rate,
            killed: false,
        }
    }

    pub fn get_value_at_current_time(&self) -> f32 {
        match &self.state.stage_data {
            StageData::Lerp(lerper, stage_time) => {
                lerper.lerp(stage_time.simd_array_start_f32() / stage_time.stage_end_time_f32)
            }
            StageData::LerpToZeroCurve(lerper, stage_time) => {
                lerper.lerp(stage_time.simd_array_start_f32() / stage_time.stage_end_time_f32)
            }
            StageData::Constant(constant) => constant[0],
        }
    }

    pub fn current_stage(&self) -> &EnvelopeStage {
        &self.state.current_stage
    }

    fn switch_to_next_stage(&mut self) {
        let amp = self.get_value_at_current_time();
        self.state = self
            .params
            .get_stage_data(self.current_stage().next_stage(), amp);
    }

    fn update_stage(&mut self) {
        let amp = self.get_value_at_current_time();
        self.state = self.params.get_stage_data(*self.current_stage(), amp);
    }

    fn increment_time_by(&mut self, increment: u32) {
        match &mut self.state.stage_data {
            StageData::Lerp(_, stage_time) => {
                stage_time.increment_by(increment);
            }
            StageData::LerpToZeroCurve(_, stage_time) => {
                stage_time.increment_by(increment);
            }
            StageData::Constant(_) => {}
        }
    }

    fn manually_build_simd_sample(&mut self) -> SIMDSampleMono<T> {
        simd_invoke!(T, {
            let mut values = unsafe { T::Vf32::set1(0.0) };
            for i in 0..T::Vf32::WIDTH {
                let sample = self.get_value_at_current_time();
                values[i] = sample;
                self.increment_time_by(1);
                let should_progress = match &mut self.state.stage_data {
                    StageData::Lerp(_, stage_time) | StageData::LerpToZeroCurve(_, stage_time) => {
                        stage_time.is_ending() && !stage_time.is_intersecting_end()
                    }
                    StageData::Constant(_) => false,
                };
                if should_progress {
                    self.switch_to_next_stage();
                }
            }
            SIMDSampleMono(values)
        })
    }

    pub fn get_modified_envelope(
        mut params: EnvelopeParameters,
        envelope: EnvelopeControlData,
        sample_rate: f32,
    ) -> EnvelopeParameters {
        fn calculate_curve(value: u8, duration: f32) -> f32 {
            match value {
                0..=64 => (value as f32 / 64.0).powi(5) * duration,
                65..=128 => duration + ((value as f32 - 64.0) / 64.0).powi(3) * 15.0,
                _ => duration,
            }
        }

        if let Some(attack) = envelope.attack {
            let duration =
                params.get_stage_duration::<T>(EnvelopeStage::Attack) as f32 / sample_rate;
            params.modify_stage_data::<T>(
                1,
                EnvelopePart::lerp(
                    1.0,
                    (calculate_curve(attack, duration) * sample_rate) as u32,
                ),
            );
        }
        if let Some(release) = envelope.release {
            let duration =
                params.get_stage_duration::<T>(EnvelopeStage::Release) as f32 / sample_rate;
            params.modify_stage_data::<T>(
                5,
                EnvelopePart::lerp_to_zero_curve(
                    (calculate_curve(release, duration).max(0.02) * sample_rate) as u32,
                ),
            );
        }

        params
    }

    pub fn modify_envelope(&mut self, envelope: EnvelopeControlData) {
        if !self.killed {
            self.params =
                Self::get_modified_envelope(self.original_params, envelope, self.sample_rate);
            self.update_stage();
        }
    }
}

impl<T: Simd> VoiceGeneratorBase for SIMDVoiceEnvelope<T> {
    #[inline(always)]
    fn ended(&self) -> bool {
        self.state.current_stage == EnvelopeStage::Finished
    }

    #[inline(always)]
    fn signal_release(&mut self, rel_type: ReleaseType) {
        if rel_type == ReleaseType::Kill {
            self.params.modify_stage_data::<T>(
                5,
                EnvelopePart::lerp(0.0, (0.001 * self.sample_rate) as u32),
            );
            self.update_stage();
            self.killed = true;
        }
        if self.allow_release || self.killed {
            let amp = self.get_value_at_current_time();
            self.state = self.params.get_stage_data(EnvelopeStage::Release, amp);
        }
    }

    #[inline(always)]
    fn process_controls(&mut self, control: &VoiceControlData) {
        self.modify_envelope(control.envelope);
    }
}

impl<T: Simd> SIMDVoiceGenerator<T, SIMDSampleMono<T>> for SIMDVoiceEnvelope<T> {
    #[inline(always)]
    fn next_sample(&mut self) -> SIMDSampleMono<T> {
        simd_invoke!(T, {
            match &mut self.state.stage_data {
                StageData::Lerp(lerper, stage_time) => {
                    if stage_time.is_ending() {
                        if stage_time.is_intersecting_end() {
                            // It is ended, and the SIMD array intersects the border of the envelope part.
                            // Therefore, this needs to generate one float sample at a time for this SIMD array.
                            self.manually_build_simd_sample()
                        } else {
                            // Is ended, except the SIMD array isn't intersecting the end.
                            // Therefore can jump to the next stage, and try again
                            self.switch_to_next_stage();
                            self.next_sample()
                        }
                    } else {
                        // No special conditions happening, return the next entire simd array lerped
                        let values = lerper.lerp_simd(stage_time.progress_simd_array());
                        stage_time.increment();
                        SIMDSampleMono(values)
                    }
                }
                StageData::LerpToZeroCurve(lerper, stage_time) => {
                    if stage_time.is_ending() {
                        if stage_time.is_intersecting_end() {
                            // It is ended, and the SIMD array intersects the border of the envelope part.
                            // Therefore, this needs to generate one float sample at a time for this SIMD array.
                            self.manually_build_simd_sample()
                        } else {
                            // Is ended, except the SIMD array isn't intersecting the end.
                            // Therefore can jump to the next stage, and try again
                            self.switch_to_next_stage();
                            self.next_sample()
                        }
                    } else {
                        // No special conditions happening, return the next entire simd array lerped
                        let values = lerper.lerp_simd(stage_time.progress_simd_array());
                        stage_time.increment();
                        SIMDSampleMono(values)
                    }
                }
                StageData::Constant(constant) => SIMDSampleMono(*constant),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use simdeez::simd_runtime_generate;
    use to_vec::ToVec;

    use super::*;

    use simdeez::*; // nuts

    use simdeez::avx2::*;
    use simdeez::scalar::*;
    use simdeez::sse2::*;
    use simdeez::sse41::*;

    fn assert_vf32_equal<S: Simd>(a: S::Vf32, b: S::Vf32) {
        for i in 0..S::Vf32::WIDTH {
            assert_eq!(a[i], b[i]);
        }
    }

    fn simd_from_vec<S: Simd>(vec: Vec<f32>) -> S::Vf32 {
        unsafe {
            let mut initial = S::Vf32::set1(0.0);
            let mut iter = vec.into_iter();
            for i in 0..S::Vf32::WIDTH {
                initial[i] = iter.next().unwrap();
            }
            initial
        }
    }

    #[test]
    fn test_simd_lerp() {
        simd_runtime_generate!(
            fn run() {
                let lerper = SIMDLerper::<S>::new(0.0, 1.0);
                assert_eq!(lerper.lerp(0.0), 0.0);
                assert_eq!(lerper.lerp(0.5), 0.5);
                assert_eq!(lerper.lerp(1.0), 1.0);
                assert_vf32_equal::<S>(lerper.lerp_simd(S::Vf32::set1(0.0)), S::Vf32::set1(0.0));
                assert_vf32_equal::<S>(lerper.lerp_simd(S::Vf32::set1(0.5)), S::Vf32::set1(0.5));
                assert_vf32_equal::<S>(lerper.lerp_simd(S::Vf32::set1(1.0)), S::Vf32::set1(1.0));
            }
        );

        run_runtime_select();
    }

    #[test]
    fn test_stage_time() {
        fn simd_from_range<S: Simd>(range: std::ops::Range<usize>) -> S::Vf32 {
            simd_from_vec::<S>(range.map(|v| v as f32).to_vec())
        }

        simd_runtime_generate!(
            fn run() {
                let mut time = StageTime::<S>::new(5, 20);
                let mut time2 = StageTime::<S>::new(4, 20);
                assert_eq!(time.simd_array_start(), 5);
                assert!(!time.is_ending());

                let end_simd = S::Vf32::set1(20.0);

                assert_vf32_equal::<S>(
                    *time.raw_simd_array(),
                    simd_from_range::<S>(5..(5 + S::Vf32::WIDTH)),
                );
                assert_vf32_equal::<S>(
                    time.progress_simd_array(),
                    simd_from_range::<S>(5..(5 + S::Vf32::WIDTH)) / end_simd,
                );

                let mut i = 5;
                while time.simd_array_start() + S::Vf32::WIDTH as u32 <= 20 {
                    assert_vf32_equal::<S>(
                        *time.raw_simd_array(),
                        simd_from_range::<S>(i..(i + S::Vf32::WIDTH)),
                    );
                    assert_vf32_equal::<S>(
                        time.progress_simd_array(),
                        simd_from_range::<S>(5..(5 + S::Vf32::WIDTH)) / end_simd,
                    );
                    assert_eq!(time.simd_array_start(), i as u32);
                    assert!(!time.is_ending());

                    assert!(!time.is_intersecting_end());

                    time.increment();
                    time2.increment();
                    i += S::Vf32::WIDTH;
                }
                assert_eq!(time.simd_array_start(), i as u32);
                assert!(time.is_ending());
                assert!(time.is_intersecting_end());

                assert!(!time2.is_ending());
                time2.increment();
                assert!(time2.is_ending());
                assert!(!time2.is_intersecting_end());
            }
        );

        run_runtime_select();
    }

    #[test]
    fn test_envelope() {
        #![allow(clippy::same_item_push)]

        fn push_simd_to_vec<S: Simd>(vec: &mut Vec<f32>, simd: S::Vf32) {
            for i in 0..S::Vf32::WIDTH {
                vec.push(simd[i]);
            }
        }

        fn lerp(from: f32, to: f32, fac: f32) -> f32 {
            from + (to - from) * fac
        }

        fn lerp_to_zero_curve(from: f32, fac: f32) -> f32 {
            let mult = (1. - fac).powi(8);
            mult * from
        }

        simd_runtime_generate!(
            fn run() {
                let mut vec = Vec::new();

                let descriptor = EnvelopeDescriptor {
                    start_percent: 0.5,
                    delay: 0.0,
                    attack: 15.0,
                    hold: 0.0,
                    decay: 17.0,
                    sustain_percent: 0.4,
                    release: 16.0,
                };
                let params = descriptor.to_envelope_params(1, Default::default());

                let mut env = SIMDVoiceEnvelope::<S>::new(params, params, true, 1.0);

                let mut i = 0;
                while i < 48 {
                    push_simd_to_vec::<S>(&mut vec, env.next_sample().0);
                    i += S::Vf32::WIDTH;
                }
                env.signal_release(ReleaseType::Standard);
                assert_eq!(env.current_stage(), &EnvelopeStage::Release);
                while i < 48 + 32 {
                    push_simd_to_vec::<S>(&mut vec, env.next_sample().0);
                    i += S::Vf32::WIDTH;
                }

                let mut expected_vec = Vec::new();

                for i in 0..15 {
                    expected_vec.push(lerp(0.5, 1.0, i as f32 / 15.0));
                }
                for i in 0..17 {
                    expected_vec.push(lerp(1.0, 0.4, i as f32 / 17.0));
                }
                for _ in 0..16 {
                    expected_vec.push(0.4);
                }
                for i in 0..16 {
                    expected_vec.push(lerp_to_zero_curve(0.4, i as f32 / 16.0));
                }
                for _ in 0..16 {
                    expected_vec.push(0.0);
                }

                for v in vec.iter_mut().chain(expected_vec.iter_mut()) {
                    // Rounding as cached values are sometimes off by tiny fractions
                    *v = (*v * 10000.0).round() / 10000.0;
                }

                assert_eq!(vec, expected_vec);
            }
        );

        run_runtime_select();
    }
}
