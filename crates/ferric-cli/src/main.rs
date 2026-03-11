//! # Ferric CLI
//!
//! Command-line interface for the Ferric rules engine.
//!
//! ## Phase 5 Baseline Assumptions
//!
//! This binary provides batch and interactive access to the Ferric runtime.
//! Phase 4 diagnostic contracts are preserved:
//!
//! - Source-located diagnostics are rendered with file/line/column context.
//! - Module visibility, ambiguity, and generic dispatch/conflict diagnostics
//!   are displayed without reinterpretation.
//! - Exit codes follow documented contracts (0 = success, 1 = runtime error,
//!   2 = usage error).
//!
//! ## Commands
//!
//! - `run <file>` — Load and execute a CLIPS file
//! - `check <file>` — Load and validate without executing
//! - `repl` — Interactive read-eval-print loop
//! - `version` — Print version information

mod commands;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Command-line interface for the Ferric rules engine.
#[derive(Parser)]
#[command(
    name = "ferric",
    version,
    subcommand_required = true,
    arg_required_else_help = true
)]
struct Cli {
    /// Write a Chrome Trace JSON file for profiling (viewable at ui.perfetto.dev).
    #[cfg(feature = "trace-chrome")]
    #[arg(long, global = true, value_name = "PATH")]
    trace: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Load and execute a CLIPS file.
    Run {
        /// Emit diagnostics as JSON objects on stderr.
        #[arg(long)]
        json: bool,

        /// Path to the CLIPS file to execute.
        file: PathBuf,
    },

    /// Parse and validate a CLIPS file without executing.
    Check {
        /// Emit diagnostics as JSON objects on stderr.
        #[arg(long)]
        json: bool,

        /// Path to the CLIPS file to validate.
        file: PathBuf,
    },

    /// Start an interactive REPL session.
    Repl {
        /// Files to load before entering interactive mode.
        #[arg(long)]
        load: Vec<PathBuf>,
    },

    /// Print version information.
    Version,
}

fn main() {
    let cli = Cli::parse();

    // When built with trace-chrome, optionally initialize a Chrome Trace subscriber.
    // The guard must live until main() returns so the trace file is flushed on drop.
    #[cfg(feature = "trace-chrome")]
    let trace_guard = {
        use tracing_subscriber::prelude::*;
        cli.trace.map(|path| {
            let (layer, guard) = tracing_chrome::ChromeLayerBuilder::new()
                .file(path)
                .include_args(true)
                .build();
            tracing_subscriber::registry().with(layer).init();
            guard
        })
    };

    let exit_code = match cli.command {
        Command::Run { json, file } => commands::run::execute(json, &file),
        Command::Check { json, file } => commands::check::execute(json, &file),
        Command::Repl { load } => commands::repl::execute(&load),
        Command::Version => commands::version::execute(),
    };

    // Drop all locals (including the trace flush guard) before exiting,
    // since std::process::exit terminates without running destructors.
    #[cfg(feature = "trace-chrome")]
    drop(trace_guard);

    std::process::exit(exit_code);
}
