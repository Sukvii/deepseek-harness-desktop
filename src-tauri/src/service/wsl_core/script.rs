//! WSL 内执行的常量脚本（方案 §0.3 规则 1）。
//!
//! 全部为 `const &str`：动态值（数据目录名、端口、超时秒数、stamp）只经位置参数
//! `$1 $2 …` 传入，绝不拼进脚本文本。调用方见 [`super::exec::run_in_distro`]。
//!
//! v8 起安装目标为**受控运行时**（`$HOME/<数据目录名>/runtime`，见
//! [`super::runtime`]）：探测、启动、验证、安装、切换、回滚全部围绕它，不再读写
//! 用户的全局 npm 前缀与全局 `dsh` 命令。

/// 所有脚本的公共前导：进入 `$HOME`（R-W2-4：脚本不得在 Windows cwd 映射出的
/// `/mnt/...` 目录下运行——npm 会读取那里的项目级 `.npmrc`，D-W2-1 事故的根因）；
/// 系统 node 优先，没有时加载 nvm（非交互 `bash -lc` 不执行 `~/.bashrc` 的 nvm
/// 段，R-W2-3）。
///
/// 刻意**不**把 `$HOME/.npm-global/bin` 前置到 PATH（v8）：受控运行时的 dsh 一律以
/// 绝对路径显式调用，脚本里不存在任何经 PATH 解析 `dsh` 的路径——全局安装不能成为
/// 隐性后备（方案 v8 执行范围第 4/5 条）。
///
/// 以宏形态定义以便 `concat!`（`concat!` 只接受字面量）。`cd` 失败退出码 97，
/// 调用方识别为 `WSL_HOME_UNAVAILABLE`。
macro_rules! env_preamble {
    () => {
        r#"cd "$HOME" || exit 97; export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"; if ! command -v node >/dev/null 2>&1 && [ -s "$NVM_DIR/nvm.sh" ]; then . "$NVM_DIR/nvm.sh" >/dev/null 2>&1 || true; fi; "#
    };
}

/// 公共前导的独立常量（单测与文档引用；脚本内用宏展开）。
/// 仅作编译期拼接的公共前导（`concat!` 只接受字面量，无法引用该常量），
/// 非 test 构建下没有运行期读者，故显式豁免 dead_code；单测断言各脚本均以其开头。
#[allow(dead_code)]
pub const ENV_PREAMBLE: &str = env_preamble!();

/// 探测受控运行时环境，输出 `KEY=VALUE` 行：`$1` = 数据目录**名**（相对 `$HOME`）。
///
/// `ROOT` = 受控运行时的 `node_modules`（未安装时为空；补丁目标定位的根）；
/// `RUNTIME` = 受控运行时目录（前端展示用）；`DSH` = 运行时内 `.bin/dsh` 的绝对路径
/// （未安装 / 不可执行时为空）；`SKIPAUTH` 是只读的 `--skip-auth` 补丁判定（needle
/// 与 Windows 侧 `web_startup_supports_skip_auth` 相同），供 W3 决定能否启动（R-W2-2）。
///
/// 判定目标与 [`super::patch`] 按**同一候选顺序**取「第一个存在的文件」——即
/// dsh 运行时实际会加载的那一份（Node 解析优先序：web-app 嵌套 > dsh 直属嵌套 >
/// 扁平）。受控树（v8 实测）里 web-app 与 connection 被提升到顶层，命中扁平候选。
/// R-W5-1 期间两处候选曾不一致：0.1.5 系把 `dsh-client-connection` 嵌在
/// `dsh-web-app/node_modules` 下，补丁（手工）打对了而探测找不到，`skip_auth_ready`
/// 与真实状态相反（D-W5R-3）。
pub const PROBE: &str = concat!(
    env_preamble!(),
    "\n",
    r#"d="$HOME/$1"; r="$d/runtime"; root="$r/node_modules"
printf 'NODE=%s\n' "$(command -v node || true)"
printf 'NODEV=%s\n' "$(node -v 2>/dev/null || true)"
printf 'NPM=%s\n' "$(command -v npm || true)"
if [ -d "$root" ]; then printf 'ROOT=%s\n' "$root"; else printf 'ROOT=\n'; fi
printf 'RUNTIME=%s\n' "$r"
b="$root/.bin/dsh"
if [ -x "$b" ]; then printf 'DSH=%s\n' "$b"; printf 'DSHV=%s\n' "$("$b" --version 2>/dev/null || true)"; else printf 'DSH=\n'; printf 'DSHV=\n'; fi
printf 'HOME=%s\n' "$HOME"
sa=0; sp=""; cp2=""
if [ -d "$root" ]; then
  for p in "$root/@deepseek-ai/dsh/node_modules/@deepseek-ai/dsh-web-app/lib/startup.js" "$root/@deepseek-ai/dsh-web-app/lib/startup.js"; do
    if [ -f "$p" ]; then sp="$p"; break; fi
  done
  for p in "$root/@deepseek-ai/dsh/node_modules/@deepseek-ai/dsh-web-app/node_modules/@deepseek-ai/dsh-client-connection/lib/index.js" "$root/@deepseek-ai/dsh/node_modules/@deepseek-ai/dsh-client-connection/lib/index.js" "$root/@deepseek-ai/dsh-client-connection/lib/index.js"; do
    if [ -f "$p" ]; then cp2="$p"; break; fi
  done
fi
if [ -n "$sp" ] && [ -n "$cp2" ] && grep -qs -- '--skip-auth' "$sp" && grep -qs 'DSH_SKIP_AUTH' "$cp2"; then sa=1; fi
printf 'SKIPAUTH=%s\n' "$sa""#
);

/// 把 dist-tag 在线解析成具体版本（`$1` = spec）。输出必须恰好一行（R-W2-1）；
/// `timeout` 保证 Linux 侧有截止时间，杀 `wsl.exe` 中继不会杀 Linux 子进程（F5 / R-W2-6）。
pub const RESOLVE_DSH: &str = concat!(
    env_preamble!(),
    r#"timeout -k 5 60 npm view "@deepseek-ai/dsh@$1" version 2>/dev/null"#
);

