use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::rules::{self, Action, Rule};
use crate::windows::{self, Location, Stamp, Volume};

#[derive(Clone, Debug, Default, Serialize)]
pub struct Totals {
    pub files: u64,
    pub directories: u64,
    pub logical_bytes: u64,
    pub empty_directories: u64,
    pub skipped: BTreeMap<String, u64>,
}

impl Totals {
    fn skip(&mut self, why: impl Into<String>) {
        *self.skipped.entry(why.into()).or_default() += 1;
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SizeEntry {
    pub path: PathBuf,
    pub logical_bytes: u64,
}

#[derive(Default)]
struct Walk {
    totals: Totals,
    files: Vec<SizeEntry>,
    directories: BTreeMap<PathBuf, u64>,
}

fn error_reason(error: &io::Error) -> &'static str {
    match error.kind() {
        io::ErrorKind::PermissionDenied => "access_denied",
        io::ErrorKind::NotFound => "missing_or_changed",
        io::ErrorKind::Unsupported => "unsupported_directory",
        _ => "read_error",
    }
}

fn rank(items: &mut Vec<SizeEntry>, entry: SizeEntry, top: usize) {
    if top == 0
        || (items.len() == top && items.last().unwrap().logical_bytes >= entry.logical_bytes)
    {
        return;
    }
    let index = items.partition_point(|existing| existing.logical_bytes >= entry.logical_bytes);
    items.insert(index, entry);
    items.truncate(top);
}

fn walk(
    root: &Location,
    recursive: bool,
    top: usize,
    cancel: &AtomicBool,
    mut file: impl FnMut(&Location, Stamp),
) -> io::Result<Walk> {
    windows::check_directory(root)?;
    let mut output = Walk::default();
    let mut pending = vec![root.clone()];
    while let Some(directory) = pending.pop() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if directory != *root
            && let Err(error) = windows::check_directory(&directory)
        {
            output.totals.skip(error_reason(&error));
            continue;
        }
        let entries = match fs::read_dir(directory.native()) {
            Ok(entries) => entries,
            Err(error) if directory == *root => return Err(error),
            Err(error) => {
                output.totals.skip(error_reason(&error));
                continue;
            }
        };
        output.totals.directories += 1;
        let mut empty = true;
        for entry in entries {
            empty = false;
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    output.totals.skip(error_reason(&error));
                    continue;
                }
            };
            let meta = match entry.metadata() {
                Ok(meta) => meta,
                Err(error) => {
                    output.totals.skip(error_reason(&error));
                    continue;
                }
            };
            let stamp = Stamp::from(&meta);
            if stamp.attributes & windows::SKIP_ATTRIBUTES != 0 {
                output.totals.skip("reparse_or_offline");
                continue;
            }
            let name = entry.file_name();
            if name.to_str().is_none() {
                output.totals.skip("unsupported_filename");
                continue;
            }
            let location = directory.child(name);
            if meta.is_dir() {
                if recursive {
                    pending.push(location);
                }
                continue;
            }
            if !meta.is_file() {
                output.totals.skip("not_regular_file");
                continue;
            }
            output.totals.files += 1;
            output.totals.logical_bytes = output.totals.logical_bytes.saturating_add(stamp.size);
            file(&location, stamp);
            if top != 0 {
                let relative = location.relative.strip_prefix(&root.relative).unwrap();
                if relative.components().count() > 1
                    && let Some(first) = relative.components().next()
                {
                    let bytes = output
                        .directories
                        .entry(first.as_os_str().into())
                        .or_default();
                    *bytes = bytes.saturating_add(stamp.size);
                }
                rank(
                    &mut output.files,
                    SizeEntry {
                        path: location.relative,
                        logical_bytes: stamp.size,
                    },
                    top,
                );
            }
        }
        if empty {
            output.totals.empty_directories += 1;
        }
    }
    Ok(output)
}

#[derive(Debug, Serialize)]
pub struct RuleReport {
    pub rule_id: String,
    pub label: String,
    pub action: Action,
    pub note: String,
    pub volume_id: Option<String>,
    pub root: Option<PathBuf>,
    pub totals: Option<Totals>,
    pub matching_files: u64,
    pub matching_logical_bytes: u64,
    pub status: &'static str,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DirectoryReport {
    pub volume_id: String,
    pub root: PathBuf,
    pub totals: Option<Totals>,
    pub largest_directories: Vec<SizeEntry>,
    pub largest_files: Vec<SizeEntry>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub mode: &'static str,
    pub volumes: Vec<Volume>,
    pub rules: Vec<RuleReport>,
    pub analysis: Vec<DirectoryReport>,
    pub elapsed_ms: u64,
    pub cancelled: bool,
}

#[derive(Debug)]
pub struct Options {
    pub volumes: Vec<PathBuf>,
    pub path: Option<PathBuf>,
    pub deep: bool,
    pub top: usize,
    pub progress: bool,
}

fn rule_row(rule: &Rule) -> RuleReport {
    RuleReport {
        rule_id: rule.id.clone(),
        label: rule.label.clone(),
        action: rule.action,
        note: rule.note.clone(),
        volume_id: None,
        root: None,
        totals: None,
        matching_files: 0,
        matching_logical_bytes: 0,
        status: "not_found",
        reason: None,
    }
}

