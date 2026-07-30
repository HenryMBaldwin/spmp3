const TRACK_URI_PREFIX: &str = "spotify:track:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackRef {
    pub id: String,
    pub uri: String,
    pub added_at: Option<i64>,
}

impl TrackRef {
    pub(crate) fn from_uri(uri: &str, added_at: Option<i64>) -> Option<Self> {
        let id = uri.strip_prefix(TRACK_URI_PREFIX)?;
        if id.is_empty() {
            return None;
        }

        Some(Self {
            id: id.to_owned(),
            uri: uri.to_owned(),
            added_at,
        })
    }
}
