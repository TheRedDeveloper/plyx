use clap::{Parser, Subcommand};
use clap_complete::Shell;

mod commands;
pub mod fonts;
pub(crate) mod templates;
pub(crate) mod tui;

/// CLI companion for ply-engine.
#[derive(Parser)]
#[command(name = "plyx", version, about, long_about = None, disable_help_subcommand = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Scaffold a new ply-engine project.
    Init,

    /// Add features or fonts to an existing project.
    Add {
        /// What to add (e.g. "shader-pipeline", "font comic sans").
        /// If omitted, opens interactive mode.
        args: Vec<String>,
    },

    /// Build an Android APK.
    Apk {
        /// Use local NDK instead of Docker.
        #[arg(long)]
        native: bool,

        /// Install APK via adb after building.
        #[arg(long)]
        install: bool,

        /// Non-interactive CI mode.
        #[arg(long)]
        auto: bool,
    },

    /// Build for web (WASM).
    Web {
        /// Non-interactive CI mode.
        #[arg(long)]
        auto: bool,
    },

    /// Generate shell completions.
    Completions {
        /// Shell to generate completions for.
        #[arg(value_enum)]
        shell: Shell,

        /// Automatically install completions into your shell config.
        #[arg(long)]
        install: bool,
    },

    // Hidden easter egg commands — not shown in help or tab completion.
    #[command(hide = true)]
    Remove {
        args: Vec<String>,
    },
    #[command(hide = true)]
    Delete {
        args: Vec<String>,
    },
    #[command(hide = true)]
    Erase {
        args: Vec<String>,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        None => commands::help::run(),
        Some(Command::Init) => commands::init::run(),
        Some(Command::Add { args }) => commands::add::run(args),
        Some(Command::Apk { native, install, auto }) => {
            commands::apk::run(native, install, auto);
        }
        Some(Command::Web { auto }) => commands::web::run(auto),
        Some(Command::Completions { shell, install }) => {
            commands::completions::run(shell, install);
        }
        Some(Command::Remove { .. } | Command::Delete { .. } | Command::Erase { .. }) => {
            commands::easter_egg::scared();
        }
    }
}
