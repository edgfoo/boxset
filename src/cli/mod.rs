//! The plain-terminal client: flags in, `TargetConfig`/`Selection`/`Sources`
//! to the backend, lines out. Holds no behaviour the backend lacks.

mod errors;
mod flags;
mod help;
mod live;
mod plan;
mod style;
mod units;

pub use errors::fail_parse;
pub use flags::FieldFlags;
pub use help::{print_build_help, print_help, print_summary};

use std::collections::{BTreeMap, HashMap};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use boxset::config::{Config, TargetConfig};
use boxset::error::Tool;
use boxset::plan::Selection;
use boxset::problem::Severity;
use boxset::sources::{SourceState, Sources};

use live::LiveReporter;

const DEFAULT_JOBS: usize = 2;

pub fn print_version() {
    println!("boxset {}", boxset::lock::boxset_version());

    for (tool, name) in [(Tool::Ffmpeg, "ffmpeg"), (Tool::Ffprobe, "ffprobe")] {
        match boxset::environment::resolve_tool_path(tool) {
            Some(path) => println!(
                "{name} {} ({})",
                boxset::lock::tool_version(&path),
                path.display()
            ),
            None => println!("{name} not found"),
        }
    }

    let Some(ffmpeg) = boxset::environment::resolve_tool_path(Tool::Ffmpeg) else {
        return;
    };
    let Some(availability) = boxset::environment::encoder_availability(&ffmpeg) else {
        return;
    };

    let (present, missing): (Vec<_>, Vec<_>) = availability.iter().partition(|(_, has)| *has);
    let names = |list: &[&(&str, bool)]| {
        list.iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(" ")
    };

    println!("  encoders: {}", names(&present));
    if !missing.is_empty() {
        println!("  missing:  {}", names(&missing));
    }

    let (major, minor) = boxset::command::MIN_FFMPEG_VERSION;
    println!("  tested against ffmpeg {major}.{minor} and newer");
}

/// How this invocation runs, as opposed to what its targets are: flags win
/// over the config file's project-wide keys.
struct RunSettings {
    out_dir: PathBuf,
    jobs: usize,
    dry_run: bool,
    verbose: bool,
    /// Skips the confirmation before overwriting existing outputs.
    yes: bool,
    /// Where the lockfile lives: beside the config, or the cwd for a
    /// single-shot run that has none.
    lock_dir: PathBuf,
    /// Named as the locator when a problem belongs to the config file rather
    /// than to one target.
    config_path: Option<PathBuf>,
}

/// `boxset video.mp4`: one positional source, described entirely by flags
pub fn run_single_shot(source: &Path, fields: &FieldFlags) -> anyhow::Result<()> {
    let config = match fields.to_target_config(source.to_path_buf()) {
        Ok(config) => config,
        Err(note) => errors::fail_with_note(note),
    };
    let settings = RunSettings {
        out_dir: fields
            .out_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("export")),
        jobs: fields.jobs.unwrap_or(DEFAULT_JOBS),
        dry_run: fields.dry_run,
        verbose: fields.verbose,
        yes: fields.yes,
        lock_dir: PathBuf::from("."),
        config_path: None,
    };
    run(
        vec![config],
        &TargetConfig::default(),
        &BTreeMap::new(),
        &Selection::all(),
        &settings,
    )
}

