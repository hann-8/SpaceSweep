use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::checked_file::CheckedFile;
use crate::discovery::{self, Finding, Groups, Kind, ReviewLevel};
use crate::rules::{self, Action, Rule};
use crate::windows::{self, Folder, Location, Stamp, Volume};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    Cancelled,
    TimeLimit,
}

struct Control<'a> {
    cancel: &'a AtomicBool,
    deadline: Option<Instant>,
    reason: Cell<Option<StopReason>>,
}

impl Control<'_> {
    fn stopped(&self) -> bool {
        if self.reason.get().is_some() {
            return true;
        }
        let reason = if self.cancel.load(Ordering::Relaxed) {
            Some(StopReason::Cancelled)
        } else if self.deadline.is_some_and(|limit| Instant::now() >= limit) {
            Some(StopReason::TimeLimit)
        } else {
            None
        };
        self.reason.set(reason);
        reason.is_some()
    }
}

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
    control: &Control<'_>,
    mut file: impl FnMut(&Location, Stamp),
) -> io::Result<Walk> {
    windows::check_directory(root)?;
    let mut output = Walk::default();
    let mut pending = vec![root.clone()];
    'walk: while let Some(directory) = pending.pop() {
        if control.stopped() {
            break;
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
            if control.stopped() {
                break 'walk;
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
    pub eligible_files: u64,
    pub blocked_by: Option<String>,
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
    pub status: &'static str,
    pub stop_reason: Option<StopReason>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub mode: &'static str,
    pub volumes: Vec<Volume>,
    pub rules: Vec<RuleReport>,
    pub analysis: Vec<DirectoryReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discovery: Option<discovery::Report>,
    pub elapsed_ms: u64,
    pub cancelled: bool,
    pub stop_reason: Option<StopReason>,
    pub scan_id: Option<String>,
    pub scan_file: Option<PathBuf>,
    pub state_error: Option<String>,
    #[serde(skip)]
    pub candidates: Vec<Candidate>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Candidate {
    pub rule_id: String,
    pub location: Location,
    pub stamp: Stamp,
}

#[derive(Debug)]
pub struct Options {
    pub volumes: Vec<PathBuf>,
    pub path: Option<PathBuf>,
    pub deep: bool,
    pub discover: bool,
    pub top: usize,
    pub max_seconds: Option<u64>,
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
        eligible_files: 0,
        blocked_by: None,
        status: "not_found",
        reason: None,
    }
}

pub fn run(options: Options, cancel: &AtomicBool) -> Result<Report> {
    let started = Instant::now();
    ensure!(options.top <= 100, "--top must be at most 100");
    let deadline = options
        .max_seconds
        .map(|seconds| {
            started
                .checked_add(Duration::from_secs(seconds))
                .context("scan deadline is too far away")
        })
        .transpose()?;
    let control = Control {
        cancel,
        deadline,
        reason: Cell::new(None),
    };
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
        discovery: options.discover.then(discovery::Report::default),
        elapsed_ms: 0,
        cancelled: false,
        stop_reason: None,
        scan_id: None,
        scan_file: None,
        state_error: None,
        candidates: Vec::new(),
    };
    if options.progress {
        eprintln!("Volumes discovered: {}", result.volumes.len());
    }
    let now = rules::windows_time_now();
    let mut covered = Vec::new();
    if focus.is_none() {
        if let Some(discovery) = &mut result.discovery {
            discover_recycle_bins(discovery, &result.volumes, &control);
        }
        // Prevent a Documents location and the same volume-root hint from counting twice.
        let mut seen = HashSet::new();
        for rule in rules::load()? {
            if control.stopped() {
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
                if control.stopped() {
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
                let verified_root = windows::check_directory(&root);
                if verified_root.is_ok() {
                    row.blocked_by = rule.block_reason(&root, &result.volumes);
                }
                let may_plan =
                    verified_root.is_ok() && rule.action.can_delete() && row.blocked_by.is_none();
                let checked = verified_root.and_then(|()| {
                    walk(
                        &root,
                        rule.action.recursive(),
                        0,
                        &control,
                        |file, stamp| {
                            if rule.matches(&file.relative, stamp, now) {
                                row.matching_files += 1;
                                row.matching_logical_bytes =
                                    row.matching_logical_bytes.saturating_add(stamp.size);
                                if may_plan {
                                    row.eligible_files += 1;
                                    result.candidates.push(Candidate {
                                        rule_id: rule.id.clone(),
                                        location: file.clone(),
                                        stamp,
                                    });
                                }
                            }
                        },
                    )
                });
                match checked {
                    Ok(output) => {
                        seen.insert(root.clone());
                        row.status = if control.reason.get().is_some()
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
        if let Some(discovery) = &mut result.discovery {
            for row in &result.rules {
                add_rule_finding(discovery, row);
            }
            let temp_candidates = result
                .candidates
                .iter()
                .filter(|item| item.rule_id == "windows.old_temp")
                .map(|item| (&item.location, item.stamp))
                .collect();
            covered = discover_known(
                discovery,
                &result.volumes,
                &control,
                now,
                options.progress,
                &temp_candidates,
                &result.rules,
            );
            discovery.quick_elapsed_ms = Some(started.elapsed().as_millis() as u64);
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
            if control.stopped() {
                break;
            }
            let report = analyze(
                &root,
                &result.volumes,
                options.top,
                &control,
                options.progress,
                result
                    .discovery
                    .as_mut()
                    .map(|report| (report, covered.as_slice(), now)),
            );
            result.analysis.push(report);
        }
    }
    if let Some(discovery) = &mut result.discovery {
        discovery.sort();
    }
    result.cancelled = cancel.load(Ordering::Relaxed);
    result.stop_reason = control.reason.get();
    result.elapsed_ms = started.elapsed().as_millis() as u64;
    Ok(result)
}

fn discover_recycle_bins(report: &mut discovery::Report, volumes: &[Volume], control: &Control) {
    for volume in volumes {
        if control.stopped() {
            break;
        }
        let mount = &volume.mounts[0];
        let mut check = discovery::Check {
            label: "回收站",
            path: mount.clone(),
            status: "checked",
            detail: None,
        };
        if mount.components().count() != 2 {
            check.status = "unavailable";
            check.detail = Some("该卷没有盘符。".into());
        } else {
            match windows::recycle_bin(mount) {
                Ok((0, 0)) => {}
                Ok((files, bytes)) => report.findings.push(Finding::new(
                    Kind::RecycleBin,
                    volume.id.clone(),
                    mount.clone(),
                    files,
                    bytes,
                )),
                Err(error) => {
                    check.status = "unavailable";
                    check.detail = Some(error.to_string());
                }
            }
        }
        report.checks.push(check);
    }
}

fn add_rule_finding(report: &mut discovery::Report, row: &RuleReport) {
    let (Some(volume), Some(path), Some(totals)) = (&row.volume_id, &row.root, &row.totals) else {
        return;
    };
    let mut finding = match row.rule_id.as_str() {
        "chrome.http_cache" | "edge.http_cache" if totals.files > 0 => Finding::new(
            Kind::BrowserCache,
            volume.clone(),
            path.clone(),
            totals.files,
            totals.logical_bytes,
        ),
        "windows.downloads_installers" if row.matching_files > 0 => Finding::new(
            Kind::DownloadInstaller,
            volume.clone(),
            path.clone(),
            row.matching_files,
            row.matching_logical_bytes,
        ),
        _ => return,
    };
    finding.label = row.label.clone();
    report.findings.push(finding);
}

fn discover_known(
    report: &mut discovery::Report,
    volumes: &[Volume],
    control: &Control,
    now: u64,
    progress: bool,
    temp_candidates: &HashMap<&Location, Stamp>,
    rules: &[RuleReport],
) -> Vec<Location> {
    let local = match Folder::LocalAppData.known() {
        Ok(local) => local,
        Err(error) => {
            report.checks.push(discovery::Check {
                label: "当前用户的应用数据",
                path: PathBuf::new(),
                status: "unavailable",
                detail: Some(error.to_string()),
            });
            return Vec::new();
        }
    };
    let mut areas: Vec<_> = [
        ("应用崩溃转储", "CrashDumps", Some(Kind::CrashDumps)),
        (
            "Windows 缩略图缓存",
            "Microsoft/Windows/Explorer",
            Some(Kind::Thumbnails),
        ),
        (
            "npm 软件包下载缓存",
            "npm-cache/_cacache",
            Some(Kind::NpmCache),
        ),
        (
            "Edge 组件更新缓存",
            "Microsoft/Edge/User Data/component_crx_cache",
            Some(Kind::BrowserComponentCache),
        ),
        ("pip HTTP 缓存", "pip/Cache/http-v2", Some(Kind::PipCache)),
        ("pip 旧版 HTTP 缓存", "pip/Cache/http", Some(Kind::PipCache)),
        ("pip wheel 缓存", "pip/Cache/wheels", Some(Kind::PipCache)),
        ("用户临时目录", "Temp", None),
    ]
    .into_iter()
    .map(|(label, path, kind)| (label, local.join(path), kind))
    .collect();
    areas.extend(rules.iter().filter_map(|row| {
        let label = match row.rule_id.as_str() {
            "chrome.http_cache" => "Chrome 网页代码缓存",
            "edge.http_cache" => "Edge 网页代码缓存",
            _ => return None,
        };
        let profile = row.root.as_deref()?.parent()?.parent()?;
        Some((
            label,
            profile.join("Code Cache"),
            Some(Kind::BrowserCodeCache),
        ))
    }));
    let mut covered = Vec::new();
    for (label, path, kind) in areas {
        if control.stopped() {
            break;
        }
        let Some(root) = windows::locate(&path, volumes) else {
            continue;
        };
        if progress {
            eprintln!("Finding junk in {}", path.display());
        }
        let mut groups = Groups::default();
        let recursive = !matches!(kind, Some(Kind::Thumbnails | Kind::BrowserComponentCache));
        let checked = walk(&root, recursive, 0, control, |file, stamp| {
            let (kind, group) = match kind {
                Some(Kind::CrashDumps)
                    if !discovery::extension(&file.relative).eq_ignore_ascii_case("dmp") =>
                {
                    return;
                }
                Some(Kind::Thumbnails)
                    if !discovery::extension(&file.relative).eq_ignore_ascii_case("db")
                        || !file
                            .relative
                            .file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| name.starts_with("thumbcache_")) =>
                {
                    return;
                }
                Some(Kind::BrowserComponentCache)
                    if !file
                        .relative
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| {
                            name == "metadata.json"
                                || (name.len() == 64
                                    && name.bytes().all(|byte| byte.is_ascii_hexdigit()))
                        }) =>
                {
                    return;
                }
                Some(kind) => (kind, root.relative.as_path()),
                None => {
                    let Some(kind) = discovery::clue(file, stamp, now, true) else {
                        return;
                    };
                    (kind, file.relative.parent().unwrap_or(&root.relative))
                }
            };
            let level = if kind == Kind::OldTemp
                && temp_candidates.get(file) == Some(&stamp)
                && CheckedFile::open(file, false).is_ok_and(|item| item.stamp == stamp)
            {
                ReviewLevel::Confirmed
            } else {
                kind.review_level()
            };
            groups.add(kind, group, file, stamp, level);
        });
        let (status, detail) = match checked {
            Ok(output) => {
                groups.append_to(
                    report,
                    volumes,
                    matches!(
                        kind,
                        Some(Kind::BrowserCodeCache | Kind::BrowserComponentCache)
                    )
                    .then_some(label),
                );
                if control.reason.get().is_some() {
                    ("partial", Some("扫描提前结束。".into()))
                } else {
                    if recursive {
                        covered.push(root);
                    }
                    if output.totals.skipped.is_empty() {
                        ("checked", None)
                    } else {
                        let count = output.totals.skipped.values().sum::<u64>();
                        ("partial", Some(format!("跳过 {count} 项。")))
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => ("not_found", None),
            Err(error) => ("unavailable", Some(error.to_string())),
        };
        report.checks.push(discovery::Check {
            label,
            path,
            status,
            detail,
        });
    }
    covered
}

fn analyze(
    root: &Location,
    volumes: &[Volume],
    top: usize,
    control: &Control<'_>,
    progress: bool,
    discovery: Option<(&mut discovery::Report, &[Location], u64)>,
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
        status: "unavailable",
        stop_reason: None,
        error: None,
    };
    let mut groups = Groups::default();
    match walk(root, true, top, control, |file, stamp| {
        if let Some((_, covered, now)) = &discovery
            && let Some(kind) = discovery::clue(file, stamp, *now, false)
            && !file.relative.starts_with("$Recycle.Bin")
            && !covered
                .iter()
                .any(|area| area.volume == file.volume && file.relative.starts_with(&area.relative))
            && let Some(parent) = file.relative.parent()
        {
            groups.add(kind, parent, file, stamp, kind.review_level());
        }
    }) {
        Ok(output) => {
            report.stop_reason = control.reason.get();
            report.status = if report.stop_reason.is_some() || !output.totals.skipped.is_empty() {
                "partial"
            } else {
                "complete"
            };
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
    if let Some((discovery, _, _)) = discovery {
        groups.append_to(discovery, volumes, None);
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
                discover: false,
                top: 1,
                max_seconds: None,
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

    #[test]
    fn discovery_finds_temporary_artifacts_without_counting_other_files() {
        let fixture = tempfile::tempdir().unwrap();
        let temp = fixture.path().join("临时文件");
        fs::create_dir(&temp).unwrap();
        fs::create_dir(temp.join("package")).unwrap();
        fs::write(temp.join("package.zip"), [0; 20]).unwrap();
        fs::write(temp.join("backup.zip"), [0; 100]).unwrap();
        fs::write(temp.join("ExampleSetup.exe"), [0; 30]).unwrap();
        fs::write(temp.join("Example.exe.123.dmp"), [0; 40]).unwrap();
        fs::write(temp.join("Example.exe.124.dmp"), b"MDMP signature sample").unwrap();
        fs::write(temp.join("shader.bin"), [0; 1000]).unwrap();

        let report = run(
            Options {
                volumes: Vec::new(),
                path: Some(temp),
                deep: false,
                discover: true,
                top: 0,
                max_seconds: None,
                progress: false,
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        let discoveries = report.discovery.unwrap();
        let found: Vec<_> = discoveries
            .findings
            .iter()
            .map(|row| (row.kind, row.bytes))
            .collect();
        assert_eq!(
            found,
            [
                (Kind::Minidump, 21),
                (Kind::TempInstaller, 30),
                (Kind::ArchiveWithDirectory, 20),
                (Kind::DumpClue, 40)
            ]
        );
        assert!(report.candidates.is_empty());
    }

    #[test]
    fn an_expired_analysis_reports_partial_coverage() {
        let fixture = tempfile::tempdir().unwrap();
        fs::write(fixture.path().join("fixture.bin"), [1, 2]).unwrap();
        let volumes = windows::volumes().unwrap();
        let root = windows::locate(fixture.path(), &volumes).unwrap();
        let control = Control {
            cancel: &AtomicBool::new(false),
            deadline: Some(Instant::now()),
            reason: Cell::new(None),
        };
        let report = analyze(&root, &volumes, 3, &control, false, None);
        assert_eq!(report.status, "partial");
        assert_eq!(report.stop_reason, Some(StopReason::TimeLimit));
        assert_eq!(report.totals.unwrap().files, 0);
    }
}
