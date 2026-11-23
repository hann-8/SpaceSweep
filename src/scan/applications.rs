use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail, ensure};

use super::{Control, walk};
use crate::discovery::{self, Check, Finding, Kind, Report};
use crate::windows::{self, Folder, Location, Stamp, Volume};

const PROBES: &str = include_str!("../../rules/application-probes.tsv");
const APPLICATION_DEPTH: u8 = 12;
const INSTALLATION_DEPTH: u8 = 8;
const ANCHORS: [(&str, Folder); 6] = [
    ("local", Folder::LocalAppData),
    ("roaming", Folder::RoamingAppData),
    ("program_data", Folder::ProgramData),
    ("program_files", Folder::ProgramFiles),
    ("program_files_x86", Folder::ProgramFilesX86),
    ("windows", Folder::Windows),
];

struct Probe<'a> {
    anchor: &'a str,
    kind: Kind,
    recursive: bool,
    label: &'a str,
    path: &'a Path,
}

impl<'a> Probe<'a> {
    fn parse(row: &'a str) -> Result<Self> {
        let fields: Vec<_> = row.split('\t').collect();
        let &[anchor, purpose, depth, label, path] = fields.as_slice() else {
            bail!("invalid application probe row");
        };
        let kind = match purpose {
            "cache" => Kind::ApplicationCacheClue,
            "logs" => Kind::ApplicationLogClue,
            "crash_reports" => Kind::ApplicationCrashClue,
            _ => bail!("unknown application probe purpose"),
        };
        let recursive = match depth {
            "top" => false,
            "tree" => true,
            _ => bail!("unknown application probe depth"),
        };
        let path = Path::new(path);
        ensure!(
            path.components().count() >= 2 && windows::valid_relative(path),
            "invalid application probe path"
        );
        Ok(Self {
            anchor,
            kind,
            recursive,
            label,
            path,
        })
    }
}

#[derive(Default)]
struct Coverage {
    checked: usize,
    found: usize,
    skipped: u64,
}

fn path_key(location: &Location) -> PathBuf {
    let mut path = PathBuf::from(&location.volume);
    path.extend(
        location
            .relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().to_lowercase()),
    );
    path
}

fn overlaps(path: &Path, accounted: &[PathBuf]) -> bool {
    accounted
        .iter()
        .any(|other| path.starts_with(other) || other.starts_with(path))
}

fn check(
    label: &'static str,
    path: PathBuf,
    scope: &str,
    stats: Coverage,
    control: &Control<'_>,
) -> Check {
    Check {
        label,
        path,
        status: if control.stopped() || stats.skipped != 0 {
            "partial"
        } else {
            "checked"
        },
        detail: Some(format!(
            "{scope} {} 处，发现 {} 处，跳过 {} 项。",
            stats.checked, stats.found, stats.skipped
        )),
    }
}

fn find_layouts(
    base: &Location,
    max_depth: u8,
    volumes: &[Volume],
    control: &Control<'_>,
    report: &mut Report,
    accounted: &mut Vec<PathBuf>,
    covered: &mut Vec<Location>,
) -> Check {
    let path = windows::view(base, volumes);
    if let Err(error) = windows::check_directory(base) {
        return Check {
            label: "应用缓存布局",
            path,
            status: if error.kind() == io::ErrorKind::NotFound {
                "not_found"
            } else {
                "unavailable"
            },
            detail: Some(error.to_string()),
        };
    }
    let mut stats = Coverage::default();
    let mut pending = vec![(base.clone(), 0)];
    'search: while let Some((root, depth)) = pending.pop() {
        if control.stopped() {
            break;
        }
        stats.checked += 1;
        if let Some(kind) = discovery::cache_layout(&root) {
            let key = path_key(&root);
            if !overlaps(&key, accounted) {
                match walk(&root, true, 0, control, None, |_, _, _| {}) {
                    Ok(output) => {
                        stats.skipped += output.totals.skipped.values().sum::<u64>();
                        if output.totals.files != 0 {
                            report.findings.push(Finding::new(
                                kind,
                                root.volume.clone(),
                                windows::view(&root, volumes),
                                output.totals.files,
                                output.totals.logical_bytes,
                            ));
                            stats.found += 1;
                            accounted.push(key);
                            covered.push(root);
                        }
                    }
                    Err(_) => stats.skipped += 1,
                }
            }
            continue;
        }
        if depth == max_depth {
            continue;
        }
        let entries = match fs::read_dir(root.native()) {
            Ok(entries) => entries,
            Err(_) => {
                stats.skipped += 1;
                continue;
            }
        };
        for entry in entries {
            if control.stopped() {
                break 'search;
            }
            let Ok(entry) = entry else {
                stats.skipped += 1;
                continue;
            };
            match entry.file_type() {
                Ok(file_type) if !file_type.is_dir() => continue,
                Ok(_) => {}
                Err(_) => {
                    stats.skipped += 1;
                    continue;
                }
            }
            let name = entry.file_name();
            if [
                "Temp",
                "node_modules",
                ".git",
                "CacheStorage",
                "IndexedDB",
                "Local Storage",
            ]
            .iter()
            .any(|excluded| name.eq_ignore_ascii_case(excluded))
            {
                continue;
            }
            match entry.metadata() {
                Ok(meta)
                    if meta.is_dir()
                        && Stamp::from(&meta).attributes & windows::SKIP_ATTRIBUTES == 0 =>
                {
                    pending.push((root.child(name), depth + 1));
                }
                _ => stats.skipped += 1,
            }
        }
    }
    check(
        "应用缓存布局",
        path,
        &format!("查看前 {max_depth} 级目录"),
        stats,
        control,
    )
}

