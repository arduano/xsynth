use std::{
    borrow::Cow,
    cell::RefCell,
    collections::HashMap,
    fs::File,
    io::{BufReader, Read},
    ops::RangeInclusive,
    path::{Path, PathBuf},
};

use crate::{FilterType, LoopMode};
use encoding_rs::UTF_8;
use encoding_rs_io::DecodeReaderBytesBuilder;

use super::grammar::{ErrorTolerantToken, Group, Opcode, Token, TokenKind};
use regex_bnf::{FileLocation, ParseError};
use thiserror::Error;

#[derive(Debug, Clone)]
pub enum SfzOpcode {
    Lovel(u8),
    Hivel(u8),
    Key(u8),
    Lokey(u8),
    Hikey(u8),
    PitchKeycenter(u8),
    Volume(i16),
    Pan(i8),
    Sample(String),
    LoopMode(LoopMode),
    LoopStart(u32),
    LoopEnd(u32),
    Offset(u32),
    Cutoff(f32),
    FilVeltrack(i16),
    FilKeycenter(u8),
    FilKeytrack(i16),
    FilterType(FilterType),
    DefaultPath(String),
    AmpegEnvelope(SfzAmpegEnvelope),
}

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub enum SfzAmpegEnvelope {
    AmpegStart(f32),
    AmpegDelay(f32),
    AmpegAttack(f32),
    AmpegHold(f32),
    AmpegDecay(f32),
    AmpegSustain(f32),
    AmpegRelease(f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SfzGroupType {
    Region,
    Group,
    Master,
    Global,
    Control,
    Other,
}

#[derive(Debug, Clone)]
pub enum SfzToken {
    Group(SfzGroupType),
    Opcode(SfzOpcode),
}

#[derive(Debug, Clone)]
pub enum SfzTokenWithMeta {
    Group(SfzGroupType),
    Opcode(SfzOpcode),
    Import(String),
    Define(String, String),
}

#[derive(Error, Debug, Clone)]
pub struct SfzValidationError {
    pub pos: FileLocation,
    pub message: String,
}

impl SfzValidationError {
    pub fn new(pos: FileLocation, message: String) -> Self {
        Self { pos, message }
    }
}

impl std::fmt::Display for SfzValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at {}", self.message, self.pos)
    }
}

