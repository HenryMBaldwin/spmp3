use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

use id3::{
    Tag, TagLike, Version,
    frame::{ExtendedText, Picture, PictureType},
};

use common::{
    manifest::{Entry, MANIFEST_FILE, Manifest},
    overrides::Overrides,
    path::sanitize_component,
    tag::TRACK_ID_DESCRIPTION,
};

use crate::{
    Client, Removed, SpsyncError, TrackRef,
    download::{Cover, TrackMeta},
    metadata::{self, TrackInfo},
    transcode, youtube,
};

const MAX_STEM: usize = 120;
const SOURCED_FORMAT: &str = "youtube-m4a";

/// Drops tracks the overrides file marks ignored or sourced; sourced ones run unpaced elsewhere.
fn queue(tracks: &[TrackRef], overrides: &Overrides) -> Vec<TrackRef> {
    let mut ignored = 0;
    let mut sourced = 0;

    let kept: Vec<TrackRef> = tracks
        .iter()
        .filter(|track| {
            if overrides.is_ignored(&track.id) {
                ignored += 1;
                return false;
            }
            if overrides.source(&track.id).is_some() {
                sourced += 1;
                return false;
            }
            true
        })
        .cloned()
        .collect();

    if ignored > 0 || sourced > 0 {
        tracing::info!(
            ignored,
            sourced,
            "excluding overridden tracks from the spotify queue"
        );
    }

    kept
}

/// Sleeps out the remainder of the track's own length, so downloads look like listening.
async fn pace(track_duration: Duration, took: Duration, position: usize, total: usize) {
    let remaining = track_duration.saturating_sub(took);
    if remaining.is_zero() {
        return;
    }

    tracing::info!(
        waiting = %human_duration(remaining),
        next = %format!("{}/{total}", position + 1),
        "pacing to realtime"
    );
    tokio::time::sleep(remaining).await;
}

fn meta_from_info(info: Option<&TrackInfo>) -> TrackMeta {
    TrackMeta {
        title: info.map(|i| i.title.clone()).unwrap_or_default(),
        album: info.map(|i| i.album.clone()).unwrap_or_default(),
        artists: info
            .map(|i| i.artist.clone())
            .filter(|artist| !artist.is_empty())
            .into_iter()
            .collect(),
        number: None,
        disc_number: None,
        duration_ms: info
            .and_then(|i| u32::try_from(i.length.as_millis()).ok())
            .unwrap_or(0),
    }
}

struct Downloaded {
    entry: Entry,
    track_duration: Duration,
    cover_error: Option<String>,
    bytes: usize,
}

fn human_duration(duration: Duration) -> String {
    let secs = duration.as_secs();

    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else if secs < 86400 {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}d{}h", secs / 86400, (secs % 86400) / 3600)
    }
}

fn human_bytes(bytes: u64) -> String {
    const MB: u64 = 1_048_576;
    const GB: u64 = 1_073_741_824;

    if bytes >= GB {
        format!("{}.{}GB", bytes / GB, (bytes % GB) * 10 / GB)
    } else if bytes >= MB {
        format!("{}.{}MB", bytes / MB, (bytes % MB) * 10 / MB)
    } else {
        format!("{}KB", bytes / 1024)
    }
}

fn library_bytes(library_dir: &std::path::Path) -> u64 {
    let Ok(entries) = fs::read_dir(library_dir) else {
        return 0;
    };

    entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|e| e == "mp3"))
        .filter_map(|entry| entry.metadata().ok())
        .map(|meta| meta.len())
        .sum()
}

fn projected_bytes(downloaded: u64, done: usize, total: usize) -> u64 {
    let done = u64::try_from(done).unwrap_or(1).max(1);
    let total = u64::try_from(total).unwrap_or(0);

    downloaded / done * total
}

fn mean(total: Duration, count: usize) -> Duration {
    u32::try_from(count)
        .ok()
        .filter(|count| *count > 0)
        .map_or(Duration::ZERO, |count| total / count)
}

/// Length of `tracks[i..]` at each index, charging tracks of unknown length the mean.
fn suffix_lengths(tracks: &[TrackRef], resolved: &HashMap<String, TrackInfo>) -> Vec<Duration> {
    let (known, count) = tracks
        .iter()
        .filter_map(|track| resolved.get(&track.uri))
        .fold((Duration::ZERO, 0), |(sum, count), info| {
            (sum + info.length, count + 1)
        });
    let fallback = mean(known, count);

    let mut suffix = vec![Duration::ZERO; tracks.len() + 1];
    for (index, track) in tracks.iter().enumerate().rev() {
        let length = resolved
            .get(&track.uri)
            .map_or(fallback, |info| info.length);
        suffix[index] = suffix[index + 1] + length;
    }

    suffix
}

