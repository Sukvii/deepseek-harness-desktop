//! WSL 核心的安装 / 升级编排（方案 W2.5；v8「受控运行时」裁决）。
//!
//! 安装目标是应用私有的受控运行时 `$HOME/<dsh_home_dir_name()>/runtime`：manifest
//! 与锁文件随桌面包分发（[`super::runtime::load`]），`npm ci` 只重放锁，不碰用户的
//! 全局 npm 前缀，也不复用用户自装的全局 dsh。
//!
//! 流程：加锁 → probe → Node 校验 → 解析目标版本（默认目标 = 桌面推荐版本）→
//! **加载并校验目标资源**（缺资源 / manifest 不匹配即停止，R-V8-2）→ 判定是否
//! 需要安装（未装 / 版本不同 / **锁基线变化**）→ 需要时走候选窗口：「准备候选 →
//! 经 UNC 写 manifest+lock → `npm ci` → 补丁候选 → 启动验证候选 → 整体切换到正式
//! 运行时 → 登记待确认切换」。窗口内 `npm ci` / 补丁 / 启动验证任一失败都只丢弃
//! 候选，旧运行时不受影响；切换阶段失败按备份恢复（`RESTORE_RUNTIME`）。
//! **备份不在安装内清理**：切换成功后由 [`super::switch`] 跟踪——正式启动与桌面
//! readiness 成功才确认并清理本次备份槽；正式启动失败则回滚（R-V8-1）。
//! 验证脚本的连接中断 / 中继超时路径统一走 [`super::script::CLEANUP_VERIFY`] 收尾
//! （验证进程组与端口不残留，R-V8-3）。
//! 已装且版本与基线都对时只有一次 probe，秒级返回——W3.4 的开机路径可安全调用。
//!
//! 资源缺失 / manifest 与目标不一致时**停止**（`WSL_RUNTIME_RESOURCE_MISSING` /
//! `WSL_RUNTIME_MANIFEST_MISMATCH`），不存在无锁安装的后备路径（方案 v8 执行范围 1）。

use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

use super::dsh_home_dir_name;
use super::exec::{
    decode_auto, run_in_distro, run_in_distro_streaming, summarize, WslOutput, WslStream,
};
use super::patch;
use super::probe::{self, WslCoreProbe};
use super::runtime::{
    RuntimeBaseline, RuntimeBundle, BACKUP_DIR_PREFIX, BASELINE_FILE_NAME, CANDIDATE_DIR_NAME,
    RUNTIME_DIR_NAME,
};
use super::script::{
    CLEANUP_VERIFY, CLEAN_RUNTIME_LEFTOVERS, DROP_RUNTIME_CANDIDATE, PORT_SCAN,
    PREPARE_RUNTIME_CANDIDATE, RESOLVE_DSH, RESTORE_RUNTIME, RUNTIME_NPM_CI, SWITCH_RUNTIME,
    VERIFY_CORE,
};
use super::{run_script, switch};
use crate::service::download::ProgressPayload;

/// Node 主版本下限（`@deepseek-ai/dsh` 无 `engines` 字段，npm 不会替我们拦，R-W2-8）。
const MIN_NODE_MAJOR: u64 = 20;

/// `npm ci` 单次尝试的 Linux 侧截止时间，由 [`RUNTIME_NPM_CI`] 的 `timeout` 执行
/// （R-W2-6；预热缓存下秒级，冷启动按十分钟量级给足余量）。
pub const INSTALL_ATTEMPT_SECS: u64 = 1200;

/// Rust 侧中继超时：只作兜底（正常情况下 Linux 侧 `timeout` 先结束，R-W2-6）。
const INSTALL_TIMEOUT: Duration = Duration::from_secs(2 * INSTALL_ATTEMPT_SECS + 60);

/// dist-tag 在线解析的中继超时（`RESOLVE_DSH` 自带 `timeout -k 5 60`，再留余量）。
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(90);

/// 候选目录准备 / 丢弃的中继超时。
const PREPARE_TIMEOUT: Duration = Duration::from_secs(120);
const DROP_TIMEOUT: Duration = Duration::from_secs(120);
/// 整体切换（同目录改名）与兜底恢复的中继超时。
const SWITCH_TIMEOUT: Duration = Duration::from_secs(120);
const RESTORE_TIMEOUT: Duration = Duration::from_secs(120);
/// 备份清理：`rm -rf` 整棵旧运行时，给足余量。
const CLEAN_TIMEOUT: Duration = Duration::from_secs(300);
/// Linux 侧端口扫描（`bind` 探测，R-W3-2）的中继超时。
const PORT_SCAN_TIMEOUT: Duration = Duration::from_secs(30);

