use std::{collections::VecDeque, io, path::PathBuf};

use self::lexer::{
    parse_all_tokens, SfzAmpegEnvelope, SfzGroupType, SfzLoopMode, SfzRegionFlags, SfzToken,
};

mod lexer;

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
            ampeg_attack: 0.0,
            ampeg_hold: 0.0,
            ampeg_decay: 0.0,
            ampeg_sustain: 1.0,
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
    key: Option<u8>,
    pitch_keycenter: Option<u8>,
    pan: i8,
    sample: Option<String>,
    default_path: Option<String>,
    loop_mode: SfzLoopMode,
    ampeg_envelope: AmpegEnvelopeParams,
}

impl Default for RegionParamsBuilder {
    fn default() -> Self {
        RegionParamsBuilder {
            lovel: 0,
            hivel: 127,
            key: None,
            pitch_keycenter: None,
            pan: 0,
            sample: None,
            default_path: None,
            loop_mode: SfzLoopMode::NoLoop,
            ampeg_envelope: AmpegEnvelopeParams::default(),
        }
    }
}

impl RegionParamsBuilder {
    fn update_from_flag(&mut self, flag: SfzRegionFlags) {
        match flag {
            SfzRegionFlags::Lovel(val) => self.lovel = val,
            SfzRegionFlags::Hivel(val) => self.hivel = val,
            SfzRegionFlags::Key(val) => self.key = Some(val),
            SfzRegionFlags::PitchKeycenter(val) => self.pitch_keycenter = Some(val),
            SfzRegionFlags::Pan(val) => self.pan = val,
            SfzRegionFlags::Sample(val) => self.sample = Some(val),
            SfzRegionFlags::LoopMode(val) => self.loop_mode = val,
            SfzRegionFlags::DefaultPath(val) => self.default_path = Some(val),
            SfzRegionFlags::AmpegEnvelope(flag) => self.ampeg_envelope.update_from_flag(flag),
        }
    }

    fn build(self, base_path: &PathBuf) -> Option<RegionParams> {
        let relative_sample_path = if let Some(default_path) = self.default_path {
            PathBuf::from(default_path).join(self.sample?)
        } else {
            self.sample?.into()
        };

        let sample_path = base_path.join(relative_sample_path);

        Some(RegionParams {
            lovel: self.lovel,
            hivel: self.hivel,
            key: self.key?,
            pitch_keycenter: self.pitch_keycenter,
            pan: self.pan,
            sample_path,
            loop_mode: self.loop_mode,
            ampeg_envelope: self.ampeg_envelope,
        })
    }
}

#[derive(Debug, Clone)]
pub struct RegionParams {
    pub lovel: u8,
    pub hivel: u8,
    pub key: u8,
    pub pitch_keycenter: Option<u8>,
    pub pan: i8,
    pub sample_path: PathBuf,
    pub loop_mode: SfzLoopMode,
    pub ampeg_envelope: AmpegEnvelopeParams,
}

fn get_group_level(group_type: SfzGroupType) -> Option<usize> {
    match group_type {
        SfzGroupType::Control => Some(1),
        SfzGroupType::Master => Some(2),
        SfzGroupType::Group => Some(3),
        SfzGroupType::Region => Some(4),
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
            SfzToken::RegionFlag(flag) => {
                if current_group != None {
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

pub fn parse_soundfont(sfz_path: impl Into<PathBuf>) -> io::Result<Vec<RegionParams>> {
    let sfz_path: PathBuf = sfz_path.into().canonicalize()?;

    let tokens = parse_all_tokens(&sfz_path)?;

    // Unwrap here is safe because the path is confirmed to be a file due to `parse_all_tokens`
    // and therefore it will always have a parent folder. The path is also canonicalized.
    let parent_path = sfz_path.parent().unwrap().into();

    let regions = parse_sf_root(tokens.into_iter(), parent_path);

    Ok(regions)
}
