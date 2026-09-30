//! Writes a starter `boxset.toml`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::config::CONFIG_FILE;
use crate::schema::schema_url;
use crate::settings::derive_ladder;

pub const REFERENCE_URL: &str = "https://github.com/edgfoo/boxset/blob/main/docs/settings.md";

/// Shown when a probe failed to return a given source's width
const EXAMPLE_LADDER: [u32; 2] = [640, 1280];

pub struct Entry<'a> {
    pub src: &'a Path,
    /// `None` when the probe failed
    pub width: Option<u32>,
}

const INTRO: &[&str] = &[
    "For help writing this file and a full list of settings and what they do, read:",
    REFERENCE_URL,
];

const EXAMPLE_TARGET: &[&str] = &[
    "For each video you'd like to process, add a [[target]] block like this one.",
    "Only `src` is required. Every other option is set for you with a sensible default.",
    "",
    "[[target]]",
    r#"src = "Full interview - final (2).mp4""#,
    r#"name = "interview""#,
    r#"quality = "low""#,
    "...",
];

pub enum WriteOutcome {
    Written(PathBuf),
    AlreadyExists(PathBuf),
    Failed {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Writes the file into `dir`. An existing config is never replaced.
pub fn write_config_file(dir: &Path, entries: &[Entry], version: &str) -> WriteOutcome {
    let path = dir.join(CONFIG_FILE);
    if path.exists() {
        return WriteOutcome::AlreadyExists(path);
    }

    match std::fs::write(&path, config_file(entries, version)) {
        Ok(()) => WriteOutcome::Written(path),
        Err(source) => WriteOutcome::Failed { path, source },
    }
}

pub fn config_file(entries: &[Entry], version: &str) -> String {
    let mut out = String::new();

    let _ = writeln!(out, "#:schema {}", schema_url(version));
    let _ = writeln!(out);
    write_comment(&mut out, INTRO);

    for entry in entries {
        let _ = writeln!(out);
        let _ = writeln!(out, "[[target]]");
        let _ = writeln!(out, "src = {}", toml_string(&entry.src.to_string_lossy()));
        for hint in hints(entry) {
            let _ = writeln!(out, "# {hint}");
        }
    }

    if entries.is_empty() {
        let _ = writeln!(out);
        write_comment(&mut out, EXAMPLE_TARGET);
    }

    out
}

fn write_comment(out: &mut String, lines: &[&str]) {
    for line in lines {
        let _ = match line.is_empty() {
            true => writeln!(out, "#"),
            false => writeln!(out, "# {line}"),
        };
    }
}

fn hints(entry: &Entry) -> Vec<String> {
    let (ladder, note) = match entry.width {
        Some(width) => (derive_ladder(width), "derived from this source"),
        None => (
            EXAMPLE_LADDER.to_vec(),
            "an example; boxset derives a ladder from the source",
        ),
    };

    let widths = ladder
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ");

    // Show two of the most common config options, prompting the user
    // to edit and add more.
    vec![
        "quality = \"balanced\"".to_string(),
        format!("widths = [{widths}]   # {note}"),
    ]
}

fn toml_string(text: &str) -> String {
    let escaped = text.replace('\\', r"\\").replace('"', r#"\""#);
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn entry(src: &str, width: Option<u32>) -> Entry<'_> {
        Entry {
            src: Path::new(src),
            width,
        }
    }

    fn parse(entries: &[Entry]) -> (String, Config) {
        let text = config_file(entries, "0.3.0");
        let config = toml::from_str(&text).expect("generated config parses");
        (text, config)
    }

    #[test]
    fn a_file_with_no_targets_still_parses() {
        let (_, config) = parse(&[]);
        assert!(config.targets.is_empty());
    }

    #[test]
    fn what_it_writes_parses_back_as_a_config() {
        let (_, config) = parse(&[entry("a.mp4", Some(1920)), entry("b.mp4", None)]);
        let srcs: Vec<_> = config.targets.iter().map(|t| t.src.clone()).collect();
        assert_eq!(
            srcs,
            vec![
                Some(Path::new("a.mp4").to_path_buf()),
                Some(Path::new("b.mp4").to_path_buf())
            ]
        );
    }

    #[test]
    fn a_quote_in_a_path_is_escaped() {
        let (_, config) = parse(&[entry(r#"od"d.mp4"#, None)]);
        assert_eq!(
            config.targets[0].src,
            Some(Path::new(r#"od"d.mp4"#).to_path_buf())
        );
    }
}
