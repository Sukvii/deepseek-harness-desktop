//! 浏览器/文件管理器唤起、日志捕获与健康检测。
//!
//! 对外部系统组件的交互：在系统浏览器打开链接、在文件管理器定位/打开目录与
//! 数据目录、复制服务地址到剪贴板；前端与后端日志的透传/读取/清空；以及通过
//! Rust 代理的服务健康检查与运行时环境诊断信息。

use crate::config;
use crate::logger;
use crate::service::{core, plugin, profile};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

/// 健康检查（通过 Rust 代理，避免 WebView CORS 问题）
///
/// R-V8-1：这也正是桌面 readiness 的完成路径——WSL 核心有待确认切换时，健康结果
/// 在这里驱动「确认（清理本次备份槽）」或「回滚（恢复切换前的运行时）」，前端
/// 启动状态机不需要任何改动。
///
/// R-V8-1B-1：探测开始**前**固定探测上下文（`claim_health_target`）——健康结果
/// 本身不携带身份，完成时按这份上下文核对（旧请求 / 另一目标的结果不得处置当前
/// 待确认切换）。
#[tauri::command]
pub async fn proxy_health_check(app_handle: AppHandle) -> Result<String, String> {
    let port = config::get_store_dat_setting(&app_handle).port;
    let claim = crate::service::wsl_core::switch::claim_health_target(&app_handle, port);
    let result = crate::service::workflow::proxy_health_check(port).await;
    crate::service::wsl_core::switch::note_health(&app_handle, &claim, &result).await;
    result
}

/// 运行时/版本/诊断信息（侧边栏展示）
#[tauri::command]
pub async fn get_runtime_info(app_handle: AppHandle) -> Result<config::RuntimeInfo, String> {
    let port = config::get_store_dat_setting(&app_handle).port;
    let mut info = config::runtime_info(&app_handle, port);
    // 实际生效的核心来源按后端判定写入（U3.1 的 `active_source`：平台 + 显式 WSL
    // 选择 + 非空发行版），前端据此对齐「谁在跑」——不自行拼 `active_core` 的状态
    // 组合（R-U9-4），也不为判来源额外联网。
    info.active_source = core::active_source(&app_handle).as_str().to_string();
    if core::is_wsl_active(&app_handle) {
        // WSL 核心（U7.1 / R-U9-2）：Linux 专属字段只认匹配发行版的 probe 缓存。
        // 缓存空时 `dsh_version` / `node_version` 为空、`data_dir` 取显式未取得
        // 标记——绝不 `.or()` / 兜底回落 Windows 宿主值，否则会把宿主的 dsh/node/
        // 数据根冒充发行版内的安装信息。桌面自身字段（app_version / service_url /
        // log_path / platform / arch）保持原义。
        let distro = config::get_store_dat_setting(&app_handle).wsl_distro;
        apply_wsl_runtime_info(&mut info, distro.as_deref(), wsl_cached_probe(distro.as_deref()));
    } else {
        info.dsh_version = core::active_version(&app_handle).or(info.dsh_version);
    }
    Ok(info)
}

/// WSL 分支的 Linux 专属字段取值口径（纯函数，便于单测锁定「不用宿主兜底」）。
///
/// Node 与数据根都先置为「未取得」再按 probe 填；`dsh_version` 由调用方用
/// `core::active_version`（WSL 来源，无缓存即 `None`）单独设置，不在此处理。
fn apply_wsl_runtime_info(
    info: &mut config::RuntimeInfo,
    distro: Option<&str>,
    probe: Option<WslProbeFacts>,
) {
    // 无论缓存是否命中都不保留宿主值：Node 与数据根先清空再按 probe 填。
    info.node_version = String::new();
    info.data_dir = DATA_DIR_PROBE_PENDING.to_string();
    let Some(facts) = probe else {
        return;
    };
    let Some(distro) = distro else {
        return;
    };
    info.node_version = facts.node_version;
    info.data_dir = crate::service::wsl_core::patch::wsl_unc_path(
        distro,
        &format!(
            "{}/{}",
            facts.home.trim_end_matches('/'),
            crate::service::wsl_core::dsh_home_dir_name()
        ),
    );
}

/// probe 缓存里与本函数相关的字段（探测结果里与诊断展示无关的部分不参与，便于单测构造）。
struct WslProbeFacts {
    node_version: String,
    home: String,
}

