//! WSL 核心的启动编排（方案 W3.1）。
//!
//! 与 Windows 侧 [`super::launch`] 的差别：入口是 `wsl.exe` 而不是本机 node/dsh
//! 文件，因此不做任何本机文件存在性检查；启动前后都经 `wsl.exe -e bash -lc`
//! 在发行版内执行常量脚本（[`script::START`] / [`script::STOP`]），动态值只作
//! 位置参数（方案 §0.3 规则 1）。
//!
//! 停止闭环（F5）：杀 `wsl.exe` 中继不会杀 Linux 子进程，因此停止路径必须先
//! 在 Linux 侧按进程组 kill（`STOP`，R-W2-7），Rust 侧的 [`super::process`]
//! 负责顺序。

use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::AppHandle;

use super::process::{
    has_owned_process, on_owned_process_exit, set_owned_wsl_process_with_handle, LaunchGuard,
    WslTarget, LAUNCH_GUARD,
};
use super::utils::{rotate_service_log, spawn_output_readers};
use super::win_spawn;
use crate::config;
use crate::service::core::wsl_exe_path;
use crate::service::wsl_core::{dsh_home_dir_name, exec, probe, script};

/// `preflight` 允许复用探测缓存的最大年龄（R-W3-6）：开机路径上
/// `runtime_ready` 与 `launch` 相隔仅几百毫秒。
pub const PREFLIGHT_MAX_AGE: Duration = Duration::from_secs(15);

/// `STOP` 的中继超时。Linux 侧自带 `sleep 1` 与 `-KILL` 兜底（秒级完成），
/// 但发行版处于 Stopped 时首次 `wsl.exe -e` 访问要先冷启动 VM——本机实测
/// 冷启动会让 3 s 的中继超时在启动路径上先到期（D-W3-1），因此留 15 s。
pub const STOP_TIMEOUT: Duration = Duration::from_secs(15);

/// `PORT_SCAN` 的中继超时。Linux 侧实测 0.18 s，留出冷启动 VM 的余量（同 STOP）。
const PORT_SCAN_TIMEOUT: Duration = Duration::from_secs(15);

/// 端口被占时最多向后尝试的个数（与 Windows 分支 `find_available_port` 的
/// 逐级递增语义一致；`PORT_SCAN` 用尽即以 98 退出）。
const PORT_SCAN_ATTEMPTS: u32 = 10;

/// WSL 侧 dsh 数据目录名（相对 `$HOME`）。
///
/// **只传目录名**：`$HOME` 必须由发行版内的 bash 展开（[`script::START`] /
/// [`script::STOP`] 里拼 `"$HOME/$1"`）。Windows 侧若传 `$HOME/.dsh-desktop.dev`
/// 这样的字面量，Linux 侧只会把它当普通字符，创建出名为 `$HOME` 的目录——
/// 启动/停止/探测三处路径随之错位（D-W3-2）。
///
/// 单一来源是 [`dsh_home_dir_name`]（debug 为 `.dsh-desktop.dev`），与 W2 的
/// 探测 / 安装路径一致。
pub fn wsl_dsh_home() -> &'static str {
    dsh_home_dir_name()
}

/// 组装 `wsl.exe` 的启动 argv（纯函数，便于单测）。
///
/// 形态固定为 `-d <distro> -e bash -lc <START> bash <dsh_home> <port>`：
/// 只用 `-e`（exec，不经登录 shell 二次展开），脚本是常量，动态值只作位置参数
/// （方案 §0.3 规则 1）。
pub fn wsl_start_args(distro: &str, dsh_home: &str, port: u16) -> Vec<OsString> {
    vec![
        OsString::from("-d"),
        OsString::from(distro),
        OsString::from("-e"),
        OsString::from("bash"),
        OsString::from("-lc"),
        OsString::from(script::START),
        OsString::from("bash"),
        OsString::from(dsh_home),
        OsString::from(port.to_string()),
    ]
}

/// 停止 WSL 侧的 Harness（按进程组 kill；幂等，无进程时也返回 Ok）。
///
/// `dsh_home` 是相对 `$HOME` 的目录名（见 [`wsl_dsh_home`]）。同步阻塞 ≤
/// [`STOP_TIMEOUT`]（含发行版 Stopped 时的冷启动），调用方负责放进
/// `spawn_blocking`（退出路径除外，见 `process::stop_on_exit`）。
pub fn stop_in_distro(distro: &str, dsh_home: &str) -> Result<(), String> {
    exec::run_in_distro(distro, script::STOP, &[dsh_home], STOP_TIMEOUT).map(|_| ())
}

