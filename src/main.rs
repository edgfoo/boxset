use clap::{Parser, Subcommand};

mod cli;

/// `boxset video.mp4` (single-shot) and `boxset studio`/`build` are siblings,
/// not a subcommand tree, so the positional source and the subcommands are
/// both optional here and main.rs picks between them.
#[derive(Parser)]
#[command(name = "boxset", about = "Prepare video for the web")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// A single video, prepared with no config read and none written.
    source: Option<std::path::PathBuf>,

    #[command(flatten)]
    fields: cli::FieldFlags,
}

#[derive(Subcommand)]
enum Command {
    /// Guided TUI: pick sources, configure targets, write config, build.
    Studio,
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
    let cli = Cli::parse();

    match (cli.command, cli.source) {
        (None, None) => cli::print_help(),
        (None, Some(source)) => cli::run_single_shot(&source, &cli.fields)?,
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
