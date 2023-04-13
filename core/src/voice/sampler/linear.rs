use std::marker::PhantomData;

use simdeez::prelude::*;

use super::{SIMDSampleGrabber, SampleReader};

pub struct SIMDLinearSampleGrabber<S: Simd, Reader: SampleReader> {
    sampler_reader: Reader,
    _s: PhantomData<S>,
}

impl<S: Simd, Reader: SampleReader> SIMDLinearSampleGrabber<S, Reader> {
    pub fn new(sampler_reader: Reader) -> Self {
        SIMDLinearSampleGrabber {
            sampler_reader,
            _s: PhantomData,
        }
    }
}

impl<S: Simd, Reader: SampleReader> SIMDSampleGrabber<S> for SIMDLinearSampleGrabber<S, Reader> {
    fn get(&mut self, indexes: S::Vi32, fractional: S::Vf32) -> S::Vf32 {
        simd_invoke!(S, {
            let ones = unsafe { S::Vf32::set1(1.0f32) };
            let blend = fractional;
            let mut values_first = ones;
            let mut values_second = ones;

            for i in 0..S::Vf32::WIDTH {
                let index = indexes[i] as usize;
                values_first[i] = self.sampler_reader.get(index);
                values_second[i] = self.sampler_reader.get(index + 1);
            }

            let blended = values_first * blend + values_second * (ones - blend);

            blended
        },)
    }

    fn is_past_end(&self, pos: f64) -> bool {
        let pos = pos as usize;
        self.sampler_reader.is_past_end(pos)
    }

    fn signal_release(&mut self) {
        self.sampler_reader.signal_release();
    }
}