/// `boxset build [--target NAME]...`: reconstructs the outputs boxset.toml
/// describes, filtered to the named targets if any are given.
pub fn build(
    targets: &[String],
    config_path: Option<&Path>,
    fields: &FieldFlags,
) -> anyhow::Result<()> {
    if let Some(flag) = fields.first_field_flag() {
        errors::fail_with_note(style::Note {
            severity: Severity::Error,
            locator: None,
            message: format!("{flag} can't be used with `build`"),
            detail: vec![
                "Target settings come from boxset.toml.".to_string(),
                format!("Try: boxset <video> {flag} ...   for a one-off"),
            ],
        });
    }

    let given = config_path.unwrap_or(Path::new(boxset::config::CONFIG_FILE));
    let path = match given.is_dir() {
        true => given.join(boxset::config::CONFIG_FILE),
        false => given.to_path_buf(),
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) => errors::fail_with_note(missing_config_note(config_path, &path)),
    };
    let config: Result<Config, _> = toml::from_str(&text);
    let mut config = match config {
        Ok(config) => config,
        Err(e) => errors::fail_with_note(errors::toml_note(&path, &text, &e)),
    };

    // Everything downstream sees paths already resolved, so no later stage
    // needs to know where the config came from.
    let dir = path.parent().unwrap_or(Path::new("")).to_path_buf();
    config.rebase(&dir);

    let settings = RunSettings {
        out_dir: match &fields.out_dir {
            Some(flag) => boxset::config::resolve_against(&dir, flag),
            None => config.out_dir.clone(),
        },
        jobs: fields.jobs.or(config.jobs).unwrap_or(DEFAULT_JOBS),
        dry_run: fields.dry_run,
        verbose: fields.verbose,
        yes: fields.yes,
        lock_dir: dir,
        config_path: Some(path.clone()),
    };
    let selection = Selection {
        names: targets.to_vec(),
    };
    run(
        config.merged_targets(),
        &config.defaults,
        &config.unknown,
        &selection,
        &settings,
    )
}

fn missing_config_note(given: Option<&Path>, path: &Path) -> style::Note {
    let message = match given {
        Some(_) => format!("There's no config at {}", path.display()),
        None => "There's no boxset.toml in this directory.".to_string(),
    };
    style::Note {
        severity: Severity::Error,
        locator: None,
        message,
        detail: vec!["Try: boxset <video>   to prepare one video without a config".to_string()],
    }
}

