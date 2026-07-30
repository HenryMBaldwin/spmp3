use std::collections::HashSet;

use crate::{
    manifest::{Entry, Manifest},
    track::TrackRef,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diff {
    pub add: Vec<TrackRef>,
    pub restore: Vec<TrackRef>,
    pub remove: Vec<Removed>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removed {
    pub id: String,
    pub entry: Entry,
}

impl Diff {
    pub fn is_empty(&self) -> bool {
        self.add.is_empty() && self.restore.is_empty() && self.remove.is_empty()
    }
}

pub(crate) fn diff(remote: &[TrackRef], manifest: &Manifest) -> Diff {
    let remote_ids: HashSet<&str> = remote.iter().map(|t| t.id.as_str()).collect();

    let mut add = Vec::new();
    let mut restore = Vec::new();

    for track in remote {
        match manifest.entries.get(&track.id) {
            None => add.push(track.clone()),
            Some(entry) if !entry.liked => restore.push(track.clone()),
            Some(_) => {}
        }
    }

    let removed = manifest
        .entries
        .iter()
        .filter(|(id, entry)| entry.liked && !remote_ids.contains(id.as_str()))
        .map(|(id, entry)| Removed {
            id: id.clone(),
            entry: entry.clone(),
        })
        .collect();

    Diff {
        add,
        restore,
        remove: removed,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Manifest, TrackRef, diff};

    fn track(id: &str) -> TrackRef {
        TrackRef {
            id: id.to_owned(),
            uri: format!("spotify:track:{id}"),
            added_at: Some(1),
        }
    }

    fn manifest(ids: &[&str]) -> Manifest {
        entries(ids, true)
    }

    fn entries(ids: &[&str], liked: bool) -> Manifest {
        let mut manifest = Manifest::default();
        for id in ids {
            manifest.entries.insert(
                (*id).to_owned(),
                crate::manifest::Entry {
                    uri: format!("spotify:track:{id}"),
                    path: PathBuf::from(format!("{id}.mp3")),
                    added_at: Some(1),
                    liked,
                },
            );
        }
        manifest
    }

    #[test]
    fn empty_manifest_adds_everything() {
        let remote = vec![track("a"), track("b")];
        let result = diff(&remote, &Manifest::default());

        assert_eq!(result.add, remote);
        assert!(result.remove.is_empty());
    }

    #[test]
    fn in_sync_is_empty() {
        let remote = vec![track("a"), track("b")];
        let result = diff(&remote, &manifest(&["a", "b"]));

        assert!(result.is_empty());
    }

    #[test]
    fn unliked_track_is_removed() {
        let remote = vec![track("a")];
        let result = diff(&remote, &manifest(&["a", "b"]));

        assert!(result.add.is_empty());
        assert_eq!(result.remove.len(), 1);
        assert_eq!(result.remove[0].id, "b");
    }

    #[test]
    fn adds_and_removes_together() {
        let remote = vec![track("a"), track("c")];
        let result = diff(&remote, &manifest(&["a", "b"]));

        assert_eq!(result.add, vec![track("c")]);
        assert_eq!(result.remove.len(), 1);
        assert_eq!(result.remove[0].id, "b");
    }

    #[test]
    fn empty_remote_removes_everything() {
        let result = diff(&[], &manifest(&["a", "b"]));

        assert!(result.add.is_empty());
        assert_eq!(result.remove.len(), 2);
    }

    #[test]
    fn relisted_track_is_restored_not_redownloaded() {
        let remote = vec![track("a")];
        let result = diff(&remote, &entries(&["a"], false));

        assert!(result.add.is_empty());
        assert_eq!(result.restore, remote);
        assert!(result.remove.is_empty());
    }

    #[test]
    fn preserved_entry_is_not_reported_as_removed_again() {
        let result = diff(&[], &entries(&["a"], false));

        assert!(result.is_empty());
    }
}
