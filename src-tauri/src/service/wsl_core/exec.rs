//! `wsl.exe` 调用与输出解码。
//!
//! 硬性规则（`DSH-WSL-CORE-PLAN.md` §0.3）：
//! - 只用 `-e`（exec）形态，动态值（发行版名、脚本位置参数）只作数组参数，
//!   绝不拼进 `bash -lc` 的脚本字符串；
//! - `wsl.exe` 自身输出为 UTF-16LE（去 BOM/NUL），Linux 子进程输出为 UTF-8。
//!
//! 全部 `wsl.exe` 调用以 `#[cfg(windows)]` 门控；非 Windows 平台为空实现（返回错误）。

use serde::Serialize;
use std::time::Duration;

#[cfg(windows)]
use std::time::Instant;

/// 隐藏子进程控制台窗口（Win32 `CREATE_NO_WINDOW`）。
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 子进程输出流标记（流式转发时区分 stdout / stderr）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WslStream {
    Stdout,
    Stderr,
}

/// `wsl -l -v` 中的一行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WslDistro {
    /// 发行版名（NAME 列，可含空格）
    pub name: String,
    /// 状态列（Running / Stopped / Installing / …）
    pub state: String,
    /// WSL 版本列（`1` / `2`）
    pub version: String,
    /// 是否为默认发行版（名称列带 `*` 前缀）
    pub is_default: bool,
}

/// 一次 `wsl.exe` 调用的结果（原始字节，交由调用方按 F9 选择解码方式）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WslOutput {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// UTF-16LE 字节 → String（去 BOM 与 NUL 填充；尾部残字节忽略）。
pub fn decode_utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let text = String::from_utf16_lossy(&units);
    let without_bom = text.strip_prefix('\u{feff}').unwrap_or(&text);
    without_bom.replace('\0', "")
}

/// 自动判别输出编码：含 NUL 字节 → UTF-16LE（`wsl.exe` 自身的错误信息），
/// 否则按 UTF-8 lossy（Linux 子进程输出，F9）。
pub fn decode_auto(bytes: &[u8]) -> String {
    if bytes.contains(&0) {
        decode_utf16(bytes)
    } else {
        String::from_utf8_lossy(bytes).to_string()
    }
}

/// 从一次调用的输出里取一句人类可读的摘要：stderr 优先、否则 stdout，
/// 首个非空行、≤160 字符（拼进所有非零退出的 `Err` 文本，R-W2-9）。
pub fn summarize(output: &WslOutput) -> String {
    let stderr = decode_auto(&output.stderr);
    let stdout = decode_auto(&output.stdout);
    let first = stderr
        .lines()
        .chain(stdout.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("no output");
    first.chars().take(160).collect()
}

/// 解析 `wsl -l -v` 文本（纯函数，便于单测）：跳过表头/空行，过滤 `docker-desktop*`。
///
/// 列解析从右侧切分：最后一列是 WSL 版本（`1`/`2`），倒数第二列是状态，
/// 其余全部归名称——发行版名允许含空格。
fn parse_distro_list(text: &str) -> Vec<WslDistro> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let (is_default, rest) = match line.strip_prefix('*') {
            Some(rest) => (true, rest.trim_start()),
            None => (false, line),
        };
        let fields: Vec<&str> = rest.split_whitespace().collect();
        if fields.len() < 3 {
            continue;
        }
        let version = fields[fields.len() - 1].to_string();
        let state = fields[fields.len() - 2].to_string();
        let name = fields[..fields.len() - 2].join(" ");
        // 按列语义过滤（R-W2-10）：VERSION 列不是 `1` / `2` 的行一律跳过——表头
        // （英文 `NAME` 或任意本地化写法）的版本列是单词，天然被过滤。
        if !matches!(version.as_str(), "1" | "2") {
            continue;
        }
        if name.starts_with("docker-desktop") {
            continue;
        }
        out.push(WslDistro {
            name,
            state,
            version,
            is_default,
        });
    }
    out
}

/// `wsl.exe --list --verbose` → 发行版列表（过滤 `docker-desktop*`，标默认）。
pub fn list_distros() -> Result<Vec<WslDistro>, String> {
    let output = run_wsl(&["--list", "--verbose"], Duration::from_secs(20))?;
    if output.code != 0 {
        return Err(format!(
            "WSL_LIST_FAILED: wsl.exe --list exited with {}: {}",
            output.code,
            summarize(&output)
        ));
    }
    // 输出为 UTF-16LE（F9）；个别版本写 stderr，stdout 为空时回退。
    let mut text = decode_utf16(&output.stdout);
    if text.trim().is_empty() {
        text = decode_utf16(&output.stderr);
    }
    Ok(parse_distro_list(&text))
}

