use std::{marker::PhantomData, sync::Arc};

use simdeez::prelude::*;

use crate::soundfont::LoopParams;
use crate::voice::{ReleaseType, VoiceControlData};

use super::{SIMDSampleMono, SIMDSampleStereo, SIMDVoiceGenerator, VoiceGeneratorBase};

mod linear;
pub use linear::*;

mod nearest;
pub use nearest::*;

// I believe some terminology reference is relevant for this one.
//
// BufferSampler: Something that grabs a sample based on an index
//
// SampleReader: Something that grabs the sample value at an arbitrary index,
// and implements sample start/end/looping
//
// SIMDSampleGrabber: Something that takes a SIMD array of float64 locations and
// returns a SIMD array of f32 interpolated sample values

// Base traits

pub trait BufferSampler: Send + Sync {
    fn get(&self, pos: usize) -> f32;
    fn length(&self) -> usize;
}

pub trait SIMDSampleGrabber<S: Simd>: Send + Sync {
    /// Indexes: the rounded index of the sample
    ///
    /// Fractional: The fractional part of the index, i.e. the 0-1 range decimal
    fn get(&mut self, indexes: S::Vi32, fractional: S::Vf32) -> S::Vf32;

    fn is_past_end(&self, pos: f64) -> bool;

    fn signal_release(&mut self);
}

// F32 sampler

pub struct F32BufferSampler(Arc<[f32]>);

impl BufferSampler for F32BufferSampler {
    #[inline(always)]
    fn get(&self, pos: usize) -> f32 {
        match self.0.get(pos) {
            Some(v) => *v,
            None => 0.0,
        }
    }

    fn length(&self) -> usize {
        self.0.len()
    }
}

// Generalized enum sampler

pub enum BufferSamplers {
    F32(F32BufferSampler),
}

impl BufferSamplers {
    #[inline(always)]
    pub fn new_f32(sample: Arc<[f32]>) -> BufferSamplers {
        BufferSamplers::F32(F32BufferSampler(sample))
    }
}

impl BufferSampler for BufferSamplers {
    #[inline(always)]
    fn get(&self, pos: usize) -> f32 {
        match self {
            BufferSamplers::F32(sampler) => sampler.get(pos),
        }
    }

    fn length(&self) -> usize {
        match self {
            BufferSamplers::F32(sampler) => sampler.length(),
        }
    }
}

// Enum sampler reader

pub trait SampleReader: Send + Sync {
    fn get(&mut self, pos: usize) -> f32;
    fn is_past_end(&self, pos: usize) -> bool;
    fn signal_release(&mut self);
}

pub struct SampleReaderNoLoop<Sampler: BufferSampler> {
    buffer: Sampler,
    length: Option<usize>,
    offset: usize,
}

impl<Sampler: BufferSampler> SampleReaderNoLoop<Sampler> {
    pub fn new(buffer: Sampler, loop_params: LoopParams) -> Self {
        let length = Some(buffer.length());
        Self {
            buffer,
            length,
            offset: loop_params.offset as usize,
        }
    }
}

impl<Sampler: BufferSampler> SampleReader for SampleReaderNoLoop<Sampler> {
    fn get(&mut self, pos: usize) -> f32 {
        self.buffer.get(pos + self.offset)
    }

    fn is_past_end(&self, pos: usize) -> bool {
        if let Some(len) = self.length {
            pos - self.offset >= len
        } else {
            false
        }
    }

    fn signal_release(&mut self) {}
}

pub struct SampleReaderLoop<Sampler: BufferSampler> {
    buffer: Sampler,
    offset: usize,
    loop_start: usize,
    loop_end: usize,
}

impl<Sampler: BufferSampler> SampleReaderLoop<Sampler> {
    pub fn new(buffer: Sampler, loop_params: LoopParams) -> Self {
        Self {
            buffer,
            offset: loop_params.offset as usize,
            loop_start: loop_params.start as usize,
            loop_end: loop_params.end as usize,
        }
    }
}

impl<Sampler: BufferSampler> SampleReader for SampleReaderLoop<Sampler> {
    fn get(&mut self, pos: usize) -> f32 {
        let mut pos = pos + self.offset;
        let end = self.loop_end;
        let start = self.loop_start;

        if pos > end {
            pos = (pos - end - 1) % (end - start) + start;
        }

        self.buffer.get(pos)
    }

    fn is_past_end(&self, _pos: usize) -> bool {
        false
    }

    fn signal_release(&mut self) {}
}

pub struct SampleReaderLoopSustain<Sampler: BufferSampler> {
    buffer: Sampler,
    length: Option<usize>,
    offset: usize,
    loop_start: usize,
    loop_end: usize,
    last: usize,
    is_released: bool,
}

impl<Sampler: BufferSampler> SampleReaderLoopSustain<Sampler> {
    pub fn new(buffer: Sampler, loop_params: LoopParams) -> Self {
        let length = Some(buffer.length());
        Self {
            buffer,
            length,
            offset: loop_params.offset as usize,
            loop_start: loop_params.start as usize,
            loop_end: loop_params.end as usize,
            last: 0,
            is_released: false,
        }
    }
}

impl<Sampler: BufferSampler> SampleReader for SampleReaderLoopSustain<Sampler> {
    fn get(&mut self, pos: usize) -> f32 {
        let mut pos = pos + self.offset;
        let end = self.loop_end;
        let start = self.loop_start;

        if !self.is_released {
            self.last = pos;
            if pos > end {
                pos = (pos - end - 1) % (end - start) + start;
            }
        } else {
            pos = pos - self.last + self.loop_end;
        }

        self.buffer.get(pos)
    }

