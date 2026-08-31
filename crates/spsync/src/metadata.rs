use std::{collections::HashMap, time::Duration};

use librespot_core::Session;
use librespot_protocol::{
    extended_metadata::{BatchedEntityRequest, EntityRequest, ExtensionQuery},
    extension_kind::ExtensionKind,
    metadata::Track,
};
use protobuf::{EnumOrUnknown, Message};

use crate::track::TrackRef;

const BATCH_SIZE: usize = 200;

pub(crate) struct TrackInfo {
    pub length: Duration,
    pub title: String,
    pub artist: String,
}

fn request(chunk: &[TrackRef]) -> BatchedEntityRequest {
    BatchedEntityRequest {
        entity_request: chunk
            .iter()
            .map(|track| EntityRequest {
                entity_uri: track.uri.clone(),
                query: vec![ExtensionQuery {
                    extension_kind: EnumOrUnknown::new(ExtensionKind::TRACK_V4),
                    ..Default::default()
                }],
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

fn info_from(track: &Track) -> TrackInfo {
    TrackInfo {
        length: u64::try_from(track.duration()).map_or(Duration::ZERO, Duration::from_millis),
        title: track.name().to_owned(),
        artist: track
            .artist
            .first()
            .map(|a| a.name().to_owned())
            .unwrap_or_default(),
    }
}

/// Track title, artist and length keyed by uri, for every track the batch resolved.
pub(crate) async fn fetch(session: &Session, tracks: &[TrackRef]) -> HashMap<String, TrackInfo> {
    let mut resolved = HashMap::with_capacity(tracks.len());

    for chunk in tracks.chunks(BATCH_SIZE) {
        let response = match session
            .spclient()
            .get_extended_metadata(request(chunk))
            .await
        {
            Ok(response) => response,
            Err(e) => {
                tracing::warn!(error = %e, tracks = chunk.len(), "batched metadata request failed");
                continue;
            }
        };

        for array in response.extended_metadata {
            for data in array.extension_data {
                let Some(any) = data.extension_data.into_option() else {
                    continue;
                };
                let Ok(track) = Track::parse_from_bytes(&any.value) else {
                    tracing::debug!(uri = %data.entity_uri, "could not parse batched track");
                    continue;
                };

                resolved.insert(data.entity_uri, info_from(&track));
            }
        }
    }

    resolved
}
