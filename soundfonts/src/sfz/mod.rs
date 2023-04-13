use std::{
    collections::VecDeque,
    ops::RangeInclusive,
    path::{Path, PathBuf},
};

use self::parse::{
    parse_tokens_resolved, SfzAmpegEnvelope, SfzGroupType, SfzOpcode, SfzParseError, SfzToken,
};

use crate::{FilterType, LoopMode};

pub mod grammar;
pub mod parse;

#[derive(Debug, Clone)]
pub struct AmpegEnvelopeParams {
    pub ampeg_start: f32,
    pub ampeg_delay: f32,
    pub ampeg_attack: f32,
    pub ampeg_hold: f32,
    pub ampeg_decay: f32,
    pub ampeg_sustain: f32,
    pub ampeg_release: f32,
}

impl Default for AmpegEnvelopeParams {
    fn default() -> Self {
        AmpegEnvelopeParams {
            ampeg_start: 0.0,
            ampeg_delay: 0.0,
            ampeg_attack: 0.001,
            ampeg_hold: 0.0,
            ampeg_decay: 0.0,
            ampeg_sustain: 100.0,
            ampeg_release: 0.001,
        }
    }
}

impl AmpegEnvelopeParams {
    fn update_from_flag(&mut self, flag: SfzAmpegEnvelope) {
        match flag {
            SfzAmpegEnvelope::AmpegStart(val) => self.ampeg_start = val,
            SfzAmpegEnvelope::AmpegDelay(val) => self.ampeg_delay = val,
            SfzAmpegEnvelope::AmpegAttack(val) => self.ampeg_attack = val,
            SfzAmpegEnvelope::AmpegHold(val) => self.ampeg_hold = val,
            SfzAmpegEnvelope::AmpegDecay(val) => self.ampeg_decay = val,
            SfzAmpegEnvelope::AmpegSustain(val) => self.ampeg_sustain = val,
            SfzAmpegEnvelope::AmpegRelease(val) => self.ampeg_release = val,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RegionParamsBuilder {
    lovel: u8,
    hivel: u8,
    lokey: u8,
    hikey: u8,
    pitch_keycenter: u8,
    volume: i16,
    pan: i8,
    sample: Option<String>,
    default_path: Option<String>,
    loop_mode: LoopMode,
    loop_start: u32,
    loop_end: u32,
    offset: u32,
    cutoff: Option<f32>,
    fil_veltrack: i16,
    fil_keycenter: u8,
    fil_keytrack: i16,
    filter_type: FilterType,
    ampeg_envelope: AmpegEnvelopeParams,
}

impl Default for RegionParamsBuilder {
    fn default() -> Self {
        RegionParamsBuilder {
            lovel: 0,
            hivel: 127,
            lokey: 0,
            hikey: 127,
            pitch_keycenter: 60,
            volume: 0,
            pan: 0,
            sample: None,
            default_path: None,
            loop_mode: LoopMode::NoLoop,
            loop_start: 0,
            loop_end: 0,
            offset: 0,
            cutoff: None,
            fil_veltrack: 0,
            fil_keycenter: 60,
            fil_keytrack: 0,
            filter_type: FilterType::default(),
            ampeg_envelope: AmpegEnvelopeParams::default(),
        }
    }
}

impl RegionParamsBuilder {
    fn update_from_flag(&mut self, flag: SfzOpcode) {
        match flag {
            SfzOpcode::Lovel(val) => self.lovel = val,
            SfzOpcode::Hivel(val) => self.hivel = val,
            SfzOpcode::Key(val) => {
                self.lokey = val;
                self.hikey = val;
                self.pitch_keycenter = val;
            }
            SfzOpcode::Lokey(val) => self.lokey = val,
            SfzOpcode::Hikey(val) => self.hikey = val,
            SfzOpcode::PitchKeycenter(val) => self.pitch_keycenter = val,
            SfzOpcode::Pan(val) => self.pan = val,
            SfzOpcode::Volume(val) => self.volume = val,
            SfzOpcode::Sample(val) => self.sample = Some(val),
            SfzOpcode::LoopMode(val) => self.loop_mode = val,
            SfzOpcode::LoopStart(val) => self.loop_start = val,
            SfzOpcode::LoopEnd(val) => self.loop_end = val,
            SfzOpcode::Offset(val) => self.offset = val,
            SfzOpcode::Cutoff(val) => self.cutoff = Some(val),
            SfzOpcode::FilVeltrack(val) => self.fil_veltrack = val,
            SfzOpcode::FilKeytrack(val) => self.fil_keytrack = val,
            SfzOpcode::FilKeycenter(val) => self.fil_keycenter = val,
            SfzOpcode::FilterType(val) => self.filter_type = val,
            SfzOpcode::DefaultPath(val) => self.default_path = Some(val),
            SfzOpcode::AmpegEnvelope(flag) => self.ampeg_envelope.update_from_flag(flag),
        }
    }

    fn build(self, base_path: &Path) -> Option<RegionParams> {
        let relative_sample_path = if let Some(default_path) = self.default_path {
            PathBuf::from(default_path).join(self.sample?)
        } else {
            self.sample?.into()
        };

        let mut sample_path = base_path.join(relative_sample_path);
        match sample_path.canonicalize() {
            Ok(path) => sample_path = path,
            Err(_) => return None,
        }

        Some(RegionParams {
            velrange: self.lovel..=self.hivel,
            keyrange: self.lokey..=self.hikey,
            pitch_keycenter: self.pitch_keycenter,
            volume: self.volume,
            pan: self.pan,
            sample_path,
            loop_mode: self.loop_mode,
            loop_start: self.loop_start,
            loop_end: self.loop_end,
            offset: self.offset,
            cutoff: self.cutoff,
            fil_veltrack: self.fil_veltrack.clamp(-9600, 9600),
            fil_keycenter: self.fil_keycenter,
            fil_keytrack: self.fil_keytrack.clamp(0, 1200),
            filter_type: self.filter_type,
            ampeg_envelope: self.ampeg_envelope,
        })
    }
}

#[derive(Debug, Clone)]
pub struct RegionParams {
    pub velrange: RangeInclusive<u8>,
    pub keyrange: RangeInclusive<u8>,
    pub pitch_keycenter: u8,
    pub volume: i16,
    pub pan: i8,
    pub sample_path: PathBuf,
    pub loop_mode: LoopMode,
    pub loop_start: u32,
    pub loop_end: u32,
    pub offset: u32,
    pub cutoff: Option<f32>,
    pub fil_veltrack: i16,
    pub fil_keycenter: u8,
    pub fil_keytrack: i16,
    pub filter_type: FilterType,
    pub ampeg_envelope: AmpegEnvelopeParams,
}

fn get_group_level(group_type: SfzGroupType) -> Option<usize> {
    match group_type {
        SfzGroupType::Control => Some(1),
        SfzGroupType::Global => Some(2),
        SfzGroupType::Master => Some(3),
        SfzGroupType::Group => Some(4),
        SfzGroupType::Region => Some(5),
        SfzGroupType::Other => None,
    }
}

fn parse_sf_root(tokens: impl Iterator<Item = SfzToken>, base_path: PathBuf) -> Vec<RegionParams> {
    let mut current_group = None;

    let mut group_data_stack = VecDeque::<RegionParamsBuilder>::new();

    let mut regions = Vec::new();

    for token in tokens {
        match token {
            SfzToken::Group(group) => {
                if current_group == Some(SfzGroupType::Region) {
                    // Step outside of the current group
                    // Unwrapping is safe because if the group is Region then there's always at least one item
                    let next_region = group_data_stack.pop_back().unwrap();
                    if let Some(built) = next_region.build(&base_path) {
                        regions.push(built);
                    }
                }

                if let Some(group_level) = get_group_level(group) {
                    current_group = Some(group);

                    // If stepping inside
                    while group_data_stack.len() < group_level {
                        let parent_group = group_data_stack.back().cloned().unwrap_or_default();
                        group_data_stack.push_back(parent_group);
                    }

                    // If stepping outside
                    while group_data_stack.len() > group_level {
                        group_data_stack.pop_back();
                    }
                } else {
                    current_group = None;
                }
            }
            SfzToken::Opcode(flag) => {
                if current_group.is_some() {
                    if let Some(group_data) = group_data_stack.back_mut() {
                        group_data.update_from_flag(flag);
                    }
                }
            }
        }
    }

    if current_group == Some(SfzGroupType::Region) {
        // Unwrapping is safe because if the group is Region then there's always at least one item
        let next_region = group_data_stack.pop_back().unwrap();
        if let Some(built) = next_region.build(&base_path) {
            regions.push(built);
        }
    }

    regions
}

pub fn parse_soundfont(sfz_path: impl Into<PathBuf>) -> Result<Vec<RegionParams>, SfzParseError> {
    let sfz_path = sfz_path.into();
    let sfz_path: PathBuf = sfz_path
        .canonicalize()
        .map_err(|_| SfzParseError::FailedToReadFile(sfz_path))?;

    let tokens = parse_tokens_resolved(&sfz_path)?;

    // Unwrap here is safe because the path is confirmed to be a file due to `parse_all_tokens`
    // and therefore it will always have a parent folder. The path is also canonicalized.
    let parent_path = sfz_path.parent().unwrap().into();

    let regions = parse_sf_root(tokens.into_iter(), parent_path);

    Ok(regions)
}
