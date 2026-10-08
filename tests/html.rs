//! `--html` against real builds of the fixtures.

use std::path::{Path, PathBuf};
use std::process::Command;

struct Project {
    dir: PathBuf,
}

impl Project {
    fn new(name: &str, fixtures: &[&str]) -> Self {
        let dir = std::env::temp_dir().join(format!("boxset-html-it-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        for fixture in fixtures {
            let from = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(fixture);
            std::fs::copy(&from, dir.join(fixture)).unwrap();
        }

        Self { dir }
    }

    fn run(&self, args: &[&str]) -> String {
        let output = Command::new(env!("CARGO_BIN_EXE_boxset"))
            .current_dir(&self.dir)
            .args(args)
            .output()
            .expect("failed to run boxset");

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(output.status.success(), "boxset {args:?} failed\n{stdout}");
        stdout
    }

    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.dir.join(path))
            .unwrap_or_else(|_| panic!("{path} wasn't written"))
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn html_paths_are_relative_to_the_out_dir() {
    let project = Project::new("relative", &["bear.mp4"]);
    project.run(&["bear.mp4", "--no-subs", "--codecs", "h264", "-y", "--html"]);

    let bear = project.read("boxset.html");
    assert!(bear.contains(r#"src="bear-320.mp4""#), "{bear}");
    assert!(bear.contains(r#"src="bear-320-poster.jpg""#), "{bear}");
}

#[test]
fn a_base_url_prefixes_html_paths() {
    let project = Project::new("base-url", &["bear.mp4"]);
    project.run(&[
        "bear.mp4",
        "--no-subs",
        "--codecs",
        "h264",
        "-y",
        "--html-base-url",
        "https://cdn.example.com/v",
    ]);

    let bear = project.read("boxset.html");
    assert!(
        bear.contains(r#"src="https://cdn.example.com/v/bear-320.mp4""#),
        "{bear}"
    );
    assert!(
        bear.contains(r#"src="https://cdn.example.com/v/bear-320-poster.jpg""#),
        "{bear}"
    );
}
