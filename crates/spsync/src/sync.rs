use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

use id3::{
    Tag, TagLike, Version,
    frame::{Picture, PictureType},
};

use crate::{
    Client, Entry, SpsyncError, TrackRef,
    download::{Cover, TrackMeta},
    manifest::MANIFEST_FILE,
    transcode,
};

const MAX_STEM: usize = 120;

#[derive(Debug, Default)]
pub struct SyncReport {
    pub added: usize,
    pub restored: usize,
    pub removed: usize,
    pub failed: Vec<Failure>,
}

#[derive(Debug)]
pub struct Failure {
    pub uri: String,
    pub error: String,
}

fn sanitize(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();

    cleaned.trim().trim_matches('.').to_owned()
}

fn truncate(value: &str, max: usize) -> String {
    value.char_indices().take(max).map(|(_, c)| c).collect()
}

fn file_name(meta: &TrackMeta, id: &str, taken: &HashSet<PathBuf>) -> PathBuf {
    let artist = meta
        .artists
        .first()
        .map_or("Unknown Artist", String::as_str);

    let stem = truncate(&sanitize(&format!("{artist} - {}", meta.title)), MAX_STEM);
    let stem = if stem.is_empty() { "untitled" } else { &stem };

    let candidate = PathBuf::from(format!("{stem}.mp3"));
    if taken.contains(&candidate) {
        return PathBuf::from(format!("{stem} [{id}].mp3"));
    }

    candidate
}

fn write_tags(
    path: &std::path::Path,
    meta: &TrackMeta,
    cover: Option<&Cover>,
) -> Result<(), SpsyncError> {
    let mut tag = Tag::new();
    tag.set_title(&meta.title);
    tag.set_album(&meta.album);

    if !meta.artists.is_empty() {
        tag.set_artist(meta.artists.join(", "));
    }
    if let Some(number) = meta.number {
        tag.set_track(number);
    }
    if let Some(disc) = meta.disc_number {
        tag.set_disc(disc);
    }
    if let Some(cover) = cover {
        tag.add_frame(Picture {
            mime_type: cover.mime.clone(),
            picture_type: PictureType::CoverFront,
            description: String::new(),
            data: cover.data.clone(),
        });
    }

    tag.write_to_path(path, Version::Id3v24)
        .map_err(|e| SpsyncError::Transcode(format!("id3: {e}")))
}

impl Client {
    async fn sync_one(
        &self,
        track: &TrackRef,
        taken: &HashSet<PathBuf>,
    ) -> Result<(Entry, Duration), SpsyncError> {
        let audio = self.download(track).await?;
        let (meta, cover, ogg) = (audio.meta, audio.cover, audio.ogg);

        let mp3 = tokio::task::spawn_blocking(move || transcode::ogg_to_mp3(ogg))
            .await
            .map_err(|_| SpsyncError::DownloadAborted)??;

        let relative = file_name(&meta, &track.id, taken);
        let absolute = self.config().library_dir.join(&relative);

        fs::write(&absolute, &mp3)?;
        write_tags(&absolute, &meta, cover.as_ref())?;

        Ok((
            Entry {
                uri: track.uri.clone(),
                path: relative,
                added_at: track.added_at,
                liked: true,
            },
            Duration::from_millis(u64::from(meta.duration_ms)),
        ))
    }

