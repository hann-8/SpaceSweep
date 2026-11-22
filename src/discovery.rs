use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::review::Review;
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
    BrowserCodeCache,
    BrowserComponentCache,
    FirefoxCache,
    NpmCache,
    PipCache,
    OldTemp,
    TempInstaller,
    DownloadInstaller,
    ArchiveWithDirectory,
    DumpClue,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewLevel {
    Confirmed,
    NeedsReview,
    Suspected,
}

impl Kind {
    pub fn review_level(self) -> ReviewLevel {
        match self {
            Self::TempInstaller
            | Self::DownloadInstaller
            | Self::ArchiveWithDirectory
            | Self::DumpClue => ReviewLevel::Suspected,
            _ => ReviewLevel::NeedsReview,
        }
    }

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
                "Chromium 浏览器和桌面应用使用的 Cache/Cache_Data，资源可按需重新下载。",
            ),
            Self::BrowserCodeCache => (
                "网页代码缓存",
                "Chromium 浏览器和桌面应用为脚本生成的缓存。清理后可能需要重新编译。",
            ),
            Self::BrowserComponentCache => (
                "浏览器组件更新缓存",
                "用户数据根目录下缓存的组件更新包及索引。清理后，组件更新时可能需要重新下载。",
            ),
            Self::FirefoxCache => (
                "Firefox 网页磁盘缓存",
                "Firefox 配置目录下的 cache2 网页资源缓存，资源可按需重新下载。",
            ),
            Self::NpmCache => (
                "npm 软件包下载缓存",
                "npm 等工具使用的 _cacache，存放可重新获取的请求和软件包数据。",
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
    pub review_level: ReviewLevel,
    pub label: String,
    pub evidence: &'static str,
    pub volume_id: String,
    pub path: PathBuf,
    pub files: u64,
    pub bytes: u64,
    pub examples: Vec<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review: Option<Review>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<PreviewSource>,
}

#[derive(Debug, Serialize)]
pub struct PreviewSource {
    pub rule_id: String,
    pub files: u64,
    pub blocked_by: Option<String>,
}

impl Finding {
    pub fn new(kind: Kind, volume_id: String, path: PathBuf, files: u64, bytes: u64) -> Self {
        let (label, evidence) = kind.description();
        let review_level = kind.review_level();
        Self {
            kind,
            identified: review_level != ReviewLevel::Suspected,
            review_level,
            label: label.into(),
            evidence,
            volume_id,
            path,
            files,
            bytes,
            examples: Vec::new(),
            review: None,
            preview: None,
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
            a.review_level
                .cmp(&b.review_level)
                .then_with(|| a.kind.cmp(&b.kind))
                .then_with(|| b.bytes.cmp(&a.bytes))
                .then_with(|| a.path.cmp(&b.path))
        });
    }
}

#[derive(Default)]
pub struct Groups(BTreeMap<(Kind, PathBuf, ReviewLevel), Finding>);

impl Groups {
    pub fn add(
        &mut self,
        kind: Kind,
        group: &Path,
        file: &Location,
        stamp: Stamp,
        level: ReviewLevel,
    ) {
        let finding = self
            .0
            .entry((kind, group.to_owned(), level))
            .or_insert_with(|| {
                let mut finding = Finding::new(kind, file.volume.clone(), group.to_owned(), 0, 0);
                if level == ReviewLevel::Confirmed {
                    finding.review_level = level;
                    finding.label = "已核验的旧用户临时文件".into();
                    finding.evidence = "匹配已验证的旧临时文件规则，当前文件状态通过复核。";
                }
                finding
            });
        finding.files += 1;
        finding.bytes = finding.bytes.saturating_add(stamp.size);
        if finding.examples.len() < 3
            && let Some(name) = file.relative.file_name()
        {
            finding.examples.push(name.into());
        }
    }

    pub fn append_to(self, report: &mut Report, volumes: &[Volume], label: Option<&str>) {
        report
            .findings
            .extend(self.0.into_values().map(|mut finding| {
                if let Some(label) = label {
                    finding.label = label.into();
                }
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

fn layout_marker(root: &Path, name: &str, directory: bool) -> bool {
    fs::symlink_metadata(root.join(name)).is_ok_and(|meta| {
        let expected_type = if directory {
            meta.is_dir()
        } else {
            meta.is_file()
        };
        expected_type && Stamp::from(&meta).attributes & windows::SKIP_ATTRIBUTES == 0
    })
}

fn chromium_index(root: &Path) -> bool {
    layout_marker(root, "index", false)
        && (layout_marker(root, "data_0", false) || layout_marker(root, "index-dir", true))
}

/// Inspect the markers of a cache directory once, regardless of its application or volume.
pub fn cache_layout(directory: &Location) -> Option<Kind> {
    let relative = &directory.relative;
    match relative.file_name()?.to_str()? {
        "Cache_Data" if relative.parent()?.file_name()? == "Cache" => {
            chromium_index(&directory.native()).then_some(Kind::BrowserCache)
        }
        "Code Cache" => {
            let root = directory.native();
            ["js", "wasm"]
                .into_iter()
                .any(|part| layout_marker(&root, part, true) && chromium_index(&root.join(part)))
                .then_some(Kind::BrowserCodeCache)
        }
        "cache2" => {
            let root = directory.native();
            (layout_marker(&root, "entries", true)
                && (layout_marker(&root, "index", false)
                    || layout_marker(&root, "index.log", false)))
            .then_some(Kind::FirefoxCache)
        }
        "_cacache" => {
            let root = directory.native();
            (layout_marker(&root, "content-v2", true) && layout_marker(&root, "index-v5", true))
                .then_some(Kind::NpmCache)
        }
        _ => None,
    }
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
