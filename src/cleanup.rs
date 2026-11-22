use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::checked_file::{CheckedFile, Identity, is_busy};
use crate::review::{Node, Review};
use crate::rules::{self, Action, RULES_TEXT, Rule};
use crate::scan::{Candidate, Report};
use crate::store::{Store, digest};
use crate::windows::{self, Location, Volume};

#[derive(Deserialize, Serialize)]
struct Snapshot {
    rules: String,
    candidates: Vec<Candidate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    review_groups: Vec<ScopedReview>,
}

#[derive(Deserialize, Serialize)]
struct ScopedReview {
    review: Review,
    target: Option<ReviewTarget>,
}

#[derive(Deserialize, Serialize)]
struct ReviewTarget {
    rule_id: String,
    root: Location,
}

impl ReviewTarget {
    fn contains(&self, candidate: &Candidate) -> bool {
        self.rule_id == candidate.rule_id
            && self.root.volume == candidate.location.volume
            && windows::relative_to(&candidate.location.relative, &self.root.relative)
                .is_some_and(|relative| relative.components().count() == 1)
    }
}

#[derive(Deserialize, Serialize)]
struct Plan {
    rules: String,
    items: Vec<PlanItem>,
}

#[derive(Deserialize, Serialize)]
struct PlanItem {
    candidate: Candidate,
    identity: Identity,
    path: PathBuf,
    allocated_bytes: u64,
}

struct ReadyRule {
    rule: Rule,
    roots: Vec<Location>,
}

fn ready_rules(volumes: &[Volume]) -> Result<HashMap<String, ReadyRule>> {
    let mut ready = HashMap::new();
    for rule in rules::load()?.into_iter().filter(|r| r.action.can_delete()) {
        let roots = rule
            .locations(volumes, &HashSet::new())
            .unwrap_or_default()
            .into_iter()
            .filter(|root| {
                windows::check_directory(root).is_ok() && rule.block_reason(root, volumes).is_none()
            })
            .collect();
        ready.insert(rule.id.clone(), ReadyRule { rule, roots });
    }
    Ok(ready)
}

fn eligible<'a>(
    candidate: &Candidate,
    ready: &'a HashMap<String, ReadyRule>,
    now: u64,
) -> Option<&'a Rule> {
    let ready = ready.get(&candidate.rule_id)?;
    let location = &candidate.location;
    let in_approved_root = ready.roots.iter().any(|root| {
        root.volume == location.volume
            && windows::relative_to(&location.relative, &root.relative)
                .is_some_and(|relative| relative.components().count() == 1)
    });
    (in_approved_root && ready.rule.matches(&location.relative, candidate.stamp, now))
        .then_some(&ready.rule)
}

pub fn save_scan(store: &Store, report: &mut Report) -> Result<()> {
    let review_groups = report
        .discovery
        .iter()
        .flat_map(|discovery| &discovery.findings)
        .filter_map(|finding| {
            let review = finding.review.clone()?;
            let target = finding.preview.as_ref().and_then(|preview| {
                let root = windows::locate(&finding.path, &report.volumes)?;
                Some(ReviewTarget {
                    rule_id: preview.rule_id.clone(),
                    root,
                })
            });
            Some(ScopedReview { review, target })
        })
        .collect();
    let snapshot = Snapshot {
        rules: digest(RULES_TEXT.as_bytes()),
        candidates: std::mem::take(&mut report.candidates),
        review_groups,
    };
    let (id, path) = store.write("scan", &snapshot)?;
    report.scan_id = Some(id);
    report.scan_file = Some(path);
    Ok(())
}

#[derive(Debug, Default, Serialize)]
struct Group {
    rule_id: String,
    label: String,
    volume: PathBuf,
    files: u64,
    logical_bytes: u64,
    allocated_bytes: u64,
    note: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    review: Option<Review>,
}

#[derive(Debug, Serialize)]
pub struct Preview {
    plan_id: String,
    plan_file: PathBuf,
    action: &'static str,
    groups: Vec<Group>,
    not_included: BTreeMap<String, u64>,
    files: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    include_groups: Vec<Node>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    exclude_groups: Vec<Node>,
}

pub struct Selections {
    pub scans: Vec<String>,
    pub include_group: Vec<String>,
    pub exclude_group: Vec<String>,
    pub exclude_rule: Vec<String>,
    pub exclude_volume: Vec<PathBuf>,
    pub exclude_file: Vec<PathBuf>,
}

