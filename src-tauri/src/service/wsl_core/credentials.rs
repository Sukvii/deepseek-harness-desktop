//! 从 Windows 侧向 WSL 侧导入 DeepSeek API Key（W5 决策项 D-W5-1；R-W5-1 重写）。
//!
//! 背景：WSL 核心的数据目录（`~/.dsh-desktop[.dev]`）与 Windows 侧（`~/.dsh`）
//! 完全独立（D-W3-5），首次切到 WSL 核心会遇到 `MISSING_CREDENTIAL`。这里做的是
//! **用户显式点击**才执行的单向搬运：只写顶层 `refs.DEEPSEEK_API_KEY` 一项，凭据
//! 文件里其余内容（`records`、注释、行序）原样保留，也**不自动同步**。
//!
//! 目标文件由运行中的 dsh 监听（watcher 在变更后重载，F18），因此写入必须遵守它
//! 自己的协议：`.credentials.yaml.lock` 独占锁 + 同目录 0600 临时文件 rename 提交。
//! 空目标会生成合法 v1 文档（`version: 1`，F17）——只写 `refs:` 的文件会让
//! credentials-local 拒绝加载、整个插件树启动失败（R-W5-1 记录的原始缺陷）。
//!
//! 结构校验用 `serde_yaml`（**只读**：文本由行级变换保留注释与行序）；
//! [`extract_api_key`] / [`merge_api_key`] 是纯函数，单测覆盖。密钥不进 `wsl.exe`
//! 的命令行与脚本、不进错误文本、不进日志。

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::AppHandle;

use super::patch::wsl_unc_path;
use super::{dsh_home_dir_name, exec, probe, script};
use crate::config;

/// 导入的凭据键名（两侧同名）。
pub const API_KEY_NAME: &str = "DEEPSEEK_API_KEY";

/// 凭据文件名与 dsh 的独占写锁名（F18）。
const CREDENTIALS_FILE: &str = ".credentials.yaml";
const LOCK_FILE: &str = ".credentials.yaml.lock";

/// 单段 Linux 脚本的中继超时（发行版 Stopped 时首次 `wsl.exe -e` 要冷启动 VM）。
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(15);

/// 取凭据 YAML 中**顶层 `refs`** 下 `API_KEY_NAME` 的值（R-W5-1）。
///
/// 只认 `refs` 节：`records` 等其它位置的同名键不是凭据（此前按行扫描会把它们
/// 误命中）。值必须是字符串、去两端空白后非空且不含控制字符，否则视为未设置。
pub fn extract_api_key(content: &str) -> Option<String> {
    let value: serde_yaml::Value = serde_yaml::from_str(content).ok()?;
    let refs = value.as_mapping()?.get("refs")?.as_mapping()?;
    let key = refs.get(API_KEY_NAME)?.as_str()?.trim();
    if key.is_empty() || key.chars().any(char::is_control) {
        return None;
    }
    Some(key.to_string())
}

/// 校验目标文档结构（R-W5-1）：顶层 mapping、`version: 1`、`refs`（若存在）是
/// mapping。格式不支持时返回 `WSL_CREDENTIAL_FORMAT_INVALID`，不猜测转换。
///
/// 错误文本只报类型与位置：`serde_yaml` 的解析错误可能回显原文片段，而原文里
/// 可能含密钥（R-W5-1 明确禁止）。
fn validate_v1(content: &str) -> Result<serde_yaml::Value, String> {
    let value: serde_yaml::Value =
        serde_yaml::from_str(content).map_err(|e| match e.location() {
            Some(loc) => format!(
                "WSL_CREDENTIAL_FORMAT_INVALID: parse error at line {} column {}",
                loc.line(),
                loc.column()
            ),
            None => "WSL_CREDENTIAL_FORMAT_INVALID: parse error".to_string(),
        })?;
    let Some(map) = value.as_mapping() else {
        return Err("WSL_CREDENTIAL_FORMAT_INVALID: top level is not a mapping".to_string());
    };
    if map.get("version").and_then(serde_yaml::Value::as_u64) != Some(1) {
        return Err(
            "WSL_CREDENTIAL_FORMAT_INVALID: missing or unsupported version (need 1)".to_string(),
        );
    }
    if let Some(refs) = map.get("refs") {
        // 空 `refs:` 块解析为 null（合并会往里插条目）；其它非 mapping 形态不支持。
        if !refs.is_mapping() && !refs.is_null() {
            return Err("WSL_CREDENTIAL_FORMAT_INVALID: refs is not a mapping".to_string());
        }
    }
    Ok(value)
}

