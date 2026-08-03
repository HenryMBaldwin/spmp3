use std::{fs, os::unix::fs::MetadataExt, path::Path};

pub(crate) fn is_mounted(path: &Path) -> bool {
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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::fs;

    use super::is_mounted;

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
}