fn resolve_groups(ids: &[String], groups: &[ScopedReview]) -> Result<Vec<Node>> {
    ids.iter()
        .map(|id| {
            groups
                .iter()
                .find_map(|group| group.review.selection(id))
                .with_context(|| format!("unknown review group {id} in the supplied scans"))
        })
        .collect()
}

fn review_exclusion(
    review: Option<&Review>,
    include: &[Node],
    exclude: &[Node],
) -> Option<&'static str> {
    let matches = |groups: &[Node]| {
        review.is_some_and(|review| groups.iter().any(|group| review.contains(&group.id)))
    };
    if !include.is_empty() && !matches(include) {
        Some("outside_review_selection")
    } else if matches(exclude) {
        Some("excluded")
    } else {
        None
    }
}

pub fn plan(store: &Store, selections: Selections, cancel: &AtomicBool) -> Result<Preview> {
    let volumes = windows::volumes()?;
    let ready = ready_rules(&volumes)?;
    build_preview(store, selections, &volumes, &ready, cancel)
}

fn build_preview(
    store: &Store,
    selections: Selections,
    volumes: &[Volume],
    ready: &HashMap<String, ReadyRule>,
    cancel: &AtomicBool,
) -> Result<Preview> {
    let rule_digest = digest(RULES_TEXT.as_bytes());
    let mut candidates = Vec::new();
    let mut review_groups = Vec::new();
    for scan in selections.scans {
        let saved: Snapshot = store.read("scan", &scan)?;
        ensure!(saved.rules == rule_digest, "rules changed; run a new scan");
        candidates.extend(saved.candidates);
        review_groups.extend(saved.review_groups);
    }
    let include_groups = resolve_groups(&selections.include_group, &review_groups)?;
    let exclude_groups = resolve_groups(&selections.exclude_group, &review_groups)?;
    let targets: Vec<_> = review_groups
        .iter()
        .filter_map(|group| Some((group.target.as_ref()?, &group.review)))
        .collect();
    let review_for = |candidate: &Candidate| {
        targets
            .iter()
            .find_map(|(target, review)| target.contains(candidate).then_some(*review))
    };
    let mut excluded_volumes = HashSet::new();
    for input in selections.exclude_volume {
        let input = windows::scope_path(&input)?;
        let location = windows::locate(&input, volumes).context("unknown excluded volume")?;
        ensure!(
            location.relative.as_os_str().is_empty(),
            "--exclude-volume requires a mount point"
        );
        excluded_volumes.insert(location.volume);
    }
    let excluded_files = selections
        .exclude_file
        .into_iter()
        .map(|p| {
            let input = windows::scope_path(&p)?;
            windows::locate(&input, volumes).context("excluded file is not on a local volume")
        })
        .collect::<Result<Vec<_>>>()?;
    let mut not_included = BTreeMap::new();
    let mut items = Vec::new();
    let mut identities = HashSet::new();
    let now = rules::windows_time_now();
    for candidate in candidates {
        if cancel.load(Ordering::Relaxed) {
            anyhow::bail!("plan cancelled");
        }
        let location = &candidate.location;
        if let Some(reason) =
            review_exclusion(review_for(&candidate), &include_groups, &exclude_groups)
        {
            count(&mut not_included, reason);
            continue;
        }
        if selections.exclude_rule.contains(&candidate.rule_id)
            || excluded_volumes.contains(&location.volume)
            || excluded_files.iter().any(|excluded| {
                excluded.volume == location.volume
                    && windows::relative_to(&location.relative, &excluded.relative)
                        .is_some_and(|suffix| suffix.as_os_str().is_empty())
            })
        {
            count(&mut not_included, "excluded");
            continue;
        }
        let item = match prepare_item(candidate, ready, volumes, now) {
            Ok(item) => item,
            Err(reason) => {
                count(&mut not_included, reason);
                continue;
            }
        };
        if !identities.insert((item.identity.serial, item.identity.file_id)) {
            count(&mut not_included, "duplicate");
            continue;
        }
        items.push(item);
    }
    items.sort_by(|a, b| {
        let sort_key = |item: &PlanItem| {
            item.candidate
                .location
                .relative
                .file_name()
                .is_some_and(|name| name == "index")
        };
        sort_key(a)
            .cmp(&sort_key(b))
            .then_with(|| a.path.cmp(&b.path))
    });
    let mut groups = BTreeMap::new();
    for item in &items {
        let candidate = &item.candidate;
        let volume = volumes
            .iter()
            .find(|v| v.id == candidate.location.volume)
            .unwrap();
        let rule = &ready[&candidate.rule_id].rule;
        let review = review_for(candidate);
        let group = groups
            .entry((
                &candidate.rule_id,
                &candidate.location.volume,
                review.map(|review| &review.purpose.id),
            ))
            .or_insert_with(|| Group {
                rule_id: rule.id.clone(),
                label: rule.label.clone(),
                note: rule.note.clone(),
                volume: volume.mounts[0].clone(),
                review: review.cloned(),
                ..Default::default()
            });
        group.files += 1;
        group.logical_bytes = group.logical_bytes.saturating_add(candidate.stamp.size);
        group.allocated_bytes = group.allocated_bytes.saturating_add(item.allocated_bytes);
    }
    let files = items.len();
    let groups = groups.into_values().collect();
    let (plan_id, plan_file) = store.write(
        "plan",
        &Plan {
            rules: rule_digest,
            items,
        },
    )?;
    Ok(Preview {
        plan_id,
        plan_file,
        action: "permanent_delete",
        groups,
        not_included,
        files,
        include_groups,
        exclude_groups,
    })
}

