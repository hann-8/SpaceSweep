use std::ffi::{OsStr, OsString};
use std::fs::{self, Metadata};
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::ptr::null_mut;

use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{ERROR_NO_MORE_FILES, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Globalization::CompareStringOrdinal;
use windows_sys::Win32::Storage::FileSystem::{
    FindFirstVolumeW, FindNextVolumeW, FindVolumeClose, GetDiskFreeSpaceExW, GetDriveTypeW,
    GetFileVersionInfoSizeW, GetFileVersionInfoW, GetVolumeInformationW,
    GetVolumePathNamesForVolumeNameW, VS_FIXEDFILEINFO, VerQueryValueW,
};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::UI::Shell::{
    FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_LocalAppData, FOLDERID_ProgramFiles,
    FOLDERID_ProgramFilesX86, SHGetKnownFolderPath,
};

pub const DIRECTORY: u32 = 0x10;
pub const PROTECTED_FILE: u32 = 0x1 | 0x2 | 0x4;
pub const SKIP_ATTRIBUTES: u32 = 0x400 | 0x1000 | 0x40000 | 0x400000;

#[derive(Clone, Debug, Serialize)]
pub struct Volume {
    pub id: String,
    pub mounts: Vec<PathBuf>,
    pub kind: &'static str,
    pub filesystem: Option<String>,
    pub space: Option<Space>,
    pub space_error: Option<String>,
}

impl Volume {
    pub fn display(&self, relative: &Path) -> PathBuf {
        self.mounts[0].join(relative)
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Space {
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct Location {
    pub volume: String,
    pub relative: PathBuf,
}

impl Location {
    pub fn native(&self) -> PathBuf {
        Path::new(&self.volume).join(&self.relative)
    }

    pub fn child(&self, name: impl AsRef<Path>) -> Self {
        Self {
            volume: self.volume.clone(),
            relative: self.relative.join(name),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Stamp {
    pub size: u64,
    pub created: u64,
    pub modified: u64,
    pub attributes: u32,
}

impl From<&Metadata> for Stamp {
    fn from(meta: &Metadata) -> Self {
        Self {
            size: meta.file_size(),
            created: meta.creation_time(),
            modified: meta.last_write_time(),
            attributes: meta.file_attributes(),
        }
    }
}

pub fn wide(value: impl AsRef<OsStr>) -> io::Result<Vec<u16>> {
    let mut utf16: Vec<_> = value.as_ref().encode_wide().collect();
    if utf16.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "embedded NUL"));
    }
    utf16.push(0);
    Ok(utf16)
}

fn text(buffer: &[u16]) -> String {
    String::from_utf16_lossy(buffer.split(|unit| *unit == 0).next().unwrap_or_default())
}

pub fn same_name(a: &OsStr, b: &OsStr) -> bool {
    if a == b {
        return true;
    }
    let a: Vec<_> = a.encode_wide().collect();
    let b: Vec<_> = b.encode_wide().collect();
    unsafe { CompareStringOrdinal(a.as_ptr(), a.len() as i32, b.as_ptr(), b.len() as i32, 1) == 2 }
}

pub fn relative_to(path: &Path, root: &Path) -> Option<PathBuf> {
    let mut path = path.components();
    for prefix in root.components() {
        if !same_name(path.next()?.as_os_str(), prefix.as_os_str()) {
            return None;
        }
    }
    let result = path.as_path();
    valid_relative(result).then(|| result.to_path_buf())
}

pub fn valid_relative(path: &Path) -> bool {
    path.components().all(|part| match part {
        Component::Normal(name) => !name
            .encode_wide()
            .any(|unit| unit == 0 || unit == b':' as u16),
        _ => false,
    })
}

pub fn locate(path: &Path, volumes: &[Volume]) -> Option<Location> {
    volumes
        .iter()
        .flat_map(|volume| {
            volume
                .mounts
                .iter()
                .map(|p| p.as_path())
                .chain(std::iter::once(Path::new(&volume.id)))
                .filter_map(|mount| {
                    let relative = relative_to(path, mount)?;
                    let specificity = mount.components().count();
                    Some((
                        specificity,
                        Location {
                            volume: volume.id.clone(),
                            relative,
                        },
                    ))
                })
        })
        .max_by_key(|(specificity, _)| *specificity)
        .map(|(_, location)| location)
}

pub fn view(location: &Location, volumes: &[Volume]) -> PathBuf {
    volumes
        .iter()
        .find(|v| v.id == location.volume)
        .map_or_else(
            || location.native(),
            |volume| volume.display(&location.relative),
        )
}

pub fn scope_path(input: &Path) -> io::Result<PathBuf> {
    if let Some(s) = input.to_str() {
        let bytes = s.as_bytes();
        if bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return Ok(PathBuf::from(format!("{s}\\")));
        }
        if let Some(dos) = s.strip_prefix(r"\\?\")
            && dos.as_bytes().get(1) == Some(&b':')
        {
            return Ok(PathBuf::from(dos));
        }
    }
    std::path::absolute(input)
}

pub fn check_directory(location: &Location) -> io::Result<()> {
    if !valid_relative(&location.relative) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid directory path",
        ));
    }
    let mut path = PathBuf::from(&location.volume);
    for part in std::iter::once(None).chain(location.relative.components().map(Some)) {
        if let Some(part) = part {
            path.push(part.as_os_str());
        }
        let meta = fs::symlink_metadata(&path)?;
        if !meta.is_dir() || meta.file_attributes() & SKIP_ATTRIBUTES != 0 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "directory is a reparse point, offline, or not a directory",
            ));
        }
    }
    Ok(())
}

