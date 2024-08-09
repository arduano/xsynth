use crate::{sfz::AmpegEnvelopeParams, LoopMode};
use std::{fs::File, ops::RangeInclusive, path::PathBuf, sync::Arc};

use thiserror::Error;

mod instrument;
mod preset;
mod sample;
mod zone;

/// Errors that can be generated when loading an SF2 file.
#[derive(Error, Debug, Clone)]
pub enum Sf2ParseError {
    #[error("Failed to read file: {0}")]
    FailedToReadFile(PathBuf),

    #[error("Failed to parse file")]
    FailedToParseFile(String),
}

/// Structure that holds the generator and modulator parameters of an SF2 region.
#[derive(Clone, Debug)]
pub struct Sf2Region {
    pub sample: Arc<[Arc<[f32]>]>,
    pub sample_rate: u32,
    pub velrange: RangeInclusive<u8>,
    pub keyrange: RangeInclusive<u8>,
    pub root_key: u8,
    pub volume: f32,
    pub pan: i16,
    pub loop_mode: LoopMode,
    pub loop_start: u32,
    pub loop_end: u32,
    pub offset: u32,
    pub cutoff: Option<f32>,
    pub resonance: f32,
    pub ampeg_envelope: AmpegEnvelopeParams,
    pub fine_tune: i16,
    pub coarse_tune: i16,
}

/// Structure that holds the parameters of an SF2 preset.
#[derive(Clone, Debug)]
pub struct Sf2Preset {
    pub bank: u16,
    pub preset: u16,
    pub regions: Vec<Sf2Region>,
}

/// Parses an SF2 file and returns its presets in a vector.
pub fn load_soundfont(
    sf2_path: impl Into<PathBuf>,
    sample_rate: u32,
) -> Result<Vec<Sf2Preset>, Sf2ParseError> {
    let sf2_path: PathBuf = sf2_path.into();
    let sf2_path: PathBuf = sf2_path
        .canonicalize()
        .map_err(|_| Sf2ParseError::FailedToReadFile(sf2_path.clone()))?;
    let mut file = File::open(sf2_path.clone())
        .map_err(|_| Sf2ParseError::FailedToReadFile(sf2_path.clone()))?;
    let file = &mut file;
    let sf2 = soundfont::SoundFont2::load(file)
        .map_err(|e| Sf2ParseError::FailedToParseFile(format!("{:#?}", e)))?
        .sort_presets();

    let sample_data = sample::Sf2Sample::parse_sf2_samples(
        file,
        sf2.sample_headers,
        sf2.sample_data,
        sample_rate,
    )?;

    let instruments = instrument::Sf2Instrument::parse_instruments(sf2.instruments);

    let presets = preset::Sf2ParsedPreset::parse_presets(sf2.presets);

    Ok(preset::Sf2ParsedPreset::merge_presets(
        sample_data,
        instruments,
        presets,
        sample_rate,
    ))
}
