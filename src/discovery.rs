use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::rules;
use crate::windows::{self, Location, Stamp, Volume};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    RecycleBin,
    CrashDumps,
    Minidump,
    Thumbnails,
    BrowserCache,
    NpmCache,
    PipCache,
    OldTemp,
    TempInstaller,
    DownloadInstaller,
    ArchiveWithDirectory,
    DumpClue,
}

impl Kind {
    fn description(self) -> (&'static str, &'static str) {
        match self {
            Self::RecycleBin => ("回收站", "Windows 返回该盘回收站的条目数和大小。"),
            Self::CrashDumps => (
                "应用崩溃转储",
                "当前用户的 Windows 崩溃转储目录里的 .dmp，供排查崩溃时使用。",
            ),
            Self::Minidump => (
                "散落的应用进程转储",
                "文件名包含程序和进程号，文件头是 Windows 进程转储格式。",
            ),
            Self::Thumbnails => (
                "Windows 缩略图缓存",
                "资源管理器生成的 thumbcache 数据库，浏览文件时会重建缩略图。",
            ),
            Self::BrowserCache => (
                "网页磁盘缓存",
                "浏览器用户配置中的 Cache/Cache_Data，网页资源可按需重新下载。",
            ),
            Self::NpmCache => (
                "npm 软件包下载缓存",
                "npm 默认位置的 _cacache，存放可重新获取的请求和软件包数据。",
            ),
            Self::PipCache => (
                "pip 下载和构建缓存",
                "pip 默认缓存位置中的 HTTP 响应或 wheel 缓存，安装时可重新获取或构建。",
            ),
            Self::OldTemp => (
                "长期未改动的 .tmp",
                "临时目录中创建和最后写入均超过 7 天的 .tmp。",
            ),
            Self::TempInstaller => (
                "临时目录里的安装包线索",
                "位于临时目录，扩展名为安装包格式或程序名含 Setup、Installer。可查对应安装或更新是否已完成。",
            ),
            Self::DownloadInstaller => (
                "下载的安装包线索",
                "下载目录中的安装包格式或可执行文件。可查是否已用完、是否需要留作离线安装。",
            ),
            Self::ArchiveWithDirectory => (
                "压缩包与同名目录",
                "临时目录中压缩包旁有同名目录，疑似已解包。可核对内容和来源。",
            ),
            Self::DumpClue => (
                "其他位置的进程转储线索",
                "文件名符合 程序.exe.进程号.dmp，可核对由哪个应用或调试任务生成。",
            ),
        }
    }
}

#[derive(Serialize, Debug)]
pub struct Finding {
    pub kind: Kind,
    pub identified: bool,
    pub label: String,
    pub evidence: &'static str,
    pub volume_id: String,
    pub path: PathBuf,
    pub files: u64,
    pub bytes: u64,
    pub examples: Vec<PathBuf>,
}

impl Finding {
    pub fn new(kind: Kind, volume_id: String, path: PathBuf, files: u64, bytes: u64) -> Self {
        let (label, evidence) = kind.description();
        Self {
            kind,
            identified: matches!(
                kind,
                Kind::RecycleBin
                    | Kind::CrashDumps
                    | Kind::Minidump
                    | Kind::Thumbnails
                    | Kind::BrowserCache
                    | Kind::NpmCache
                    | Kind::PipCache
                    | Kind::OldTemp
            ),
            label: label.into(),
            evidence,
            volume_id,
            path,
            files,
            bytes,
            examples: Vec::new(),
        }
    }
}

#[derive(Serialize, Debug)]
pub struct Check {
    pub label: &'static str,
    pub path: PathBuf,
    pub status: &'static str,
    pub detail: Option<String>,
}

#[derive(Default, Serialize, Debug)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub checks: Vec<Check>,
    pub quick_elapsed_ms: Option<u64>,
}

impl Report {
    pub fn sort(&mut self) {
        self.findings.sort_unstable_by(|a, b| {
            a.kind
                .cmp(&b.kind)
                .then_with(|| b.bytes.cmp(&a.bytes))
                .then_with(|| a.path.cmp(&b.path))
        });
    }
}

#[derive(Default)]
pub struct Groups(BTreeMap<(Kind, PathBuf), Finding>);

impl Groups {
    pub fn add(&mut self, kind: Kind, group: &Path, file: &Location, stamp: Stamp) {
        let finding = self
            .0
            .entry((kind, group.to_owned()))
            .or_insert_with(|| Finding::new(kind, file.volume.clone(), group.to_owned(), 0, 0));
        finding.files += 1;
        finding.bytes = finding.bytes.saturating_add(stamp.size);
        if finding.examples.len() < 3
            && let Some(name) = file.relative.file_name()
        {
            finding.examples.push(name.into());
        }
    }

    pub fn append_to(self, report: &mut Report, volumes: &[Volume]) {
        report
            .findings
            .extend(self.0.into_values().map(|mut finding| {
                finding.path = windows::view(
                    &Location {
                        volume: finding.volume_id.clone(),
                        relative: finding.path,
                    },
                    volumes,
                );
                finding
            }));
    }
}

pub fn extension(file: &Path) -> &str {
    file.extension()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
}

/// Classify file metadata, with deeper checks limited to a small set of matching names.
pub fn clue(file: &Location, stamp: Stamp, now: u64, known_temp: bool) -> Option<Kind> {
    let ext = extension(&file.relative);
    if ext.eq_ignore_ascii_case("dmp") {
        let stem = file.relative.file_stem()?.to_str()?;
        let (program, pid) = stem.rsplit_once('.')?;
        if !extension(Path::new(program)).eq_ignore_ascii_case("exe")
            || pid.is_empty()
            || !pid.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let mut signature = [0; 4];
        let minidump = fs::File::open(file.native())
            .and_then(|mut file| file.read_exact(&mut signature))
            .is_ok()
            && &signature == b"MDMP";
        return Some(if minidump {
            Kind::Minidump
        } else {
            Kind::DumpClue
        });
    }
    let installer = ["msi", "msix", "msixbundle", "appx", "appxbundle"]
        .iter()
        .any(|format| ext.eq_ignore_ascii_case(format));
    let exe = ext.eq_ignore_ascii_case("exe");
    let tmp = ext.eq_ignore_ascii_case("tmp");
    let archive = ext.eq_ignore_ascii_case("zip") || ext.eq_ignore_ascii_case("7z");
    if !(installer || exe || tmp || archive) {
        return None;
    }
    let parent = file.relative.parent()?;
    let in_temp = known_temp
        || parent.components().any(|item| {
            item.as_os_str().to_str().is_some_and(|name| {
                name.eq_ignore_ascii_case("temp")
                    || name.eq_ignore_ascii_case("tmp")
                    || name == "临时文件"
            })
        });
    if !in_temp {
        return None;
    }
    if tmp && rules::old(stamp, now) {
        return Some(Kind::OldTemp);
    }
    if installer
        || exe
            && file.relative.file_stem()?.to_str().is_some_and(|name| {
                let name = name.to_ascii_lowercase();
                name.contains("setup") || name.contains("installer")
            })
    {
        return Some(Kind::TempInstaller);
    }
    if archive {
        let sibling = file.native().with_extension("");
        if fs::symlink_metadata(sibling).is_ok_and(|meta| {
            meta.is_dir() && Stamp::from(&meta).attributes & windows::SKIP_ATTRIBUTES == 0
        }) {
            return Some(Kind::ArchiveWithDirectory);
        }
    }
    None
}