fn remaining_estimate(
    realtime: bool,
    elapsed: Duration,
    work_time: Duration,
    track_time: Duration,
    remaining_length: Option<Duration>,
    done: usize,
    remaining: usize,
) -> Duration {
    let left = u32::try_from(remaining).unwrap_or(0);
    let per_track = |total: Duration| mean(total, done) * left;

    if !realtime {
        return per_track(elapsed);
    }

    match remaining_length {
        Some(length) => length + per_track(work_time),
        None => per_track(track_time),
    }
}
const PARTIAL_EXTENSION: &str = "mp3.part";

fn sweep_partials(library_dir: &std::path::Path) -> Result<usize, SpsyncError> {
    let mut swept = 0;

    for entry in fs::read_dir(library_dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "part") {
            fs::remove_file(&path)?;
            tracing::warn!(path = %path.display(), "removed partial download");
            swept += 1;
        }
    }

    Ok(swept)
}

#[derive(Debug, Default)]
pub struct SyncReport {
    pub added: usize,
    pub restored: usize,
    pub removed: usize,
    pub missing_covers: usize,
    pub failed: Vec<Failure>,
}

#[derive(Debug, Default)]
pub struct SourcedReport {
    pub tracks: usize,
    pub downloaded: usize,
    pub failures: Vec<Failure>,
}

#[derive(Debug)]
pub struct Failure {
    pub uri: String,
    pub artist: String,
    pub title: String,
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

#[derive(Debug, Default)]
pub struct Removals {
    pub removed: usize,
    pub failed: Vec<Failure>,
}

fn apply_removals(
    manifest: &mut Manifest,
    removed: &[Removed],
    library_dir: &std::path::Path,
    preserve: bool,
) -> Removals {
    let mut result = Removals::default();

    for entry in removed {
        if preserve {
            if let Some(existing) = manifest.entries.get_mut(&entry.id) {
                existing.liked = false;
            }
            tracing::info!(id = %entry.id, "unliked, keeping local file");
            result.removed += 1;
            continue;
        }

        let path = library_dir.join(&entry.entry.path);
        match fs::remove_file(&path) {
            Ok(()) => {
                manifest.entries.remove(&entry.id);
                result.removed += 1;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                manifest.entries.remove(&entry.id);
                result.removed += 1;
            }
            Err(e) => result.failed.push(Failure {
                uri: entry.entry.uri.clone(),
                artist: entry.entry.artist.clone(),
                title: String::new(),
                error: format!("could not remove {}: {e}", path.display()),
            }),
        }
    }

    result
}

fn write_tags(
    path: &std::path::Path,
    id: &str,
    meta: &TrackMeta,
    cover: Option<&Cover>,
) -> Result<(), SpsyncError> {
    let mut tag = Tag::new();
    tag.add_frame(ExtendedText {
        description: TRACK_ID_DESCRIPTION.to_owned(),
        value: id.to_owned(),
    });
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
        source: Option<&str>,
        info: Option<&TrackInfo>,
    ) -> Result<Downloaded, SpsyncError> {
        let (mp3, meta, cover, cover_error, source_format) = if let Some(url) = source {
            let m4a = youtube::download(url).await?;
            let mp3 = tokio::task::spawn_blocking(move || transcode::m4a_to_mp3(m4a))
                .await
                .map_err(|_| SpsyncError::DownloadAborted)??;

            (
                mp3,
                meta_from_info(info),
                None,
                None,
                SOURCED_FORMAT.to_owned(),
            )
        } else {
            let audio = self.download(track).await?;
            let source_format = format!("{:?}", audio.format);
            let (meta, cover, ogg, cover_error) =
                (audio.meta, audio.cover, audio.ogg, audio.cover_error);

            let mp3 = tokio::task::spawn_blocking(move || transcode::ogg_to_mp3(ogg))
                .await
                .map_err(|_| SpsyncError::DownloadAborted)??;

            (mp3, meta, cover, cover_error, source_format)
        };

        let relative = file_name(&meta, &track.id, taken);
        let absolute = self.config().library_dir.join(&relative);
        let partial = absolute.with_extension(PARTIAL_EXTENSION);

        fs::write(&partial, &mp3)?;
        write_tags(&partial, &track.id, &meta, cover.as_ref())?;
        fs::rename(&partial, &absolute)?;

        Ok(Downloaded {
            entry: Entry {
                uri: track.uri.clone(),
                path: relative,
                added_at: track.added_at,
                liked: true,
                artist: meta.artists.first().cloned().unwrap_or_default(),
                album: meta.album.clone(),
                source_format,
                encoder: transcode::ENCODER.to_owned(),
                source_url: source.unwrap_or_default().to_owned(),
            },
            track_duration: Duration::from_millis(u64::from(meta.duration_ms)),
            cover_error,
            bytes: mp3.len(),
        })
    }