pub fn run(options: Options, cancel: &AtomicBool) -> Result<Report> {
    let started = Instant::now();
    ensure!(options.top <= 100, "--top must be at most 100");
    let all = windows::volumes().context("enumerate mounted Windows volumes")?;
    let focus = match options.path {
        Some(path) => {
            let path = windows::scope_path(&path)?;
            Some(
                windows::locate(&path, &all)
                    .context("directory is not on a mounted local volume")?,
            )
        }
        None => None,
    };
    let mut requested = HashSet::new();
    for input in options.volumes {
        let path = windows::scope_path(&input)?;
        let location = windows::locate(&path, &all)
            .with_context(|| format!("unknown local volume: {}", input.display()))?;
        ensure!(
            location.relative.as_os_str().is_empty(),
            "use --path for a directory"
        );
        requested.insert(location.volume);
    }
    if let Some(focus) = &focus {
        requested.insert(focus.volume.clone());
    }
    let volumes: Vec<_> = all
        .iter()
        .filter(|volume| requested.is_empty() || requested.contains(&volume.id))
        .cloned()
        .collect();
    let deep = options.deep || focus.is_some();
    let mut result = Report {
        schema_version: 1,
        mode: if deep { "directory_analysis" } else { "quick" },
        volumes,
        rules: Vec::new(),
        analysis: Vec::new(),
        elapsed_ms: 0,
        cancelled: false,
    };
    if options.progress {
        eprintln!("Volumes discovered: {}", result.volumes.len());
    }
    let now = rules::windows_time_now();
    if focus.is_none() {
        // Prevent a Documents location and the same volume-root hint from counting twice.
        let mut seen = HashSet::new();
        for rule in rules::load()? {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let roots = match rule.locations(&all, &requested) {
                Ok(roots) => roots,
                Err(error) => {
                    let mut report = rule_row(&rule);
                    if error.kind() != io::ErrorKind::NotFound {
                        report.status = "unavailable";
                        report.reason = Some(error.to_string());
                    }
                    result.rules.push(report);
                    continue;
                }
            };
            for root in roots {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                if seen.contains(&root) {
                    continue;
                }
                let mut row = rule_row(&rule);
                if options.progress {
                    eprintln!(
                        "Checking {} on {}",
                        rule.id,
                        windows::view(&root, &result.volumes).display()
                    );
                }
                let checked = walk(&root, rule.action.recursive(), 0, cancel, |file, stamp| {
                    if rule.matches(&file.relative, stamp, now) {
                        row.matching_files += 1;
                        row.matching_logical_bytes =
                            row.matching_logical_bytes.saturating_add(stamp.size);
                    }
                });
                match checked {
                    Ok(output) => {
                        seen.insert(root.clone());
                        row.status = if cancel.load(Ordering::Relaxed)
                            || !output.totals.skipped.is_empty()
                        {
                            "partial"
                        } else {
                            "checked"
                        };
                        row.totals = Some(output.totals);
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => {
                        row.status = "unavailable";
                        row.reason = Some(error.to_string());
                    }
                }
                row.root = Some(windows::view(&root, &result.volumes));
                row.volume_id = Some(root.volume);
                result.rules.push(row);
            }
        }
    }
    if deep {
        let roots = focus.map_or_else(
            || {
                result
                    .volumes
                    .iter()
                    .map(|v| Location {
                        volume: v.id.clone(),
                        relative: PathBuf::new(),
                    })
                    .collect()
            },
            |focus| vec![focus],
        );
        for root in roots {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let report = analyze(
                &root,
                &result.volumes,
                options.top,
                cancel,
                options.progress,
            );
            result.analysis.push(report);
        }
    }
    result.cancelled = cancel.load(Ordering::Relaxed);
    result.elapsed_ms = started.elapsed().as_millis() as u64;
    Ok(result)
}

fn analyze(
    root: &Location,
    volumes: &[Volume],
    top: usize,
    cancel: &AtomicBool,
    progress: bool,
) -> DirectoryReport {
    let display = windows::view(root, volumes);
    if progress {
        eprintln!("Analyzing {}", display.display());
    }
    let mut report = DirectoryReport {
        volume_id: root.volume.clone(),
        root: display,
        totals: None,
        largest_directories: Vec::new(),
        largest_files: Vec::new(),
        error: None,
    };
    match walk(root, true, top, cancel, |_, _| {}) {
        Ok(output) => {
            report.totals = Some(output.totals);
            report.largest_files = output
                .files
                .into_iter()
                .map(|mut row| {
                    row.path = windows::view(
                        &Location {
                            volume: root.volume.clone(),
                            relative: row.path,
                        },
                        volumes,
                    );
                    row
                })
                .collect();
            for (path, bytes) in output.directories {
                rank(
                    &mut report.largest_directories,
                    SizeEntry {
                        path: report.root.join(path),
                        logical_bytes: bytes,
                    },
                    top,
                );
            }
        }
        Err(error) => report.error = Some(error.to_string()),
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_analysis_reports_sizes_without_writing_to_the_scope() {
        let fixture = tempfile::tempdir().unwrap();
        let folder = fixture.path().join("微信样本");
        fs::create_dir(&folder).unwrap();
        fs::create_dir(fixture.path().join("empty")).unwrap();
        let file = folder.join("large.bin");
        fs::write(&file, vec![0; 8192]).unwrap();
        fs::write(fixture.path().join("small.bin"), [1, 2, 3]).unwrap();

        let report = run(
            Options {
                volumes: Vec::new(),
                path: Some(fixture.path().to_owned()),
                deep: false,
                top: 1,
                progress: false,
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(report.volumes.len(), 1);
        assert!(report.rules.is_empty());
        let analysis = &report.analysis[0];
        let totals = analysis.totals.as_ref().unwrap();
        assert_eq!(totals.files, 2);
        assert_eq!(totals.logical_bytes, 8195);
        assert_eq!(totals.empty_directories, 1);
        assert_eq!(analysis.largest_files[0].path, file);
        assert_eq!(analysis.largest_directories[0].path, folder);
        assert_eq!(fs::metadata(file).unwrap().len(), 8192);
    }
}