/// 拉起受控运行时的 dsh：`$1` = 数据目录**名**（相对 `$HOME`，如 `.dsh-desktop.dev`），
/// `$2` = 端口。
///
/// 二进制固定为 `<数据目录>/runtime/node_modules/.bin/dsh` 的**绝对路径**（v8）：
/// 缺失 / 不可执行时以 96 立即失败，绝不落回 PATH 上的全局 `dsh`。
///
/// 目录在脚本内用 `"$HOME/$1"` 拼出：`$HOME` 必须交给发行版内的 bash 展开，
/// Windows 侧传字面量会被当作普通字符（会创建出名为 `$HOME` 的目录）。
///
/// `--skip-auth` 固定传；能否传由调用方用探测结果的 `skip_auth_ready` 提前把关
/// （未命中直接拒绝启动，不盲传，R-W2-2）。
#[cfg_attr(not(windows), allow(dead_code))] // 仅 Windows 调用路径可达（R-W3-9）
pub const START: &str = concat!(
    env_preamble!(),
    "\n",
    r#"d="$HOME/$1"; b="$d/runtime/node_modules/.bin/dsh"
mkdir -p "$d"
if [ ! -x "$b" ]; then echo 'WSL_RUNTIME_MISSING: managed runtime is not installed in this distro' >&2; exit 96; fi
export DSH_HOME="$d" DSH_TELEMETRY_DISABLED=1 NO_COLOR=1 DSH_WEB_PORT="$2"
printf '%s\n%s\n' "$$" "$(cat /proc/sys/kernel/random/boot_id 2>/dev/null)" > "$d/.harness.pid"
exec "$b" --profile web --port "$2" --no-open --skip-auth"#
);

/// 停止 dsh：`$1` = 数据目录**名**（相对 `$HOME`）。
///
/// `wsl.exe -e` 拉起的命令自成会话（实测 `pid == pgid == sid`），`exec dsh` 后 pid
/// 文件里的 `$$` 就是进程组长——按进程组 kill 才能带走 dsh 的工具子进程（R-W2-7）；
/// 再兜底 `-KILL`，最后清理 pid 文件；始终以 0 退出（幂等）。
///
/// kill 前**三重确认**（R-W3-5）：pid 非空、pid 文件里的 boot_id 等于当前
/// `/proc/sys/kernel/random/boot_id`（不同 = VM 重启过，pid 必然已被复用）、
/// `/proc/<pid>/cmdline` 含 `dsh --profile`。任一条不满足只删文件不 kill——
/// 与 Windows 侧 sweep 的「绝不凭 PID 猜进程」同一约束（F14）。
///
/// 受控入口是 `<runtime>/node_modules/.bin/dsh`（shebang 脚本）；Linux 把
/// **execve 时给定的路径**（不是解析后的符号链接目标）交给解释器，故 cmdline 形如
/// `node /…/runtime/node_modules/.bin/dsh --profile …`，`dsh --profile` 仍然命中
/// （v8 集成验收对新入口复验过）。
#[cfg_attr(not(windows), allow(dead_code))] // 仅 Windows 调用路径可达（R-W3-9）
pub const STOP: &str = concat!(
    env_preamble!(),
    r#"d="$HOME/$1"; f="$d/.harness.pid"; p=$(sed -n 1p "$f" 2>/dev/null); b=$(sed -n 2p "$f" 2>/dev/null); cur=$(cat /proc/sys/kernel/random/boot_id 2>/dev/null); if [ -n "$p" ] && [ "$b" = "$cur" ] && tr '\0' ' ' < "/proc/$p/cmdline" 2>/dev/null | grep -q -- 'dsh --profile'; then kill -TERM -- "-$p" 2>/dev/null || kill -TERM "$p" 2>/dev/null; sleep 1; kill -KILL -- "-$p" 2>/dev/null; kill -KILL "$p" 2>/dev/null; fi; rm -f "$f"; true"#
);

/// 在 **Linux 侧**查找第一个可绑定端口：`$1` = 起始端口，`$2` = 最多尝试个数。
///
/// 输出第一个可绑定的端口号；尝试个数用尽时以 98 退出（调用方识别为扫描耗尽）。
/// 用 node `net.createServer` 而不是 `/dev/tcp`：后者只能 connect，探不出
/// Windows 侧 TIME_WAIT 造成的 bind 失败（F12）。
///
/// 必须在发行版内判定（R-W3-2）：镜像网络下 Linux 能看见 Windows 侧的监听与
/// TIME_WAIT，反过来 Windows 的 `bind()` 探测看不见 Linux 侧刚关闭的 socket——
/// 从 Windows 核心切到 WSL 并立即重启时，Windows 侧探测报「空闲」而 Linux
/// `bind` 失败约 120 s，dsh 随即 `EADDRINUSE` 退出（D-W3-6）。
pub const PORT_SCAN: &str = concat!(
    env_preamble!(),
    r#"node -e 'const net=require("net");const [s,m]=[+process.argv[1],+process.argv[2]];(function t(p,l){if(l===0)process.exit(98);const v=net.createServer();v.on("error",()=>t(p+1,l-1));v.listen(p,"127.0.0.1",()=>v.close(()=>{console.log(p);process.exit(0)}))})(s,m)' "$1" "$2""#
);