/// 启动验证：端口从该起点开始扫描、最多尝试个数、**真实总截止秒数**（首次启动含
/// 临时 profile 初始化，给足余量）与中继超时。
///
/// R-V8-3：`VERIFY_CORE` 的 `$4` 是 Linux 侧的真实截止时间（时间戳差值），中继
/// 超时必须**晚于它并留足清理余量**（每次探测 ≤ 25 s + 进程组回收 ≤ 数秒）。
const VERIFY_PORT_BASE: u16 = 3080;
const VERIFY_PORT_TRIES: u32 = 20;
const VERIFY_WAIT_SECS: u64 = 90;
const VERIFY_TIMEOUT: Duration = Duration::from_secs(VERIFY_WAIT_SECS + 45);

/// 同一时刻只允许一个 WSL 核心安装（R-W2-5；与上游 `bridge/lifecycle.rs` 的
/// `INSTALL_LOCK` 同款）。
static WSL_INSTALL_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

fn install_lock() -> &'static tokio::sync::Mutex<()> {
    WSL_INSTALL_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// 安装阶段的日志转发器（直接 emit `install-progress`，不经过 50ms 节流）。
struct InstallProgress {
    window: tauri::WebviewWindow,
    lines: usize,
}

impl InstallProgress {
    fn new(window: tauri::WebviewWindow) -> Self {
        Self { window, lines: 0 }
    }

    fn emit(&self, percentage: f64, detail: &str, log: &str) {
        let _ = self.window.emit(
            "install-progress",
            ProgressPayload {
                title: "WSL 核心".to_string(),
                r#type: "wsl-core".to_string(),
                percentage,
                progress: percentage,
                detail: detail.to_string(),
                log: log.to_string(),
            },
        );
    }

    /// 阶段切换（无日志行）。
    fn stage(&self, percentage: f64, detail: &str) {
        self.emit(percentage, detail, "");
    }

    /// 转发一行安装输出：百分比随行数缓升（真实进度 npm 不提供机器可读格式）。
    fn push_line(&mut self, stream: WslStream, line: &str) {
        self.lines += 1;
        let percentage = (20.0 + self.lines as f64 * 1.5).min(80.0);
        let prefix = match stream {
            WslStream::Stdout => "",
            WslStream::Stderr => "[stderr] ",
        };
        self.emit(
            percentage,
            "正在建立 WSL 运行时依赖（npm ci）",
            &format!("{prefix}{line}"),
        );
    }
}

/// 解析 `NODEV` 的主版本号（`v22.22.1` → 22；无法解析 → `None`）。R-W2-8。
fn node_major(version: &str) -> Option<u64> {
    version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .next()?
        .parse()
        .ok()
}

/// `RESOLVE_DSH` 的输出必须恰好一行且可被 semver 解析；否则视为解析失败（R-W2-1）。
fn parse_resolved_version(stdout: &str) -> Result<String, String> {
    let lines: Vec<&str> = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    match lines.as_slice() {
        [one] if semver::Version::parse(one).is_ok() => Ok((*one).to_string()),
        _ => Err(format!(
            "WSL_DSH_RESOLVE_FAILED: unexpected npm view output: {stdout:?}"
        )),
    }
}

/// 是否需要走候选安装窗口（v8）：
/// - 目标未知（离线且已装）→ 不装（R-W2-1 行为不变）；
/// - 未装（运行时缺失 / 二进制不可执行）或版本与目标不同 → 装；
/// - 版本相同 → 由基线是否一致决定：不一致才更新（相同主包、锁基线变化仍能更新
///   的唯一触发点）。
///
/// R-V8-2：调用方在进入本判定**之前**已经 `runtime::load` 校验过资源，因此这里
/// 不存在「资源缺失 → `None` → 静默保留」的路径——`None` 只表示「没算基线」
/// （版本不同 / 未装 / 目标未知）；版本相同时基线必然是真实布尔值。
fn needs_runtime_update(
    target: Option<&str>,
    installed: Option<&str>,
    baseline_matches: Option<bool>,
) -> bool {
    let Some(target) = target else {
        return false;
    };
    match installed {
        Some(installed) if installed.trim() == target => baseline_matches == Some(false),
        _ => true,
    }
}

