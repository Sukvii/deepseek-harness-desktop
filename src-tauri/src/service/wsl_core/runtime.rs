//! 受控 WSL 运行时（方案 v8「W5-R v8 兼容依赖裁决」）。
//!
//! WSL 侧不再管理用户的全局 npm 前缀，也不复用用户自己安装的全局 dsh：安装目标是
//! 应用私有的 npm 项目 `$HOME/<dsh_home_dir_name()>/runtime`，其依赖树由随桌面包
//! 分发的 manifest + 锁文件（本模块的 [`load`]）精确固定——推荐核心的六项 Cordis
//! 组件经 `overrides` 钉死，其余传递依赖全部由同一把锁固定，`npm ci` 只做重放。
//!
//! 缺资源 / manifest 与目标版本不一致时必须**停止**（`WSL_RUNTIME_RESOURCE_MISSING`
//! / `WSL_RUNTIME_MANIFEST_MISMATCH`），绝不回退到无锁安装（那会重新落回 `^` 范围
//! 解析：cordis 4.0.4 / loader 1.0.5 / hmr 1.0.19 与 app-boot 0.1.2-rc.1 的 API
//! 不兼容，HMR 服务静默不就绪，`patchReload: "live"` 下 dsh 直接退出——W5-R 实测）。
//!
//! 「相同主包、锁基线变化仍能更新」靠运行时根目录的基线记录
//! （[`BASELINE_FILE_NAME`]，`npm ci` 不触碰该文件）与资源锁哈希比对：版本一致但
//! 锁变了（重新验证过的兼容组合调整）同样触发候选安装与切换。

use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// 受控运行时目录名（相对数据目录，如 `~/.dsh-desktop.dev/runtime`）。
pub const RUNTIME_DIR_NAME: &str = "runtime";
/// 候选运行时目录名（与正式运行时同目录，验证通过后整体切换）。
pub const CANDIDATE_DIR_NAME: &str = "runtime-candidate";
/// 切换窗口的备份目录前缀（`<数据目录>/runtime-backup-<stamp>`）。
pub const BACKUP_DIR_PREFIX: &str = "runtime-backup-";
/// 基线记录文件名（运行时根目录内；记录版本与锁哈希，供更新判定）。
pub const BASELINE_FILE_NAME: &str = ".dsh-runtime.json";

/// 资源根目录名（`src-tauri/resources/wsl-runtime/<version>/`）。
const RESOURCE_ROOT_DIR: &str = "wsl-runtime";
const MANIFEST_FILE_NAME: &str = "package.json";
const LOCK_FILE_NAME: &str = "package-lock.json";

/// 一次受控安装的输入（随桌面包分发；锁文件固定完整依赖树）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeBundle {
    /// 目标主包版本（manifest 校验通过后的值）
    pub version: String,
    /// `package.json` 原文（经 UNC 写入候选目录）
    pub manifest: String,
    /// `package-lock.json` 原文（经 UNC 写入候选目录）
    pub lock: String,
    /// 锁文件的 SHA-256（十六进制小写）；基线记录与更新判定用
    pub lock_sha256: String,
}

/// 运行时基线记录（`<runtime>/.dsh-runtime.json`）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeBaseline {
    pub version: String,
    pub lock_sha256: String,
}