/// 在 Linux 侧创建凭据数据目录（R-W5-1）：`$1` = 数据目录**名**（相对 `$HOME`）。
///
/// 必须先于经 UNC 取锁执行（9p 不会自动建链路上的目录）。`umask 077` 只影响本次
/// 新建（目录 0700）；`mkdir -p` 不改变已存在目录的权限（dsh 自建的目录保持原样）。
pub const ENSURE_CREDENTIALS_DIR: &str =
    concat!(env_preamble!(), r#"d="$HOME/$1"; umask 077; mkdir -p "$d""#);

/// 在目标同目录创建 0600 临时文件并输出其**文件名**（R-W5-1，dsh 的原子写协议，
/// F18）：调用方经 UNC 写入内容后用 [`COMMIT_CREDENTIALS`] 提交。
pub const MAKE_CREDENTIALS_TMP: &str = concat!(
    env_preamble!(),
    r#"d="$HOME/$1"; umask 077; t=$(mktemp "$d/.credentials.yaml.import.XXXXXX") || exit 1; basename -- "$t""#
);

/// 提交导入结果（R-W5-1）：`$1` = 数据目录**名**，`$2` = 临时文件**名**。
///
/// 先收紧到 0600，成功才 `mv` 覆盖正式目标（同目录 rename 即提交点）；任一步失败
/// 以非零退出，由调用方报 `WSL_CREDENTIAL_WRITE_FAILED` 并清理临时文件。密钥不在
/// 脚本与位置参数里（内容经 UNC 直接写文件）。
pub const COMMIT_CREDENTIALS: &str = concat!(
    env_preamble!(),
    r#"d="$HOME/$1"; t="$d/$2"; chmod 600 "$t" && mv -f -- "$t" "$d/.credentials.yaml""#
);

/// 确认 / 收紧**既有**凭据文件为 0600（R-W5-1「同值」路径用，不再吞错）：
/// `$1` = 数据目录**名**。文件不存在或 chmod 失败以非零退出，调用方报错。
pub const CHMOD_CREDENTIALS: &str = concat!(
    env_preamble!(),
    r#"d="$HOME/$1"; [ -f "$d/.credentials.yaml" ] && chmod 600 "$d/.credentials.yaml""#
);

/// 准备候选运行时目录（v8）：`$1` = 数据目录**名**。
///
/// 每次候选安装前清空重建 `<数据目录>/runtime-candidate`（0700）；manifest 与锁文件
/// 随后由 Rust 经 UNC 写入，再执行 [`RUNTIME_NPM_CI`]。正式运行时此刻不受影响。
pub const PREPARE_RUNTIME_CANDIDATE: &str = concat!(
    env_preamble!(),
    r#"d="$HOME/$1"; umask 077; rm -rf "$d/runtime-candidate"; mkdir -p "$d/runtime-candidate""#
);

/// 在候选目录内按锁文件重放依赖树（v8）：`$1` = 数据目录**名**，
/// `$2` = 单次尝试秒数。
///
/// `npm ci` 只按 `package-lock.json` 精确重建（六项 Cordis 兼容组件由 manifest 的
/// `overrides` 钉死，其余传递依赖全部由同一把锁固定）；不用 `npm install`
/// （会重新做 `^` 范围解析：cordis 4.0.4 / loader 1.0.5 / hmr 1.0.19 与
/// app-boot 0.1.2-rc.1 不兼容，live 下 dsh 无法启动——W5-R 实测），也不带
/// `--prefer-offline`（D-W5R-1：陈旧 packument 缓存会让精确版本报 `notarget`）。
/// `cd` 进候选目录再执行：npm 只读取项目级 `.npmrc`，不会碰到 `/mnt/...`。
/// `timeout -k 10 "$2"` 让 Linux 侧自带截止时间（R-W2-6）。
pub const RUNTIME_NPM_CI: &str = concat!(
    env_preamble!(),
    "\n",
    r#"cd "$HOME/$1/runtime-candidate" || exit 1
timeout -k 10 "$2" npm ci --no-audit --no-fund"#
);

/// 切换候选运行时为正式运行时（v8）：`$1` = 数据目录**名**，`$2` = stamp。
///
/// 候选已在上一步通过补丁与启动验证。切换以**整个受控运行时为单位**：
/// 1. 清空本次的备份槽 `<数据目录>/runtime-backup-<stamp>`；
/// 2. 现有运行时整体改名进备份槽（首装没有正式运行时则跳过）；
/// 3. 候选整体改名到 `<数据目录>/runtime`。
///
/// 任一步失败以非零退出；第 3 步失败时把备份改回正式位置（`SWITCH_RESTORED`），
/// 恢复不了则明确 `SWITCH_RESTORE_FAILED`（调用方另经 [`RESTORE_RUNTIME`] 兜底）。
/// 候选缺失、目标已存在（不覆盖、不嵌套）都在动作前拦下。
pub const SWITCH_RUNTIME: &str = concat!(
    env_preamble!(),
    "\n",
    r#"d="$HOME/$1"; c="$d/runtime-candidate"; r="$d/runtime"; b="$d/runtime-backup-$2"
if [ ! -d "$c" ]; then echo 'SWITCH_FAILED: candidate missing' >&2; exit 1; fi
rm -rf "$b" || { echo 'SWITCH_FAILED: cannot clear backup slot' >&2; exit 1; }
moved=0
if [ -e "$r" ]; then mv "$r" "$b" || { echo 'SWITCH_FAILED: cannot move current runtime' >&2; exit 1; }; moved=1; fi
if [ -e "$r" ]; then echo 'SWITCH_FAILED: destination exists' >&2; if [ "$moved" = "1" ]; then mv "$b" "$r" 2>/dev/null; fi; exit 1; fi
if mv "$c" "$r"; then printf 'SWITCH_OK\n'; exit 0; fi
echo 'SWITCH_FAILED: cannot activate candidate' >&2
if [ "$moved" = "1" ]; then if mv "$b" "$r"; then echo 'SWITCH_RESTORED' >&2; else echo 'SWITCH_RESTORE_FAILED' >&2; fi; fi
exit 1"#
);

/// 切换阶段失败后的兜底恢复（v8）：`$1` = 数据目录**名**，`$2` = stamp。
///
/// 幂等：正式运行时已存在（切换其实成功 / 已恢复）→ `RESTORE_SKIPPED`；备份不存在
/// → `RESTORE_NOTHING`（两种都按成功处理）；备份存在而正式缺失 → 改回并输出
/// `RESTORE_OK`，失败非零退出。
pub const RESTORE_RUNTIME: &str = concat!(
    env_preamble!(),
    "\n",
    r#"d="$HOME/$1"; r="$d/runtime"; b="$d/runtime-backup-$2"
if [ -e "$r" ]; then printf 'RESTORE_SKIPPED\n'; exit 0; fi
if [ -d "$b" ]; then if mv "$b" "$r"; then printf 'RESTORE_OK\n'; exit 0; fi; echo 'RESTORE_FAILED' >&2; exit 1; fi
printf 'RESTORE_NOTHING\n'; exit 0"#
);

/// 清理**已确认成功**的本次备份（v8；R-V8-1）：`$1` = 数据目录**名**，`$2` = 本次 stamp。
///
/// 只删 `runtime-backup-<stamp>` 这一个槽位——确认（正式启动 + 桌面 readiness
/// 成功）之前不得触碰任何恢复槽；历史遗留槽位由 [`CLEAN_RUNTIME_LEFTOVERS`]
/// 在没有待确认切换时单独清理。目标不存在视为已清理（`CLEAN_NOTHING`）。
pub const CLEAN_RUNTIME_BACKUP: &str = concat!(
    env_preamble!(),
    "\n",
    r#"d="$HOME/$1"; x="$d/runtime-backup-$2"
if [ -e "$x" ]; then rm -rf -- "$x" || { echo 'CLEAN_FAILED' >&2; exit 1; }; printf 'CLEANED %s\n' "$2"; exit 0; fi
printf 'CLEAN_NOTHING %s\n' "$2"; true"#
);

/// 安装窗口开始前的遗留槽位清理（v8-R1 房务；R-V8-1 第 3 条）：`$1` = 数据目录名。
///
/// **只在没有待确认切换时调用**（调用方以 `switch::pending_for` 把关）：那时
/// `runtime-backup-*` / `runtime-failed-*` 都是已确认成功后的残留或已被后续切换
/// 取代的孤儿，没有恢复价值；有待确认切换时一个都不碰。
pub const CLEAN_RUNTIME_LEFTOVERS: &str = concat!(
    env_preamble!(),
    "\n",
    r#"d="$HOME/$1"; n=0
for x in "$d"/runtime-backup-* "$d"/runtime-failed-*; do
  if [ -e "$x" ] && rm -rf -- "$x"; then n=$((n + 1)); fi
done
printf 'CLEANED_LEFTOVERS %s\n' "$n""#
);

/// 切换后正式启动失败的回滚（v8；R-V8-1 第 2 条）：`$1` = 数据目录**名**，
/// `$2` = 本次 stamp。
///
/// 与 [`RESTORE_RUNTIME`] 的差别：**不因 `runtime` 存在就跳过**——正式启动失败时
/// 那个 `runtime` 正是要换掉的失败新树。步骤：
/// 1. 失败的新树整体移出到 `runtime-failed-<stamp>`（保留现场，不删除）；
/// 2. 备份槽整体移回 `runtime`；
/// 3. 失败兜底：若第 2 步失败，把失败树移回原位，保持 `runtime` 存在。
///
/// 备份不存在（切换从未发生 / 已被处理）→ `ROLLBACK_NOTHING`（按成功处理）。
/// 任一步失败非零退出并输出 `ROLLBACK_FAILED`，调用方保留全部文件并明确报告，
/// 不宣称成功。
pub const ROLLBACK_RUNTIME: &str = concat!(
    env_preamble!(),
    "\n",
    r#"d="$HOME/$1"; r="$d/runtime"; b="$d/runtime-backup-$2"; f="$d/runtime-failed-$2"
if [ ! -d "$b" ]; then printf 'ROLLBACK_NOTHING\n'; exit 0; fi
moved=0
if [ -e "$r" ]; then
  rm -rf -- "$f" || { echo 'ROLLBACK_FAILED: cannot clear failed slot' >&2; exit 1; }
  if mv "$r" "$f"; then moved=1; else echo 'ROLLBACK_FAILED: cannot move failed runtime aside' >&2; exit 1; fi
fi
if mv "$b" "$r"; then printf 'ROLLBACK_OK\n'; exit 0; fi
if [ "$moved" = "1" ]; then if mv "$f" "$r" 2>/dev/null; then printf 'ROLLBACK_RESTORED_FAILED_TREE\n' >&2; fi; fi
echo 'ROLLBACK_FAILED: cannot restore backup' >&2; exit 1"#
);

/// 丢弃候选运行时（v8，安装窗口失败收尾）：`$1` = 数据目录**名**。
///
/// 候选失败（`npm ci` / 补丁 / 启动验证）时清场，正式运行时从未被动过。
pub const DROP_RUNTIME_CANDIDATE: &str = concat!(
    env_preamble!(),
    r#"rm -rf "$HOME/$1/runtime-candidate"; printf 'DROPPED\n'; true"#
);

/// 安装后的启动验证（v8）：`$1` = 数据目录**名**，`$2` = 运行时目录**名**
/// （正式 `runtime` 或候选 `runtime-candidate`），`$3` = 端口，`$4` = 总截止秒数。
///
/// 用临时 `DSH_HOME`（`<数据目录>/.wsl-core-verify`）在独立端口拉起**指定**运行时
/// 的 dsh，轮询「boot 页面 + 其声明的全部 client bundle 可加载」——与桌面端
/// `proxy_health_check` 同一判定口径的 Linux 侧镜像（boot HTML 里的
/// `/plugins/**/client.js` 全部返回 200 且不是 SPA fallback 的 HTML）。两层补丁
/// 任何一层失效都会在这里被检出（startup 未命中 → dsh 对未知选项直接退出；
/// connection 未命中 → client bundle 返回 401）。live 的 `patchReload` 路径同样
/// 会被真实启动覆盖（HMR 服务不就绪时 dsh 直接退出）。
///
/// `$4` 是**真实总截止时间**（`date +%s` 差值），不是迭代次数：每次探测都可能
/// 串行 fetch 多个资源，按次数计会把「慢资源」拖成远超预期的时间（R-V8-3）。
/// 每次探测另加 `timeout` 上限，保证循环退出后收尾时间可预期；Rust 中继超时
/// 晚于该截止时间并留出清理余量（见 `install::VERIFY_TIMEOUT`）。
///
/// pidfile 由内层 bash 写入 pid + boot_id 后 `exec` 运行时二进制（`setsid` 使
/// `pid == pgid == sid`，[`START`] 的会话语义一致）；**EXIT / TERM / INT / HUP
/// 统一走 `cleanup`**（三重确认后按进程组 kill），正常成功、验证失败、控制端
/// 中断都不留进程；只操作本次持有的 pid/进程组。成功输出 `VERIFY_OK` 并清理
/// 临时目录；失败输出 `VERIFY_FAILED` 与启动日志尾部（日志保留供诊断）。
pub const VERIFY_CORE: &str = concat!(
    env_preamble!(),
    "\n",
    r#"d="$HOME/$1"; bin="$d/$2/node_modules/.bin/dsh"; v="$d/.wsl-core-verify"; l="$d/.wsl-core-verify.log"; pf="$v/.verify.pid"
if [ ! -x "$bin" ]; then echo 'VERIFY_FAILED: managed runtime binary missing' >&2; exit 1; fi
umask 077; rm -rf "$v"; mkdir -p "$v"
if [ -f "$d/.credentials.yaml" ]; then cp "$d/.credentials.yaml" "$v/.credentials.yaml" && chmod 600 "$v/.credentials.yaml"; fi
cleanup() {
  p=$(sed -n 1p "$pf" 2>/dev/null); b=$(sed -n 2p "$pf" 2>/dev/null); cur=$(cat /proc/sys/kernel/random/boot_id 2>/dev/null)
  if [ -n "$p" ] && [ "$b" = "$cur" ] && tr '\0' ' ' < "/proc/$p/cmdline" 2>/dev/null | grep -q -- 'dsh --profile'; then
    kill -TERM -- "-$p" 2>/dev/null || kill -TERM "$p" 2>/dev/null; sleep 1; kill -KILL -- "-$p" 2>/dev/null; kill -KILL "$p" 2>/dev/null
  fi
  rm -f "$pf"
}
trap 'cleanup' EXIT
trap 'exit 143' TERM INT HUP
export DSH_HOME="$v" DSH_TELEMETRY_DISABLED=1 NO_COLOR=1 DSH_WEB_PORT="$3"
setsid bash -c 'echo $$ > "$1"; cat /proc/sys/kernel/random/boot_id 2>/dev/null >> "$1"; exec "$2" --profile web --port "$3" --no-open --skip-auth' -- "$pf" "$bin" "$3" > "$l" 2>&1 &
secs="$4"; deadline=$(( $(date +%s) + secs )); ok=0
while [ "$(date +%s)" -lt "$deadline" ]; do
  p=$(sed -n 1p "$pf" 2>/dev/null)
  if [ -n "$p" ] && ! kill -0 "$p" 2>/dev/null; then break; fi
  if timeout -k 2 25 node -e '
const base = "http://127.0.0.1:" + process.argv[1];
const fail = c => process.exit(c);
const get = u => fetch(u, { signal: AbortSignal.timeout(2500) });
(async () => {
  let r;
  try { r = await get(base + "/"); } catch (e) { return fail(11); }
  if (!r.ok) return fail(12);
  const html = await r.text();
  const marker = "globalThis[" + String.fromCharCode(34) + "__DSH_BOOT__" + String.fromCharCode(34) + "] = ";
  const i = html.indexOf(marker);
  if (i < 0) return fail(13);
  const j = html.indexOf("</script>", i);
  if (j < 0) return fail(14);
  let boot;
  try { boot = JSON.parse(html.slice(i + marker.length, j).trim().replace(/;\s*$/, "")); } catch (e) { return fail(15); }
  const entries = Array.isArray(boot.entries) ? boot.entries : [];
  const paths = new Set();
  const q = String.fromCharCode(34);
  for (const part of html.split("<script").slice(1)) {
    const a = part.indexOf("src=" + q);
    if (a < 0) continue;
    const rest = part.slice(a + 5);
    const b = rest.indexOf(q);
    if (b < 0) continue;
    const u = rest.slice(0, b).replace(/&amp;/g, "&");
    if (u.startsWith("/plugins/") && u.includes("client.js") && !u.startsWith("//")) paths.add(u);
  }
  for (const en of entries) {
    const u = en && typeof en.url === "string" ? en.url.replace(/&amp;/g, "&") : null;
    if (u && u.startsWith("/plugins/") && u.includes("client.js") && !u.startsWith("//")) paths.add(u);
  }
  if (paths.size === 0) return fail(16);
  let ready = 0;
  for (const p of paths) {
    try {
      const rr = await get(base + p);
      if (!rr.ok) continue;
      const t = (await rr.text()).trimStart();
      if (!t) continue;
      const low = t.slice(0, 16).toLowerCase();
      if (low.startsWith("<!doctype") || low.startsWith("<html")) continue;
      ready++;
    } catch (e) {}
  }
  if (ready !== paths.size) return fail(17);
  console.log("VERIFY_READY " + ready + "/" + paths.size);
  process.exit(0);
})().catch(() => fail(18));
' "$3" >/dev/null 2>&1; then ok=1; break; fi
  sleep 1
done
cleanup
if [ "$ok" = "1" ]; then rm -rf "$v" "$l"; printf 'VERIFY_OK\n'; exit 0; fi
printf 'VERIFY_FAILED\n'; tail -n 20 "$l" 2>/dev/null; exit 1"#
);

/// 验证收尾（v8；R-V8-3）：`$1` = 数据目录**名**。
///
/// 供 Rust 侧的错误/超时路径调用：中继超时只杀 `wsl.exe`，Linux 侧的验证进程组
/// 不会随之退出（F5），必须单独收口。读 pidfile（pid + boot_id）后三重确认
/// （非空、boot_id 一致、cmdline 含 `dsh --profile`）再按进程组 kill——只操作
/// 本次验证自己写下的 pid，绝不按模糊命令匹配。临时目录 `$v` 一并删除，
/// 验证日志 `$l` 保留。幂等：pidfile 缺失 / 不匹配即视为无需清理。
pub const CLEANUP_VERIFY: &str = concat!(
    env_preamble!(),
    "\n",
    r#"d="$HOME/$1"; v="$d/.wsl-core-verify"; pf="$v/.verify.pid"
p=$(sed -n 1p "$pf" 2>/dev/null); b=$(sed -n 2p "$pf" 2>/dev/null); cur=$(cat /proc/sys/kernel/random/boot_id 2>/dev/null)
if [ -n "$p" ] && [ "$b" = "$cur" ] && tr '\0' ' ' < "/proc/$p/cmdline" 2>/dev/null | grep -q -- 'dsh --profile'; then
  kill -TERM -- "-$p" 2>/dev/null || kill -TERM "$p" 2>/dev/null; sleep 1; kill -KILL -- "-$p" 2>/dev/null; kill -KILL "$p" 2>/dev/null
  rm -rf "$v"
  printf 'VERIFY_CLEANED\n'; exit 0
fi
rm -rf "$v"
printf 'VERIFY_CLEAN_NOTHING\n'; true"#
);

#[cfg(test)]
mod tests {
    use super::{
        CHMOD_CREDENTIALS, CLEANUP_VERIFY, CLEAN_RUNTIME_BACKUP, CLEAN_RUNTIME_LEFTOVERS,
        COMMIT_CREDENTIALS, DROP_RUNTIME_CANDIDATE, ENSURE_CREDENTIALS_DIR, ENV_PREAMBLE,
        MAKE_CREDENTIALS_TMP, PORT_SCAN, PREPARE_RUNTIME_CANDIDATE, PROBE, RESOLVE_DSH,
        RESTORE_RUNTIME, ROLLBACK_RUNTIME, RUNTIME_NPM_CI, START, STOP, SWITCH_RUNTIME,
        VERIFY_CORE,
    };

    const SCRIPTS: [(&str, &str); 19] = [
        ("PROBE", PROBE),
        ("RESOLVE_DSH", RESOLVE_DSH),
        ("START", START),
        ("STOP", STOP),
        ("PORT_SCAN", PORT_SCAN),
        ("ENSURE_CREDENTIALS_DIR", ENSURE_CREDENTIALS_DIR),
        ("MAKE_CREDENTIALS_TMP", MAKE_CREDENTIALS_TMP),
        ("COMMIT_CREDENTIALS", COMMIT_CREDENTIALS),
        ("CHMOD_CREDENTIALS", CHMOD_CREDENTIALS),
        ("PREPARE_RUNTIME_CANDIDATE", PREPARE_RUNTIME_CANDIDATE),
        ("RUNTIME_NPM_CI", RUNTIME_NPM_CI),
        ("SWITCH_RUNTIME", SWITCH_RUNTIME),
        ("RESTORE_RUNTIME", RESTORE_RUNTIME),
        ("ROLLBACK_RUNTIME", ROLLBACK_RUNTIME),
        ("CLEAN_RUNTIME_BACKUP", CLEAN_RUNTIME_BACKUP),
        ("CLEAN_RUNTIME_LEFTOVERS", CLEAN_RUNTIME_LEFTOVERS),
        ("DROP_RUNTIME_CANDIDATE", DROP_RUNTIME_CANDIDATE),
        ("VERIFY_CORE", VERIFY_CORE),
        ("CLEANUP_VERIFY", CLEANUP_VERIFY),
    ];

    /// 所有脚本都必须以公共前导开头：`cd $HOME`（R-W2-4）与 nvm 回退（R-W2-3）
    /// 对每条命令一致生效。
    #[test]
    fn all_scripts_start_with_env_preamble() {
        for (name, script) in SCRIPTS {
            assert!(script.starts_with(ENV_PREAMBLE), "{name} 缺少 ENV_PREAMBLE");
        }
        assert!(ENV_PREAMBLE.contains("nvm.sh"));
        assert!(ENV_PREAMBLE.contains("cd \"$HOME\" || exit 97"));
    }

    /// v8：公共前导不得把用户全局前缀塞进 PATH——受控运行时之外不存在任何
    /// 全局 `dsh` 解析路径（脚本一律显式绝对路径）。
    #[test]
    fn env_preamble_has_no_global_prefix_on_path() {
        assert!(!ENV_PREAMBLE.contains(".npm-global"), "{ENV_PREAMBLE}");
        for (name, script) in SCRIPTS {
            assert!(!script.contains(".npm-global"), "{name} 不得引用全局前缀");
        }
    }

    /// 位置参数（`$1`/`$2`…）必须整体落在双引号内：出现未加引号的 `$<digit>`
    /// 即视为把动态值拼进了脚本（注入面，方案 §0.3 规则 1）。
    ///
    /// 判据是「所在区域的双引号状态」而非前一字节——`"pkg@$1"` 这类
    /// 后缀拼接同样安全（`$1` 前是 `@`，但整体在引号内）。
    #[test]
    fn positional_args_are_quoted_in_all_scripts() {
        for (name, script) in SCRIPTS {
            let chars: Vec<char> = script.chars().collect();
            let mut in_double_quotes = false;
            for (idx, ch) in chars.iter().enumerate() {
                match ch {
                    '"' => in_double_quotes = !in_double_quotes,
                    '$' if chars.get(idx + 1).is_some_and(char::is_ascii_digit) => {
                        assert!(
                            in_double_quotes,
                            "{name}: 位置参数必须加双引号（char {idx}）"
                        );
                    }
                    _ => {}
                }
            }
            assert!(!in_double_quotes, "{name}: 双引号未成对（脚本被截断？）");
        }
    }

    /// v8：START 显式调用受控运行时的 `.bin/dsh`（绝对路径，缺失即 96 退出），
    /// 数据目录名经位置参数传入，`$HOME` 由发行版内的 bash 展开。
    #[test]
    fn start_execs_managed_runtime_binary_only() {
        assert!(START.contains("--skip-auth"));
        assert!(START.contains("d=\"$HOME/$1\""));
        assert!(START.contains("b=\"$d/runtime/node_modules/.bin/dsh\""));
        assert!(START.contains("exec \"$b\" --profile web"));
        assert!(START.contains("exit 96"));
        assert!(START.contains("DSH_HOME=\"$d\""));
        assert!(START.contains("DSH_WEB_PORT=\"$2\""));
        assert!(!START.contains("DSH_HOME=\"$1\""));
        // 不得出现裸 `dsh` 命令（经 PATH 解析全局安装）
        assert!(!START.contains("exec dsh"), "{START}");
    }

    #[test]
    fn probe_reports_runtime_keys_and_detects_skip_auth_patch() {
        for key in [
            "NODE=",
            "NODEV=",
            "NPM=",
            "DSH=",
            "DSHV=",
            "ROOT=",
            "RUNTIME=",
            "HOME=",
            "SKIPAUTH=",
        ] {
            assert!(PROBE.contains(key), "PROBE 缺少 {key}");
        }
        // 探测根与二进制都落在受控运行时内
        assert!(PROBE.contains("r=\"$d/runtime\""));
        assert!(PROBE.contains("root=\"$r/node_modules\""));
        assert!(PROBE.contains("b=\"$root/.bin/dsh\""));
        assert!(PROBE.contains("if [ -x \"$b\" ]"));
        assert!(PROBE.contains("DSH_SKIP_AUTH"));
        // 运行时缺失时不得再从任何全局位置兜底
        assert!(!PROBE.contains("npm root -g"));
    }

    /// D-W5R-3：PROBE 的补丁判定覆盖真实 npm 布局的全部三种 connection 位置，
    /// 且与 `patch` 一样按「第一个存在的文件」取目标（web-app 嵌套 > dsh 直属 >
    /// 扁平）——两处候选顺序漂移会让 `skip_auth_ready` 与真实状态相反。
    #[test]
    fn probe_skip_auth_candidates_cover_real_npm_layouts() {
        // 0.1.5 系实测：connection 嵌在 dsh-web-app/node_modules 下
        assert!(PROBE.contains(
            "@deepseek-ai/dsh/node_modules/@deepseek-ai/dsh-web-app/node_modules/@deepseek-ai/dsh-client-connection/lib/index.js"
        ));
        // 0.1.2-rc.1 实测：connection 在 dsh 直属嵌套
        assert!(PROBE.contains(
            "@deepseek-ai/dsh/node_modules/@deepseek-ai/dsh-client-connection/lib/index.js"
        ));
        // 扁平（hoisted）兼容（受控 runtime 的 npm ci 实测命中这一档）
        assert!(PROBE.contains("@deepseek-ai/dsh-client-connection/lib/index.js"));
        assert!(PROBE.contains("@deepseek-ai/dsh-web-app/lib/startup.js"));
        // 与 patch 相同的「第一个存在」语义
        assert!(PROBE.contains("if [ -f \"$p\" ]; then sp=\"$p\"; break; fi"));
        assert!(PROBE.contains("if [ -f \"$p\" ]; then cp2=\"$p\"; break; fi"));
        assert!(PROBE.contains("if [ -n \"$sp\" ] && [ -n \"$cp2\" ]"));
        // 根目录不可用时不得凭空串猜测目标
        assert!(PROBE.contains("if [ -d \"$root\" ]; then"));
    }

    /// v8：候选安装与切换脚本的契约——候选目录清空重建、`npm ci` 只重放锁、
    /// 切换以整个 runtime 为单位且失败可恢复，删除范围绝不越界。
    #[test]
    fn runtime_window_scripts_guard_scope_and_rollback() {
        assert!(PREPARE_RUNTIME_CANDIDATE.contains("rm -rf \"$d/runtime-candidate\""));
        assert!(PREPARE_RUNTIME_CANDIDATE.contains("umask 077"));
        assert!(RUNTIME_NPM_CI.contains("cd \"$HOME/$1/runtime-candidate\""));
        assert!(RUNTIME_NPM_CI.contains("timeout -k 10 \"$2\" npm ci"));
        assert!(!RUNTIME_NPM_CI.contains("npm i "), "必须用 npm ci 重放锁");
        // D-W5R-1：不带 --prefer-offline（陈旧 packument 缓存会让精确版本报 notarget）
        assert!(
            !RUNTIME_NPM_CI.contains("--prefer-offline"),
            "{RUNTIME_NPM_CI}"
        );
        // 切换：候选缺失 / 目标已存在（不覆盖不嵌套）先拦下；失败回滚备份
        assert!(SWITCH_RUNTIME.contains("[ ! -d \"$c\" ]"));
        assert!(SWITCH_RUNTIME.contains("[ -e \"$r\" ]"));
        assert!(SWITCH_RUNTIME.contains("mv \"$r\" \"$b\""));
        assert!(SWITCH_RUNTIME.contains("mv \"$c\" \"$r\""));
        assert!(SWITCH_RUNTIME.contains("SWITCH_OK"));
        assert!(SWITCH_RUNTIME.contains("SWITCH_RESTORED"));
        assert!(SWITCH_RUNTIME.contains("SWITCH_RESTORE_FAILED"));
        // 兜底恢复幂等三态
        assert!(RESTORE_RUNTIME.contains("RESTORE_SKIPPED"));
        assert!(RESTORE_RUNTIME.contains("RESTORE_NOTHING"));
        assert!(RESTORE_RUNTIME.contains("RESTORE_OK"));
        assert!(RESTORE_RUNTIME.contains("exit 1"));
        // v8-R1：确认前的清理只针对本次备份槽（`$2` = stamp），不再遍历所有历史槽
        assert!(CLEAN_RUNTIME_BACKUP.contains("runtime-backup-$2"));
        assert!(CLEAN_RUNTIME_BACKUP.contains("CLEANED %s"));
        assert!(CLEAN_RUNTIME_BACKUP.contains("CLEAN_NOTHING %s"));
        assert!(
            !CLEAN_RUNTIME_BACKUP.contains("runtime-backup-*"),
            "确认清理不得遍历历史恢复槽（R-V8-1）"
        );
        // 历史槽位的豁免清理是独立脚本（调用方以「无待确认切换」把关）
        assert!(CLEAN_RUNTIME_LEFTOVERS.contains("\"$d\"/runtime-backup-*"));
        assert!(CLEAN_RUNTIME_LEFTOVERS.contains("\"$d\"/runtime-failed-*"));
        assert!(CLEAN_RUNTIME_LEFTOVERS.contains("CLEANED_LEFTOVERS"));
        // v8-R1：回滚不因 runtime 存在就跳过——失败新树移出到 runtime-failed-<stamp>，
        // 备份整体移回；失败时把失败树移回原位，绝不删除备份
        assert!(ROLLBACK_RUNTIME.contains("runtime-failed-$2"));
        assert!(ROLLBACK_RUNTIME.contains("mv \"$r\" \"$f\""));
        assert!(ROLLBACK_RUNTIME.contains("mv \"$b\" \"$r\""));
        assert!(ROLLBACK_RUNTIME.contains("ROLLBACK_OK"));
        assert!(ROLLBACK_RUNTIME.contains("ROLLBACK_NOTHING"));
        assert!(ROLLBACK_RUNTIME.contains("ROLLBACK_FAILED"));
        assert!(ROLLBACK_RUNTIME.contains("ROLLBACK_RESTORED_FAILED_TREE"));
        assert!(
            !ROLLBACK_RUNTIME.contains("rm -rf -- \"$b\""),
            "回滚不得删除备份"
        );
        // 清理只清备份槽、失败槽与候选；绝不 rm 正式 runtime / 数据目录本体
        assert!(DROP_RUNTIME_CANDIDATE.contains("rm -rf \"$HOME/$1/runtime-candidate\""));
        for script in [
            CLEAN_RUNTIME_BACKUP,
            CLEAN_RUNTIME_LEFTOVERS,
            ROLLBACK_RUNTIME,
            DROP_RUNTIME_CANDIDATE,
            SWITCH_RUNTIME,
            RESTORE_RUNTIME,
        ] {
            assert!(
                !script.contains("rm -rf \"$d/runtime\""),
                "不得删除正式运行时"
            );
            assert!(!script.contains("rm -rf \"$r\""), "不得删除正式运行时");
        }
    }

    #[test]
    fn stop_kills_process_group_after_three_way_check() {
        assert!(RESOLVE_DSH.contains("timeout -k 5 60 npm view"));
        assert!(STOP.contains("kill -TERM -- \"-$p\""));
        assert!(STOP.contains("kill -KILL -- \"-$p\""));
        assert!(STOP.contains("d=\"$HOME/$1\""));
        assert!(STOP.ends_with("true"));
        // R-W3-5：kill 前三重确认（boot_id + /proc/<pid>/cmdline）
        assert!(STOP.contains("boot_id"));
        assert!(STOP.contains("/proc/$p/cmdline"));
        assert!(STOP.contains("dsh --profile"));
        // START 写 pid + boot_id 两行
        assert!(START.contains("boot_id"));
        assert!(START.contains("printf '%s\\n%s\\n'"));
    }

    /// R-W5-1：凭据脚本的关键属性——新目录 0700、0600 临时文件、先 chmod 后 mv
    /// 的原子提交，以及 chmod 失败不再被 `|| true` 吞掉。
    #[test]
    fn credentials_scripts_use_umask_and_atomic_commit() {
        assert!(ENSURE_CREDENTIALS_DIR.contains("umask 077; mkdir -p \"$d\""));
        assert!(MAKE_CREDENTIALS_TMP.contains("mktemp \"$d/.credentials.yaml.import.XXXXXX\""));
        assert!(MAKE_CREDENTIALS_TMP.contains("basename -- \"$t\""));
        assert!(COMMIT_CREDENTIALS.contains("chmod 600 \"$t\" && mv -f -- \"$t\""));
        assert!(COMMIT_CREDENTIALS.contains("\"$d/.credentials.yaml\""));
        assert!(CHMOD_CREDENTIALS.contains("chmod 600"));
        for (name, script) in [
            ("ENSURE_CREDENTIALS_DIR", ENSURE_CREDENTIALS_DIR),
            ("MAKE_CREDENTIALS_TMP", MAKE_CREDENTIALS_TMP),
            ("COMMIT_CREDENTIALS", COMMIT_CREDENTIALS),
            ("CHMOD_CREDENTIALS", CHMOD_CREDENTIALS),
        ] {
            // 只看脚本主体：公共前导里的 nvm 容错 `|| true` 不算（R-W5-1）。
            let body = script.strip_prefix(ENV_PREAMBLE).unwrap_or(script);
            assert!(
                !body.contains("|| true"),
                "{name}: chmod/提交失败不得吞掉（R-W5-1）"
            );
        }
    }

    /// v8：启动验证脚本的契约——按参数选运行时（正式 / 候选）、临时 DSH_HOME、
    /// pidfile + 进程组 kill、就绪判定覆盖 boot 页面与 client bundle、失败保留日志、
    /// 成功清理现场；v8-R3 追加：**真实总截止时间**、EXIT/TERM 统一清理、
    /// 每次探测有界；独立收尾脚本可按 pidfile 三重确认回收验证进程组。
    #[test]
    fn verify_core_mirrors_desktop_readiness_and_cleans_up() {
        assert!(VERIFY_CORE.contains("DSH_HOME=\"$v\""));
        assert!(VERIFY_CORE.contains(".wsl-core-verify"));
        assert!(VERIFY_CORE.contains("--skip-auth"));
        assert!(VERIFY_CORE.contains("bin=\"$d/$2/node_modules/.bin/dsh\""));
        assert!(VERIFY_CORE.contains("exec \"$2\" --profile web --port \"$3\""));
        assert!(VERIFY_CORE.contains("kill -TERM -- \"-$p\""));
        assert!(VERIFY_CORE.contains("__DSH_BOOT__"));
        assert!(VERIFY_CORE.contains("client.js"));
        assert!(VERIFY_CORE.contains("<!doctype"));
        assert!(VERIFY_CORE.contains("VERIFY_OK"));
        assert!(VERIFY_CORE.contains("VERIFY_FAILED"));
        assert!(VERIFY_CORE.contains("tail -n 20 \"$l\""));
        assert!(VERIFY_CORE.contains("rm -rf \"$v\" \"$l\""));
        // R-V8-3：真实截止时间（时间戳而非迭代次数）+ 每次探测有界
        assert!(VERIFY_CORE.contains("secs=\"$4\"; deadline=$(( $(date +%s) + secs ))"));
        assert!(VERIFY_CORE.contains("while [ \"$(date +%s)\" -lt \"$deadline\" ]"));
        assert!(!VERIFY_CORE.contains("n=$((n+1))"), "不得按迭代次数计时");
        assert!(VERIFY_CORE.contains("timeout -k 2 25 node -e"));
        // R-V8-3：EXIT/TERM 统一清理出口（成功/失败/中断同一收口）
        assert!(VERIFY_CORE.contains("cleanup() {"));
        assert!(VERIFY_CORE.contains("trap 'cleanup' EXIT"));
        assert!(VERIFY_CORE.contains("trap 'exit 143' TERM INT HUP"));
        // pidfile 写 pid + boot_id（三重确认可核对，与 STOP 同一约束）
        assert!(VERIFY_CORE.contains("cat /proc/sys/kernel/random/boot_id"));
        assert!(VERIFY_CORE.contains("'dsh --profile'"));
        // 独立收尾脚本：三重确认 + 进程组 kill + 清临时目录（保留日志）
        assert!(CLEANUP_VERIFY.contains(".verify.pid"));
        assert!(CLEANUP_VERIFY.contains("boot_id"));
        assert!(CLEANUP_VERIFY.contains("'dsh --profile'"));
        assert!(CLEANUP_VERIFY.contains("kill -TERM -- \"-$p\""));
        assert!(CLEANUP_VERIFY.contains("kill -KILL -- \"-$p\""));
        assert!(CLEANUP_VERIFY.contains("rm -rf \"$v\""));
        assert!(CLEANUP_VERIFY.contains("VERIFY_CLEANED"));
        assert!(CLEANUP_VERIFY.contains("VERIFY_CLEAN_NOTHING"));
        assert!(!CLEANUP_VERIFY.contains("rm -rf \"$l\""), "验证日志保留");
    }
}
