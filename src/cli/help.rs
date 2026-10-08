//! `boxset help` output

use boxset::fields::{self, HelpGroup, Shape};

use super::style::{dim, dim_gray, pad, section};

const COLUMN_GAP: usize = 3;

struct HelpRow {
    name: String,
    value_name: &'static str,
    default_note: &'static str,
    /// One entry per printed line.
    description: Vec<String>,
}

struct Group {
    heading: Option<&'static str>,
    rows: Vec<HelpRow>,
}

const fn flag(
    name: &'static str,
    value_name: &'static str,
    default_note: &'static str,
    description: &'static str,
) -> CommandFlag {
    CommandFlag {
        name,
        value_name,
        default_note,
        description,
    }
}

struct CommandFlag {
    name: &'static str,
    value_name: &'static str,
    default_note: &'static str,
    description: &'static str,
}

impl CommandFlag {
    fn to_row(&self) -> HelpRow {
        HelpRow {
            name: self.name.to_string(),
            value_name: self.value_name,
            default_note: self.default_note,
            description: match self.description.is_empty() {
                true => vec![String::new()],
                false => vec![self.description.to_string()],
            },
        }
    }
}

const GENERAL_COMMAND_FLAGS: &[CommandFlag] = &[
    flag("--dry-run", "", "", "print the plan but output nothing"),
    flag("-y, --yes", "", "", "skip confirmations"),
    flag(
        "--html",
        "",
        "",
        "write boxset.html, with a <video> per target",
    ),
    flag(
        "--html-base-url",
        "<url>",
        "",
        "where boxset.html's paths point",
    ),
    flag("--verbose", "", "", "show ffmpeg errors"),
    flag("-h, --help", "", "", "show this doc"),
    flag("-V, --version", "", "", ""),
    flag(
        "--print-schema",
        "",
        "",
        "boxset.toml JSON schema, to stdout",
    ),
];

const BUILD_COMMAND_FLAGS: &[CommandFlag] = &[
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
    flag(
        "--html",
        "",
        "",
        "write boxset.html, with a <video> per target",
    ),
    flag(
        "--html-base-url",
        "<url>",
        "",
        "where boxset.html's paths point",
    ),
    flag("--verbose", "", "", "show ffmpeg's output on failure"),
    flag("-h, --help", "", "", "this doc"),
];

/// A field becomes a help row only if it has a flag
fn rows_in_group(group: HelpGroup) -> Vec<HelpRow> {
    fields::TOP_LEVEL_FIELDS
        .iter()
        .chain(fields::TARGET_FIELDS)
        .flat_map(rows_for_field)
        .filter(|(field_group, _)| *field_group == group)
        .map(|(_, row)| row)
        .collect()
}

/// One field can document two flags: `poster` carries `--no-poster` and its
/// `at` sub-field carries `--poster`.
fn rows_for_field(field: &'static fields::Field) -> Vec<(HelpGroup, HelpRow)> {
    let mut rows = Vec::new();

    if let Some(name) = field.flag {
        rows.push((
            field.group,
            HelpRow {
                name: row_name(name, field.aliases),
                value_name: field.value_name,
                default_note: field.default_note,
                description: vec![field.help_note.to_string()],
            },
        ));
    }

    if let Shape::Table(inner) | Shape::Toggle(inner) = field.shape {
        for sub in inner {
            let Some(name) = sub.flag else {
                continue;
            };
            rows.push((
                sub.group,
                HelpRow {
                    name: row_name(name, sub.aliases),
                    value_name: sub.value_name,
                    default_note: sub.default_note,
                    description: description_lines(sub),
                },
            ));
        }
    }

    rows
}

/// Short aliases go before the flag, long ones after: `-q, --quality`,
/// `--quality-audio, --qa`.
fn row_name(flag: &str, aliases: &[&str]) -> String {
    let (long, short): (Vec<&str>, Vec<&str>) =
        aliases.iter().partition(|alias| alias.starts_with("--"));
    short
        .into_iter()
        .chain([flag])
        .chain(long)
        .collect::<Vec<_>>()
        .join(", ")
}