/// 解析 `PORT_SCAN` 的输出：恰好一行可解析为 `u16` 的端口号（0 不算）。
fn parse_port_scan_output(stdout: &str) -> Result<u16, String> {
    let lines: Vec<&str> = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    match lines.as_slice() {
        [one] => one
            .parse::<u16>()
            .ok()
            .filter(|port| *port != 0)
            .ok_or_else(|| format!("WSL_PORT_SCAN_INVALID: unexpected output: {stdout:?}")),
        _ => Err(format!(
            "WSL_PORT_SCAN_INVALID: unexpected output: {stdout:?}"
        )),
    }
}

/// 在发行版内扫描端口：从 `start` 起最多试 `attempts` 个，返回第一个可绑定端口；
/// 全部被占（`PORT_SCAN` 以 98 退出）返回 `Ok(None)`。
async fn scan_port(distro: &str, start: u16, attempts: u32) -> Result<Option<u16>, String> {
    let distro_owned = distro.to_string();
    let start_arg = start.to_string();
    let attempts_arg = attempts.to_string();
    let output = tauri::async_runtime::spawn_blocking(move || {
        exec::run_in_distro(
            &distro_owned,
            script::PORT_SCAN,
            &[start_arg.as_str(), attempts_arg.as_str()],
            PORT_SCAN_TIMEOUT,
        )
    })
    .await
    .map_err(|e| format!("WSL_PORT_SCAN_JOIN_FAILED: {e}"))??;
    if output.code == 98 {
        return Ok(None);
    }
    if output.code != 0 {
        return Err(format!(
            "WSL_PORT_SCAN_FAILED: exit {}: {}",
            output.code,
            exec::summarize(&output)
        ));
    }
    parse_port_scan_output(&exec::decode_auto(&output.stdout)).map(Some)
}

/// 端口自愈与冲突避让的 WSL 版（R-W3-2）：判定全部在发行版内做。
///
/// 与 Windows 分支 [`super::launch::launch`] 的内联端口自愈步骤一一对应——先 heal（配置端口
/// 已被自动避让顶高、回落目标又空闲时回落），再从当前值起找第一个可绑定端口，
/// 每次变更都持久化并打同样的日志。差别只在占用判定改用 `PORT_SCAN`
/// （Linux 侧 `bind`）：镜像网络下 Windows 的 `TcpListener::bind` 看不见 Linux
/// 侧 TIME_WAIT，会让 WSL 内 dsh 直接 `EADDRINUSE` 退出（D-W3-6 / F12）。
/// Linux 侧监听 socket 随进程死亡立即释放，Linux 自身的 TIME_WAIT 又被 node 的
/// `SO_REUSEADDR` 绕过，因此不需要 `wait_for_port_release` 的等待窗口。
///
/// 持久化用**精确写入** `update_store_dat_setting(|s| s.port = …)`（R-W4-4，同
/// W1 审核 R-3 的口径）：本函数在 `preflight`（最长 30 s）之后调用，整对象回写会
/// 覆盖期间用户改动的其它字段。调用方的 `setting.port` 同步更新，供紧随的
/// `spawn` 使用。
pub async fn resolve_port_wsl(
    app: &AppHandle,
    distro: &str,
    setting: &mut config::Setting,
) -> Result<(), String> {
    let heal_target = setting.manual_port.unwrap_or(config::default_port());
    if setting.port != heal_target {
        let heal_target_free = scan_port(distro, heal_target, 1).await?.is_some();
        let healed = super::launch::resolve_heal_port(setting.port, heal_target, heal_target_free);
        if healed != setting.port {
            log::info!(
                "Harness port healed from {} back to {} (no longer occupied)",
                setting.port,
                healed
            );
            setting.port = healed;
            config::update_store_dat_setting(app, |s| s.port = healed);
        }
    }

    // 尝试个数按剩余可表示端口数截断，避免 u16 溢出（配置端口接近 65535 时）。
    let attempts = PORT_SCAN_ATTEMPTS.min(u32::from(u16::MAX - setting.port) + 1);
    let available = scan_port(distro, setting.port, attempts)
        .await?
        .ok_or_else(|| {
            format!(
                "WSL_PORT_SCAN_EXHAUSTED: no free port in {}..={}",
                setting.port,
                setting.port + u16::try_from(attempts - 1).unwrap_or(0)
            )
        })?;
    if available != setting.port {
        log::info!(
            "Harness port changed from {} to {} because the configured port is occupied",
            setting.port,
            available
        );
        setting.port = available;
        config::update_store_dat_setting(app, |s| s.port = available);
    }
    Ok(())
}