/// The shared pipeline both entry paths run.
fn run(
    configs: Vec<TargetConfig>,
    defaults: &TargetConfig,
    top_level: &BTreeMap<String, toml::Value>,
    selection: &Selection,
    settings: &RunSettings,
) -> anyhow::Result<()> {
    let paths: Vec<PathBuf> = configs.iter().filter_map(|c| c.src.clone()).collect();

    let mut sources = Sources::new();
    sources.request(&paths);
    sources.wait();

    let problems = boxset::validate(&configs, defaults, top_level, &settings.out_dir, &sources);
    let srcs: Vec<Option<PathBuf>> = configs.iter().map(|c| c.src.clone()).collect();
    let notes: Vec<style::Note> = problems
        .iter()
        .map(|problem| errors::problem_note(problem, &srcs, settings.config_path.as_deref()))
        .collect();

    if problems.iter().any(|p| p.severity == Severity::Error) {
        println!();
        style::print_notes(&notes);
        println!();
        println!("  {}", style::dim("Nothing was encoded."));
        println!();
        std::process::exit(1);
    }

    let mut resolved = Vec::new();
    let mut probes = Vec::new();
    for config in &configs {
        let src = config.src.as_ref().expect("validated: src is present");
        let Some(SourceState::Probed(probe)) = sources.get(src) else {
            unreachable!("validated: every source probed");
        };
        resolved.push(boxset::resolve(config, probe, &settings.out_dir));
        probes.push(Arc::new(probe.clone()));
    }

    let known: Vec<String> = resolved.iter().map(boxset::plan::target_name).collect();
    let unmatched: Vec<&String> = selection
        .names
        .iter()
        .filter(|name| !known.contains(name))
        .collect();
    if let Some(name) = unmatched.first() {
        errors::fail_with_note(style::Note {
            severity: Severity::Error,
            locator: None,
            message: format!("boxset.toml has no target called `{name}`"),
            detail: match known.first() {
                Some(first) => vec![
                    format!("It defines: {}", known.join(", ")),
                    format!("Try: boxset build --target {first}"),
                ],
                None => vec!["It defines no targets.".to_string()],
            },
        });
    }

    let plan = boxset::plan(&resolved, &probes, selection);
    if plan.tasks.is_empty() {
        println!("Nothing to do.");
        return Ok(());
    }

    let blocks = plan::group_by_source(&plan);
    let targets = plan
        .tasks
        .iter()
        .map(|t| t.id.target)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    plan::print_plan(
        &blocks,
        &settings.out_dir,
        targets,
        plan.tasks.len(),
        &notes,
    );

    let requirements = boxset::check_environment(&plan);

    if settings.dry_run {
        if let Err(e) = boxset::ensure_available(&requirements) {
            errors::fail_with_note(errors::task_note(&e));
        }
        return Ok(());
    }

    if !confirm(settings.yes)? {
        return Ok(());
    }

    let mut reporter = LiveReporter::new(&plan, settings.verbose);
    if let Err(e) = boxset::ensure_met(&requirements, &mut reporter) {
        errors::fail_with_note(errors::task_note(&e));
    }

    let source_hashes = hash_sources(&plan);

    style::section("Building");
    let started = std::time::Instant::now();
    let outcome = boxset::execute(&plan, &mut reporter, settings.jobs);
    let wall = started.elapsed();
    let bytes = written_bytes(&plan, &outcome);

    style::section(live::closing_section(outcome.failed));
    for line in live::closing_lines(
        &plan,
        &outcome.produced,
        outcome.failed,
        bytes,
        wall,
        &settings.out_dir,
    ) {
        println!("{line}");
    }

    let all_outputs = boxset::plan::all_output_paths(&resolved);
    write_lockfile(
        &plan,
        &all_outputs,
        &outcome,
        &source_hashes,
        &settings.lock_dir,
    );

    if outcome.failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

/// Whether to go ahead. Nothing to answer the prompt outside a terminal, so
/// a non-interactive run proceeds rather than blocking forever.
fn confirm(yes: bool) -> anyhow::Result<bool> {
    if yes {
        return Ok(true);
    }

    if !std::io::stdin().is_terminal() {
        return Ok(true);
    }

    print!("  {} [y/n] ", style::bold("Proceed?"));
    std::io::stdout().flush()?;

    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;

    let answer = answer.trim().to_lowercase();
    if answer.is_empty() || answer == "y" || answer == "yes" {
        return Ok(true);
    }

    println!("  {}", style::dim("Stopped; nothing was encoded."));
    Ok(false)
}

fn written_bytes(plan: &boxset::Plan, outcome: &boxset::execute::ExecutionOutcome) -> u64 {
    plan.tasks
        .iter()
        .filter(|task| outcome.produced.contains(&task.id))
        .filter_map(|task| std::fs::metadata(&task.output_path).ok())
        .map(|meta| meta.len())
        .sum()
}

fn hash_sources(plan: &boxset::Plan) -> HashMap<PathBuf, String> {
    let mut hashes = HashMap::new();
    for task in &plan.tasks {
        let src = &task.probe.src;
        if hashes.contains_key(src) {
            continue;
        }
        if let Some(hash) = boxset::lock::hash_file(src) {
            hashes.insert(src.clone(), hash);
        }
    }
    hashes
}

/// Writes an entry per produced output, beside `boxset.toml`. Entries for
/// outputs outside `all_outputs` are dropped, so a target the config no longer
/// describes leaves nothing behind.
fn write_lockfile(
    plan: &boxset::Plan,
    all_outputs: &[PathBuf],
    outcome: &boxset::execute::ExecutionOutcome,
    source_hashes: &HashMap<PathBuf, String>,
    dir: &Path,
) {
    let ffmpeg_version = boxset::lock::ffmpeg_version();
    let boxset_version = boxset::lock::boxset_version();

    let entries = plan
        .tasks
        .iter()
        .filter(|task| outcome.produced.contains(&task.id))
        .filter_map(|task| {
            let output_hash = boxset::lock::hash_file(&task.output_path)?;
            Some((
                task.output_path.clone(),
                boxset::LockEntry {
                    source_hash: source_hashes
                        .get(&task.probe.src)
                        .cloned()
                        .unwrap_or_default(),
                    output_hash,
                    args_hash: boxset::lock::args_hash(&task.work),
                    boxset_version: boxset_version.clone(),
                    ffmpeg_version: ffmpeg_version.clone(),
                    commands: outcome.commands.get(&task.id).cloned().unwrap_or_default(),
                },
            ))
        });

    let mut lockfile = boxset::Lockfile::read(dir);
    lockfile.absorb(entries, all_outputs);

    // The outputs are already written, so a lockfile that won't write is
    // worth reporting but not worth failing the build over.
    if let Err(e) = lockfile.write(dir) {
        println!();
        style::print_notes(&[style::Note {
            severity: Severity::Warning,
            locator: None,
            message: format!("couldn't write {}: {e}", boxset::lock::LOCK_FILE),
            detail: vec!["Your outputs are fine; boxset will re-encode them next run.".to_string()],
        }]);
    }
}
