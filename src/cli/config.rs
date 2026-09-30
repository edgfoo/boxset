//! `boxset config`: writes a starter boxset.toml in the working directory.

use std::path::{Path, PathBuf};

use boxset::generate::{self, Entry, WriteOutcome};
use boxset::problem::Severity;
use boxset::sources::{SourceState, Sources};

use super::errors;
use super::style::{self, bold, dim, dim_gray};

pub fn write_config(sources: &[PathBuf]) -> anyhow::Result<()> {
    let widths = probe_widths(sources);
    let entries: Vec<Entry> = sources
        .iter()
        .zip(&widths)
        .map(|(src, width)| Entry {
            src: src.as_path(),
            width: *width,
        })
        .collect();

    let outcome =
        generate::write_config_file(Path::new("."), &entries, &boxset::lock::boxset_version());
    match outcome {
        WriteOutcome::Written(_) => {}
        WriteOutcome::AlreadyExists(_) => errors::fail_with_note(style::Note {
            severity: Severity::Error,
            locator: None,
            message: format!("There's already a {} here.", boxset::config::CONFIG_FILE),
            detail: vec![
                "boxset won't overwrite it.".to_string(),
                "Move or delete it first, or add targets to it by hand.".to_string(),
            ],
            cause: None,
        }),
        WriteOutcome::Failed { path, source } => errors::fail_with_note(style::Note {
            severity: Severity::Error,
            locator: None,
            message: format!("Couldn't write {}", path.display()),
            detail: vec![],
            cause: Some(source.to_string()),
        }),
    }

    let unreadable: Vec<&PathBuf> = sources
        .iter()
        .zip(&widths)
        .filter(|(_, width)| width.is_none())
        .map(|(src, _)| src)
        .collect();

    report(sources, &unreadable);
    Ok(())
}

fn probe_widths(sources: &[PathBuf]) -> Vec<Option<u32>> {
    if sources.is_empty() {
        return Vec::new();
    }

    let mut probed = Sources::new();
    probed.request(sources);
    probed.wait();

    // If probing fails, silently ignore the error
    sources
        .iter()
        .map(|src| match probed.get(src) {
            Some(SourceState::Probed(probe)) => Some(probe.width),
            _ => None,
        })
        .collect()
}

fn report(sources: &[PathBuf], unreadable: &[&PathBuf]) {
    style::section("Wrote boxset.toml");

    if !sources.is_empty() {
        let entry = match sources.len() {
            1 => "entry",
            _ => "entries",
        };
        println!(
            "  {}",
            dim(&format!(
                "[[target]] {entry} added for {}.",
                names(sources.iter())
            ))
        );

        if !unreadable.is_empty() {
            println!(
                "  {}",
                dim(&format!(
                    "Failed to probe {}.",
                    names(unreadable.iter().copied())
                ))
            );
        }

        println!();
    }

    println!(
        "  Use this file to list your source videos and describe what boxset should produce for them."
    );
    println!();
    println!("  Then run {} to execute your plan.", bold(BUILD_COMMAND));

    println!();
    println!(
        "  {}",
        dim("Read boxset's project guide for guidance on adding to boxset.toml.")
    );
    println!("  {}", dim_gray(generate::REFERENCE_URL));
    println!();
    println!(
        "  {}",
        dim("Your IDE can show you documentation and available options, once configured.")
    );
    println!();
    println!(
        "  {}",
        dim("In VS Code, just download the Even Better TOML extension.")
    );
    println!("  {}", dim_gray(EVEN_BETTER_TOML_URL));
    println!();
}

fn names<'a>(paths: impl Iterator<Item = &'a PathBuf>) -> String {
    paths
        .map(|p| p.to_string_lossy())
        .collect::<Vec<_>>()
        .join(", ")
}

const BUILD_COMMAND: &str = "boxset build";

const EVEN_BETTER_TOML_URL: &str =
    "https://marketplace.visualstudio.com/items?itemName=tamasfe.even-better-toml";
