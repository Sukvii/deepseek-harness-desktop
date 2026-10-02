//! 受控运行时切换的确认与回滚（方案 v8；REVIEW §F R-V8-1、§G R-V8-1A/1B、§H R-V8-1B-1/2）。
//!
//! 切换成功的判据不是 `mv` 成功，而是**正式服务真正启动且桌面 readiness 通过**：
//!
//! 1. `install::ensure` 整体切换完成后写下「待确认切换」标记（应用数据目录内
//!    `wsl-runtime-switch.json`），旧运行时一直留在 `runtime-backup-<stamp>`；
//! 2. **确认** = 目标身份（发行版 + 数据目录 + 已尝试启动 + 运行时基线）全部吻合的
//!    `proxy_health_check` 成功（桌面 readiness 的实际完成路径）→ 只清理本次备份槽
//!    并删除标记（清理放后台，不拖慢健康响应）；
//! 3. **回滚** = 正式启动失败（启动路径报错 / 进程退出后的 `HARNESS_NOT_OWNED` /
//!    超过宽限）→ 停服 → 失败的新树移出到 `runtime-failed-<stamp>` → 备份移回
//!    `runtime` → 刷新探测缓存。回滚失败保留全部文件、置 `rollbackFailed` 不再
//!    反复重试，并明确报告（不宣称成功）。
//!
//! R-V8-1A（前端停止轮询后的后端收尾）：前端 readiness 轮询有 180 s inactivity 与
//! 300 s absolute 两条停止路径（`src/utils/readiness.ts`、
//! `src/store/modules/harness/store.ts`），停止后不会再有健康调用。因此
//! [`note_launch_started`] 在登记正式启动的同时建立**后端截止任务**
//! （[`deadline_task`]）：睡到 `started_at + PENDING_GRACE`，先固定探测上下文，
//! 再自测一次健康——健康则确认，否则回滚。正常就绪会在截止前确认（标记随即
//! 被清除），迟到的任务自动退出，不会把已确认的运行时回滚掉。
//!
//! R-V8-1B-1（健康结果绑定发起时的探测上下文）：健康结果本身不携带身份，探测开始
//! **前**先由 [`claim_health_target`] 固定探测上下文（核心来源、目标发行版与端口、
//! 持有进程、对应 pending 的 stamp / 启动身份），完成时由 [`claim_resolves`] 核对
//! claim ↔ 当前世界 ↔ 当前标记三方一致——旧请求、另一目标的成功与失败都不得处置
//! 当前 pending；失败路径与成功、截止自测共用这一份身份规则。
//!
//! R-V8-1B-2（确认 / 回滚互斥）：确认、回滚、安装前解决（安装窗口内的回滚）与登记
//! （[`arm`] / [`note_launch_started`]）共享进程内处置互斥（[`disposal`]）——取得
//! 处置权后复核身份；回滚进入处置后确认不得再清其备份，已确认后回滚不得开始停服；
//! 结束时只清除 / 更新自己仍持有的记录，换代标记一概不碰。
//!
//! R-V8-1A-EXIT（截止自测期间实例退出的收尾）：探测结果的核对要求「持有进程与发起
//! 时一致」——探测**期间**本次实例退出（`Some(目标) → None`）会让核对不再成立，
//! 而此刻前端早已停止轮询、退出回调也不会补发健康请求（不存在「下一次探测」来重新
//! 绑定）。因此截止任务的失败路径在核对不成立时按**原 pending 身份**做一次重绑定
//! 核验（[`exit_rebind_matches`]）：标记 / 目标 / 启动身份未变、且本次实例确为
//! 「已退出」而不是被新实例替代，才在此完成回滚；换代标记、新启动、异目标与成功
//! 结果照旧一律忽略。重绑定入口与 [`resolve_claim`] 一样以**当前**核心来源为准
//! （REVIEW §J.1：历史 WSL 来源不等于当前仍是 WSL，用户切走核心造成的主动停服
//! 不得当成实例退出）。
//!
//! 前端不需要任何改动：确认挂在既有的 health 完成路径上，回滚挂在本就存在的
//! 启动失败路径上（方案 §0.3 规则 5：WSL 分支全部藏在 Rust 侧）。

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::AppHandle;

use super::exec::{decode_auto, summarize};
use super::runtime::{RuntimeBaseline, BASELINE_FILE_NAME, RUNTIME_DIR_NAME};
use super::{patch, probe, run_script, script};
use crate::config;

/// 待确认切换标记文件名（应用数据目录内；debug/release 随 `get_base_dir` 隔离）。
pub const MARKER_FILE_NAME: &str = "wsl-runtime-switch.json";
/// 回滚时失败的新运行时移出到的目录前缀（数据目录内）。
pub const FAILED_DIR_PREFIX: &str = "runtime-failed-";
/// readiness 宽限：`started_at` 之后超过它仍未健康才按「readiness 失败」回滚。
///
/// 前端两条停止路径：inactivity 180 s、absolute 300 s（`STARTUP_INACTIVITY_TIMEOUT`
/// / `STARTUP_ABSOLUTE_TIMEOUT`）；后端截止必须晚于两者，否则会把「慢但最终成功」
/// 的启动误判成失败并回滚掉（单测断言 `PENDING_GRACE > 300`）。
pub const PENDING_GRACE: Duration = Duration::from_secs(360);
/// 回滚脚本 / 确认清理的中继超时（停服走 `wsl_launch::stop_in_distro` 自带超时）。
const ROLLBACK_TIMEOUT: Duration = Duration::from_secs(120);
const CLEAN_TIMEOUT: Duration = Duration::from_secs(300);

/// 同一次待确认切换的**处置互斥**（R-V8-1B-2）：确认、回滚、安装前解决与登记
/// （`arm` / `note_launch_started`）串行化。取得处置权之后一律**重新读取并核对
/// 身份**才动作；确认用 `try_lock`——另一路处置（回滚 / 安装）正在进行时直接放弃，
/// 绝不清理回滚正在恢复的备份。
static DISPOSAL: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

fn disposal() -> &'static tokio::sync::Mutex<()> {
    DISPOSAL.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// 「待确认切换」标记：身份 = 发行版 + 数据目录（`$HOME` + 目录名）+ stamp
/// （脚本只按位置参数操作，不拼自由字符串）；`started_at` 记录正式启动尝试
/// （回滚宽限与后端截止的起点）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingSwitch {
    pub distro: String,
    /// Linux 侧 `$HOME` 绝对路径（确认前经 UNC 读运行时基线需要它）。
    pub home: String,
    /// 数据目录名（相对 `$HOME`，如 `.dsh-desktop.dev`）。
    pub dir_name: String,
    pub stamp: String,
    pub version: String,
    pub lock_sha256: String,
    pub switched_at: u64,
    #[serde(default)]
    pub started_at: Option<u64>,
    /// 回滚失败：保留全部文件，不再自动重试（人工可见、可恢复）。
    #[serde(default)]
    pub rollback_failed: bool,
}

