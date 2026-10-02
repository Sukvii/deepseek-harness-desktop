//! 核心来源判定与「当前活动核心」入口选择。
//!
//! 承载 [`CoreSource`] / [`HarnessCore`] 两个公开类型，以及
//! [`active_source`] / [`active_dsh_binary`] / [`active_version`] 三个供服务启动
//! 与插件操作统一取用的入口。本地核心探测见 [`super::local`]。

use crate::config;
use serde::Serialize;
use std::path::PathBuf;
use tauri::AppHandle;

use super::local::local_core;
use crate::service::download::parse_version_from_tag;

/// 核心来源
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CoreSource {
    /// 用户通过 CLI 安装的本地核心
    Local,
    /// 桌面端预打包核心
    App,
    /// WSL 发行版内的 dsh（桌面壳经 wsl.exe 拉起，见 `service::wsl_core`）
    Wsl,
}

impl CoreSource {
    pub fn as_str(self) -> &'static str {
        match self {
            CoreSource::Local => "local",
            CoreSource::App => "app",
            CoreSource::Wsl => "wsl",
        }
    }

    pub fn parse(source: &str) -> Option<CoreSource> {
        match source {
            "local" => Some(CoreSource::Local),
            "app" => Some(CoreSource::App),
            "wsl" => Some(CoreSource::Wsl),
            _ => None,
        }
    }
}

/// 核心列表项（序列化 camelCase 给前端）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessCore {
    /// `local` | `wsl` | `app`（无 tag 记录的旧激活行）| `app-<tag>`
    pub id: String,
    pub source: CoreSource,
    /// 版本号（不含 `v` 前缀；缺失为空串）
    pub version: String,
    /// 完整 release tag（如 `dsh-0.1.0-rc.8-32331963388`；local 行为空串）
    pub tag: String,
    /// 核心入口（cli path）：本地核心为 bin.js 绝对路径，预打包为安装目录
    pub path: String,
    /// 「打开目录」入口：本地核心为包目录，预打包为安装/槽位目录；未下载为空
    pub dir: String,
    /// 本地是否可用（文件在盘/可解析）
    pub present: bool,
    /// 当前是否使用中的核心
    pub active: bool,
    /// 是否预览版（GitHub Release 标记 Pre-release，或 tag 命名含预览标记，见
    /// `download::is_preview_tag`）：预览版不参与自动更新提示，但可在核心列表
    /// 手动下载安装，并以「预览版」标签展示。
    pub preview: bool,
    /// 当前版本是否高于资源清单中的推荐版本。
    pub above_recommended: bool,
    /// 本地存在但远程 pkg 仓库已不再提供的历史槽位。
    pub orphaned: bool,
    /// 资源清单中的推荐版本，用于切换前风险提示。
    pub recommended_version: Option<String>,
    pub error: Option<String>,
}

/// 纯函数形式的来源判定（便于单测）。
///
/// `wsl_supported` = 当前平台是否支持 WSL 核心（`cfg!(windows)`）；显式选择
/// `wsl` 但未选择发行版、或平台不支持时按自动来源回落（方案 W1.1/W1.2：
/// 归一化只发生在判定处，不改写 store 中的 `active_core`）。
fn resolve_active_source(
    active_core: Option<&str>,
    wsl_distro: Option<&str>,
    local_present: bool,
    wsl_supported: bool,
) -> CoreSource {
    match active_core.and_then(CoreSource::parse) {
        Some(CoreSource::Wsl) if wsl_supported && wsl_distro.is_some() => CoreSource::Wsl,
        Some(CoreSource::App) => CoreSource::App,
        // 显式选择本地但本地已失效 → 回退预打包
        Some(CoreSource::Local) if local_present => CoreSource::Local,
        // 未设置（自动）/ 显式 wsl 缺发行版 / 平台不支持：本地存在时优先
        _ => {
            if local_present {
                CoreSource::Local
            } else {
                CoreSource::App
            }
        }
    }
}

/// 当前活动核心来源（需求 3：本地核心存在时优先，除非用户显式选择预打包）。
pub fn active_source(app_handle: &AppHandle) -> CoreSource {
    let setting = config::get_store_dat_setting(app_handle);
    let local_present = local_core(app_handle).is_some();
    resolve_active_source(
        setting.active_core.as_deref(),
        setting.wsl_distro.as_deref(),
        local_present,
        cfg!(windows),
    )
}

/// `wsl.exe` 的绝对路径（`%SystemRoot%\System32\wsl.exe`）。
///
/// 定义已随 R-W4-2 移到 [`crate::service::wsl_core::exec`]——那里是所有 `wsl.exe`
/// 调用的收口点（探测 / 安装 / 停止 / 端口扫描 / 列发行版 / START 中继），单一来源
/// 避免 `wsl_core` → `core` 的反向引用；此处 `pub use` 转发，既有调用点不变。
pub use crate::service::wsl_core::exec::wsl_exe_path;

