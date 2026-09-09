use std::{collections::BTreeMap, fs, io, path::Path};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const OVERRIDES_FILE: &str = "overrides.json";
pub const OVERRIDES_VERSION: u32 = 1;

const ALLOWED_HOSTS: &[&str] = &[
    "youtube.com",
    "www.youtube.com",
    "m.youtube.com",
    "music.youtube.com",
    "youtu.be",
];

#[derive(Debug, Error)]
pub enum OverrideError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("overrides json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("overrides version {found} is newer than the supported version {supported}")]
    Version { found: u32, supported: u32 },

    #[error("{0} is not an accepted youtube url")]
    BadUrl(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum Action {
    Ignore,
    Source { url: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Override {
    #[serde(flatten)]
    pub action: Action,
    /// Human-readable name, kept so the entry stays identifiable after it leaves the failure list.
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub title: String,
    pub at: i64,
}

impl Override {
    /// Falls back to splitting `label` on the first " - " when artist and title are unset.
    #[must_use]
    pub fn names(&self) -> (String, String) {
        if !self.artist.is_empty() || !self.title.is_empty() {
            return (self.artist.clone(), self.title.clone());
        }

        match self.label.split_once(" - ") {
            Some((artist, title)) => (artist.trim().to_owned(), title.trim().to_owned()),
            None => (String::new(), self.label.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Overrides {
    pub version: u32,
    #[serde(default)]
    pub entries: BTreeMap<String, Override>,
}

impl Default for Overrides {
    fn default() -> Self {
        Self {
            version: OVERRIDES_VERSION,
            entries: BTreeMap::new(),
        }
    }
}

/// Accepts only https urls on a known youtube host.
///
/// # Errors
///
/// Returns [`OverrideError::BadUrl`] if the url is not an https youtube link.
pub fn validate_url(url: &str) -> Result<(), OverrideError> {
    let bad = || OverrideError::BadUrl(url.to_owned());

    let rest = url.strip_prefix("https://").ok_or_else(bad)?;
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .ok_or_else(bad)?
        .to_ascii_lowercase();

    if !ALLOWED_HOSTS.contains(&host.as_str()) || url.len() > 2048 {
        return Err(bad());
    }

    Ok(())
}

impl Overrides {
    /// # Errors
    ///
    /// Returns [`OverrideError`] if the file exists but cannot be read or parsed, or if
    /// it was written by a newer version of this crate.
    pub fn load(path: &Path) -> Result<Self, OverrideError> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };

        let overrides: Self = serde_json::from_slice(&bytes)?;
        if overrides.version > OVERRIDES_VERSION {
            return Err(OverrideError::Version {
                found: overrides.version,
                supported: OVERRIDES_VERSION,
            });
        }

        Ok(overrides)
    }

    /// # Errors
    ///
    /// Returns [`OverrideError::Io`] if the file cannot be written or renamed.
    pub fn save(&self, path: &Path) -> Result<(), OverrideError> {
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(&tmp, path)?;

        Ok(())
    }

    #[must_use]
    pub fn is_ignored(&self, id: &str) -> bool {
        matches!(
            self.entries.get(id),
            Some(Override {
                action: Action::Ignore,
                ..
            })
        )
    }

    #[must_use]
    pub fn source(&self, id: &str) -> Option<&str> {
        match self.entries.get(id) {
            Some(Override {
                action: Action::Source { url },
                ..
            }) => Some(url),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{Action, Override, Overrides, validate_url};

    #[test]
    fn accepts_known_youtube_hosts() {
        for url in [
            "https://www.youtube.com/watch?v=abc",
            "https://youtu.be/abc",
            "https://music.youtube.com/watch?v=abc",
            "https://YouTube.com/watch?v=abc",
        ] {
            assert!(validate_url(url).is_ok(), "should accept {url}");
        }
    }

    #[test]
    fn rejects_anything_else() {
        for url in [
            "http://youtube.com/watch?v=abc",
            "https://evil.com/watch?v=abc",
            "https://youtube.com.evil.com/x",
            "--exec=rm -rf /",
            "file:///etc/passwd",
            "https://notyoutu.be/abc",
        ] {
            assert!(validate_url(url).is_err(), "should reject {url}");
        }
    }

    #[test]
    fn round_trips_and_answers_lookups() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("overrides.json");

        let mut overrides = Overrides::default();
        overrides.entries.insert(
            "gone".to_owned(),
            Override {
                action: Action::Ignore,
                label: "Artist - Gone".to_owned(),
                artist: "Artist".to_owned(),
                title: "Gone".to_owned(),
                at: 1,
            },
        );
        overrides.entries.insert(
            "elsewhere".to_owned(),
            Override {
                action: Action::Source {
                    url: "https://youtu.be/abc".to_owned(),
                },
                label: "Artist - Elsewhere".to_owned(),
                artist: "Artist".to_owned(),
                title: "Elsewhere".to_owned(),
                at: 2,
            },
        );
        overrides.save(&path).expect("save");

        let loaded = Overrides::load(&path).expect("load");
        assert!(loaded.is_ignored("gone"));
        assert!(!loaded.is_ignored("elsewhere"));
        assert_eq!(loaded.source("elsewhere"), Some("https://youtu.be/abc"));
        assert_eq!(loaded.source("gone"), None);
        assert!(!loaded.is_ignored("unknown"));
    }

    #[test]
    fn names_prefer_explicit_fields() {
        let o = Override {
            action: Action::Ignore,
            label: "ignored label".to_owned(),
            artist: "Hippo Campus".to_owned(),
            title: "Passenger".to_owned(),
            at: 0,
        };

        assert_eq!(
            o.names(),
            ("Hippo Campus".to_owned(), "Passenger".to_owned())
        );
    }

    #[test]
    fn names_fall_back_to_splitting_the_label() {
        let o = Override {
            action: Action::Ignore,
            label: "Hippo Campus - Passenger".to_owned(),
            artist: String::new(),
            title: String::new(),
            at: 0,
        };

        assert_eq!(
            o.names(),
            ("Hippo Campus".to_owned(), "Passenger".to_owned())
        );
    }

    #[test]
    fn names_treat_an_unsplittable_label_as_a_title() {
        let o = Override {
            action: Action::Ignore,
            label: "Passenger".to_owned(),
            artist: String::new(),
            title: String::new(),
            at: 0,
        };

        assert_eq!(o.names(), (String::new(), "Passenger".to_owned()));
    }

    #[test]
    fn missing_file_is_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let loaded = Overrides::load(&dir.path().join("nope.json")).expect("load");

        assert!(loaded.entries.is_empty());
    }
}