impl PendingSwitch {
    pub fn new(
        distro: &str,
        home: &str,
        dir_name: &str,
        stamp: &str,
        version: &str,
        lock_sha256: &str,
    ) -> Self {
        Self {
            distro: distro.to_string(),
            home: home.to_string(),
            dir_name: dir_name.to_string(),
            stamp: stamp.to_string(),
            version: version.to_string(),
            lock_sha256: lock_sha256.to_string(),
            switched_at: unix_now(),
            started_at: None,
            rollback_failed: false,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// 解析标记；任一身份字段为空 / 缺失（或 `home` 不是绝对路径）→ `None`
    /// （损坏的标记按不存在处理，对应备份槽留给后续房务清理，绝不按半截标记动文件）。
    pub fn parse(content: &str) -> Option<Self> {
        let pending: Self = serde_json::from_str(content).ok()?;
        let complete = !pending.distro.is_empty()
            && pending.home.starts_with('/')
            && !pending.dir_name.is_empty()
            && !pending.stamp.is_empty()
            && !pending.version.is_empty()
            && !pending.lock_sha256.is_empty();
        complete.then_some(pending)
    }

    /// 标记是否指向这次启动目标（发行版 + 数据目录名；stamp 只影响清理范围）。
    #[cfg_attr(not(windows), allow(dead_code))] // 仅 Windows 调用路径可达（R-W3-9）
    pub fn matches(&self, distro: &str, dir_name: &str) -> bool {
        self.distro == distro && self.dir_name == dir_name
    }
}

/// 一次健康探测的**发起上下文**（R-V8-1B-1）：探测开始前固定，完成后据此核对。
///
/// 健康结果只说明「某个端点当时健康」，不携带「探测的是谁」。因此在发起探测前把
/// 当时的核心来源、目标（发行版 / 端口）、持有进程与待确认切换的 stamp / 启动身份
/// 一起固定下来；完成时 [`claim_resolves`] 要求 claim ↔ 当前世界 ↔ 当前标记三方
/// 一致，旧请求与异目标结果才不可能处置当前 pending。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthClaim {
    /// 探测发起时核心来源是 WSL。
    pub wsl_active: bool,
    /// 探测发起时 store 中的核心发行版。
    pub store_distro: Option<String>,
    /// 探测发起时持有进程的 (发行版, 数据目录)；`None` = 当时没有持有进程。
    pub owned: Option<(String, String)>,
    /// 探测所用端口。
    pub port: u16,
    /// 探测发起时待确认切换的 stamp（`None` = 当时没有待确认切换）。
    pub stamp: Option<String>,
    /// 探测发起时待确认切换的启动时刻（`None` = 尚未登记启动）。
    pub started_at: Option<u64>,
}

/// 在**发起探测前**固定本次探测上下文（R-V8-1B-1）。
///
/// 同步读取、探测期间不再依赖；调用方在 `await` 探测之前取好即可。
pub fn claim_health_target(app: &AppHandle, port: u16) -> HealthClaim {
    let marker = read(app);
    HealthClaim {
        wsl_active: crate::service::core::is_wsl_active(app),
        store_distro: config::get_store_dat_setting(app).wsl_distro,
        owned: crate::service::workflow::owned_wsl_target(),
        port,
        stamp: marker.as_ref().map(|pending| pending.stamp.clone()),
        started_at: marker.as_ref().and_then(|pending| pending.started_at),
    }
}

/// 本次探测的上下文是否指向 `pending` 这次切换（纯函数）。
///
/// 用于探测**发起前**的判定（后端截止任务先确认「要自测的就是本次切换」再探测）；
/// 完成后的判定用 [`claim_resolves`]，两者共用 [`target_matches`] 的目标规则。
#[cfg_attr(not(windows), allow(dead_code))] // 仅 Windows 调用路径可达（R-W3-9）
pub fn claim_matches_pending(claim: &HealthClaim, pending: &PendingSwitch) -> bool {
    let owned = claim
        .owned
        .as_ref()
        .map(|(distro, dir_name)| (distro.as_str(), dir_name.as_str()));
    claim.wsl_active
        && !pending.rollback_failed
        && claim.started_at.is_some()
        && claim.stamp.as_deref() == Some(pending.stamp.as_str())
        && claim.started_at == pending.started_at
        && target_matches(pending, claim.store_distro.as_deref(), owned)
}

/// 健康结果完成时的三方核对（纯函数，R-V8-1B-1）：claim（发起时上下文）、当前
/// 世界（核心发行版 / 端口 / 持有进程）与当前标记必须一致，且探测目标就是这次
/// pending——否则结果不归它（旧请求 / 另一目标 / 已换代），成功不得确认、失败
/// 不得回滚。
///
/// 「无法核对」一律不当作证据：核心来源不是 WSL、标记缺失 / 损坏 / 已换代、
/// 尚未登记启动、发行版 / 端口 / 持有进程与发起时不同 → `false`。持有进程必须与
/// 发起时完全一致（`None` 亦然）：不允许用「当前任意实例不在」代替「本次持有实例
/// 已退出」——退出信号由下一次探测在正确的目标上下文中重新绑定。
pub fn claim_resolves(
    claim: &HealthClaim,
    world_distro: Option<&str>,
    world_port: u16,
    world_owned: Option<(&str, &str)>,
    marker: Option<&PendingSwitch>,
) -> bool {
    if !claim.wsl_active {
        return false;
    }
    let Some(pending) = marker else {
        return false;
    };
    if pending.rollback_failed || claim.started_at.is_none() {
        return false;
    }
    if claim.stamp.as_deref() != Some(pending.stamp.as_str())
        || claim.started_at != pending.started_at
    {
        return false;
    }
    if claim.store_distro.as_deref() != world_distro || claim.port != world_port {
        return false;
    }
    let world_owned =
        world_owned.map(|(distro, dir_name)| (distro.to_string(), dir_name.to_string()));
    if world_owned != claim.owned {
        return false;
    }
    target_matches(
        pending,
        claim.store_distro.as_deref(),
        world_owned.as_ref().map(|(d, n)| (d.as_str(), n.as_str())),
    )
}

/// R-V8-1A-EXIT：截止失败路径的「本次实例已退出」重绑定判定（纯函数）。
///
/// 探测失败结果没能按发起时上下文解决（[`claim_resolves`] 为 `false`）时，典型原因
/// 是探测期间持有进程退出（`Some(目标) → None`）。前端已停止轮询、退出的实例也不
/// 会再补发健康请求，因此截止路径按**原 pending 身份**重新核对一次：
///
/// - claim 就是本次启动尝试发出的（stamp / 启动时刻一致）；
/// - 发起探测时持有进程就是 pending 的目标（排除异目标 / 未启动的结果）；
/// - 现在**已无持有进程**——「本次实例已退出」；被新实例替代（owned 仍为某个值，
///   即使是同一目标）与异目标一概不算，交给下一次探测；
/// - 标记仍是这次切换（未被确认 / 回滚 / 换代 / 重新登记启动 / 回滚失败）；
/// - 核心来源仍是 WSL，store 发行版与端口与发起时一致。
///
/// 只放宽「owned 由本次目标 → `None`」这一种情形；owned 比较本身不放宽。返回 `true`
/// 表示失败结果可按原 pending 处置（本判定只用于截止点的失败，不采信成功结果）。
#[cfg_attr(not(windows), allow(dead_code))] // 仅 Windows 调用路径可达（R-W3-9）
pub fn exit_rebind_matches(
    claim: &HealthClaim,
    world_distro: Option<&str>,
    world_port: u16,
    world_owned: Option<(&str, &str)>,
    marker: Option<&PendingSwitch>,
    expected: &PendingSwitch,
) -> bool {
    if !claim.wsl_active || expected.rollback_failed {
        return false;
    }
    if claim.stamp.as_deref() != Some(expected.stamp.as_str())
        || claim.started_at != expected.started_at
        || claim.started_at.is_none()
    {
        return false;
    }
    // 发起探测时持有的必须就是本次目标（异目标 / 未启动的结果不在此路径采信）。
    let claim_owned = claim
        .owned
        .as_ref()
        .map(|(distro, dir_name)| (distro.as_str(), dir_name.as_str()));
    if claim_owned != Some((expected.distro.as_str(), expected.dir_name.as_str())) {
        return false;
    }
    // 世界：来源 / store 发行版 / 端口与发起时一致，且已无持有进程（本次实例退出）；
    // 仍持有任何进程（被替代）都不算。
    if claim.store_distro.as_deref() != world_distro
        || claim.port != world_port
        || world_distro != Some(expected.distro.as_str())
        || world_owned.is_some()
    {
        return false;
    }
    marker_still_current(marker, expected)
}

/// [`claim_resolves`] 的实参版：读取当前世界与标记后判定，返回本次切换。
fn resolve_claim(app: &AppHandle, claim: &HealthClaim) -> Option<PendingSwitch> {
    if !crate::service::core::is_wsl_active(app) {
        return None;
    }
    let setting = config::get_store_dat_setting(app);
    let owned = crate::service::workflow::owned_wsl_target();
    let owned_ref = owned
        .as_ref()
        .map(|(distro, dir_name)| (distro.as_str(), dir_name.as_str()));
    let marker = read(app);
    if claim_resolves(
        claim,
        setting.wsl_distro.as_deref(),
        setting.port,
        owned_ref,
        marker.as_ref(),
    ) {
        marker
    } else {
        None
    }
}

/// [`exit_rebind_matches`] 的实参版：读取当前世界与标记后判定，返回本次切换。
///
/// 只读、不改变任何状态——处置仍由 `rollback` 在其自身的身份复核与共享互斥下执行。
#[cfg_attr(not(windows), allow(dead_code))] // 仅 Windows 调用路径可达（R-W3-9）
fn rebind_exited_target(
    app: &AppHandle,
    claim: &HealthClaim,
    expected: &PendingSwitch,
) -> Option<PendingSwitch> {
    // REVIEW §J.1：历史来源不等于当前来源——用户从 WSL 切到 app / local 会停掉原
    // 服务，但已选发行版与端口保持不变。必须像 [`resolve_claim`] 一样查询**当前**
    // 核心来源（不能用 claim 里探测发起时的布尔值替代），否则会把主动切换造成的
    // 停服当成「本次实例退出」而回滚旧树。
    if !crate::service::core::is_wsl_active(app) {
        return None;
    }
    let setting = config::get_store_dat_setting(app);
    let owned = crate::service::workflow::owned_wsl_target();
    let owned_ref = owned
        .as_ref()
        .map(|(distro, dir_name)| (distro.as_str(), dir_name.as_str()));
    let marker = read(app);
    if exit_rebind_matches(
        claim,
        setting.wsl_distro.as_deref(),
        setting.port,
        owned_ref,
        marker.as_ref(),
        expected,
    ) {
        marker
    } else {
        None
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn marker_path(app: &AppHandle) -> PathBuf {
    config::get_base_dir(app).join(MARKER_FILE_NAME)
}

/// 读取待确认切换标记；不存在 / 损坏 → `None`。
pub fn read(app: &AppHandle) -> Option<PendingSwitch> {
    PendingSwitch::parse(&std::fs::read_to_string(marker_path(app)).ok()?)
}

/// 写标记（原子性要求低：损坏按不存在处理，最坏只是备份晚一轮得到清理）。
pub fn write(app: &AppHandle, pending: &PendingSwitch) -> Result<(), String> {
    let path = marker_path(app);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            format!(
                "WSL_RUNTIME_SWITCH_MARKER_FAILED: {}: {e}",
                parent.display()
            )
        })?;
    }
    std::fs::write(&path, pending.to_json())
        .map_err(|e| format!("WSL_RUNTIME_SWITCH_MARKER_FAILED: {}: {e}", path.display()))
}