/// 当前活动核心是否为 WSL 核心。
///
/// 供上游「本机 node/dsh 文件」类检查（`active_dsh_binary` / `active_core_install_dir`
/// 派生的存在性判断、插件安装等）在进入文件系统逻辑前显式分支（W3 已落地各调用方：
/// R-1 追踪所得的 `start` / `launch` / `ensure_internal_plugins` / `plugin::install*`）。
pub fn is_wsl_active(app_handle: &AppHandle) -> bool {
    active_source(app_handle) == CoreSource::Wsl
}

/// 当前活动核心的 dsh 入口（bin.js 绝对路径）。
///
/// 供服务启动（workflow::launch）与插件操作（plugin::install 等）统一取用，
/// 本地核心解析在调用瞬间失效时回退预打包入口。
///
/// Wsl：返回 [`wsl_exe_path`] 的绝对路径，**只可用于展示与存在性探测**；实际拉起
/// 经 `wsl.exe -d <distro> -e bash -lc <START>`（W3），任何 `node <bin> …` 形态的
/// 调用方必须先经 [`is_wsl_active`] 分支。
pub fn active_dsh_binary(app_handle: &AppHandle) -> PathBuf {
    match active_source(app_handle) {
        CoreSource::Local => local_core(app_handle)
            .map(|c| c.bin)
            .unwrap_or_else(|| config::get_dsh_binary_path(app_handle)),
        CoreSource::App => config::get_dsh_binary_path(app_handle),
        CoreSource::Wsl => wsl_exe_path(),
    }
}

/// 当前活动核心的版本号（`--no-open` 等按版本判定的能力以它为准）。
pub fn active_version(app_handle: &AppHandle) -> Option<String> {
    match active_source(app_handle) {
        CoreSource::Local => local_core(app_handle).map(|c| c.version),
        CoreSource::App => config::get_dsh_pkg_tag(app_handle)
            .as_deref()
            .and_then(parse_version_from_tag)
            .or_else(|| config::get_dsh_version(app_handle)),
        // WSL 核心版本来自 W2 的探测缓存；无缓存返回 None（方案 W1.3）
        CoreSource::Wsl => config::get_store_dat_setting(app_handle)
            .wsl_distro
            .as_deref()
            .and_then(crate::service::wsl_core::probe::cached)
            .and_then(|probe| probe.dsh_version),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_source_round_trips() {
        assert_eq!(CoreSource::parse("local"), Some(CoreSource::Local));
        assert_eq!(CoreSource::parse("app"), Some(CoreSource::App));
        assert_eq!(CoreSource::parse("wsl"), Some(CoreSource::Wsl));
        assert_eq!(CoreSource::parse("other"), None);
        assert_eq!(CoreSource::Local.as_str(), "local");
        assert_eq!(CoreSource::App.as_str(), "app");
        assert_eq!(CoreSource::Wsl.as_str(), "wsl");
    }

    #[test]
    fn resolve_active_source_requires_distro_and_platform_for_wsl() {
        assert_eq!(
            resolve_active_source(Some("wsl"), Some("Ubuntu"), false, true),
            CoreSource::Wsl
        );
        assert_eq!(
            resolve_active_source(Some("wsl"), Some("Ubuntu"), true, true),
            CoreSource::Wsl
        );
        // 显式 wsl 但未选发行版 → 视为自动
        assert_eq!(
            resolve_active_source(Some("wsl"), None, true, true),
            CoreSource::Local
        );
        assert_eq!(
            resolve_active_source(Some("wsl"), None, false, true),
            CoreSource::App
        );
        // 非 Windows 平台不支持 WSL 核心 → 回落
        assert_eq!(
            resolve_active_source(Some("wsl"), Some("Ubuntu"), false, false),
            CoreSource::App
        );
        assert_eq!(
            resolve_active_source(Some("wsl"), Some("Ubuntu"), true, false),
            CoreSource::Local
        );
    }

    #[test]
    fn resolve_active_source_keeps_existing_rules() {
        assert_eq!(
            resolve_active_source(Some("app"), Some("Ubuntu"), true, true),
            CoreSource::App
        );
        assert_eq!(
            resolve_active_source(Some("local"), None, true, true),
            CoreSource::Local
        );
        // 显式本地但本地失效 → 回退预打包
        assert_eq!(
            resolve_active_source(Some("local"), None, false, true),
            CoreSource::App
        );
        assert_eq!(
            resolve_active_source(None, None, true, true),
            CoreSource::Local
        );
        assert_eq!(
            resolve_active_source(None, None, false, true),
            CoreSource::App
        );
        // 未知值按自动处理
        assert_eq!(
            resolve_active_source(Some("bogus"), None, true, true),
            CoreSource::Local
        );
    }
}