/// 用 `serde_yaml` 生成安全的标量文本（含特殊字符时自动加引号）。
fn yaml_scalar(value: &str) -> Result<String, String> {
    let text = serde_yaml::to_string(&serde_yaml::Value::String(value.to_string()))
        .map_err(|_| "WSL_CREDENTIAL_FORMAT_INVALID: cannot encode key value".to_string())?;
    Ok(text.trim_end().to_string())
}

/// 把 `key` 合并进凭据 YAML（R-W5-1）：
/// - 空目标 → 生成合法 v1 文档：`version: 1` + `refs:` + 键值（F17）；
/// - 非空目标先经 [`validate_v1`] 校验，只改顶层 `refs` 块内同名键的值；块内
///   没有该键则插到块首（沿用块内既有缩进），连 `refs:` 都没有时追加一个。
///   其余行（含注释与 `records`）逐字保留。
///
/// 不合法/不支持的目标返回 [`validate_v1`] 的错误，不清空、不迁移。
pub fn merge_api_key(existing: &str, key: &str) -> Result<String, String> {
    let entry_value = yaml_scalar(key)?;
    if existing.trim().is_empty() {
        return Ok(format!(
            "version: 1\nrefs:\n  {API_KEY_NAME}: {entry_value}\n"
        ));
    }
    validate_v1(existing)?;

    let lines: Vec<&str> = existing.lines().collect();
    // 顶层 `refs:` 行：无前导空白且去除尾随空白后恰为 `refs:`。
    let refs_head = lines
        .iter()
        .position(|raw| raw.len() == raw.trim_start().len() && raw.trim() == "refs:");
    let Some(refs_head) = refs_head else {
        let mut text = existing.to_string();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!("refs:\n  {API_KEY_NAME}: {entry_value}\n"));
        return Ok(text);
    };

    // `refs:` 块 = 其后直到下一个「非空、非注释、无缩进」的行。
    let block_start = refs_head + 1;
    let block_end = (block_start..lines.len())
        .find(|&i| {
            let raw = lines[i];
            let trimmed = raw.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#') && raw.len() == raw.trim_start().len()
        })
        .unwrap_or(lines.len());

    let mut out: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    let existing_entry = (block_start..block_end).find(|&i| {
        lines[i]
            .trim_start()
            .starts_with(&format!("{API_KEY_NAME}:"))
    });
    match existing_entry {
        Some(i) => {
            let indent: String = lines[i].chars().take_while(|c| c.is_whitespace()).collect();
            out[i] = format!("{indent}{API_KEY_NAME}: {entry_value}");
        }
        None => {
            // 与块内首个条目一致的缩进；块内无条目时用 2 空格。
            let indent = (block_start..block_end)
                .map(|i| lines[i])
                .find(|raw| !raw.trim().is_empty() && !raw.trim_start().starts_with('#'))
                .map(|raw| {
                    raw.chars()
                        .take_while(|c| c.is_whitespace())
                        .collect::<String>()
                })
                .unwrap_or_else(|| "  ".to_string());
            out.insert(
                block_start,
                format!("{indent}{API_KEY_NAME}: {entry_value}"),
            );
        }
    }

    let mut text = out.join("\n");
    text.push('\n');
    Ok(text)
}

/// 本次导入自己取得的锁（只有 `create_new` 成功后才构造，因此 Drop 删除的必然
/// 是自己的锁）：先关闭文件句柄再删除，失败仅告警（R-W5-1）。
struct LockGuard {
    path: PathBuf,
    file: Option<std::fs::File>,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        self.file = None;
        if let Err(e) = std::fs::remove_file(&self.path) {
            log::warn!("[wsl-core] failed to remove credentials lock: {e}");
        }
    }
}

/// 经 UNC 以 `create_new` 取锁；既有锁（别人/上次残留）→ `WSL_CREDENTIAL_BUSY`，
/// 不删除、不重试（R-W5-1）。
fn acquire_lock(path: &Path) -> Result<LockGuard, String> {
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(file) => Ok(LockGuard {
            path: path.to_path_buf(),
            file: Some(file),
        }),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Err(format!(
            "WSL_CREDENTIAL_BUSY: another writer holds {}",
            path.display()
        )),
        Err(e) => Err(format!(
            "WSL_CREDENTIAL_WRITE_FAILED: cannot create lock {}: {e}",
            path.display()
        )),
    }
}

/// 跑一段固定脚本并要求退出码为 0；失败映射为 `WSL_CREDENTIAL_WRITE_FAILED`
/// （脚本本身不读密钥，输出摘要因此可安全入错误文本，R-W5-1）。
fn run_checked(distro: &str, script: &str, args: &[&str], what: &str) -> Result<String, String> {
    let output = exec::run_in_distro(distro, script, args, SCRIPT_TIMEOUT)
        .map_err(|e| format!("WSL_CREDENTIAL_WRITE_FAILED: {what} failed: {e}"))?;
    if output.code != 0 {
        return Err(format!(
            "WSL_CREDENTIAL_WRITE_FAILED: {what} failed (exit {}): {}",
            output.code,
            exec::summarize(&output)
        ));
    }
    Ok(exec::decode_auto(&output.stdout))
}

