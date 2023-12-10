use xsynth_core::channel::ChannelInitOptions;
use std::ops::RangeInclusive;

pub struct XSynthRealtimeConfig {
    pub channel_init_options: ChannelInitOptions,
    pub render_window_ms: f64,
    pub channel_count: u32,
    pub drums_channels: Vec<u32>,
    pub use_threadpool: bool,
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
