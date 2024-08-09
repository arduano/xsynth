use super::Sf2ParseError;
use crate::resample::resample_vec;
use soundfont::data::{
    hydra::sample::{SampleHeader, SampleLink},
    sample_data::SampleData,
};
use std::{fs::File, sync::Arc};

#[derive(Clone, Debug)]
pub struct Sf2Sample {
    pub data: Arc<[f32]>,
    pub link_type: i8,
    pub loop_start: u32,
    pub loop_end: u32,
    pub sample_rate: u32,
    pub origpitch: u8,
    pub pitchadj: i8,
}

impl Sf2Sample {
    pub fn parse_sf2_samples(
        file: &mut File,
        headers: Vec<SampleHeader>,
        data: SampleData,
        sample_rate: u32,
    ) -> Result<Vec<Self>, Sf2ParseError> {
        let smpl = if let Some(data) = data.smpl {
            data.read_contents(file).map_err(|_| {
                Sf2ParseError::FailedToParseFile("Error reading sample contents".to_string())
            })?
        } else {
            return Err(Sf2ParseError::FailedToParseFile(
                "Soundfont does not contain samples".to_string(),
            ));
        };

        let mut samples = Vec::new();

        if let Some(sm24) = data.sm24 {
            // SF2 is 24-bit
            let extra = sm24.read_contents(file).map_err(|_| {
                Sf2ParseError::FailedToParseFile("Error reading extra sample contents".to_string())
            })?;

            let smpllen = smpl.len() / 2;
            let extralen = extra.len() - (smpllen % 2);
            if smpllen != extralen {
                return Err(Sf2ParseError::FailedToParseFile(
                    "Invalid sample length".to_string(),
                ));
            }

            for i in 0..extralen {
                let n0 = 0;
                let n1 = extra[i];
                let n2 = smpl[i * 2];
                let n3 = smpl[i * 2 + 1];
                let sample = i32::from_le_bytes([n0, n1, n2, n3]);
                let conv = sample as f32 / i32::MAX as f32;
                samples.push(conv);
            }
        } else {
            // SF2 is 16-bit
            for i in smpl.chunks(2) {
                let n0 = i[0];
                let n1 = i[1];
                let sample = i16::from_le_bytes([n0, n1]);
                let conv = sample as f32 / i16::MAX as f32;
                samples.push(conv);
            }
        }

        let mut out: Vec<Sf2Sample> = Vec::new();

        for h in headers {
            let start = h.start;
            let end = h.end;
            let sample: Vec<f32> = samples[start as usize..end as usize].into();

            let new = Sf2Sample {
                data: if h.sample_rate != sample_rate || !sample.is_empty() {
                    resample_vec(sample, h.sample_rate as f32, sample_rate as f32)
                } else {
                    sample.into()
                },
                link_type: match h.sample_type {
                    SampleLink::LeftSample => -1,
                    SampleLink::RightSample => 1,
                    _ => 0,
                },
                loop_start: h.loop_start - start,
                loop_end: h.loop_end - start,
                sample_rate: h.sample_rate,
                origpitch: h.origpitch,
                pitchadj: h.pitchadj,
            };
            out.push(new)
        }

        Ok(out)
    }
}
