use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, DELETE, FILE_DISPOSITION_INFO,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO, FILE_READ_ATTRIBUTES,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_STANDARD_INFO, FileDispositionInfo, FileIdInfo,
    FileStandardInfo, GetFileInformationByHandle, GetFileInformationByHandleEx,
    GetFinalPathNameByHandleW, OPEN_EXISTING, SetFileInformationByHandle, VOLUME_NAME_GUID,
};

use crate::windows::{self, Location, Stamp};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Identity {
    pub serial: u64,
    pub file_id: [u8; 16],
    pub final_path: PathBuf,
}

pub struct CheckedFile {
    file: OwnedHandle,
    _parents: Vec<OwnedHandle>,
    pub identity: Identity,
    pub stamp: Stamp,
    pub allocated_bytes: u64,
}

fn open(path: &Path, access: u32, share: u32, flags: u32) -> io::Result<OwnedHandle> {
    let name = windows::wide(path)?;
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            access,
            share,
            null(),
            OPEN_EXISTING,
            flags | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn basic(handle: HANDLE) -> io::Result<BY_HANDLE_FILE_INFORMATION> {
    let mut result = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(handle, &mut result) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(result)
}

fn final_path(handle: HANDLE) -> io::Result<PathBuf> {
    let mut buffer = vec![0u16; 512];
    loop {
        let size = unsafe {
            GetFinalPathNameByHandleW(
                handle,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                VOLUME_NAME_GUID,
            )
        } as usize;
        if size == 0 {
            return Err(io::Error::last_os_error());
        }
        if size < buffer.len() {
            let name = String::from_utf16(&buffer[..size])
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
            return Ok(PathBuf::from(name));
        }
        buffer.resize(size + 1, 0);
    }
}

fn check_final(handle: HANDLE, expected: &Path) -> io::Result<PathBuf> {
    let actual = final_path(handle)?;
    if windows::relative_to(&actual, expected).is_none_or(|path| !path.as_os_str().is_empty()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "resolved path differs from the approved path",
        ));
    }
    Ok(actual)
}

pub fn resolved_directory(path: &Path) -> io::Result<PathBuf> {
    let handle = open(
        path,
        FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        FILE_FLAG_BACKUP_SEMANTICS,
    )?;
    let attrs = basic(handle.as_raw_handle())?.dwFileAttributes;
    if attrs & windows::DIRECTORY == 0 || attrs & windows::SKIP_ATTRIBUTES != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "not an ordinary directory",
        ));
    }
    final_path(handle.as_raw_handle())
}

pub fn pin_directory(location: &Location) -> io::Result<Vec<OwnedHandle>> {
    if !windows::valid_relative(&location.relative) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid directory path",
        ));
    }
    let mut path = PathBuf::from(&location.volume);
    let mut handles = Vec::new();
    for part in std::iter::once(None).chain(location.relative.components().map(Some)) {
        if let Some(part) = part {
            path.push(part.as_os_str());
        }
        // Keep each parent open without sharing delete, preventing a rename during verification.
        let handle = open(
            &path,
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_FLAG_BACKUP_SEMANTICS,
        )?;
        let attrs = basic(handle.as_raw_handle())?.dwFileAttributes;
        if attrs & windows::DIRECTORY == 0 || attrs & windows::SKIP_ATTRIBUTES != 0 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "parent is a reparse point or unavailable directory",
            ));
        }
        check_final(handle.as_raw_handle(), &path)?;
        handles.push(handle);
    }
    Ok(handles)
}

impl CheckedFile {
    pub fn open(location: &Location, delete: bool) -> io::Result<Self> {
        if !windows::valid_relative(&location.relative) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid file path",
            ));
        }
        let parent = location
            .relative
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file path required"))?;
        let parents = pin_directory(&Location {
            volume: location.volume.clone(),
            relative: parent.to_path_buf(),
        })?;
        let path = location.native();
        let access = FILE_READ_ATTRIBUTES | if delete { DELETE } else { 0 };
        // No write or delete sharing while the selected object is verified.
        let file = open(&path, access, FILE_SHARE_READ, 0)?;
        let info = basic(file.as_raw_handle())?;
        if info.dwFileAttributes
            & (windows::DIRECTORY | windows::SKIP_ATTRIBUTES | windows::PROTECTED_FILE)
            != 0
        {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "protected or non-regular file",
            ));
        }
        if info.nNumberOfLinks != 1 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "file has multiple hard links",
            ));
        }
        let actual = check_final(file.as_raw_handle(), &path)?;
        let mut id = FILE_ID_INFO::default();
        let mut standard = FILE_STANDARD_INFO::default();
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileIdInfo,
                (&mut id as *mut FILE_ID_INFO).cast(),
                size_of::<FILE_ID_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileStandardInfo,
                (&mut standard as *mut FILE_STANDARD_INFO).cast(),
                size_of::<FILE_STANDARD_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if standard.DeletePending || standard.AllocationSize < 0 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "file is pending deletion or allocation is invalid",
            ));
        }
        let combine = |high: u32, low: u32| (u64::from(high) << 32) | u64::from(low);
        Ok(Self {
            file,
            _parents: parents,
            identity: Identity {
                serial: id.VolumeSerialNumber,
                file_id: id.FileId.Identifier,
                final_path: actual,
            },
            stamp: Stamp {
                size: combine(info.nFileSizeHigh, info.nFileSizeLow),
                created: combine(
                    info.ftCreationTime.dwHighDateTime,
                    info.ftCreationTime.dwLowDateTime,
                ),
                modified: combine(
                    info.ftLastWriteTime.dwHighDateTime,
                    info.ftLastWriteTime.dwLowDateTime,
                ),
                attributes: info.dwFileAttributes,
            },
            allocated_bytes: standard.AllocationSize as u64,
        })
    }

    pub fn request_deletion(self) -> io::Result<()> {
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        let ok = unsafe {
            SetFileInformationByHandle(
                self.file.as_raw_handle(),
                FileDispositionInfo,
                (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

pub fn is_busy(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(32 | 33))
}
