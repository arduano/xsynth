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