    /// # Errors
    ///
    /// Returns [`SpsyncError::NotAuthenticated`] if no credentials are cached. Per-track
    /// failures are collected into the report rather than aborting the run.
    pub async fn sync_tracks(&self, tracks: &[TrackRef]) -> Result<SyncReport, SpsyncError> {
        let mut manifest = self.manifest()?;
        let manifest_path = self.config().library_dir.join(MANIFEST_FILE);

        let mut report = SyncReport::default();
        let mut taken: HashSet<PathBuf> =
            manifest.entries.values().map(|e| e.path.clone()).collect();

        for (index, track) in tracks.iter().enumerate() {
            let started = Instant::now();

            match self.sync_one(track, &taken).await {
                Ok((entry, duration)) => {
                    tracing::info!(
                        uri = %track.uri,
                        path = %entry.path.display(),
                        progress = format!("{}/{}", index + 1, tracks.len()),
                        "downloaded"
                    );
                    taken.insert(entry.path.clone());
                    manifest.entries.insert(track.id.clone(), entry);
                    manifest.save(&manifest_path)?;
                    report.added += 1;

                    if self.config().realtime && index + 1 < tracks.len() {
                        let remaining = duration.saturating_sub(started.elapsed());
                        if !remaining.is_zero() {
                            tracing::debug!(secs = remaining.as_secs(), "pacing to realtime");
                            tokio::time::sleep(remaining).await;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(uri = %track.uri, error = %e, "track failed");
                    report.failed.push(Failure {
                        uri: track.uri.clone(),
                        error: e.to_string(),
                    });
                }
            }
        }

        Ok(report)
    }

    /// # Errors
    ///
    /// Returns [`SpsyncError::NotAuthenticated`] if no credentials are cached, or
    /// [`SpsyncError::Json`] if the manifest on disk is malformed.
    pub async fn sync_library(&self) -> Result<SyncReport, SpsyncError> {
        let diff = self.sync_diff().await?;

        let mut queue = diff.add.clone();
        let mut restorable = Vec::new();

        {
            let manifest = self.manifest()?;
            for track in &diff.restore {
                match manifest.entries.get(&track.id) {
                    Some(entry) if self.config().library_dir.join(&entry.path).is_file() => {
                        restorable.push(track.id.clone());
                    }
                    _ => {
                        tracing::info!(uri = %track.uri, "preserved file is gone, re-downloading");
                        queue.push(track.clone());
                    }
                }
            }
        }

        let mut report = self.sync_tracks(&queue).await?;

        if restorable.is_empty() && diff.remove.is_empty() {
            return Ok(report);
        }

        let mut manifest = self.manifest()?;

        for id in &restorable {
            if let Some(entry) = manifest.entries.get_mut(id) {
                entry.liked = true;
                report.restored += 1;
                tracing::info!(id = %id, path = %entry.path.display(), "restored from existing file");
            }
        }

        for removed in &diff.remove {
            if self.config().preserve {
                if let Some(entry) = manifest.entries.get_mut(&removed.id) {
                    entry.liked = false;
                }
                tracing::info!(id = %removed.id, "unliked, keeping local file");
            } else {
                let path = self.config().library_dir.join(&removed.entry.path);
                if let Err(e) = fs::remove_file(&path) {
                    tracing::warn!(path = %path.display(), error = %e, "could not remove file");
                }
                manifest.entries.remove(&removed.id);
            }

            report.removed += 1;
        }

        manifest.save(&self.config().library_dir.join(MANIFEST_FILE))?;

        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{TrackMeta, file_name, sanitize};

    fn meta(artist: &str, title: &str) -> TrackMeta {
        TrackMeta {
            title: title.to_owned(),
            album: String::new(),
            artists: vec![artist.to_owned()],
            number: None,
            disc_number: None,
            duration_ms: 0,
        }
    }

    #[test]
    fn strips_path_separators() {
        assert_eq!(sanitize("AC/DC"), "AC_DC");
        assert_eq!(sanitize("a:b*c?"), "a_b_c_");
    }

    #[test]
    fn builds_readable_name() {
        let taken = HashSet::new();
        assert_eq!(
            file_name(&meta("hey, nothing", "Maine"), "abc", &taken).to_str(),
            Some("hey, nothing - Maine.mp3")
        );
    }

    #[test]
    fn disambiguates_collisions_with_id() {
        let mut taken = HashSet::new();
        taken.insert("hey, nothing - Maine.mp3".into());

        assert_eq!(
            file_name(&meta("hey, nothing", "Maine"), "abc", &taken).to_str(),
            Some("hey, nothing - Maine [abc].mp3")
        );
    }
}
