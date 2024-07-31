use core::channel::ChannelInitOptions;
use std::ops::RangeInclusive;

/// Options for initializing a new RealtimeSynth.
pub struct XSynthRealtimeConfig {
    /// Channel initialization options (same for all channels).
    pub channel_init_options: ChannelInitOptions,

    /// The length of the buffer reader in ms.
    ///
    /// Default: `10.0`
    pub render_window_ms: f64,

    /// nt of VoiceChannel objects to be created.
    /// (Number of MIDI channels)
    ///
    /// Default: `16`
    pub channel_count: u32,

    /// A vector which specifies which of the created channels (indexes) will be used for drums.
    /// For example in a conventional 16 MIDI channel setup where channel 10 is used for
    /// drums, the vector would be set as \[9\] (counting from 0).
    ///
    /// Default: `[9]`
    pub drums_channels: Vec<u32>,

    /// Whether or not to use a threadpool to render voices.
    ///
    /// Default: `false`
    pub use_threadpool: bool,

    /// A range of velocities that will not be played.
    ///
    /// Default: `0..=0`
    pub ignore_range: RangeInclusive<u8>,
}

impl Default for XSynthRealtimeConfig {
    fn default() -> Self {
        Self {
            channel_init_options: Default::default(),
            render_window_ms: 10.0,
            channel_count: 16,
            drums_channels: vec![9],
            use_threadpool: false,
            ignore_range: 0..=0,
        }
    }
}