/// 清理发行版内的残留 Harness（R-W4-3）：必须在端口判定**之前**执行。
///
/// 上一轮崩溃 / 强杀留下的 Linux 侧 dsh 仍会监听配置端口；若先做端口判定，
/// `PORT_SCAN` 会把它判为占用 → 漂移到下一个端口并持久化，`spawn` 这才杀掉残留，
/// 下次重启再 heal 回来——用户看到的是「每次崩溃后端口都白漂一次」。`STOP` 幂等
/// （无 pid 文件时不 sleep），失败仅告警不阻断：随后的 START 会重新拉起，端口
/// 判定按当时的真实占用做。
///
/// 清残留后**不需要**等端口释放（对照 Windows 分支的 `wait_for_port_release`）：
/// Linux 侧监听 socket 随进程死亡立即释放，残留连接的 TIME_WAIT 对 node 的
/// `SO_REUSEADDR`（dsh 与 `PORT_SCAN` 用的同一原语）不构成占用。
pub async fn clear_stale(distro: &str) {
    let distro_owned = distro.to_string();
    let home = wsl_dsh_home().to_string();
    match tauri::async_runtime::spawn_blocking(move || stop_in_distro(&distro_owned, &home)).await {
        Ok(Ok(())) => log::info!("Cleared stale WSL Harness processes in {distro}"),
        Ok(Err(e)) => log::warn!("WSL STOP before launch failed (continuing): {e}"),
        Err(e) => log::warn!("WSL STOP join failed (continuing): {e}"),
    }
}

/// 启动前置检查（**在核心转换锁之外**调用）：现场探测 + 两个门槛。
///
/// 探测最长 30 s（含发行版冷启动），放在转换锁内会让并发的 `set_active_core`
/// 撞上 `CORE_TRANSITION_TIMEOUT`（R-W3-7）。
///
/// 缓存窗口内（[`PREFLIGHT_MAX_AGE`]）复用 `runtime_ready` 刚写入的探测结果，
/// 省掉开机路径上重复的 2–7 s 探测（R-W3-6）；超窗仍现场探测——`skip_auth_ready`
/// 的可信度依赖「刚刚测过」（R-W2-2）。
pub async fn preflight(distro: &str) -> Result<probe::WslCoreProbe, String> {
    let distro_owned = distro.to_string();
    let probed = tauri::async_runtime::spawn_blocking(move || {
        probe::cached_fresh(&distro_owned, PREFLIGHT_MAX_AGE)
            .map(Ok)
            .unwrap_or_else(|| probe::probe(&distro_owned))
    })
    .await
    .map_err(|e| format!("WSL_PROBE_JOIN_FAILED: {e}"))??;
    if probed.dsh.is_none() {
        return Err(format!(
            "WSL_DSH_NOT_INSTALLED: dsh is not installed in {distro}"
        ));
    }
    if !probed.skip_auth_ready {
        return Err("WSL_SKIP_AUTH_PATCH_MISSING: run install/update again".to_string());
    }
    Ok(probed)
}

