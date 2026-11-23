use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::discovery::{Finding, Kind, ReviewLevel};
use crate::store::digest;
use crate::windows::{self, Folder};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Node {
    pub id: String,
    pub label: String,
}

impl Node {
    fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }

    fn child(&self, key: &str, label: &str) -> Self {
        Self::new(format!("{}/{key}", self.id), label)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Review {
    pub owner: Node,
    pub feature: Node,
    pub purpose: Node,
    pub usage: String,
    pub impact: String,
}

impl Review {
    pub fn contains(&self, id: &str) -> bool {
        [&self.owner, &self.feature, &self.purpose]
            .into_iter()
            .any(|node| node.id == id)
    }

    pub fn selection(&self, id: &str) -> Option<Node> {
        let label = if self.owner.id == id {
            self.owner.label.clone()
        } else if self.feature.id == id {
            format!("{} / {}", self.owner.label, self.feature.label)
        } else if self.purpose.id == id {
            format!(
                "{} / {} / {}",
                self.owner.label, self.feature.label, self.purpose.label
            )
        } else {
            return None;
        };
        Some(Node::new(id, label))
    }
}

pub fn annotate(findings: &mut [Finding]) {
    let app_data: Vec<_> = [
        Folder::LocalAppData,
        Folder::RoamingAppData,
        Folder::ProgramData,
    ]
    .into_iter()
    .filter_map(|folder| folder.known().ok())
    .collect();
    for finding in findings {
        if finding.review_level == ReviewLevel::NeedsReview {
            finding.review = describe(finding.kind, &finding.path, &app_data);
        }
    }
}

fn describe(kind: Kind, path: &Path, app_data: &[PathBuf]) -> Option<Review> {
    let owner = match kind {
        Kind::BrowserCache => {
            let cache = if path.file_name()?.eq_ignore_ascii_case("Cache_Data") {
                path.parent()?
            } else {
                path
            };
            cache_owner(cache.parent()?, app_data)
        }
        Kind::BrowserCodeCache | Kind::BrowserComponentCache => {
            cache_owner(path.parent()?, app_data)
        }
        Kind::FirefoxCache => Node::new("firefox", "Firefox"),
        Kind::NpmCache => Node::new("npm", "npm"),
        Kind::PipCache => Node::new("pip", "pip"),
        Kind::RecycleBin | Kind::Thumbnails | Kind::CrashDumps | Kind::Minidump => {
            Node::new("windows", "Windows")
        }
        Kind::OldTemp => Node::new("temporary-folders", "临时目录"),
        _ => return None,
    };
    let (web_key, web_feature) = match owner.id.as_str() {
        "chrome" | "edge" | "firefox" | "brave" | "vivaldi" | "opera" | "opera-gx" | "chromium" => {
            ("web", "网页浏览缓存")
        }
        "qq" if path.iter().any(|part| {
            part.to_str()
                .is_some_and(|name| name.starts_with("qq-browser-"))
        }) =>
        {
            ("built-in-browser", "内置网页缓存")
        }
        _ => ("client-web", "客户端网页缓存"),
    };
    let (feature_key, feature_label, purpose_key, purpose_label, usage, impact) = match kind {
        Kind::BrowserCache | Kind::FirefoxCache => (
            web_key,
            web_feature,
            "resources",
            "网页资源缓存",
            "保存访问过的网页图片和其他资源，加快再次打开。",
            "后续打开网页时可能重新下载资源，首次加载可能变慢。",
        ),
        Kind::BrowserCodeCache => (
            web_key,
            web_feature,
            "scripts",
            "脚本缓存",
            "保存编译后的网页脚本，减少应用重复编译。",
            "再次打开时可能重新编译脚本，首次启动或打开网页可能变慢。",
        ),
        Kind::BrowserComponentCache => (
            "updates",
            "浏览器更新",
            "components",
            "组件更新缓存",
            "存放浏览器组件更新下载的安装包和索引。",
            "更新组件时可能重新下载。",
        ),
        Kind::RecycleBin => (
            "recovery",
            "文件恢复",
            "recycle-bin",
            "回收站",
            "暂存已删除的文件，供需要时恢复。",
            "清空后将失去通过回收站恢复本批文件的机会。",
        ),
        Kind::Thumbnails => (
            "explorer",
            "资源管理器",
            "thumbnails",
            "缩略图缓存",
            "为图片和视频等文件保存预览图。",
            "再次浏览相应文件时会重建，首次显示缩略图可能变慢。",
        ),
        Kind::CrashDumps | Kind::Minidump => (
            "diagnostics",
            "故障诊断",
            "crash-dumps",
            "崩溃转储",
            "记录应用崩溃时的状态，供定位故障。",
            "清理后，这些崩溃记录将无法用于排查。",
        ),
        Kind::NpmCache => (
            "packages",
            "软件包管理",
            "download-cache",
            "软件包下载缓存",
            "保存安装依赖时下载的软件包和响应。",
            "下次安装可能重新下载，离线安装可能受影响。",
        ),
        Kind::PipCache if path.file_name().is_some_and(|name| name == "wheels") => (
            "packages",
            "软件包管理",
            "wheel-cache",
            "已构建的软件包",
            "保存 pip 已构建的 wheel，供再次安装复用。",
            "再次安装时可能重新下载或构建。",
        ),
        Kind::PipCache => (
            "packages",
            "软件包管理",
            "download-cache",
            "软件下载缓存",
            "保存 pip 下载的索引响应和软件包。",
            "下次安装可能重新下载，离线安装可能受影响。",
        ),
        Kind::OldTemp => (
            "temporary",
            "临时文件",
            "old",
            "旧临时文件",
            "应用运行、安装或处理任务时生成的中间文件。",
            "仍依赖该文件的任务可能需要重新运行。",
        ),
        _ => return None,
    };
    let feature = owner.child(feature_key, feature_label);
    let purpose = feature.child(purpose_key, purpose_label);
    Some(Review {
        owner,
        feature,
        purpose,
        usage: usage.into(),
        impact: impact.into(),
    })
}

fn cache_owner(profile: &Path, app_data: &[PathBuf]) -> Node {
    for base in app_data {
        if let Some(relative) = windows::relative_to(profile, base) {
            let parts: Vec<_> = relative.iter().filter_map(|part| part.to_str()).collect();
            if let Some((first, rest)) = parts.split_first() {
                let vendor = [
                    "Google",
                    "Microsoft",
                    "Tencent",
                    "BraveSoftware",
                    "Opera Software",
                    "Mozilla",
                    "Packages",
                ]
                .iter()
                .any(|vendor| first.eq_ignore_ascii_case(vendor));
                if vendor && let Some(app) = rest.first() {
                    return app_data_owner(first, app);
                }
                return app_data_owner("", first);
            }
        }
    }
    let parts: Vec<_> = profile.iter().filter_map(|part| part.to_str()).collect();
    if let Some(parts) = parts.windows(3).find(|parts| {
        parts[0].eq_ignore_ascii_case("steamapps") && parts[1].eq_ignore_ascii_case("common")
    }) {
        return Node::new(
            format!("steam-app-{}", digest(parts[2].to_lowercase().as_bytes())),
            if parts[2].eq_ignore_ascii_case("wallpaper_engine") {
                "Wallpaper Engine"
            } else {
                parts[2]
            },
        );
    }
    let location = profile.to_string_lossy();
    let name = profile
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&location);
    Node::new(
        format!("directory-{}", digest(location.to_lowercase().as_bytes())),
        format!("{name}（来源待核实）"),
    )
}

