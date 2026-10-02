use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager, Runtime, WebviewWindow};

use crate::config;
use crate::service::core::{active_source, local_core_package_dir, CoreSource};

/// 对某个 dsh 包文件的一次性幂等补丁判定结果。
#[derive(Debug, PartialEq, Eq)]
pub enum PatchOutcome {
    /// 目标已含补丁标记（本补丁已生效，或上游官方已合并），无需再改。
    AlreadyPatched,
    /// 锚点缺失（上游布局变更），跳过并向调用方说明降级兜底。
    AnchorMissing,
    /// 已生成补丁后的完整内容。
    Patched(String),
}

/// 活动核心安装目录：本地核心用其包目录（全局安装路径），预打包用桌面端目录。
///
/// 与 [`crate::service::core::active_dsh_binary`] 的取舍一致——本地核心解析在调用
/// 瞬间失效时回退预打包目录，绝不让补丁打到永不加载的预打包文件上。
///
/// WSL 核心没有本机安装目录（其 `--skip-auth` 补丁由 W2 经 UNC 直接改 WSL 内文件），
/// 返回 `None`，调用方按「不适用」跳过。
fn active_core_install_dir(app_handle: &tauri::AppHandle) -> Option<PathBuf> {
    match active_source(app_handle) {
        CoreSource::Local => Some(
            local_core_package_dir(app_handle)
                .unwrap_or_else(|| config::get_dsh_install_path(app_handle)),
        ),
        CoreSource::App => Some(config::get_dsh_install_path(app_handle)),
        CoreSource::Wsl => None,
    }
}

/// 对活动核心安装目录下的某个 dsh 包文件应用一次性幂等补丁。
///
/// - `rel_path`：相对活动核心安装目录的包内路径，例如
///   `node_modules/@deepseek-ai/dsh-client-ui-renderer/lib/client.js`（即 `patch_dsh("packagename/xxx/xxx.js", ..)` 里的包路径）。
/// - `patch`：纯函数式补丁判定，输入文件原文、返回 [`PatchOutcome`]；只做内容变换，
///   不触碰文件系统，便于单测。
///
/// 统一处理「定位文件 → 读取 → 打补丁 → 写回」与对应的日志。文件缺失、已打过、
/// 锚点变更均静默跳过并返回 Ok；只有真实读/写失败才返回 Err（不阻断启动的调用方
/// 据此仅告警）。活动核心的判定与 [`active_core_install_dir`] 一致。
pub fn patch_dsh(
    app_handle: &tauri::AppHandle,
    rel_path: &str,
    patch: impl FnOnce(&str) -> PatchOutcome,
) -> Result<(), String> {
    let Some(target) = active_core_install_dir(app_handle).map(|dir| dir.join(rel_path)) else {
        log::info!("dsh patch not applicable for current core, skip: {rel_path}");
        return Ok(());
    };
    match patch_file_at(&target, patch)? {
        None => log::info!("dsh patch target not found, skip: {}", target.display()),
        Some(PatchOutcome::AlreadyPatched) => {
            log::info!("dsh patch already applied: {}", target.display());
        }
        Some(PatchOutcome::AnchorMissing) => {
            log::warn!("dsh patch anchor missing, skip: {}", target.display());
        }
        Some(PatchOutcome::Patched(_)) => {
            log::info!("dsh patch applied: {}", target.display());
        }
    }
    Ok(())
}

/// 对**任意路径**应用一次性幂等补丁（[`patch_dsh`] 的通用版；W2.4 供 WSL 的
/// UNC 文件使用）。
///
/// - 目标不存在 → `Ok(None)`（调用方自行决定日志与跳过）；
/// - 三态 `PatchOutcome`：`Patched` 时已写回文件，`AlreadyPatched` / `AnchorMissing`
///   原样返回（不写文件）；
/// - 读 / 写失败 → `Err`。
pub fn patch_file_at(
    path: &Path,
    patch: impl FnOnce(&str) -> PatchOutcome,
) -> Result<Option<PatchOutcome>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let source = std::fs::read_to_string(path)
        .map_err(|e| format!("DSH_PATCH_READ: {} failed: {e}", path.display()))?;
    let outcome = patch(&source);
    if let PatchOutcome::Patched(patched) = &outcome {
        std::fs::write(path, patched)
            .map_err(|e| format!("DSH_PATCH_WRITE: {} failed: {e}", path.display()))?;
    }
    Ok(Some(outcome))
}