/// 解析目标版本：精确 semver 直接采用（默认目标 = 桌面推荐版本走这条，离线可用）；
/// dist-tag 才在线解析（`RESOLVE_DSH`，R-W2-1；显式 spec 仍可能是 dist-tag）。
async fn resolve_target(distro: &str, version_spec: &str) -> Result<String, String> {
    if semver::Version::parse(version_spec).is_ok() {
        return Ok(version_spec.to_string());
    }
    let distro_name = distro.to_string();
    let spec = version_spec.to_string();
    let output = tauri::async_runtime::spawn_blocking(move || {
        run_in_distro(&distro_name, RESOLVE_DSH, &[spec.as_str()], RESOLVE_TIMEOUT)
    })
    .await
    .map_err(|e| format!("WSL_RESOLVE_JOIN_FAILED: {e}"))?;
    let output = output?;
    if output.code == 0 {
        parse_resolved_version(&String::from_utf8_lossy(&output.stdout))
    } else {
        Err(format!(
            "WSL_DSH_RESOLVE_FAILED: npm view exited with {}: {}",
            output.code,
            summarize(&output)
        ))
    }
}

/// 默认安装目标的解析口径（D-W5R-2 / R-V8-2）：与桌面核心共用
/// `recommended_dsh_version()`，**对所有调用方统一**——缺失/无效一律明确报错，
/// 绝不以「已装版本」代替推荐值、也绝不回退 latest。
///
/// R-V8-2 之前开机自愈传入已装版本兜底（「缺失配置不把已装用户卡死」），但那会
/// 让缺配置在自愈路径上静默变成 no-op；现在统一失败，用户在任何安装/自愈入口都
/// 能看到明确原因（资源是随包分发的，缺失属于发布/安装异常）。
pub fn default_version_spec(app: &AppHandle) -> Result<String, String> {
    crate::config::recommended_dsh_version(app).ok_or_else(|| {
        "WSL_DSH_VERSION_UNCONFIGURED: desktop recommended dsh version is missing or invalid"
            .to_string()
    })
}

/// 秒级 Unix 时间戳：候选切换窗口的备份目录名（无秘密，仅用于区分窗口）。
fn unix_stamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

/// 脚本 stdout（`wsl.exe` 输出可能是 UTF-16LE）。
fn output_text(output: &WslOutput) -> String {
    decode_auto(&output.stdout)
}

/// 经 UNC 写入受控运行时目录内的文件（v8；目录必须已由 Linux 侧脚本建好）。
fn write_unc_file(distro: &str, linux_path: &str, content: &str) -> Result<(), String> {
    let path = std::path::PathBuf::from(patch::wsl_unc_path(distro, linux_path));
    std::fs::write(&path, content)
        .map_err(|e| format!("WSL_RUNTIME_WRITE_FAILED: {}: {e}", path.display()))
}

/// 读取运行时基线记录；文件缺失 / 内容损坏 → `None`。
fn read_runtime_baseline(distro: &str, linux_path: &str) -> Option<RuntimeBaseline> {
    let path = std::path::PathBuf::from(patch::wsl_unc_path(distro, linux_path));
    RuntimeBaseline::parse(&std::fs::read_to_string(path).ok()?)
}

/// 已装运行时是否与资源锁基线一致（v8 的「相同主包、锁基线变化仍能更新」判定）。
///
/// R-V8-2：资源由调用方**先**经 [`super::runtime::load`] 校验加载（缺失 / 无效在
/// 那一步就中止），因此这里不再有「无从核对」的分支——只有 true / false 两个
/// 真实答案，「无法核对」不再被当成「已匹配」。
fn runtime_baseline_matches(distro: &str, home: &str, bundle: &RuntimeBundle) -> bool {
    let marker = format!(
        "{home}/{}/{RUNTIME_DIR_NAME}/{BASELINE_FILE_NAME}",
        dsh_home_dir_name()
    );
    read_runtime_baseline(distro, &marker)
        .is_some_and(|b| b.version == bundle.version && b.lock_sha256 == bundle.lock_sha256)
}

/// 启动验证失败时的诊断摘要：`VERIFY_FAILED` 之后的启动日志尾部（最后 8 行，
/// 截断 400 字符；dsh 启动日志不含凭据）。
fn summarize_verify(output: &WslOutput) -> String {
    let stdout = output_text(output);
    let lines: Vec<&str> = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let tail = lines
        .iter()
        .rev()
        .take(8)
        .rev()
        .copied()
        .collect::<Vec<_>>()
        .join(" | ");
    tail.chars().take(400).collect()
}

/// 在 Linux 侧扫一个空闲端口（验证候选运行时用；避开 Windows 核心可能占用的业务端口）。
async fn scan_verify_port(distro: &str) -> Result<u16, String> {
    let out = run_script(
        distro,
        PORT_SCAN,
        vec![VERIFY_PORT_BASE.to_string(), VERIFY_PORT_TRIES.to_string()],
        PORT_SCAN_TIMEOUT,
    )
    .await?;
    if out.code != 0 {
        return Err(format!(
            "WSL_VERIFY_NO_PORT: no bindable port in {VERIFY_PORT_BASE}..+{VERIFY_PORT_TRIES}: {}",
            summarize(&out)
        ));
    }
    output_text(&out)
        .trim()
        .parse::<u16>()
        .map_err(|e| format!("WSL_VERIFY_NO_PORT: bad PORT_SCAN output: {e}"))
}

