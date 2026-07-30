use std::io::{Cursor, ErrorKind};

use mp3lame_encoder::{
    Builder, FlushNoGap, InterleavedPcm, Quality, VbrMode, max_required_buffer_size,
};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::DecoderOptions,
    errors::Error as SymphoniaError,
    formats::FormatOptions,
    io::{MediaSourceStream, MediaSourceStreamOptions},
    meta::MetadataOptions,
    probe::Hint,
};

use crate::error::SpsyncError;

const FLUSH_RESERVE: usize = 7200;

pub(crate) const ENCODER: &str = "lame-vbr-v0";

struct Pcm {
    samples: Vec<i16>,
    sample_rate: u32,
    channels: u8,
}

fn decode_ogg(ogg: Vec<u8>) -> Result<Pcm, SpsyncError> {
    let source = MediaSourceStream::new(
        Box::new(Cursor::new(ogg)),
        MediaSourceStreamOptions::default(),
    );

    let mut hint = Hint::new();
    hint.with_extension("ogg");

    let probed = symphonia::default::get_probe().format(
        &hint,
        source,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;

    let mut format = probed.format;
    let (track_id, codec_params) = {
        let track = format
            .default_track()
            .ok_or_else(|| SpsyncError::Transcode("ogg stream has no default track".to_owned()))?;

        (track.id, track.codec_params.clone())
    };

    let mut decoder =
        symphonia::default::get_codecs().make(&codec_params, &DecoderOptions::default())?;

    let mut samples: Vec<i16> = Vec::new();
    let mut buffer: Option<SampleBuffer<i16>> = None;
    let mut sample_rate = 0;
    let mut channels = 0;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(e)) if e.kind() == ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let frames = decoder.decode(&packet)?;
        let spec = *frames.spec();
        sample_rate = spec.rate;
        channels = u8::try_from(spec.channels.count())
            .map_err(|_| SpsyncError::Transcode("too many channels".to_owned()))?;

        if buffer.is_none() {
            let capacity = u64::try_from(frames.capacity())
                .map_err(|_| SpsyncError::Transcode("packet too large".to_owned()))?;
            buffer = Some(SampleBuffer::new(capacity, spec));
        }

        if let Some(buffer) = buffer.as_mut() {
            buffer.copy_interleaved_ref(frames);
            samples.extend_from_slice(buffer.samples());
        }
    }

    if samples.is_empty() {
        return Err(SpsyncError::Transcode(
            "ogg stream decoded to nothing".to_owned(),
        ));
    }

    Ok(Pcm {
        samples,
        sample_rate,
        channels,
    })
}

fn encode_mp3(pcm: &Pcm) -> Result<Vec<u8>, SpsyncError> {
    let mut builder =
        Builder::new().ok_or_else(|| SpsyncError::Transcode("could not create lame".to_owned()))?;

    let setup = |result: Result<(), mp3lame_encoder::BuildError>| {
        result.map_err(|e| SpsyncError::Transcode(format!("lame setup: {e}")))
    };

    setup(builder.set_sample_rate(pcm.sample_rate))?;
    setup(builder.set_num_channels(pcm.channels))?;
    setup(builder.set_vbr_mode(VbrMode::Mtrh))?;
    setup(builder.set_vbr_quality(Quality::Best))?;
    setup(builder.set_quality(Quality::Best))?;
    setup(builder.set_to_write_vbr_tag(true))?;

    let mut encoder = builder
        .build()
        .map_err(|e| SpsyncError::Transcode(format!("lame build: {e}")))?;

    let per_channel = pcm.samples.len() / usize::from(pcm.channels);
    let mut mp3 = Vec::with_capacity(max_required_buffer_size(per_channel));

    encoder
        .encode_to_vec(InterleavedPcm(&pcm.samples), &mut mp3)
        .map_err(|e| SpsyncError::Transcode(format!("lame encode: {e}")))?;

    mp3.reserve(FLUSH_RESERVE);
    encoder
        .flush_to_vec::<FlushNoGap>(&mut mp3)
        .map_err(|e| SpsyncError::Transcode(format!("lame flush: {e}")))?;

    Ok(mp3)
}

pub(crate) fn ogg_to_mp3(ogg: Vec<u8>) -> Result<Vec<u8>, SpsyncError> {
    encode_mp3(&decode_ogg(ogg)?)
}
