//! The lockfile a real build writes, by running the built binary against the
//! fixtures.
//!
//! Output hashes and ffmpeg versions differ between ffmpeg builds, so nothing
//! here asserts a literal hash: the assertions are over which entries exist
//! and how they relate.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Lockfile {
    #[serde(default, rename = "output")]
    outputs: BTreeMap<String, LockEntry>,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
struct LockEntry {
    source_hash: String,
    output_hash: String,
    args_hash: String,
    boxset_version: String,
    ffmpeg_version: String,
}

/// A temp directory with the named fixtures copied in. The binary reads
/// `boxset.toml` and writes `boxset.lock` relative to its working directory,
/// so each test needs its own.
struct Project {
    dir: PathBuf,
}

impl Project {
    fn new(name: &str, fixtures: &[&str]) -> Self {
        let dir = std::env::temp_dir().join(format!("boxset-lock-it-{name}"));
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

    fn write_config(&self, toml: &str) {
        std::fs::write(self.dir.join("boxset.toml"), toml).unwrap();
    }

    fn run(&self, args: &[&str]) -> String {
        let output = Command::new(env!("CARGO_BIN_EXE_boxset"))
            .current_dir(&self.dir)
            .args(args)
            .output()
            .expect("failed to run boxset");
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn lockfile(&self) -> Lockfile {
        let text = std::fs::read_to_string(self.dir.join("boxset.lock"))
            .expect("build wrote no boxset.lock");
        toml::from_str(&text).expect("boxset.lock is not valid TOML")
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn is_sha256(hash: &str) -> bool {
    hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit())
}

#[test]
fn a_build_records_an_entry_per_output_it_produced() {
    let project = Project::new("per-output", &["bear.mp4"]);
    let stdout = project.run(&["bear.mp4", "--no-subs", "--codecs", "h264"]);
    assert!(
        stdout.contains("all 2 outputs created successfully"),
        "{stdout}"
    );

    let lock = project.lockfile();
    assert_eq!(
        lock.outputs.keys().collect::<Vec<_>>(),
        vec![
            "assets/video/bear-320-poster.jpg",
            "assets/video/bear-320.mp4"
        ]
    );

    for entry in lock.outputs.values() {
        assert!(is_sha256(&entry.source_hash), "{entry:?}");
        assert!(is_sha256(&entry.output_hash), "{entry:?}");
        assert!(is_sha256(&entry.args_hash), "{entry:?}");
        assert_eq!(entry.boxset_version, env!("CARGO_PKG_VERSION"));
        assert!(!entry.ffmpeg_version.is_empty());
    }
}

/// Every output of one source records the same source hash, and each output
/// records its own distinct args hash.
#[test]
fn outputs_share_a_source_hash_and_differ_by_args() {
    let project = Project::new("shared-source", &["bear.mp4"]);
    project.run(&["bear.mp4", "--no-subs", "--codecs", "h264"]);

    let lock = project.lockfile();
    let source_hashes: Vec<&String> = lock.outputs.values().map(|e| &e.source_hash).collect();
    assert_eq!(source_hashes[0], source_hashes[1]);

    let poster = &lock.outputs["assets/video/bear-320-poster.jpg"];
    let rendition = &lock.outputs["assets/video/bear-320.mp4"];
    assert_ne!(poster.args_hash, rendition.args_hash);
    assert_ne!(poster.output_hash, rendition.output_hash);
}

/// A narrowed run updates its own targets' entries and leaves the rest
/// untouched.
#[test]
fn a_narrowed_run_leaves_other_targets_entries_alone() {
    let project = Project::new("narrowed", &["bear.mp4", "bear-1280x720.mp4"]);
    project.write_config(
        r#"
[[target]]
src = "bear.mp4"
codecs = ["h264"]
subtitles = false

[[target]]
src = "bear-1280x720.mp4"
codecs = ["h264"]
widths = [640]
subtitles = false
"#,
    );

    project.run(&["build"]);
    let before = project.lockfile();
    assert_eq!(before.outputs.len(), 4);

    // A CRF change the rebuilt target must pick up, and the other must not.
    project.write_config(
        r#"
[[target]]
src = "bear.mp4"
codecs = ["h264"]
h264 = { crf = 30 }
subtitles = false

[[target]]
src = "bear-1280x720.mp4"
codecs = ["h264"]
widths = [640]
subtitles = false
"#,
    );
    project.run(&["build", "--target", "bear"]);
    let after = project.lockfile();

    assert_eq!(
        after.outputs.len(),
        4,
        "the unbuilt target's entries should survive"
    );
    assert_eq!(
        after.outputs["assets/video/bear-1280x720-640.mp4"],
        before.outputs["assets/video/bear-1280x720-640.mp4"],
        "an untouched target's entry should be unchanged"
    );
    assert_ne!(
        after.outputs["assets/video/bear-320.mp4"].args_hash,
        before.outputs["assets/video/bear-320.mp4"].args_hash,
        "the rebuilt target's args hash should follow the crf change"
    );
}

/// A failed task gets no entry, while the tasks beside it still record.
/// An encoder ffmpeg doesn't have is the cheapest reliable failure: it fails
/// before encoding starts, and subtitles are left off so the run stays fast.
#[test]
fn a_failed_task_gets_no_entry_and_the_rest_still_record() {
    let project = Project::new("failed-task", &["bear.mp4"]);
    // `=` form: a value starting with `-` is otherwise parsed as a flag.
    let stdout = project.run(&[
        "bear.mp4",
        "--no-subs",
        "--codecs",
        "h264",
        "--h264-extra-args=-c:v libx266",
    ]);
    assert!(stdout.contains("1 of 2 outputs failed"), "{stdout}");

    let lock = project.lockfile();
    assert!(
        !lock.outputs.contains_key("assets/video/bear-320.mp4"),
        "a failed task should leave no entry"
    );
    assert!(
        lock.outputs
            .contains_key("assets/video/bear-320-poster.jpg"),
        "the task beside it should still record"
    );
    assert_eq!(lock.outputs.len(), 1);
}

/// Every task in the plan runs, so a rebuild of unchanged config re-encodes
/// and rewrites the same entries.
#[test]
fn rebuilding_unchanged_config_records_the_same_entries() {
    let project = Project::new("rebuild", &["bear.mp4"]);
    project.write_config(
        r#"
[[target]]
src = "bear.mp4"
codecs = ["h264"]
subtitles = false
"#,
    );

    project.run(&["build"]);
    let first = project.lockfile();

    let stdout = project.run(&["build"]);
    assert!(
        stdout.contains("all 2 outputs created successfully"),
        "{stdout}"
    );
    let second = project.lockfile();

    assert_eq!(
        first.outputs.keys().collect::<Vec<_>>(),
        second.outputs.keys().collect::<Vec<_>>()
    );
    for (path, entry) in &first.outputs {
        assert_eq!(entry.args_hash, second.outputs[path].args_hash);
        assert_eq!(entry.source_hash, second.outputs[path].source_hash);
    }
}

/// `-c` given a directory finds the `boxset.toml` in it, and resolves paths
/// and the lockfile against that directory just as naming the file does.
#[test]
fn a_config_directory_builds_the_boxset_toml_inside_it() {
    let project = Project::new("config-dir", &[]);
    let videos = project.dir.join("videos");
    std::fs::create_dir_all(&videos).unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bear.mp4"),
        videos.join("bear.mp4"),
    )
    .unwrap();
    std::fs::write(
        videos.join("boxset.toml"),
        r#"
[[target]]
src = "bear.mp4"
codecs = ["h264"]
subtitles = false
poster = false
"#,
    )
    .unwrap();

    project.run(&["build", "-c", "videos"]);

    let text =
        std::fs::read_to_string(videos.join("boxset.lock")).expect("no lockfile beside the config");
    let lock: Lockfile = toml::from_str(&text).unwrap();
    assert_eq!(
        lock.outputs.keys().collect::<Vec<_>>(),
        vec!["videos/assets/video/bear-320.mp4"]
    );
    assert!(videos.join("assets/video/bear-320.mp4").exists());
}

/// Rows group under their target rather than appearing in completion order,
/// which concurrent execution would otherwise interleave.
#[test]
fn each_targets_rows_print_together_under_its_header() {
    let project = Project::new("grouping", &["bear.mp4", "bear-1280x720.mp4"]);
    project.write_config(
        r#"
[[target]]
src = "bear.mp4"
codecs = ["h264"]
subtitles = false
poster = false

[[target]]
src = "bear-1280x720.mp4"
codecs = ["h264"]
widths = [640]
subtitles = false
poster = false
"#,
    );

    // The second target is much slower to encode, so in completion order its
    // row would land first.
    let stdout = project.run(&["build", "--jobs", "2"]);

    let first = stdout
        .find("bear.mp4 (1 version)")
        .expect("no first header");
    let first_row = stdout.find("bear-320.mp4").expect("no first row");
    let second = stdout
        .find("bear-1280x720.mp4 (1 version)")
        .expect("no second header");
    let second_row = stdout.find("bear-1280x720-640.mp4").expect("no second row");

    assert!(first < first_row, "{stdout}");
    assert!(first_row < second, "{stdout}");
    assert!(second < second_row, "{stdout}");
}
