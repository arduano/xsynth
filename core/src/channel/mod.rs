use std::sync::{atomic::AtomicU64, Arc};

use crate::{
    effects::MultiChannelBiQuad,
    helpers::{db_to_amp, prepapre_cache_vec, sum_simd, FREQS},
    voice::VoiceControlData,
    AudioStreamParams, ChannelCount,
};

use soundfonts::FilterType;

use self::{
    key::KeyData,
    params::{VoiceChannelParams, VoiceChannelStatsReader},
};

use super::AudioPipe;

use biquad::Q_BUTTERWORTH_F32;

use rayon::prelude::*;

mod channel_sf;
mod key;
mod params;
mod voice_buffer;
mod voice_spawner;

mod event;
pub use event::*;

pub struct ValueLerp {
    lerp_length: f32,
    step: f32,
    current: f32,
    end: f32,
}

impl ValueLerp {
    pub fn new(current: f32, sample_rate: u32) -> Self {
        Self {
            lerp_length: sample_rate as f32 * 0.01,
            step: 0.0,
            current,
            end: current,
        }
    }

    pub fn set_end(&mut self, end: f32) {
        self.step = (end - self.current) / self.lerp_length;
        self.end = end;
    }

    pub fn get_next(&mut self) -> f32 {
        if self.end > self.current {
            self.current = (self.current + self.step).min(self.end);
        } else if self.end < self.current {
            self.current = (self.current + self.step).max(self.end);
        }
        self.current
    }
}

struct Key {
    data: KeyData,
    audio_cache: Vec<f32>,
    event_cache: Vec<KeyNoteEvent>,
}

impl Key {
    pub fn new(key: u8, shared_voice_counter: Arc<AtomicU64>, options: ChannelInitOptions) -> Self {
        Key {
            data: KeyData::new(key, shared_voice_counter, options),
            audio_cache: Vec::new(),
            event_cache: Vec::new(),
        }
    }
}

struct ControlEventData {
    selected_lsb: i8,
    selected_msb: i8,
    pitch_bend_sensitivity_lsb: u8,
    pitch_bend_sensitivity_msb: u8,
    pitch_bend_sensitivity: f32,
    pitch_bend_value: f32,
    fine_tune_lsb: u8,
    fine_tune_msb: u8,
    fine_tune_value: f32,
    coarse_tune_value: f32,
    volume: ValueLerp, // 0.0 = silent, 1.0 = max volume
    pan: ValueLerp,    // 0.0 = left, 0.5 = center, 1.0 = right
    cutoff: Option<f32>,
    resonance: Option<f32>,
    expression: ValueLerp,
    preset: u8,
    bank: u8,
}