impl RuntimeBaseline {
    pub fn new(version: &str, lock_sha256: &str) -> Self {
        Self {
            version: version.to_string(),
            lock_sha256: lock_sha256.to_string(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// 解析基线记录；内容损坏 / 字段缺失 → `None`（调用方按「需要更新」处理）。
    pub fn parse(content: &str) -> Option<Self> {
        let baseline: Self = serde_json::from_str(content).ok()?;
        (!baseline.version.is_empty() && !baseline.lock_sha256.is_empty()).then_some(baseline)
    }
}

/// manifest 的 `dependencies["@deepseek-ai/dsh"]`（唯一权威的「这个项目装哪个版本」）。
fn manifest_dsh_version(manifest: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(manifest).ok()?;
    let version = value
        .get("dependencies")?
        .get("@deepseek-ai/dsh")?
        .as_str()?
        .trim();
    (!version.is_empty()).then(|| version.to_string())
}

/// 锁文件的根依赖声明的 dsh 版本（用于 manifest / lock 交叉校验）。
fn lock_dsh_version(lock: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(lock).ok()?;
    let version = value
        .get("packages")?
        .get("")?
        .get("dependencies")?
        .get("@deepseek-ai/dsh")?
        .as_str()?
        .trim();
    (!version.is_empty()).then(|| version.to_string())
}

/// 锁文件内容的 SHA-256（十六进制小写）。
pub fn lock_sha256(lock: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(lock.as_bytes()))
}

/// 资源目录候选：debug 构建优先当前 checkout（与 `version_recommend` 同一口径，
/// 避免用旧构建产物里的过期锁去校验当前代码），release 优先随包资源。
fn resource_candidates(app: &AppHandle, version: &str) -> Vec<PathBuf> {
    let rel = Path::new(RESOURCE_ROOT_DIR).join(version);
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join(&rel);
    let mut paths = Vec::new();
    if cfg!(debug_assertions) {
        paths.push(source.clone());
    }
    if let Ok(root) = app.path().resource_dir() {
        paths.push(root.join(&rel));
        paths.push(root.join("resources").join(&rel));
    }
    if !cfg!(debug_assertions) {
        paths.push(source);
    }
    paths
}

/// 读资源文件（失败归一到 `WSL_RUNTIME_RESOURCE_INVALID`）。
fn read_resource(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map_err(|e| format!("WSL_RUNTIME_RESOURCE_INVALID: {}: {e}", path.display()))
}

/// 加载指定版本的受控安装资源并交叉校验。
///
/// 失败即停止：`WSL_RUNTIME_RESOURCE_MISSING`（没有该版本的资源目录）、
/// `WSL_RUNTIME_RESOURCE_INVALID`（文件不可读）、`WSL_RUNTIME_MANIFEST_MISMATCH`
/// （manifest / lock 钉的版本与请求不一致）。**没有任何回退路径**。
pub fn load(app: &AppHandle, version: &str) -> Result<RuntimeBundle, String> {
    let Some(dir) = resource_candidates(app, version)
        .into_iter()
        .find(|path| path.is_dir())
    else {
        return Err(format!(
            "WSL_RUNTIME_RESOURCE_MISSING: no bundled runtime resources for dsh {version}"
        ));
    };
    let manifest = read_resource(&dir.join(MANIFEST_FILE_NAME))?;
    let lock = read_resource(&dir.join(LOCK_FILE_NAME))?;
    match manifest_dsh_version(&manifest) {
        Some(pinned) if pinned == version => {}
        Some(pinned) => {
            return Err(format!(
                "WSL_RUNTIME_MANIFEST_MISMATCH: manifest pins {pinned}, requested {version}"
            ));
        }
        None => {
            return Err("WSL_RUNTIME_MANIFEST_MISMATCH: manifest has no \
                 dependencies[\"@deepseek-ai/dsh\"]"
                .to_string());
        }
    }
    match lock_dsh_version(&lock) {
        Some(pinned) if pinned == version => {}
        Some(pinned) => {
            return Err(format!(
                "WSL_RUNTIME_MANIFEST_MISMATCH: lockfile pins {pinned}, requested {version}"
            ));
        }
        None => {
            return Err("WSL_RUNTIME_MANIFEST_MISMATCH: lockfile has no root \
                 dependencies[\"@deepseek-ai/dsh\"]"
                .to_string());
        }
    }
    Ok(RuntimeBundle {
        version: version.to_string(),
        lock_sha256: lock_sha256(&lock),
        manifest,
        lock,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        lock_dsh_version, lock_sha256, manifest_dsh_version, RuntimeBaseline, BACKUP_DIR_PREFIX,
        BASELINE_FILE_NAME, CANDIDATE_DIR_NAME, LOCK_FILE_NAME, MANIFEST_FILE_NAME,
        RESOURCE_ROOT_DIR, RUNTIME_DIR_NAME,
    };

    #[test]
    fn manifest_version_reads_pinned_dependency_only() {
        assert_eq!(
            manifest_dsh_version(r#"{"dependencies":{"@deepseek-ai/dsh":"0.1.2-rc.1"}}"#),
            Some("0.1.2-rc.1".to_string())
        );
        // overrides 不是依赖来源；只有 dependencies 算
        assert_eq!(
            manifest_dsh_version(r#"{"overrides":{"@deepseek-ai/dsh":"0.1.2-rc.1"}}"#),
            None
        );
        assert_eq!(manifest_dsh_version("not json"), None);
        assert_eq!(manifest_dsh_version(r#"{"dependencies":{}}"#), None);
    }

    #[test]
    fn lock_version_reads_root_package_dependencies() {
        let lock = r#"{"lockfileVersion":3,"packages":{"":{"dependencies":{"@deepseek-ai/dsh":"0.1.2-rc.1"}}}}"#;
        assert_eq!(lock_dsh_version(lock), Some("0.1.2-rc.1".to_string()));
        assert_eq!(lock_dsh_version(r#"{"packages":{"":{}}}"#), None);
        assert_eq!(lock_dsh_version("[]"), None);
    }

    #[test]
    fn lock_sha256_matches_known_vector() {
        // NIST 向量：sha256("abc")
        assert_eq!(
            lock_sha256("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(lock_sha256("").len(), 64);
    }

    #[test]
    fn baseline_round_trips_and_rejects_damage() {
        let baseline = RuntimeBaseline::new("0.1.2-rc.1", &"a".repeat(64));
        let parsed = RuntimeBaseline::parse(&baseline.to_json()).expect("roundtrip");
        assert_eq!(parsed, baseline);
        for bad in ["", "not json", r#"{"version":"","lockSha256":"x"}"#] {
            assert_eq!(RuntimeBaseline::parse(bad), None, "{bad:?}");
        }
    }

    /// 受控运行时的目录约定是本模块与脚本层（`script.rs`）的共享契约，防止两边
    /// 各自硬编码后漂移（脚本内的字符串与这些常量必须一致）。
    #[test]
    fn directory_names_match_script_layer() {
        use crate::service::wsl_core::script::{
            CLEAN_RUNTIME_BACKUP, PREPARE_RUNTIME_CANDIDATE, PROBE, RUNTIME_NPM_CI, START,
            SWITCH_RUNTIME, VERIFY_CORE,
        };
        assert_eq!(RUNTIME_DIR_NAME, "runtime");
        assert_eq!(CANDIDATE_DIR_NAME, "runtime-candidate");
        assert_eq!(BACKUP_DIR_PREFIX, "runtime-backup-");
        assert_eq!(BASELINE_FILE_NAME, ".dsh-runtime.json");
        for script in [
            START,
            PROBE,
            PREPARE_RUNTIME_CANDIDATE,
            RUNTIME_NPM_CI,
            SWITCH_RUNTIME,
        ] {
            assert!(
                script.contains(CANDIDATE_DIR_NAME) || script.contains(RUNTIME_DIR_NAME),
                "脚本必须使用受控运行时目录约定"
            );
        }
        assert!(CLEAN_RUNTIME_BACKUP.contains(BACKUP_DIR_PREFIX));
        // 正式运行时的二进制必须由脚本按目录约定拼出
        assert!(START.contains(&format!("{RUNTIME_DIR_NAME}/node_modules/.bin/dsh")));
        assert!(VERIFY_CORE.contains("node_modules/.bin/dsh"));
    }

    /// U1：随包 WSL 运行时资源必须与清单推荐值配对——默认目标 0.1.2-rc.1 的
    /// manifest / lock 在盘且都钉住该版本；锁哈希是基线记录（`.dsh-runtime.json`）
    /// 与更新判定的输入端，重新生成锁（即使只差换行）必须显式更新此断言。
    #[test]
    fn shipped_wsl_runtime_resources_pin_recommended_version() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join(RESOURCE_ROOT_DIR)
            .join("0.1.2-rc.1");
        let manifest = std::fs::read_to_string(dir.join(MANIFEST_FILE_NAME))
            .expect("runtime manifest in tree");
        let lock = std::fs::read_to_string(dir.join(LOCK_FILE_NAME)).expect("runtime lock in tree");
        assert_eq!(
            manifest_dsh_version(&manifest).as_deref(),
            Some("0.1.2-rc.1")
        );
        assert_eq!(lock_dsh_version(&lock).as_deref(), Some("0.1.2-rc.1"));
        assert_eq!(
            lock_sha256(&lock),
            "46d0671df390de891168284922d4ee3dd62a6e1ba4ca469dd957f37b045fbe54"
        );
    }
}
