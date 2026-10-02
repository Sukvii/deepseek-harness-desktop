//! WSL 核心桥接命令（方案 W2.6）。
//!
//! 发行版列举、环境探测与安装 / 升级。`set_active("wsl")` 的可用性闸门
//! （R-5）位于 `service/core/version.rs`，直接读 `wsl_core::probe::cached`。

use crate::service::wsl_core;
use crate::service::wsl_core::exec::WslDistro;
use crate::service::wsl_core::probe::WslCoreProbe;
use tauri::AppHandle;

/// 列举可用 WSL 发行版（已过滤 `docker-desktop*` 工具发行版）。
#[tauri::command]
pub async fn list_wsl_distros() -> Result<Vec<WslDistro>, String> {
    tauri::async_runtime::spawn_blocking(wsl_core::exec::list_distros)
        .await
        .map_err(|e| format!("WSL_LIST_JOIN_FAILED: {e}"))?
}

/// 探测指定发行版内的 node / npm / dsh 环境（成功后写入进程内缓存）。
#[tauri::command]
pub async fn probe_wsl_core(distro: String) -> Result<WslCoreProbe, String> {
    let distro = wsl_core::validate_distro(&distro)?.to_string();
    tauri::async_runtime::spawn_blocking(move || wsl_core::probe::probe(&distro))
        .await
        .map_err(|e| format!("WSL_PROBE_JOIN_FAILED: {e}"))?
}

/// 确保发行版内装好 dsh 并打 `--skip-auth` 补丁，返回最新探测结果；进度经
/// `install-progress` 事件逐行转发。
///
/// `version_spec` 缺省 = 桌面端推荐版本（`version-recommend.json`，D-W5R-2）：
/// WSL 核心与桌面核心共用同一份权威清单，缺失/无效时明确报错、**绝不回退
/// latest**（latest 可能是桌面端自己标记为 above-recommended 的预览版，装上后
/// 客户端资源不兼容——W5-R 验收实测 0.2.0-rc.2 的 client bundle 404）。
#[tauri::command]
pub async fn install_wsl_core(
    app_handle: AppHandle,
    distro: String,
    version_spec: Option<String>,
) -> Result<WslCoreProbe, String> {
    let distro = wsl_core::validate_distro(&distro)?.to_string();
    let spec = match version_spec {
        Some(spec) => wsl_core::validate_version_spec(&spec)?.to_string(),
        None => wsl_core::install::default_version_spec(&app_handle)?,
    };
    wsl_core::install::ensure(&app_handle, &distro, &spec).await
}

/// 把 Windows 侧 `~/.dsh[.dev]` 的 `DEEPSEEK_API_KEY` 导入发行版内的凭据文件
/// （W5 决策项 D-W5-1）。
///
/// 显式点击才执行（前端有确认对话框）：只搬这一个键，凭据文件其余内容不动、
/// 权限收紧到 0600，也**不自动同步**。返回 `false` = 目标已是同一个 key。
#[tauri::command]
pub async fn import_wsl_credentials(app_handle: AppHandle, distro: String) -> Result<bool, String> {
    let distro = wsl_core::validate_distro(&distro)?.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        wsl_core::credentials::import_api_key(&app_handle, &distro)
    })
    .await
    .map_err(|e| format!("WSL_CREDENTIAL_JOIN_FAILED: {e}"))?
}