pub fn space(path: impl AsRef<OsStr>) -> io::Result<Space> {
    let path = wide(path)?;
    let mut available = 0;
    let mut total = 0;
    let ok = unsafe { GetDiskFreeSpaceExW(path.as_ptr(), &mut available, &mut total, null_mut()) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Space {
        total_bytes: total,
        available_bytes: available,
    })
}

pub fn process_running(executable: &str) -> io::Result<bool> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let _close = unsafe { OwnedHandle::from_raw_handle(snapshot) };
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut available = unsafe { Process32FirstW(snapshot, &mut entry) };
    while available != 0 {
        if text(&entry.szExeFile).eq_ignore_ascii_case(executable) {
            return Ok(true);
        }
        available = unsafe { Process32NextW(snapshot, &mut entry) };
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
        Ok(false)
    } else {
        Err(error)
    }
}

fn executable_major(path: &Path) -> io::Result<u32> {
    let path = wide(path)?;
    let mut handle = 0;
    let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), &mut handle) };
    if size == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut data = vec![0u32; (size as usize).div_ceil(4)];
    if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, data.as_mut_ptr().cast()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let root = [b'\\' as u16, 0];
    let mut value = null_mut();
    let mut length = 0;
    let ok =
        unsafe { VerQueryValueW(data.as_ptr().cast(), root.as_ptr(), &mut value, &mut length) };
    if ok == 0 || value.is_null() || (length as usize) < size_of::<VS_FIXEDFILEINFO>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid executable version",
        ));
    }
    let info = unsafe { value.cast::<VS_FIXEDFILEINFO>().read_unaligned() };
    Ok(info.dwProductVersionMS >> 16)
}

pub fn installed_chrome_majors() -> Vec<u32> {
    [
        Folder::ProgramFiles,
        Folder::ProgramFilesX86,
        Folder::LocalAppData,
    ]
    .into_iter()
    .filter_map(|folder| {
        let path = folder
            .known()
            .ok()?
            .join("Google/Chrome/Application/chrome.exe");
        executable_major(&path).ok()
    })
    .collect()
}