const CHOICES_PER_LINE: usize = 3;
const WRAP_ABOVE_CHOICES: usize = 5;

fn description_lines(field: &'static fields::Field) -> Vec<String> {
    match field.shape {
        Shape::Choice(options) if options.len() > WRAP_ABOVE_CHOICES => {
            comma_separated_lines(options, CHOICES_PER_LINE)
        }
        _ => vec![field.help_note.to_string()],
    }
}

fn comma_separated_lines(items: &[&str], per_line: usize) -> Vec<String> {
    let mut lines: Vec<String> = items
        .chunks(per_line)
        .map(|chunk| format!("{},", chunk.join(", ")))
        .collect();

    if let Some(last) = lines.last_mut() {
        last.pop();
    }

    lines
}

fn single_shot_groups() -> Vec<Group> {
    let mut general = rows_in_group(HelpGroup::General);
    general.extend(GENERAL_COMMAND_FLAGS.iter().map(CommandFlag::to_row));

    vec![
        Group {
            heading: Some("General"),
            rows: general,
        },
        Group {
            heading: Some("Video"),
            rows: rows_in_group(HelpGroup::Video),
        },
        Group {
            heading: Some("Poster"),
            rows: rows_in_group(HelpGroup::Poster),
        },
        Group {
            heading: Some("Subtitles"),
            rows: rows_in_group(HelpGroup::Subtitles),
        },
    ]
}

fn build_groups() -> Vec<Group> {
    vec![Group {
        heading: None,
        rows: BUILD_COMMAND_FLAGS
            .iter()
            .map(CommandFlag::to_row)
            .collect(),
    }]
}

fn flags_by_codec() -> Vec<Vec<String>> {
    fields::TARGET_FIELDS
        .iter()
        .filter_map(|field| match field.shape {
            Shape::CodecTable(inner) => Some(
                inner
                    .iter()
                    .map(|sub| fields::codec_flag(field.toml_key, sub.toml_key))
                    .collect(),
            ),
            _ => None,
        })
        .collect()
}

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
    ("boxset config <video>...", "write a boxset.toml to edit"),
    (
        "boxset build",
        "prepare many videos according to boxset.toml",
    ),
];

const HELP_HINT: (&str, &str) = ("boxset --help", "options and recipes");

/// The command column, wide enough for every command printed in it
fn command_width() -> usize {
    COMMANDS
        .iter()
        .chain([&HELP_HINT])
        .map(|(command, _)| command.chars().count())
        .max()
        .unwrap_or(0)
        + COLUMN_GAP
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
    print_groups(&single_shot_groups());

    println!();
    println!("  {}", dim("Per-codec ffmpeg overrides"));
    print_encoder_flags();
    println!();
}

pub fn print_config_help() {
    section("boxset config");
    println!("  Create a starter boxset.toml.");
    println!();

    let width = CONFIG_FORMS
        .iter()
        .map(|(form, _)| form.chars().count())
        .max()
        .unwrap_or(0)
        + COLUMN_GAP;
    for (form, gloss) in CONFIG_FORMS {
        println!("  {}{}", pad(form, width), dim(gloss));
    }

    println!();
    println!("  {}", dim("For help with writing config files, refer to:"));
    println!("  {}", dim_gray(boxset::generate::REFERENCE_URL));

    section("Recipes");
    for (index, (description, command)) in CONFIG_RECIPES.iter().enumerate() {
        if index > 0 {
            println!();
        }
        println!("  {}", dim_gray(description));
        println!("  {command}");
    }
    println!();
}

const CONFIG_FORMS: &[(&str, &str)] = &[
    ("boxset config", "Create an empty boxset.toml file"),
    (
        "boxset config <video>...",
        "Create a boxset.toml file with config presets",
    ),
];