/// 读进程内 probe 缓存（只读，不触发探测/启动发行版）。
fn wsl_cached_probe(distro: Option<&str>) -> Option<WslProbeFacts> {
    let distro = distro?;
    let probed = crate::service::wsl_core::probe::cached(distro)?;
    Some(WslProbeFacts {
        node_version: probed.node_version.unwrap_or_default(),
        home: probed.home,
    })
}

/// 数据目录的「尚未探测」显式标记（R-U9-2：不用宿主路径兜底）。
const DATA_DIR_PROBE_PENDING: &str = "(WSL: not probed yet)";

/// 在系统浏览器中打开 Harness 界面
#[tauri::command]
pub async fn open_in_browser(app_handle: AppHandle) -> Result<(), String> {
    let url = config::get_dsh_service_url(config::get_store_dat_setting(&app_handle).port);
    app_handle
        .opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

/// 复制 Harness 服务地址到剪贴板
///
/// 走 `bridge::clipboard::write_clipboard_text`（惰性短期 `arboard` 句柄），规避
/// Linux Wayland 合成器不支持 data-control 时 `tauri-plugin-clipboard-manager`
/// 单例剪贴板导致的崩溃/挂死（同「复制日志」）。
#[tauri::command]
pub async fn copy_service_url(app_handle: AppHandle) -> Result<(), String> {
    let url = config::get_dsh_service_url(config::get_store_dat_setting(&app_handle).port);
    crate::bridge::write_clipboard_text(url).await
}

/// 在系统文件管理器中定位指定文件（Session 日志下载完成后的"在文件夹中显示"）
#[tauri::command]
pub fn reveal_in_folder(app_handle: AppHandle, path: String) -> Result<(), String> {
    // 安全边界：只允许定位允许根目录（下载目录/数据目录/$DSH_HOME）内的文件，
    // 防止第三方插件通过 IPC 驱动宿主打开任意路径。
    if !crate::bridge::guard::is_allowed_path(&app_handle, std::path::Path::new(&path)) {
        return Err(format!("REVEAL_PATH_REJECTED: {path}"));
    }
    tauri_plugin_opener::reveal_item_in_dir(&path).map_err(|e| format!("REVEAL_FAILED: {e}"))
}

/// 在系统文件管理器中打开指定目录（核心版本「打开目录」按钮；目录用 open 而非
/// reveal——reveal 是定位父目录，open 是直接打开该目录本身）。
#[tauri::command]
pub fn open_dir(app_handle: AppHandle, path: String) -> Result<(), String> {
    // 安全边界同 reveal_in_folder：仅允许打开允许根目录内的目录
    if !crate::bridge::guard::is_allowed_path(&app_handle, std::path::Path::new(&path)) {
        return Err(format!("OPEN_DIR_REJECTED: {path}"));
    }
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| format!("OPEN_DIR_FAILED: {e}"))
}

/// 在系统文件管理器中打开数据目录（官方 `$DSH_HOME`，即 `~/.dsh`）。
///
/// `path` 为屏幕上显示的那个数据目录（`get_runtime_info` 的 `data_dir`），由前端
/// 原样回传：R-U9-3 之前本命令无条件打开**Windows** `$DSH_HOME`，而 WSL 核心下
/// 面板显示的是发行版内的 UNC 数据根，点按钮会进到另一个环境的数据目录（还会顺手
/// 创建 Windows 目录）。现在：
/// - 给了合法且存在的路径 → 走与 `open_dir` 同一套允许根校验后打开（WSL 数据根的
///   UNC 白名单来自 `bridge::guard::allowed_roots` 的 U7.2 受限根，不因此扩大）；
/// - 路径是「WSL 尚未探测」标记 → 直接拒绝，**不悄悄回落 Windows 目录**；
/// - 其它不可用形态 → 拒绝；
/// - 未给路径 → 仅当当前不是 WSL 核心时打开 Windows 数据目录（兼容旧调用）。
#[tauri::command]
pub async fn reveal_data_dir(app_handle: AppHandle, path: Option<String>) -> Result<(), String> {
    match resolve_reveal_target(
        path.as_deref(),
        core::is_wsl_active(&app_handle),
        DATA_DIR_PROBE_PENDING,
    ) {
        RevealTarget::Custom(target) => {
            if !crate::bridge::guard::is_allowed_path(&app_handle, std::path::Path::new(&target)) {
                return Err(format!("REVEAL_PATH_REJECTED: {target}"));
            }
            tauri_plugin_opener::open_path(&target, None::<&str>)
                .map_err(|e| format!("REVEAL_FAILED: {e}"))
        }
        RevealTarget::HostDefault => {
            let dsh_home = config::get_dsh_data_path(&app_handle);
            // 目录可能尚未创建（全新安装），先建好再打开，避免资源管理器报路径不存在
            std::fs::create_dir_all(&dsh_home).map_err(|e| e.to_string())?;
            tauri_plugin_opener::open_path(&dsh_home, None::<&str>).map_err(|e| e.to_string())
        }
        RevealTarget::None(reason) => Err(reason.to_string()),
    }
}