/// 丢弃候选运行时（失败收尾；不影响正式运行时）。尽力而为，失败只告警。
async fn drop_candidate(distro: &str, dir_name: &str) {
    let dropped = run_script(
        distro,
        DROP_RUNTIME_CANDIDATE,
        vec![dir_name.to_string()],
        DROP_TIMEOUT,
    )
    .await;
    match dropped {
        Ok(out) if out.code == 0 => {}
        Ok(out) => log::warn!(
            "[wsl-core] drop candidate exited with {}: {}",
            out.code,
            summarize(&out)
        ),
        Err(e) => log::warn!("[wsl-core] drop candidate failed: {e}"),
    }
}

/// 验证收尾（R-V8-3）：按 pidfile 三重确认回收本次验证的进程组并清临时目录
/// （日志保留）。中继超时（只杀了 `wsl.exe`，Linux 子进程不会随之退出，F5）与
/// 验证失败都走这里；尽力而为，失败只告警。
async fn cleanup_verify(distro: &str, dir_name: &str) {
    let cleaned = run_script(
        distro,
        CLEANUP_VERIFY,
        vec![dir_name.to_string()],
        DROP_TIMEOUT,
    )
    .await;
    match cleaned {
        Ok(out) if out.code == 0 => log::info!("[wsl-core] verify cleanup: {}", summarize(&out)),
        Ok(out) => log::warn!(
            "[wsl-core] verify cleanup exited with {}: {}",
            out.code,
            summarize(&out)
        ),
        Err(e) => log::warn!("[wsl-core] verify cleanup failed: {e}"),
    }
}

