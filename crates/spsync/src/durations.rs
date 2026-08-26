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

/// Track lengths keyed by uri, for every track the batched metadata call resolved.
pub(crate) async fn fetch(session: &Session, tracks: &[TrackRef]) -> HashMap<String, Duration> {
    let mut lengths = HashMap::with_capacity(tracks.len());

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

                if let Ok(ms) = u64::try_from(track.duration()) {
                    lengths.insert(data.entity_uri, Duration::from_millis(ms));
                }
            }
        }
    }

    lengths
}
