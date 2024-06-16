use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use core::{
    channel::{ChannelAudioEvent, ChannelConfigEvent, ControlEvent},
    soundfont::{SampleSoundfont, SoundfontBase},
};

use midi_toolkit::{
    events::{Event, MIDIEventEnum},
    io::MIDIFile,
    pipe,
    sequence::{
        event::{cancel_tempo_events, scale_event_time},
        unwrap_items, TimeCaster,
    },
};
use xsynth_realtime::{RealtimeSynth, SynthEvent};

fn main() {
    let args = std::env::args().collect::<Vec<String>>();
    let (Some(midi), Some(sfz)) = (
        args.get(1)
            .cloned()
            .or_else(|| std::env::var("XSYNTH_EXAMPLE_MIDI").ok()),
        args.get(2)
            .cloned()
            .or_else(|| std::env::var("XSYNTH_EXAMPLE_SF").ok()),
    ) else {
        println!(
            "Usage: {} [midi] [sfz/sf2]",
            std::env::current_exe()
                .unwrap_or("example".into())
                .display()
        );
        return;
    };

    let synth = RealtimeSynth::open_with_all_defaults();
    let mut sender = synth.get_senders();

    let params = synth.stream_params();

    let soundfonts: Vec<Arc<dyn SoundfontBase>> = vec![Arc::new(
        SampleSoundfont::new(sfz, params, Default::default()).unwrap(),
    )];

    sender.send_config(ChannelConfigEvent::SetSoundfonts(soundfonts));

    let midi = MIDIFile::open(&midi, None).unwrap();

    let ppq = midi.ppq();
    let merged = pipe!(
        midi.iter_all_events_merged()
        |>TimeCaster::<f64>::cast_event_delta()
        |>cancel_tempo_events(250000)
        |>scale_event_time(1.0 / ppq as f64)
        |>unwrap_items()
    );

    let collected = merged.collect::<Vec<_>>();

    let stats = synth.get_stats();
    thread::spawn(move || loop {
        println!(
            "Voice Count: {}\tBuffer: {}\tRender time: {}",
            stats.voice_count(),
            stats.buffer().last_samples_after_read(),
            stats.buffer().average_renderer_load()
        );
        thread::sleep(Duration::from_millis(10));
    });

    let now = Instant::now();
    let mut time = 0.0;
    for e in collected.into_iter() {
        if e.delta != 0.0 {
            time += e.delta;
            let diff = time - now.elapsed().as_secs_f64();
            if diff > 0.0 {
                spin_sleep::sleep(Duration::from_secs_f64(diff));
            }
        }

        match e.as_event() {
            Event::NoteOn(e) => {
                sender.send_event(SynthEvent::Channel(
                    e.channel as u32,
                    ChannelAudioEvent::NoteOn {
                        key: e.key,
                        vel: e.velocity,
                    },
                ));
            }
            Event::NoteOff(e) => {
                sender.send_event(SynthEvent::Channel(
                    e.channel as u32,
                    ChannelAudioEvent::NoteOff { key: e.key },
                ));
            }
            Event::ControlChange(e) => {
                sender.send_event(SynthEvent::Channel(
                    e.channel as u32,
                    ChannelAudioEvent::Control(ControlEvent::Raw(e.controller, e.value)),
                ));
            }
            Event::PitchWheelChange(e) => {
                sender.send_event(SynthEvent::Channel(
                    e.channel as u32,
                    ChannelAudioEvent::Control(ControlEvent::PitchBendValue(
                        e.pitch as f32 / 8192.0,
                    )),
                ));
            }
            Event::ProgramChange(e) => {
                sender.send_event(SynthEvent::Channel(
                    e.channel as u32,
                    ChannelAudioEvent::ProgramChange(e.program),
                ));
            }
            _ => {}
        }
    }

    std::thread::sleep(Duration::from_secs(10000));
}