/// 判定活动核心安装目录下的某个 dsh 包文件是否包含给定子串。
///
/// 用于「按能力追加启动参数」：例如 web 启动命令已具备 `--skip-auth`（本工具已打
/// 过补丁或上游官方合并）才向服务参数追加该标志。目标不存在或读取失败一律视为
/// 不包含，调用方据此保守不传标志；WSL 核心（无本机安装目录）同样视为不包含。
pub fn dsh_rel_contains(app_handle: &tauri::AppHandle, rel_path: &str, needle: &str) -> bool {
    let Some(target) = active_core_install_dir(app_handle).map(|dir| dir.join(rel_path)) else {
        return false;
    };
    match std::fs::read_to_string(&target) {
        Ok(content) => content.contains(needle),
        Err(_) => false,
    }
}

pub fn show_window<R: Runtime>(window: &WebviewWindow<R>) {
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// 显示主窗口：托盘「打开面板」、托盘左键点击、macOS Dock 图标点击共用。
/// 关闭按钮只隐藏窗口（见 builder 的 on_window_event），所以这里取到即可 show；
/// 若窗口确实不存在（非预期路径），仅记录日志，不重建。
pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        // 关窗驻留用的是应用级 hide（NSApp hide:，见 builder 的 CloseRequested），
        // 恢复前必须先 unhide 整个应用，否则 window.show() 在隐藏态应用上不可见。
        #[cfg(target_os = "macos")]
        if let Err(error) = app.show() {
            log::warn!("[window] APP_SHOW_FAILED: {error}");
        }
        // Accessory 下 window.show() 有历史问题（tauri #5122），必须先切回
        // regular 再 show；放在这里可一次覆盖托盘菜单「打开面板」、托盘左键、
        // RunEvent::Reopen、release single-instance 四条恢复路径。
        #[cfg(target_os = "macos")]
        crate::desktop::activation::set_regular_policy(app);
        // 恢复路径不改变全屏状态：全屏关窗后从托盘恢复，窗口依旧全屏。
        // 不在此重新挂起 Accessory：窗口此时已可见，若挂起推迟标志，用户
        // 退出全屏时 on_window_resized 会把可见应用切进 Accessory，导致 Dock
        // 图标与 ⌘-Tab 在窗口仍打开时消失。Accessory 只在关窗驻留路径的
        // hide 之后生效，前台可见态应保持 regular。
        show_window(&window);
        // 恢复路径不改变全屏状态：全屏关窗后从托盘恢复，窗口依旧全屏，
        // 而 set_regular_policy 已无条件清掉推迟标志。这里按需重新挂起，
        // 否则用户退出全屏时 Accessory 永不生效（Dock / ⌘-Tab 整个驻留期可见）。
        #[cfg(target_os = "macos")]
        crate::desktop::activation::rearm_pending_accessory_if_fullscreen(&window);
    } else {
        log::warn!("[window] main window not found, skip show");
    }
}

pub fn app_icon_temp_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    let icon = app.default_window_icon()?;
    let path = std::env::temp_dir().join(format!("dsh-notification-{}.png", std::process::id()));
    let rgba = icon.rgba().to_vec();
    let img = image::RgbaImage::from_raw(icon.width(), icon.height(), rgba)?;
    img.save(&path).ok()?;
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::{patch_file_at, PatchOutcome};

    #[test]
    fn patch_file_at_handles_missing_and_three_outcomes() {
        let dir = std::env::temp_dir().join(format!("dsh-patch-file-at-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("sample.js");
        let patch = |source: &str| {
            if source.contains("PATCHED") {
                PatchOutcome::AlreadyPatched
            } else if source.contains("anchor") {
                PatchOutcome::Patched(source.replace("anchor", "PATCHED"))
            } else {
                PatchOutcome::AnchorMissing
            }
        };

        // 目标缺失：Ok(None)，不创建文件
        assert_eq!(patch_file_at(&target, patch).unwrap(), None);
        assert!(!target.exists());

        // Patched：写回文件
        std::fs::write(&target, "anchor line\n").unwrap();
        assert!(matches!(
            patch_file_at(&target, patch).unwrap(),
            Some(PatchOutcome::Patched(_))
        ));
        assert!(std::fs::read_to_string(&target)
            .unwrap()
            .contains("PATCHED"));

        // AlreadyPatched：不再改写
        assert_eq!(
            patch_file_at(&target, patch).unwrap(),
            Some(PatchOutcome::AlreadyPatched)
        );

        // AnchorMissing：跳过且保留原文
        std::fs::write(&target, "no markers here\n").unwrap();
        assert_eq!(
            patch_file_at(&target, patch).unwrap(),
            Some(PatchOutcome::AnchorMissing)
        );
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "no markers here\n"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
