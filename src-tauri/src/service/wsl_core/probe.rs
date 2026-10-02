//! WSL 内核心探测：运行 PROBE 脚本、解析结果、进程内缓存。
//!
//! 缓存供 `active_version`、`core::list` 的 WSL 行与 W3 的启动路径读取；
//! 键为发行版名（WSL 的 NAME 即主键，同名发行版不共存）。
//!
//! v8 起探测目标是**受控运行时**（`$HOME/<数据目录名>/runtime`，见
//! [`super::runtime`]）：`dsh` / `npm_root` / `skip_auth_ready` 都只反映它，
//! 用户全局安装的 dsh 不参与（不会被当成「已安装」）。

use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use super::dsh_home_dir_name;
use super::exec::{run_in_distro, summarize};
use super::script::PROBE;

/// 探测超时：`npm root -g` 首次可能较慢，给足余量。
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// WSL 内核心探测结果（序列化 camelCase 给前端）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WslCoreProbe {
    /// 发行版名（`wsl -l` 中的 NAME）
    pub distro: String,
    /// `command -v node` 输出；`None` = 未安装 Node
    pub node: Option<String>,
    /// `node -v` 输出；`None` = 不可用
    pub node_version: Option<String>,
    /// 受控运行时内 `.bin/dsh` 的绝对路径；`None` = 运行时未安装 / 不可执行
    pub dsh: Option<String>,
    /// `dsh --version` 输出；`None` = 不可用
    pub dsh_version: Option<String>,
    /// 受控运行时的 `node_modules`（补丁目标定位根；运行时缺失时为空）
    pub npm_root: Option<String>,
    /// 受控运行时目录（绝对路径，用于 UI 展示与确认框；运行时缺失时该目录不存在）
    pub runtime_dir: String,
    /// Linux 侧 `$HOME`（数据目录 UNC 化用；必须是绝对路径，R-4）
    pub home: String,
    /// `--skip-auth` 两层补丁是否已就位（PROBE 只读判定；W3 以此决定能否启动）
    pub skip_auth_ready: bool,
}

/// 进程内探测缓存：值带写入时刻，供 [`cached_fresh`] 判断新鲜度（R-W3-6）。
fn cache() -> &'static Mutex<HashMap<String, (WslCoreProbe, std::time::Instant)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (WslCoreProbe, std::time::Instant)>>> =
        OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 写入 / 刷新进程内探测缓存（探测、安装、补丁完成后调用）。
pub fn store(probe: WslCoreProbe) {
    cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(probe.distro.clone(), (probe, std::time::Instant::now()));
}

/// 读取进程内探测缓存（不看新鲜度）；未探测过返回 `None`。
pub fn cached(distro: &str) -> Option<WslCoreProbe> {
    cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(distro)
        .map(|(probe, _)| probe.clone())
}

/// 读取「足够新」的缓存：写入时刻距今不超过 `max_age` 才返回（R-W3-6）。
///
/// 开机路径上 `runtime_ready` 刚探测过几百毫秒，`launch_wsl` 的启动前探测可以
/// 直接复用，省掉一次 2–7 s 的完整探测；超过窗口仍走现场探测（`skip_auth_ready`
/// 这类判定的可信度依赖「刚刚测过」，R-W2-2）。
#[cfg_attr(not(windows), allow(dead_code))] // 仅 Windows 调用路径可达（R-W3-9）
pub fn cached_fresh(distro: &str, max_age: std::time::Duration) -> Option<WslCoreProbe> {
    cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(distro)
        .filter(|(_, at)| at.elapsed() <= max_age)
        .map(|(probe, _)| probe.clone())
}

/// 有缓存用缓存，否则现场探测一次（阻塞 ≤ [`PROBE_TIMEOUT`]，必须在
/// `spawn_blocking` 内调用）。供 `runtime_ready` / `set_active("wsl")` 等需要真实
/// 答案的路径——缓存是进程内的，应用重启即空（R-W2-1）；`core::list` 保持只读
/// 缓存（列表必须快）。
pub fn cached_or_probe(distro: &str) -> Result<WslCoreProbe, String> {
    cached(distro).map(Ok).unwrap_or_else(|| probe(distro))
}

