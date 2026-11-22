#![cfg_attr(not(windows), allow(unused))]

#[cfg(not(windows))]
compile_error!("SpaceSweep supports Windows.");

mod checked_file;
mod cleanup;
mod discovery;
mod rules;
mod scan;
mod store;
mod windows;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about = "Inspect and clean local Windows storage")]
struct Cli {
    /// Existing local directory for scan snapshots, plans and receipts.
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
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
        /// Find common kinds of junk and look for scattered temporary artifacts.
        #[arg(long)]
        discover: bool,
        /// Number of largest files and directories to return in an analysis.
        #[arg(long, default_value_t = 10)]
        top: usize,
        /// Stop after this many seconds and report the coverage reached so far.
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        max_seconds: Option<u64>,
        /// Write progress to stderr.
        #[arg(long)]
        progress: bool,
    },
    /// Build a local, read-only preview for eligible candidates in saved scans.
    Plan {
        /// Saved scan ID. Repeat to combine scans.
        #[arg(long, required = true)]
        scan: Vec<String>,
        #[arg(long)]
        exclude_rule: Vec<String>,
        #[arg(long)]
        exclude_volume: Vec<PathBuf>,
        /// Remove an exact displayed path from the preview.
        #[arg(long)]
        exclude_file: Vec<PathBuf>,
    },
    /// Permanently delete the exact items in an explicitly approved preview.
    Apply { plan_id: String },
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
            discover,
            top,
            max_seconds,
            progress,
        } => {
            let mut report = scan::run(
                scan::Options {
                    volumes: volume,
                    path,
                    deep,
                    discover,
                    top,
                    max_seconds,
                    progress,
                },
                &cancellation,
            )?;
            if !report.cancelled
                && !report.candidates.is_empty()
                && let Err(error) = store::Store::open(cli.state_dir.as_deref())
                    .and_then(|store| cleanup::save_scan(&store, &mut report))
            {
                report.state_error = Some(format!("{error:#}"));
            }
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::Plan {
            scan,
            exclude_rule,
            exclude_volume,
            exclude_file,
        } => {
            let store = store::Store::open(cli.state_dir.as_deref())?;
            let preview = cleanup::plan(
                &store,
                cleanup::Selections {
                    scans: scan,
                    exclude_rule,
                    exclude_volume,
                    exclude_file,
                },
                &cancellation,
            )?;
            println!("{}", serde_json::to_string_pretty(&preview)?);
        }
        Command::Apply { plan_id } => {
            let store = store::Store::open(cli.state_dir.as_deref())?;
            let report = cleanup::apply(&store, plan_id, &cancellation)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
    }
    Ok(())
}