/// 数据目录按钮的取值决策（纯函数，便于单测锁定「显示与打开同一目标」）。
#[derive(Debug, PartialEq, Eq)]
enum RevealTarget<'a> {
    /// 打开前端回传的路径（仍需过允许根校验）
    Custom(&'a str),
    /// 前端未回传路径：退回宿主数据目录（仅非 WSL 核心）
    HostDefault,
    /// 无可用目标：带拒绝原因，绝不回落宿主目录
    None(&'static str),
}

fn resolve_reveal_target<'a>(
    path: Option<&'a str>,
    wsl_active: bool,
    probe_pending_marker: &str,
) -> RevealTarget<'a> {
    match path {
        // WSL 核心 + 未探测：屏幕上没有真实数据根，不能拿 Windows 目录顶替
        Some(pending) if pending == probe_pending_marker => {
            RevealTarget::None("REVEAL_DATA_DIR_UNPROBED: WSL data dir is not probed yet")
        }
        // 空串按「未回传」处理（旧前端只 invoke 不带参数）
        Some(target) if !target.trim().is_empty() => RevealTarget::Custom(target),
        _ if wsl_active => {
            RevealTarget::None("REVEAL_DATA_DIR_UNPROBED: WSL data dir is not probed yet")
        }
        _ => RevealTarget::HostDefault,
    }
}

/// 前端日志透传：前端 `console.*` 劫持经此命令落盘到 `desktop.frontdesk.log`
/// （与持有后端 + `dsh` target 的 `desktop.log` 分离），见 `logger/mod.rs`。
#[tauri::command]
pub fn log_frontend(level: String, target: String, message: String) {
    let lvl = logger::FrontendLevel::from_str(&level);
    logger::log_frontend(lvl, &target, &message);
}

/// 按字节上限取 `s` 的尾部，并在裁剪起点回退到 UTF-8 字符边界。
///
/// 日志必然包含中文/ANSI 等多字节字符，直接用
/// `&s[s.len() - max_bytes..]` 在起点落在字符中间时会 panic
/// （`byte index ... is not a char boundary`），此实现保证安全。
fn tail_bytes(s: &str, max_bytes: usize) -> &str {
    let start = s.len().saturating_sub(max_bytes);
    let mut i = start;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    &s[i..]
}

/// 读取 dsh 服务日志
#[tauri::command]
pub async fn read_service_logs(
    app_handle: AppHandle,
    max_bytes: Option<usize>,
) -> Result<String, String> {
    let log_path = config::get_service_log_path(&app_handle);
    if !log_path.exists() {
        return Ok(String::new());
    }

    let content = std::fs::read_to_string(&log_path).map_err(|e| e.to_string())?;
    let max_bytes = max_bytes.unwrap_or(64 * 1024);
    if content.len() <= max_bytes {
        Ok(content)
    } else {
        Ok(tail_bytes(&content, max_bytes).to_string())
    }
}

/// 清空 dsh 服务日志
#[tauri::command]
pub async fn clear_service_logs(app_handle: AppHandle) -> Result<(), String> {
    let log_path = config::get_service_log_path(&app_handle);
    std::fs::write(&log_path, "").map_err(|e| e.to_string())
}