fn app_data_owner(vendor: &str, name: &str) -> Node {
    let vendor = vendor.to_ascii_lowercase();
    match (vendor.as_str(), name.to_ascii_lowercase().as_str()) {
        ("google", "chrome") => Node::new("chrome", "Chrome"),
        ("microsoft", "edge") => Node::new("edge", "Edge"),
        ("microsoft", "windows") => Node::new("windows", "Windows"),
        ("bravesoftware", "brave-browser") => Node::new("brave", "Brave"),
        ("", "chromium") => Node::new("chromium", "Chromium"),
        ("", "vivaldi") => Node::new("vivaldi", "Vivaldi"),
        ("opera software", "opera stable") => Node::new("opera", "Opera"),
        ("opera software", "opera gx stable") => Node::new("opera-gx", "Opera GX"),
        ("", "qq") | ("tencent", "qq") => Node::new("qq", "QQ"),
        ("", "code") => Node::new("vscode", "VS Code"),
        ("", "obsidian") => Node::new("obsidian", "Obsidian"),
        ("", "steam") => Node::new("steam", "Steam"),
        ("", "spotify") => Node::new("spotify", "Spotify"),
        ("microsoft", "teams") => Node::new("teams", "Microsoft Teams"),
        _ => Node::new(
            format!(
                "appdata-{}",
                digest(format!("{vendor}/{}", name.to_lowercase()).as_bytes())
            ),
            name,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn related_uses_group_together_within_each_application() {
        let app_data = [
            PathBuf::from(r"C:\Users\Test\AppData\Local"),
            PathBuf::from(r"D:\Roaming"),
        ];
        let describe_at = |kind, path| describe(kind, Path::new(path), &app_data).unwrap();
        let resources = describe_at(
            Kind::BrowserCache,
            r"D:\Roaming\QQ\Partitions\qq-browser-1\Cache\Cache_Data",
        );
        let scripts = describe_at(
            Kind::BrowserCodeCache,
            r"D:\Roaming\QQ\Partitions\qq-browser-tab\Code Cache",
        );
        let client = describe_at(
            Kind::BrowserCache,
            r"C:\Users\Test\AppData\Local\Tencent\QQ\Cache\Cache_Data",
        );
        assert_eq!(resources.feature.id, scripts.feature.id);
        assert_ne!(resources.purpose.id, scripts.purpose.id);
        assert_eq!(resources.owner.id, client.owner.id);
        assert_ne!(resources.feature.id, client.feature.id);
        let browser = describe_at(
            Kind::BrowserCodeCache,
            r"C:\Users\Test\AppData\Local\Microsoft\Edge\User Data\Default\Code Cache",
        );
        let updates = describe_at(
            Kind::BrowserComponentCache,
            r"C:\Users\Test\AppData\Local\Microsoft\Edge\User Data\component_crx_cache",
        );
        assert_eq!(browser.owner.id, updates.owner.id);
        assert_ne!(browser.feature.id, updates.feature.id);
        assert_ne!(browser.impact, updates.impact);
    }
}