    fn is_past_end(&self, pos: usize) -> bool {
        if let Some(len) = self.length {
            pos - self.last - self.offset >= len
        } else {
            false
        }
    }

    fn signal_release(&mut self) {
        self.is_released = true;
    }
}

// Sample grabbers enum

pub enum SIMDSampleGrabbers<S: Simd, Reader: SampleReader> {
    Nearest(SIMDNearestSampleGrabber<S, Reader>),
    Linear(SIMDLinearSampleGrabber<S, Reader>),
}

impl<S: Simd, Reader: SampleReader> SIMDSampleGrabbers<S, Reader> {
    pub fn nearest(reader: Reader) -> Self {
        SIMDSampleGrabbers::Nearest(SIMDNearestSampleGrabber::new(reader))
    }

    pub fn linear(reader: Reader) -> Self {
        SIMDSampleGrabbers::Linear(SIMDLinearSampleGrabber::new(reader))
    }
}

impl<S: Simd, Reader: SampleReader> SIMDSampleGrabber<S> for SIMDSampleGrabbers<S, Reader> {
    #[inline(always)]
    fn get(&mut self, indexes: S::Vi32, fractional: S::Vf32) -> S::Vf32 {
        match self {
            SIMDSampleGrabbers::Linear(grabber) => grabber.get(indexes, fractional),
            SIMDSampleGrabbers::Nearest(grabber) => grabber.get(indexes, fractional),
        }
    }

    #[inline(always)]
    fn is_past_end(&self, pos: f64) -> bool {
        match self {
            SIMDSampleGrabbers::Linear(grabber) => grabber.is_past_end(pos),
            SIMDSampleGrabbers::Nearest(grabber) => grabber.is_past_end(pos),
        }
    }

    #[inline(always)]
    fn signal_release(&mut self) {
        match self {
            SIMDSampleGrabbers::Linear(grabber) => grabber.signal_release(),
            SIMDSampleGrabbers::Nearest(grabber) => grabber.signal_release(),
        }
    }
}

// Sampler generator

pub struct SIMDStereoVoiceSampler<S, Pitch, Grabber>
where
    S: Simd,
    Pitch: SIMDVoiceGenerator<S, SIMDSampleMono<S>>,
    Grabber: SIMDSampleGrabber<S>,
{
    grabber_left: Grabber,
    grabber_right: Grabber,

    pitch_gen: Pitch,

    time: f64,

    _s: PhantomData<S>,
}

impl<S, Pitch, Grabber> SIMDStereoVoiceSampler<S, Pitch, Grabber>
where
    S: Simd,
    Pitch: SIMDVoiceGenerator<S, SIMDSampleMono<S>>,
    Grabber: SIMDSampleGrabber<S>,
{
    pub fn new(grabber_left: Grabber, grabber_right: Grabber, pitch_gen: Pitch) -> Self {
        SIMDStereoVoiceSampler {
            grabber_left,
            grabber_right,
            pitch_gen,
            time: 0.0,
            _s: PhantomData,
        }
    }

    fn increment_time(&mut self, by: f64) -> f64 {
        let time = self.time;
        self.time += by;
        time
    }
}

impl<S, Pitch, Grabber> VoiceGeneratorBase for SIMDStereoVoiceSampler<S, Pitch, Grabber>
where
    S: Simd,
    Pitch: SIMDVoiceGenerator<S, SIMDSampleMono<S>>,
    Grabber: SIMDSampleGrabber<S>,
{
    #[inline(always)]
    fn ended(&self) -> bool {
        self.grabber_left.is_past_end(self.time) || self.grabber_right.is_past_end(self.time)
    }

    #[inline(always)]
    fn signal_release(&mut self, rel_type: ReleaseType) {
        self.pitch_gen.signal_release(rel_type);
        self.grabber_left.signal_release();
        self.grabber_right.signal_release();
    }

    #[inline(always)]
    fn process_controls(&mut self, control: &VoiceControlData) {
        self.pitch_gen.process_controls(control);
    }
}

impl<S, Pitch, Grabber> SIMDVoiceGenerator<S, SIMDSampleStereo<S>>
    for SIMDStereoVoiceSampler<S, Pitch, Grabber>
where
    S: Simd,
    Pitch: SIMDVoiceGenerator<S, SIMDSampleMono<S>>,
    Grabber: SIMDSampleGrabber<S>,
{
    #[inline(always)]
    fn next_sample(&mut self) -> SIMDSampleStereo<S> {
        simd_invoke!(S, {
            let speed = self.pitch_gen.next_sample().0;
            let mut indexes = unsafe { S::Vi32::zeroes() };
            let mut fractionals = unsafe { S::Vf32::zeroes() };

            unsafe {
                for i in 0..S::Vf32::WIDTH {
                    let time = self.increment_time(speed.get_unchecked(i) as f64);
                    *indexes.get_unchecked_mut(i) = time as i32;
                    *fractionals.get_unchecked_mut(i) = (time % 1.0) as f32;
                }
            }

            let left = self.grabber_left.get(indexes, fractionals);
            let right = self.grabber_right.get(indexes, fractionals);

            SIMDSampleStereo(left, right)
        })
    }
}
