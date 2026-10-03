//! Harness 服务进程生命周期编排（workflow）。
//!
//! 模块划分：
//! - [`process`]：本应用持有的 Harness 根进程登记（PID + Windows 句柄成对）、
//!   启动守卫、停止/退出回收、进程树终止与按 dsh 安装路径清扫历史残留
//! - [`launch`]：start / restart / launch 编排（端口自愈、补丁挂点、
//!   Windows 隐藏控制台启动）；WSL 核心走 [`wsl_launch`]
//! - [`wsl_launch`]：WSL 核心的启动编排（`wsl.exe -e bash -lc` + 常量脚本、
//!   `STOP` 前置清扫、`skip_auth_ready` 闸门；仅 Windows）
//! - [`sweep`]：孤儿 Harness 清扫（`.harness.pid` + 端口/PID 双重确认）与
//!   Windows RedirectionGuard(448) 逃逸重拉
//! - [`install`]：安装环境（Node.js 运行时 + Harness 发行版 + pnpm + MinGit）
//! - [`health`]：健康检查（Rust 代理，避免 WebView CORS 问题）
//! - [`status`] / [`utils`] / [`win_inspector`] / [`win_spawn`]：既有子模块

pub mod status;
pub mod utils;
pub(crate) mod win_inspector;
#[cfg(windows)]
pub(crate) mod win_spawn;
// WSL 核心的启动编排（入口是 wsl.exe，仅 Windows 需要；方案 W3.1）
#[cfg(windows)]
mod wsl_launch;

mod health;
mod heap;
mod install;
mod launch;
mod process;
mod sweep;

pub use health::proxy_health_check;
pub use install::install;
pub use launch::{launch, restart, start};
pub use process::{
    acquire_core_transition, has_owned_process, owned_process_pid, owned_wsl_target, stop,
    stop_for_installer, stop_on_exit, terminate_stale_harness_processes,
};
pub use sweep::sweep_orphan_harness;
// R-V8-1：运行时切换回滚需要在发行版内停服（wsl_core::switch 的唯一跨模块入口）
#[cfg(windows)]
pub use wsl_launch::stop_in_distro;
