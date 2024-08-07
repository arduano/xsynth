pub use xsynth_core::{
    channel_group::ChannelGroupConfig, soundfont::SoundfontInitOptions, AudioStreamParams,
};

/// Supported audio formats of XSynthRender.
#[derive(PartialEq, Clone, Copy)]
pub enum XSynthRenderAudioFormat {
    Wav,
}

/// Options for initializing a new XSynthRender object.
#[derive(Clone)]
pub struct XSynthRenderConfig {
    /// Synthesizer initialization options.
    /// See the `ChannelGroupConfig` documentation for more information.
    pub group_options: ChannelGroupConfig,

    /// If set to true, the rendered audio will be limited to 0dB using
    /// the `VolumeLimiter` effect from `core` to prevent clipping.
    pub use_limiter: bool,

    /// Audio output format. Supported: WAV
    pub audio_format: XSynthRenderAudioFormat,
}
