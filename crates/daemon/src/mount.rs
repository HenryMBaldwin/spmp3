use std::{fs, os::unix::fs::MetadataExt, path::Path};

const PROBE_FILE: &str = ".spmp3-probe";

fn is_mount_point(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };

    if !meta.is_dir() {
        return false;
    }

    let Some(parent) = path.parent() else {
        return false;
    };

    let Ok(parent_meta) = fs::metadata(parent) else {
        return false;
    };

    meta.dev() != parent_meta.dev()
}

/// Stale mounts keep their entry and cached `statfs` values; only a write detects them.
fn is_writable(path: &Path) -> bool {
    let probe = path.join(PROBE_FILE);
    if fs::write(&probe, b"").is_err() {
        return false;
    }

    let _ = fs::remove_file(&probe);

    true
}

pub(crate) fn is_mounted(path: &Path) -> bool {
    is_mount_point(path) && is_writable(path)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::fs;

    use super::{PROBE_FILE, is_mounted, is_writable};

    #[test]
    fn plain_subdirectory_is_not_a_mount_point() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).expect("create");

        assert!(!is_mounted(&sub));
    }

    #[test]
    fn missing_path_is_not_mounted() {
        let dir = tempfile::tempdir().expect("tempdir");

        assert!(!is_mounted(&dir.path().join("nope")));
    }

    #[test]
    fn file_is_not_mounted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("f");
        fs::write(&file, b"x").expect("write");

        assert!(!is_mounted(&file));
    }

    #[test]
    fn writable_directory_probes_clean() {
        let dir = tempfile::tempdir().expect("tempdir");

        assert!(is_writable(dir.path()));
        assert!(!dir.path().join(PROBE_FILE).exists());
    }

    #[test]
    #[cfg(unix)]
    fn unwritable_directory_fails_the_probe() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let mut perms = fs::metadata(dir.path()).expect("metadata").permissions();
        perms.set_mode(0o555);
        fs::set_permissions(dir.path(), perms).expect("chmod");

        let writable = is_writable(dir.path());

        let mut perms = fs::metadata(dir.path()).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(dir.path(), perms).expect("restore");

        assert!(!writable);
    }
}