#[derive(Error, Debug, Clone)]
pub enum SfzParseError {
    #[error("Failed to parse SFZ file: {0}")]
    GrammarError(#[from] ParseError),

    #[error("Failed to parse SFZ file: {0}")]
    ValidationError(#[from] SfzValidationError),

    #[error("Failed to read file: {0}")]
    FailedToReadFile(PathBuf),
}

fn parse_key_number(val: &str) -> Option<u8> {
    match val.parse::<u8>().ok() {
        Some(val) => Some(val.clamp(0, 128)),
        None => {
            let note: String = val
                .chars()
                .filter(|c| !(c.is_ascii_digit() || c == &'-'))
                .collect();
            let semitone: i16 = match note.to_lowercase().as_str() {
                "c" => 0,
                "c#" => 1,
                "db" => 1,
                "d" => 2,
                "d#" => 3,
                "eb" => 3,
                "e" => 4,
                "f" => 5,
                "f#" => 6,
                "gb" => 6,
                "g" => 7,
                "g#" => 8,
                "ab" => 8,
                "a" => 9,
                "a#" => 10,
                "bb" => 10,
                "b" => 11,
                _ => return None,
            };
            let octave: String = val
                .chars()
                .filter(|c| c.is_ascii_digit() || c == &'-')
                .collect();
            let octave: i16 = octave.parse().ok().unwrap_or(-10);
            if octave < -1 {
                None
            } else {
                let midi_note = 12 + semitone + octave * 12;
                Some(midi_note.clamp(0, 128) as u8)
            }
        }
    }
}

fn parse_u8_in_range(val: &str, range: RangeInclusive<u8>) -> Option<u8> {
    val.parse()
        .ok()
        .map(|val: u8| val.clamp(*range.start(), *range.end()))
}

fn parse_i8_in_range(val: &str, range: RangeInclusive<i8>) -> Option<i8> {
    val.parse()
        .ok()
        .map(|val: i8| val.clamp(*range.start(), *range.end()))
}

fn parse_i16_in_range(val: &str, range: RangeInclusive<i16>) -> Option<i16> {
    val.parse()
        .ok()
        .map(|val: i16| val.clamp(*range.start(), *range.end()))
}

fn parse_u32_in_range(val: &str, range: RangeInclusive<u32>) -> Option<u32> {
    val.parse()
        .ok()
        .map(|val: u32| val.clamp(*range.start(), *range.end()))
}

fn parse_float_in_range(val: &str, range: RangeInclusive<f32>) -> Option<f32> {
    val.parse()
        .ok()
        .map(|val: f32| val.clamp(*range.start(), *range.end()))
}

fn parse_filter_kind(val: &str) -> Option<FilterType> {
    match val {
        "lpf_1p" => Some(FilterType::LowPassPole),
        "lpf_2p" => Some(FilterType::LowPass),
        "lpf_4p" => Some(FilterType::LowPass),
        "lpf_6p" => Some(FilterType::LowPass),
        "hpf_1p" => Some(FilterType::HighPass),
        "hpf_2p" => Some(FilterType::HighPass),
        "hpf_4p" => Some(FilterType::HighPass),
        "hpf_6p" => Some(FilterType::HighPass),
        "bpf_1p" => Some(FilterType::BandPass),
        "bpf_2p" => Some(FilterType::BandPass),
        _ => None,
    }
}

fn parse_loop_mode(val: &str) -> Option<LoopMode> {
    match val {
        "no_loop" => Some(LoopMode::NoLoop),
        "one_shot" => Some(LoopMode::OneShot),
        "loop_continuous" => Some(LoopMode::LoopContinuous),
        "loop_sustain" => Some(LoopMode::LoopSustain),
        _ => None,
    }
}

fn parse_sfz_opcode(
    opcode: Opcode,
    defines: &RefCell<HashMap<String, String>>,
) -> Result<Option<SfzOpcode>, SfzValidationError> {
    let name = opcode.name.name.text;
    let mut name = Cow::Borrowed(name.trim());

    let val = opcode.value.as_string();
    let mut val = Cow::Borrowed(val.trim());

    for (key, replace) in defines.borrow().iter() {
        if val.contains(key) {
            val = Cow::Owned(val.replace(key, replace));
        }
        if name.contains(key) {
            name = Cow::Owned(name.replace(key, replace));
        }
    }

    use SfzAmpegEnvelope::*;
    use SfzOpcode::*;

    let val = val.as_ref();
    let name = name.as_ref();

    Ok(match name {
        "lokey" => parse_key_number(val).map(Lokey),
        "hikey" => parse_key_number(val).map(Hikey),
        "lovel" => parse_u8_in_range(val, 0..=128).map(Lovel),
        "hivel" => parse_u8_in_range(val, 0..=128).map(Hivel),
        "volume" => parse_i16_in_range(val, -144..=6).map(Volume),
        "pan" => parse_i8_in_range(val, -100..=100).map(Pan),
        "pitch_keycenter" => parse_key_number(val).map(PitchKeycenter),
        "key" => parse_key_number(val).map(Key),
        "cutoff" => parse_float_in_range(val, 1.0..=100000.0).map(Cutoff),
        "fil_veltrack" => parse_i16_in_range(val, -9600..=9600).map(FilVeltrack),
        "fil_keytrack" => parse_i16_in_range(val, 0..=1200).map(FilKeytrack),
        "fil_keycenter" => parse_key_number(val).map(FilKeycenter),
        "fil_type" => parse_filter_kind(val).map(FilterType),
        "loop_mode" | "loopmode" => parse_loop_mode(val).map(LoopMode),
        "loop_start" | "loopstart" => parse_u32_in_range(val, 0..=u32::MAX).map(LoopStart),
        "loop_end" | "loopend" => parse_u32_in_range(val, 0..=u32::MAX).map(LoopEnd),
        "offset" => parse_u32_in_range(val, 0..=u32::MAX).map(Offset),
        "default_path" => Some(DefaultPath(val.replace('\\', "/"))),

        "ampeg_delay" => parse_float_in_range(val, 0.0..=100.0)
            .map(AmpegDelay)
            .map(AmpegEnvelope),
        "ampeg_start" => parse_float_in_range(val, 0.0..=100.0)
            .map(AmpegStart)
            .map(AmpegEnvelope),
        "ampeg_attack" => parse_float_in_range(val, 0.0..=100.0)
            .map(AmpegAttack)
            .map(AmpegEnvelope),
        "ampeg_hold" => parse_float_in_range(val, 0.0..=100.0)
            .map(AmpegHold)
            .map(AmpegEnvelope),
        "ampeg_decay" => parse_float_in_range(val, 0.0..=100.0)
            .map(AmpegDecay)
            .map(AmpegEnvelope),
        "ampeg_sustain" => parse_float_in_range(val, 0.0..=100.0)
            .map(AmpegSustain)
            .map(AmpegEnvelope),
        "ampeg_release" => parse_float_in_range(val, 0.0..=100.0)
            .map(AmpegRelease)
            .map(AmpegEnvelope),

        "sample" => Some(Sample(val.replace('\\', "/"))),

        _ => None,
    })
}

fn parse_sfz_group(group: Group) -> Result<SfzGroupType, SfzValidationError> {
    Ok(match group.name.text {
        "region" => SfzGroupType::Region,
        "group" => SfzGroupType::Group,
        "master" => SfzGroupType::Master,
        "global" => SfzGroupType::Global,
        "control" => SfzGroupType::Control,
        _ => SfzGroupType::Other,
    })
}

fn grammar_token_into_sfz_token(
    token: Token,
    defines: &RefCell<HashMap<String, String>>,
) -> Result<Option<SfzTokenWithMeta>, SfzValidationError> {
    match token.kind {
        TokenKind::Comment(_) => Ok(None),
        TokenKind::Group(group_type) => {
            Ok(Some(SfzTokenWithMeta::Group(parse_sfz_group(group_type)?)))
        }
        TokenKind::Opcode(opcode) => {
            Ok(parse_sfz_opcode(opcode, defines)?.map(SfzTokenWithMeta::Opcode))
        }
        TokenKind::Include(include) => Ok(Some(SfzTokenWithMeta::Import(
            include.path.text.replace('\\', "/"),
        ))),
        TokenKind::Define(define) => {
            let variable = define.variable.text.to_owned();
            let value = define.value.first.value.text.text.to_owned();
            //defines.borrow_mut().insert(variable.clone(), value.clone());
            Ok(Some(SfzTokenWithMeta::Define(variable, value)))
        }
    }
}

pub fn parse_tokens_raw<'a>(
    input: &'a str,
    defines: &'a RefCell<HashMap<String, String>>,
) -> impl 'a + Iterator<Item = Result<SfzTokenWithMeta, SfzParseError>> {
    let iter = ErrorTolerantToken::parse_as_iter(input);