/// 在发行版内执行常量脚本：`wsl.exe -d <distro> -e bash -lc <script> bash <args…>`。
///
/// 脚本是常量，动态值只经位置参数传入、不经 shell 展开（方案 §0.3 规则 1）。
pub fn run_in_distro(
    distro: &str,
    script: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<WslOutput, String> {
    let argv = distro_argv(distro, script, args);
    run_wsl(&argv, timeout)
}

/// 与 [`run_in_distro`] 相同，但把每个输出行实时回调（WSL 内安装等长任务用）。
pub fn run_in_distro_streaming<F>(
    distro: &str,
    script: &str,
    args: &[&str],
    timeout: Duration,
    on_line: F,
) -> Result<WslOutput, String>
where
    F: FnMut(WslStream, &str),
{
    let argv = distro_argv(distro, script, args);
    run_wsl_streaming(&argv, timeout, on_line)
}

/// 拼装 `-e` 形态的 argv（纯函数）：`-d <distro> -e bash -lc <script> bash <args…>`。
fn distro_argv<'a>(distro: &'a str, script: &'a str, args: &[&'a str]) -> Vec<&'a str> {
    let mut argv: Vec<&str> = vec!["-d", distro, "-e", "bash", "-lc", script, "bash"];
    argv.extend_from_slice(args);
    argv
}

#[cfg(windows)]
fn spawn_wsl(args: &[&str]) -> Result<std::process::Child, String> {
    spawn_wsl_at(&wsl_exe_path(), args)
}

/// `wsl.exe` 的绝对路径：`%SystemRoot%\System32\wsl.exe`（`SystemRoot` 缺失时回落
/// `C:\Windows`）。
///
/// **所有** `wsl.exe` 调用都必须经它解析（R-W4-2）：`Command::new("wsl.exe")` 按
/// 「可执行文件所在目录 > PATH > System32」解析（F16），而 Tauri 按用户安装时
/// 应用目录（`%LOCALAPPDATA%\…`）对当前用户可写——同用户的任何进程放一个同名
/// 文件进去就会被本应用以用户权限执行（探测输出与安装脚本都会交给它）。
/// W1 审核 R-1 引入本函数正是不依赖搜索顺序；`core::source` 以 `pub use` 转发它，
/// 保持既有调用点不变（避免 `wsl_core` → `core` 的反向引用）。
pub fn wsl_exe_path() -> std::path::PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    std::path::PathBuf::from(root)
        .join("System32")
        .join("wsl.exe")
}

/// 用指定的可执行文件拉起 `wsl.exe`（拆出来便于单测「可执行文件不可用」的错误
/// 映射：`WSL_EXEC_FAILED` + 绝对路径 + `os error N`，R-W4-2）。
#[cfg(windows)]
fn spawn_wsl_at(exe: &std::path::Path, args: &[&str]) -> Result<std::process::Child, String> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new(exe)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("WSL_EXEC_FAILED: {} {}: {e}", exe.display(), args.join(" ")))
}

/// 轮询等待子进程退出；到 deadline 仍未退出返回 `None`（由调用方 kill）。
#[cfg(windows)]
fn wait_child(child: &mut std::process::Child, deadline: Instant) -> Result<Option<i32>, String> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(Some(status.code().unwrap_or(-1))),
            Ok(None) => {
                if Instant::now() >= deadline {
                    return Ok(None);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(format!("WSL_EXEC_FAILED: wait failed: {e}")),
        }
    }
}

/// 有界 join：读线程可能因孙进程持有管道而阻塞，超时则放弃该缓冲（返回 `None`）。
#[cfg(windows)]
fn join_bounded<T: Send + 'static>(
    handle: std::thread::JoinHandle<T>,
    grace: Duration,
) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(handle.join().ok());
    });
    rx.recv_timeout(grace).ok().flatten()
}

#[cfg(windows)]
fn run_wsl_raw(args: &[&str], timeout: Duration) -> Result<WslOutput, String> {
    use std::io::Read;

    let mut child = spawn_wsl(args)?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "WSL_EXEC_FAILED: stdout pipe unavailable".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "WSL_EXEC_FAILED: stderr pipe unavailable".to_string())?;

    let out_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });

    let deadline = Instant::now() + timeout;
    let code = match wait_child(&mut child, deadline)? {
        Some(code) => code,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "WSL_TIMEOUT: wsl.exe {} exceeded {}s",
                args.join(" "),
                timeout.as_secs()
            ));
        }
    };
    let stdout = join_bounded(out_thread, Duration::from_secs(5)).unwrap_or_default();
    let stderr = join_bounded(err_thread, Duration::from_secs(5)).unwrap_or_default();
    Ok(WslOutput {
        code,
        stdout,
        stderr,
    })
}