/// 经 Linux 侧 `umask 077; mktemp` 在目标同目录建 0600 临时文件，返回文件名。
fn make_temp_file(distro: &str, dir_name: &str) -> Result<String, String> {
    let out = run_checked(
        distro,
        script::MAKE_CREDENTIALS_TMP,
        &[dir_name],
        "create temp file",
    )?;
    let name = out.trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        return Err(
            "WSL_CREDENTIAL_WRITE_FAILED: unexpected temp file name from distro".to_string(),
        );
    }
    Ok(name.to_string())
}

/// 执行导入：`Ok(true)` = 写入了新值；`Ok(false)` = 目标已是同一个 key（幂等）。
///
/// 目标路径经 UNC（`\\wsl.localhost\<distro>\<home>\<dsh home>\.credentials.yaml`）
/// 读写——9p 挂载在发行版运行时可直接访问（与 [`super::patch`] 同一条路径），
/// 不依赖发行版内的 node / python。
pub fn import_api_key(app: &AppHandle, distro: &str) -> Result<bool, String> {
    let source = config::get_dsh_data_path(app).join(CREDENTIALS_FILE);
    let text = std::fs::read_to_string(&source).map_err(|e| {
        format!(
            "WSL_CREDENTIAL_SOURCE_MISSING: cannot read {}: {e}",
            source.display()
        )
    })?;
    let key = extract_api_key(&text).ok_or_else(|| {
        format!(
            "WSL_CREDENTIAL_KEY_MISSING: {API_KEY_NAME} is not set in {}",
            source.display()
        )
    })?;

    let home = probe::cached_or_probe(distro)?.home;
    let dir_name = dsh_home_dir_name();
    let dir_unc = PathBuf::from(wsl_unc_path(distro, &format!("{home}/{dir_name}")));

    // 目录必须在 Linux 侧创建：9p 新建目录是 0755（F18），而凭据目录按 dsh 惯例
    // 用 0700；`umask 077` 只影响本次新建，已存在目录的权限不变。
    run_checked(
        distro,
        script::ENSURE_CREDENTIALS_DIR,
        &[dir_name],
        "create data dir",
    )?;

    // 拿到 dsh 自己的锁之后才允许读改写；锁在函数返回时由 guard 释放。
    let _lock = acquire_lock(&dir_unc.join(LOCK_FILE))?;

    let target = dir_unc.join(CREDENTIALS_FILE);
    let existing = match std::fs::read_to_string(&target) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err(format!(
                "WSL_CREDENTIAL_WRITE_FAILED: cannot read {}: {e}",
                target.display()
            ))
        }
    };

    if !existing.trim().is_empty() {
        // 格式与权限检查不能被「同值」分支绕过（R-W5-1）：先校验结构。
        validate_v1(&existing)?;
        if extract_api_key(&existing).as_deref() == Some(key.as_str()) {
            run_checked(
                distro,
                script::CHMOD_CREDENTIALS,
                &[dir_name],
                "confirm credential mode",
            )?;
            log::info!("[wsl-core] {API_KEY_NAME} already up to date in {distro}");
            return Ok(false);
        }
    }
    let merged = merge_api_key(&existing, &key)?;

    // 同目录 0600 临时文件 + rename 提交（dsh 的原子写协议，F18）；密钥只经
    // 文件内容传递，不进 argv / 脚本 / 日志。
    let tmp_name = make_temp_file(distro, dir_name)?;
    let tmp_path = dir_unc.join(&tmp_name);
    if let Err(e) = std::fs::write(&tmp_path, &merged) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(format!(
            "WSL_CREDENTIAL_WRITE_FAILED: write temp file failed: {e}"
        ));
    }
    if let Err(e) = run_checked(
        distro,
        script::COMMIT_CREDENTIALS,
        &[dir_name, &tmp_name],
        "commit credentials",
    ) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }

    // 只记事实，不记密钥内容。
    log::info!("[wsl-core] imported {API_KEY_NAME} into {distro} credentials");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{extract_api_key, merge_api_key, validate_v1, API_KEY_NAME};

    #[test]
    fn extract_reads_value_from_top_level_refs_only() {
        let text = "version: 1\nrefs:\n  DEEPSEEK_API_KEY: sk-abc\n";
        assert_eq!(extract_api_key(text).as_deref(), Some("sk-abc"));
        // 缩进与引号由 YAML 解析处理
        assert_eq!(
            extract_api_key("version: 1\nrefs:\n    DEEPSEEK_API_KEY:   \"sk-q\"  \n").as_deref(),
            Some("sk-q")
        );
        assert_eq!(
            extract_api_key("version: 1\nrefs:\n  DEEPSEEK_API_KEY: 'sk-s'\n").as_deref(),
            Some("sk-s")
        );
        // 同名前缀的其它键不算命中
        assert_eq!(
            extract_api_key("version: 1\nrefs:\n  DEEPSEEK_API_KEY_OLD: x\n"),
            None
        );
        // 空值、缺失、空文档都返回 None
        assert_eq!(
            extract_api_key("version: 1\nrefs:\n  DEEPSEEK_API_KEY:\n"),
            None
        );
        assert_eq!(extract_api_key("version: 1\n"), None);
        assert_eq!(extract_api_key(""), None);
        // 只认顶层 refs：records 等其它位置的同名键不是凭据
        assert_eq!(
            extract_api_key("version: 1\nrecords:\n  DEEPSEEK_API_KEY: nope\n"),
            None
        );
        assert_eq!(
            extract_api_key(
                "version: 1\nrefs:\n  DEEPSEEK_API_KEY: ok\nrecords:\n  DEEPSEEK_API_KEY: nope\n"
            )
            .as_deref(),
            Some("ok")
        );
    }

    #[test]
    fn merge_replaces_key_in_refs_and_keeps_comments_and_records() {
        let existing = "version: 1\n# keep this comment\nrefs:\n  DEEPSEEK_API_KEY: old\n  OTHER: keep\nrecords:\n  client-connection: abc\n";
        let merged = merge_api_key(existing, "new").unwrap();
        assert_eq!(
            merged,
            "version: 1\n# keep this comment\nrefs:\n  DEEPSEEK_API_KEY: new\n  OTHER: keep\nrecords:\n  client-connection: abc\n"
        );
        assert_eq!(extract_api_key(&merged).as_deref(), Some("new"));
        // 幂等：同值再合并内容不变
        assert_eq!(merge_api_key(&merged, "new").unwrap(), merged);
        // records 下的同名键不被替换
        assert!(merged.contains("records:\n  client-connection: abc\n"));
    }

    #[test]
    fn merge_inserts_into_refs_block_with_block_indent() {
        let existing = "version: 1\nrefs:\nrecords:\n  x: 1\n";
        assert_eq!(
            merge_api_key(existing, "k").unwrap(),
            format!("version: 1\nrefs:\n  {API_KEY_NAME}: k\nrecords:\n  x: 1\n")
        );
        // 块内已有条目时沿用其缩进
        let existing = "version: 1\nrefs:\n    OTHER: y\n";
        assert_eq!(
            merge_api_key(existing, "k").unwrap(),
            format!("version: 1\nrefs:\n    {API_KEY_NAME}: k\n    OTHER: y\n")
        );
    }

    #[test]
    fn merge_generates_valid_v1_document_for_empty_target() {
        let generated = merge_api_key("", "k").unwrap();
        assert_eq!(
            generated,
            format!("version: 1\nrefs:\n  {API_KEY_NAME}: k\n")
        );
        assert!(
            validate_v1(&generated).is_ok(),
            "空目标产物必须是合法 v1 文档"
        );
        // 仅空白的既有内容按空目标处理
        assert_eq!(merge_api_key("  \n\n", "k").unwrap(), generated);
        // 只有 version 时补 refs 块
        assert_eq!(
            merge_api_key("version: 1\n", "k").unwrap(),
            format!("version: 1\nrefs:\n  {API_KEY_NAME}: k\n")
        );
    }

    #[test]
    fn merge_rejects_unsupported_targets_without_echoing_content() {
        let secret = "sk-super-secret-value";
        for bad in [
            format!("version: 2\nrefs:\n  {API_KEY_NAME}: {secret}\n"),
            format!("{API_KEY_NAME}: {secret}\n"), // 缺 version: 1
            format!("version: 1\nrefs: {secret}\n"), // refs 不是 mapping
            format!("- {secret}\n"),               // 顶层不是 mapping
            "version: 1\nrefs:\n  bad: [\n".to_string(), // YAML 语法错误
        ] {
            let err = merge_api_key(&bad, "k").unwrap_err();
            assert!(err.starts_with("WSL_CREDENTIAL_FORMAT_INVALID"), "{err}");
            assert!(!err.contains(secret), "错误文本不得回显内容: {err}");
        }
        // 值含需要加引号的字符时由 YAML 标量编码保证结构合法
        let merged = merge_api_key("version: 1\n", "a: b # c").unwrap();
        assert_eq!(extract_api_key(&merged).as_deref(), Some("a: b # c"));
    }
}