/// 删除标记（不存在视为已删除）。
pub fn clear(app: &AppHandle) {
    let path = marker_path(app);
    if let Err(e) = std::fs::remove_file(&path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            log::warn!(
                "[wsl-core] cannot remove switch marker {}: {e}",
                path.display()
            );
        }
    }
}

/// 该发行版是否有待确认切换（安装窗口的房务清理据此把关：有待确认就一个都不动）。
pub fn pending_for(app: &AppHandle, distro: &str) -> Option<PendingSwitch> {
    read(app).filter(|pending| pending.distro == distro)
}

/// 切换成功后登记待确认切换（`install::ensure` 在 `SWITCH_OK` 后调用；失败即中止）。
///
/// R-V8-1B ② / R-V8-1B-2：持有处置互斥再读——已有未完成的旧切换时**拒绝覆盖**——
/// 一次只允许一个待确认切换；调用方（`install::ensure`）须先把它确认或回滚掉再来
/// 登记（静默覆盖会让旧备份在没有任何记录的情况下被清理）。
pub async fn arm(
    app: &AppHandle,
    distro: &str,
    home: &str,
    dir_name: &str,
    stamp: &str,
    version: &str,
    lock_sha256: &str,
) -> Result<(), String> {
    let _disposal = disposal().lock().await;
    if let Some(existing) = read(app) {
        return Err(format!(
            "WSL_RUNTIME_SWITCH_MARKER_OCCUPIED: an unresolved runtime switch already exists \
             (distro {}, stamp {}); resolve it before arming another",
            existing.distro, existing.stamp
        ));
    }
    write(
        app,
        &PendingSwitch::new(distro, home, dir_name, stamp, version, lock_sha256),
    )
}

/// 后端截止时刻（`started_at + PENDING_GRACE`，饱和加）。
#[cfg_attr(not(windows), allow(dead_code))] // 仅 Windows 调用路径可达（R-W3-9）
pub fn deadline_at(started_at: u64) -> u64 {
    started_at.saturating_add(PENDING_GRACE.as_secs())
}

/// 标记是否仍是 `expected` 这一次切换（R-V8-1A/1B 共用的身份规则）：stamp 与启动
/// 时刻一致、目标未变、且未处于回滚失败状态。已被确认（无标记）/ 已回滚 / 被新
/// 切换替换 / 已重新登记启动时刻 → `false`（迟到的截止任务与健康结果据此退出）。
fn marker_still_current(marker: Option<&PendingSwitch>, expected: &PendingSwitch) -> bool {
    marker.is_some_and(|marker| {
        marker.stamp == expected.stamp
            && marker.started_at == expected.started_at
            && marker.distro == expected.distro
            && marker.dir_name == expected.dir_name
            && !marker.rollback_failed
    })
}