const CONFIG_RECIPES: &[(&str, &str)] = &[(
    "Create a boxset.toml file with config presets for Interview.mp4 and Cutaway.mp4",
    "boxset config Interview.mp4 Cutaway.mp4",
)];

pub fn print_build_help() {
    section("boxset build");
    println!("  Prepare videos according to boxset.toml config.");
    println!();
    print_groups(&build_groups());
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
    let (name_width, description_width) = column_widths(groups);

    println!(
        "  {}{}{}",
        pad(&dim_gray("Setting"), name_width + COLUMN_GAP),
        pad(&dim_gray("Description"), description_width + COLUMN_GAP),
        dim_gray("Default")
    );

    for group in groups {
        println!();
        if let Some(heading) = group.heading {
            println!("  {}", dim(heading));
        }
        for row in &group.rows {
            let name = match row.value_name.is_empty() {
                true => row.name.to_string(),
                false => format!("{} {}", row.name, dim(row.value_name)),
            };

            for (index, note) in row.description.iter().enumerate() {
                // The name and default sit on the first line.
                // Description can break over multiple lines.
                let (name, default) = match index {
                    0 => (name.as_str(), row.default_note),
                    _ => ("", ""),
                };
                let line = format!(
                    "  {}{}{}",
                    pad(name, name_width + COLUMN_GAP),
                    pad(&dim(note), description_width + COLUMN_GAP),
                    dim(default)
                );
                println!("{}", line.trim_end());
            }
        }
    }
}

fn print_encoder_flags() {
    let rows = flags_by_codec();
    let width = rows
        .iter()
        .flat_map(|flags| flags.iter())
        .map(|flag| flag.chars().count())
        .max()
        .unwrap_or(0);

    for flags in &rows {
        let row: String = flags
            .iter()
            .map(|flag| pad(&dim(flag), width + 1))
            .collect();
        println!("  {}", row.trim_end());
    }
}

fn column_widths(groups: &[Group]) -> (usize, usize) {
    let rows = || groups.iter().flat_map(|group| group.rows.iter());

    let name = rows()
        .map(|row| match row.value_name.is_empty() {
            true => row.name.chars().count(),
            false => row.name.chars().count() + 1 + row.value_name.chars().count(),
        })
        .chain(["Setting".len()])
        .max()
        .unwrap_or(0);

    let note = rows()
        .flat_map(|row| row.description.iter())
        .map(|note| note.chars().count())
        .chain(["Description".len()])
        .max()
        .unwrap_or(0);

    (name, note)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::flags::{BUILD_FLAGS, RUN_FLAGS, field_flag_names};

    fn documented(groups: &[Group]) -> Vec<String> {
        groups
            .iter()
            .flat_map(|group| group.rows.iter())
            // `-y, --yes` documents two spellings in one row.
            .flat_map(|row| row.name.split(", "))
            .map(str::to_string)
            .collect()
    }

    fn all_documented() -> Vec<String> {
        documented(&single_shot_groups())
            .into_iter()
            .chain(documented(&build_groups()))
            .chain(flags_by_codec().into_iter().flatten())
            .collect()
    }

    fn accepted() -> Vec<String> {
        field_flag_names()
            .into_iter()
            .chain(RUN_FLAGS.iter().copied())
            .chain(BUILD_FLAGS.iter().copied())
            .chain(fields::flag_aliases())
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn every_flag_is_documented() {
        let documented = all_documented();
        let missing: Vec<String> = accepted()
            .into_iter()
            .filter(|flag| !documented.contains(flag))
            .collect();

        assert!(missing.is_empty(), "undocumented flags: {missing:?}");
    }

    #[test]
    fn every_documented_flag_exists() {
        let accepted = accepted();
        let unknown: Vec<String> = all_documented()
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
        let listed = documented(&build_groups());
        let target_settings: Vec<&str> = field_flag_names()
            .into_iter()
            .filter(|flag| listed.iter().any(|listed| listed == flag))
            .collect();

        assert!(
            target_settings.is_empty(),
            "build help lists target settings: {target_settings:?}"
        );
    }
}
