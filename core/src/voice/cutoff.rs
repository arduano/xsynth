use std::marker::PhantomData;

use simdeez::Simd;

use crate::{
    effects::AudioFilter,
    voice::{SIMDVoiceGenerator, VoiceControlData},
};

use soundfonts::FilterType;

use super::{SIMDSampleStereo, VoiceGeneratorBase};

pub struct SIMDStereoVoiceCutoff<S, V>
where
    S: Simd,
    V: SIMDVoiceGenerator<S, SIMDSampleStereo<S>>,
{
    v: V,
    cutoff1: AudioFilter,
    cutoff2: AudioFilter,
    _s: PhantomData<S>,
}

impl<S, V> SIMDStereoVoiceCutoff<S, V>
where
    S: Simd,
    V: SIMDVoiceGenerator<S, SIMDSampleStereo<S>>,
{
    pub fn new(v: V, filter_type: FilterType, sample_rate: f32, initial_cutoff: f32) -> Self {
        SIMDStereoVoiceCutoff {
            v,
            cutoff1: AudioFilter::new(filter_type, 1, initial_cutoff, sample_rate),
            cutoff2: AudioFilter::new(filter_type, 1, initial_cutoff, sample_rate),
            _s: PhantomData,
        }
    }
}

impl<S, V> VoiceGeneratorBase for SIMDStereoVoiceCutoff<S, V>
where
    S: Simd,
    V: SIMDVoiceGenerator<S, SIMDSampleStereo<S>>,
{
    #[inline(always)]
    fn ended(&self) -> bool {
        self.v.ended()
    }

    #[inline(always)]
    fn signal_release(&mut self) {
        self.v.signal_release();
    }

    #[inline(always)]
    fn process_controls(&mut self, control: &VoiceControlData) {
        self.v.process_controls(control);
    }
}

impl<S, V> SIMDVoiceGenerator<S, SIMDSampleStereo<S>> for SIMDStereoVoiceCutoff<S, V>
where
    S: Simd,
    V: SIMDVoiceGenerator<S, SIMDSampleStereo<S>>,
{
    #[inline(always)]
    fn next_sample(&mut self) -> SIMDSampleStereo<S> {
        let mut next_sample = self.v.next_sample();
        next_sample.0 = self.cutoff1.process_sample_simd::<S>(next_sample.0);
        next_sample.1 = self.cutoff2.process_sample_simd::<S>(next_sample.1);
        next_sample
    }
}