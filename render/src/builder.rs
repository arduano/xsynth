use crate::{
    config::{XSynthRenderAudioFormat, XSynthRenderConfig},
    XSynthRender,
};

use std::sync::Arc;

use core::{
    channel::{ChannelAudioEvent, ChannelConfigEvent, ControlEvent},
    channel_group::SynthEvent,
    soundfont::{LoadSfError, SoundfontBase},
};

use thiserror::Error;

use midi_toolkit::{
    events::{Event, MIDIEventEnum},
    io::{MIDIFile, MIDILoadError},
    pipe,
    sequence::{
        event::{cancel_tempo_events, scale_event_time},
        unwrap_items, TimeCaster,
    },
};

pub struct XSynthRenderStats {
    pub progress: f64,
    pub voice_count: u64,
    // pub render_time: f64,
}

#[derive(Debug, Error)]
pub enum XSynthRenderError {
    #[error("SF loading failed")]
    SfLoadingFailed(#[from] LoadSfError),

    #[error("MIDI loading failed")]
    MidiLoadingFailed(MIDILoadError),
}

impl From<MIDILoadError> for XSynthRenderError {
    fn from(e: MIDILoadError) -> Self {
        XSynthRenderError::MidiLoadingFailed(e)
    }
}

pub struct XSynthRenderBuilder<'a, StatsCallback: FnMut(XSynthRenderStats)> {
    config: XSynthRenderConfig,
    midi_path: &'a str,
    soundfonts: Vec<Arc<dyn SoundfontBase>>,
    layer_count: Option<usize>,
    out_path: &'a str,
    stats_callback: StatsCallback,
}

pub fn xsynth_renderer<'a>(
    midi_path: &'a str,
    out_path: &'a str,
) -> XSynthRenderBuilder<'a, impl FnMut(XSynthRenderStats)> {
    XSynthRenderBuilder {
        config: XSynthRenderConfig::default(),
        midi_path,
        soundfonts: vec![],
        layer_count: Some(4),
        out_path,
        stats_callback: |_| {},
    }
}

impl<'a, ProgressCallback: FnMut(XSynthRenderStats)> XSynthRenderBuilder<'a, ProgressCallback> {
    // Config functions
    pub fn with_config(mut self, config: XSynthRenderConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_channel_count(mut self, channels: u32) -> Self {
        self.config.channel_count = channels;
        self
    }

    pub fn use_threadpool(mut self, use_threadpool: bool) -> Self {
        self.config.use_threadpool = use_threadpool;
        self
    }

    pub fn use_limiter(mut self, use_limiter: bool) -> Self {
        self.config.use_limiter = use_limiter;
        self
    }

    pub fn with_sample_rate(mut self, sample_rate: u32) -> Self {
        self.config.sample_rate = sample_rate;
        self
    }

    pub fn with_audio_channels(mut self, audio_channels: u16) -> Self {
        self.config.audio_channels = audio_channels;
        self
    }

    /// Unused because only WAV is supported
    pub fn _with_audio_format(mut self, audio_format: XSynthRenderAudioFormat) -> Self {
        self.config.audio_format = audio_format;
        self
    }

    pub fn with_layer_count(mut self, layers: Option<usize>) -> Self {
        self.layer_count = layers;
        self
    }

    // Set up functions
    pub fn add_soundfonts(mut self, soundfonts: Vec<Arc<dyn SoundfontBase>>) -> Self {
        self.soundfonts.extend(soundfonts);
        self
    }

    pub fn with_progress_callback<F: FnMut(XSynthRenderStats)>(
        self,
        stats_callback: F,
    ) -> XSynthRenderBuilder<'a, F> {
        XSynthRenderBuilder {
            config: self.config,
            midi_path: self.midi_path,
            soundfonts: self.soundfonts,
            layer_count: self.layer_count,
            out_path: self.out_path,
            stats_callback,
        }
    }

    pub fn run(mut self) -> Result<(), XSynthRenderError> {
        let mut synth = XSynthRender::new(self.config.clone(), self.out_path.into());

        synth.send_event(SynthEvent::ChannelConfig(
            ChannelConfigEvent::SetSoundfonts(
                self.soundfonts
                    .drain(..)
                    .collect::<Vec<Arc<dyn SoundfontBase>>>(),
            ),
        ));

        synth.send_event(SynthEvent::ChannelConfig(
            ChannelConfigEvent::SetLayerCount(self.layer_count),
        ));

        let midi = MIDIFile::open(self.midi_path, None)?;

        let ppq = midi.ppq();
        let merged = pipe!(
            midi.iter_all_track_events_merged_batches()
            |>TimeCaster::<f64>::cast_event_delta()
            |>cancel_tempo_events(250000)
            |>scale_event_time(1.0 / ppq as f64)
            |>unwrap_items()
        );

        let mut pos: f64 = 0.0;

        for batch in merged {
            if batch.delta > 0.0 {
                synth.render_batch(batch.delta);
                pos += batch.delta;
            }
            for e in batch.iter_events() {
                (self.stats_callback)(XSynthRenderStats {
                    progress: pos,
                    voice_count: synth.voice_count(),
                });
                match e.as_event() {
                    Event::NoteOn(e) => {
                        synth.send_event(SynthEvent::Channel(
                            e.channel as u32,
                            ChannelAudioEvent::NoteOn {
                                key: e.key,
                                vel: e.velocity,
                            },
                        ));
                    }
                    Event::NoteOff(e) => {
                        synth.send_event(SynthEvent::Channel(
                            e.channel as u32,
                            ChannelAudioEvent::NoteOff { key: e.key },
                        ));
                    }
                    Event::ControlChange(e) => {
                        synth.send_event(SynthEvent::Channel(
                            e.channel as u32,
                            ChannelAudioEvent::Control(ControlEvent::Raw(e.controller, e.value)),
                        ));
                    }
                    Event::PitchWheelChange(e) => {
                        synth.send_event(SynthEvent::Channel(
                            e.channel as u32,
                            ChannelAudioEvent::Control(ControlEvent::PitchBendValue(
                                e.pitch as f32 / 8192.0,
                            )),
                        ));
                    }
                    Event::ProgramChange(e) => {
                        synth.send_event(SynthEvent::Channel(
                            e.channel as u32,
                            ChannelAudioEvent::ProgramChange(e.program),
                        ));
                    }
                    _ => {}
                }
            }
        }
        synth.send_event(SynthEvent::AllChannels(ChannelAudioEvent::AllNotesOff));
        synth.send_event(SynthEvent::AllChannels(ChannelAudioEvent::ResetControl));
        synth.finalize();

        Ok(())
    }
}
