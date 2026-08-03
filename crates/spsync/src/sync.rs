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

use common::{
    manifest::{Entry, MANIFEST_FILE, Manifest},
    path::sanitize_component,
};

use crate::{
    Client, Removed, SpsyncError, TrackRef,
    download::{Cover, TrackMeta},
    transcode,
};

const MAX_STEM: usize = 120;
const PARTIAL_EXTENSION: &str = "mp3.part";

fn sweep_partials(library_dir: &std::path::Path) -> usize {
    let Ok(entries) = fs::read_dir(library_dir) else {
        return 0;
    };

    let mut swept = 0;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "part") && fs::remove_file(&path).is_ok() {
            tracing::warn!(path = %path.display(), "removed partial download");
            swept += 1;
        }
    }

    swept
}

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

fn file_name(meta: &TrackMeta, id: &str, taken: &HashSet<PathBuf>) -> PathBuf {
    let artist = meta
        .artists
        .first()
        .map_or("Unknown Artist", String::as_str);

    let stem = sanitize_component(&format!("{artist} - {}", meta.title), MAX_STEM, "untitled");

    let candidate = PathBuf::from(format!("{stem}.mp3"));
    if taken.contains(&candidate) {
        return PathBuf::from(format!("{stem} [{id}].mp3"));
    }

    candidate
}

fn partition_restores(
    restore: &[TrackRef],
    manifest: &Manifest,
    library_dir: &std::path::Path,
) -> (Vec<String>, Vec<TrackRef>) {
    let mut restorable = Vec::new();
    let mut redownload = Vec::new();

    for track in restore {
        match manifest.entries.get(&track.id) {
            Some(entry) if library_dir.join(&entry.path).is_file() => {
                restorable.push(track.id.clone());
            }
            _ => {
                tracing::info!(uri = %track.uri, "preserved file is gone, re-downloading");
                redownload.push(track.clone());
            }
        }
    }

    (restorable, redownload)
}

fn apply_restores(manifest: &mut Manifest, ids: &[String]) -> usize {
    let mut restored = 0;

    for id in ids {
        if let Some(entry) = manifest.entries.get_mut(id) {
            entry.liked = true;
            restored += 1;
            tracing::info!(id = %id, path = %entry.path.display(), "restored from existing file");
        }
    }

    restored
}