fn mounts(volume: &[u16]) -> io::Result<Vec<PathBuf>> {
    let mut buffer = vec![0u16; 256];
    loop {
        let mut required = 0;
        let ok = unsafe {
            GetVolumePathNamesForVolumeNameW(
                volume.as_ptr(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                &mut required,
            )
        };
        if ok != 0 {
            return Ok(buffer
                .split(|unit| *unit == 0)
                .take_while(|s| !s.is_empty())
                .map(|s| PathBuf::from(OsString::from_wide(s)))
                .collect());
        }
        if required as usize <= buffer.len() {
            return Err(io::Error::last_os_error());
        }
        buffer.resize(required as usize, 0);
    }
}

fn filesystem(volume: &[u16]) -> Option<String> {
    let mut name = [0u16; 64];
    let ok = unsafe {
        GetVolumeInformationW(
            volume.as_ptr(),
            null_mut(),
            0,
            null_mut(),
            null_mut(),
            null_mut(),
            name.as_mut_ptr(),
            name.len() as u32,
        )
    };
    (ok != 0).then(|| text(&name))
}

struct VolumeSearch(HANDLE);

impl Drop for VolumeSearch {
    fn drop(&mut self) {
        unsafe { FindVolumeClose(self.0) };
    }
}

pub fn volumes() -> io::Result<Vec<Volume>> {
    let mut name = [0u16; 128];
    let handle = unsafe { FindFirstVolumeW(name.as_mut_ptr(), name.len() as u32) };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let search = VolumeSearch(handle);
    let mut result = Vec::new();
    loop {
        let mut locations = mounts(&name).unwrap_or_default();
        locations.sort_by_key(|path| (path.as_os_str().len(), path.clone()));
        if let Some(first) = locations.first() {
            let kind = match unsafe { GetDriveTypeW(wide(first)?.as_ptr()) } {
                2 => "removable",
                3 => "fixed",
                5 => "optical",
                6 => "ram_disk",
                _ => "unavailable",
            };
            let id = text(&name);
            let (capacity, error) = match space(first) {
                Ok(value) => (Some(value), None),
                Err(error) => (None, Some(error.to_string())),
            };
            result.push(Volume {
                id,
                mounts: locations,
                kind,
                filesystem: filesystem(&name),
                space: capacity,
                space_error: error,
            });
        }
        let ok = unsafe { FindNextVolumeW(search.0, name.as_mut_ptr(), name.len() as u32) };
        if ok == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
                return Err(error);
            }
            break;
        }
    }
    result.sort_by(|a, b| a.mounts[0].cmp(&b.mounts[0]));
    Ok(result)
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Folder {
    LocalAppData,
    Documents,
    Downloads,
    ProgramFiles,
    ProgramFilesX86,
    VolumeRoot,
}

impl Folder {
    pub fn known(self) -> io::Result<PathBuf> {
        let id = match self {
            Self::LocalAppData => &FOLDERID_LocalAppData,
            Self::Documents => &FOLDERID_Documents,
            Self::Downloads => &FOLDERID_Downloads,
            Self::ProgramFiles => &FOLDERID_ProgramFiles,
            Self::ProgramFilesX86 => &FOLDERID_ProgramFilesX86,
            Self::VolumeRoot => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "volume root is not a known folder",
                ));
            }
        };
        let mut pointer = null_mut();
        // KF_FLAG_DONT_VERIFY avoids implicitly creating or opening a known folder.
        let hr = unsafe { SHGetKnownFolderPath(id, 0x4000, null_mut(), &mut pointer) };
        let result = if hr >= 0 && !pointer.is_null() {
            let len = (0..)
                .take_while(|i| unsafe { *pointer.add(*i) != 0 })
                .count();
            let units = unsafe { std::slice::from_raw_parts(pointer, len) };
            Ok(PathBuf::from(OsString::from_wide(units)))
        } else {
            Err(io::Error::other(format!(
                "known folder unavailable (HRESULT {hr:#x})"
            )))
        };
        unsafe { CoTaskMemFree(pointer.cast()) };
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_uses_the_deepest_mount_and_preserves_relative_components() {
        let volume = |id: &str, mount: &str| Volume {
            id: id.to_owned(),
            mounts: vec![mount.into()],
            kind: "fixed",
            filesystem: None,
            space: None,
            space_error: None,
        };
        let volumes = [
            volume(r"\\?\Volume{root}\", r"C:\"),
            volume(r"\\?\Volume{other}\", r"C:\Mount\Data\"),
        ];
        let location = locate(Path::new(r"c:\MOUNT\DATA\微信\缓存"), &volumes).unwrap();
        assert_eq!(location.volume, volumes[1].id);
        assert_eq!(location.relative, Path::new(r"微信\缓存"));
        assert!(locate(Path::new(r"C:\file.txt:stream"), &volumes).is_none());
    }
}
