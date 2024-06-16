use std::{fs::File, io, path::PathBuf, sync::Arc};

use symphonia::core::formats::FormatOptions;
use symphonia::core::{audio::AudioBuffer, conv::IntoSample, probe::Hint, sample::Sample};
use symphonia::core::{audio::AudioBufferRef, meta::MetadataOptions};
use symphonia::core::{audio::Signal, io::MediaSourceStream};
use symphonia::core::{codecs::DecoderOptions, errors::Error};

use crate::{AudioStreamParams, ChannelCount};
use soundfonts::resample::resample_vecs;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AudioLoadError {
    #[error("IO Error")]
    IOError(#[from] io::Error),

    #[error("Audio decoding failed for {0}")]
    AudioDecodingFailed(PathBuf, Error),

    #[error("Audio file {0} has an invalid channel count")]
    InvalidChannelCount(PathBuf),

    #[error("Audio file {0} has no tracks")]
    NoTracks(PathBuf),
}

type ProcessedSample = (Arc<[Arc<[f32]>]>, u32);

pub fn load_audio_file(
    path: &PathBuf,
    stream_params: AudioStreamParams,
) -> Result<ProcessedSample, AudioLoadError> {
    let new_sample_rate = stream_params.sample_rate as f32;

    let extension = path.extension().and_then(|ext| ext.to_str());

    let file = Box::new(File::open(path)?);

    // Create the media source stream using the boxed media source from above.
    let mss = MediaSourceStream::new(file, Default::default());

    // Create a hint to help the format registry guess what format reader is appropriate.
    let mut hint = Hint::new();
    if let Some(extension) = extension {
        hint.with_extension(extension);
    }

    // Use the default options when reading and decoding.
    let format_opts: FormatOptions = Default::default();
    let metadata_opts: MetadataOptions = Default::default();
    let decoder_opts: DecoderOptions = Default::default();

    // Probe the media source stream for a format.
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .map_err(|x| AudioLoadError::AudioDecodingFailed(path.clone(), x))?;

    // Get the format reader yielded by the probe operation.
    let mut format = probed.format;

    // Get the default track.
    let track = format
        .default_track()
        .ok_or_else(|| AudioLoadError::NoTracks(path.clone()))?;

    let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
    let channel_count = track.codec_params.channels.map(|c| c.count()).unwrap_or(1);

    // Create a decoder for the track.
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &decoder_opts)
        .map_err(|x| AudioLoadError::AudioDecodingFailed(path.clone(), x))?;

    // Store the track identifier, we'll use it to filter packets.
    let track_id = track.id;

    // Builder for the parsed audio buffers
    let mut builder = BuilderVecs::new(channel_count);

    loop {
        // Get the next packet from the format reader.
        let packet = match format.next_packet() {
            Err(symphonia::core::errors::Error::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                // Audio source ended. Currently the lib has no cleaner way of detecting this.
                break;
            }
            Err(error) => return Err(AudioLoadError::AudioDecodingFailed(path.clone(), error)),
            Ok(packet) => packet,
        };

        // If the packet does not belong to the selected track, skip it.
        if packet.track_id() != track_id {
            continue;
        }

        // Decode the packet into audio samples, ignoring any decode errors.
        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                builder.push(audio_buf);
            }

            Err(Error::DecodeError(_)) => (),
            Err(e) => return Err(AudioLoadError::AudioDecodingFailed(path.clone(), e)),
        }
    }

    let built = builder.finish(sample_rate as f32, new_sample_rate, stream_params.channels);

    Ok((built, sample_rate))
}

struct BuilderVecs {
    vecs: Vec<Vec<f32>>,
}

impl BuilderVecs {
    fn new(channels: usize) -> Self {
        let mut vecs = Vec::new();
        for _ in 0..channels {
            vecs.push(Vec::new());
        }

        Self { vecs }
    }

    fn push(&mut self, buffer: AudioBufferRef) {
        match buffer {
            AudioBufferRef::U8(buf) => self.push_buffer(&buf),
            AudioBufferRef::U16(buf) => self.push_buffer(&buf),
            AudioBufferRef::U24(buf) => self.push_buffer(&buf),
            AudioBufferRef::U32(buf) => self.push_buffer(&buf),
            AudioBufferRef::S8(buf) => self.push_buffer(&buf),
            AudioBufferRef::S16(buf) => self.push_buffer(&buf),
            AudioBufferRef::S24(buf) => self.push_buffer(&buf),
            AudioBufferRef::S32(buf) => self.push_buffer(&buf),
            AudioBufferRef::F32(buf) => self.push_buffer(&buf),
            AudioBufferRef::F64(buf) => self.push_buffer(&buf),
        }
    }

    fn push_buffer(&mut self, buffer: &AudioBuffer<impl Sample + IntoSample<f32>>) {
        let channels = buffer.spec().channels.count();

        for c in 0..channels {
            let channel = buffer.chan(c);
            self.vecs[c].reserve(channel.len());
            for &sample in channel.iter() {
                self.vecs[c].push(sample.into_sample());
            }
        }
    }

    fn finish(
        self,
        sample_rate: f32,
        new_sample_rate: f32,
        channels: ChannelCount,
    ) -> Arc<[Arc<[f32]>]> {
        let mut vecs = self.vecs;

        if channels == ChannelCount::Mono && vecs.len() >= 2 {
            let right = vecs.pop().unwrap_or_default();
            let left = vecs.pop().unwrap_or_default();

            let combined: Vec<f32> = left
                .iter()
                .zip(right.iter())
                .map(|(&l, &r)| (l + r) * 0.5)
                .collect();
            vecs.push(combined);
        }

        for chan in vecs.iter_mut() {
            chan.shrink_to_fit();
        }

        resample_vecs(vecs, sample_rate, new_sample_rate)
    }
}