    iter.filter_map(move |t| match t {
        Ok(t) => match grammar_token_into_sfz_token(t, defines) {
            Ok(Some(t)) => Some(Ok(t)),
            Ok(None) => None,
            Err(e) => Some(Err(SfzParseError::from(e))),
        },
        Err(e) => Some(Err(SfzParseError::from(e))),
    })
}

fn parse_tokens_resolved_recursive(
    instr_path: &Path,
    file_path: &Path,
    defines: &RefCell<HashMap<String, String>>,
) -> Result<Vec<SfzToken>, SfzParseError> {
    let file_path = file_path
        .canonicalize()
        .map_err(|_| SfzParseError::FailedToReadFile(file_path.to_owned()))?;

    let f = File::open(&file_path)
        .map_err(|_| SfzParseError::FailedToReadFile(file_path.to_owned()))?;

    let mut reader = BufReader::new(
        DecodeReaderBytesBuilder::new()
            .encoding(Some(UTF_8))
            .build(f),
    );
    let mut file = String::new();

    reader.read_to_string(&mut file).unwrap();

    // Unwrap here is safe because the path is confirmed to be a file (read above)
    // and therefore it will always have a parent folder. The path is also canonicalized.
    let parent_path = instr_path.parent().unwrap();

    let mut tokens = Vec::new();

    let iter = parse_tokens_raw(&file, defines);

    let mut parsed_includes = HashMap::new();

    for t in iter {
        match t {
            Ok(t) => match t {
                SfzTokenWithMeta::Import(mut path) => {
                    for (key, replace) in defines.borrow().iter() {
                        if path.contains(key) {
                            path = path.replace(key, replace);
                        }
                    }

                    // Get the cached tokens for this current path, or parse them if they haven't been parsed yet
                    let parsed_tokens = parsed_includes.entry(path.clone()).or_insert_with(|| {
                        let full_path = parent_path.join(&path);
                        parse_tokens_resolved_recursive(instr_path, &full_path, defines)
                    });

                    if let Ok(parsed_tokens) = parsed_tokens {
                        tokens.extend_from_slice(parsed_tokens);
                    } else {
                        // If we recieved an error, then extact the owned error from the hashmap and return it
                        return Err(parsed_includes.remove(&path).unwrap().unwrap_err());
                    }
                }
                SfzTokenWithMeta::Group(group) => tokens.push(SfzToken::Group(group)),
                SfzTokenWithMeta::Opcode(opcode) => tokens.push(SfzToken::Opcode(opcode)),
                SfzTokenWithMeta::Define(variable, value) => {
                    // We clear the include cache here so if the same file is included
                    // it will use the new definition values
                    parsed_includes.clear();

                    defines
                        .borrow_mut()
                        .insert(variable.trim().to_owned(), value.trim().to_owned());
                }
            },
            Err(e) => return Err(e),
        }
    }

    Ok(tokens)
}

pub fn parse_tokens_resolved(file_path: &Path) -> Result<Vec<SfzToken>, SfzParseError> {
    let defines = RefCell::new(HashMap::new());
    parse_tokens_resolved_recursive(file_path, file_path, &defines)
}
