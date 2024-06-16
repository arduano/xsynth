use atomic_float::AtomicF64;
use core::{
    soundfont::{SampleSoundfont, SoundfontBase},
    AudioStreamParams, ChannelCount,
};
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use xsynth_render::{builder::xsynth_renderer, XSynthRenderConfig, XSynthRenderStats};

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
    let out = "out.wav";

    println!("\n--- STARTING RENDER ---");

    let render_time = Instant::now();
    let position = Arc::new(AtomicF64::new(0.0));
    let voices = Arc::new(AtomicU64::new(0));

    let max_voices = Arc::new(AtomicU64::new(0));

    let callback = |stats: XSynthRenderStats| {
        position.store(stats.progress, Ordering::Relaxed);
        voices.store(stats.voice_count, Ordering::Relaxed);
        if stats.voice_count > max_voices.load(Ordering::Relaxed) {
            max_voices.store(stats.voice_count, Ordering::Relaxed);
        }
    };

    let position_thread = position.clone();
    let voices_thread = voices.clone();

    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(100));
        let pos = position_thread.load(Ordering::Relaxed);
        let time = Duration::from_secs_f64(pos);
        println!(
            "Progress: {:?}, Voice Count: {}",
            time,
            voices_thread.load(Ordering::Relaxed)
        );
    });

    let config: XSynthRenderConfig = Default::default();

    let soundfonts: Vec<Arc<dyn SoundfontBase>> = vec![Arc::new(
        SampleSoundfont::new(
            sfz,
            AudioStreamParams::new(
                config.sample_rate,
                ChannelCount::from(config.audio_channels),
            ),
            config.sf_init_options,
        )
        .unwrap(),
    )];

    xsynth_renderer(&midi, out)
        .with_config(config)
        .add_soundfonts(soundfonts)
        .with_layer_count(Some(128))
        .with_progress_callback(callback)
        .run()
        .unwrap();

    println!("Render time: {} seconds", render_time.elapsed().as_secs());
}
