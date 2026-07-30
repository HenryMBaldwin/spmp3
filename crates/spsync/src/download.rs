use std::io::{Read, Seek, SeekFrom};

use librespot_audio::{AudioDecrypt, AudioFile};
use librespot_core::{FileId, Session, SpotifyId, SpotifyUri};
use librespot_metadata::audio::{AudioFileFormat, AudioFiles, AudioItem, UniqueFields};

use crate::error::SpsyncError;

const SPOTIFY_OGG_HEADER_END: u64 = 0xa7;

const PREFERRED_FORMATS: &[AudioFileFormat] = &[
    AudioFileFormat::OGG_VORBIS_320,
    AudioFileFormat::OGG_VORBIS_160,
    AudioFileFormat::OGG_VORBIS_96,
];

#[derive(Debug, Clone)]
pub struct TrackMeta {
    pub title: String,
    pub album: String,
    pub artists: Vec<String>,
    pub number: Option<u32>,
    pub disc_number: Option<u32>,
    pub duration_ms: u32,
}

pub struct TrackAudio {
    pub ogg: Vec<u8>,
    pub meta: TrackMeta,
    pub format: AudioFileFormat,
}

impl std::fmt::Debug for TrackAudio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrackAudio")
            .field("bytes", &self.ogg.len())
            .field("meta", &self.meta)
            .field("format", &self.format)
            .finish()
    }
}

fn stream_data_rate(format: AudioFileFormat) -> usize {
    let kbps = match format {
        AudioFileFormat::OGG_VORBIS_96 => 12,
        AudioFileFormat::OGG_VORBIS_160 => 20,
        _ => 40,
    };

    kbps * 1024
}

fn pick_format(files: &AudioFiles) -> Option<(AudioFileFormat, FileId)> {
    PREFERRED_FORMATS
        .iter()
        .find_map(|format| files.get(format).map(|id| (*format, *id)))
}

fn track_id(uri: &SpotifyUri) -> Result<SpotifyId, SpsyncError> {
    match uri {
        SpotifyUri::Track { id } => Ok(*id),
        other => Err(SpsyncError::UnsupportedUri {
            uri: other.to_uri().unwrap_or_else(|_| "<invalid>".to_owned()),
        }),
    }
}

fn meta_from(item: &AudioItem) -> TrackMeta {
    let mut meta = TrackMeta {
        title: item.name.clone(),
        album: String::new(),
        artists: Vec::new(),
        number: None,
        disc_number: None,
        duration_ms: item.duration_ms,
    };

    match &item.unique_fields {
        UniqueFields::Track {
            album,
            artists,
            album_artists,
            number,
            disc_number,
            ..
        } => {
            meta.album.clone_from(album);
            meta.artists = artists.iter().map(|a| a.name.clone()).collect();
            if meta.artists.is_empty() {
                meta.artists.clone_from(album_artists);
            }
            meta.number = Some(*number);
            meta.disc_number = Some(*disc_number);
        }
        UniqueFields::Local {
            album,
            artists,
            number,
            disc_number,
            ..
        } => {
            meta.album = album.clone().unwrap_or_default();
            meta.artists = artists.clone().into_iter().collect();
            meta.number = *number;
            meta.disc_number = *disc_number;
        }
        UniqueFields::Episode { show_name, .. } => meta.album.clone_from(show_name),
    }

    meta
}

async fn resolve_playable(session: &Session, item: AudioItem) -> Result<AudioItem, SpsyncError> {
    if item.availability.is_err() {
        return Err(SpsyncError::TrackUnavailable { uri: item.uri });
    }

    if !item.files.is_empty() {
        return Ok(item);
    }

    let alternatives = item
        .alternatives
        .clone()
        .ok_or_else(|| SpsyncError::TrackUnavailable {
            uri: item.uri.clone(),
        })?;

    for alt in alternatives.iter() {
        if let Ok(alt) = AudioItem::get_file(session, alt.clone()).await
            && alt.availability.is_ok()
            && !alt.files.is_empty()
        {
            return Ok(alt);
        }
    }

    Err(SpsyncError::TrackUnavailable { uri: item.uri })
}

pub(crate) async fn download(
    session: &Session,
    uri: &SpotifyUri,
) -> Result<TrackAudio, SpsyncError> {
    let item = AudioItem::get_file(session, uri.clone()).await?;
    let meta = meta_from(&item);

    let item = resolve_playable(session, item).await?;
    let (format, file_id) =
        pick_format(&item.files).ok_or_else(|| SpsyncError::NoSupportedFormat {
            uri: item.uri.clone(),
        })?;

    let key = session
        .audio_key()
        .request(track_id(&item.track_id)?, file_id)
        .await?;

    let file = AudioFile::open(session, file_id, stream_data_rate(format)).await?;
    let controller = file.get_stream_loader_controller()?;
    controller.set_random_access_mode();

    let ogg = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, SpsyncError> {
        let len = controller.len();
        controller.fetch_next_and_wait(len, len)?;

        let mut decrypt = AudioDecrypt::new(Some(key), file);
        decrypt.seek(SeekFrom::Start(SPOTIFY_OGG_HEADER_END))?;

        let mut ogg = Vec::with_capacity(len);
        decrypt.read_to_end(&mut ogg)?;

        Ok(ogg)
    })
    .await
    .map_err(|_| SpsyncError::DownloadAborted)??;

    Ok(TrackAudio { ogg, meta, format })
}
