//! The only place flags are documented. The tests check this file against the
//! flags the parser accepts, both ways.

use super::style::{dim, dim_gray, pad, section};

const PAD: usize = 3;

struct Flag {
    name: &'static str,
    value: &'static str,
    default: &'static str,
    note: &'static str,
}

struct Group {
    heading: Option<&'static str>,
    flags: &'static [Flag],
}

const fn flag(
    name: &'static str,
    value: &'static str,
    default: &'static str,
    note: &'static str,
) -> Flag {
    Flag {
        name,
        value,
        default,
        note,
    }
}

const SINGLE_SHOT: &[Group] = &[
    Group {
        heading: Some("General"),
        flags: &[
            flag(
                "--name",
                "<name>",
                "source filename",
                "base name for outputs",
            ),
            flag("--out-dir", "<dir>", "./export", "output folder"),
            flag("--jobs", "<n>", "2", "encodes in parallel"),
            flag("--dry-run", "", "", "print the plan but output nothing"),
            flag("-y, --yes", "", "", "skip confirmations"),
            flag("--verbose", "", "", "show ffmpeg errors"),
            flag("-h, --help", "", "", "show this doc"),
            flag("-V, --version", "", "", ""),
        ],
    },
    Group {
        heading: Some("Video"),
        flags: &[
            flag(
                "--quality",
                "<tier>",
                "balanced",
                "video quality: low, balanced, high, max",
            ),
            flag(
                "--codecs",
                "<list>",
                "h264,vp9",
                "transcode to: h264, h265, vp9, av1",
            ),
            flag(
                "--widths",
                "<list>",
                "derived from source",
                "output width(s) in pixels",
            ),
            flag("--crop", "<w:h>", "none", "crop outputs to aspect ratio"),
            flag(
                "--crop-anchor",
                "<where>",
                "centre",
                "centre, top, bottom, left, right",
            ),
            flag(
                "--trim",
                "<start-end>",
                "none",
                "eg 0:05-0:30, either side omittable",
            ),
            flag(
                "--fps",
                "<rate>",
                "the source's",
                "output frames per second",
            ),
            flag("--no-audio", "", "", "drop the audio track"),
        ],
    },
    Group {
        heading: Some("Poster"),
        flags: &[
            flag(
                "--poster",
                "[<at>]",
                "first frame",
                "timestamp of poster, eg. 0:04",
            ),
            flag("--no-poster", "", "", ""),
        ],
    },
    Group {
        heading: Some("Subtitles"),
        flags: &[
            flag(
                "--subs-lang",
                "<code>",
                "auto-detected",
                "force transcription language",
            ),
            flag(
                "--subs-model",
                "<tier>",
                "base",
                "whispr model: base, small, medium, large",
            ),
            flag(
                "--subs-cue-len",
                "<length>",
                "short",
                "cue length: short, medium, long",
            ),
            flag("--no-subs", "", "", ""),
        ],
    },
];

const BUILD: &[Group] = &[Group {
    heading: None,
    flags: &[
        flag("--target", "<name>", "every target", "repeatable"),
        flag(
            "-c, --config",
            "<path>",
            "boxset.toml",
            "a file or its directory",
        ),
        flag("--out-dir", "<dir>", "./export", "or the config's out_dir"),
        flag("--jobs", "<n>", "2", "or the config's jobs"),
        flag("--dry-run", "", "", "print the plan, encode nothing"),
        flag("-y, --yes", "", "", "skip the confirmation"),
        flag("--verbose", "", "", "show ffmpeg's output on failure"),
        flag("-h, --help", "", "", "this doc"),
    ],
}];

const ENCODER_FLAGS: &[&[&str]] = &[
    &[
        "--h264-crf",
        "--h264-preset",
        "--h264-profile",
        "--h264-extra-args",
    ],
    &[
        "--h265-crf",
        "--h265-preset",
        "--h265-profile",
        "--h265-extra-args",
    ],
    &[
        "--vp9-crf",
        "--vp9-cpu-used",
        "--vp9-row-mt",
        "--vp9-extra-args",
    ],
    &["--av1-crf", "--av1-preset", "--av1-extra-args"],
];

const RECIPES: &[(&str, &str)] = &[
    (
        "Create more compressed, lower quality outputs at 640px and 1280px wide",
        "boxset Interview.mp4 --quality low --widths 640,1280",
    ),
    (
        "Rename all outputs interview-web-* and place them in static/media",
        "boxset Interview.mp4 --name interview-web --out-dir static/media",
    ),
    (
        "Crop outputs to a portrait 9:16 aspect ratio",
        "boxset Interview.mp4 --crop 9:16 --crop-anchor top",
    ),
];

const COMMANDS: &[(&str, &str)] = &[
    ("boxset <video>", "prepare one video"),
    (
        "boxset build",
        "prepare many videos according to boxset.toml",
    ),
];

const HELP_HINT: (&str, &str) = ("boxset --help", "options and recipes");

