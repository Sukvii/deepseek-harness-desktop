//! WSL 核心支持（W 系列方案的 `service/wsl_core/`）。
//!
//! 桌面壳经 `wsl.exe -e`（不经登录 shell 展开，见方案硬性规则 1）在发行版内拉起
//! dsh：工作区与命令天然位于 Linux 内，Windows 侧不再改写任何命令。
//!
//! 模块划分：`exec`（wsl.exe 调用与解码）/ `script`（常量脚本）/ `probe`（探测与
//! 进程内缓存）/ `install`（安装与补丁编排）/ `auth_patch`（`--skip-auth` 纯补丁
//! 函数）/ `patch`（UNC 路径补丁）。

mod auth_patch;

pub mod credentials;
pub mod exec;
pub mod install;
pub mod patch;
pub mod probe;
pub mod runtime;
pub mod script;
pub mod switch;

/// 以位置参数调一条固定脚本（`spawn_blocking` 包装；`install` 与 `switch` 共用）。
pub(crate) async fn run_script(
    distro: &str,
    script: &'static str,
    args: Vec<String>,
    timeout: std::time::Duration,
) -> Result<exec::WslOutput, String> {
    let distro_name = distro.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        exec::run_in_distro(&distro_name, script, &refs, timeout)
    })
    .await
    .map_err(|e| format!("WSL_SCRIPT_JOIN_FAILED: {e}"))?
}

/// WSL 侧 dsh 数据目录名（相对 `$HOME`）。
///
/// release 构建为 `.dsh-desktop`、debug 构建为 `.dsh-desktop.dev`——刻意与
/// Windows 侧 `~/.dsh` / `~/.dsh.dev` 区分，也不会与用户在 WSL 内自己安装的
/// dsh（`~/.dsh`）冲突（方案硬性规则 3）。
pub fn dsh_home_dir_name() -> &'static str {
    if cfg!(debug_assertions) {
        ".dsh-desktop.dev"
    } else {
        ".dsh-desktop"
    }
}

/// 发行版名校验（bridge 命令与设置写入共用，R-W2-11 / W1-R R-3）：trim 后非空、
/// 不含控制字符；返回 trim 后的名字。
pub fn validate_distro(name: &str) -> Result<&str, String> {
    let distro = name.trim();
    if distro.is_empty() {
        return Err("WSL_DISTRO_INVALID: name is empty".to_string());
    }
    if distro.chars().any(char::is_control) {
        return Err("WSL_DISTRO_INVALID: control characters are not allowed".to_string());
    }
    Ok(distro)
}

/// 版本 spec 校验（R-W2-11）：只允许精确 semver 或 npm dist-tag
/// （`[A-Za-z][A-Za-z0-9._-]*`）。`file:` / `git+` / URL / range / 空白一律拒绝——
/// 只允许这两种 spec 也保证了 `RESOLVE_DSH` 的输出恒为单行。
pub fn validate_version_spec(spec: &str) -> Result<&str, String> {
    let spec = spec.trim();
    if semver::Version::parse(spec).is_ok() {
        return Ok(spec);
    }
    let mut chars = spec.chars();
    let head_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
    let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if head_ok && rest_ok {
        Ok(spec)
    } else {
        Err(format!("WSL_VERSION_SPEC_INVALID: {spec}"))
    }
}

#[cfg(test)]
mod tests {
    use super::{dsh_home_dir_name, validate_distro, validate_version_spec};

    #[test]
    fn validate_distro_trims_and_rejects_control_characters() {
        assert_eq!(validate_distro("  Ubuntu  ").unwrap(), "Ubuntu");
        let err = validate_distro("   ").unwrap_err();
        assert!(err.starts_with("WSL_DISTRO_INVALID"), "{err}");
        let err = validate_distro("a\nb").unwrap_err();
        assert!(err.starts_with("WSL_DISTRO_INVALID"), "{err}");
    }

    #[test]
    fn validate_version_spec_allows_semver_and_dist_tags_only() {
        assert_eq!(validate_version_spec(" 0.1.5-rc.2 ").unwrap(), "0.1.5-rc.2");
        assert_eq!(validate_version_spec("latest").unwrap(), "latest");
        assert_eq!(validate_version_spec("next").unwrap(), "next");
        for bad in [
            "",
            "file:../x",
            "git+https://x/y",
            "https://x/y.tgz",
            "^0.1.5",
            "0.1.x",
            "a b",
            "a/b",
            "1.2.3;echo",
        ] {
            let err = validate_version_spec(bad).unwrap_err();
            assert!(err.starts_with("WSL_VERSION_SPEC_INVALID"), "{bad}: {err}");
        }
    }

    #[test]
    fn data_dir_name_is_debug_isolated() {
        let name = dsh_home_dir_name();
        if cfg!(debug_assertions) {
            assert_eq!(
                name, ".dsh-desktop.dev",
                "debug 必须与 release 数据目录隔离"
            );
        } else {
            assert_eq!(name, ".dsh-desktop");
        }
    }
}
