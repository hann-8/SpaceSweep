use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::windows::{self, Folder, Location, Stamp, Volume};

pub const RULES_TEXT: &str = include_str!("../rules/windows.toml");
const TICKS_PER_SECOND: u64 = 10_000_000;
const WINDOWS_UNIX_OFFSET: u64 = 11_644_473_600;
const SEVEN_DAYS: u64 = 7 * 24 * 60 * 60 * TICKS_PER_SECOND;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Usage,
    OldTemp,
    ChromeHttpCache,
    Installers,
}

impl Action {
    pub fn recursive(self) -> bool {
        self == Self::Usage
    }

    pub fn can_delete(self) -> bool {
        matches!(self, Self::OldTemp | Self::ChromeHttpCache)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub label: String,
    pub anchor: Folder,
    #[serde(default)]
    pub path: PathBuf,
    #[serde(default)]
    pub profiles: bool,
    pub verified_major: Option<u32>,
    pub action: Action,
    pub note: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    rules: Vec<Rule>,
}

pub fn load() -> Result<Vec<Rule>> {
    let bundle: Bundle = toml::from_str(RULES_TEXT)?;
    let mut ids = std::collections::HashSet::new();
    for rule in &bundle.rules {
        ensure!(
            windows::valid_relative(&rule.path),
            "invalid rule path: {}",
            rule.id
        );
        ensure!(ids.insert(&rule.id), "duplicate rule: {}", rule.id);
        if rule.action == Action::OldTemp {
            ensure!(
                matches!(rule.anchor, Folder::LocalAppData)
                    && rule.path == Path::new("Temp")
                    && !rule.profiles,
                "temporary file rule must use the current user's LocalAppData Temp"
            );
        }
        if rule.action == Action::ChromeHttpCache {
            ensure!(
                matches!(rule.anchor, Folder::LocalAppData)
                    && rule.path == Path::new("Google/Chrome/User Data")
                    && rule.profiles
                    && rule.verified_major.is_some(),
                "Chrome cache rule must use the verified profile layout"
            );
        }
    }
    Ok(bundle.rules)
}

fn profile_name(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    name == "Default"
        || name
            .strip_prefix("Profile ")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|byte| byte.is_ascii_digit()))
}

impl Rule {
    pub fn locations(
        &self,
        volumes: &[Volume],
        selected: &HashSet<String>,
    ) -> io::Result<Vec<Location>> {
        if matches!(self.anchor, Folder::VolumeRoot) {
            return Ok(volumes
                .iter()
                .filter(|volume| selected.is_empty() || selected.contains(&volume.id))
                .map(|volume| Location {
                    volume: volume.id.clone(),
                    relative: self.path.clone(),
                })
                .collect());
        }
        let absolute = self.anchor.known()?.join(&self.path);
        let base = windows::locate(&absolute, volumes).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "root is outside mounted local volumes",
            )
        })?;
        if !selected.is_empty() && !selected.contains(&base.volume) {
            return Ok(Vec::new());
        }
        if !self.profiles {
            return Ok(vec![base]);
        }
        windows::check_directory(&base)?;
        let mut locations = Vec::new();
        for entry in fs::read_dir(base.native())? {
            let entry = entry?;
            if profile_name(&entry.file_name()) {
                locations.push(
                    base.child(entry.file_name())
                        .child("Cache")
                        .child("Cache_Data"),
                );
            }
        }
        locations.sort_by(|a, b| a.relative.cmp(&b.relative));
        Ok(locations)
    }

    pub fn matches(&self, file: &Path, stamp: Stamp, now: u64) -> bool {
        let extension = file.extension().and_then(OsStr::to_str).unwrap_or_default();
        let ordinary = stamp.attributes
            & (windows::PROTECTED_FILE | windows::SKIP_ATTRIBUTES | windows::DIRECTORY)
            == 0;
        match self.action {
            Action::Usage => true,
            Action::Installers => ["exe", "msi", "msix", "appx", "msixbundle", "appxbundle"]
                .iter()
                .any(|ending| extension.eq_ignore_ascii_case(ending)),
            Action::OldTemp => {
                let old = now.saturating_sub(SEVEN_DAYS);
                extension.eq_ignore_ascii_case("tmp")
                    && ordinary
                    && stamp.created != 0
                    && stamp.modified != 0
                    && stamp.created <= old
                    && stamp.modified <= old
            }
            Action::ChromeHttpCache => {
                ordinary
                    && file
                        .file_name()
                        .and_then(OsStr::to_str)
                        .is_some_and(chrome_blockfile_name)
            }
        }
    }

    pub fn block_reason(&self, root: &Location, volumes: &[Volume]) -> Option<String> {
        if !self.action.can_delete() {
            return None;
        }
        if !volumes
            .iter()
            .any(|v| v.id == root.volume && v.filesystem.as_deref() == Some("NTFS"))
        {
            return Some("filesystem_not_verified".into());
        }
        if self.action == Action::ChromeHttpCache {
            let majors = windows::installed_chrome_majors();
            if majors.is_empty()
                || majors
                    .iter()
                    .any(|major| Some(*major) != self.verified_major)
            {
                return Some("chrome_version_not_verified".into());
            }
            if !["index", "data_0"].into_iter().all(|name| {
                fs::symlink_metadata(root.native().join(name)).is_ok_and(|meta| {
                    let stamp = Stamp::from(&meta);
                    meta.is_file() && stamp.attributes & windows::SKIP_ATTRIBUTES == 0
                })
            }) {
                return Some("chrome_cache_layout_not_verified".into());
            }
            match windows::process_running("chrome.exe") {
                Ok(true) => return Some("chrome_running".into()),
                Err(_) => return Some("chrome_process_state_unavailable".into()),
                Ok(false) => {}
            }
        }
        None
    }
}

pub fn chrome_blockfile_name(name: &str) -> bool {
    name == "index"
        || name.strip_prefix("data_").is_some_and(|digits| {
            !digits.is_empty()
                && digits.len() <= 3
                && digits.bytes().all(|byte| byte.is_ascii_digit())
        })
        || name.strip_prefix("f_").is_some_and(|digits| {
            digits.len() == 6 && digits.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

pub fn windows_time_now() -> u64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    (duration.as_secs() + WINDOWS_UNIX_OFFSET) * TICKS_PER_SECOND
        + u64::from(duration.subsec_nanos()) / 100
}