/// 纯函数版目标判定（单测覆盖；见 [`health_target_matches`]）：store 中的核心发行版
/// 必须就是 pending 的发行版；持有进程存在时其 (发行版, 数据目录) 也必须一致。
///
/// store 缺失（核心来源状态不完整）时返回 `false`——「无法核对」不当作确认证据。
pub fn target_matches(
    pending: &PendingSwitch,
    store_distro: Option<&str>,
    owned: Option<(&str, &str)>,
) -> bool {
    if store_distro != Some(pending.distro.as_str()) {
        return false;
    }
    if let Some((distro, dir_name)) = owned {
        return distro == pending.distro && dir_name == pending.dir_name;
    }
    true
}

/// 当前世界（store 发行版 + 持有进程）是否就是 pending 的目标（提交前复核用）。
fn health_target_matches(app: &AppHandle, pending: &PendingSwitch) -> bool {
    let store_distro = config::get_store_dat_setting(app).wsl_distro;
    let owned = crate::service::workflow::owned_wsl_target();
    let owned_ref = owned
        .as_ref()
        .map(|(distro, dir_name)| (distro.as_str(), dir_name.as_str()));
    target_matches(pending, store_distro.as_deref(), owned_ref)
}

/// `runtime/` 内的基线记录与 pending 是否一致（R-V8-1B「运行时基线」）：经 UNC 读
/// `$HOME/<dir_name>/runtime/.dsh-runtime.json`，比 version + 锁哈希；读取失败 /
/// 文件缺失 / 内容损坏 → `None`（无法核对，不作为确认证据）。
pub fn runtime_baseline_matches(pending: &PendingSwitch) -> Option<bool> {
    let linux_path = format!(
        "{}/{}/{RUNTIME_DIR_NAME}/{BASELINE_FILE_NAME}",
        pending.home, pending.dir_name
    );
    let path = PathBuf::from(patch::wsl_unc_path(&pending.distro, &linux_path));
    let baseline = RuntimeBaseline::parse(&std::fs::read_to_string(path).ok()?)?;
    Some(baseline.version == pending.version && baseline.lock_sha256 == pending.lock_sha256)
}

/// 正式启动尝试登记（WSL `spawn` 调用）：刷新 `started_at`，让回滚宽限从本次
/// 启动算起，并建立**后端截止任务**（R-V8-1A）——前端轮询停掉后由它收尾。
/// 没有待确认切换时是 no-op。
///
/// R-V8-1B-2：启动登记同样是标记变更，与确认 / 回滚共用处置互斥——确认的
/// 「读取-核对-清除」不会与这里的「读取-核对-改写」交错。
#[cfg_attr(not(windows), allow(dead_code))] // 仅 Windows 调用路径可达（R-W3-9）
pub async fn note_launch_started(app: &AppHandle, distro: &str, dir_name: &str) {
    let _disposal = disposal().lock().await;
    let Some(mut pending) = read(app) else {
        return;
    };
    if !pending.matches(distro, dir_name) || pending.rollback_failed {
        return;
    }
    pending.started_at = Some(unix_now());
    if let Err(e) = write(app, &pending) {
        log::warn!("[wsl-core] pending switch marker not updated on launch: {e}");
        return;
    }
    if let Some(started_at) = pending.started_at {
        log::info!(
            "[wsl-core] pending switch {}: backend deadline armed at {}",
            pending.stamp,
            deadline_at(started_at)
        );
    }
    tauri::async_runtime::spawn(deadline_task(app.clone(), pending));
}

/// 后端截止任务（R-V8-1A / R-V8-1B-1）：前端停止轮询后唯一的收尾路径。
///
/// 睡到截止点 → **先固定探测上下文**（[`claim_health_target`]）并确认它指的就是
/// 本次切换（被确认 / 已回滚 / 被替换 / 重新启动过 → 直接退出，不碰任何文件）→
/// 自测一次健康：成功按与前端健康路径完全相同的规则确认；失败则宽限已到点
/// （本任务就是宽限的终点），身份核对通过即回滚。
#[cfg_attr(not(windows), allow(dead_code))] // 仅 Windows 调用路径可达（R-W3-9）
async fn deadline_task(app: AppHandle, pending: PendingSwitch) {
    let Some(started_at) = pending.started_at else {
        return;
    };
    let wait = deadline_at(started_at).saturating_sub(unix_now());
    if wait > 0 {
        tokio::time::sleep(Duration::from_secs(wait)).await;
    }
    let port = config::get_store_dat_setting(&app).port;
    let claim = claim_health_target(&app, port);
    if !claim_matches_pending(&claim, &pending) {
        log::info!(
            "[wsl-core] switch deadline for stamp {} reached; the marker was already resolved \
             (confirmed / rolled back / replaced) or is not the active target; nothing to do",
            pending.stamp
        );
        return;
    }
    log::info!(
        "[wsl-core] pending switch {} reached its backend deadline; evaluating readiness",
        pending.stamp
    );
    let result = crate::service::workflow::proxy_health_check(port).await;
    if result.is_ok() {
        note_health(&app, &claim, &result).await;
        return;
    }
    // 先按发起时上下文核对；不成立时再考虑「探测期间本次实例退出」的重绑定
    // （R-V8-1A-EXIT）——前端已停止轮询，没有下一次探测能重新绑定。
    let verified = resolve_claim(&app, &claim).or_else(|| {
        let rebound = rebind_exited_target(&app, &claim, &pending);
        if rebound.is_some() {
            log::warn!(
                "[wsl-core] pending switch {}: the owned instance exited while the deadline probe \
                 was in flight; disposing of the failure under the pending identity",
                pending.stamp
            );
        }
        rebound
    });
    let Some(verified) = verified else {
        log::info!(
            "[wsl-core] deadline health result for stamp {} ignored: it does not resolve the \
             pending switch (stale request / another target / already resolved)",
            pending.stamp
        );
        return;
    };
    let detail = result.err().unwrap_or_default();
    log::warn!(
        "[wsl-core] pending switch {} is not ready at its backend deadline ({detail}); rolling back",
        verified.stamp
    );
    rollback(&app, &verified).await;
}

/// 回滚判定（纯函数）：只有「已经尝试过正式启动」才可能回滚——
/// `HARNESS_NOT_OWNED` 是进程已退出的确定信号；其余失败要超过宽限（覆盖
/// 「进程活着但 readiness 始终不通过」，以及用户已放弃的现场）。
pub fn rollback_reason(err: &str, started_at: Option<u64>, now: u64) -> Option<&'static str> {
    let started = started_at?;
    if err.starts_with("HARNESS_NOT_OWNED") {
        return Some("the Harness process exited after the runtime switch");
    }
    if now.saturating_sub(started) > PENDING_GRACE.as_secs() {
        return Some("readiness did not pass within the grace window");
    }
    None
}