impl ControlEventData {
    pub fn new_defaults(sample_rate: u32, drums_only: bool) -> Self {
        ControlEventData {
            selected_lsb: -1,
            selected_msb: -1,
            pitch_bend_sensitivity_lsb: 0,
            pitch_bend_sensitivity_msb: 2,
            pitch_bend_sensitivity: 2.0,
            pitch_bend_value: 0.0,
            fine_tune_lsb: 0,
            fine_tune_msb: 0,
            fine_tune_value: 0.0,
            coarse_tune_value: 0.0,
            volume: ValueLerp::new(1.0, sample_rate),
            pan: ValueLerp::new(0.5, sample_rate),
            cutoff: None,
            resonance: None,
            expression: ValueLerp::new(1.0, sample_rate),
            preset: 0,
            bank: if drums_only { 128 } else { 0 },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ChannelInitOptions {
    pub fade_out_killing: bool,
    pub drums_only: bool,
}

#[allow(clippy::derivable_impls)]
impl Default for ChannelInitOptions {
    fn default() -> Self {
        Self {
            fade_out_killing: false,
            drums_only: false,
        }
    }
}

pub struct VoiceChannel {
    key_voices: Vec<Key>,

    params: VoiceChannelParams,
    threadpool: Option<Arc<rayon::ThreadPool>>,

    stream_params: AudioStreamParams,
    options: ChannelInitOptions,

    /// The helper struct for keeping track of MIDI control event data
    control_event_data: ControlEventData,

    /// Processed control data, ready to feed to voices
    voice_control_data: VoiceControlData,

    // Effects
    cutoff: MultiChannelBiQuad,
}

impl VoiceChannel {
    pub fn new(
        options: ChannelInitOptions,
        stream_params: AudioStreamParams,
        threadpool: Option<Arc<rayon::ThreadPool>>,
    ) -> VoiceChannel {
        fn fill_key_array<T, F: Fn(u8) -> T>(func: F) -> Vec<T> {
            let mut vec = Vec::with_capacity(128);
            for i in 0..128 {
                vec.push(func(i));
            }
            vec
        }

        let params = VoiceChannelParams::new(stream_params);
        let shared_voice_counter = params.stats.voice_counter.clone();

        VoiceChannel {
            params,
            key_voices: fill_key_array(|i| Key::new(i, shared_voice_counter.clone(), options)),

            threadpool,

            stream_params,
            options,

            control_event_data: ControlEventData::new_defaults(
                stream_params.sample_rate,
                options.drums_only,
            ),
            voice_control_data: VoiceControlData::new_defaults(),

            cutoff: MultiChannelBiQuad::new(
                stream_params.channels.count() as usize,
                FilterType::LowPass,
                20000.0,
                stream_params.sample_rate as f32,
                None,
            ),
        }
    }

    fn apply_channel_effects(&mut self, out: &mut [f32]) {
        let control = &mut self.control_event_data;

        match self.stream_params.channels {
            ChannelCount::Mono => {
                // Volume
                for sample in out.iter_mut() {
                    let vol = control.volume.get_next() * control.expression.get_next();
                    let vol = vol.powi(2);
                    *sample *= vol;
                }
            }
            ChannelCount::Stereo => {
                // Volume
                for sample in out.chunks_mut(2) {
                    let vol = control.volume.get_next() * control.expression.get_next();
                    let vol = vol.powi(2);
                    sample[0] *= vol;
                    sample[1] *= vol;
                }

                // Pan
                for sample in out.chunks_mut(2) {
                    let pan = control.pan.get_next();
                    sample[0] *= ((pan * std::f32::consts::PI / 2.0).cos()).min(1.0);
                    sample[1] *= ((pan * std::f32::consts::PI / 2.0).sin()).min(1.0);
                }
            }
        }

        // Cutoff
        if let Some(cutoff) = control.cutoff {
            self.cutoff
                .set_filter_type(FilterType::LowPass, cutoff, control.resonance);
            self.cutoff.process(out);
        }
    }

    fn push_key_events_and_render(&mut self, out: &mut [f32]) {
        fn render_for_key(
            key: &mut Key,
            len: usize,
            control: &VoiceControlData,
            params: &VoiceChannelParams,
        ) {
            for e in key.event_cache.drain(..) {
                key.data
                    .send_event(e, control, &params.channel_sf, params.layers);
            }

            prepapre_cache_vec(&mut key.audio_cache, len, 0.0);

            key.data.render_to(&mut key.audio_cache);
        }

        out.fill(0.0);
        match self.threadpool.as_ref() {
            Some(pool) => {
                let len = out.len();
                let key_voices = &mut self.key_voices;
                let params = &self.params;
                let control_data = &self.voice_control_data;
                pool.install(|| {
                    key_voices.par_iter_mut().for_each(move |key| {
                        render_for_key(key, len, control_data, params);
                    });
                });

                for key in self.key_voices.iter() {
                    sum_simd(&key.audio_cache, out);
                }
            }
            None => {
                let len = out.len();

                for key in self.key_voices.iter_mut() {
                    render_for_key(key, len, &self.voice_control_data, &self.params);
                }

                for key in self.key_voices.iter() {
                    sum_simd(&key.audio_cache, out);
                }
            }
        }

        self.apply_channel_effects(out);
    }

    fn propagate_voice_controls(&mut self) {
        for key in self.key_voices.iter_mut() {
            key.data.process_controls(&self.voice_control_data);
        }
    }

    pub fn process_control_event(&mut self, event: ControlEvent) {
        match event {
            ControlEvent::Raw(controller, value) => match controller {
                0x00 => {
                    // Bank select
                    if !self.options.drums_only {
                        self.control_event_data.bank = value;
                        self.params.channel_sf.change_program(
                            self.control_event_data.bank,
                            self.control_event_data.preset,
                        );
                    }
                }
                0x64 => {
                    self.control_event_data.selected_lsb = value as i8;
                }
                0x65 => {
                    self.control_event_data.selected_msb = value as i8;
                }
                0x06 | 0x26 => {
                    let (lsb, msb) = {
                        let data = &self.control_event_data;
                        (data.selected_lsb, data.selected_msb)
                    };
                    if msb == 0 {
                        match lsb {
                            0 => {
                                // Pitch
                                match controller {
                                    0x06 => {
                                        self.control_event_data.pitch_bend_sensitivity_msb = value
                                    }
                                    0x26 => {
                                        self.control_event_data.pitch_bend_sensitivity_lsb = value
                                    }
                                    _ => (),
                                }

                                let sensitivity = {
                                    let data = &self.control_event_data;
                                    (data.pitch_bend_sensitivity_msb as f32)
                                        + (data.pitch_bend_sensitivity_lsb as f32) / 100.0
                                };

                                self.process_control_event(ControlEvent::PitchBendSensitivity(
                                    sensitivity,
                                ))
                            }
                            1 => {
                                // Fine tune
                                match controller {
                                    0x06 => self.control_event_data.fine_tune_msb = value,
                                    0x26 => self.control_event_data.fine_tune_lsb = value,
                                    _ => (),
                                }
                                let val: u16 = ((self.control_event_data.fine_tune_msb as u16)
                                    << 6)
                                    + self.control_event_data.fine_tune_lsb as u16;
                                let val = (val as f32 - 4096.0) / 4096.0 * 100.0;
                                self.process_control_event(ControlEvent::FineTune(val));
                            }
                            2 => {
                                // Coarse tune
                                if controller == 0x06 {
                                    self.process_control_event(ControlEvent::CoarseTune(
                                        value as f32 - 64.0,
                                    ))
                                }
                            }
                            _ => {}
                        }
                    }
                }
                0x07 => {
                    // Volume
                    let vol: f32 = value as f32 / 128.0;
                    self.control_event_data.volume.set_end(vol);
                }
                0x0A => {
                    // Pan
                    let pan: f32 = value as f32 / 128.0;
                    self.control_event_data.pan.set_end(pan);
                }
                0x0B => {
                    // Expression
                    let expr = value as f32 / 128.0;
                    self.control_event_data.expression.set_end(expr);
                }
                0x40 => {
                    // Damper / Sustain
                    let damper = match value {
                        0..=63 => false,
                        64..=127 => true,
                        _ => false,
                    };

                    for key in self.key_voices.iter_mut() {
                        key.data.set_damper(damper);
                    }
                }
                0x47 => {
                    // Resonance
                    if value > 64 {
                        let db = (value as f32 - 64.0) / 2.4;
                        let value = db_to_amp(db) * Q_BUTTERWORTH_F32;
                        self.control_event_data.resonance = Some(value);
                    } else {
                        self.control_event_data.resonance = None;
                    }
                }
                0x48 => {
                    // Release
                    self.voice_control_data.envelope.release = Some(value);
                    self.propagate_voice_controls();
                }
                0x49 => {
                    // Attack
                    self.voice_control_data.envelope.attack = Some(value);
                    self.propagate_voice_controls();
                }
                0x4A => {
                    // Cutoff
                    if value < 64 {
                        let value = value as usize + 64;
                        let mut freq = FREQS[value];
                        if freq > 7000.0 {
                            // I hate BASS
                            let mult = freq / 7000.0 - 1.0;
                            let mult = mult * 2.36 + 1.0;
                            freq = mult * 7000.0;
                        }
                        self.control_event_data.cutoff = Some(freq);
                    } else {
                        self.control_event_data.cutoff = None;
                    }
                }
                0x78 => {
                    // All Sounds Off
                    if value == 0 {
                        self.process_event(ChannelEvent::Audio(ChannelAudioEvent::AllNotesKilled));
                    }
                }
                0x79 => {
                    // Reset All Controllers
                    if value == 0 {
                        self.reset_control();
                    }
                }
                0x7B => {
                    // All Notes Off
                    if value == 0 {
                        self.process_event(ChannelEvent::Audio(ChannelAudioEvent::AllNotesOff));
                    }
                }
                _ => {}
            },
            ControlEvent::PitchBendSensitivity(sensitivity) => {
                let pitch_bend = {
                    let data = &mut self.control_event_data;
                    data.pitch_bend_sensitivity = sensitivity;
                    data.pitch_bend_sensitivity * data.pitch_bend_value
                };
                self.process_control_event(ControlEvent::PitchBend(pitch_bend));
            }
            ControlEvent::PitchBendValue(value) => {
                let pitch_bend = {
                    let data = &mut self.control_event_data;
                    data.pitch_bend_value = value;
                    data.pitch_bend_sensitivity * data.pitch_bend_value
                };
                self.process_control_event(ControlEvent::PitchBend(pitch_bend));
            }
            ControlEvent::PitchBend(value) => {
                self.control_event_data.pitch_bend_value = value;
                self.process_pitch();
            }
            ControlEvent::FineTune(value) => {
                self.control_event_data.fine_tune_value = value;
                self.process_pitch();
            }
            ControlEvent::CoarseTune(value) => {
                self.control_event_data.coarse_tune_value = value;
                self.process_pitch();
            }
        }
    }

    fn process_pitch(&mut self) {
        let data = &mut self.control_event_data;
        let pitch_bend = data.pitch_bend_value;
        let fine_tune = data.fine_tune_value;
        let coarse_tune = data.coarse_tune_value;
        let combined = pitch_bend + coarse_tune + fine_tune / 100.0;

        self.voice_control_data.voice_pitch_multiplier = 2.0f32.powf(combined / 12.0);
        self.propagate_voice_controls();
    }

    pub fn process_event(&mut self, event: ChannelEvent) {
        self.push_events_iter(std::iter::once(event));
    }

    pub fn push_events_iter<T: Iterator<Item = ChannelEvent>>(&mut self, iter: T) {
        for e in iter {
            match e {
                ChannelEvent::Audio(audio) => match audio {
                    ChannelAudioEvent::NoteOn { key, vel } => {
                        if let Some(key) = self.key_voices.get_mut(key as usize) {
                            let ev = KeyNoteEvent::On(vel);
                            key.event_cache.push(ev);
                        }
                    }
                    ChannelAudioEvent::NoteOff { key } => {
                        if let Some(key) = self.key_voices.get_mut(key as usize) {
                            let ev = KeyNoteEvent::Off;
                            key.event_cache.push(ev);
                        }
                    }
                    ChannelAudioEvent::AllNotesOff => {
                        for key in self.key_voices.iter_mut() {
                            let ev = KeyNoteEvent::AllOff;
                            key.event_cache.push(ev);
                        }
                    }
                    ChannelAudioEvent::AllNotesKilled => {
                        for key in self.key_voices.iter_mut() {
                            let ev = KeyNoteEvent::AllKilled;
                            key.event_cache.push(ev);
                        }
                    }
                    ChannelAudioEvent::ResetControl => {
                        self.reset_control();
                    }
                    ChannelAudioEvent::Control(control) => {
                        self.process_control_event(control);
                    }
                    ChannelAudioEvent::ProgramChange(preset) => {
                        self.control_event_data.preset = preset;
                        self.params.channel_sf.change_program(
                            self.control_event_data.bank,
                            self.control_event_data.preset,
                        );
                    }
                },
                ChannelEvent::Config(config) => self.params.process_config_event(config),
            }
        }
    }

    pub fn get_channel_stats(&self) -> VoiceChannelStatsReader {
        let stats = self.params.stats.clone();
        VoiceChannelStatsReader::new(stats)
    }

    fn reset_control(&mut self) {
        self.control_event_data =
            ControlEventData::new_defaults(self.stream_params.sample_rate, self.options.drums_only);
        self.voice_control_data = VoiceControlData::new_defaults();
        self.process_event(ChannelEvent::Audio(ChannelAudioEvent::ProgramChange(0)));
        self.propagate_voice_controls();

        self.control_event_data.cutoff = None;

        for key in self.key_voices.iter_mut() {
            key.data.set_damper(false);
        }
    }
}

impl AudioPipe for VoiceChannel {
    fn stream_params(&self) -> &AudioStreamParams {
        &self.params.constant.stream_params
    }

    fn read_samples_unchecked(&mut self, out: &mut [f32]) {
        self.push_key_events_and_render(out);
    }
}