/// 确保受控运行时已装好目标版本并完成补丁；返回最新探测结果（并写入进程内缓存）。
pub async fn ensure(
    app: &AppHandle,
    distro: &str,
    version_spec: &str,
) -> Result<WslCoreProbe, String> {
    // 并发/重入防护（R-W2-5）：W5 的双击与 W3.4 的开机路径可能并发调用。
    let Ok(_guard) = install_lock().try_lock() else {
        return Err("WSL_INSTALL_BUSY: another WSL core install is running".to_string());
    };
    // 运行中判定（R-W3-3）：dsh 正以该核心运行时，窗口内的整体切换会搬走正在被
    // 懒加载的运行时目录，与运行中的 `import()` 和工具子进程竞争。闸门放在唯一
    // 收口点（本函数），调用方不必各自判断；W5 的「安装 / 更新」按钮流程是
    // 「停服 → 安装 → 重启」（方案 W5.3）。
    let running =
        crate::service::workflow::has_owned_process() && crate::service::core::is_wsl_active(app);
    let window = app
        .get_webview_window("main")
        .ok_or("WINDOW_NOT_FOUND: main window missing")?;
    let mut progress = InstallProgress::new(window);
    progress.stage(5.0, "探测 WSL 环境");

    let dir_name = dsh_home_dir_name().to_string();
    let distro_name = distro.to_string();
    let before = tauri::async_runtime::spawn_blocking(move || probe::probe(&distro_name))
        .await
        .map_err(|e| format!("WSL_PROBE_JOIN_FAILED: {e}"))??;
    if before.node.is_none() {
        return Err(format!(
            "WSL_NODE_MISSING: Node.js >= {MIN_NODE_MAJOR} is required inside {distro} (apt / nvm)"
        ));
    }
    match before.node_version.as_deref().and_then(node_major) {
        Some(major) if major >= MIN_NODE_MAJOR => {}
        _ => {
            return Err(format!(
                "WSL_NODE_TOO_OLD: {} (< {MIN_NODE_MAJOR}) in {distro}",
                before.node_version.as_deref().unwrap_or("unknown")
            ));
        }
    }

    // 目标版本：dist-tag 解析失败且已装 → 保留当前版本（离线不阻断，R-W2-1）。
    let target = match resolve_target(distro, version_spec).await {
        Ok(version) => Some(version),
        Err(e) => {
            if before.dsh.is_some() {
                log::warn!("[wsl-core] resolve target version failed, keeping current: {e}");
                progress.stage(15.0, "无法联网检查更新，保留当前版本");
                None
            } else {
                return Err(e);
            }
        }
    };

    // 资源（v8 / R-V8-2）：目标解析成功后**先**加载并交叉校验资源——缺资源 /
    // manifest 不匹配在判定「是否需要安装」之前就停止；「无法核对基线」绝不能
    // 被当成「已匹配」而静默保留（早退路径也不再绕过资源校验）。
    let bundle = target
        .as_deref()
        .map(|version| super::runtime::load(app, version))
        .transpose()?;

    // 更新判定（v8）：版本不同 / 未装 / 锁基线变化才进候选窗口。
    let baseline = match (bundle.as_ref(), before.dsh_version.as_deref()) {
        (Some(bundle), Some(installed)) if installed.trim() == bundle.version => {
            Some(runtime_baseline_matches(distro, &before.home, bundle))
        }
        _ => None,
    };
    if !needs_runtime_update(target.as_deref(), before.dsh_version.as_deref(), baseline) {
        probe::store(before.clone());
        return Ok(before);
    }
    if running {
        return Err("WSL_INSTALL_BUSY: stop the WSL core service before installing".to_string());
    }
    let version = target
        .clone()
        .ok_or("WSL_DSH_RESOLVE_FAILED: no target version to install")?;
    let bundle = bundle.ok_or("WSL_DSH_RESOLVE_FAILED: no target version to install")?;
    let candidate_dir = format!("{}/{}", before.home, dir_name);

    // 0a. 未完成的旧切换（R-V8-1B ②）：一次只允许一个待确认切换。属于本发行版的
    //     先回滚恢复旧运行时再继续；已 rollbackFailed / 属于另一发行版（其服务可能
    //     正在运行，静默回滚会打断它）都明确拒绝——绝不覆盖旧标记（旧备份是唯一
    //     可恢复副本，被覆盖就等于在无记录的情况下丢掉了它）。
    if let Some(old) = switch::read(app) {
        if old.distro != distro {
            return Err(format!(
                "WSL_RUNTIME_SWITCH_UNRESOLVED: a runtime switch for {} is still awaiting \
                 confirmation (stamp {}); launch that core once or resolve it before installing {}",
                old.distro, old.stamp, distro
            ));
        }
        if old.rollback_failed {
            return Err(format!(
                "WSL_RUNTIME_SWITCH_UNRESOLVED: the previous runtime switch (stamp {}) failed to \
                 roll back; files kept at ~/{}/{}{} and ~/{}/runtime-backup-{}; resolve it \
                 manually before installing again",
                old.stamp,
                dir_name,
                switch::FAILED_DIR_PREFIX,
                old.stamp,
                dir_name,
                old.stamp
            ));
        }
        log::warn!(
            "[wsl-core] unresolved runtime switch (stamp {}) found before install; rolling it \
             back first",
            old.stamp
        );
        let resolved = switch::rollback(app, &old).await;
        if !resolved || switch::pending_for(app, distro).is_some() {
            return Err(format!(
                "WSL_RUNTIME_SWITCH_UNRESOLVED: could not resolve the previous runtime switch \
                 (stamp {}); refusing to install over it",
                old.stamp
            ));
        }
    }

    // 0b. 房务（v8-R1 / R-V8-1 第 3 条）：**没有待确认切换**时才清理遗留的
    //     `runtime-backup-*` / `runtime-failed-*`——待确认的备份是唯一可恢复副本，
    //     确认之前一个都不动。清理失败只告警，不阻断安装。
    if switch::pending_for(app, distro).is_none() {
        match run_script(
            distro,
            CLEAN_RUNTIME_LEFTOVERS,
            vec![dir_name.clone()],
            CLEAN_TIMEOUT,
        )
        .await
        {
            Ok(out) if out.code == 0 => log::info!(
                "[wsl-core] leftover runtime slots cleaned before install ({})",
                summarize(&out)
            ),
            Ok(out) => log::warn!(
                "[wsl-core] leftover cleanup exited with {}: {}",
                out.code,
                summarize(&out)
            ),
            Err(e) => log::warn!("[wsl-core] leftover cleanup failed (continuing): {e}"),
        }
    } else {
        log::info!("[wsl-core] pending runtime switch detected; skipping leftover cleanup");
    }

    // 1. 准备候选目录（Linux 侧建目录；正式运行时不受影响）。
    progress.stage(8.0, "准备受控运行时");
    let prepared = run_script(
        distro,
        PREPARE_RUNTIME_CANDIDATE,
        vec![dir_name.clone()],
        PREPARE_TIMEOUT,
    )
    .await?;
    if prepared.code != 0 {
        return Err(format!(
            "WSL_RUNTIME_PREPARE_FAILED: could not create the candidate runtime: {}",
            summarize(&prepared)
        ));
    }

    // 2. 经 UNC 写入 manifest 与锁文件（9p 是网络文件系统，放 spawn_blocking）。
    let candidate_root = format!("{candidate_dir}/{CANDIDATE_DIR_NAME}");
    let (manifest, lock, expected_sha) = (
        bundle.manifest.clone(),
        bundle.lock.clone(),
        bundle.lock_sha256.clone(),
    );
    let write_distro = distro.to_string();
    let (candidate_root_for_write, candidate_root_for_check) =
        (candidate_root.clone(), candidate_root.clone());
    let written = tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        write_unc_file(
            &write_distro,
            &format!("{candidate_root_for_write}/package.json"),
            &manifest,
        )?;
        write_unc_file(
            &write_distro,
            &format!("{candidate_root_for_write}/package-lock.json"),
            &lock,
        )?;
        // 回读校验：9p 写入必须逐字落盘，否则 `npm ci` 会按坏输入重放。
        let path = std::path::PathBuf::from(patch::wsl_unc_path(
            &write_distro,
            &format!("{candidate_root_for_check}/package-lock.json"),
        ));
        let read_back = std::fs::read_to_string(&path)
            .map_err(|e| format!("WSL_RUNTIME_WRITE_FAILED: {}: {e}", path.display()))?;
        if super::runtime::lock_sha256(&read_back) != expected_sha {
            return Err(
                "WSL_RUNTIME_WRITE_FAILED: package-lock.json differs after write".to_string(),
            );
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("WSL_RUNTIME_WRITE_JOIN_FAILED: {e}"))?;
    if let Err(reason) = written {
        drop_candidate(distro, &dir_name).await;
        return Err(reason);
    }

    // 3. `npm ci` 重放锁（流式进度）。
    progress.stage(12.0, "npm ci 建立依赖树（WSL）");
    let mut installing = progress;
    let attempt = INSTALL_ATTEMPT_SECS.to_string();
    let install_distro = distro.to_string();
    let install_dir = dir_name.clone();
    let (result, done) = tauri::async_runtime::spawn_blocking(move || {
        let result = run_in_distro_streaming(
            &install_distro,
            RUNTIME_NPM_CI,
            &[install_dir.as_str(), attempt.as_str()],
            INSTALL_TIMEOUT,
            |stream, line| installing.push_line(stream, line),
        );
        (result, installing)
    })
    .await
    .map_err(|e| format!("WSL_INSTALL_JOIN_FAILED: {e}"))?;
    progress = done;
    let failure = match result {
        Err(e) => Some(format!("WSL_DSH_INSTALL_FAILED: {e}")),
        Ok(output) if output.code == 124 => Some(format!(
            "WSL_DSH_INSTALL_TIMEOUT: npm ci exceeded {INSTALL_ATTEMPT_SECS}s in {distro}"
        )),
        Ok(output) if output.code != 0 => Some(format!(
            "WSL_DSH_INSTALL_FAILED: npm ci exited with {}: {}",
            output.code,
            summarize(&output)
        )),
        Ok(_) => None,
    };
    if let Some(reason) = failure {
        drop_candidate(distro, &dir_name).await;
        return Err(reason);
    }

    // 4. 基线记录（版本 + 锁哈希）：切换后「相同主包、锁基线变化」靠它判定。
    let baseline = RuntimeBaseline::new(&bundle.version, &bundle.lock_sha256);
    let marker_path = format!("{candidate_root}/{BASELINE_FILE_NAME}");
    let baseline_json = baseline.to_json();
    let marker_distro = distro.to_string();
    let marker = tauri::async_runtime::spawn_blocking(move || {
        write_unc_file(&marker_distro, &marker_path, &baseline_json)
    })
    .await
    .map_err(|e| format!("WSL_RUNTIME_WRITE_JOIN_FAILED: {e}"))?;
    if let Err(reason) = marker {
        drop_candidate(distro, &dir_name).await;
        return Err(reason);
    }

    // 5. 补丁候选（只动候选树；UNC 是 9P，放 spawn_blocking）。
    progress.stage(88.0, "应用 --skip-auth 补丁");
    let patch_distro = distro.to_string();
    let patch_root = format!("{candidate_root}/node_modules");
    let patched =
        tauri::async_runtime::spawn_blocking(move || patch::apply(&patch_distro, &patch_root))
            .await
            .map_err(|e| format!("WSL_PATCH_JOIN_FAILED: {e}"))?
            .unwrap_or_else(|e| {
                log::warn!("[wsl-core] --skip-auth patch failed: {e}");
                false
            });
    if !patched {
        drop_candidate(distro, &dir_name).await;
        return Err(format!(
            "WSL_DSH_PATCH_FAILED: --skip-auth patch did not apply to the new runtime in {distro}"
        ));
    }

    // 6. 启动验证候选（live 首次启动 + 全部 client bundle 就绪才放行）。
    progress.stage(92.0, "验证新运行时（启动与客户端资源）");
    let port = match scan_verify_port(distro).await {
        Ok(port) => port,
        Err(reason) => {
            drop_candidate(distro, &dir_name).await;
            return Err(reason);
        }
    };
    let verify = match run_script(
        distro,
        VERIFY_CORE,
        vec![
            dir_name.clone(),
            CANDIDATE_DIR_NAME.to_string(),
            port.to_string(),
            VERIFY_WAIT_SECS.to_string(),
        ],
        VERIFY_TIMEOUT,
    )
    .await
    {
        Ok(output) => output,
        Err(e) => {
            // R-V8-3：中继超时只杀 `wsl.exe`，Linux 侧验证进程组不会随之退出
            // （F5）——先按 pidfile 三重确认收尾，再丢候选（候选树正被验证进程
            // 使用，顺序不能反）。错误路径不得经 `?` 绕过收尾。
            cleanup_verify(distro, &dir_name).await;
            drop_candidate(distro, &dir_name).await;
            return Err(format!("WSL_DSH_VERIFY_FAILED: {e}"));
        }
    };
    if verify.code != 0 || !output_text(&verify).contains("VERIFY_OK") {
        let reason = format!(
            "WSL_DSH_VERIFY_FAILED: the new runtime did not become ready in {distro}: {}",
            summarize_verify(&verify)
        );
        cleanup_verify(distro, &dir_name).await;
        drop_candidate(distro, &dir_name).await;
        return Err(reason);
    }

    // 7. 整体切换（运行中判定在这里重新采样：窗口可能持续数分钟，期间开机自愈 /
    //    手动启动都可能把服务拉起来——正在使用的运行时不能被改名搬走，D-W3-7）。
    let running =
        crate::service::workflow::has_owned_process() && crate::service::core::is_wsl_active(app);
    if running {
        drop_candidate(distro, &dir_name).await;
        return Err(
            "WSL_INSTALL_BUSY: the WSL core service started during the update; retry".to_string(),
        );
    }
    progress.stage(97.0, "切换受控运行时");
    let stamp = unix_stamp();
    let switched = run_script(
        distro,
        SWITCH_RUNTIME,
        vec![dir_name.clone(), stamp.clone()],
        SWITCH_TIMEOUT,
    )
    .await;
    let switch_ok = matches!(
        &switched,
        Ok(out) if out.code == 0 && output_text(out).contains("SWITCH_OK")
    );
    if !switch_ok {
        let detail = match &switched {
            Ok(out) => summarize(out),
            Err(e) => e.clone(),
        };
        // 兜底恢复（幂等）：切换失败时把备份放回正式位置；恢复不了也要把实际
        // 状态探测给缓存，避免 UI 显示过期结果。
        let restored = run_script(
            distro,
            RESTORE_RUNTIME,
            vec![dir_name.clone(), stamp.clone()],
            RESTORE_TIMEOUT,
        )
        .await;
        let restore_note = match &restored {
            Ok(out) if out.code == 0 => output_text(out).trim().to_string(),
            Ok(out) => format!("restore exited with {}", out.code),
            Err(e) => format!("restore failed to run: {e}"),
        };
        let distro_name = distro.to_string();
        if let Ok(Ok(fresh)) =
            tauri::async_runtime::spawn_blocking(move || probe::probe(&distro_name)).await
        {
            probe::store(fresh);
        }
        return Err(format!(
            "WSL_RUNTIME_SWITCH_FAILED: {detail} ({restore_note}); backup slot: \
             ~/{dir_name}/{BACKUP_DIR_PREFIX}{stamp}"
        ));
    }

    // 8. 登记「待确认切换」（v8-R1 / R-V8-1A / R-V8-1B）：备份**不在这里清理**
    //    ——正式启动与桌面 readiness 成功（目标身份 + 运行时基线齐备）后才由
    //    `switch::note_health` / 后端截止任务确认并清理本次备份槽；正式启动失败
    //    （启动报错 / 进程退出 / readiness 超宽限）由它回滚到旧运行时。
    //    标记写不进就无法追踪备份与确认，按失败处理：立即回滚并明确报错。
    //    R-V8-1B-2：`switch::arm` 自身持有处置互斥（与确认 / 回滚串行化，拒绝
    //    覆盖任何未完成旧切换）。
    let marker = switch::arm(
        app,
        distro,
        &before.home,
        &dir_name,
        &stamp,
        &bundle.version,
        &bundle.lock_sha256,
    )
    .await;
    if let Err(e) = marker {
        let pending = switch::PendingSwitch::new(
            distro,
            &before.home,
            &dir_name,
            &stamp,
            &bundle.version,
            &bundle.lock_sha256,
        );
        let rolled = switch::rollback_unarmed(app, &pending).await;
        return Err(format!(
            "WSL_RUNTIME_SWITCH_MARKER_FAILED: {e}; rolled back: {rolled}"
        ));
    }
    log::info!(
        "[wsl-core] runtime switched to {version} (stamp {stamp}); backup kept until the service \
         confirms readiness"
    );

    let distro_name = distro.to_string();
    let after = tauri::async_runtime::spawn_blocking(move || probe::probe(&distro_name))
        .await
        .map_err(|e| format!("WSL_PROBE_JOIN_FAILED: {e}"))??;
    if after.dsh.is_none() {
        return Err(format!(
            "WSL_DSH_NOT_INSTALLED: managed runtime still missing in {distro} after install"
        ));
    }
    if !after.skip_auth_ready {
        log::warn!(
            "[wsl-core] --skip-auth patch not detected in {distro} after install (patch was applied to the candidate)"
        );
    }
    probe::store(after.clone());

    progress.stage(100.0, "WSL 核心已就绪");
    Ok(after)
}

