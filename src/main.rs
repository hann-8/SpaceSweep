#![cfg_attr(not(windows), allow(unused))]

#[cfg(not(windows))]
compile_error!("SpaceSweep supports Windows.");

mod rules;
mod scan;
mod windows;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about = "Inspect and clean local Windows storage")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Inspect known storage locations, or analyze a directory using file metadata.
    Scan {
        /// Restrict to these drive letters or volume mount points.
        #[arg(long, conflicts_with = "path")]
        volume: Vec<PathBuf>,
        /// Analyze this directory instead of the default locations.
        #[arg(long)]
        path: Option<PathBuf>,
        /// Analyze each selected volume in addition to the quick checks.
        #[arg(long)]
        deep: bool,
        /// Number of largest files and directories to return in an analysis.
        #[arg(long, default_value_t = 10)]
        top: usize,
        /// Write progress to stderr.
        #[arg(long)]
        progress: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cancellation = Arc::new(AtomicBool::new(false));
    let handler = cancellation.clone();
    ctrlc::set_handler(move || handler.store(true, Ordering::Relaxed))?;
    match cli.command {
        Command::Scan {
            volume,
            path,
            deep,
            top,
            progress,
        } => {
            let report = scan::run(
                scan::Options {
                    volumes: volume,
                    path,
                    deep,
                    top,
                    progress,
                },
                &cancellation,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
    }
    Ok(())
}
