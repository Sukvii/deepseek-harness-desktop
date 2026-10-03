//! WSL 受控运行时的 `--skip-auth` 纯补丁函数（U2：自桌面 `service/patch/alpha_auth`
//! 迁入并局部化）。
//!
//! 桌面核心自 v0.21.0 起改用 `DSH_TAURI_EMBEDDED=1` + 内置 `dsh-tauri` gate 的嵌入
//! 鉴权，旧的 `--skip-auth` 调度入口已随上游删除；WSL 主路线继续使用受控旧核心
//! （0.1.2-rc.1）与启动参数 `--skip-auth`，因此只把两个纯函数与锚点常量保留在
//! WSL 模块内（UNC 路径的读写编排见 [`super::patch`]）。
//!
//! - `dsh-web-app/lib/startup.js`：给 web 命令补上 `--skip-auth` 选项；命中时设置
//!   `DSH_SKIP_AUTH=1`（进程级开关，WSL 的 `START` 在启动参数里显式传该标志）。
//! - `dsh-client-connection/lib/index.js`：仅当 `DSH_SKIP_AUTH=1` 时跳过
//!   browser-session 层（`authorizeIndex` 直接放行 index、`requestRejection` 不再
//!   校验 cookie），Host/Origin 信任边界仍由 `isTrustedApiRequest` 保留。
//!
//! 锚点兼容两种核心布局（issue #358）：pkg 打包核心在 `opts()` 之后有
//! `DSH_PKG_ALLOW_LAN` 护栏行，npm 原版没有；action 锚点只匹配
//! `const options = program.opts();` 单行，注入的 `--skip-auth` 处理把后续行原样
//! 保留，两种布局都能命中。

use crate::utils::PatchOutcome;

const PATCH_MARKER: &str = "dsh-tauri-desktop: alpha embedded auth --skip-auth flag";

// ── dsh-web-app/lib/startup.js ────────────────────────────────────────────────
const STARTUP_OPTION_ANCHOR: &str =
    ".option(\"--no-open\", \"do not open the Web UI in the default browser\")";
const STARTUP_OPTION_REPLACEMENT: &str = ".option(\"--no-open\", \"do not open the Web UI in the default browser\")\n\t\t.option(\"--skip-auth\", \"skip the browser-session token/cookie exchange; keeps the Host/Origin trust fence (for embedded UIs)\")";
const STARTUP_ACTION_ANCHOR: &str = "\t\tconst options = program.opts();";
const STARTUP_ACTION_REPLACEMENT: &str = "\t\tconst options = program.opts();\n\t\t/* dsh-tauri-desktop: alpha embedded auth --skip-auth flag */\n\t\tif (options.skipAuth) process.env.DSH_SKIP_AUTH = \"1\";";

// ── dsh-client-connection/lib/index.js ────────────────────────────────────────
const REJECTION_ANCHOR: &str = "\trequestRejection(request) {\n\t\tif (!isTrustedApiRequest(request, this.trustedHosts)) return 403;\n\t\treturn this.browserAuth.isAuthenticated(request) ? void 0 : 401;\n\t}";
const REJECTION_PATCHED: &str = "\trequestRejection(request) {\n\t\tif (!isTrustedApiRequest(request, this.trustedHosts)) return 403;\n\t\tif (process.env.DSH_SKIP_AUTH === \"1\") return void 0;\n\t\treturn this.browserAuth.isAuthenticated(request) ? void 0 : 401;\n\t} /* dsh-tauri-desktop: alpha embedded auth --skip-auth flag */";
const AUTHORIZE_ANCHOR: &str = "\tauthorizeIndex(request, response) {\n\t\treturn this.browserAuth.authorizeIndex(request, response);\n\t}";
const AUTHORIZE_PATCHED: &str = "\tauthorizeIndex(request, response) {\n\t\tif (process.env.DSH_SKIP_AUTH === \"1\") return true;\n\t\treturn this.browserAuth.authorizeIndex(request, response);\n\t} /* dsh-tauri-desktop: alpha embedded auth --skip-auth flag */";

/// 替换 web 启动命令：接受 `--skip-auth`；命中时写入 `DSH_SKIP_AUTH=1` 作为与
/// connection 层之间的进程级开关。
///
/// `pub(super)`：只由 [`super::patch`] 的 UNC 补丁编排调用。
pub(super) fn patch_startup(source: &str) -> PatchOutcome {
    if source.contains(PATCH_MARKER) {
        return PatchOutcome::AlreadyPatched;
    }
    if !source.contains(STARTUP_OPTION_ANCHOR) || !source.contains(STARTUP_ACTION_ANCHOR) {
        return PatchOutcome::AnchorMissing;
    }

    let patched = source
        .replacen(STARTUP_OPTION_ANCHOR, STARTUP_OPTION_REPLACEMENT, 1)
        .replacen(STARTUP_ACTION_ANCHOR, STARTUP_ACTION_REPLACEMENT, 1);
    PatchOutcome::Patched(patched)
}

