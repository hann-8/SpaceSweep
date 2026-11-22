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
    Installers,
}

impl Action {
    pub fn recursive(self) -> bool {
        self == Self::Usage
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
        match self.action {
            Action::Usage => true,
            Action::Installers => ["exe", "msi", "msix", "appx", "msixbundle", "appxbundle"]
                .iter()
                .any(|ending| extension.eq_ignore_ascii_case(ending)),
            Action::OldTemp => {
                let old = now.saturating_sub(SEVEN_DAYS);
                extension.eq_ignore_ascii_case("tmp")
                    && stamp.attributes
                        & (windows::PROTECTED_FILE | windows::SKIP_ATTRIBUTES | windows::DIRECTORY)
                        == 0
                    && stamp.created != 0
                    && stamp.modified != 0
                    && stamp.created <= old
                    && stamp.modified <= old
            }
        }
    }
}

pub fn windows_time_now() -> u64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    (duration.as_secs() + WINDOWS_UNIX_OFFSET) * TICKS_PER_SECOND
        + u64::from(duration.subsec_nanos()) / 100
}