#[cfg(test)]
mod tests {
    use super::{
        install_lock, needs_runtime_update, node_major, parse_resolved_version, unix_stamp,
    };

    #[test]
    fn parse_resolved_version_requires_single_semver_line() {
        assert_eq!(
            parse_resolved_version("0.1.5-rc.1\n").unwrap(),
            "0.1.5-rc.1"
        );
        assert_eq!(
            parse_resolved_version("  0.1.5-rc.2  ").unwrap(),
            "0.1.5-rc.2"
        );
        for bad in ["", "\n", "0.1.5-rc.1\n0.1.5-rc.2\n", "latest", "v0.1.5"] {
            let err = parse_resolved_version(bad).unwrap_err();
            assert!(err.starts_with("WSL_DSH_RESOLVE_FAILED"), "{bad:?}: {err}");
        }
    }

    /// v8 的更新判定：未装 / 版本不同一律装；版本相同按（真实）基线是否一致。
    ///
    /// R-V8-2：调用方在判定前已 `runtime::load` 过资源，所以「版本相同」必然带着
    /// 真实布尔基线（`Some(true)` / `Some(false)`）；`None` 只会出现在
    /// 「未装 / 版本不同 / 目标未知」这些分支里，不再表示「资源缺失」。
    #[test]
    fn needs_runtime_update_covers_version_and_baseline_cases() {
        // 未装（运行时缺失 / 二进制不可执行）
        assert!(needs_runtime_update(Some("0.1.2-rc.1"), None, None));
        // 版本不同
        assert!(needs_runtime_update(
            Some("0.1.2-rc.1"),
            Some("0.1.5-rc.2"),
            None
        ));
        // 版本相同：基线一致 → 不装
        assert!(!needs_runtime_update(
            Some("0.1.2-rc.1"),
            Some("0.1.2-rc.1"),
            Some(true)
        ));
        // 版本相同：锁基线变化 → 更新（相同主包、锁基线变化仍能更新）
        assert!(needs_runtime_update(
            Some("0.1.2-rc.1"),
            Some("0.1.2-rc.1"),
            Some(false)
        ));
        // 版本带空白：按 trim 后比较
        assert!(!needs_runtime_update(
            Some("0.1.2-rc.1"),
            Some(" 0.1.2-rc.1 "),
            Some(true)
        ));
        // 目标未知：不装（R-W2-1 离线保留）
        assert!(!needs_runtime_update(None, Some("0.1.2-rc.1"), None));
        assert!(!needs_runtime_update(None, None, None));
    }

    #[test]
    fn node_major_parses_v_prefixed_versions() {
        assert_eq!(node_major("v22.22.1"), Some(22));
        assert_eq!(node_major("20"), Some(20));
        assert_eq!(node_major("v18.20.8"), Some(18));
        assert_eq!(node_major(""), None);
        assert_eq!(node_major("nonsense"), None);
    }

    #[test]
    fn install_lock_is_exclusive_while_held() {
        let first = install_lock().try_lock();
        assert!(first.is_ok());
        assert!(install_lock().try_lock().is_err());
        drop(first);
        assert!(install_lock().try_lock().is_ok());
    }

    #[test]
    fn unix_stamp_is_a_plain_number() {
        let stamp = unix_stamp();
        assert!(!stamp.is_empty());
        assert!(stamp.chars().all(|c| c.is_ascii_digit()), "{stamp}");
    }
}