/// 健康探测完成路径（`bridge::proxy_health_check` 调用）：确认或回滚。
///
/// R-V8-1B-1：完成时用**发起探测前**固定的上下文（[`HealthClaim`]）核对——
/// claim ↔ 当前世界 ↔ 当前标记三方一致才处置；旧请求、另一目标的结果直接忽略
/// （成功不确认、失败不回滚）。本函数不向调用方返回错误（健康结果原样返回给
/// 前端，切换处置只写日志）。
pub async fn note_health(app: &AppHandle, claim: &HealthClaim, result: &Result<String, String>) {
    let Some(pending) = resolve_claim(app, claim) else {
        if claim.stamp.is_some() {
            log::info!(
                "[wsl-core] health result ignored: it does not match the pending switch captured \
                 at request start (stale request / another target / already resolved)"
            );
        }
        return;
    };
    match result {
        Ok(_) => {
            try_confirm(app, &pending).await;
        }
        Err(err) => {
            if let Some(reason) = rollback_reason(err, pending.started_at, unix_now()) {
                log::warn!(
                    "[wsl-core] runtime switch not confirmed ({reason}); rolling back {}/{}",
                    pending.distro,
                    pending.stamp
                );
                rollback(app, &pending).await;
            }
        }
    }
}

/// 健康成功后的确认尝试（健康路径与截止任务共用；返回是否完成确认）。
///
/// R-V8-1B：所有身份证据齐备才清标记、清本次备份——未尝试启动的 pending、异目标
/// 的健康结果、基线不吻合的运行时都只记日志（标记与备份原样保留）。
pub async fn try_confirm(app: &AppHandle, pending: &PendingSwitch) -> bool {
    if pending.started_at.is_none() {
        log::debug!(
            "[wsl-core] health result ignored: the launch of pending switch {} has not started",
            pending.stamp
        );
        return false;
    }
    if !health_target_matches(app, pending) {
        log::debug!(
            "[wsl-core] health result ignored: not the target of pending switch {} ({})",
            pending.stamp,
            pending.distro
        );
        return false;
    }
    let checked = {
        let pending = pending.clone();
        tauri::async_runtime::spawn_blocking(move || runtime_baseline_matches(&pending)).await
    };
    match checked {
        Ok(Some(true)) => confirm(app, pending).await,
        Ok(Some(false)) | Ok(None) | Err(_) => {
            log::warn!(
                "[wsl-core] runtime baseline does not match pending switch {}; keeping the marker \
                 and the previous runtime",
                pending.stamp
            );
            false
        }
    }
}

/// 正式启动失败路径（`workflow::launch` 的 WSL 分支调用）：有待确认切换即回滚。
#[cfg_attr(not(windows), allow(dead_code))] // 仅 Windows 调用路径可达（R-W3-9）
pub async fn on_launch_failure(app: &AppHandle, distro: &str, dir_name: &str) {
    let Some(pending) = read(app) else {
        return;
    };
    if !pending.matches(distro, dir_name) || pending.rollback_failed {
        return;
    }
    log::warn!(
        "[wsl-core] WSL launch failed while a runtime switch is pending; rolling back {}/{}",
        distro,
        pending.stamp
    );
    rollback(app, &pending).await;
}

/// 确认（R-V8-1B-2）：取得处置权后复核身份，再清标记并把本次备份槽清理放后台。
///
/// `try_lock`：另一路处置（回滚 / 安装 / 并发确认）正在进行时不参与——回滚中的
/// 备份绝不能被这里清掉；拿到处置权后标记与目标再核对一次，任一不符即放弃。
/// 清理放后台：`rm -rf` 一棵运行时可能要数秒，不能拖慢前端 8 s 的健康探测超时。
async fn confirm(app: &AppHandle, pending: &PendingSwitch) -> bool {
    let Ok(_disposal) = disposal().try_lock() else {
        log::warn!(
            "[wsl-core] confirmation for stamp {} skipped: another disposition (rollback / \
             install) is in flight",
            pending.stamp
        );
        return false;
    };
    // 取得处置权后复核：迟到的健康成功不得清掉已经换代 / 已回滚的标记，也不能按
    // 旧 stamp 去清理备份；当前世界也必须仍是 pending 的目标。
    if !marker_still_current(read(app).as_ref(), pending) {
        log::debug!(
            "[wsl-core] confirmation for stamp {} skipped: the marker changed",
            pending.stamp
        );
        return false;
    }
    if !health_target_matches(app, pending) {
        log::debug!(
            "[wsl-core] confirmation for stamp {} skipped: the active target is no longer {}/{}",
            pending.stamp,
            pending.distro,
            pending.dir_name
        );
        return false;
    }
    clear(app);
    log::info!(
        "[wsl-core] runtime switch confirmed ({} stamp {}); cleaning the previous runtime in background",
        pending.version,
        pending.stamp
    );
    tauri::async_runtime::spawn(clean_slot(pending.clone()));
    true
}

/// 后台清理本次备份槽（只此一个；历史遗留槽位由安装窗口的房务清理处理）。
async fn clean_slot(pending: PendingSwitch) {
    let cleaned = run_script(
        &pending.distro,
        script::CLEAN_RUNTIME_BACKUP,
        vec![pending.dir_name.clone(), pending.stamp.clone()],
        CLEAN_TIMEOUT,
    )
    .await;
    match cleaned {
        Ok(out) if out.code == 0 => log::info!(
            "[wsl-core] previous runtime cleanup finished ({})",
            summarize(&out)
        ),
        Ok(out) => log::warn!(
            "[wsl-core] previous runtime cleanup exited with {}: {} (leftover slot kept for housekeeping)",
            out.code,
            summarize(&out)
        ),
        Err(e) => log::warn!(
            "[wsl-core] previous runtime cleanup failed (leftover slot kept for housekeeping): {e}"
        ),
    }
}

/// 回滚（受标记身份约束；R-V8-1B-2：与确认 / 安装共用处置互斥，取得处置权后复核）。
///
/// 返回 `true` = 本次 pending 已无需再处理（回滚完成，或标记已被确认 / 回滚 /
/// 替换）；`false` = 回滚确实失败（`rollbackFailed` 已落盘）。
pub async fn rollback(app: &AppHandle, pending: &PendingSwitch) -> bool {
    let _disposal = disposal().lock().await;
    // 取得处置权后复核标记仍是本次切换——已确认（无标记）/ 已被换代 / 已标记回滚
    // 失败都直接退出，绝不按过期身份停服或移动文件。
    if !marker_still_current(read(app).as_ref(), pending) {
        log::info!(
            "[wsl-core] rollback for stamp {} skipped: the switch marker was already resolved or \
             replaced",
            pending.stamp
        );
        return true;
    }
    rollback_now(app, pending).await
}

/// 未登记切换的回滚（`install::ensure` 的「标记写入失败」兜底专用）：该切换从未
/// 写进标记，没有身份可供核对（身份复核防的是「迟到结果清掉已换代的标记」，这里
/// 根本没有标记）；调用方刚刚完成这次切换，按它给出的身份直接处置。
pub(crate) async fn rollback_unarmed(app: &AppHandle, pending: &PendingSwitch) -> bool {
    let _disposal = disposal().lock().await;
    rollback_now(app, pending).await
}

