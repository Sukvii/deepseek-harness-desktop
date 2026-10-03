//! 核心来源判定与「当前活动核心」入口选择。
//!
//! 承载 [`CoreSource`] / [`HarnessCore`] 两个公开类型，以及
//! [`active_source`] / [`active_dsh_binary`] / [`active_version`] 三个供服务启动
//! 与插件操作统一取用的入口。本地核心探测见 [`super::local`]。

use crate::config;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::OnceLock;
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
    /// 能否卸载：只有磁盘上存在独立槽位目录（`dependencies/<tag>`）的行才有卸载落点。
    /// 就地安装的激活副本只有 `dependencies/dsh` 一份目录，按 tag 找不到槽位，卸载必然
    /// 失败（issue #790）。
    pub removable: bool,
    /// 是否预览版（GitHub Release 标记 Pre-release，或 tag 命名含预览标记，见
    /// `download::is_preview_tag`）：预览版不参与自动更新提示，但可在核心列表
    /// 手动下载安装，并以「预览版」标签展示。
    pub preview: bool,
    /// 当前版本是否高于资源清单中的推荐版本。
    pub above_recommended: bool,
    /// 本地存在但远程 pkg 仓库已不再提供的历史槽位。
    pub orphaned: bool,
    /// 是否随安装包分发（离线包把内核托管到 `$Resources/dsh`）：核心面板把它置顶并
    /// 标记「本地」，且不提供卸载——它是内网/离线环境唯一的兜底内核。
    pub bundled: bool,
    /// 资源清单中的推荐版本，用于切换前风险提示。
    pub recommended_version: Option<String>,
    pub error: Option<String>,
}

/// 显式且有效的 WSL 选择（纯函数便于单测）：平台支持 + `active_core == "wsl"` +
/// 已选非空发行版。归一化只发生在判定处（W1.1/W1.2），不改写 store 中的值。
fn explicit_wsl_selection(
    active_core: Option<&str>,
    wsl_distro: Option<&str>,
    wsl_supported: bool,
) -> bool {
    wsl_supported
        && active_core == Some("wsl")
        && wsl_distro.is_some_and(|distro| !distro.trim().is_empty())
}

/// 基线告警只发一次：`active_source` 在启动、核心列表与插件操作中反复调用。
static UNSUPPORTED_LOCAL_WARNED: OnceLock<()> = OnceLock::new();

/// 版本是否达到最低支持基线（来自 `resources/manifest.jsonc` 的 `engines.dsh.minimum`）；
/// 基线缺失或任一版本无法解析时按「达标」处理。
fn meets_baseline(version: &str, baseline: &str) -> bool {
    match (
        semver::Version::parse(version),
        semver::Version::parse(baseline),
    ) {
        (Ok(actual), Ok(baseline)) => actual >= baseline,
        _ => true,
    }
}

/// 本地核心能否承载随包内置插件（基线取自清单 `engines.dsh.minimum`）。
///
/// 最低支持版本由随包清单声明；清单缺失（基线与版本都读不到）时不阻断，宁可放行
/// 也不把可用的本地核心判死——清单损坏的诊断由 `manifest` 模块记录。
pub(super) fn core_supports_bundled_plugins(version: &str, baseline: Option<&str>) -> bool {
    match baseline {
        Some(baseline) => meets_baseline(version, baseline),
        None => true,
    }
}

fn warn_unsupported_local_core(version: &str, baseline: Option<&str>) {
    if UNSUPPORTED_LOCAL_WARNED.set(()).is_err() {
        return;
    }
    log::warn!(
        "CORE_LOCAL_UNSUPPORTED: local dsh {} is below the minimum supported core {}; using the bundled core instead (issue #596)",
        version,
        baseline.unwrap_or("<unset>"),
    );
}

/// WSL 判定之后的上游来源规则（纯函数便于单测）：显式 App 直选；显式 Local 须
/// 可用；未设置或显式 Local 不可用时按「本地可用则本地，否则预打包」。
fn resolve_local_or_app(active_core: Option<&str>, local_usable: bool) -> CoreSource {
    match active_core.and_then(CoreSource::parse) {
        Some(CoreSource::App) => CoreSource::App,
        Some(CoreSource::Local) if local_usable => CoreSource::Local,
        _ => {
            if local_usable {
                CoreSource::Local
            } else {
                CoreSource::App
            }
        }
    }
}