/// 拼装「复制日志」的环境信息段（纯函数，便于单测锁定报障格式）。
///
/// 报障时最常缺的就是「当前档案」与「装了哪些插件、什么版本」：插件问题几乎都
/// 与档案内的插件组合相关，没有这两项只能反复向用户追问。因此这里在原有的
/// app/dsh/node/os 之外补 `profile` 与 `plugins` 两块。
///
/// `dsh_version` 缺失（核心未安装）时整行省略：`dsh: -` 会让用户误以为核心在
/// 但版本不明。插件为 0 时只留 `plugins: 0 installed` 一行：计数本身已说明没有插件，
/// 再补 `(none)` 反而与「读取失败」一样无法区分，徒增噪音。
fn format_env_info(
    app_version: &str,
    dsh_version: Option<&str>,
    node_version: &str,
    os: &str,
    arch: &str,
    profile: &str,
    plugins: &[String],
) -> String {
    let mut lines = vec![format!("app: {app_version}")];
    if let Some(version) = dsh_version {
        lines.push(format!("dsh: {version}"));
    }
    lines.push(format!("node: {node_version}"));
    lines.push(format!("os: {os} ({arch})"));
    lines.push(format!("profile: {profile}"));
    lines.push(format!("plugins: {} installed", plugins.len()));
    lines.extend(plugins.iter().map(|p| format!("  - {p}")));
    lines.join("\n")
}