fn apply_removals(
    manifest: &mut Manifest,
    removed: &[Removed],
    library_dir: &std::path::Path,
    preserve: bool,
) -> usize {
    for entry in removed {
        if preserve {
            if let Some(existing) = manifest.entries.get_mut(&entry.id) {
                existing.liked = false;
            }
            tracing::info!(id = %entry.id, "unliked, keeping local file");
        } else {
            let path = library_dir.join(&entry.entry.path);
            if let Err(e) = fs::remove_file(&path) {
                tracing::warn!(path = %path.display(), error = %e, "could not remove file");
            }
            manifest.entries.remove(&entry.id);
        }
    }

    removed.len()
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
        let source_format = format!("{:?}", audio.format);
        let (meta, cover, ogg) = (audio.meta, audio.cover, audio.ogg);

        let mp3 = tokio::task::spawn_blocking(move || transcode::ogg_to_mp3(ogg))
            .await
            .map_err(|_| SpsyncError::DownloadAborted)??;

        let relative = file_name(&meta, &track.id, taken);
        let absolute = self.config().library_dir.join(&relative);
        let partial = absolute.with_extension(PARTIAL_EXTENSION);

        fs::write(&partial, &mp3)?;
        write_tags(&partial, &meta, cover.as_ref())?;
        fs::rename(&partial, &absolute)?;

        Ok((
            Entry {
                uri: track.uri.clone(),
                path: relative,
                added_at: track.added_at,
                liked: true,
                artist: meta.artists.first().cloned().unwrap_or_default(),
                album: meta.album.clone(),
                source_format,
                encoder: transcode::ENCODER.to_owned(),
            },
            Duration::from_millis(u64::from(meta.duration_ms)),
        ))
    }

    /// # Errors
    ///
    /// Returns [`SpsyncError::NotAuthenticated`] if no credentials are cached. Per-track
    /// failures are collected into the report rather than aborting the run.
    pub async fn sync_tracks(&self, tracks: &[TrackRef]) -> Result<SyncReport, SpsyncError> {
        sweep_partials(&self.config().library_dir);

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
    /// [`SpsyncError::Manifest`] if the manifest on disk is malformed.
    pub async fn sync_library(&self) -> Result<SyncReport, SpsyncError> {
        let diff = self.sync_diff().await?;
        let library_dir = &self.config().library_dir;

        let (restorable, redownload) =
            partition_restores(&diff.restore, &self.manifest()?, library_dir);

        let mut queue = diff.add.clone();
        queue.extend(redownload);

        let mut report = self.sync_tracks(&queue).await?;

        if restorable.is_empty() && diff.remove.is_empty() {
            return Ok(report);
        }

        let mut manifest = self.manifest()?;
        report.restored = apply_restores(&mut manifest, &restorable);
        report.removed = apply_removals(
            &mut manifest,
            &diff.remove,
            library_dir,
            self.config().preserve,
        );
        manifest.save(&library_dir.join(MANIFEST_FILE))?;

        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::{collections::HashSet, fs, path::PathBuf};

    use tempfile::TempDir;

    use super::{
        Entry, Manifest, Removed, TrackMeta, TrackRef, apply_removals, apply_restores, file_name,
        partition_restores, sweep_partials,
    };

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

    fn entry(id: &str, liked: bool) -> Entry {
        Entry {
            uri: format!("spotify:track:{id}"),
            path: PathBuf::from(format!("{id}.mp3")),
            added_at: Some(1),
            liked,
            artist: "artist".to_owned(),
            album: "album".to_owned(),
            source_format: "OGG_VORBIS_320".to_owned(),
            encoder: "lame-vbr-v0".to_owned(),
        }
    }

    fn library(ids: &[(&str, bool)], write_files: bool) -> (TempDir, Manifest) {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut manifest = Manifest::default();

        for (id, liked) in ids {
            let entry = entry(id, *liked);
            if write_files {
                fs::write(dir.path().join(&entry.path), b"mp3").expect("write");
            }
            manifest.entries.insert((*id).to_owned(), entry);
        }

        (dir, manifest)
    }

    fn track(id: &str) -> TrackRef {
        TrackRef {
            id: id.to_owned(),
            uri: format!("spotify:track:{id}"),
            added_at: Some(1),
        }
    }

    #[test]
    fn restores_when_preserved_file_is_present() {
        let (dir, manifest) = library(&[("a", false)], true);
        let (restorable, redownload) = partition_restores(&[track("a")], &manifest, dir.path());

        assert_eq!(restorable, vec!["a".to_owned()]);
        assert!(redownload.is_empty());
    }

    #[test]
    fn redownloads_when_preserved_file_was_deleted() {
        let (dir, manifest) = library(&[("a", false)], false);
        let (restorable, redownload) = partition_restores(&[track("a")], &manifest, dir.path());

        assert!(restorable.is_empty());
        assert_eq!(redownload, vec![track("a")]);
    }

    #[test]
    fn apply_restores_marks_liked() {
        let (_dir, mut manifest) = library(&[("a", false)], true);
        assert_eq!(apply_restores(&mut manifest, &["a".to_owned()]), 1);

        assert!(manifest.entries["a"].liked);
    }

    #[test]
    fn preserve_keeps_file_and_marks_unliked() {
        let (dir, mut manifest) = library(&[("a", true)], true);
        let removed = vec![Removed {
            id: "a".to_owned(),
            entry: entry("a", true),
        }];

        assert_eq!(apply_removals(&mut manifest, &removed, dir.path(), true), 1);

        assert!(dir.path().join("a.mp3").is_file());
        assert!(!manifest.entries["a"].liked);
    }

    #[test]
    fn sweeps_partial_downloads_and_keeps_finished_ones() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("a.mp3.part"), b"partial").expect("write");
        fs::write(dir.path().join("b.mp3.part"), b"partial").expect("write");
        fs::write(dir.path().join("c.mp3"), b"done").expect("write");

        assert_eq!(sweep_partials(dir.path()), 2);

        assert!(!dir.path().join("a.mp3.part").exists());
        assert!(!dir.path().join("b.mp3.part").exists());
        assert!(dir.path().join("c.mp3").is_file());
    }

    #[test]
    fn sweep_tolerates_missing_directory() {
        let dir = tempfile::tempdir().expect("tempdir");

        assert_eq!(sweep_partials(&dir.path().join("nope")), 0);
    }

    #[test]
    fn partial_extension_appends_rather_than_replaces() {
        let target = PathBuf::from("/lib/Artist - Song 1.5.mp3");

        assert_eq!(
            target.with_extension(super::PARTIAL_EXTENSION),
            PathBuf::from("/lib/Artist - Song 1.5.mp3.part")
        );
    }

    #[test]
    fn without_preserve_deletes_file_and_entry() {
        let (dir, mut manifest) = library(&[("a", true)], true);
        let removed = vec![Removed {
            id: "a".to_owned(),
            entry: entry("a", true),
        }];

        assert_eq!(
            apply_removals(&mut manifest, &removed, dir.path(), false),
            1
        );

        assert!(!dir.path().join("a.mp3").exists());
        assert!(!manifest.entries.contains_key("a"));
    }
}