/// The command column, wide enough for every command printed in it.
fn command_width() -> usize {
    COMMANDS
        .iter()
        .chain([&HELP_HINT])
        .map(|(command, _)| command.chars().count())
        .max()
        .unwrap_or(0)
        + PAD
}

pub fn print_summary() {
    print_intro();
    println!();
    println!("  {}  boxset myvideo.mp4", dim("Try it:"));
    println!();
    let (command, gloss) = HELP_HINT;
    println!("  {}{}", pad(command, command_width()), dim(gloss));
    println!();
}

pub fn print_help() {
    print_intro();

    section("Recipes");
    for (index, (description, command)) in RECIPES.iter().enumerate() {
        if index > 0 {
            println!();
        }
        println!("  {}", dim_gray(description));
        println!("  {command}");
    }

    section("Options");
    print_groups(SINGLE_SHOT);

    println!();
    println!("  {}", dim("Per-codec ffmpeg overrides"));
    print_encoder_flags();
    println!();
}

pub fn print_build_help() {
    section("boxset build");
    println!("  Prepare videos according to boxset.toml config.");
    println!();
    print_groups(BUILD);
    println!();
    println!("  {}", dim("Target settings live in boxset.toml."));
    println!();
}

fn print_intro() {
    section("boxset");
    println!("  Prepare video for the web: transcode, compress, capture subtitles and posters.");
    println!();
    for (command, gloss) in COMMANDS {
        println!("  {}{}", pad(command, command_width()), dim(gloss));
    }
}

fn print_groups(groups: &[Group]) {
    let (name_width, note_width) = columns(groups);

    println!(
        "  {}{}{}",
        pad(&dim_gray("Setting"), name_width + PAD),
        pad(&dim_gray("Description"), note_width + PAD),
        dim_gray("Default")
    );

    for group in groups {
        println!();
        if let Some(heading) = group.heading {
            println!("  {}", dim(heading));
        }
        for flag in group.flags {
            let name = match flag.value.is_empty() {
                true => flag.name.to_string(),
                false => format!("{} {}", flag.name, dim(flag.value)),
            };
            let line = format!(
                "  {}{}{}",
                pad(&name, name_width + PAD),
                pad(&dim(flag.note), note_width + PAD),
                dim(flag.default)
            );
            println!("{}", line.trim_end());
        }
    }
}

fn print_encoder_flags() {
    let width = ENCODER_FLAGS
        .iter()
        .flat_map(|flags| flags.iter())
        .map(|flag| flag.chars().count())
        .max()
        .unwrap_or(0);

    for flags in ENCODER_FLAGS {
        let row: String = flags
            .iter()
            .map(|flag| pad(&dim(flag), width + 1))
            .collect();
        println!("  {}", row.trim_end());
    }
}

fn columns(groups: &[Group]) -> (usize, usize) {
    let rows = || groups.iter().flat_map(|group| group.flags);

    let name = rows()
        .map(|flag| match flag.value.is_empty() {
            true => flag.name.chars().count(),
            false => flag.name.chars().count() + 1 + flag.value.chars().count(),
        })
        .chain(["Setting".len()])
        .max()
        .unwrap_or(0);

    let note = rows()
        .map(|flag| flag.note.chars().count())
        .chain(["Description".len()])
        .max()
        .unwrap_or(0);

    (name, note)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::flags::{BUILD_FLAGS, RUN_FLAGS, field_flag_names};

    fn documented(groups: &[Group]) -> Vec<&'static str> {
        groups
            .iter()
            .flat_map(|group| group.flags)
            // `-y, --yes` documents two spellings in one row.
            .flat_map(|flag| flag.name.split(", "))
            .collect()
    }

    fn all_documented() -> Vec<&'static str> {
        documented(SINGLE_SHOT)
            .into_iter()
            .chain(documented(BUILD))
            .chain(ENCODER_FLAGS.iter().flat_map(|flags| flags.iter().copied()))
            .collect()
    }

    fn accepted() -> Vec<&'static str> {
        field_flag_names()
            .chain(RUN_FLAGS.iter().copied())
            .chain(BUILD_FLAGS.iter().copied())
            .collect()
    }

    #[test]
    fn every_flag_is_documented() {
        let documented = all_documented();
        let missing: Vec<&str> = accepted()
            .into_iter()
            .filter(|flag| !documented.contains(flag))
            .collect();

        assert!(missing.is_empty(), "undocumented flags: {missing:?}");
    }

    #[test]
    fn every_documented_flag_exists() {
        let accepted = accepted();
        let unknown: Vec<&str> = all_documented()
            .into_iter()
            .filter(|flag| !accepted.contains(flag))
            .collect();

        assert!(
            unknown.is_empty(),
            "flags in help that don't exist: {unknown:?}"
        );
    }

    /// `build` rejects target settings, so documenting one there would point
    /// at a flag it refuses.
    #[test]
    fn build_documents_no_target_settings() {
        let listed = documented(BUILD);
        let target_settings: Vec<&str> = field_flag_names()
            .filter(|flag| listed.contains(flag))
            .collect();

        assert!(
            target_settings.is_empty(),
            "build help lists target settings: {target_settings:?}"
        );
    }
}
