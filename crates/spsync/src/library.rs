use std::collections::HashSet;

use librespot_core::Session;

use crate::{error::SpsyncError, track::TrackRef};

const ADDED_AT: &str = "added_at";

fn collection_uri(username: &str) -> String {
    format!("spotify:user:{username}:collection")
}

fn page_url_to_uri(page_url: &str) -> String {
    page_url
        .strip_prefix("hm://")
        .unwrap_or(page_url)
        .split('/')
        .skip_while(|s| *s != "spotify")
        .take(3)
        .collect::<Vec<&str>>()
        .join(":")
}

pub(crate) async fn list_liked(session: &Session) -> Result<Vec<TrackRef>, SpsyncError> {
    let mut pending = vec![collection_uri(&session.username())];
    let mut requested: HashSet<String> = HashSet::new();
    let mut tracks = Vec::new();

    while let Some(uri) = pending.pop() {
        if !requested.insert(uri.clone()) {
            continue;
        }

        let context = session.spclient().get_context(&uri).await?;

        for page in context.pages {
            if page.tracks.is_empty()
                && let Some(url) = page.page_url.filter(|u| !u.is_empty())
            {
                pending.push(page_url_to_uri(&url));
                continue;
            }

            if let Some(url) = page.next_page_url.filter(|u| !u.is_empty()) {
                tracing::warn!(
                    next_page_url = %url,
                    "context returned an unhandled next page; the track list may be incomplete"
                );
            }

            for track in &page.tracks {
                let Some(uri) = track.uri.as_deref() else {
                    continue;
                };
                let added_at = track.metadata.get(ADDED_AT).and_then(|v| v.parse().ok());

                if let Some(track) = TrackRef::from_uri(uri, added_at) {
                    tracks.push(track);
                } else {
                    tracing::debug!(uri, "skipping non-track entry");
                }
            }
        }
    }

    Ok(tracks)
}

#[cfg(test)]
mod tests {
    use super::page_url_to_uri;

    #[test]
    fn extracts_uri_from_page_url() {
        assert_eq!(
            page_url_to_uri("hm://artistplaycontext/v1/page/spotify/album/5LFz/km_artist"),
            "spotify:album:5LFz"
        );
    }

    #[test]
    fn tolerates_missing_scheme() {
        assert_eq!(
            page_url_to_uri("some/path/spotify/playlist/abc"),
            "spotify:playlist:abc"
        );
    }
}
