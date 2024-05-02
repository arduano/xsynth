pub mod config;
mod util;

pub use xsynth_core::channel_group::SynthEvent;

mod realtime_synth;
pub use realtime_synth::*;

mod event_senders;
pub use event_senders::*;