/// 读取运行日志（DSH 服务日志 + 桌面端 Rust 运行日志），格式化为便于
/// 反馈/报障复制的纯文本块：`### 环境信息`、`### 服务日志`、`### 前台日志`
/// 与 `### 后台日志` 四段。
///
/// 服务日志来自 `logs/dsh-web.log`（debug 构建为 `logs/dsh-web.dev.log`）；
/// 运行日志来自 `logs/desktop.log`（桌面端自身 `logger::init` 每次启动落盘，
/// 见 logger/mod.rs）。前端 `console.*` 已在 logger 的文件层按 `target: "frontend"`
/// 跳过、不会写入 `desktop.log`（见 logger/mod.rs），因此「后台日志」取的是纯后端
/// `log::*`；仅对旧版本已落盘、尚未轮转掉的 `frontend:` 行做一次兜底剔除。据此把
/// 「运行日志」拆成：
/// - `### 前台日志`：取自前端独立文件 `logs/desktop.frontdesk.log`
///   （`logger::init` 单独落盘，见 logger/mod.rs），含壳层前端 `console.*` 与
///   注入脚本从 dsh iframe 转回的帧内 console/未捕获异常（标识 `[iframe]`，
///   见 desktop/frame_log.rs）；
/// - `### 后台日志`：取自 `logs/desktop.log`，剔除残余 `target: "frontend"` 行，
///   仅保留后端 `log::*`。
/// 每段取末尾最多 `MAX_LINES` 行（前端日志量大，仅取一半 `FRONTEND_MAX_LINES`），
/// 避免粘贴内容超出 GitHub issue 长度上限。
#[tauri::command]
pub async fn read_run_logs(app_handle: AppHandle) -> Result<String, String> {
    const MAX_LINES: usize = 100;
    // 前端日志量大，复制的行数减半（避免粘贴内容过长）；后端仍取满 MAX_LINES
    const FRONTEND_MAX_LINES: usize = MAX_LINES / 2;

    let base = config::get_base_dir(&app_handle);
    let service = config::get_service_log_path(&app_handle);
    let desktop = base.join("logs").join("desktop.log");
    let frontend = base.join("logs").join("desktop.frontdesk.log");

    // 环境信息：桌面端应用版本、dsh 发行版本、Node 版本、系统平台/架构，以及当前
    // 档案名与已安装插件列表，便于报障时快速定位环境差异。
    // 插件名取 npm 包名（profile `dependencies` 的依赖键），与 `dsh plugin <cmd> <id>`
    // 及插件面板一致；版本解析不出时只留包名，避免出现读起来像被截断的 `name@`。
    let dsh_version =
        core::active_version(&app_handle).or_else(|| config::get_dsh_version(&app_handle));
    let mut plugins = plugin::watch::list(&app_handle);
    // 稳定排序：插件面板按加载顺序展示，报障块按包名字典序，便于两次日志对比差异。
    // 必须在拼上 `@version` 之前排，否则 `foo@1` 会排到 `foo-bar@1` 之后。
    plugins.sort_by(|a, b| a.id.cmp(&b.id));
    let plugin_lines: Vec<String> = plugins
        .iter()
        .map(|p| {
            if p.version.is_empty() {
                p.id.clone()
            } else {
                format!("{}@{}", p.id, p.version)
            }
        })
        .collect();
    let app_version = app_handle.package_info().version.to_string();
    let env_text = format_env_info(
        &app_version,
        dsh_version.as_deref(),
        &config::get_active_node_version(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        &profile::active_profile(&app_handle),
        &plugin_lines,
    );

    let service_text = read_tail(&service, MAX_LINES, false);
    let frontend_text = read_tail(&frontend, FRONTEND_MAX_LINES, false);
    let backend_text = read_tail(&desktop, MAX_LINES, true);

    Ok(format!(
        "### 环境信息\n\n{}\n\n### 服务日志\n\n```\n{}\n```\n\n### 前台日志\n\n```\n{}\n```\n\n### 后台日志\n\n```\n{}\n```",
        env_text,
        service_text.trim_end(),
        frontend_text.trim_end(),
        backend_text.trim_end()
    ))
}

fn read_tail(path: &std::path::Path, max_lines: usize, filter_frontend: bool) -> String {
    if !path.exists() {
        return String::new();
    }
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = content
        .lines()
        .filter(|line| !filter_frontend || !is_frontend_log_line(line))
        .collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

fn is_frontend_log_line(line: &str) -> bool {
    const LEVELS: [&str; 5] = ["TRACE", "DEBUG", "INFO", "WARN", "ERROR"];
    let trimmed = line.trim_start();
    LEVELS
        .iter()
        .any(|lvl| trimmed.contains(&format!("{lvl} frontend:")))
}

/// 在系统浏览器中打开任意 http(s) 链接（更新说明 / 关于对话框仓库链接等）
#[tauri::command]
pub async fn open_external_url(app_handle: AppHandle, url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(format!("EXTERNAL_URL_INVALID: {url}"));
    }
    app_handle
        .opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::format_env_info;
    use super::is_frontend_log_line;
    use super::tail_bytes;
    use super::RevealTarget;
    use super::WslProbeFacts;
    use super::DATA_DIR_PROBE_PENDING;

    /// R-U9-2：WSL 分支在没有 probe 缓存时，不得用宿主 Node / 数据根兜底。
    #[test]
    fn wsl_runtime_info_without_probe_clears_host_fields() {
        let mut info = host_info();
        super::apply_wsl_runtime_info(&mut info, Some("Ubuntu"), None);
        assert_eq!(info.node_version, "");
        assert_eq!(info.data_dir, DATA_DIR_PROBE_PENDING);
        // 宿主字段保持原义
        assert_eq!(info.platform, "windows");
        assert_eq!(info.log_path, r"C:\Users\u\AppData\dsh\logs\dsh-web.log");
        assert_eq!(info.active_source, "wsl");
    }

    /// R-U9-2：有 probe 缓存时 Node / 数据根取发行版内的值（UNC），仍不带宿主值。
    #[test]
    fn wsl_runtime_info_with_probe_uses_distro_values() {
        let mut info = host_info();
        super::apply_wsl_runtime_info(
            &mut info,
            Some("Ubuntu"),
            Some(WslProbeFacts {
                node_version: "v22.11.0".to_string(),
                home: "/home/pixel/".to_string(),
            }),
        );
        assert_eq!(info.node_version, "v22.11.0");
        assert!(info.data_dir.starts_with("\\\\wsl.localhost\\Ubuntu\\home\\pixel\\"));
        assert!(info.data_dir.contains(crate::service::wsl_core::dsh_home_dir_name()));
        assert!(!info.data_dir.contains("AppData"));
    }

    /// R-U9-2：发行版缺失（缓存已清但来源判定仍为 wsl）时同样不回落宿主值。
    #[test]
    fn wsl_runtime_info_without_distro_does_not_fall_back() {
        let mut info = host_info();
        super::apply_wsl_runtime_info(
            &mut info,
            None,
            Some(WslProbeFacts {
                node_version: "v22.11.0".to_string(),
                home: "/home/pixel".to_string(),
            }),
        );
        assert_eq!(info.node_version, "");
        assert_eq!(info.data_dir, DATA_DIR_PROBE_PENDING);

        let mut info = host_info();
        super::apply_wsl_runtime_info(&mut info, Some("   "), None);
        assert_eq!(info.data_dir, DATA_DIR_PROBE_PENDING);
    }

    /// 构造一份「宿主（Windows）形态」的基础运行时信息作为对照。
    fn host_info() -> crate::config::RuntimeInfo {
        crate::config::RuntimeInfo {
            app_version: "0.21.0".to_string(),
            dsh_version: Some("0.2.0-rc.2".to_string()),
            node_version: "22.22.0".to_string(),
            service_url: "http://127.0.0.1:37321".to_string(),
            data_dir: r"C:\Users\u\.dsh".to_string(),
            log_path: r"C:\Users\u\AppData\dsh\logs\dsh-web.log".to_string(),
            platform: "windows".to_string(),
            arch: "x86_64".to_string(),
            active_source: "wsl".to_string(),
        }
    }

    /// R-U9-3：数据目录按钮必须与屏幕上显示的数据根指向同一目标。
    #[test]
    fn reveal_target_follows_displayed_data_dir() {
        // 屏幕上显示 WSL UNC → 打开同一 UNC（不再打开 Windows 目录）
        let unc = "\\\\wsl.localhost\\Ubuntu\\home\\pixel\\.dsh-desktop";
        assert_eq!(
            super::resolve_reveal_target(Some(unc), true, DATA_DIR_PROBE_PENDING),
            RevealTarget::Custom(unc)
        );
        // 屏幕上显示 Windows 数据根 → 打开同一 Windows 目录
        assert_eq!(
            super::resolve_reveal_target(Some(r"C:\Users\u\.dsh"), false, DATA_DIR_PROBE_PENDING),
            RevealTarget::Custom(r"C:\Users\u\.dsh")
        );
    }

    /// R-U9-3：WSL 核心且未探测时拒绝打开，绝不悄悄回落 Windows 目录。
    #[test]
    fn reveal_target_refuses_host_fallback_under_wsl() {
        let pending = super::resolve_reveal_target(Some(DATA_DIR_PROBE_PENDING), true, DATA_DIR_PROBE_PENDING);
        assert!(matches!(pending, RevealTarget::None(reason) if reason.contains("UNPROBED")));
        // 旧调用形态（不带 path）+ WSL 核心：同样拒绝
        let no_path = super::resolve_reveal_target(None, true, DATA_DIR_PROBE_PENDING);
        assert!(matches!(no_path, RevealTarget::None(_)));
        let empty = super::resolve_reveal_target(Some(""), true, DATA_DIR_PROBE_PENDING);
        assert!(matches!(empty, RevealTarget::None(_)));
        let blank = super::resolve_reveal_target(Some("   "), true, DATA_DIR_PROBE_PENDING);
        assert!(matches!(blank, RevealTarget::None(_)));
    }

    /// 非 WSL 核心保持旧行为：不带路径时打开宿主数据目录。
    #[test]
    fn reveal_target_defaults_to_host_when_not_wsl() {
        assert_eq!(
            super::resolve_reveal_target(None, false, DATA_DIR_PROBE_PENDING),
            RevealTarget::HostDefault
        );
        assert_eq!(
            super::resolve_reveal_target(Some(""), false, DATA_DIR_PROBE_PENDING),
            RevealTarget::HostDefault
        );
    }

    #[test]
    fn log_tail_filters_frontend_before_selecting_last_lines() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("dsh-shell-log-tail-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("desktop.log");
        std::fs::write(
            &path,
            "INFO dsh: first\r\nINFO frontend: noisy\r\nWARN dsh: 中文\nINFO frontend: last\n",
        )
        .unwrap();
        assert_eq!(
            super::read_tail(&path, 2, true),
            "INFO dsh: first\nWARN dsh: 中文"
        );
        assert_eq!(
            super::read_tail(&path, 2, false),
            "WARN dsh: 中文\nINFO frontend: last"
        );
        assert_eq!(super::read_tail(&path, 0, true), "");
        assert_eq!(super::read_tail(&root.join("missing"), 100, false), "");
        assert_eq!(super::read_tail(&root, 100, true), "");
        std::fs::write(&path, [0xff]).unwrap();
        assert_eq!(super::read_tail(&path, 100, false), "");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn env_info_carries_profile_and_plugins() {
        let text = format_env_info(
            "0.19.0",
            Some("0.1.7-rc.2"),
            "24.19.0",
            "windows",
            "x86_64",
            "web",
            &[
                "dshmarket@0.22.1".to_string(),
                "@scope/tool@1.2.3".to_string(),
            ],
        );
        assert_eq!(
            text,
            "app: 0.19.0\ndsh: 0.1.7-rc.2\nnode: 24.19.0\nos: windows (x86_64)\nprofile: web\nplugins: 2 installed\n  - dshmarket@0.22.1\n  - @scope/tool@1.2.3"
        );
    }

    #[test]
    fn env_info_omits_dsh_line_when_version_unknown() {
        let text = format_env_info("0.19.0", None, "24.19.0", "linux", "aarch64", "tauri", &[]);
        assert_eq!(
            text,
            "app: 0.19.0\nnode: 24.19.0\nos: linux (aarch64)\nprofile: tauri\nplugins: 0 installed"
        );
        assert!(!text.contains("dsh:"));
    }

    #[test]
    fn env_info_empty_plugin_list_is_just_a_count() {
        let text = format_env_info(
            "1.0.0",
            Some("0.1.7"),
            "22.19.0",
            "macos",
            "aarch64",
            "web",
            &[],
        );
        // 计数行自身已表达「没有插件」，不再补 `(none)` 之类的占位行
        assert!(text.ends_with("plugins: 0 installed"));
        assert!(!text.contains("(none)"));
    }

    #[test]
    fn env_info_keeps_plugin_without_version_readable() {
        // 版本解析失败时只留包名，不能出现 `name@` 这种看起来被截断的形态
        let text = format_env_info(
            "1.0.0",
            Some("0.1.7"),
            "22.19.0",
            "windows",
            "x86_64",
            "web",
            &["dsh-broken".to_string()],
        );
        assert!(text.contains("plugins: 1"));
        assert!(text.contains("dsh-broken"));
        assert!(!text.contains("dsh-broken@"));
    }

    #[test]
    fn frontend_line_detected() {
        // tracing 文件层（desktop.log）与前端独立文件（desktop.frontdesk.log）两种时间戳格式都应命中
        assert!(is_frontend_log_line(
            "2024-06-01 12:00:00.123Z INFO frontend: [tag] message"
        ));
        assert!(is_frontend_log_line(
            "[2024-06-01 12:00:00.123Z] INFO frontend: message"
        ));
        assert!(is_frontend_log_line(
            "2024-06-01 12:00:00.123Z WARN frontend: something"
        ));
        assert!(is_frontend_log_line(
            "2024-06-01 12:00:00.123Z ERROR frontend: boom"
        ));
    }

    #[test]
    fn backend_line_not_detected() {
        // 后端（dsh 等 target）不应误判为前端；消息正文里出现 "frontend" 也不应命中
        assert!(!is_frontend_log_line(
            "2024-06-01 12:00:00.123Z INFO dsh: starting server"
        ));
        assert!(!is_frontend_log_line(
            "[2024-06-01 12:00:00.123Z] INFO dsh: emit to frontend: 3"
        ));
        assert!(!is_frontend_log_line(
            "2024-06-01 12:00:00.123Z DEBUG reqwest: GET /ping"
        ));
    }

    #[test]
    fn frontend_level_padding_and_extra_spaces() {
        // 级别可能带前导空格（`{:>5}` 或 tracing 层多空格），frontend 目标仍应命中
        assert!(is_frontend_log_line(
            "2024-06-01 12:00:00.123Z  INFO frontend: padded"
        ));
    }

    #[test]
    fn tail_bytes_keeps_ascii_within_limit() {
        assert_eq!(tail_bytes("hello world", 5), "world");
        // 起点已落在字符边界时原样截取
        assert_eq!(tail_bytes("abc", 2), "bc");
    }

    #[test]
    fn tail_bytes_advances_to_char_boundary() {
        // 截取起点落在 3 字节中文中间 → 回退到字符边界，不 panic 且结果 ≤ max_bytes
        assert_eq!(tail_bytes("中a", 2), "a");
        // 4 字节 emoji 同理（非边界前缀字节会连续回退）
        assert_eq!(tail_bytes("😀x", 3), "x");
        // 多字节 + 超限，回退后长度仍不超过 max_bytes
        assert_eq!(tail_bytes("中文abc", 3), "abc");
    }

    #[test]
    fn tail_bytes_shorter_than_limit_returns_whole() {
        assert_eq!(tail_bytes("中文", 10), "中文");
        assert_eq!(tail_bytes("", 10), "");
    }
}
