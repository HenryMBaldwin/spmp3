use std::path::PathBuf;

use common::manifest::Entry;

pub(crate) const MUSIC_DIR: &str = "Music";
const UNKNOWN_ARTIST: &str = "Unknown Artist";
const UNKNOWN_ALBUM: &str = "Unknown Album";
const MAX_COMPONENT: usize = 64;

fn sanitize(value: &str, fallback: &str) -> String {
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

    let trimmed = cleaned.trim().trim_matches('.');
    let truncated: String = trimmed.chars().take(MAX_COMPONENT).collect();
    let truncated = truncated.trim_end();

    if truncated.is_empty() {
        fallback.to_owned()
    } else {
        truncated.to_owned()
    }
}

pub(crate) fn device_path(entry: &Entry) -> PathBuf {
    let artist = sanitize(&entry.artist, UNKNOWN_ARTIST);
    let album = sanitize(&entry.album, UNKNOWN_ALBUM);

    let file = entry
        .path
        .file_name()
        .map_or_else(|| "untitled.mp3".to_owned(), |n| n.to_string_lossy().into());

    PathBuf::from(MUSIC_DIR).join(artist).join(album).join(file)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use common::manifest::Entry;

    use super::device_path;

    fn entry(artist: &str, album: &str, file: &str) -> Entry {
        Entry {
            uri: "spotify:track:abc".to_owned(),
            path: PathBuf::from(file),
            added_at: None,
            liked: true,
            artist: artist.to_owned(),
            album: album.to_owned(),
            source_format: String::new(),
            encoder: String::new(),
        }
    }

    #[test]
    fn nests_under_music_artist_album() {
        assert_eq!(
            device_path(&entry("hey, nothing", "Maine", "hey, nothing - Maine.mp3")),
            PathBuf::from("Music/hey, nothing/Maine/hey, nothing - Maine.mp3")
        );
    }

    #[test]
    fn falls_back_when_metadata_is_missing() {
        assert_eq!(
            device_path(&entry("", "", "x.mp3")),
            PathBuf::from("Music/Unknown Artist/Unknown Album/x.mp3")
        );
    }

    #[test]
    fn strips_separators_from_components() {
        assert_eq!(
            device_path(&entry("AC/DC", "Back:In*Black", "song.mp3")),
            PathBuf::from("Music/AC_DC/Back_In_Black/song.mp3")
        );
    }

    #[test]
    fn ignores_library_subdirectories() {
        assert_eq!(
            device_path(&entry("A", "B", "nested/dir/song.mp3")),
            PathBuf::from("Music/A/B/song.mp3")
        );
    }
}
