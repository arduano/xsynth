use super::{instrument::Sf2Instrument, sample::Sf2Sample, zone::Sf2Zone, Sf2Preset, Sf2Region};
use crate::{convert_sample_index, sfz::AmpegEnvelopeParams, LoopMode};
use soundfont::Preset;
use std::{ops::RangeInclusive, sync::Arc};

#[derive(Clone, Debug)]
pub struct Sf2ParsedPreset {
    pub bank: u16,
    pub preset: u16,
    pub zones: Vec<Sf2Zone>,
}

impl Sf2ParsedPreset {
    pub fn parse_presets(presets: Vec<Preset>) -> Vec<Sf2ParsedPreset> {
        let mut presets_parsed: Vec<Sf2ParsedPreset> = Vec::new();

        for preset in presets {
            let zones = Sf2Zone::parse(preset.zones);

            presets_parsed.push(Sf2ParsedPreset {
                preset: preset.header.preset,
                bank: preset.header.bank,
                zones,
            });
        }

        presets_parsed
    }

    pub fn merge_presets(
        sample_data: Vec<Sf2Sample>,
        instruments: Vec<Sf2Instrument>,
        presets: Vec<Sf2ParsedPreset>,
        sample_rate: u32,
    ) -> Vec<Sf2Preset> {
        let mut out: Vec<Sf2Preset> = Vec::new();

        for preset in presets {
            let mut new_preset = Sf2Preset {
                preset: preset.preset,
                bank: preset.bank,
                regions: Vec::new(),
            };

            let mut regions = Vec::new();

            for zone in preset.zones {
                if let Some(instrument_idx) = zone.index {
                    let instrument = &instruments[instrument_idx as usize];

                    for subzone in &instrument.regions {
                        if let Some(sample_idx) = subzone.index {
                            let sample = &sample_data[sample_idx as usize];

                            let new_region = Sf2Region {
                                sample: Arc::new([]),
                                sample_rate: sample.sample_rate,
                                velrange: combine_ranges(
                                    zone.velrange.clone().unwrap_or(0..=127),
                                    subzone.velrange.clone().unwrap_or(0..=127),
                                ),
                                keyrange: combine_ranges(
                                    zone.keyrange.clone().unwrap_or(0..=127),
                                    subzone.keyrange.clone().unwrap_or(0..=127),
                                ),
                                root_key: subzone.root_override.unwrap_or(sample.origpitch as i16)
                                    as u8,
                                volume: {
                                    let v = zone
                                        .attenuation
                                        .unwrap_or(subzone.attenuation.unwrap_or(0));
                                    10f32.powf(-0.4 * v as f32 / 200.0)
                                },
                                pan: zone.pan.unwrap_or(subzone.pan.unwrap_or(0)),
                                loop_mode: zone
                                    .loop_mode
                                    .unwrap_or(subzone.loop_mode.unwrap_or(LoopMode::NoLoop)),
                                loop_start: {
                                    let v = (sample.loop_start as i32
                                        + subzone.loop_start_offset.unwrap_or(0) as i32)
                                        as u32;
                                    convert_sample_index(v, sample.sample_rate, sample_rate)
                                },
                                loop_end: {
                                    let v = (sample.loop_end as i32
                                        + subzone.loop_end_offset.unwrap_or(0) as i32)
                                        as u32;
                                    convert_sample_index(v, sample.sample_rate, sample_rate)
                                },
                                offset: convert_sample_index(
                                    subzone.offset.unwrap_or(0) as u32,
                                    sample.sample_rate,
                                    sample_rate,
                                ),
                                cutoff: subzone.cutoff.map(|v| {
                                    2f32.powf(v as f32 / 1200.0)
                                        * 8.176
                                        * 2f32.powf(zone.cutoff.unwrap_or(0) as f32 / 1200.0)
                                }),
                                resonance: zone.resonance.unwrap_or(subzone.resonance.unwrap_or(0))
                                    as f32
                                    / 10.0,
                                fine_tune: zone.fine_tune.unwrap_or(subzone.fine_tune.unwrap_or(0))
                                    + sample.pitchadj as i16,
                                coarse_tune: zone
                                    .coarse_tune
                                    .unwrap_or(subzone.coarse_tune.unwrap_or(0)),
                                ampeg_envelope: AmpegEnvelopeParams {
                                    ampeg_start: 0.0,
                                    ampeg_delay: subzone.env_delay.unwrap_or(0.0)
                                        * zone.env_delay.unwrap_or(1.0),
                                    ampeg_attack: subzone.env_attack.unwrap_or(0.0)
                                        * zone.env_attack.unwrap_or(1.0),
                                    ampeg_hold: subzone.env_hold.unwrap_or(0.0)
                                        * zone.env_hold.unwrap_or(1.0),
                                    ampeg_decay: subzone.env_decay.unwrap_or(0.0)
                                        * zone.env_decay.unwrap_or(1.0),
                                    ampeg_sustain: zone
                                        .env_sustain
                                        .unwrap_or(subzone.env_sustain.unwrap_or(100.0)),
                                    ampeg_release: subzone.env_release.unwrap_or(0.0)
                                        * zone.env_release.unwrap_or(1.0),
                                },
                            };

                            regions.push((new_region, sample.clone()));
                        }
                    }
                }
            }

            // Second pass -> Stereo sample linking
            // This is kinda hacky, there has to be a better way to do this
            // I hate this code
            let mut ignored_idx = Vec::new();
            for (i, region) in regions.clone().into_iter().enumerate() {
                if ignored_idx.contains(&i) {
                    continue;
                }
                if region.1.link_type.abs() == 1 {
                    if region.0.pan.abs() == 500 {
                        match regions
                            .clone()
                            .into_iter()
                            .position(|v: (Sf2Region, Sf2Sample)| {
                                let v1 = v.0.clone();
                                let v2 = region.0.clone();
                                v.1.link_type == -region.1.link_type
                                    && v1.pan == -v2.pan
                                    && v1.root_key == v2.root_key
                                    && v1.keyrange == v2.keyrange
                                    && v1.velrange == v2.velrange
                            }) {
                            Some(reg) => {
                                let sample_match = regions[reg].1.clone();
                                let mut new_region = region.0.clone();
                                match region.1.link_type {
                                    -1 => {
                                        new_region.sample = Arc::new([
                                            region.1.data.clone(),
                                            sample_match.data.clone(),
                                        ])
                                    }
                                    1 => {
                                        new_region.sample = Arc::new([
                                            sample_match.data.clone(),
                                            region.1.data.clone(),
                                        ])
                                    }
                                    _ => {}
                                }
                                new_region.pan = 0;
                                new_preset.regions.push(new_region);
                                ignored_idx.push(reg);
                            }
                            None => {
                                let mut new_region = region.0.clone();
                                new_region.sample = Arc::new([region.1.data.clone()]);
                                new_preset.regions.push(new_region);
                            }
                        }
                    }
                } else {
                    let mut new_region = region.0.clone();
                    new_region.sample = Arc::new([region.1.data.clone()]);
                    new_preset.regions.push(new_region);
                }
            }
            out.push(new_preset);
        }

        out
    }
}

fn combine_ranges<T: Ord + Copy>(
    r1: RangeInclusive<T>,
    r2: RangeInclusive<T>,
) -> RangeInclusive<T> {
    let start1 = r1.start();
    let start2 = r2.start();
    let end1 = r1.end();
    let end2 = r2.end();

    (*start1.max(start2))..=(*end1.min(end2))
}
