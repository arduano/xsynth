pub mod resample;
pub mod sf2;
pub mod sfz;

#[derive(Debug, PartialEq, Clone, Copy, Default)]
pub enum FilterType {
    LowPassPole,
    #[default]
    LowPass,
    HighPass,
    BandPass,
}

#[derive(Debug, PartialEq, Clone, Copy, Default)]
pub enum LoopMode {
    #[default]
    NoLoop,
    OneShot,
    LoopContinuous,
    LoopSustain,
}

pub fn convert_sample_index(idx: u32, old_sample_rate: u32, new_sample_rate: u32) -> u32 {
    (new_sample_rate as f32 * idx as f32 / old_sample_rate as f32).round() as u32
}
