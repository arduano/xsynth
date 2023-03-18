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
    events::Event,
    io::MIDIFile,
    pipe,
    sequence::{
        event::{cancel_tempo_events, scale_event_time},
        unwrap_items, TimeCaster,
    },
};
use xsynth_realtime::{config::XSynthRealtimeConfig, RealtimeSynth, SynthEvent};

fn main() {
    let args = std::env::args().collect::<Vec<String>>();
    let (Some(midi), Some(sfz)) =
        (args.get(1).cloned().or_else(|| std::env::var("XSYNTH_EXAMPLE_MIDI").ok()),
         args.get(2).cloned().or_else(|| std::env::var("XSYNTH_EXAMPLE_SFZ").ok())) else {
        println!(
            "Usage: {} [midi] [sfz]",
            std::env::current_exe()
                .unwrap_or("example".into())
                .display()
        );
        return;
    };

    let config = XSynthRealtimeConfig {
        render_window_ms: 0.5,
        use_threadpool: true,
        ..Default::default()
    };

    let synth = RealtimeSynth::open_with_default_output(config);
    let mut sender = synth.get_senders();

    let params = synth.stream_params();

    println!("Loading Soundfont");
    let soundfonts: Vec<Arc<dyn SoundfontBase>> = vec![Arc::new(
        SampleSoundfont::new(sfz, params, Default::default()).unwrap(),
    )];
    println!("Loaded");

    sender.send_config(ChannelConfigEvent::SetSoundfonts(soundfonts));
    sender.send_config(ChannelConfigEvent::SetLayerCount(Some(8)));

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

    let midi = MIDIFile::open(&midi, None).unwrap();

    let ppq = midi.ppq();
    let merged = pipe!(
        midi.iter_all_events_merged_batches()
        |>TimeCaster::<f64>::cast_event_delta()
        |>cancel_tempo_events(250000)
        |>scale_event_time(1.0 / ppq as f64)
        |>unwrap_items()
    );

    let (snd, rcv) = crossbeam_channel::bounded(100);

    thread::spawn(move || {
        for batch in merged {
            snd.send(batch).unwrap();
        }
    });

    let now = Instant::now();
    let mut time = 0.0;
    for batch in rcv {
        if batch.delta != 0.0 {
            time += batch.delta;
            let diff = time - now.elapsed().as_secs_f64();
            if diff > 0.0 {
                spin_sleep::sleep(Duration::from_secs_f64(diff));
            }
        }

        for e in batch.iter_inner() {
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
