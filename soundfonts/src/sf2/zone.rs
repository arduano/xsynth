use crate::LoopMode;
use soundfont::{data::hydra::generator::GeneratorType, Zone};
use std::ops::RangeInclusive;

#[derive(Default, Clone, Debug)]
pub struct Sf2Zone {
    pub index: Option<u16>,
    pub offset: Option<i16>,
    pub loop_start_offset: Option<i16>,
    pub loop_end_offset: Option<i16>,
    pub loop_mode: Option<LoopMode>,
    pub cutoff: Option<i16>,
    pub resonance: Option<i16>,
    pub pan: Option<i16>,
    pub env_delay: Option<f32>,
    pub env_attack: Option<f32>,
    pub env_hold: Option<f32>,
    pub env_decay: Option<f32>,
    pub env_sustain: Option<f32>,
    pub env_release: Option<f32>,
    pub velrange: Option<RangeInclusive<u8>>,
    pub keyrange: Option<RangeInclusive<u8>>,
    pub attenuation: Option<i16>,
    pub fine_tune: Option<i16>,
    pub coarse_tune: Option<i16>,
    pub root_override: Option<i16>,
}

impl Sf2Zone {
    pub fn parse(zones: Vec<Zone>) -> Vec<Self> {
        let mut regions: Vec<Sf2Zone> = Vec::new();
        let mut global_region = Sf2Zone::default();

        for (i, zone) in zones.iter().enumerate() {
            let mut region = global_region.clone();

            for gen in &zone.gen_list {
                match gen.ty {
                    GeneratorType::StartAddrsOffset => region.offset = gen.amount.as_i16().copied(),
                    GeneratorType::StartloopAddrsOffset => {
                        region.loop_start_offset = gen.amount.as_i16().copied()
                    }
                    GeneratorType::EndloopAddrsOffset => {
                        region.loop_end_offset = gen.amount.as_i16().copied()
                    }
                    GeneratorType::InitialFilterFc => region.cutoff = gen.amount.as_i16().copied(),
                    GeneratorType::InitialFilterQ => {
                        region.resonance = gen.amount.as_i16().copied()
                    }
                    GeneratorType::Pan => region.pan = gen.amount.as_i16().copied(),
                    GeneratorType::DelayVolEnv => {
                        region.env_delay =
                            gen.amount.as_i16().map(|v| 2f32.powf(*v as f32 / 1200.0))
                    }
                    GeneratorType::AttackVolEnv => {
                        region.env_attack =
                            gen.amount.as_i16().map(|v| 2f32.powf(*v as f32 / 1200.0))
                    }
                    GeneratorType::HoldVolEnv => {
                        region.env_hold = gen.amount.as_i16().map(|v| 2f32.powf(*v as f32 / 1200.0))
                    }
                    GeneratorType::DecayVolEnv => {
                        region.env_decay =
                            gen.amount.as_i16().map(|v| 2f32.powf(*v as f32 / 1200.0))
                    }
                    GeneratorType::SustainVolEnv => {
                        region.env_sustain = gen
                            .amount
                            .as_i16()
                            .map(|v| 10f32.powf(-1.0 * *v as f32 / 200.0) * 100.0)
                    }
                    GeneratorType::ReleaseVolEnv => {
                        region.env_release =
                            gen.amount.as_i16().map(|v| 2f32.powf(*v as f32 / 1200.0))
                    }
                    GeneratorType::KeyRange => {
                        let range = gen.amount.as_range().copied();
                        region.keyrange = range.map(|v| v.low..=v.high)
                    }
                    GeneratorType::VelRange => {
                        let range = gen.amount.as_range().copied();
                        region.velrange = range.map(|v| v.low..=v.high)
                    }
                    GeneratorType::InitialAttenuation => {
                        region.attenuation = gen.amount.as_i16().copied()
                    }
                    GeneratorType::CoarseTune => region.coarse_tune = gen.amount.as_i16().copied(),
                    GeneratorType::FineTune => region.fine_tune = gen.amount.as_i16().copied(),
                    GeneratorType::SampleID => region.index = gen.amount.as_u16().copied(),
                    GeneratorType::Instrument => region.index = gen.amount.as_u16().copied(),
                    GeneratorType::SampleModes => {
                        region.loop_mode = gen.amount.as_i16().map(|v| match v {
                            1 => LoopMode::LoopContinuous,
                            3 => LoopMode::LoopSustain,
                            _ => LoopMode::NoLoop,
                        })
                    }
                    GeneratorType::OverridingRootKey => {
                        region.root_override = gen.amount.as_i16().copied()
                    }
                    _ => {}
                }
            }

            if i == 0 && region.index.is_none() {
                global_region = region;
            } else {
                regions.push(region);
            }
        }

        regions
    }
}