#[cfg(windows)]
fn run_wsl_lines<F>(args: &[&str], timeout: Duration, mut on_line: F) -> Result<WslOutput, String>
where
    F: FnMut(WslStream, &str),
{
    use std::io::{BufRead, BufReader};
    use std::sync::mpsc;

    fn read_lines<R: std::io::Read + Send + 'static>(
        reader: R,
        stream: WslStream,
        tx: mpsc::Sender<(WslStream, String)>,
    ) {
        for line in BufReader::new(reader).lines() {
            match line {
                Ok(line) => {
                    if tx.send((stream, line)).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    }

    let mut child = spawn_wsl(args)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "WSL_EXEC_FAILED: stdout pipe unavailable".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "WSL_EXEC_FAILED: stderr pipe unavailable".to_string())?;

    let (tx, rx) = mpsc::channel::<(WslStream, String)>();
    let tx_out = tx.clone();
    let out_thread = std::thread::spawn(move || read_lines(stdout, WslStream::Stdout, tx_out));
    let err_thread = std::thread::spawn(move || read_lines(stderr, WslStream::Stderr, tx));

    let deadline = Instant::now() + timeout;
    let mut stdout_lines: Vec<String> = Vec::new();
    let mut stderr_lines: Vec<String> = Vec::new();
    loop {
        let now = Instant::now();
        if now >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "WSL_TIMEOUT: wsl.exe {} exceeded {}s",
                args.join(" "),
                timeout.as_secs()
            ));
        }
        match rx.recv_timeout(deadline - now) {
            Ok((stream, line)) => {
                on_line(stream, &line);
                match stream {
                    WslStream::Stdout => stdout_lines.push(line),
                    WslStream::Stderr => stderr_lines.push(line),
                }
            }
            // 到 deadline 由循环顶部统一 kill
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let code = match wait_child(&mut child, deadline)? {
        Some(code) => code,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "WSL_TIMEOUT: wsl.exe {} exceeded {}s",
                args.join(" "),
                timeout.as_secs()
            ));
        }
    };
    let _ = join_bounded(out_thread, Duration::from_secs(5));
    let _ = join_bounded(err_thread, Duration::from_secs(5));
    Ok(WslOutput {
        code,
        stdout: join_lines(&stdout_lines),
        stderr: join_lines(&stderr_lines),
    })
}

#[cfg(windows)]
fn join_lines(lines: &[String]) -> Vec<u8> {
    let mut text = lines.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    text.into_bytes()
}

/// 运行 `wsl.exe <args>` 并捕获原始输出；超时杀进程并返回 `WSL_TIMEOUT`。
#[cfg(windows)]
pub fn run_wsl(args: &[&str], timeout: Duration) -> Result<WslOutput, String> {
    run_wsl_raw(args, timeout)
}

/// 运行 `wsl.exe <args>`，逐行实时回调（lossy UTF-8、已去行尾换行），并返回拼接后的完整输出。
#[cfg(windows)]
pub fn run_wsl_streaming<F>(
    args: &[&str],
    timeout: Duration,
    on_line: F,
) -> Result<WslOutput, String>
where
    F: FnMut(WslStream, &str),
{
    run_wsl_lines(args, timeout, on_line)
}

#[cfg(not(windows))]
pub fn run_wsl(_args: &[&str], _timeout: Duration) -> Result<WslOutput, String> {
    Err("WSL_UNAVAILABLE: wsl.exe is only available on Windows".to_string())
}