/// 替换 alpha 的 connection 鉴权：仅在 `DSH_SKIP_AUTH=1` 时跳过 browser-session，
/// 始终保留 Host/Origin trust fence；未设置时行为与上游一致。
///
/// `pub(super)`：只由 [`super::patch`] 的 UNC 补丁编排调用。
pub(super) fn patch_connection(source: &str) -> PatchOutcome {
    if source.contains(PATCH_MARKER) {
        return PatchOutcome::AlreadyPatched;
    }
    if !source.contains(REJECTION_ANCHOR) || !source.contains(AUTHORIZE_ANCHOR) {
        return PatchOutcome::AnchorMissing;
    }

    let patched = source
        .replacen(REJECTION_ANCHOR, REJECTION_PATCHED, 1)
        .replacen(AUTHORIZE_ANCHOR, AUTHORIZE_PATCHED, 1);
    PatchOutcome::Patched(patched)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection_fixture() -> String {
        let mut source = String::new();
        source.push_str("\trequestRejection(request) {\n");
        source.push_str("\t\tif (!isTrustedApiRequest(request, this.trustedHosts)) return 403;\n");
        source.push_str("\t\treturn this.browserAuth.isAuthenticated(request) ? void 0 : 401;\n");
        source.push_str("\t}\n");
        source.push_str("\t/** Authenticate an index request through the process-token exchange or cookie. */\n");
        source.push_str(AUTHORIZE_ANCHOR);
        source
            .push_str("\n\tdsh web authentication required; reopen the URL printed by dsh web.\n");
        source
    }

    fn startup_fixture() -> String {
        let mut source = String::new();
        source.push_str("function webCommand() {\n");
        source.push_str(
            "\treturn new Command().name(\"dsh --profile web\").description(\"Serve the DeepSeek Harness browser UI.\").helpOption(\"-h, --help\", \"show this help\").option(\"--host <host>\", \"bind host\").option(\"--no-open\", \"do not open the Web UI in the default browser\").option(\"--port <port>\", \"listen port; pass 0 to let the OS pick a free one\").option(\"--trusted-host <authority...>\", \"extra authority the /api browser-trust fence accepts (host or host:port; repeatable)\").addHelpText(\"after\", `\nExamples:\n`);\n",
        );
        source.push_str("}\n\n");
        source.push_str("function apply(ctx) {\n");
        source.push_str("\tconst program = webCommand();\n");
        source.push_str("\tprogram.action(() => {\n");
        source.push_str(STARTUP_ACTION_ANCHOR);
        source.push_str("\n\t});\n");
        source.push_str("}\n");
        source
    }

    /// pkg 打包核心（deepseek-harness-pkg）布局：`opts()` 之后紧跟 pkg 注入的
    /// allowLan 护栏行（issue #358 中 dev 核心正是此布局，补丁必须保留该行）。
    fn startup_fixture_pkg_layout() -> String {
        let mut source = startup_fixture();
        source = source.replace(
            STARTUP_ACTION_ANCHOR,
            "\t\tconst options = program.opts();\n\t\tconst allowLan = process.env.DSH_PKG_ALLOW_LAN === \"1\";",
        );
        source
    }

    /// 真实 npm 样本形态（D-W5R-3）：`0.1.2-rc.1`（推荐版本）与 `0.1.5-rc.2`
    /// （web-app / connection 均为 0.1.5-rc.3）的隔离干净安装实测——`startup.js`
    /// 的整条 option 链在**同一行**，action 是 `opts()` 单行后紧跟 `--host` 校验。
    ///
    /// 早前「单行链式使锚点失配」的诊断不成立（审核端 D-W5R-3 指出）：OPTION 锚点
    /// 没有行首/缩进条件，`contains` 匹配在单行链式里同样命中；两个样本实测
    /// OPTION / ACTION / REJECTION / AUTHORIZE 计数均为 1。
    fn startup_fixture_single_line_chain() -> String {
        let mut source = String::new();
        source.push_str("function webCommand() {\n");
        source.push_str("\treturn new Command().name(\"dsh --profile web\").description(\"Serve the DeepSeek Harness browser UI.\").option(\"--host <host>\", \"bind host\").option(\"--no-open\", \"do not open the Web UI in the default browser\").option(\"--port <port>\", \"listen port; pass 0 to let the OS pick a free one\").option(\"--trusted-host <authority...>\", \"extra authority the /api browser-trust fence accepts\");\n");
        source.push_str("}\n\n");
        source.push_str("function apply(ctx) {\n");
        source.push_str("\tconst program = webCommand();\n");
        source.push_str("\tprogram.action(() => {\n");
        source.push_str(STARTUP_ACTION_ANCHOR);
        source.push_str("\n\t\tif (options.host === \"0.0.0.0\") program.error(\"error: --host 0.0.0.0 is intentionally not supported yet\");\n");
        source.push_str("\t});\n");
        source.push_str("}\n");
        source
    }

    /// 真实样本形态的 connection：委托方法前还有上游自己的 `authorizeIndex(req, res)`
    /// 实现（0.1.2-rc.1 实测 L363），锚点带参数名 `request, response` 不得误伤它。
    fn connection_fixture_with_real_extra_method() -> String {
        let mut source = String::new();
        source.push_str("\tauthorizeIndex(req, res) {\n");
        source.push_str("\t\tconst url = new URL(req.url ?? \"/\", \"http://dsh.invalid\");\n");
        source.push_str("\t\tconst tokens = url.searchParams.get(\"token\");\n");
        source.push_str("\t}\n");
        source.push_str(&connection_fixture());
        source
    }

    #[test]
    fn connection_patch_keeps_trust_fence_and_gates_browser_session() {
        let PatchOutcome::Patched(patched) = patch_connection(&connection_fixture()) else {
            panic!("expected connection patch");
        };
        assert!(patched.contains(PATCH_MARKER));
        // 信任边界保留。
        assert!(
            patched.contains("if (!isTrustedApiRequest(request, this.trustedHosts)) return 403;")
        );
        // 命中开关时不返回 401。
        assert!(patched.contains("if (process.env.DSH_SKIP_AUTH === \"1\") return void 0;"));
        // 未命中时仍走原鉴权。
        assert!(patched.contains("this.browserAuth.isAuthenticated(request) ? void 0 : 401"));
        // 未命中 index 仍交给上游。
        assert!(patched
            .contains("if (process.env.DSH_SKIP_AUTH === \"1\") return true;\n\t\treturn this.browserAuth.authorizeIndex(request, response);"));
    }

    #[test]
    fn startup_patch_adds_option_and_env_handoff() {
        let PatchOutcome::Patched(patched) = patch_startup(&startup_fixture()) else {
            panic!("expected startup patch");
        };
        assert!(patched.contains(PATCH_MARKER));
        assert!(patched.contains(".option(\"--skip-auth\", \"skip the browser-session token/cookie exchange; keeps the Host/Origin trust fence (for embedded UIs)\")"));
        assert!(patched.contains("\t\tif (options.skipAuth) process.env.DSH_SKIP_AUTH = \"1\";"));
    }

    /// 回归（issue #358）：npm 全局原版核心的 startup.js 在 `opts()` 之后**没有**
    /// allowLan 行，旧锚点（要求两行紧邻）导致 AnchorMissing、`--skip-auth` 未注入，
    /// boot page 返回 401。现在锚点只匹配 `opts()` 单行，npm 布局必须可打补丁。
    #[test]
    fn startup_patch_applies_to_npm_original_layout_without_allow_lan() {
        let fixture = startup_fixture();
        // npm 原版无 allowLan 行
        assert!(!fixture.contains("DSH_PKG_ALLOW_LAN"));

        let PatchOutcome::Patched(patched) = patch_startup(&fixture) else {
            panic!("expected startup patch on npm original layout");
        };
        assert!(patched.contains(PATCH_MARKER));
        assert!(patched.contains("\t\tif (options.skipAuth) process.env.DSH_SKIP_AUTH = \"1\";"));
    }

    /// 回归（issue #358）：pkg 打包核心的 allowLan 行必须在打补丁后原样保留，
    /// 不能因锚点放宽而被吞掉。
    #[test]
    fn startup_patch_preserves_pkg_allow_lan_line() {
        let fixture = startup_fixture_pkg_layout();
        assert!(fixture.contains("DSH_PKG_ALLOW_LAN"));

        let PatchOutcome::Patched(patched) = patch_startup(&fixture) else {
            panic!("expected startup patch on pkg layout");
        };
        assert!(patched.contains(PATCH_MARKER));
        assert!(patched.contains("\t\tconst allowLan = process.env.DSH_PKG_ALLOW_LAN === \"1\";"));
        assert!(patched.contains("\t\tif (options.skipAuth) process.env.DSH_SKIP_AUTH = \"1\";"));
    }

    #[test]
    fn patches_are_idempotent() {
        let PatchOutcome::Patched(startup) = patch_startup(&startup_fixture()) else {
            panic!("expected startup patch");
        };
        assert_eq!(patch_startup(&startup), PatchOutcome::AlreadyPatched);

        let PatchOutcome::Patched(connection) = patch_connection(&connection_fixture()) else {
            panic!("expected connection patch");
        };
        assert_eq!(patch_connection(&connection), PatchOutcome::AlreadyPatched);
    }

    /// D-W5R-3：按隔离干净安装的真实样本形态验证——未修改样本可打、已补丁样本
    /// 重复打是 `AlreadyPatched`；connection 的 `authorizeIndex(req, res)` 变体
    /// （同一文件里的另一处方法）不被误伤。
    #[test]
    fn patches_match_real_npm_sample_shapes() {
        // startup：单行链式 option + opts() action（0.1.2-rc.1 / 0.1.5-rc.3 实测）
        let sample = startup_fixture_single_line_chain();
        let PatchOutcome::Patched(patched) = patch_startup(&sample) else {
            panic!("expected startup patch on the real single-line sample shape");
        };
        assert!(patched.contains(PATCH_MARKER));
        assert!(patched.contains(".option(\"--skip-auth\""));
        assert!(patched.contains("\t\tif (options.skipAuth) process.env.DSH_SKIP_AUTH = \"1\";"));
        // `--host` 校验行原样保留
        assert!(patched.contains("if (options.host === \"0.0.0.0\")"));
        // 重复补丁幂等
        assert_eq!(patch_startup(&patched), PatchOutcome::AlreadyPatched);

        // connection：另一处 `authorizeIndex(req, res)`（真实样本 L363）不受影响
        let sample = connection_fixture_with_real_extra_method();
        let PatchOutcome::Patched(patched) = patch_connection(&sample) else {
            panic!("expected connection patch on the real delegated-method sample");
        };
        assert!(patched.contains(PATCH_MARKER));
        assert!(patched.contains("\tauthorizeIndex(req, res) {"));
        assert!(patched.contains("if (process.env.DSH_SKIP_AUTH === \"1\") return true;"));
        assert_eq!(patch_connection(&patched), PatchOutcome::AlreadyPatched);
    }

    #[test]
    fn legacy_or_changed_layout_is_untouched() {
        assert_eq!(
            patch_connection("requestRejection(request) { return 401; }"),
            PatchOutcome::AnchorMissing
        );
        assert_eq!(
            patch_startup("no web command here"),
            PatchOutcome::AnchorMissing
        );

        // `opts()` 行本身不存在（旧核心/上游布局彻底变化）→ 跳过
        let partial = startup_fixture().replace(
            STARTUP_ACTION_ANCHOR,
            "\t\tconst options = program.parse();",
        );
        assert_eq!(patch_startup(&partial), PatchOutcome::AnchorMissing);

        let partial = connection_fixture().replace(
            REJECTION_ANCHOR,
            "requestRejection(request) { return 401; }",
        );
        assert_eq!(patch_connection(&partial), PatchOutcome::AnchorMissing);
    }

    /// U2.3：任一认证锚点缺失都拒绝打补丁（不放行「半层补丁」——例如只放行 index
    /// 而 `requestRejection` 仍拦 401 的不一致状态），四个方向逐一验证。
    #[test]
    fn missing_either_auth_anchor_rejects_the_patch() {
        let only_rejection = connection_fixture().replace(AUTHORIZE_ANCHOR, "");
        assert_eq!(
            patch_connection(&only_rejection),
            PatchOutcome::AnchorMissing
        );

        let only_authorize = connection_fixture().replace(REJECTION_ANCHOR, "");
        assert_eq!(
            patch_connection(&only_authorize),
            PatchOutcome::AnchorMissing
        );

        let only_option = startup_fixture().replace(STARTUP_ACTION_ANCHOR, "");
        assert_eq!(patch_startup(&only_option), PatchOutcome::AnchorMissing);

        let only_action = startup_fixture().replace(STARTUP_OPTION_ANCHOR, "");
        assert_eq!(patch_startup(&only_action), PatchOutcome::AnchorMissing);
    }
}