    async fn track_info(
        &self,
        tracks: &[TrackRef],
    ) -> Result<HashMap<String, TrackInfo>, SpsyncError> {
        if tracks.is_empty() {
            return Ok(HashMap::new());
        }

        let resolved = metadata::fetch(&self.session().await?, tracks).await;
        if !resolved.is_empty() && resolved.len() < tracks.len() {
            tracing::warn!(
                resolved = resolved.len(),
                tracks = tracks.len(),
                "some track lengths are unresolved; the estimate charges them the mean"
            );
        }

        Ok(resolved)
    }

    /// # Errors
    ///
    /// Returns [`SpsyncError::NotAuthenticated`] if no credentials are cached. Per-track
    /// failures are collected into the report.
    pub async fn sync_tracks(&self, tracks: &[TrackRef]) -> Result<SyncReport, SpsyncError> {
        sweep_partials(&self.config().library_dir)?;

        let overrides = self.overrides()?;
        let tracks: &[TrackRef] = &queue(tracks, &overrides);

        let mut manifest = self.manifest()?;
        let manifest_path = self.config().library_dir.join(MANIFEST_FILE);

        let mut report = SyncReport::default();
        let mut taken: HashSet<PathBuf> =
            manifest.entries.values().map(|e| e.path.clone()).collect();

        let resolved = self.track_info(tracks).await?;
        let suffix = suffix_lengths(tracks, &resolved);
        let known_lengths = !resolved.is_empty();

        let run_started = Instant::now();
        let mut library_size = library_bytes(&self.config().library_dir);
        let mut downloaded_bytes: u64 = 0;
        let mut downloaded_time = Duration::ZERO;
        let mut work_time = Duration::ZERO;

        tracing::info!(
            tracks = tracks.len(),
            realtime = self.config().realtime,
            library = %human_bytes(library_size),
            length = %human_duration(suffix.first().copied().unwrap_or(Duration::ZERO)),
            "starting library sync"
        );

        for (index, track) in tracks.iter().enumerate() {
            let position = index + 1;
            let progress = format!("{position}/{}", tracks.len());
            let started = Instant::now();

            tracing::info!(progress = %progress, uri = %track.uri, "downloading");

            match self
                .sync_one(
                    track,
                    &taken,
                    overrides.source(&track.id),
                    resolved.get(&track.uri),
                )
                .await
            {
                Ok(done) => {
                    let took = started.elapsed();
                    work_time += took;

                    if let Some(e) = done.cover_error {
                        tracing::warn!(uri = %track.uri, error = %e, "cover art unavailable");
                        report.missing_covers += 1;
                    }

                    report.added += 1;
                    let bytes = u64::try_from(done.bytes).unwrap_or(0);
                    downloaded_bytes += bytes;
                    downloaded_time += done.track_duration;
                    library_size += bytes;

                    tracing::info!(
                        progress = %progress,
                        track = %done.entry.path.display(),
                        took = %human_duration(took),
                        size = %human_bytes(bytes),
                        library = %human_bytes(library_size),
                        projected = %human_bytes(projected_bytes(downloaded_bytes, report.added, tracks.len())),
                        eta = %human_duration(remaining_estimate(
                            self.config().realtime,
                            run_started.elapsed(),
                            work_time,
                            downloaded_time,
                            known_lengths.then(|| suffix[position]),
                            position,
                            tracks.len() - position,
                        )),
                        "downloaded"
                    );

                    taken.insert(done.entry.path.clone());
                    manifest.entries.insert(track.id.clone(), done.entry);
                    manifest.save(&manifest_path)?;

                    if self.config().realtime && position < tracks.len() {
                        pace(done.track_duration, took, position, tracks.len()).await;
                    }
                }
                Err(e) => {
                    work_time += started.elapsed();
                    tracing::warn!(progress = %progress, uri = %track.uri, error = %e, "track failed");
                    let info = resolved.get(&track.uri);
                    report.failed.push(Failure {
                        uri: track.uri.clone(),
                        artist: info.map(|i| i.artist.clone()).unwrap_or_default(),
                        title: info.map(|i| i.title.clone()).unwrap_or_default(),
                        error: e.to_string(),
                    });
                }
            }
        }

        tracing::info!(
            added = report.added,
            failed = report.failed.len(),
            missing_covers = report.missing_covers,
            library = %human_bytes(library_size),
            took = %human_duration(run_started.elapsed()),
            "library sync finished"
        );

        Ok(report)
    }