fn count(totals: &mut BTreeMap<String, u64>, label: &str) {
    *totals.entry(label.to_owned()).or_default() += 1;
}

fn open_reason(error: &io::Error) -> &'static str {
    if is_busy(error) {
        return "in_use";
    }
    match error.kind() {
        io::ErrorKind::NotFound => "missing",
        io::ErrorKind::PermissionDenied => "access_denied",
        io::ErrorKind::Unsupported => "protected_path_or_file",
        _ => "verification_failed",
    }
}

fn prepare_item(
    candidate: Candidate,
    ready: &HashMap<String, ReadyRule>,
    volumes: &[Volume],
    now: u64,
) -> std::result::Result<PlanItem, &'static str> {
    if eligible(&candidate, ready, now).is_none() {
        return Err("rule_not_ready");
    }
    let handle = CheckedFile::open(&candidate.location, false).map_err(|e| open_reason(&e))?;
    if handle.stamp != candidate.stamp {
        return Err("changed_since_scan");
    }
    Ok(PlanItem {
        path: windows::view(&candidate.location, volumes),
        candidate,
        identity: handle.identity.clone(),
        allocated_bytes: handle.allocated_bytes,
    })
}

#[derive(Debug, Serialize)]
struct Outcome {
    status: &'static str,
    reason: &'static str,
    os_error: Option<String>,
}

impl Outcome {
    fn skip(reason: &'static str) -> Self {
        Self {
            status: "skipped",
            reason,
            os_error: None,
        }
    }
}

fn perform(item: &PlanItem, rule: &Rule) -> Outcome {
    let location = &item.candidate.location;
    let handle = match CheckedFile::open(location, true) {
        Ok(handle) => handle,
        Err(error) => {
            return Outcome {
                os_error: Some(error.to_string()),
                ..Outcome::skip(open_reason(&error))
            };
        }
    };
    if handle.identity != item.identity || handle.stamp != item.candidate.stamp {
        return Outcome::skip("changed_since_preview");
    }
    if rule.action == Action::ChromeHttpCache
        && !matches!(windows::process_running("chrome.exe"), Ok(false))
    {
        return Outcome::skip("chrome_running_or_state_unavailable");
    }
    match handle.request_deletion() {
        Ok(()) => {
            // A shared reader may keep a Win32 file pending deletion after our handle closes.
            let removed = matches!(fs::symlink_metadata(location.native()), Err(error) if error.kind() == io::ErrorKind::NotFound);
            if removed {
                Outcome {
                    status: "deleted",
                    reason: "deleted",
                    os_error: None,
                }
            } else {
                Outcome {
                    status: "submitted",
                    reason: "deletion_pending_or_path_reused",
                    os_error: None,
                }
            }
        }
        Err(error) => Outcome {
            status: "failed",
            reason: "delete_failed",
            os_error: Some(error.to_string()),
        },
    }
}

#[derive(Debug, Serialize)]
struct VolumeChange {
    volume_id: String,
    mount: Option<PathBuf>,
    available_before: Option<u64>,
    available_after: Option<u64>,
    available_change_bytes: Option<i128>,
}

#[derive(Debug, Serialize)]
pub struct ApplyReport {
    plan_id: String,
    deleted_files: u64,
    deleted_logical_bytes: u64,
    submitted_files: u64,
    skipped_files: u64,
    failed_files: u64,
    unattempted_files: usize,
    cancelled: bool,
    outcomes: BTreeMap<String, u64>,
    volumes: Vec<VolumeChange>,
    receipt_file: PathBuf,
    receipt_error: Option<String>,
}