/// 当前活动核心来源（需求 3：本地核心存在时优先，除非用户显式选择预打包）。
///
/// 显式且有效的 WSL 选择最先处理（U3.1：平台门控 + 已选发行版），先于本地核心
/// 判定——WSL 核心不受本机最低支持基线约束，也不改写 store。其余沿用上游规则：
/// 本地核心低于最低支持版本时不参与优先，一律回退预打包核心；内置插件是桌面壳的
/// 组成部分，装上也无法加载，只会把启动卡在插件阶段（issue #596）。
pub fn active_source(app_handle: &AppHandle) -> CoreSource {
    let setting = config::get_store_dat_setting(app_handle);
    if explicit_wsl_selection(
        setting.active_core.as_deref(),
        setting.wsl_distro.as_deref(),
        cfg!(windows),
    ) {
        return CoreSource::Wsl;
    }
    let local = local_core(app_handle);
    let baseline = config::manifest::minimum_dsh_version(app_handle);
    let local_usable = local
        .as_ref()
        .is_some_and(|core| core_supports_bundled_plugins(&core.version, baseline.as_deref()));
    if local.is_some() && !local_usable {
        warn_unsupported_local_core(
            local
                .as_ref()
                .map(|core| core.version.as_str())
                .unwrap_or_default(),
            baseline.as_deref(),
        );
    }
    resolve_local_or_app(setting.active_core.as_deref(), local_usable)
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

/// 当前生效的核心是否就是随安装包分发的那份（离线包把内核托管到 `$Resources/dsh`）。
///
/// 判定「来源是 App **且** 生效根就是随包内核目录」：切到本地核心时映射被记为系统环境、
/// 生效根回落到随包目录，此时不能再算作随包内核。
pub fn active_is_bundled(app_handle: &AppHandle) -> bool {
    active_source(app_handle) == CoreSource::App
        && config::dependencies::bundled_core_dir(app_handle).is_some_and(|dir| {
            config::dependencies::active_root(app_handle, config::dependencies::DEP_DSH) == dir
        })
}

/// 当前活动核心的版本号（`--no-open` 等按版本判定的能力以它为准）。
pub fn active_version(app_handle: &AppHandle) -> Option<String> {
    match active_source(app_handle) {
        CoreSource::Local => local_core(app_handle).map(|c| c.version),
        CoreSource::App => {
            // 随包内核没有对应的 pkg tag：store 里的 tag 属于上一次下载的版本，直接采用
            // 会把随包内核当成那个版本（能力判定、补丁与插件兼容性都会走错分支）。
            if active_is_bundled(app_handle) {
                return config::get_dsh_version(app_handle);
            }
            config::get_dsh_pkg_tag(app_handle)
                .as_deref()
                .and_then(parse_version_from_tag)
                .or_else(|| config::get_dsh_version(app_handle))
        }
        // WSL 核心版本来自 W2 的探测缓存；无缓存返回 None，不回落 Windows 版本（W1.3 / U3.1）
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
    fn explicit_wsl_selection_requires_platform_flag_and_distro() {
        // 支持平台 + 显式 wsl + 非空发行版 → 生效
        assert!(explicit_wsl_selection(Some("wsl"), Some("Ubuntu"), true));
        // 非 Windows → 不生效（回落上游来源规则）
        assert!(!explicit_wsl_selection(Some("wsl"), Some("Ubuntu"), false));
        // 未选发行版 / 空串 / 纯空白 → 视为自动
        assert!(!explicit_wsl_selection(Some("wsl"), None, true));
        assert!(!explicit_wsl_selection(Some("wsl"), Some(""), true));
        assert!(!explicit_wsl_selection(Some("wsl"), Some("   "), true));
        // 其它来源值不受影响
        assert!(!explicit_wsl_selection(Some("local"), Some("Ubuntu"), true));
        assert!(!explicit_wsl_selection(None, Some("Ubuntu"), true));
    }

    #[test]
    fn resolve_local_or_app_follows_upstream_rules() {
        // 显式 App 直选
        assert_eq!(resolve_local_or_app(Some("app"), true), CoreSource::App);
        // 显式 Local 且可用
        assert_eq!(resolve_local_or_app(Some("local"), true), CoreSource::Local);
        // 显式 Local 但本地过低/失效 → 回退预打包（最低版本门禁只作用于本地核心）
        assert_eq!(resolve_local_or_app(Some("local"), false), CoreSource::App);
        // 自动：本地可用优先，否则预打包
        assert_eq!(resolve_local_or_app(None, true), CoreSource::Local);
        assert_eq!(resolve_local_or_app(None, false), CoreSource::App);
        // 残余 wsl（未选发行版，早退未命中）按自动处理
        assert_eq!(resolve_local_or_app(Some("wsl"), true), CoreSource::Local);
        assert_eq!(resolve_local_or_app(Some("wsl"), false), CoreSource::App);
        // 未知值按自动处理
        assert_eq!(resolve_local_or_app(Some("bogus"), true), CoreSource::Local);
    }

    #[test]
    fn baseline_gate_accepts_equal_or_newer_versions() {
        let baseline = "0.1.5-rc.1";
        assert!(meets_baseline("0.1.5-rc.1", baseline));
        assert!(meets_baseline("0.1.5-rc.2", baseline));
        assert!(meets_baseline("0.1.6-alpha.2", baseline));
    }

    /// issue #596：更早的核心（0.1.2-rc.1 / 0.1.0-rc.7）早于内置插件依赖的平台
    /// 种子词，内置插件必然加载失败，不再支持。
    #[test]
    fn baseline_gate_rejects_older_versions() {
        let baseline = "0.1.5-rc.1";
        assert!(!meets_baseline("0.1.0-rc.7", baseline));
        assert!(!meets_baseline("0.1.2-rc.1", baseline));
        assert!(!meets_baseline("0.1.5-alpha.2", baseline));
    }

    /// 版本不可解析（旧安装记录/异常清单）时不阻断：漏放行只是回到修复前的行为，
    /// 误拦截会把可用的本地核心判死。
    #[test]
    fn baseline_gate_passes_unparsable_versions() {
        assert!(meets_baseline("", "0.1.5-rc.1"));
        assert!(meets_baseline("not-a-version", "0.1.5-rc.1"));
        assert!(meets_baseline("0.1.0-rc.7", ""));
    }

    /// 清单缺失（读不到最低支持基线）时同样放行，避免资源损坏阻断本地核心。
    #[test]
    fn missing_baseline_keeps_local_core_usable() {
        assert!(core_supports_bundled_plugins("0.1.0-rc.7", None));
        assert!(!core_supports_bundled_plugins(
            "0.1.0-rc.7",
            Some("0.1.5-rc.1")
        ));
    }
}