#[cfg(not(windows))]
pub fn run_wsl_streaming<F>(
    _args: &[&str],
    _timeout: Duration,
    _on_line: F,
) -> Result<WslOutput, String>
where
    F: FnMut(WslStream, &str),
{
    Err("WSL_UNAVAILABLE: wsl.exe is only available on Windows".to_string())
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::spawn_wsl_at;
    use super::{
        decode_auto, decode_utf16, distro_argv, parse_distro_list, summarize, wsl_exe_path,
        WslDistro, WslOutput,
    };

    fn utf16le(text: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn decode_utf16_strips_bom_and_nul_padding() {
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend_from_slice(&utf16le("Ubuntu  Running  2\0\0"));
        assert_eq!(decode_utf16(&bytes), "Ubuntu  Running  2");
    }

    #[test]
    fn decode_auto_switches_on_nul_bytes() {
        let utf16 = utf16le("Ubuntu  Running  2\0\0");
        assert_eq!(decode_auto(&utf16), "Ubuntu  Running  2");
        assert_eq!(decode_auto("plain utf-8".as_bytes()), "plain utf-8");
        assert_eq!(decode_auto("\u{4e0d} exists".as_bytes()), "\u{4e0d} exists");
    }

    #[test]
    fn summarize_prefers_stderr_first_non_empty_line() {
        let output = WslOutput {
            code: 127,
            stdout: utf16le("distro not found\r\n"),
            stderr: Vec::new(),
        };
        assert_eq!(summarize(&output), "distro not found");
        let output = WslOutput {
            code: 1,
            stdout: b"stdout line".to_vec(),
            stderr: b"\n\nstderr line\n".to_vec(),
        };
        assert_eq!(summarize(&output), "stderr line");
    }

    #[test]
    fn parse_distro_list_skips_non_english_header_rows() {
        let text = "NOM             \u{00c9}TAT        VERSION\n* Ubuntu        Running     2\n";
        let rows = parse_distro_list(&decode_utf16(&utf16le(text)));
        assert_eq!(rows.len(), 1, "非英文表头不得成为伪发行版: {rows:?}");
        assert_eq!(rows[0].name, "Ubuntu");
    }

    #[test]
    fn parse_distro_list_handles_default_docker_and_spaced_names() {
        let text = "\u{feff}  NAME            STATE           VERSION\n* Ubuntu          Running         2\n  Debian          Stopped         2\n  My Dev Box      Running         1\n  docker-desktop  Stopped         2\n  docker-desktop-data  Stopped    2\n";
        let rows = parse_distro_list(&decode_utf16(&utf16le(text)));
        assert_eq!(rows.len(), 3, "docker-desktop* 与表头都要被过滤: {rows:?}");
        assert_eq!(
            rows[0],
            WslDistro {
                name: "Ubuntu".to_string(),
                state: "Running".to_string(),
                version: "2".to_string(),
                is_default: true,
            }
        );
        assert_eq!(rows[1].name, "Debian");
        assert!(!rows[1].is_default);
        assert_eq!(rows[2].name, "My Dev Box", "含空格的发行版名必须整体还原");
        assert_eq!(rows[2].version, "1");
    }

    #[test]
    fn distro_argv_uses_exec_form_and_positional_args() {
        let argv = distro_argv("Ubuntu", "SCRIPT", &["$HOME", "3081"]);
        assert_eq!(
            argv,
            vec!["-d", "Ubuntu", "-e", "bash", "-lc", "SCRIPT", "bash", "$HOME", "3081"]
        );
        assert!(!argv.contains(&"--"), "禁止 -- 形态（F8 注入面）");
        let e_pos = argv.iter().position(|a| *a == "-e").unwrap();
        let d_pos = argv.iter().position(|a| *a == "-d").unwrap();
        assert_eq!(e_pos, d_pos + 2, "-e 必须紧跟 -d <distro>");
    }

    /// R-W4-2：「`wsl.exe` 不可用」的可重复验收——经**绝对路径**拉起，路径不存在
    /// 时立即失败，错误文本含 `WSL_EXEC_FAILED`、被调用的绝对路径与 `os error 2`
    /// （目录存在、文件缺失时 Windows 才报 2；目录不存在会报 3，故用临时目录）。
    /// （不再用「往应用目录放伪造 wsl.exe」的方式测试：那测的是错误的解析路径。）
    #[cfg(windows)]
    #[test]
    fn spawn_wsl_at_reports_missing_executable() {
        let exe = std::env::temp_dir().join("dsh-w4r-missing-wsl.exe");
        let _ = std::fs::remove_file(&exe);
        let err = spawn_wsl_at(&exe, &["--list"]).expect_err("缺失的可执行文件必须失败");
        assert!(err.starts_with("WSL_EXEC_FAILED"), "{err}");
        assert!(err.contains(&exe.display().to_string()), "{err}");
        assert!(err.contains("os error 2"), "{err}");
    }

    /// `wsl_exe_path()` 必须是绝对路径且指向 `System32\wsl.exe`（W1 审核 R-1 的
    /// 哨兵语义在 R-W4-2 搬到本模块后仍成立）。
    #[test]
    fn wsl_exe_path_is_absolute_and_points_at_system32() {
        if !cfg!(windows) {
            return;
        }
        let path = wsl_exe_path();
        assert!(
            path.is_absolute(),
            "wsl 哨兵必须是绝对路径: {}",
            path.display()
        );
        let text = path.to_string_lossy().into_owned();
        assert!(text.ends_with(r"System32\wsl.exe"), "{text}");
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        assert!(
            text.starts_with(&root.to_string_lossy().into_owned()),
            "wsl 路径必须以 SystemRoot 为前缀: {text}"
        );
    }
}
