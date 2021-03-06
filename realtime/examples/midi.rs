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
    events::{Event, MIDIEvent},
    io::MIDIFile,
    pipe,
    sequence::{
        event::{cancel_tempo_events, convert_events_into_batches, scale_event_time},
        unwrap_items, TimeCaster,
    },
};
use xsynth_realtime::{RealtimeSynth, SynthEvent};

fn main() {
    let synth = RealtimeSynth::open_with_all_defaults();
    let mut sender = synth.get_senders();

    let params = synth.stream_params();

    println!("Loading Soundfont");
    let soundfonts: Vec<Arc<dyn SoundfontBase>> = vec![Arc::new(
        SampleSoundfont::new(
            "D:/Midis/Steinway-B-211-master/Steinway-B-211-master/Presets/1960 Steinway B-211.sfz",
            params.clone(),
        )
        .unwrap(),
    )];
    println!("Loaded");

    sender.send_config(ChannelConfigEvent::SetSoundfonts(soundfonts));

    let stats = synth.get_stats();
    thread::spawn(move || loop {
        println!(
            "Voice Count: {}  \tBuffer: {}\tRender time: {}",
            stats.voice_count(),
            stats.buffer().last_samples_after_read(),
            stats.buffer().average_renderer_load()
        );
        thread::sleep(Duration::from_millis(10));
    });

    let midi = MIDIFile::open("D:/Midis/The Quarantine Project.mid", None).unwrap();

    let ppq = midi.ppq();
    let merged = pipe!(
        midi.iter_all_events_merged()
        |>TimeCaster::<f64>::cast_event_delta()
        |>cancel_tempo_events(250000)
        |>convert_events_into_batches()
        |>scale_event_time(1.0 / ppq as f64)
        |>unwrap_items()
    );

    let (tx, rx) = crossbeam_channel::unbounded();
    let max_channel_len = midi.ppq() as usize * 120;

    thread::spawn(move || {
        for e in merged {
            tx.send(e).unwrap();
            if tx.len() > max_channel_len {
                thread::sleep(Duration::from_millis(100));
            }
        }
    });

    let now = Instant::now() - Duration::from_secs_f64(0.0);
    let mut time = 0.0;
    while let Ok(batch) = rx.recv() {
        if batch.delta() != 0.0 {
            time += batch.delta();
            let diff = time - now.elapsed().as_secs_f64();
            if diff > 0.0 {
                spin_sleep::sleep(Duration::from_secs_f64(diff));
            }
        }

        for e in batch.into_iter() {
            match e {
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
                _ => {}
            }
        }
    }

    std::thread::sleep(Duration::from_secs(10000));
}