    fn sourced_is_current(
        manifest: &Manifest,
        library: &std::path::Path,
        id: &str,
        url: &str,
    ) -> bool {
        manifest
            .entries
            .get(id)
            .is_some_and(|entry| entry.source_url == url && library.join(&entry.path).is_file())
    }

    /// Downloads url-sourced tracks, unpaced and without a spotify session.
    ///
    /// # Errors
    ///
    /// Returns [`SpsyncError::Overrides`] if the overrides file is malformed, or
    /// [`SpsyncError::Manifest`] if the manifest cannot be read or written.
    pub async fn sync_sourced(&self) -> Result<SourcedReport, SpsyncError> {
        let overrides = self.overrides()?;
        let library = self.config().library_dir.clone();
        let manifest_path = library.join(MANIFEST_FILE);
        let mut manifest = self.manifest()?;
        let mut taken: HashSet<PathBuf> =
            manifest.entries.values().map(|e| e.path.clone()).collect();

        let mut report = SourcedReport::default();

        for (id, entry) in &overrides.entries {
            let Some(url) = overrides.source(id) else {
                continue;
            };

            report.tracks += 1;
            if Self::sourced_is_current(&manifest, &library, id, url) {
                continue;
            }

            let (artist, title) = entry.names();
            let info = TrackInfo {
                length: Duration::ZERO,
                title,
                artist,
                album: String::new(),
            };
            let track = TrackRef {
                id: id.clone(),
                uri: format!("spotify:track:{id}"),
                added_at: None,
            };

            tracing::info!(id = %id, url = %url, "sourcing");
            let started = Instant::now();

            match self.sync_one(&track, &taken, Some(url), Some(&info)).await {
                Ok(done) => {
                    tracing::info!(
                        track = %done.entry.path.display(),
                        took = %human_duration(started.elapsed()),
                        size = %human_bytes(u64::try_from(done.bytes).unwrap_or(0)),
                        "sourced"
                    );

                    report.downloaded += 1;
                    taken.insert(done.entry.path.clone());
                    manifest.entries.insert(id.clone(), done.entry);
                    manifest.save(&manifest_path)?;
                }
                Err(e) => {
                    tracing::warn!(id = %id, url = %url, error = %e, "sourcing failed");
                    report.failures.push(Failure {
                        uri: track.uri,
                        artist: info.artist,
                        title: info.title,
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
        let removals = apply_removals(
            &mut manifest,
            &diff.remove,
            library_dir,
            self.config().preserve,
        );
        report.removed = removals.removed;
        report.failed.extend(removals.failed);
        manifest.save(&library_dir.join(MANIFEST_FILE))?;

        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::{
        collections::{HashMap, HashSet},
        fs,
        path::PathBuf,
        time::Duration,
    };

    use tempfile::TempDir;

    use super::{
        Entry, Manifest, Removed, TrackInfo, TrackMeta, TrackRef, apply_removals, apply_restores,
        file_name, human_bytes, human_duration, partition_restores, projected_bytes,
        remaining_estimate, suffix_lengths, sweep_partials,
    };

    fn info(secs: u64) -> TrackInfo {
        TrackInfo {
            length: Duration::from_secs(secs),
            title: String::new(),
            artist: String::new(),
            album: String::new(),
        }
    }

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
    fn formats_durations_by_magnitude() {
        assert_eq!(human_duration(Duration::from_secs(45)), "45s");
        assert_eq!(human_duration(Duration::from_secs(125)), "2m5s");
        assert_eq!(human_duration(Duration::from_secs(3700)), "1h1m");
        assert_eq!(human_duration(Duration::from_hours(100)), "4d4h");
    }

    #[test]
    fn formats_bytes_by_magnitude() {
        assert_eq!(human_bytes(5120), "5KB");
        assert_eq!(human_bytes(9_000_000), "8.5MB");
        assert_eq!(human_bytes(13_000_000_000), "12.1GB");
    }

    #[test]
    fn projects_total_size_from_progress() {
        assert_eq!(projected_bytes(9_000_000, 1, 1000), 9_000_000_000);
        assert_eq!(projected_bytes(0, 0, 1000), 0);
    }

    #[test]
    fn estimates_remaining_from_known_lengths_when_pacing() {
        let remaining_length = Duration::from_secs(20_000);
        let work_time = Duration::from_secs(50);

        assert_eq!(
            remaining_estimate(
                true,
                Duration::from_secs(10),
                work_time,
                Duration::ZERO,
                Some(remaining_length),
                10,
                100,
            ),
            Duration::from_secs(20_500)
        );
    }

    #[test]
    fn falls_back_to_average_length_when_lengths_are_unknown() {
        assert_eq!(
            remaining_estimate(
                true,
                Duration::from_secs(10),
                Duration::from_secs(50),
                Duration::from_secs(2000),
                None,
                10,
                100,
            ),
            Duration::from_secs(20_000)
        );
    }

    #[test]
    fn estimates_remaining_from_elapsed_when_not_pacing() {
        assert_eq!(
            remaining_estimate(
                false,
                Duration::from_secs(100),
                Duration::from_secs(100),
                Duration::from_secs(2000),
                Some(Duration::from_secs(20_000)),
                10,
                100,
            ),
            Duration::from_secs(1000)
        );
    }

    #[test]
    fn estimates_nothing_remaining_when_done() {
        assert_eq!(
            remaining_estimate(
                true,
                Duration::from_secs(100),
                Duration::from_secs(100),
                Duration::from_secs(100),
                Some(Duration::ZERO),
                10,
                0,
            ),
            Duration::ZERO
        );
    }

    #[test]
    fn suffix_lengths_run_backwards_from_zero() {
        let tracks = [track("a"), track("b"), track("c")];
        let lengths: HashMap<String, TrackInfo> = tracks
            .iter()
            .zip([30, 60, 90])
            .map(|(t, secs)| (t.uri.clone(), info(secs)))
            .collect();

        assert_eq!(
            suffix_lengths(&tracks, &lengths),
            vec![
                Duration::from_mins(3),
                Duration::from_secs(150),
                Duration::from_secs(90),
                Duration::ZERO,
            ]
        );
    }

    #[test]
    fn suffix_lengths_charge_unknown_tracks_the_mean() {
        let tracks = [track("a"), track("b"), track("c")];
        let lengths: HashMap<String, TrackInfo> = [
            (tracks[0].uri.clone(), info(30)),
            (tracks[1].uri.clone(), info(90)),
        ]
        .into_iter()
        .collect();

        assert_eq!(suffix_lengths(&tracks, &lengths)[0], Duration::from_mins(3));
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
            source_url: String::new(),
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

        let result = apply_removals(&mut manifest, &removed, dir.path(), true);

        assert_eq!(result.removed, 1);
        assert!(result.failed.is_empty());
        assert!(dir.path().join("a.mp3").is_file());
        assert!(!manifest.entries["a"].liked);
    }

    #[test]
    #[cfg(unix)]
    fn undeletable_file_is_reported_and_entry_kept() {
        use std::os::unix::fs::PermissionsExt;

        let (dir, mut manifest) = library(&[("a", true)], true);
        let removed = vec![Removed {
            id: "a".to_owned(),
            entry: entry("a", true),
        }];

        let mut perms = fs::metadata(dir.path()).expect("metadata").permissions();
        perms.set_mode(0o555);
        fs::set_permissions(dir.path(), perms).expect("chmod");

        let result = apply_removals(&mut manifest, &removed, dir.path(), false);

        let mut perms = fs::metadata(dir.path()).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(dir.path(), perms).expect("restore chmod");

        assert_eq!(result.removed, 0);
        assert_eq!(result.failed.len(), 1);
        assert!(manifest.entries.contains_key("a"));
        assert!(dir.path().join("a.mp3").is_file());
    }

    #[test]
    fn sweeps_partial_downloads_and_keeps_finished_ones() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("a.mp3.part"), b"partial").expect("write");
        fs::write(dir.path().join("b.mp3.part"), b"partial").expect("write");
        fs::write(dir.path().join("c.mp3"), b"done").expect("write");

        assert_eq!(sweep_partials(dir.path()).expect("sweep"), 2);

        assert!(!dir.path().join("a.mp3.part").exists());
        assert!(!dir.path().join("b.mp3.part").exists());
        assert!(dir.path().join("c.mp3").is_file());
    }

    #[test]
    fn sweep_reports_unreadable_directory() {
        let dir = tempfile::tempdir().expect("tempdir");

        assert!(sweep_partials(&dir.path().join("nope")).is_err());
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

        let result = apply_removals(&mut manifest, &removed, dir.path(), false);

        assert_eq!(result.removed, 1);
        assert!(result.failed.is_empty());
        assert!(!dir.path().join("a.mp3").exists());
        assert!(!manifest.entries.contains_key("a"));
    }
}