pub(super) fn discover(
    report: &mut Report,
    volumes: &[Volume],
    control: &Control<'_>,
    progress: bool,
    covered: &mut Vec<Location>,
) -> Result<()> {
    let bases: BTreeMap<_, _> = ANCHORS
        .into_iter()
        .filter_map(|(key, folder)| Some((key, windows::locate(&folder.known().ok()?, volumes)?)))
        .collect();
    let mut accounted: Vec<_> = covered.iter().map(path_key).collect();
    accounted.extend(report.findings.iter().filter_map(|finding| {
        (finding.kind == Kind::BrowserCache)
            .then(|| windows::locate(&finding.path, volumes))
            .flatten()
            .as_ref()
            .map(path_key)
    }));
    let data_anchors = ["local", "roaming", "program_data"];
    for key in data_anchors {
        if control.stopped() {
            break;
        }
        if let Some(base) = bases.get(key) {
            if progress {
                eprintln!(
                    "Finding application cache layouts in {}",
                    windows::view(base, volumes).display()
                );
            }
            let result = find_layouts(
                base,
                APPLICATION_DEPTH,
                volumes,
                control,
                report,
                &mut accounted,
                covered,
            );
            report.checks.push(result);
        }
    }

    if !control.stopped() {
        let general_roots: Vec<_> = data_anchors
            .into_iter()
            .chain(["windows"])
            .filter_map(|anchor| bases.get(anchor).map(path_key))
            .collect();
        let mut seen = HashSet::new();
        for path in windows::installed_app_locations()? {
            if control.stopped() {
                break;
            }
            let Some(root) = windows::locate(&path, volumes) else {
                continue;
            };
            if root.relative.as_os_str().is_empty() {
                continue;
            }
            let key = path_key(&root);
            if general_roots
                .iter()
                .any(|general| key.starts_with(general) || general.starts_with(&key))
                || root.relative.components().count() == 1
                    && root.relative.file_name().is_some_and(|name| {
                        [
                            "Program Files",
                            "Program Files (x86)",
                            "Windows",
                            "Users",
                            "ProgramData",
                        ]
                        .iter()
                        .any(|general| name.eq_ignore_ascii_case(general))
                    })
                || !seen.insert(key)
            {
                continue;
            }
            let result = find_layouts(
                &root,
                INSTALLATION_DEPTH,
                volumes,
                control,
                report,
                &mut accounted,
                covered,
            );
            report.checks.push(result);
        }
    }

    let mut coverage = BTreeMap::<_, Coverage>::new();
    for row in PROBES
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        if control.stopped() {
            break;
        }
        let probe = Probe::parse(row)?;
        let Some(base) = bases.get(probe.anchor) else {
            continue;
        };
        let stats = coverage.entry(probe.anchor).or_default();
        let root = base.child(probe.path);
        stats.checked += 1;
        let key = path_key(&root);
        if overlaps(&key, &accounted) {
            continue;
        }
        let output = match walk(&root, probe.recursive, 0, control, None, |_, _, _| {}) {
            Ok(output) => output,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => {
                stats.skipped += 1;
                continue;
            }
        };
        stats.skipped += output.totals.skipped.values().sum::<u64>();
        if output.totals.files == 0 {
            continue;
        }
        let mut finding = Finding::new(
            probe.kind,
            root.volume.clone(),
            windows::view(&root, volumes),
            output.totals.files,
            output.totals.logical_bytes,
        );
        finding.label = format!("{} · {}", probe.label, finding.label);
        report.findings.push(finding);
        stats.found += 1;
        accounted.push(key);
        if probe.recursive {
            covered.push(root);
        }
    }
    for (key, stats) in coverage {
        report.checks.push(check(
            "Winapp2 应用路径",
            windows::view(&bases[key], volumes),
            "规则位置",
            stats,
            control,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn quick_search_uses_cache_markers_and_keeps_one_finding_per_directory() {
        let temp = tempfile::tempdir().unwrap();
        let real_cache = temp.path().join("Unlisted App/Cache");
        let unrelated = temp.path().join("Other App/Cache");
        fs::create_dir_all(&real_cache).unwrap();
        fs::create_dir_all(&unrelated).unwrap();
        fs::write(real_cache.join("index"), b"index").unwrap();
        fs::write(real_cache.join("data_0"), b"cache").unwrap();
        fs::write(unrelated.join("image.png"), b"picture").unwrap();

        let volumes = windows::volumes().unwrap();
        let base = windows::locate(temp.path(), &volumes).unwrap();
        let cancel = AtomicBool::new(false);
        let control = Control {
            cancel: &cancel,
            deadline: None,
            reason: Cell::new(None),
        };
        let mut report = Report::default();
        let mut accounted = Vec::new();
        let mut covered = Vec::new();
        for _ in 0..2 {
            find_layouts(
                &base,
                2,
                &volumes,
                &control,
                &mut report,
                &mut accounted,
                &mut covered,
            );
        }
        assert_eq!(report.findings.len(), 1);
        let finding = &report.findings[0];
        assert!(finding.path.ends_with("Unlisted App/Cache"));
        assert_eq!(finding.kind, Kind::BrowserCache);
        assert_eq!(finding.bytes, 10);
        assert!(finding.preview.is_none());
    }
}
