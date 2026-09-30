use clap::{Parser, Subcommand};

mod cli;

/// `boxset video.mp4` (single-shot) and `boxset studio`/`build`/`config` are
/// siblings, not a subcommand tree, so the positional sources and the
/// subcommands are both optional here and main.rs picks between them.
#[derive(Parser)]
#[command(
    name = "boxset",
    about = "Prepare video for the web",
    disable_help_flag = true,
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// A single video, prepared with no config read and none written.
    /// Several are accepted only to point at `boxset config`.
    sources: Vec<std::path::PathBuf>,

    /// Print boxset's version, and the version and path of the ffmpeg and
    /// ffprobe it resolves.
    #[arg(long, short = 'V')]
    version: bool,

    /// Write the JSON schema for boxset.toml to stdout.
    #[arg(long = "print-schema")]
    print_schema: bool,

    #[arg(long, short = 'h', global = true)]
    help: bool,

    #[command(flatten)]
    fields: cli::FieldFlags,
}

#[derive(Subcommand)]
enum Command {
    /// Guided TUI: pick sources, configure targets, write config, build.
    Studio,
    /// Write a starter boxset.toml, including config presets for the given sources
    Config { sources: Vec<std::path::PathBuf> },
    /// Build every target in boxset.toml, or the ones named with --target.
    Build {
        #[arg(long = "target")]
        targets: Vec<String>,
        /// The config to build, or a directory holding one, defaulting to
        /// boxset.toml here. Paths inside it, and the lockfile beside it,
        /// resolve against its directory.
        #[arg(long = "config", short = 'c')]
        config: Option<std::path::PathBuf>,
        #[command(flatten)]
        fields: Box<cli::FieldFlags>,
    },
}

fn main() -> anyhow::Result<()> {
    // `boxset help` and `boxset build help`: clap would read the word as a
    // source file, or reject it outright after a subcommand. Only the leading
    // words are checked, so a file genuinely named `help` still works.
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["help"] => {
            cli::print_help();
            return Ok(());
        }
        ["build", "help"] => {
            cli::print_build_help();
            return Ok(());
        }
        ["config", "help"] => {
            cli::print_config_help();
            return Ok(());
        }
        _ => {}
    }

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => cli::fail_parse(&e),
    };

    if cli.help {
        match cli.command {
            Some(Command::Build { .. }) => cli::print_build_help(),
            Some(Command::Config { .. }) => cli::print_config_help(),
            _ => cli::print_help(),
        }
        return Ok(());
    }

    if cli.version {
        cli::print_version();
        return Ok(());
    }

    if cli.print_schema {
        cli::print_schema();
        return Ok(());
    }

    match (cli.command, cli.sources.as_slice()) {
        (None, []) => cli::print_summary(),
        (None, [source]) => cli::run_single_shot(source, &cli.fields)?,
        (None, several) => cli::several_sources(several),
        (Some(Command::Config { sources }), _) => cli::write_config(&sources)?,
        (Some(Command::Studio), _) => todo!("launch the TUI"),
        (
            Some(Command::Build {
                targets,
                config,
                fields,
            }),
            _,
        ) => cli::build(&targets, config.as_deref(), &fields)?,
    }

    Ok(())
}