/// 运行 PROBE 脚本探测受控运行时（node / npm / dsh 与补丁状态），成功后写入缓存。
///
/// 数据目录名（[`dsh_home_dir_name`]）经位置参数传入，`$HOME` 由发行版内的 bash
/// 展开（与 START / STOP 同一口径）。
///
/// `home` 不是绝对路径（脚本/环境异常）→ `Err("WSL_PROBE_INVALID_HOME")`，
/// 且**不写缓存**（R-4：避免用坏 home 拼出错误的 UNC 数据目录）。退出码 97 表示
/// 前导的 `cd "$HOME"` 失败 → `WSL_HOME_UNAVAILABLE`（R-W2-4）。
pub fn probe(distro: &str) -> Result<WslCoreProbe, String> {
    let output = run_in_distro(distro, PROBE, &[dsh_home_dir_name()], PROBE_TIMEOUT)?;
    if output.code == 97 {
        return Err(format!("WSL_HOME_UNAVAILABLE: cd $HOME failed in {distro}"));
    }
    if output.code != 0 {
        return Err(format!(
            "WSL_PROBE_FAILED: PROBE exited with {} in {distro}: {}",
            output.code,
            summarize(&output)
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let parsed = parse_probe_output(distro, &text)?;
    store(parsed.clone());
    Ok(parsed)
}

/// 空值 → `None`（PROBE 中 `|| true` 兜底会产出 `KEY=` 行）。
fn opt(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// 解析 PROBE 的 `KEY=VALUE` 输出（纯函数，便于单测）。
fn parse_probe_output(distro: &str, text: &str) -> Result<WslCoreProbe, String> {
    let mut probe = WslCoreProbe {
        distro: distro.to_string(),
        node: None,
        node_version: None,
        dsh: None,
        dsh_version: None,
        npm_root: None,
        runtime_dir: String::new(),
        home: String::new(),
        skip_auth_ready: false,
    };
    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "NODE" => probe.node = opt(value),
            "NODEV" => probe.node_version = opt(value),
            "DSH" => probe.dsh = opt(value),
            "DSHV" => probe.dsh_version = opt(value),
            "ROOT" => probe.npm_root = opt(value),
            "RUNTIME" => probe.runtime_dir = value.trim().to_string(),
            "HOME" => probe.home = value.trim().to_string(),
            "SKIPAUTH" => probe.skip_auth_ready = value.trim() == "1",
            // NPM：本版结构体不保留 npm 路径，直接忽略
            _ => {}
        }
    }
    if !probe.home.starts_with('/') {
        return Err(format!(
            "WSL_PROBE_INVALID_HOME: {distro}: {:?}",
            probe.home
        ));
    }
    Ok(probe)
}

#[cfg(test)]
mod tests {
    use super::{cached, cached_fresh, parse_probe_output, store, WslCoreProbe};

    #[test]
    fn cached_is_none_for_unknown_distro() {
        // 用不会与真实发行版重名的键，避免测试互相污染
        assert!(cached("dsh-wsl-core-test-nonexistent").is_none());
    }

    #[test]
    fn parse_probe_output_reads_keys_and_empty_values() {
        let text = "NODE=/usr/bin/node\nNODEV=v22.22.1\nNPM=/usr/bin/npm\nDSH=\nDSHV=\nROOT=\nRUNTIME=/home/pixel/.dsh-desktop.dev/runtime\nHOME=/home/pixel\nSKIPAUTH=1\n";
        let probe = parse_probe_output("Ubuntu", text).expect("probe should parse");
        assert_eq!(probe.distro, "Ubuntu");
        assert_eq!(probe.node.as_deref(), Some("/usr/bin/node"));
        assert_eq!(probe.node_version.as_deref(), Some("v22.22.1"));
        assert_eq!(probe.dsh, None, "空值必须归 None");
        assert_eq!(probe.dsh_version, None);
        assert_eq!(probe.npm_root, None, "运行时缺失时补丁根为空");
        assert_eq!(probe.runtime_dir, "/home/pixel/.dsh-desktop.dev/runtime");
        assert_eq!(probe.home, "/home/pixel");
        assert!(probe.skip_auth_ready, "SKIPAUTH=1 必须解析为 true");
    }

    #[test]
    fn parse_probe_output_defaults_skip_auth_false_when_missing() {
        let text = "NODE=/usr/bin/node\nHOME=/home/pixel\n";
        let probe = parse_probe_output("Ubuntu", text).unwrap();
        assert!(!probe.skip_auth_ready);
        let text = "NODE=/usr/bin/node\nHOME=/home/pixel\nSKIPAUTH=0\n";
        assert!(!parse_probe_output("Ubuntu", text).unwrap().skip_auth_ready);
    }

    #[test]
    fn parse_probe_output_rejects_relative_or_empty_home() {
        for text in ["HOME=\n", "HOME=home/pixel\n", "NODE=/usr/bin/node\n"] {
            let err = parse_probe_output("Ubuntu", text).unwrap_err();
            assert!(err.starts_with("WSL_PROBE_INVALID_HOME"), "{err}");
        }
    }

    /// R-W3-6：`cached_fresh` 只在窗口内返回缓存，超窗视为未探测（走现场探测）。
    #[test]
    fn cached_fresh_respects_max_age() {
        let distro = "FreshAgeTest";
        let probe = WslCoreProbe {
            distro: distro.to_string(),
            node: Some("/usr/bin/node".to_string()),
            node_version: Some("v22.22.1".to_string()),
            dsh: Some("/home/u/.dsh-desktop.dev/runtime/node_modules/.bin/dsh".to_string()),
            dsh_version: Some("0.1.2-rc.1".to_string()),
            npm_root: Some("/home/u/.dsh-desktop.dev/runtime/node_modules".to_string()),
            runtime_dir: "/home/u/.dsh-desktop.dev/runtime".to_string(),
            home: "/home/u".to_string(),
            skip_auth_ready: true,
        };
        store(probe);
        assert!(cached_fresh(distro, std::time::Duration::from_secs(15)).is_some());
        assert!(cached_fresh(distro, std::time::Duration::ZERO).is_none());
        // `cached` 不看新鲜度，语义不变
        assert!(cached(distro).is_some());
        assert!(cached_fresh("NeverProbed", std::time::Duration::from_secs(15)).is_none());
    }

    #[test]
    fn store_then_cached_round_trips() {
        let probe = WslCoreProbe {
            distro: "dsh-wsl-core-test-roundtrip".to_string(),
            node: Some("/usr/bin/node".to_string()),
            node_version: Some("v22.22.1".to_string()),
            dsh: Some("/home/pixel/.dsh-desktop.dev/runtime/node_modules/.bin/dsh".to_string()),
            dsh_version: Some("0.1.2-rc.1".to_string()),
            npm_root: Some("/home/pixel/.dsh-desktop.dev/runtime/node_modules".to_string()),
            runtime_dir: "/home/pixel/.dsh-desktop.dev/runtime".to_string(),
            home: "/home/pixel".to_string(),
            skip_auth_ready: true,
        };
        store(probe.clone());
        assert_eq!(cached("dsh-wsl-core-test-roundtrip"), Some(probe));
    }
}
