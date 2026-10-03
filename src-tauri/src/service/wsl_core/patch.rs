//! WSL 核心的 `--skip-auth` 补丁：对 UNC 路径直接读写 WSL 内的 dsh 文件。
//!
//! 复用 [`super::auth_patch`] 的两个纯字符串补丁函数；目标文件经
//! `\\wsl.localhost\<distro>\<受控运行时 node_modules>\…` 的 UNC 路径访问（本机可
//! 直接读写）。候选相对路径的**根**由调用方给出（v8：正式运行时或候选运行时），
//! 本模块只认这一个根，不自行推导。

use super::auth_patch::{patch_connection, patch_startup};
use crate::utils::{patch_file_at, PatchOutcome};
use std::path::PathBuf;

/// 补丁目标候选（相对受控运行时的 `node_modules`），取第一个存在者。
///
/// 候选与 [`super::script::PROBE`] 的 `SKIPAUTH` 判定**完全一致**（含顺序；单测
/// 交叉锁定），选出的就是 dsh 运行时实际加载的那一份（Node 解析优先序）。
/// 真实样本（R-W5-3 隔离安装与 v8 受控 `npm ci` 实测）：
/// - 受控树（v8，manifest overrides + lock）：`dsh-web-app` 与 `dsh-client-connection`
///   提升到顶层 `node_modules`（扁平档命中）；
/// - `0.1.2-rc.1` 全局安装：connection 在 `@deepseek-ai/dsh/node_modules/
///   @deepseek-ai/dsh-client-connection`（dsh 直属嵌套）；
/// - `0.1.5-rc.2`：connection 嵌在 `dsh-web-app/node_modules/@deepseek-ai/` 下。
///
/// 与历史桌面补丁（`alpha_auth`，U2 已删）的相对路径口径一致（那边以 dsh 包根为
/// 基，此处以运行时 `node_modules` 为基）。
const WEB_STARTUP_RELS: [&str; 2] = [
    "@deepseek-ai/dsh/node_modules/@deepseek-ai/dsh-web-app/lib/startup.js",
    "@deepseek-ai/dsh-web-app/lib/startup.js",
];
const CONNECTION_INDEX_RELS: [&str; 3] = [
    "@deepseek-ai/dsh/node_modules/@deepseek-ai/dsh-web-app/node_modules/@deepseek-ai/dsh-client-connection/lib/index.js",
    "@deepseek-ai/dsh/node_modules/@deepseek-ai/dsh-client-connection/lib/index.js",
    "@deepseek-ai/dsh-client-connection/lib/index.js",
];

/// UNC 根：`\\wsl.localhost\<distro>`。
fn wsl_unc_root(distro: &str) -> String {
    format!("\\\\wsl.localhost\\{distro}")
}

/// Linux 绝对路径 → UNC 路径（`/home/x` → `\\wsl.localhost\<distro>\home\x`）。
///
/// `pub(crate)`（U7.2）：`bridge::guard::allowed_roots` 需要用它构造 WSL 数据根的
/// 允许根（仅限已探测 home 下的本应用数据根，不放开整个 home / 发行版共享根）。
pub(crate) fn wsl_unc_path(distro: &str, linux_path: &str) -> String {
    format!("{}{}", wsl_unc_root(distro), linux_path.replace('/', "\\"))
}

/// 单个目标的命中判定：刚打上（`Patched`）与此前已打（`AlreadyPatched`）都算命中；
/// 文件缺失 / 锚点缺失视为未命中。
fn hit(outcome: Option<PatchOutcome>) -> bool {
    matches!(
        outcome,
        Some(PatchOutcome::Patched(_)) | Some(PatchOutcome::AlreadyPatched)
    )
}

/// 按候选相对路径定位目标并打补丁；全部候选都不存在时告警并视为未命中。
fn patch_first<F>(distro: &str, npm_root: &str, rels: &[&str], patch: F) -> Result<bool, String>
where
    F: FnOnce(&str) -> PatchOutcome + Copy,
{
    for rel in rels {
        let path = PathBuf::from(wsl_unc_path(distro, &format!("{npm_root}/{rel}")));
        if !path.exists() {
            continue;
        }
        return Ok(hit(patch_file_at(&path, patch)?));
    }
    log::warn!("[wsl-core] patch target not found under {npm_root} for {distro}");
    Ok(false)
}