/// 回滚的实际执行（调用方已持有处置互斥并完成身份复核 / 兜底判定）。
async fn rollback_now(app: &AppHandle, pending: &PendingSwitch) -> bool {
    // 1. 停掉可能仍在运行的本次新服务（幂等；按 pid 文件三重确认后在 Linux 侧
    //    按进程组 kill——中继死掉不会带走 Linux 子进程，F5）。
    #[cfg(windows)]
    {
        let stop_distro = pending.distro.clone();
        let stop_dir = pending.dir_name.clone();
        let stopped = tauri::async_runtime::spawn_blocking(move || {
            crate::service::workflow::stop_in_distro(&stop_distro, &stop_dir)
        })
        .await;
        match &stopped {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::warn!("[wsl-core] STOP before rollback failed (continuing): {e}"),
            Err(e) => log::warn!("[wsl-core] STOP join before rollback failed (continuing): {e}"),
        }
    }

    // 2. 移出失败的新树并恢复备份（脚本内完成；身份 = dir_name + stamp）。
    let rolled = run_script(
        &pending.distro,
        script::ROLLBACK_RUNTIME,
        vec![pending.dir_name.clone(), pending.stamp.clone()],
        ROLLBACK_TIMEOUT,
    )
    .await;
    let ok = matches!(
        &rolled,
        Ok(out) if out.code == 0 && {
            let text = decode_auto(&out.stdout);
            text.contains("ROLLBACK_OK") || text.contains("ROLLBACK_NOTHING")
        }
    );

    // 3. 刷新探测缓存：无论成败都用真实状态覆盖，避免 UI 显示过期结果。
    let probe_distro = pending.distro.clone();
    if let Ok(Ok(fresh)) =
        tauri::async_runtime::spawn_blocking(move || probe::probe(&probe_distro)).await
    {
        probe::store(fresh);
    }

    if ok {
        // R-V8-1B-2：收尾只清除自己仍持有的记录——处置期间标记若已被换代 / 更新
        // （互斥下理论不可达，仍防御），不碰它，也不按旧身份宣称清除。
        if marker_still_current(read(app).as_ref(), pending) {
            clear(app);
            log::info!(
                "[wsl-core] runtime rolled back to the previous tree (stamp {}); failed tree kept at \
                 ~/{}/{FAILED_DIR_PREFIX}{}",
                pending.stamp,
                pending.dir_name,
                pending.stamp
            );
        } else {
            log::info!(
                "[wsl-core] runtime rolled back to the previous tree (stamp {}); the marker changed \
                 meanwhile and was left untouched",
                pending.stamp
            );
        }
        return true;
    }

    let detail = match &rolled {
        Ok(out) => summarize(out),
        Err(e) => e.clone(),
    };
    if marker_still_current(read(app).as_ref(), pending) {
        let mut failed = pending.clone();
        failed.rollback_failed = true;
        if let Err(e) = write(app, &failed) {
            log::error!("[wsl-core] cannot persist rollback failure state: {e}");
        }
    } else {
        log::warn!(
            "[wsl-core] rollback for stamp {} failed but the marker changed meanwhile; not \
             persisting the failure onto it",
            pending.stamp
        );
    }
    log::error!(
        "[wsl-core] runtime rollback FAILED for {}/{}: {detail}; files kept: \
         ~/{}/runtime-backup-{} and ~/{}/{FAILED_DIR_PREFIX}{}; manual restore may be required",
        pending.distro,
        pending.stamp,
        pending.dir_name,
        pending.stamp,
        pending.dir_name,
        pending.stamp
    );
    false
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        claim_matches_pending, claim_resolves, deadline_at, exit_rebind_matches,
        marker_still_current, rollback_reason, target_matches, HealthClaim, PendingSwitch,
        PENDING_GRACE,
    };

    fn sample() -> PendingSwitch {
        PendingSwitch::new(
            "Ubuntu",
            "/home/pixel",
            ".dsh-desktop.dev",
            "1790869038",
            "0.1.2-rc.1",
            &"a".repeat(64),
        )
    }

    #[test]
    fn pending_switch_round_trips_and_rejects_incomplete() {
        let pending = sample();
        let parsed = PendingSwitch::parse(&pending.to_json()).expect("roundtrip");
        assert_eq!(parsed, pending);
        assert!(parsed.started_at.is_none());
        assert!(!parsed.rollback_failed);
        // 合法 JSON 但缺身份字段 / home 非绝对路径：按不存在处理（绝不按半截标记动文件）
        for bad in [
            "",
            "not json",
            r#"{"distro":"Ubuntu"}"#,
            r#"{"distro":"","home":"/home/pixel","dirName":".dsh-desktop.dev","stamp":"1","version":"0.1.2-rc.1","lockSha256":"a","switchedAt":1}"#,
            r#"{"distro":"Ubuntu","home":"relative/home","dirName":".dsh-desktop.dev","stamp":"1","version":"0.1.2-rc.1","lockSha256":"a","switchedAt":1}"#,
            r#"{"distro":"Ubuntu","home":"/home/pixel","dirName":".dsh-desktop.dev","stamp":"","version":"0.1.2-rc.1","lockSha256":"a","switchedAt":1}"#,
        ] {
            assert!(PendingSwitch::parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn pending_switch_tracks_start_and_failed_state() {
        let mut pending = sample();
        pending.started_at = Some(pending.switched_at + 5);
        pending.rollback_failed = true;
        let parsed = PendingSwitch::parse(&pending.to_json()).expect("roundtrip");
        assert_eq!(parsed.started_at, Some(pending.switched_at + 5));
        assert!(parsed.rollback_failed);
    }

    #[test]
    fn pending_switch_matches_distro_and_dir_only() {
        let pending = sample();
        assert!(pending.matches("Ubuntu", ".dsh-desktop.dev"));
        assert!(!pending.matches("Debian", ".dsh-desktop.dev"));
        assert!(!pending.matches("Ubuntu", ".dsh-desktop"));
    }

    /// R-V8-1B：健康成功必须绑定**实际运行目标**——store 中的核心发行版必须就是
    /// pending 的发行版；持有进程存在时其 (发行版, 数据目录) 也必须一致（另一
    /// 发行版同为 0.1.2-rc.1 也不能把本发行版的 pending 确认掉）。
    #[test]
    fn target_matches_binds_to_store_distro_and_owned_process() {
        let pending = sample();
        // 正确目标：无持有进程（探测在即）或持有进程一致都算匹配
        assert!(target_matches(&pending, Some("Ubuntu"), None));
        assert!(target_matches(
            &pending,
            Some("Ubuntu"),
            Some(("Ubuntu", ".dsh-desktop.dev"))
        ));
        // store 发行版不是本次切换的目标（典型：待确认 Ubuntu、当前核心是 Debian）
        assert!(!target_matches(
            &pending,
            Some("Debian"),
            Some(("Debian", ".dsh-desktop.dev"))
        ));
        // store 缺失（核心来源状态不完整）不作为确认证据
        assert!(!target_matches(&pending, None, None));
        // 持有进程属于另一发行版 / 另一数据目录
        assert!(!target_matches(
            &pending,
            Some("Ubuntu"),
            Some(("Debian", ".dsh-desktop.dev"))
        ));
        assert!(!target_matches(
            &pending,
            Some("Ubuntu"),
            Some(("Ubuntu", ".dsh-desktop"))
        ));
    }

    /// R-V8-1A/1B：迟到的截止任务 / 健康结果只在标记仍是本次切换时生效——
    /// 同 stamp + 同启动时刻 + 同目标 + 未回滚失败。
    #[test]
    fn marker_still_current_requires_same_identity() {
        let mut expected = sample();
        expected.started_at = Some(1_790_869_100);
        let expected = expected;

        let mut current = expected.clone();
        current.switched_at = 99; // 与身份无关的字段不影响判定
        assert!(marker_still_current(Some(&current), &expected));

        assert!(!marker_still_current(None, &expected)); // 已确认 / 已清理
        let mut restored = expected.clone();
        restored.started_at = None; // 未登记启动
        assert!(!marker_still_current(Some(&restored), &expected));
        let mut relaunched = expected.clone();
        relaunched.started_at = Some(expected.started_at.unwrap() + 1); // 重新启动过（新截止任务负责）
        assert!(!marker_still_current(Some(&relaunched), &expected));
        let mut replaced = expected.clone();
        replaced.stamp = "1790869999".to_string(); // 被更新的切换替换
        assert!(!marker_still_current(Some(&replaced), &expected));
        let mut failed = expected.clone();
        failed.rollback_failed = true; // 已标记回滚失败，不再自动重试
        assert!(!marker_still_current(Some(&failed), &expected));
    }

    /// R-V8-1A：后端截止必须晚于前端两条停止路径（180 s inactivity / 300 s
    /// absolute），否则「慢但最终成功」的启动会被迟到回滚掉；`deadline_at` 为
    /// 饱和加（时间倒流 / 极端值不 panic）。
    #[test]
    fn deadline_sits_after_frontend_polling_limits() {
        const FRONTEND_INACTIVITY_SECS: u64 = 180;
        const FRONTEND_ABSOLUTE_SECS: u64 = 300;
        assert!(PENDING_GRACE.as_secs() > FRONTEND_ABSOLUTE_SECS);
        assert!(PENDING_GRACE.as_secs() > FRONTEND_INACTIVITY_SECS);
        let started = 1_790_869_100u64;
        assert_eq!(deadline_at(started), started + PENDING_GRACE.as_secs());
        assert_eq!(deadline_at(u64::MAX), u64::MAX);
        assert!(Duration::from_secs(FRONTEND_ABSOLUTE_SECS) < PENDING_GRACE);
    }

    /// R-V8-1 的回滚判定：没试过启动就不回滚；`HARNESS_NOT_OWNED`（进程已退出）
    /// 立即回滚；其余失败只有在超过宽限（含「用户已放弃」）后才回滚。
    #[test]
    fn rollback_reason_requires_launch_attempt_and_respects_grace() {
        let now = 1_000_000u64;
        // 未尝试启动：任何失败都不回滚
        assert_eq!(rollback_reason("HARNESS_NOT_OWNED: x", None, now), None);
        assert_eq!(rollback_reason("HARNESS_NOT_READY: x", None, now), None);
        // 已尝试启动：进程退出 → 立即回滚
        assert!(
            rollback_reason("HARNESS_NOT_OWNED: no Harness process", Some(now - 3), now)
                .is_some_and(|reason| reason.contains("exited"))
        );
        // 已尝试启动：仍在宽限内不回滚（健康探测会继续轮询）
        assert_eq!(
            rollback_reason("HARNESS_NOT_READY: still starting", Some(now - 10), now),
            None
        );
        // 超过宽限：按 readiness 失败回滚
        let past = now - PENDING_GRACE.as_secs() - 1;
        assert!(rollback_reason("HARNESS_NOT_READY: x", Some(past), now)
            .is_some_and(|reason| reason.contains("grace")));
        // 恰好等于宽限：仍不算超时
        let edge = now - PENDING_GRACE.as_secs();
        assert_eq!(
            rollback_reason("HARNESS_NOT_READY: x", Some(edge), now),
            None
        );
    }

    /// R-V8-1B-1：探测上下文必须在**发起前**绑定——完成后按 claim ↔ 世界 ↔ 标记
    /// 三方核对；旧请求（启动身份已换代）、另一目标（发行版 / 端口 / 持有进程）与
    /// 已解决的标记都不能处置当前 pending；失败路径同样走这套规则。
    #[test]
    fn claim_binds_probe_to_request_time_identity() {
        let mut pending = sample();
        pending.started_at = Some(1_790_869_100);
        let pending = pending;
        let claim = HealthClaim {
            wsl_active: true,
            store_distro: Some("Ubuntu".into()),
            owned: Some(("Ubuntu".into(), ".dsh-desktop.dev".into())),
            port: 3081,
            stamp: Some(pending.stamp.clone()),
            started_at: pending.started_at,
        };
        // 发起时的现场：claim 指的就是这次 pending，完成后世界与标记未变 → 采信
        assert!(claim_matches_pending(&claim, &pending));
        assert!(claim_resolves(
            &claim,
            Some("Ubuntu"),
            3081,
            Some(("Ubuntu", ".dsh-desktop.dev")),
            Some(&pending)
        ));

        // 探测发给了另一发行版（待确认 Ubuntu、当前核心 Debian）：成功与失败都不得处置
        let mut other = claim.clone();
        other.store_distro = Some("Debian".into());
        other.owned = Some(("Debian".into(), ".dsh-desktop.dev".into()));
        assert!(!claim_matches_pending(&other, &pending));
        assert!(!claim_resolves(
            &other,
            Some("Debian"),
            3081,
            Some(("Debian", ".dsh-desktop.dev")),
            Some(&pending)
        ));

        // 迟到请求：探测发起后重新登记过启动（新一次尝试的截止任务负责）
        let mut relaunched = pending.clone();
        relaunched.started_at = Some(pending.started_at.unwrap() + 1);
        assert!(!claim_matches_pending(&claim, &relaunched));
        assert!(!claim_resolves(
            &claim,
            Some("Ubuntu"),
            3081,
            Some(("Ubuntu", ".dsh-desktop.dev")),
            Some(&relaunched)
        ));

        // 标记已解决（确认 / 清理）或已换代：
        assert!(!claim_resolves(
            &claim,
            Some("Ubuntu"),
            3081,
            Some(("Ubuntu", ".dsh-desktop.dev")),
            None
        ));
        let mut replaced = pending.clone();
        replaced.stamp = "1790869999".into();
        assert!(!claim_resolves(
            &claim,
            Some("Ubuntu"),
            3081,
            Some(("Ubuntu", ".dsh-desktop.dev")),
            Some(&replaced)
        ));
        let mut failed = pending.clone();
        failed.rollback_failed = true;
        assert!(!claim_resolves(
            &claim,
            Some("Ubuntu"),
            3081,
            Some(("Ubuntu", ".dsh-desktop.dev")),
            Some(&failed)
        ));

        // 当前世界与发起时不同：端口 / 发行版 / 持有进程任一变化都不采信
        assert!(!claim_resolves(
            &claim,
            Some("Ubuntu"),
            3082,
            Some(("Ubuntu", ".dsh-desktop.dev")),
            Some(&pending)
        ));
        assert!(!claim_resolves(
            &claim,
            Some("Debian"),
            3081,
            Some(("Ubuntu", ".dsh-desktop.dev")),
            Some(&pending)
        ));
        assert!(!claim_resolves(
            &claim,
            Some("Ubuntu"),
            3081,
            None,
            Some(&pending)
        ));
        assert!(!claim_resolves(
            &claim,
            Some("Ubuntu"),
            3081,
            Some(("Ubuntu", ".dsh-desktop")),
            Some(&pending)
        ));
        assert!(!claim_resolves(
            &claim,
            Some("Ubuntu"),
            3081,
            Some(("Debian", ".dsh-desktop.dev")),
            Some(&pending)
        ));

        // 探测时无持有进程、目标正确：这是「本次持有实例已退出」的信号——三方核对
        // 通过（回滚与否由 rollback_reason 决定，HARNESS_NOT_OWNED 立即回滚）
        let mut exited = claim.clone();
        exited.owned = None;
        assert!(claim_resolves(
            &exited,
            Some("Ubuntu"),
            3081,
            None,
            Some(&pending)
        ));
        assert!(rollback_reason(
            "HARNESS_NOT_OWNED: no Harness process",
            pending.started_at,
            pending.started_at.unwrap() + 5
        )
        .is_some());

        // 核心来源不是 WSL / 尚未登记启动：不得作为确认或回滚证据
        let mut not_wsl = claim.clone();
        not_wsl.wsl_active = false;
        assert!(!claim_resolves(
            &not_wsl,
            Some("Ubuntu"),
            3081,
            Some(("Ubuntu", ".dsh-desktop.dev")),
            Some(&pending)
        ));
        let mut unstarted = pending.clone();
        unstarted.started_at = None;
        let mut unstarted_claim = claim.clone();
        unstarted_claim.started_at = None;
        assert!(!claim_matches_pending(&unstarted_claim, &unstarted));
        assert!(!claim_resolves(
            &unstarted_claim,
            Some("Ubuntu"),
            3081,
            Some(("Ubuntu", ".dsh-desktop.dev")),
            Some(&unstarted)
        ));
    }

    /// R-V8-1A-EXIT：截止失败路径只在「发起探测时持有的是本次目标、现在已无持有
    /// 进程（本次实例退出）、标记 / 目标 / 端口未变」时重绑定处置——被新实例替代、
    /// 异目标、换代标记、重新启动与成功比所依赖的严格核对都不放宽。
    #[test]
    fn exit_rebind_only_covers_own_instance_exit_under_pending_identity() {
        let mut pending = sample();
        pending.started_at = Some(1_790_869_100);
        let pending = pending;
        let claim = HealthClaim {
            wsl_active: true,
            store_distro: Some("Ubuntu".into()),
            owned: Some(("Ubuntu".into(), ".dsh-desktop.dev".into())),
            port: 3081,
            stamp: Some(pending.stamp.clone()),
            started_at: pending.started_at,
        };
        // 探测期间本次实例退出（owned 由本次目标 → None）、标记与目标未变 → 重绑定
        assert!(exit_rebind_matches(
            &claim,
            Some("Ubuntu"),
            3081,
            None,
            Some(&pending),
            &pending
        ));

        // 仍持有某个进程（被新实例替代，即使同目标）：不算「退出」，交给下一次探测
        assert!(!exit_rebind_matches(
            &claim,
            Some("Ubuntu"),
            3081,
            Some(("Ubuntu", ".dsh-desktop.dev")),
            Some(&pending),
            &pending
        ));
        assert!(!exit_rebind_matches(
            &claim,
            Some("Ubuntu"),
            3081,
            Some(("Debian", ".dsh-desktop.dev")),
            Some(&pending),
            &pending
        ));
        // 发起时就没有持有进程：不是「本次实例退出」的信号（该现场走 claim_resolves）
        let mut never_owned = claim.clone();
        never_owned.owned = None;
        assert!(!exit_rebind_matches(
            &never_owned,
            Some("Ubuntu"),
            3081,
            None,
            Some(&pending),
            &pending
        ));
        // 发起时持有的是另一目标（异目标 / 数据目录）
        let mut other_owned = claim.clone();
        other_owned.owned = Some(("Debian".into(), ".dsh-desktop.dev".into()));
        assert!(!exit_rebind_matches(
            &other_owned,
            Some("Ubuntu"),
            3081,
            None,
            Some(&pending),
            &pending
        ));
        let mut other_dir = claim.clone();
        other_dir.owned = Some(("Ubuntu".into(), ".dsh-desktop".into()));
        assert!(!exit_rebind_matches(
            &other_dir,
            Some("Ubuntu"),
            3081,
            None,
            Some(&pending),
            &pending
        ));

        // 标记已解决（确认 / 回滚）、被换代、重新登记启动、已回滚失败：一概不处置
        assert!(!exit_rebind_matches(
            &claim,
            Some("Ubuntu"),
            3081,
            None,
            None,
            &pending
        ));
        let mut replaced = pending.clone();
        replaced.stamp = "1790869999".into();
        assert!(!exit_rebind_matches(
            &claim,
            Some("Ubuntu"),
            3081,
            None,
            Some(&replaced),
            &pending
        ));
        let mut relaunched = pending.clone();
        relaunched.started_at = Some(pending.started_at.unwrap() + 1);
        assert!(!exit_rebind_matches(
            &claim,
            Some("Ubuntu"),
            3081,
            None,
            Some(&relaunched),
            &pending
        ));
        let mut failed = pending.clone();
        failed.rollback_failed = true;
        assert!(!exit_rebind_matches(
            &claim,
            Some("Ubuntu"),
            3081,
            None,
            Some(&failed),
            &pending
        ));

        // 世界变化：store 发行版 / 端口与发起时不同
        assert!(!exit_rebind_matches(
            &claim,
            Some("Debian"),
            3081,
            None,
            Some(&pending),
            &pending
        ));
        assert!(!exit_rebind_matches(
            &claim,
            Some("Ubuntu"),
            3082,
            None,
            Some(&pending),
            &pending
        ));

        // 核心来源不是 WSL / 探测不是本次启动（旧启动尝试）发出的
        let mut not_wsl = claim.clone();
        not_wsl.wsl_active = false;
        assert!(!exit_rebind_matches(
            &not_wsl,
            Some("Ubuntu"),
            3081,
            None,
            Some(&pending),
            &pending
        ));
        let mut old_attempt = claim.clone();
        old_attempt.started_at = Some(pending.started_at.unwrap() - 1);
        assert!(!exit_rebind_matches(
            &old_attempt,
            Some("Ubuntu"),
            3081,
            None,
            Some(&pending),
            &pending
        ));
        let mut unstarted = claim.clone();
        unstarted.started_at = None;
        let mut unstarted_pending = pending.clone();
        unstarted_pending.started_at = None;
        assert!(!exit_rebind_matches(
            &unstarted,
            Some("Ubuntu"),
            3081,
            None,
            Some(&unstarted_pending),
            &unstarted_pending
        ));
    }
}
