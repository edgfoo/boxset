//! Temp paths that delete themselves.

use std::path::{Path, PathBuf};

/// Holds a path to a file that's deleted when this drops.
#[derive(Debug)]
pub struct TempFile {
    path: PathBuf,
    remove: bool,
}

impl TempFile {
    pub fn new(path: PathBuf) -> Self {
        Self { path, remove: true }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Call once the file has been renamed into place, so it isn't deleted.
    pub fn keep(mut self) {
        self.remove = false;
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if self.remove {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn dropping_deletes_the_file() {
        let dir = temp_dir("boxset-tempfile-drop");
        let path = dir.join("partial.mp4");
        std::fs::write(&path, b"x").unwrap();

        drop(TempFile::new(path.clone()));

        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_kept_file_survives_the_drop() {
        let dir = temp_dir("boxset-tempfile-keep");
        let path = dir.join("final.mp4");
        std::fs::write(&path, b"x").unwrap();

        TempFile::new(path.clone()).keep();

        assert!(path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dropping_a_path_that_was_never_written_is_fine() {
        let dir = temp_dir("boxset-tempfile-absent");
        drop(TempFile::new(dir.join("never-created.log")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