/// 对 WSL 内指定运行时的 dsh 打两层补丁（web 启动选项 + connection 鉴权闸）。
///
/// `npm_root` = 目标运行时的 `node_modules` 绝对路径：正式运行时是
/// `$HOME/<数据目录名>/runtime/node_modules`，候选是 `runtime-candidate/node_modules`
/// （v8 的候选安装先打补丁再验证、切换）。**只操作受控树**，不碰用户全局安装。
///
/// 返回 `true` 表示两层都已命中（`START` 脚本固定带 `--skip-auth`，未命中即
/// dsh 收到未知选项会直接退出——W4 的降级路径）。
pub fn apply(distro: &str, npm_root: &str) -> Result<bool, String> {
    if !npm_root.starts_with('/') {
        log::warn!(
            "[wsl-core] managed runtime root unavailable, skip --skip-auth patch for {distro}"
        );
        return Ok(false);
    }
    let startup_hit = patch_first(distro, npm_root, &WEB_STARTUP_RELS, patch_startup)?;
    let connection_hit = patch_first(distro, npm_root, &CONNECTION_INDEX_RELS, patch_connection)?;
    log::info!(
        "[wsl-core] --skip-auth patch for {distro}: startup={startup_hit}, connection={connection_hit} (root {npm_root})"
    );
    Ok(startup_hit && connection_hit)
}

#[cfg(test)]
mod tests {
    use super::{hit, wsl_unc_path, CONNECTION_INDEX_RELS, WEB_STARTUP_RELS};
    use crate::utils::PatchOutcome;

    #[test]
    fn unc_path_maps_linux_absolute_paths() {
        assert_eq!(
            wsl_unc_path("Ubuntu", "/home/pixel"),
            "\\\\wsl.localhost\\Ubuntu\\home\\pixel"
        );
        assert_eq!(
            wsl_unc_path("My Dev Box", "/root"),
            "\\\\wsl.localhost\\My Dev Box\\root"
        );
        assert!(wsl_unc_path("Ubuntu", "/home/pixel").ends_with("\\pixel"));
        assert!(wsl_unc_path("Ubuntu", "/root").starts_with("\\\\wsl.localhost\\"));
    }

    #[test]
    fn hit_counts_patched_and_already_patched_only() {
        assert!(hit(Some(PatchOutcome::AlreadyPatched)));
        assert!(hit(Some(PatchOutcome::Patched("x".to_string()))));
        assert!(!hit(Some(PatchOutcome::AnchorMissing)));
        assert!(!hit(None));
    }

    #[test]
    fn web_startup_rels_cover_nested_and_flat_npm_layouts() {
        assert!(WEB_STARTUP_RELS[0].contains("@deepseek-ai/dsh/node_modules/@deepseek-ai/"));
        assert!(WEB_STARTUP_RELS[0].ends_with("dsh-web-app/lib/startup.js"));
        assert!(WEB_STARTUP_RELS[1].starts_with("@deepseek-ai/dsh-web-app/"));
        assert!(CONNECTION_INDEX_RELS[0].ends_with("dsh-client-connection/lib/index.js"));
    }

    /// D-W5R-3：connection 候选覆盖三种真实布局，且顺序 = Node 解析优先序
    /// （web-app 嵌套 > dsh 直属嵌套 > 扁平）——「第一个存在」即运行时实际加载。
    #[test]
    fn connection_rels_follow_node_resolution_order() {
        assert!(CONNECTION_INDEX_RELS[0].contains(
            "@deepseek-ai/dsh/node_modules/@deepseek-ai/dsh-web-app/node_modules/@deepseek-ai/"
        ));
        assert!(CONNECTION_INDEX_RELS[1]
            .starts_with("@deepseek-ai/dsh/node_modules/@deepseek-ai/dsh-client-connection"));
        assert!(CONNECTION_INDEX_RELS[2].starts_with("@deepseek-ai/dsh-client-connection"));
    }

    /// D-W5R-3：补丁候选必须与 [`crate::service::wsl_core::script::PROBE`] 的
    /// `SKIPAUTH` 判定逐字一致（两边各自手写候选，漂移过一次：补丁打对了、
    /// 探测找不到，`skip_auth_ready` 与真实状态相反）。
    #[test]
    fn patch_targets_match_probe_candidates_exactly() {
        use crate::service::wsl_core::script::PROBE;
        for rel in WEB_STARTUP_RELS.iter().chain(CONNECTION_INDEX_RELS.iter()) {
            assert!(PROBE.contains(rel), "PROBE 与 patch 候选不一致: {rel}");
        }
    }
}
