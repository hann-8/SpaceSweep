use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::OwnedHandle;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;

use crate::checked_file;
use crate::windows::{self, Folder};

pub struct Store {
    native: PathBuf,
    display: PathBuf,
    _parents: Vec<OwnedHandle>,
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn valid_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Store {
    pub fn open(override_path: Option<&Path>) -> Result<Self> {
        let volumes = windows::volumes()?;
        let display = match override_path {
            Some(path) => windows::scope_path(path)?,
            None => {
                let app_data = Folder::LocalAppData.known()?;
                let parent = windows::locate(&app_data, &volumes)
                    .context("LocalAppData is outside the local mounted volumes")?;
                let _parent = checked_file::pin_directory(&parent)?;
                let directory = app_data.join("SpaceSweep");
                match fs::create_dir(&directory) {
                    Ok(()) => {}
                    Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(err) => {
                        return Err(err).context("create a local SpaceSweep state directory");
                    }
                }
                directory
            }
        };
        let location = windows::locate(&display, &volumes)
            .context("state directory must be on a local mounted volume")?;
        Ok(Self {
            native: location.native(),
            display,
            _parents: checked_file::pin_directory(&location)
                .context("verify the state directory")?,
        })
    }

    pub fn write<T: Serialize>(&self, kind: &str, document: &T) -> Result<(String, PathBuf)> {
        let bytes = serde_json::to_vec_pretty(document)?;
        let id = digest(&bytes);
        let name = format!("{kind}-{id}.json");
        let path = self.native.join(&name);
        if path.exists() {
            self.bytes(kind, &id)?;
        } else {
            let staging = self.native.join(format!("staging-{}.json", Uuid::new_v4()));
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&staging)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(staging, &path)?;
        }
        Ok((id, self.display.join(name)))
    }

    fn bytes(&self, kind: &str, id: &str) -> Result<Vec<u8>> {
        ensure!(valid_id(id), "invalid {kind} ID");
        let path = self.native.join(format!("{kind}-{id}.json"));
        let metadata = fs::symlink_metadata(&path).with_context(|| format!("open saved {kind}"))?;
        let stamp = windows::Stamp::from(&metadata);
        ensure!(
            metadata.is_file() && stamp.attributes & windows::SKIP_ATTRIBUTES == 0,
            "local {kind} document is not a regular file"
        );
        let contents = fs::read(path)?;
        ensure!(digest(&contents) == id, "saved {kind} has changed");
        Ok(contents)
    }

    pub fn read<T: DeserializeOwned>(&self, kind: &str, id: &str) -> Result<T> {
        Ok(serde_json::from_slice(&self.bytes(kind, id)?)?)
    }

    pub fn acquire_apply_lock(&self, id: &str) -> Result<File> {
        ensure!(valid_id(id), "invalid plan ID");
        let path = self.native.join(format!("apply-{id}.lock"));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        file.try_lock()
            .context("this plan is already running or its lock is unavailable")?;
        Ok(file)
    }

    pub fn receipt(&self) -> Result<(File, PathBuf)> {
        let name = format!("receipt-{}.jsonl", Uuid::new_v4());
        let file = File::create_new(self.native.join(&name))?;
        Ok((file, self.display.join(name)))
    }
}
