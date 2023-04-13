use std::marker::PhantomData;

use simdeez::prelude::*;
use simdeez::Simd;

use super::{SIMDSampleGrabber, SampleReader};

pub struct SIMDNearestSampleGrabber<S: Simd, Reader: SampleReader> {
    sampler_reader: Reader,
    _s: PhantomData<S>,
}

impl<S: Simd, Reader: SampleReader> SIMDNearestSampleGrabber<S, Reader> {
    pub fn new(sampler_reader: Reader) -> Self {
        SIMDNearestSampleGrabber {
            sampler_reader,
            _s: PhantomData,
        }
    }
}

impl<S: Simd, Reader: SampleReader> SIMDSampleGrabber<S> for SIMDNearestSampleGrabber<S, Reader> {
    fn get(&mut self, indexes: S::Vi32, _: S::Vf32) -> S::Vf32 {
        simd_invoke!(S, unsafe {
            let mut values = S::Vf32::zeroes();

            for i in 0..S::Vf32::WIDTH {
                let index = indexes.get_unchecked(i) as usize;
                *values.get_unchecked_mut(i) = self.sampler_reader.get(index);
            }

            values
        })
    }

    fn is_past_end(&self, pos: f64) -> bool {
        let pos = pos as usize;
        self.sampler_reader.is_past_end(pos)
    }

    fn signal_release(&mut self) {
        self.sampler_reader.signal_release();
    }
}