pub fn apply(store: &Store, plan_id: String, cancel: &AtomicBool) -> Result<ApplyReport> {
    ensure!(
        unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() } == 0,
        "apply requires a standard user session"
    );
    let _lock = store.acquire_apply_lock(&plan_id)?;
    let plan: Plan = store.read("plan", &plan_id)?;
    ensure!(
        plan.rules == digest(RULES_TEXT.as_bytes()),
        "rules changed; create a new preview"
    );
    let volumes = windows::volumes()?;
    let ready = ready_rules(&volumes)?;
    run_plan(store, plan_id, plan, &volumes, &ready, cancel)
}

fn available(volume: &str, volumes: &[Volume]) -> Option<u64> {
    let known = volumes.iter().find(|v| v.id == volume)?;
    windows::space(&known.id)
        .ok()
        .map(|space| space.available_bytes)
}

fn run_plan(
    store: &Store,
    plan_id: String,
    plan: Plan,
    volumes: &[Volume],
    ready: &HashMap<String, ReadyRule>,
    cancel: &AtomicBool,
) -> Result<ApplyReport> {
    let (mut receipt, receipt_file) = store.receipt()?;
    let mut affected = BTreeMap::new();
    for item in &plan.items {
        let id = &item.candidate.location.volume;
        affected.entry(id.clone()).or_insert_with(|| VolumeChange {
            mount: volumes
                .iter()
                .find(|volume| &volume.id == id)
                .map(|volume| volume.mounts[0].clone()),
            volume_id: id.clone(),
            available_before: available(id, volumes),
            available_after: None,
            available_change_bytes: None,
        });
    }
    let mut report = ApplyReport {
        plan_id,
        deleted_files: 0,
        deleted_logical_bytes: 0,
        submitted_files: 0,
        skipped_files: 0,
        failed_files: 0,
        unattempted_files: plan.items.len(),
        cancelled: false,
        outcomes: BTreeMap::new(),
        volumes: Vec::new(),
        receipt_file,
        receipt_error: None,
    };
    let now = rules::windows_time_now();
    for item in &plan.items {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        let outcome = match eligible(&item.candidate, ready, now) {
            Some(rule) => perform(item, rule),
            None => Outcome::skip("rule_not_ready"),
        };
        match outcome.status {
            "deleted" => {
                report.deleted_files += 1;
                report.deleted_logical_bytes = report
                    .deleted_logical_bytes
                    .saturating_add(item.candidate.stamp.size);
            }
            "submitted" => report.submitted_files += 1,
            "skipped" => report.skipped_files += 1,
            _ => report.failed_files += 1,
        }
        report.unattempted_files -= 1;
        count(&mut report.outcomes, outcome.reason);
        let entry = serde_json::json!({
            "path": item.path,
            "rule_id": item.candidate.rule_id,
            "outcome": outcome,
        });
        if let Err(error) = writeln!(receipt, "{entry}") {
            report.receipt_error = Some(error.to_string());
            break;
        }
    }
    if let Err(error) = receipt.sync_data() {
        report.receipt_error = Some(error.to_string());
    }
    for mut change in affected.into_values() {
        change.available_after = available(&change.volume_id, volumes);
        change.available_change_bytes = change
            .available_after
            .zip(change.available_before)
            .map(|(after, before)| i128::from(after) - i128::from(before));
        report.volumes.push(change);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{FileTimes, OpenOptions};
    use std::os::windows::fs::{FileTimesExt, OpenOptionsExt};
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    fn write_old(path: &Path, value: &[u8], stamp: SystemTime) {
        let mut file = fs::File::create(path).unwrap();
        file.write_all(value).unwrap();
        file.set_times(FileTimes::new().set_created(stamp).set_modified(stamp))
            .unwrap();
    }

    fn from_fixture(rule: &Rule, root: &Location, name: &str) -> Candidate {
        let location = root.child(name);
        let stamp = windows::Stamp::from(&fs::symlink_metadata(location.native()).unwrap());
        Candidate {
            rule_id: rule.id.clone(),
            location,
            stamp,
        }
    }

    #[test]
    fn review_selection_keeps_other_scopes_and_excluded_children_out_of_the_plan() {
        let fixture = tempfile::tempdir().unwrap();
        let store = Store::open(Some(fixture.path())).unwrap();
        let volumes = windows::volumes().unwrap();
        let rule = rules::load()
            .unwrap()
            .into_iter()
            .find(|rule| rule.action == Action::OldTemp)
            .unwrap();
        let old = SystemTime::now() - Duration::from_secs(9 * 24 * 60 * 60);
        let roots: Vec<_> = ["resources", "scripts", "other"]
            .into_iter()
            .map(|name| {
                let directory = fixture.path().join(name);
                fs::create_dir(&directory).unwrap();
                write_old(&directory.join("sample.tmp"), &[0; 32], old);
                windows::locate(&directory, &volumes).unwrap()
            })
            .collect();
        let node = |id: &str| Node {
            id: id.into(),
            label: id.into(),
        };
        let review = |purpose: &str| Review {
            owner: node("example"),
            feature: node("example/web"),
            purpose: node(purpose),
            usage: String::new(),
            impact: String::new(),
        };
        let review_groups: Vec<_> = ["example/web/resources", "example/web/scripts"]
            .into_iter()
            .zip(&roots)
            .map(|(purpose, root)| ScopedReview {
                review: review(purpose),
                target: Some(ReviewTarget {
                    rule_id: rule.id.clone(),
                    root: root.clone(),
                }),
            })
            .collect();
        assert!(resolve_groups(&["example/typo".into()], &review_groups).is_err());
        let snapshot = Snapshot {
            rules: digest(RULES_TEXT.as_bytes()),
            candidates: roots
                .iter()
                .map(|root| from_fixture(&rule, root, "sample.tmp"))
                .collect(),
            review_groups,
        };
        let (scan, _) = store.write("scan", &snapshot).unwrap();
        let ready = HashMap::from([(rule.id.clone(), ReadyRule { rule, roots })]);
        let preview = build_preview(
            &store,
            Selections {
                scans: vec![scan],
                include_group: vec!["example/web".into()],
                exclude_group: vec!["example/web/scripts".into()],
                exclude_rule: Vec::new(),
                exclude_volume: Vec::new(),
                exclude_file: Vec::new(),
            },
            &volumes,
            &ready,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(preview.files, 1);
        assert_eq!(preview.not_included["excluded"], 1);
        assert_eq!(preview.not_included["outside_review_selection"], 1);
        let saved: Plan = store.read("plan", &preview.plan_id).unwrap();
        assert_eq!(
            saved.items[0].path,
            fixture.path().join("resources/sample.tmp")
        );
        assert_eq!(
            preview.groups[0].review.as_ref().unwrap().purpose.id,
            "example/web/resources"
        );
    }

    #[test]
    fn previews_preserve_replacements_locked_files_and_hard_links() {
        let fixture = tempfile::tempdir().unwrap();
        let volumes = windows::volumes().unwrap();
        let root = windows::locate(fixture.path(), &volumes).unwrap();
        let rule = rules::load()
            .unwrap()
            .into_iter()
            .find(|r| r.action == Action::OldTemp)
            .unwrap();
        let ready = HashMap::from([(
            rule.id.clone(),
            ReadyRule {
                rule: rule.clone(),
                roots: vec![root.clone()],
            },
        )]);
        let old = SystemTime::now() - Duration::from_secs(9 * 24 * 60 * 60);
        for name in ["allowed.tmp", "replaced.tmp", "locked.tmp", "hardlink.tmp"] {
            write_old(&fixture.path().join(name), &[1, 2, 3, 4], old);
        }
        let hardlink = fixture.path().join("has-link.bin");
        fs::hard_link(fixture.path().join("hardlink.tmp"), &hardlink).unwrap();
        let now = rules::windows_time_now();
        assert!(matches!(
            prepare_item(
                from_fixture(&rule, &root, "hardlink.tmp"),
                &ready,
                &volumes,
                now
            ),
            Err("protected_path_or_file")
        ));
        let items: Vec<_> = ["allowed.tmp", "replaced.tmp", "locked.tmp"]
            .into_iter()
            .map(|name| {
                prepare_item(from_fixture(&rule, &root, name), &ready, &volumes, now).unwrap()
            })
            .collect();
        let plan = Plan {
            rules: digest(RULES_TEXT.as_bytes()),
            items,
        };
        let store = Store::open(Some(fixture.path())).unwrap();
        let (id, _) = store.write("plan", &plan).unwrap();
        let replaced = fixture.path().join("replaced.tmp");
        fs::rename(&replaced, fixture.path().join("replaced-before.bin")).unwrap();
        write_old(&replaced, &[5, 6, 7, 8], old);
        let locked = fixture.path().join("locked.tmp");
        let _lock = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&locked)
            .unwrap();

        let result = run_plan(
            &store,
            id.clone(),
            store.read("plan", &id).unwrap(),
            &volumes,
            &ready,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(result.deleted_files, 1);
        assert_eq!(result.deleted_logical_bytes, 4);
        assert_eq!(result.outcomes["changed_since_preview"], 1);
        assert_eq!(result.outcomes["in_use"], 1);
        assert!(result.volumes[0].available_before.is_some());
        assert!(result.volumes[0].available_after.is_some());
        assert!(!fixture.path().join("allowed.tmp").exists());
        assert_eq!(fs::read(&replaced).unwrap(), [5, 6, 7, 8]);
        assert_eq!(fs::read(hardlink).unwrap(), [1, 2, 3, 4]);

        let cancelled = run_plan(
            &store,
            id.clone(),
            store.read("plan", &id).unwrap(),
            &volumes,
            &ready,
            &AtomicBool::new(true),
        )
        .unwrap();
        assert!(cancelled.cancelled);
        assert_eq!(cancelled.unattempted_files, 3);
        let repeated = run_plan(
            &store,
            id.clone(),
            store.read("plan", &id).unwrap(),
            &volumes,
            &ready,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(repeated.deleted_files, 0);
        assert_eq!(repeated.outcomes["missing"], 1);
    }

    #[test]
    fn chrome_file_patterns_exclude_profile_data() {
        for name in ["index", "data_0", "data_3", "f_00a001"] {
            assert!(rules::chrome_blockfile_name(name), "{name}");
        }
        for name in [
            "History",
            "Cookies",
            "Preferences",
            "other.tmp",
            "f_example",
            "index-dir",
        ] {
            assert!(!rules::chrome_blockfile_name(name), "{name}");
        }
    }

    #[test]
    fn edited_preview_and_malformed_id_are_rejected() {
        let fixture = tempfile::tempdir().unwrap();
        let store = Store::open(Some(fixture.path())).unwrap();
        let (id, path) = store
            .write("plan", &serde_json::json!({"items": []}))
            .unwrap();
        fs::write(path, "{}").unwrap();
        assert!(store.read::<Plan>("plan", &id).is_err());
        assert!(store.read::<Plan>("plan", "..\\plan").is_err());
    }

    #[test]
    fn directory_replaced_by_a_junction_is_skipped() {
        let fixture = tempfile::tempdir().unwrap();
        let old_dir = fixture.path().join("planned");
        let other = fixture.path().join("outside-plan");
        fs::create_dir(&old_dir).unwrap();
        fs::create_dir(&other).unwrap();
        let age = SystemTime::now() - Duration::from_secs(9 * 24 * 60 * 60);
        write_old(&old_dir.join("sample.tmp"), &[1, 2], age);
        write_old(&other.join("sample.tmp"), &[3, 4], age);
        let volumes = windows::volumes().unwrap();
        let root = windows::locate(&old_dir, &volumes).unwrap();
        let rule = rules::load()
            .unwrap()
            .into_iter()
            .find(|r| r.action == Action::OldTemp)
            .unwrap();
        let ready = HashMap::from([(
            rule.id.clone(),
            ReadyRule {
                rule: rule.clone(),
                roots: vec![root.clone()],
            },
        )]);
        let item = prepare_item(
            from_fixture(&rule, &root, "sample.tmp"),
            &ready,
            &volumes,
            rules::windows_time_now(),
        )
        .unwrap();
        fs::rename(&old_dir, fixture.path().join("prior-directory")).unwrap();
        let junction = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command",
                "New-Item -ItemType Junction -Path $env:SPACESWEEP_TEST_LINK -Target $env:SPACESWEEP_TEST_TARGET -ErrorAction Stop | Out-Null"])
            .env("SPACESWEEP_TEST_LINK", &old_dir)
            .env("SPACESWEEP_TEST_TARGET", &other)
            .creation_flags(0x0800_0000)
            .output()
            .unwrap();
        assert!(
            junction.status.success(),
            "{}",
            String::from_utf8_lossy(&junction.stderr)
        );
        let result = perform(&item, &rule);
        assert_eq!(result.reason, "protected_path_or_file");
        assert_eq!(fs::read(other.join("sample.tmp")).unwrap(), [3, 4]);
        fs::remove_dir(old_dir).unwrap();
    }
}