/// 在发行版内拉起 Harness（`wsl.exe` 中继）并登记为当前持有的进程。
///
/// 前置条件：[`preflight`] 已通过，调用方持有核心转换锁，[`clear_stale`] 已在
/// **端口判定之前**清过残留，且端口已定（见 `super::launch` 的 WSL 分支）。本函数
/// 内部只处理启动守卫、日志轮转与退出监视——**不再跑 STOP**（R-W4-3：若在这里清，
/// 残留占着配置端口会让 `resolve_port_wsl` 先漂移一次）。
pub async fn spawn(app: &AppHandle, distro: &str, port: u16) -> Result<(), String> {
    let distro_owned = distro.to_string();

    // 避免重复启动（与 Windows 分支同款守卫：并发的 launch 只拉起一个中继）。
    if has_owned_process() {
        log::info!("Owned Harness process is already running, skipping WSL launch");
        return Ok(());
    }
    if LAUNCH_GUARD
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        log::info!("Harness launch already in progress, skipping");
        return Ok(());
    }
    let _launch_guard = LaunchGuard;

    let dsh_home = wsl_dsh_home().to_string();

    // 日志文件与 Windows 分支共用（前端日志面板读取），每次真实启动前轮转。
    let log_path = config::get_service_log_path(app);
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("LOG_DIR_MKDIR_FAILED: create log dir failed: {e}"))?;
    }
    rotate_service_log(&log_path, 3);

    log::info!("Starting Harness process in WSL distro {distro} (port {port})");
    let args = wsl_start_args(&distro_owned, &dsh_home, port);
    let (stdout, stderr, pid, handle) =
        win_spawn::spawn_with_hidden_console_owned(&wsl_exe_path(), &args, None, &HashMap::new())
            .map_err(|e| format!("PROCESS_START_FAILED: spawn wsl.exe failed: {e}"))?;

    // PID 与句柄作为整体一次登记，与退出清理（take 一并取出）配对；WSL 目标随
    // 登记表保存——停止路径据此在**同一个**发行版里跑 `STOP`，不看当时的 store
    // （R-W3-1）。
    let handle_value = handle as usize;
    set_owned_wsl_process_with_handle(
        pid,
        handle_value,
        WslTarget {
            distro: distro_owned.clone(),
            dsh_home: dsh_home.clone(),
        },
    );
    let exit_app_handle = app.clone();
    std::thread::spawn(move || unsafe {
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, WaitForSingleObject, INFINITE,
        };
        let process_handle = handle_value as HANDLE;
        WaitForSingleObject(process_handle, INFINITE);
        let owned = on_owned_process_exit(&exit_app_handle, pid, |owned| {
            let handle = owned.handle as HANDLE;
            let mut exit_code: u32 = 0;
            (GetExitCodeProcess(handle, &mut exit_code) != 0).then_some(i64::from(exit_code))
        });
        if let Some(owned) = owned {
            CloseHandle(owned.handle as HANDLE);
        }
    });

    spawn_output_readers(Some(stdout), Some(stderr), log_path);
    log::info!("Harness process started successfully in WSL: pid={pid}, port={port}");
    // R-V8-1：有待确认切换时登记「正式启动已尝试」——回滚宽限从本次启动算起
    // （没有待确认切换时是 no-op，不产生额外文件 IO）。
    crate::service::wsl_core::switch::note_launch_started(app, distro, &dsh_home).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    /// argv 形态：只用 `-e`（exec）形态、`-d <distro>` 在前；脚本是常量；
    /// 位置参数顺序为 `bash <dsh_home> <port>`（方案 §0.3 规则 1）。
    #[test]
    fn start_args_use_exec_form_with_positional_values() {
        let args = wsl_start_args("Ubuntu", wsl_dsh_home(), 3081);
        let parts = parts(&args);
        assert_eq!(
            &parts[..6],
            &["-d", "Ubuntu", "-e", "bash", "-lc", script::START]
        );
        assert_eq!(&parts[6..], &["bash", ".dsh-desktop.dev", "3081"]);
        // 目录名必须是相对 `$HOME` 的裸名：含 `$` 或 `/` 会让 Linux 侧把
        // `$HOME` 当普通字符（D-W3-2）
        assert!(!parts[7].contains('$'));
        assert!(!parts[7].contains('/'));
        // 禁止 `--` 形态（经登录 shell 二次展开，方案硬性规则 1）
        assert!(!parts.iter().any(|p| p == "--"));
        // 动态值不得拼进脚本文本
        assert!(!script::START.contains("3081"));
        assert!(!script::START.contains(".dsh-desktop.dev"));
    }

    /// 位置参数必须整体位于脚本之后（`bash -lc <script> bash <args…>`），
    /// 否则 `wsl.exe` 会把动态值当作脚本的一部分。
    #[test]
    fn start_args_place_values_after_the_script() {
        let args = wsl_start_args("Ubuntu", wsl_dsh_home(), 3080);
        let script_idx = args
            .iter()
            .position(|a| a == script::START)
            .expect("START script argument");
        assert_eq!(script_idx, 5);
        assert_eq!(
            parts(&args[script_idx + 1..]),
            vec!["bash", ".dsh-desktop.dev", "3080"]
        );
    }

    /// 数据目录名与 W2 的单一来源一致（debug/release 隔离），是相对 `$HOME`
    /// 的裸目录名（不含 `$`、`/`、反斜杠），由发行版内的 bash 拼接（D-W3-2）。
    #[test]
    fn dsh_home_is_a_bare_relative_dir_name() {
        let home = wsl_dsh_home();
        assert_eq!(home, dsh_home_dir_name());
        assert!(home.starts_with('.'), "{home}");
        assert!(!home.contains('$'), "{home}");
        assert!(!home.contains('/'), "{home}");
        assert!(!home.contains('\\'), "{home}");
    }

    /// `PORT_SCAN` 正常输出恰好一行端口号；容忍中继带出的空白与 CR（R-W3-2）。
    #[test]
    fn parse_port_scan_output_accepts_single_port_line() {
        assert_eq!(parse_port_scan_output("3082\n").unwrap(), 3082);
        assert_eq!(parse_port_scan_output("  3081  ").unwrap(), 3081);
        assert_eq!(parse_port_scan_output("3081\r\n").unwrap(), 3081);
        assert_eq!(parse_port_scan_output("65535").unwrap(), 65535);
    }

    /// 空输出、多个数字、越界与非法输入一律拒绝（调用方据此报
    /// `WSL_PORT_SCAN_INVALID`，不把垃圾值写进设置）。
    #[test]
    fn parse_port_scan_output_rejects_empty_and_non_numeric() {
        for bad in ["", "\n", "0", "65536", "abc", "-1", "3.5", "3081\n3082\n"] {
            let err = parse_port_scan_output(bad).unwrap_err();
            assert!(err.starts_with("WSL_PORT_SCAN_INVALID"), "{bad:?}: {err}");
        }
    }
}
