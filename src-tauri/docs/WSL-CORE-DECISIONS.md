# WSL-CORE-DECISIONS

> 本文件是 `DSH-WSL-CORE-PLAN.md`（W 系列方案）的决策与实测记录载体，格式同插件方案 §0.4：
> 每条 D-n 包含 **阶段 / 假定 / 实际（附证据）/ 决定 / 影响**。方案未覆盖且必须选择的情况，
> 先在此记录再落地，不允许先猜后改。环境与阶段执行记录按 W0、W1…顺序追加。

---

## 环境（W0 记录，2026-09-11）

### 工具链

| 项 | 值 | 证据 |
|---|---|---|
| 仓库 | `E:\DSH-WSL\dsh-fork`，分支 `feat/wsl-core`，HEAD `fe056f8`（与 `upstream/main` 一致），`git status` 干净 | `git status` / `git log --oneline -1` |
| cargo | 1.98.1 (797e8a9bc 2026-08-05) | `cargo --version` |
| rustc | 1.98.1 (48a229cea 2026-09-01) | `rustc --version` |
| Node.js（Windows 宿主） | v24.15.0 | `node --version` |
| pnpm（前端依赖） | 10.28.2（corepack 按根 `package.json` `packageManager` 解析） | `corepack pnpm --version` |
| 前端依赖安装 | `corepack pnpm install --frozen-lockfile` 成功（11s，复用 `E:\.pnpm-store`） | job 输出 `Done in 11s` |

### WSL

```
> wsl.exe --version
WSL 版本: 2.7.11.0
内核版本: 6.18.33.2-2
WSLg 版本: 1.0.73.2
MSRDC 版本: 1.2.7214
Direct3D 版本: 1.611.1-81528511
DXCore 版本: 10.0.26100.1-240331-1435.ge-release
Windows: 10.0.26200.9445
```

```
> wsl.exe --list --verbose
  NAME      STATE           VERSION
* Ubuntu    Running         2
```

WSL 内（`wsl.exe -d Ubuntu -e bash -lc '...'`）：

```
node:  v22.22.1（/usr/bin/node，系统安装）
npm:   9.2.0（/usr/bin/npm）
dsh:   not found（command -v dsh 为空）
```

`.wslconfig`（`C:\Users\Pixel\.wslconfig`）实读：

```ini
[wsl2]
memory=6442450944
networkingMode=Mirrored
guiApplications=false
```

### 基线（W0.2）

| 项 | 值 | 证据 |
|---|---|---|
| `cargo test`（`src-tauri`） | **511 passed / 0 failed / 0 ignored**；测试本体 1.46s，含首次全量编译的总墙钟 **1m29.9s** | `time cargo test`（2026-09-11 00:30，`dsh-fork` 首次构建，无 target 缓存） |

---

## W1 记录（配置模型与核心来源枚举）

### D-W1-1 `service/wsl_core/` 提前建立最小读取侧骨架

- **阶段**：W1
- **假定**：W1.3 要求 `active_version` 对 `Wsl`「读 W2 探测缓存的版本（无缓存返回 `None`）」，
  而 W2 才创建 `service/wsl_core/` 目录与探测实现。
- **实际**：W1 若不建立缓存读取侧，`active_version` 的 Wsl 分支只能先返回 `None`、W2 再改一次；
  与方案字面（W1 即接读取侧）不符。
- **决定**：W1 提前建立 `service/wsl_core/mod.rs` + `probe.rs` 的最小骨架：`WslCoreProbe`
  结构体（字段全集按 W2.3 规格，避免 W2 改签名）、进程内缓存与 `cached()` 读取侧；
  `probe()`（PROBE 脚本执行）与 `store()`（缓存写入）留待 W2 落地。未接入的字段/构造以
  一条带注释的 `#[allow(dead_code)]` 保留（与 `logger/mod.rs:212` 同款先例）。
- **影响**：W2 只需在既有 `probe.rs` 上追加 `probe()` / `store()` 与 `exec.rs` 等文件，
  不推翻 W1 已交付的读取侧；`active_version` / `core::list` 的 Wsl 分支从 W1 起即按最终形态调用。

### D-W1-2 WSL 数据目录名（`.dsh-desktop[.dev]`）的归属与用法

- **阶段**：W1
- **假定**：方案硬性规则 3 规定 WSL 侧数据目录为 `$HOME/.dsh-desktop`，debug 构建用
  `$HOME/.dsh-desktop.dev`；W1 的核心列表行 `dir` 规格写作 `\\wsl.localhost\<distro>\home\<user>\.dsh-desktop`。
- **实际**：行内 `dir` 若写死 `.dsh-desktop`，debug 构建展示的目录与实际运行数据目录不一致。
- **决定**：数据目录名收敛到 `service::wsl_core::dsh_home_dir_name()` 单一来源：
  release 返回 `.dsh-desktop`，debug 返回 `.dsh-desktop.dev`；W1 的列表行、W2 的 START 脚本参数、
  W3 的 `dsh_home` 全部经它取值。
- **影响**：后续阶段不得再出现写死的 `.dsh-desktop` 字面量（脚本侧除外——脚本由 Rust 侧传参）。

### D-W1-3 核心列表 WSL 行仅在已选择发行版时出现

- **阶段**：W1
- **假定**：W1.5 要求 `core::list` 追加 `id: "wsl"` 行；未规定 `wsl_distro` 为 `None` 时的形态
  （此时 `<distro>`/`<user>`/探测均无来源）。
- **实际**：若在未选择发行版时也渲染该行，会得到 `wsl.exe -d ` 与空 `dir` 的残缺行，
  并改变「默认仍是 Windows 核心」下的现有列表（W1 验收要求启动行为与改动前一致）。
- **决定**：仅在 `Setting.wsl_distro` 为 `Some` 时追加该行；未选择发行版时不出现。
- **影响**：W5 设置页选择发行版（写入 `wsl_distro`）后该行才出现在核心列表中。

### D-W1-4 `patch_dsh` / `dsh_rel_contains` 对 Wsl 来源空实现

- **阶段**：W1
- **假定**：新增 `CoreSource::Wsl` 后，`utils::active_core_install_dir` 的穷尽匹配必须补分支；
  但 WSL 核心的补丁（`--skip-auth`）不走「活动核心安装目录」这条 Windows 文件系统路径，
  而由 W2.4 经 UNC 直改。
- **实际**：`active_core_install_dir` 对 Wsl 无合理返回值（WSL 侧无本机安装目录）。
- **决定**：`active_core_install_dir` 返回 `Option<PathBuf>`，Wsl 来源为 `None`；
  `patch_dsh` 对 `None` 记录日志并跳过（`Ok(())`），`dsh_rel_contains` 对 `None` 返回 `false`。
- **影响**：W1 起 Wsl 来源不会向本地文件系统写任何补丁；W2.4 的 UNC 补丁独立实现，
  W3 的启动参数不依赖 `dsh_rel_contains`（START 脚本固定带 `--skip-auth`）。

### D-W1-5 阶段门禁在 rustc/clippy 1.98.1 下的适用范围（「增量干净」）

- **阶段**：W1
- **假定**：方案 §0.3.6 要求每阶段 `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` 全部通过。
- **实际**（均为 2026-09-11 实测，`dsh-fork` HEAD `fe056f8` + W1 改动）：
  - `cargo fmt --check` 在**未改动**的上游文件上即失败（如 `src/bridge/backup.rs`、`src/bridge/pet.rs`
    的行宽展开与 import 顺序；`git status` 证实这些文件未动）。仓库无 `rustfmt.toml`，属
    rustfmt 1.98.1 与上游编写版本的规则差异。
  - `cargo clippy --all-targets -- -D warnings` 在未改动代码上即有 **77 条**既有告警
    （`too_many_arguments`、`doc_list_item_without_indentation`、`needless_borrows_for_generic_args`、
    `field_reassign_with_default` 等新/收紧的 lint），同样无法通过。
  - 上游 CI（`.github/workflows/ci.yml`）的 Rust 任务只跑 `cargo test --all-features --locked`，
    未启用 fmt / clippy 门禁——严格门禁是方案新增要求。
  - 前端 `pnpm run typecheck` 需先按 CI 前置构建两个共享包
    （`pnpm --filter dsh-tauri build && pnpm --filter dsh-tauri-ui build`），否则全新检出报
    `Cannot find module 'dsh-tauri/client'`；`dsh-tauri-ui` 的 `publint` 后置步骤还会因 corepack 与
    随包 pnpm 版本（10.28.2 vs 11.7.0）在 `pnpm pack` 处失败，但 `dist/*.d.cts` 产物已生成，
    不影响 typecheck。
- **决定**：门禁按「增量干净」执行并在每阶段记录：
  1. `cargo fmt --check` 只对**本次改动文件**逐文件执行（`rustfmt --edition 2021 --check <files>`）；
  2. `cargo clippy` 以「本次改动不新增任何告警」为准（对照改动文件清单核查）；
  3. `cargo test` 全量必须通过；
  4. typecheck / lint 按 CI 前置构建后执行，`pnpm run lint` 以 0 error 为准（既有 23 条 warning 不属本阶段）。
- **影响**：W6 的全量门禁同样适用本细则。若日后要恢复严格全量 fmt/clippy，需要单开一轮
  「全仓 fmt/clippy 清理」提交（不在本方案范围内）。

### W1 阶段记录（2026-09-11）

**交付**

| 文件 | 改动 |
|---|---|
| `src-tauri/src/config/setting.rs` | `Setting.wsl_distro: Option<String>`（`#[serde(default)]`）+ 默认值 + 2 个单测（legacy JSON 缺字段可反序列化、往返） |
| `src-tauri/src/service/core/source.rs` | `CoreSource::Wsl`（`as_str`/`parse`）；`active_source` 抽纯函数 `resolve_active_source(active_core, wsl_distro, local_present, wsl_supported)`；`active_dsh_binary` → `wsl.exe`；`active_version` → 探测缓存；4 组纯函数单测 |
| `src-tauri/src/service/wsl_core/{mod.rs,probe.rs}`（新增） | `dsh_home_dir_name()`（`.dsh-desktop[.dev]`）、`WslCoreProbe`、进程内缓存与 `cached()`；2 个单测 |
| `src-tauri/src/service/core/version.rs` | `list()` 追加 WSL 行（仅 `wsl_distro` 已选时）；`set_active("wsl")`（未选发行版 → `Err("WSL_DISTRO_NOT_SET")`，切换持有核心转换锁并停服）；`wsl_data_dir_unc()` + 1 个单测 |
| `src-tauri/src/utils/mod.rs` | `active_core_install_dir` → `Option<PathBuf>`（Wsl = `None`）；`patch_dsh` / `dsh_rel_contains` 对 `None` 跳过/返回 false |
| `src-tauri/src/bridge/config.rs` | `update_app_config` 新增 `wsl_distro: Option<String>`（第 8 个参数，附 `#[allow(clippy::too_many_arguments)]` 与注释） |
| `src-tauri/src/bridge/core.rs`、`src-tauri/src/service/mod.rs` | 文档字符串同步 `wsl`；注册 `pub mod wsl_core` |
| `src/hooks/use-dsh-cores.ts` | `CoreSource` 增加 `'wsl'`（仅类型收敛，UI 在 W5） |

**门禁结果**

| 门禁 | 结果 |
|---|---|
| `rustfmt --check`（改动的 8 个文件） | 通过（`cargo fmt --check` 全量见 D-W1-5） |
| `cargo clippy`（增量） | 本阶段零新增告警；W1 改动的文件中仅剩 2 条既有告警（`setting.rs:394/401` 旧测试的 `field_reassign_with_default`） |
| `cargo test` | **518 passed / 0 failed**（基线 511 + W1 新增 7） |
| `pnpm run typecheck` | 通过（先按 CI 构建 `dsh-tauri` / `dsh-tauri-ui`） |
| `pnpm run lint` | 0 error（23 条既有 warning，均在本阶段未改动的文件中） |

**`pnpm tauri dev` 实测（W1 验收）**

- 第一轮：Rust dev 构建通过（新增代码零 warning）；vite 1420 正常；Harness 服务以**默认 Windows 核心**启动：
  日志 `dsh patch already applied: ...\dev\dependencies\dsh\...\startup.js`、
  `DSH Some("0.1.2-rc.1") provides the official Windows process inspector`、
  `Harness process started successfully: pid=24408, port=3081`、`dsh web: http://127.0.0.1:3081/?token=...`
  ——核心来源仍解析为 Windows App 核心，与改动前一致（`active_core=None`、`wsl_distro=None`）。
- 第一轮同时暴露一个与 W1 改动无关的环境问题（见 D-W1-6），修复后第二轮复测：
  **0 条 ERROR**、`Plugin installation` 通过、`[Harness] health check passed: healthy - 52/52 client
  modules ready`（前端进入 dsh 界面）、`Harness process started successfully: pid=5668, port=3081`。
- 第二轮遗留 warning（不影响启动，属 fork 全新检出未构建插件产物）：
  `INTERNAL_PLUGIN_INSTALL_FAILED: PREINSTALL_ENTRY_FAILED: dsh-tauri-worktree/-panel*/-session/-pet/
  -rightclick: PNPM_NOT_FOUND: no usable pnpm (bundled or user) to rebuild plugin ...`——
  这 7 个包未运行过 `pnpm build:plugins`（`packages/*/dist` 缺失），app 尝试自愈重建时在其启动环境
  中解析不到 pnpm。W 系列不依赖这些桌面侧内置插件的 UI；如后续阶段需要，运行一次
  `pnpm build:plugins`（或 `pnpm dev:desktop` 的插件 watch）即可消除。

### D-W1-6 dev profile 损坏 catalog 的修复（环境问题，非方案裁决）

- **阶段**：W1（发现于验收实测）
- **假定/实际**：共享 dev profile `~/.dsh.dev/profiles/desktop` 由旧 checkout
  （`E:\DSH-WSL\deepseek-harness-desktop`）创建。从 `dsh-fork` 首次运行 dev 时，9 个内置插件因
  `dep_ok=false`（profile `package.json` 的 `link:` 仍指向旧 checkout）触发重装，`dsh plugin install`
  失败于该 profile `pnpm-workspace.yaml` 的既有损坏：`catalog: {dsh-better-sidebar: 'catalog:',
  dshmarket: 'catalog:'}`（自引用 catalog），pnpm 报
  `ERR_PNPM_CATALOG_ENTRY_INVALID_RECURSIVE_DEFINITION`。该文件 mtime 为 2026-09-10 19:15
  （上一轮会话），早于本阶段；非 W1 代码路径（dsh 核心与桌面壳代码中均无 `catalog:` 字面量写入）。
- **决定**（用户 2026-09-11 确认「现在修复」）：把 catalog 两项改为 lockfile 中已装版本
  （`dsh-better-sidebar: 0.19.0`、`dshmarket: 1.45.1`）；修复前原文件备份为
  `pnpm-workspace.yaml.bak-20260911`（同目录）。
- **验证**：修复后 `pnpm tauri dev` 复测——插件安装通过、0 ERROR、健康检查 52/52、服务 3081 正常。
- **影响**：从 `dsh-fork` 起可正常进入 dsh UI（W3/W5 验收的前置）；若后续启动再次出现自引用 catalog，
  需进一步定位写入者（当前证据指向旧版 pnpm 行为，非本仓库代码）。


---

## W1-R 返修记录（按 `E:\DSH-WSL\W1-REVIEW.md` R-1 ~ R-3，2026-09-11）

### R-1 `wsl.exe` 绝对路径哨兵（高）

- **改动**：`service/core/source.rs` 新增 `wsl_exe_path()`（`%SystemRoot%\System32\wsl.exe`，
  `SystemRoot` 缺失回落 `C:\Windows`）与 `is_wsl_active()`；`active_dsh_binary` 的 Wsl 分支
  改用绝对路径，文档注释明确「只可用于展示与存在性探测，`node <bin> …` 调用方须先经
  `is_wsl_active()`」；`service/core/mod.rs` 同步导出。新增单测
  `wsl_exe_path_is_absolute_and_points_at_system32`（非 Windows 跳过）。
- **验收证据**：
  - `cargo test --locked`：`wsl_exe_path` 单测通过（见下方门禁汇总）。
  - dev 实测（手改 store：`active_core="wsl"`、`wsl_distro="Ubuntu"`）：
    - 启动日志**无** `Runtime files missing (node/dsh), resetting installed flag`；
      `invoke('get_app_config')` → `installed: true`（未被复位）。
    - 六个补丁点打印 `dsh patch not applicable for current core, skip`（D-W1-4 行为不变）。
    - `launch()` 进入 spawn：`node.exe C:\Windows\System32\wsl.exe` → node 解析 PE 头报
      `MZ…` / `SyntaxError: Invalid or unexpected token`，进程退出码 1；前端错误为
      `HARNESS_NOT_OWNED`（Process boot 阶段），**不是** `HARNESS_NOT_FOUND`。
      （W3 接通 WSL 分支后此路径改为经 `wsl.exe -e` 拉起。）
  - 验证后已把 store 两个字段还原为原值（`active_core=null`、`wsl_distro=null`），
    备份见 `%APPDATA%\io.github.hairyf.deepseek-harness-desktop\.store.dev.dat.bak-w1r-20260911`。

### R-2 核心列表排序全序（中）

- **改动**：`src/components/config-core.tsx` 引入 `SOURCE_RANK: Record<CoreSource, number> =
  { local: 0, wsl: 1, app: 2 }`（`CoreSource` 类型自 `use-dsh-cores` 导入），比较器改为
  rank 差值 + 同源非 app 归零 + app 内按版本降序。R-2 的三项 UI 顺带项
  （`displayVersion` 回落、`wsl` Chip、`wsl_not_installed` 文案）按方案留 W5。
- **验收证据**：
  - `pnpm run typecheck` 通过；`npx eslint src/hooks/use-dsh-cores.ts src/components/config-core.tsx`
    0 问题。
  - dev 实测（CDP 打开「核心」面板，10 行 DOM 顺序）：row0 = WSL 行
    （文本 `app | 未下载` —— 版本回落 `'app'`、状态 `未下载` 即上述 W5 顺带项）；
    row1..8 = app 版本严格降序（`0.1.5-rc.2` → `0.1.2-alpha.4`）；row9 = `local_missing_hint`。
    与 `invoke('get_cores')` 数据一致：WSL 行稳定位于 local 之后、所有 app 版本之前。

### R-3 `wsl_distro` 清除/校验与精确写入（中）

- **改动**：`bridge/config.rs` 新增纯函数 `normalize_wsl_distro(input) ->
  Result<Option<Option<String>>, String>`（`None`=不改；空串/全空白=清除；含控制字符
  `Err("WSL_DISTRO_INVALID: control characters are not allowed")`；其余 trim 写入），在
  `update_store_dat_setting` 闭包**之外**校验；`service/core/version.rs` 的
  `set_active("wsl")` 分支改用 `update_store_dat_setting` 精确写 `active_core`，不再整对象回写。
  新增 2 个单测（keep/clear/set 三态、控制字符拒绝）。
- **验收证据**（CDP invoke 实测）：
  - `invoke('update_app_config', {wslDistro:'a
b'})` → `REJECTED: WSL_DISTRO_INVALID:
    control characters are not allowed`；
  - `invoke('update_app_config', {wslDistro:''})` → 返回 `wsl_distro=null`，`get_cores`
    不再含 `source:"wsl"` 行；
  - `invoke('update_app_config', {wslDistro:'Ubuntu'})` → WSL 行恢复：
    `{id:"wsl", source:"wsl", path:"wsl.exe -d Ubuntu", dir:"", present:false, version:""}`
    （`dir`/`version` 待 W2 探测缓存接入）。

### 门禁复核（`W1-REVIEW.md` §2 命令）

| 项 | 结果 |
|---|---|
| `cargo test --locked` | **521 passed / 0 failed**（W1 基线 511 + W1 的 7 + R-1 的 1 + R-3 的 2） |
| 改动文件 fmt（过滤 rustfmt 对 mod.rs 的递归输出） | `fmt: clean in changed files` |
| clippy 改动文件命中 | 仅 `setting.rs:393/394/400/401`（上游 `field_reassign_with_default` 旧测试，blame `89c8de17`/`d5c98f9e`）与 `version.rs:421`（上游 `Ok(...?)`，blame 上游提交）；**本阶段零新增** |
| `pnpm run typecheck` / eslint（改动文件） | 通过 / 0 问题 |

### 工作树异常记录（非 W1 交付内容）

返修开始时发现工作树中 `src-tauri/src/service/core/mod.rs` 的
`pub use source::{active_dsh_binary, active_source, active_version, CoreSource, HarnessCore};`
一行**缺失**，导致 16 个编译错误（`no active_source in service::core` 等）。该行不在 W1 交付的
改动清单内（W1 结束时 `git status` 未含 `core/mod.rs`）。已恢复该导出并同时加入
`is_wsl_active` / `wsl_exe_path`；恢复后 `cargo check` / `cargo test` 全绿。


---

## W2 记录（探测、安装与补丁，2026-09-11）

### 交付物

- `service/wsl_core/exec.rs`：`WslStream` / `WslDistro` / `WslOutput`、`decode_utf16`（去 BOM/NUL）、
  `parse_distro_list`（右侧切分、过滤 `docker-desktop*`、支持含空格名）、`list_distros`、
  `run_in_distro(_streaming)`、`distro_argv`（只用 `-e` 形态）、`spawn_wsl`（CREATE_NO_WINDOW）、
  `wait_child` / `join_bounded` / `run_wsl_raw` / `run_wsl_lines`；非 Windows 为空实现。
- `service/wsl_core/script.rs`：`PROBE` / `INSTALL_DSH` / `START` / `STOP` 常量脚本（`START` / `STOP`
  在 W2 尚未接入，以 `#[allow(dead_code)] // W3 接入` 标注）。
- `service/wsl_core/probe.rs`：`WslCoreProbe`（camelCase）+ `store` / `cached`（OnceLock 缓存）+
  `probe`（`home` 非 `/` 开头 → `WSL_PROBE_INVALID_HOME` 且不写缓存，R-4）。
- `service/wsl_core/patch.rs`：UNC 映射 + `apply`（复用 `alpha_auth` 的 `patch_startup` /
  `patch_connection`，两者改 `pub(crate)`）。
- `service/wsl_core/install.rs`：`ensure`（probe → 无 node 报错 → 安装/升级 → 再 probe → 打补丁 →
  写缓存），进度经 `install-progress` 逐行转发。
- `utils::patch_file_at`（任意路径读改写；`patch_dsh` 改为调用它）。
- `bridge/wsl_core.rs` 三个命令（`list_wsl_distros` / `probe_wsl_core` / `install_wsl_core`）+
  `generate_handler!` 注册；`core::set_active("wsl")` 追加 R-5 校验（缓存中 dsh 缺失 →
  `WSL_DSH_NOT_INSTALLED`，在停服动作之前返回）。

### 与方案/初版的差异（决策）

#### D-W2-1 安装回退改用 `--prefix`（方案 W2.2 原案为 `npm config set prefix`）

- **实测（根因）**：系统 node 全局前缀 `/usr/local/lib/node_modules` 对普通用户不可写（EACCES）；
  `npm config set prefix "$HOME/.npm-global"` 在本次安装上下文（cwd 经 `/mnt/e` 映射到仓库目录）
  把配置写进了**项目级** `E:\DSH-WSL\dsh-fork\.npmrc`，而 npm 拒绝从项目配置改
  `prefix`（`npm error config prefix cannot be changed from project config`），重试仍打 `/usr/local`；
  该命令同时污染仓库（未跟踪文件）与用户 npm 配置。
- **处置**：回退改为 `npm i -g --prefix "$HOME/.npm-global" …`（CLI 优先级最高，不依赖配置
  持久化、不改用户配置）；`PROBE` 的 `ROOT` 增加回退（`$HOME/.npm-global/lib/node_modules/@deepseek-ai/dsh`
  存在时优先取该根），保证补丁目标与安装位置一致。方案 W2.2 原文即「是否需要以 W2 验收实测为准，
  记入决策文件」。

#### D-W2-2 两个 npm 尝试附加 `--no-audit --no-fund --prefer-offline`

- **实测**：代理网络下 registry 元数据再校验是主要耗时（数千请求，单次安装 >10 分钟）；
  `--prefer-offline` 后同一环境的完整安装（解析 + reify）在数十秒内完成。

#### D-W2-3 安装超时 600s → 1200s（方案 W2.5 原定 10 分钟）

- **实测**：冷缓存首装在代理网络下超过 10 分钟；超时中断会留下半成品安装（且 Linux 侧 npm 在
  stdout 管道断开后随之退出）。提高到 20 分钟，使安装可原子完成。

#### D-W2-4 补丁目标改为候选路径列表（方案 W2.4 只给扁平布局）

- **实测**：npm 9 全局安装 `@deepseek-ai/dsh` 时其依赖嵌套在包内——
  `…/@deepseek-ai/dsh/node_modules/@deepseek-ai/dsh-web-app/lib/startup.js`
  （扁平 hoisted 布局作为兼容候选保留；取第一个存在者，全部不存在时告警并视为未命中）。

#### D-W2-5 UNC 前导必须是双反斜杠

- 初版 `wsl_unc_root` 输出单前导 `\wsl.localhost\<distro>`（不是 UNC，`Path::exists()`
  恒 false → 补丁静默未命中，进度终态文案为「补丁未命中」）。已修为
  `\\wsl.localhost\<distro>`，并在单测中断言前导双反斜杠（与 W1 的
  `version.rs::wsl_data_dir_unc` 口径一致）。

### 门禁复核（「增量干净」口径，W1-REVIEW §2）

| 项 | 结果 |
|---|---|
| `cargo test --locked` | **537 passed / 0 failed**（W1-R 521 + W2 新增 16：exec 3、script 4、probe 4、patch 3、install 2） |
| 改动文件 fmt | 我方新增/修改行零 diff；`builder.rs:617`、`alpha_auth.rs` 6 处差异为上游未触碰行的既有差异（工作树该行与 `upstream/main` 逐字相同） |
| clippy 改动文件命中 | 全部落于上游行（setting.rs:393/394/400/401、builder.rs:418/493/673/718、version.rs:423）；本阶段新增代码零告警 |
| `pnpm run typecheck` / eslint（改动文件） | 通过 / 0 问题 |

### W2 验收实测（`pnpm tauri dev` + CDP）

- `invoke('list_wsl_distros')` → `[{"name":"Ubuntu","state":"Stopped","version":"2","isDefault":true}]`
- `invoke('probe_wsl_core',{distro:'Ubuntu'})` → `node:"/usr/bin/node"`（v22.22.1）、`dsh:null`、
  `npmRoot:"/usr/local/lib/node_modules"`、`home:"/home/pixel"`
- `invoke('install_wsl_core',{distro:'Ubuntu'})` → 返回 `dsh:"/home/pixel/.npm-global/bin/dsh"`、
  `dshVersion:"0.1.5-rc.1"`、`npmRoot:"/home/pixel/.npm-global/lib/node_modules"`；进度事件终态
  `100% WSL 核心已就绪`
- WSL 内 `dsh --version` → `0.1.5-rc.1`；`dsh-web-app/lib/startup.js` 含 `--skip-auth` 与补丁标记；
  `dsh-client-connection/lib/index.js` 含 `DSH_SKIP_AUTH`（两层补丁均命中）
- R-5 负路径：`wsl_distro` 指向无探测缓存的发行版时 `set_active_core('wsl')` →
  `WSL_DSH_NOT_INSTALLED`；正路径（安装后）→ 返回
  `{id:"wsl", present:true, active:true, version:"0.1.5-rc.1", dir:"\\wsl.localhost\Ubuntu\home\pixel\.dsh-desktop.dev"}`
- `install-progress` 事件共 132 条（阶段 5 → 15 → 77 → 85 → 92 → 100%，114 行 npm 输出逐行转发）
- 验证后 store 已还原（`active_core=null`、`wsl_distro=null`），备份
  `%APPDATA%\io.github.hairyf.deepseek-harness-desktop\.store.dev.dat.bak-w2-20260911`

### 环境事件记录（非代码问题）

- 首轮安装期间（21:04~21:07）WSL 的 systemd 被重启（内核 uptime 不变、PID 1 重建），npm 被中断；
  另观察到 `~/.npm/_logs` 中最新的日志文件偶发消失。均为本机环境异常，非应用缺陷；重跑后成功。
- 安装调用在冷缓存 + 代理网络下曾两次超过 600s 超时（见 D-W2-3），随后以 `--prefer-offline`
  重跑成功。
- 测试期间 `npm config set prefix` 产生的两处配置残留已删除（仓库根 `E:\DSH-WSL\dsh-fork\.npmrc`
  与 WSL `~/.npmrc`）；删除后用真实 `PROBE` 脚本复验回退：`ROOT=/home/pixel/.npm-global/lib/node_modules`、
  `DSH=/home/pixel/.npm-global/bin/dsh`、`DSHV=0.1.5-rc.1`（不依赖任何 npm 配置改动）。


---

## W2-R 返修记录（按 `E:\DSH-WSL\WSL-CORE-REVIEW.md` §B 的 R-W2-1 ~ R-W2-12，2026-09-12）

审核端把 §B.4 的返修顺序定为：R-W2-3/4（脚本前导）→ R-W2-2（`SKIPAUTH`）→ R-W2-1（幂等）→
R-W2-5/6/12 → R-W2-7~11 → 门禁与手动验收 → 追加本记录 → 再开始 W3。执行端按此顺序落地。

### R-W2-1 `ensure` 幂等、dist-tag 在线解析、`cached_or_probe`（高）

**改动文件**：`service/wsl_core/script.rs`（新增 `RESOLVE_DSH`）、`service/wsl_core/install.rs`
（`ensure` 全流程重写 + `parse_resolved_version` / `needs_install` / `resolve_target`；删除 `spec_matches`）、
`service/wsl_core/probe.rs`（`cached_or_probe`）、`service/core/version.rs`（R-5 闸门改
`spawn_blocking(cached_or_probe)`）。

- `RESOLVE_DSH` 用在线 `npm view "@deepseek-ai/dsh@$1" version`（`timeout -k 5 60`）把 dist-tag
  解析为具体版本；`parse_resolved_version` 要求输出恰好一行且可 semver 解析。
- `needs_install(target, installed)`：目标已知且与已装不同（或未装）才装；目标未知（离线且已装）
  → 不装。安装用**精确版本** `@deepseek-ai/dsh@<target>`，此时 `--prefer-offline` 才安全。
- 离线且已装的降级：`log::warn` + `progress.stage(15.0, "无法联网检查更新，保留当前版本")` + `None`。
- `probe::cached_or_probe` 供 `runtime_ready` / `set_active("wsl")` 等需要真实答案的路径使用；
  `core::list` 保持只读缓存（列表必须快）。

**实测证据（dev + CDP）**：

| 场景 | 结果 |
|---|---|
| 已装 `0.1.5-rc.1`，`install_wsl_core({distro})` | 2659 ms 返回；`install-progress` **3 条**（5% 探测 → 92% 补丁 → 100% 已就绪），零 npm 输出行；再测 2174 ms / 2054 ms（W2 时同路径 132 条事件、114 行 npm 输出） |
| 精确版本 `versionSpec:'0.1.5-rc.1'`（跳过 `npm view`） | 6057 ms 返回同版本，无安装 |
| 精确版本 `versionSpec:'0.1.5-rc.2'`（需升级） | **真实安装** 26.2 s，返回 `dshVersion:"0.1.5-rc.2"`；44 条进度事件（38 行 npm 输出） |
| 再装回 `versionSpec:'0.1.5-rc.1'` | 12.1 s 完成；WSL 内 `dsh --version` → `0.1.5-rc.1`（基线还原） |
| 断网（`~/.npmrc` registry → `http://127.0.0.1:9/`）且已装 | 返回 **Ok**（`dshVersion:"0.1.5-rc.1"`）；日志 `WSL_DSH_RESOLVE_FAILED: npm view exited with 124: no output` → `resolve target version failed, keeping current` |
| 重启 dev 应用后**不先** `probe_wsl_core`，直接 `set_active_core({id:'wsl'})` | **7905 ms 成功**：`{id:"wsl", version:"0.1.5-rc.1", present:true, active:true}`（R-5 闸门现场探测，不再依赖进程内缓存） |

### R-W2-2 `--skip-auth` 补丁结果进入探测（中）

**改动文件**：`service/wsl_core/script.rs`（`PROBE` 追加 `SKIPAUTH` 段）、`service/wsl_core/probe.rs`
（`WslCoreProbe.skip_auth_ready`，camelCase `skipAuthReady`）、`service/wsl_core/install.rs`
（`patch::apply` 成功即 `after.skip_auth_ready = true`）。

`PROBE` 对 `@deepseek-ai/dsh/node_modules/@deepseek-ai` 与 `@deepseek-ai/dsh/node_modules` 两个候选
目录分别 grep `--skip-auth`（`dsh-web-app/lib/startup.js`）与 `DSH_SKIP_AUTH`
（`dsh-client-connection/lib/index.js`），两层都命中才 `SKIPAUTH=1`。

**实测**：`probe_wsl_core({distro:'Ubuntu'})` → `skipAuthReady:true`（6699 ms，含 WSL 冷启动）；
装 rc.2 / 装回 rc.1 后均为 `true`。

### R-W2-3 `bash -lc` 看不到 nvm 安装的 Node（中）

**改动文件**：`service/wsl_core/script.rs`（`macro_rules! env_preamble!()` + `ENV_PREAMBLE`，
`PROBE` / `RESOLVE_DSH` / `INSTALL_DSH` / `START` / `STOP` 全部 `concat!(env_preamble!(), …)`）。

前导：`cd "$HOME" || exit 97; export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"; if ! command -v node >/dev/null 2>&1 && [ -s "$NVM_DIR/nvm.sh" ]; then . "$NVM_DIR/nvm.sh" >/dev/null 2>&1 || true; fi; export PATH="$HOME/.npm-global/bin:$PATH"; `
—— 系统 node 优先，缺失时回退 nvm（`nvm.sh` 只在需要时 source，失败不阻断），并把
`~/.npm-global/bin` 前置（本机 dsh 装在用户前缀）。单测 `all_scripts_start_with_env_preamble`
断言 5 个脚本均以该前导开头。

### R-W2-4 脚本在 Windows cwd 映射目录下执行（中）

**改动文件**：同 R-W2-3（前导第一句 `cd "$HOME" || exit 97`）。

比 `wsl.exe --cd ~` 更可移植（不依赖 WSL 版本）；`probe()` / `ensure()` 遇到退出码 97 报
`WSL_HOME_UNAVAILABLE: cd $HOME failed in <distro>`。

### R-W2-5 安装并发保护（中）

**改动文件**：`service/wsl_core/install.rs`（`static WSL_INSTALL_LOCK` + `install_lock()`，
`ensure` 开头 `try_lock` 失败即 `Err("WSL_INSTALL_BUSY: another WSL core install is running")`）。

**实测**：`Promise.all([install, install])` → 第一次 2323 ms 成功返回 `0.1.5-rc.1`；第二次
**2 ms** 返回 `WSL_INSTALL_BUSY: another WSL core install is running`。单测
`install_lock_is_exclusive_while_held` 同步覆盖。W3.4 的「运行中不允许安装」策略见 W3 记录。

### R-W2-6 Linux 侧截止时间（中）

**改动文件**：`service/wsl_core/script.rs`（`INSTALL_DSH` 两次 npm 均用 `timeout -k 10 "$2"` 包裹，
`$2` 为每次尝试秒数；`RESOLVE_DSH` 包 `timeout -k 5 60`）、`service/wsl_core/install.rs`
（`INSTALL_ATTEMPT_SECS = 1200`、`INSTALL_TIMEOUT = 2 * 1200 + 60` 秒仅作中继兜底、
`RESOLVE_TIMEOUT = 90` 秒；退出码 124 → `WSL_DSH_INSTALL_TIMEOUT`）。

**实测（临时把 `INSTALL_ATTEMPT_SECS` 改为 5，registry 指向黑洞 `http://10.255.255.1/`）**：

- t≈4 s 的进程树：`bash -lc … timeout -k 10 "$2" npm i -g … bash 0.1.5-rc.2 5`（pid 1703，
  pgid==pid）与 `timeout -k 10 5 npm i -g --no-audit --no-fund --prefer-offline @deepseek-ai/dsh@0.1.5-rc.2`
  （pid 1770）——证明 `timeout` 与位置参数确实生效。
- 返回：`WSL_DSH_INSTALL_TIMEOUT: npm exceeded 5s in Ubuntu`（15409 ms = 2 × 5 s + `-k` 缓冲 + WSL 开销）。
- t≈10 s：WSL 内 `ps … | grep -E "npm|timeout -k"` → **(none)**，无孤儿进程。
- 已装目录未被破坏（`dsh --version` 仍 `0.1.5-rc.1`）；随后常量已改回 1200，registry 已删除。

### R-W2-7 `STOP` 按进程组结束（低，W3 前）

**改动文件**：`service/wsl_core/script.rs` 的 `STOP`：

```sh
p=$(cat "$1/.harness.pid" 2>/dev/null); if [ -n "$p" ]; then kill -TERM -- "-$p" 2>/dev/null || kill -TERM "$p" 2>/dev/null; sleep 1; kill -KILL -- "-$p" 2>/dev/null; kill -KILL "$p" 2>/dev/null; fi; rm -f "$1/.harness.pid"; true
```

（`exec dsh` 后 pid 文件里的 `$$` 即进程组长，负 PID 一次带走子进程；无独立进程组时回退单 pid；
以 `true` 结尾保证幂等不报错。）单测 `install_uses_bounded_attempts_and_stop_kills_process_group`
断言 `-- "-$p"` 存在且仍以 `true` 结尾。

### R-W2-8 Node 主版本下限（低）

**改动文件**：`service/wsl_core/install.rs`（`MIN_NODE_MAJOR = 20`、`node_major()` 解析 `v` 前缀、
`ensure` 在 probe 后校验 → 无 node `WSL_NODE_MISSING`、主版本 < 20 `WSL_NODE_TOO_OLD`）。

单测 `node_major_parses_v_prefixed_versions` 覆盖 `v22.22.1` → 22、非法输入 → None。
实测环境 `nodeVersion:"v22.22.1"` 通过校验。

### R-W2-9 `wsl.exe` 自身错误进入 `Err` 文本（低）

**改动文件**：`service/wsl_core/exec.rs`（新增 `decode_auto` / `summarize`，`list_distros` 的
`WSL_LIST_FAILED` 追加 `summarize`）。

`decode_auto`：输出含 NUL 字节 → 按 UTF-16LE 解码（`wsl.exe` 自身错误是 UTF-16 且可能写在
stdout），否则 UTF-8 lossy（`wsl.exe -e` 的 Linux 侧输出）。`summarize`：stderr 优先、首个非空
行、截断 160 字符、无内容回退 `"no output"`。单测 `decode_auto_switches_on_nul_bytes` /
`summarize_prefers_stderr_first_non_empty_line`。

### R-W2-10 发行版列表不依赖英文表头（低）

**改动文件**：`service/wsl_core/exec.rs` 的 `parse_distro_list`。

改为**按列语义过滤**：`VERSION` 列不是 `"1" | "2"` 的行一律跳过（表头无论语言都不满足），
并过滤 `docker-desktop*`；移除原先的 `name.eq_ignore_ascii_case("NAME")`。单测
`parse_distro_list_skips_non_english_header_rows` 用 `NOM  ÉTAT  VERSION` 样本验证。

### R-W2-11 三个 bridge 命令的入参校验（低）

**改动文件**：`service/wsl_core/mod.rs`（`validate_distro`：trim 后非空、无控制字符；
`validate_version_spec`：精确 semver 或 `[A-Za-z][A-Za-z0-9._-]*` dist-tag）、
`bridge/wsl_core.rs`（`probe_wsl_core` / `install_wsl_core` 入口各校验一次）、
`bridge/config.rs`（`normalize_wsl_distro` 复用 `validate_distro`）。

`file:` / `git+` / URL / range / 空白一律拒绝（错误码 `WSL_DISTRO_INVALID` /
`WSL_VERSION_SPEC_INVALID`）；只允许两种 spec 也保证了 `RESOLVE_DSH` 输出恒为单行。
单测 `validate_distro_trims_and_rejects_control_characters`、
`validate_version_spec_allows_semver_and_dist_tags_only`。

### R-W2-12 补丁 I/O 移出 async 线程 / 失败降级（低）

**改动文件**：`service/wsl_core/install.rs`。`patch::apply` 包 `spawn_blocking`（UNC 是 9P 网络
文件系统，同步 I/O 不得在 async 运行时线程执行）；`apply` 的 `Err` 降级为
`log::warn!("[wsl-core] --skip-auth patch failed (degraded): {e}")` + `false`，由
`skip_auth_ready=false` 与终态文案「WSL 核心已安装（补丁未命中，--skip-auth 不可用）」承担
告知责任，W3 的 `launch_wsl` 据此拒绝启动。

### 门禁复核（「增量干净」口径，W1-REVIEW §2）

| 项 | 结果 |
|---|---|
| `cargo test --locked` | **547 passed / 0 failed**（W2 基线 537 + W2-R 新增 10：exec 3、script 1、probe 1、install 3、mod 2） |
| 改动文件 fmt | `rustfmt --check --config skip_children=true` 对 9 个改动文件退出码 0（本轮修掉 exec/install/version 三处换行差异） |
| clippy 改动文件命中 | 8 条全部为**上游既有**：`setting.rs:394/401`、`builder.rs:418/493/673/718`、`version.rs:430`（= W2 记录的 `version.rs:423`，W2-R 插行后行号位移；对应上游 377-381 的 `Ok(list(...)?)` 结构，未在本轮 diff hunk 内）。W2-R 新增代码零告警（`script.rs:21` 的 `constant ENV_PREAMBLE is never used` 已加 `#[allow(dead_code)]` + 注释豁免，复跑消失） |
| `pnpm run typecheck` | 通过（`tsc --noEmit` 退出码 0） |
| `pnpm run lint` | 0 errors / 23 warnings（全部上游既有） |

### W2-R 验收实测（`pnpm tauri dev` + CDP，2026-09-12）

- 见上文 R-W2-1 / R-W2-2 / R-W2-5 / R-W2-6 各条的证据表；store 在实测前备份为
  `%APPDATA%\io.github.hairyf.deepseek-harness-desktop\.store.dev.dat.bak-w2r-20260912`。
- 断网验收用的 WSL `~/.npmrc` registry 改动已删除（`npm config delete registry`，验后
  `~/.npmrc` 不存在，`npm config get registry` 回默认 `https://registry.npmjs.org/`）。
- WSL 内 dsh 版本经「rc.2 → rc.1」往返后回到基线 `0.1.5-rc.1`（两条路径都验证了精确版本安装）。

### 环境事件记录（W2-R 期间，非代码问题）

- dev 应用二次启动时 Windows 侧 Harness 因 `dsh-tauri-panel` / `dsh-tauri-panel-extension` /
  `dsh-tauri-worktree` 等包的 `dist/index.js` 缺失而 boot 失败（`ERR_MODULE_NOT_FOUND`）。
  根因是本机 `packages/*` 构建产物缺失（`pnpm tauri dev` 不构建插件包），用
  `pnpm -r --filter "./packages/*" run build` 补齐后恢复正常；与应用代码无关，记录备查。
- 人工 `TaskStop` 终止 dev 时未走应用退出路径（`stop_on_exit` 不执行），Windows 侧可能残留
  Harness 孤儿进程；本次由 `restart_harness` / 下次启动的 `.harness.pid` 回收覆盖。

---

## W3 记录（启动 / 停止 / 就绪判定，2026-09-12）

### 交付物

| 文件 | 内容 |
|---|---|
| `service/workflow/wsl_launch.rs`（新增，219 行） | `launch_wsl` / `wsl_start_args`（纯函数）/ `wsl_dsh_home` / `stop_in_distro` / `STOP_TIMEOUT`；3 个单测 |
| `service/workflow/launch.rs` | `resolve_port` 从 `launch` 抽出（Windows / WSL 共用）；`start()` / `launch()` 的 WSL 分支 |
| `service/workflow/process.rs` | `stop_wsl_harness`；`terminate_owned_process(&app_handle)` 前置 STOP；`stop_on_exit` 同理；`terminate_stale_harness_processes` 在 Wsl 时改跑 STOP（debug 也执行） |
| `bridge/lifecycle.rs` | `runtime_ready` 改 async + Wsl 分支（`cached_or_probe`）；`install_dependencies` 的 Wsl 分支；`check_dsh_update` 返回 `Ok(None)` |
| `bridge/plugin.rs` | `get_preinstall_pending` 在 Wsl 时返回 false |
| `service/plugin/internal/mod.rs` | `ensure` 在 Wsl 时 `Ok(())` + `internal plugins are not injected into the WSL core profile` |
| `service/plugin/install/{mod,single}.rs` | `CORE_WSL_PLUGIN_UNSUPPORTED`（在 `dsh_bin.exists()` 之前） |
| `service/core/source.rs` | 移除 `is_wsl_active` 的 `#[allow(dead_code)]`（W3 已接入全部调用方） |
| `service/wsl_core/script.rs` | `START` / `STOP` 的数据目录改为脚本内 `"$HOME/$1"`（D-W3-2） |

### 决策

#### D-W3-1 `STOP` 中继超时 3 s → 15 s

方案 W3.3 写「跑 STOP（阻塞 ≤3s）」，但本机实测：发行版处于 Stopped 时首次
`wsl.exe -e` 访问要先冷启动 VM，3 s 会在**每次冷启动的启动路径上**先到期——
首轮实测日志 `WSL STOP failed in Ubuntu: WSL_TIMEOUT: … exceeded 3s`，
`terminate_stale_harness_processes` 的清扫静默失效。改为 15 s（Linux 侧 STOP 本身
是秒级的：`kill -TERM` + `sleep 1` + `-KILL`），冷启动与热路径都覆盖。

#### D-W3-2 WSL 数据目录经位置参数传「相对 `$HOME` 的裸目录名」

**现象（首轮验收实测）**：`START` / `STOP` 的 `$1` 原为字面量 `$HOME/.dsh-desktop.dev`
（方案 W3.1 的 `dsh_home = "$HOME/.dsh-desktop"`）。位置参数在 Linux 侧**不做二次
展开**，于是 `mkdir -p "$DSH_HOME"` 创建出 `/home/pixel/$HOME/.dsh-desktop.dev`
（名为 `$HOME` 的目录），dsh 的 profile/storages 全部落在那里；`STOP` 与 `probe`
按 `$HOME` 展开后的真实路径找不到 `.harness.pid` → 停止闭环失效。

**返修**：位置参数只传目录名（`.dsh-desktop.dev`），脚本内 `d="$HOME/$1"` 拼路径
（`START` / `STOP` 各一处）；`wsl_dsh_home()` 返回值随之改为 `&'static str`。
单测断言目录名不含 `$` / `/` / 反斜杠，且 `START` 不再出现 `DSH_HOME="$1"`。
错位目录 `~/` + 字面 `$HOME` 已在验收后清理。

#### D-W3-3 WSL 分支的 `has_owned_process` 必须早于 `resolve_port`

**现象（验收 3 期间实测）**：前端「重试」在服务已运行时调用 `launch_harness`，
WSL 分支先跑 `resolve_port`——它把**本进程正在监听的端口**判为占用并递增
（3083 → 3084），随后 `launch_wsl` 又因 `has_owned_process()` 为真直接返回；
前端于是按 3084 做健康检查，永远失败（`HARNESS_BOOT_MANIFEST_REQUEST_FAILED`）。
Windows 分支的同等检查本来就在端口处理之前。

**返修**：WSL 分支在 `resolve_port` 之前加 `has_owned_process()` 早退。

#### D-W3-4 运行中的 WSL 核心不允许 `install_dependencies` 触发安装（R-W2-5 二选一）

审查端允许「先 stop 再装」或「返回 `Err(WSL_INSTALL_BUSY)`」。执行端选择**第三条、
更贴合 boot 时序**的折中：

- 服务**运行中**且探测显示已就绪（`dsh.is_some() && skip_auth_ready`）→ `Ok(false)`
  （no-op：boot 阶段 `launch` 已经在跑，这里的自愈调用本就不该做任何事）；
- 服务**运行中**但确实需要安装 → `Err("WSL_INSTALL_BUSY: stop the WSL core service
  before installing")`（不擅自停用户的运行中服务）；
- 服务未运行 → `wsl_core::install::ensure`（幂等，秒级），成功补记 `installed = true`，
  返回值以「版本是否变化」判定是否发生安装/更新。

**理由**：`install_dependencies` 是 boot 自愈路径而非「更新」按钮（后者走 W5 面板的
`install_wsl_core`，本身有 `WSL_INSTALL_LOCK` 与运行中判定）。纯 Err 会让
`runtime_ready=false` 的 boot 直接失败页。

#### D-W3-5 验收用 DeepSeek 凭据：Windows → WSL 单向合并（用户授权）

验收 3 需要 WSL 内的 dsh 有模型凭据（UI 报 `MISSING_CREDENTIAL`）。经用户明确同意，
把 Windows 侧 `~/.dsh/.credentials.yaml` 的 `refs.DEEPSEEK_API_KEY` 合并进 WSL 侧
`~/.dsh-desktop.dev/.credentials.yaml`（保留原有 `records.client-connection/
browser-session`，写入后 `chmod 600`）。**只做这一处单向复制，未改动 Windows 侧文件**；
凭据互通策略（是否自动同步、W5 面板是否提示）留待 W4/W5 决策。

#### D-W3-6 核心切换后立即重启服务的端口残留（W4.2 边界的实测复现）

从 Windows 核心切到 WSL 核心后立即 `restart_harness`：Windows 侧 `127.0.0.1:3081`
的 TIME_WAIT 转发尚未消失，`is_port_in_use` 从 Windows 侧探测为「空闲」，但 WSL 内
dsh 绑定时报 `EADDRINUSE` → dsh 退出（`exit code 1`）→ 前端失败页。等待数分钟后
仍复现一次，改用 3083 端口即成功；随后（D-W3-3 修复 + 数分钟间隔）3081 亦成功。
按方案 W4.2「不做 WSL 侧端口探测」，本次**不修**，仅记录：W4 需考虑「切换后首次
启动的端口等待窗口」或「EADDRINUSE 早期退出时换端口重试」。

### 门禁复核（「增量干净」口径，W1-REVIEW §2）

| 项 | 结果 |
|---|---|
| `cargo test --locked` | **550 passed / 0 failed**（W2-R 547 + W3 新增 3：`wsl_launch` 的 `start_args_use_exec_form_with_positional_values` / `start_args_place_values_after_the_script` / `dsh_home_is_a_bare_relative_dir_name`） |
| 改动文件 fmt | 我方新增/修改行零 diff（`wsl_launch.rs` 首轮 3 处换行差异已 rustfmt 整理）；剩余 8 处差异经逐行比对**全部与 `upstream/main` 逐字相同**：`plugin.rs`、`internal/mod.rs`、`install/mod.rs` 的上游未触碰行 |
| clippy 改动文件命中 | 首轮 18 条：其中 `process.rs:214`（`needless_borrow`：`wsl_dsh_home()` 已返回 `&'static str`，`&home` 是多余借用）**为本阶段新增，当场已修**；其余 17 条逐条用「命中行及上下文在上游逐字存在」证明为**上游既有**（`setting.rs:394/401`、`builder.rs:418/493/673/718`、`version.rs:430`、`install/mod.rs:9/10/16/393/459`、`internal/mod.rs:112/221`、`process.rs:331/336/355`）。修复后复跑：**改动文件命中 17 条（全部上游既有），本阶段新增代码零告警** |
| `pnpm run typecheck` | 通过（`tsc --noEmit` 退出码 0） |
| `pnpm run lint` | 0 errors / 23 warnings（全部上游既有） |

### W3 验收实测（`pnpm tauri dev` + CDP，2026-09-12）

1. **设置 `active_core="wsl"`、`wsl_distro="Ubuntu"` 重启服务** ✔
2. **日志出现 wsl.exe 拉起 + Harness process started，前端进入 dsh 界面** ✔
   - `Starting WSL Harness service` → `Harness process started successfully in WSL: pid=37708, port=3081`
   - `internal plugins are not injected into the WSL core profile`、`Suppressing dsh update check because the WSL core is active`（W3.6 生效）
   - WSL 内：`.harness.pid` = 2219、`node /home/pixel/.npm-global/bin/dsh --profile web --port 3081 --no-open --skip-auth`；
     前端 `health check passed: healthy - 54/54 client modules ready`
3. **在 dsh 内新建工作区 `/home/pixel`，模型执行 `pwd && uname -a`** ✔
   - 工作区对话框列出的是 Linux `$HOME` 内容（camoufox/Coding/Downloads/go/snap），选定「主目录」→ 工作区名 `pixel`
   - 模型输出（原文）：`/home/pixel` + `Linux LAPTOP-QMU9QT6R 6.18.33.2-microsoft-standard-WSL2 #1 SMP PREEMPT_DYNAMIC Thu Jun 18 21:54:43 UTC 2026 x86_64 GNU/Linux`；
     会话显示「1 次工具调用」「Exit code was 0. Working directory is /home/pixel」（22.8K tok / 6 秒）
4. **应用内「重启服务」：旧 Linux 进程被 kill 后新进程起来，端口不漂移** ✔
   - `Stopping Harness service...` → `Stopped WSL Harness processes in Ubuntu` →
     `Harness port healed from 3082 back to 3081 (no longer occupied)` → 新 pid 起来；
     重启前后 WSL 内 pid 从 2028 换成 2586（旧的被进程组 kill）
5. **退出应用：`pgrep -fa "dsh --profile"` 为空** ✔
   - 用 `taskkill /PID`（不带 `/F`，走 `close_action=quit` 正常退出路径）触发 `RunEvent::Exit`
     → `Stopped WSL Harness processes in Ubuntu`；退出后 `pidfile=none`、`(no dsh)`，无残留（F5 闭环）
6. **切回 `active_core="app"` 重启：Windows 核心正常，无残留** ✔
   - `Harness process started successfully: pid=31680, port=3081`（Windows 分支文案）+
     `health check passed: healthy - 59/59 client modules ready`；WSL 内 `(no dsh in WSL)`

### 环境事件记录（W3 期间，非代码问题）

- 首轮 WSL 启动时 `~/.dsh-desktop.dev` 被写成字面 `$HOME` 目录（D-W3-2），验收后已
  `rm -rf` 清理；期间 dsh 的 profile 数据全部落在错位目录，清理前已确认其中只有本次
  验收产生的 profile/storages/凭据副本。
- 核心切换 + 立即重启引发的 `EADDRINUSE`（D-W3-6）在验收期间出现两次，均以换端口（3083）
  或等待后重试（3081）解决；`manual_port` 临时改为 3083 已还原为 `None`、`port` 还原为 3081。
- 验收用的 `close_action` 曾临时改为 `quit`（用于触发正常退出路径），验后已还原 `tray`；
  dev store 备份 `.store.dev.dat.bak-w3-20260912`。
- 验收 3 的凭据复制（D-W3-5）经用户明确授权后执行，未打印 key 内容。

## W3-R 返修记录（按 `E:\DSH-WSL\WSL-CORE-REVIEW.md` §C 的 R-W3-1 ~ R-W3-9，2026-09-13）

按 §C.4 顺序执行：步骤 1–7（代码 + 门禁）→ 步骤 8（`pnpm tauri dev` 手动验收，store 先备份、验后还原）→ 本记录（步骤 9）。
范围说明：**R-W3-2 按审核端裁定为 W4 首项**、**R-W3-9 留待 W6.4**（推送到 fork 后运行三平台 CI），二者不在本记录内。

### 交付物（W3-R 改动文件）

| 文件 | 内容 |
|---|---|
| `service/workflow/process.rs` | `WslTarget { distro, dsh_home }`；`OwnedProcess` 去 `Copy` + `wsl: Option<WslTarget>`；`set_owned_wsl_process_with_handle`；`terminate_owned_process()` 去掉 `app_handle`、改按登记表 STOP；`stop_wsl_harness` 降级为仅 `terminate_stale_harness_processes` 使用；新增单测 `wsl_target_survives_take` |
| `service/workflow/wsl_launch.rs` | R-W3-6 / R-W3-7：拆 `preflight(distro)`（锁外探测 + `skip_auth_ready` 判定）与 `spawn(app, distro, port)`（锁内拉起、登记 `WslTarget`）；`PREFLIGHT_MAX_AGE = 15 s` |
| `service/wsl_core/probe.rs` | 缓存值改 `(WslCoreProbe, Instant)`；新增 `cached_fresh(distro, max_age)`；单测 `cached_fresh_respects_max_age` |
| `service/wsl_core/install.rs` | `ensure` 内置运行中判定（`needs_install` 且 running → `WSL_INSTALL_BUSY`）；补丁决策前**重新采样** running（D-W3-7）；经 rustfmt 整理 |
| `service/wsl_core/script.rs` | `START` 写 pid + `boot_id` 两行；`STOP` 三重确认（boot_id 相符 + `/proc/<pid>/cmdline` 含 `dsh --profile`）才 kill；单测补 5 条断言 |
| `service/workflow/launch.rs` | WSL 分支顺序：preflight → 取转换锁 → `has_owned_process` → `resolve_port` → `spawn` |
| `desktop/builder.rs` | 开机清扫（sweep → migrate → heal_stale_pnpm_metadata → ensure_first_run_desktop_profile）与 `auto_start` 移入 `tauri::async_runtime::spawn` 内的 `spawn_blocking`；`scheduler::start` 仍在主线程且在其之前 |
| `lib.rs` | `RunEvent::Exit` 条件改为 `setting.installed \|\| workflow::has_owned_process()` |
| `bridge/config.rs` | `update_app_config`：WSL 服务运行中且改 `wsl_distro` → `WSL_DISTRO_LOCKED` |
| `bridge/lifecycle.rs` | `install_dependencies` 的 WSL 分支精简：运行中且已就绪 → `Ok(false)`，其余交给 `ensure`（删掉重复 `Err` 分支） |

### 逐条记录

#### R-W3-1（高）停止路径按「已拉起的进程」而不是「当前设置」

**实现**（§C.4 步骤 1–2）：`WslTarget` 随 `OwnedProcess` 登记；`terminate_owned_process()` 先 `take_owned_process()`，仅当 `owned.wsl == Some(t)` 时在 `t.distro` / `t.dsh_home` 上跑 STOP，不再读 store（顺带完成 R-W3-8：`app_handle` 参数删除）；`stop_wsl_harness`（设置驱动）只留给 `terminate_stale_harness_processes`；`stop()` / `stop_on_exit` 回到 `spawn_blocking(terminate_owned_process)`；`lib.rs` 退出条件加 `has_owned_process()`；`update_app_config` 加 `WSL_DISTRO_LOCKED` 闸门。

**验收实测**（`pnpm tauri dev` + CDP）：

| 场景 | 结果 |
|---|---|
| 服务运行中 `update_app_config({wslDistro:''})` | 被拒 `WSL_DISTRO_LOCKED: stop the WSL core service before changing the distro` |
| 服务运行中 `update_app_config({wslDistro:'Debian'})` | 同上（新值 ≠ 旧值即拒，含清空与改名两种） |
| `shutdown_harness` 后上述两次调用 | 成功（可清空、可设回 `Ubuntu`）；WSL 内 `pgrep -fa "dsh --profile"` 为空（无孤儿） |
| 运行中 `restart_harness` | 日志 `Stopped WSL Harness processes` **delta = 1**（不再出现「无持有进程时」的空跑 STOP） |
| store `installed=false` + 服务运行中 → 正常退出应用 | `Stopped WSL Harness processes in Ubuntu` 出现，退出后 `pgrep` 为空（退出条件里的 `has_owned_process()` 生效，F5 闭环） |

#### R-W3-3（中）`ensure` 内置运行中判定

**实现**：`ensure` 紧接 `install_lock` 后采样 `running = has_owned_process() && is_wsl_active(app)`；`needs_install` 且 running → `Err(WSL_INSTALL_BUSY: stop the WSL core service before installing)`；补丁在 running 时只探测不改文件（`skip_auth_ready` 由 PROBE 只读判定给出）。`install_dependencies` 保留「运行中且已就绪 → `Ok(false)`」快路径。

**验收实测**：

- 服务运行中 `install_wsl_core({distro:'Ubuntu',versionSpec:'0.1.5-rc.2'})` → **482 ms** 返回 `WSL_INSTALL_BUSY: stop the WSL core service before installing`；WSL 内 `dsh --version` 仍为 `0.1.5-rc.1`（安装目录未被改写）。
- `shutdown_harness` 后同一调用 → 11.65 s 完成，`0.1.5-rc.1 → 0.1.5-rc.2`；随后装回 `0.1.5-rc.1`（11.6 s），基线还原。

#### R-W3-4（中）开机清扫移出主线程

**实现**：`builder.rs::setup` 把 sweep → migrate → heal → ensure_first_run 四步与 `auto_start` 的 `start()` 一起放进 `tauri::async_runtime::spawn`（同步 I/O 再套 `spawn_blocking`），`scheduler::start` 保持主线程且在任务创建之前。

**验收实测**：`wsl --shutdown` 后冷启动 dev 应用（发行版 Stopped，首次访问要冷启动 VM）：`04:50:50.913 Starting dsh process monitor`（主线程继续）→ `04:50:56.268 Stopped WSL Harness processes in Ubuntu`（异步任务内完成）。窗口立即可交互，无白窗冻结。

> **已由 R-W4-1 取代**（2026-09-20，见下方 W4-R 记录）：把 migrate / heal / 首装档案一并挪进后台任务破坏了上游「事件循环启动前完成」的顺序保证——前端 boot 的 `launch_harness` 会与数据迁移并发。现行做法是 `builder.rs::setup` 回到上游同步形态，改为 **WSL 核心下 `sweep_orphan_harness` 跳过发行版内 STOP**（开机不再有 `wsl.exe` 调用，同样没有主线程冻结）。

#### R-W3-5（中）`STOP` 三重确认（pid 复用防护）

**实现**：`START` 写两行（`$$` + `cat /proc/sys/kernel/random/boot_id`）；`STOP` 读两行，仅当 boot_id 相符且 `/proc/<pid>/cmdline` 含 `dsh --profile` 时才 kill（`kill -TERM -- -pid` → sleep 1 → `-KILL`），无论结果如何都 `rm -f` pid 文件。

**验收实测**：把 `.harness.pid` 伪造成 `2935` + `stale-boot-id-0000`（2935 是当时真实存活的 `sleep 300`）→ `launch_harness` 后 `sleep 300` **仍在运行**（未被误杀）；pid 文件被重写为新 dsh 的 pid（`3124`）+ 真实 boot_id（`5a59c033-3527-404c-8d36-feba8817b19f`），新 dsh 正常起来。

#### R-W3-6（低）`cached_fresh` 消除开机重复探测

**实现**：`probe.rs` 缓存值带写入时刻；preflight 用 `cached_fresh(distro, 15 s)`，命中即复用。`cached()` 语义不变。

#### R-W3-7（低）探测移出核心转换锁

**实现**：`launch_wsl` 拆成 `preflight`（锁外）与 `spawn`（锁内）；最坏持锁时长从「探测 + STOP + spawn」回落到与 Windows 分支同量级。

#### R-W3-8（低）去掉 `terminate_owned_process` 的 `app_handle`

**实现**：随 R-W3-1 一并完成（目标随登记表走），非 Windows 编译不再有 `unused_variables`。

#### R-W3-9（低，W6 前）非 Windows 三平台编译验证

不在本轮：W6 按方案 W6.4 配好当前开发分支的 push 触发，完成本地提交并推送到 fork `Sukvii/deepseek-harness-desktop` 的 `origin/feat/wsl-core`，核对该次推送的三平台 CI；已同步进方案 W6.4。

#### D-W3-7（本轮验收新增裁决）`ensure` 的补丁决策前重新采样运行中判定

**现象（W3-R 手动验收实测，`installed=false` 场景开机日志）**：`install_dependencies`（自愈）与 `auto_start` 的 `start()` 并发——

```
04:58:44.806 Starting WSL Harness service
04:58:45.681 Harness process started successfully in WSL: pid=59000
04:58:47.596 [wsl-core] --skip-auth patch for Ubuntu: startup=true, connection=true
```

`ensure` 在服务登记前采样 `running=false`，其后的两次探测（各 2–7 s）期间 dsh 已被拉起，补丁仍在 47.596 写入正在启动的 `startup.js` / `index.js`。本次因版本未变、补丁内容与现状一致而无害（健康检查 54/54 通过），但存在「node 读取到写了一半的文件 → ESM 解析失败」的窄窗口。

**返修**：补丁决策处重新采样 `running`（顶部采样只用于安装闸门）；修复后同一场景下补丁写入被跳过、`skip_auth_ready` 取 PROBE 只读判定。

### 门禁复核（「增量干净」口径，§0 v2）

| 项 | 结果 |
|---|---|
| 命令 1 `cargo test --locked` | **552 passed / 0 failed**（W3 的 550 + 2 新增：`wsl_target_survives_take`、`cached_fresh_respects_max_age`） |
| 命令 2（v2）fmt | 首次报 `FMT REGRESSION: src/service/wsl_core/install.rs (upstream=1 now=2)`——**两处真实差异**（R-W3-3 的闸门行 1 处 + D-W3-7 的新增行 1 处），即 rustfmt 想写成 `let running =\n        …has_owned_process() && …is_wsl_active(app);`；已 rustfmt 整理，复跑 **无 REGRESSION** |
| 命令 3a clippy（改动文件） | **20 条命中全部为上游既有行**（`setting.rs:393/394/400/401`、`builder.rs:422/497/679×2/724`、`version.rs:430`、`install/mod.rs:9/10/16/393/459`、`internal/mod.rs:112/221`、`process.rs:375/380/399`）；新增文件（`wsl_launch.rs`、`wsl_core/*`）**零命中** |
| 命令 3b clippy 全量 | `^warning` 计数 **81 = 基线 81** |
| 命令 4 | `pnpm run typecheck` 通过；`npx eslint src/hooks/use-dsh-cores.ts src/components/config-core.tsx` 0 errors（前端自 W2-R 后未再改动） |

**门禁脚本观察（供审核端）**：命令 2 的 v2 脚本对**新文件**给出的上游基线是 1 而不是 0——`git show` 对不存在的路径返回空，而 `rustfmt --check` 对空 stdin 也会报 1 条 `Diff in`（本机 rustfmt 1.98.1 实测）。后果：新文件里恰好 1 处未格式化会漏过门禁（本轮 install.rs 的 R-W3-3 行正是如此，直到 D-W3-7 变成 2 才报出）。是否修正交由审核端；本轮维持 v2 脚本不动，仅把 install.rs 整理为 0。

### 环境事件记录（W3-R 期间，非代码问题）

- 验收期间 dev store 多次改动（`active_core` / `wsl_distro` / `installed` / `close_action` / `port`），每次先备份（`.store.dev.dat.bak-w3r-*`），结束后还原为基线（`active_core=null`、`wsl_distro=null`、`port=3081`、`manual_port=null`、`close_action=tray`、`installed=true`）。
- WSL 内 dsh 版本在 R-W3-3 验收中临时升至 `0.1.5-rc.2`，验后装回 `0.1.5-rc.1`。
- 验收用伪造 pid 文件与测试用 `sleep 300` 均已清理；`.harness.pid` 已删除。
- 应用退出后 Windows 侧曾留有一条 `127.0.0.1:3081` 的 TIME_WAIT（D-W3-6 / F12 同源现象），约 120 s 自行消失，未影响后续验收。

## W4 记录（边界与降级，2026-09-13）

W4 首项即 R-W3-2（端口探测改到 Linux 侧）；W4.1 三条边界错误路径与 W4.3 两种网络模式均做了真机实测。

### 交付物

| 文件 | 内容 |
|---|---|
| `service/wsl_core/script.rs` | 新增 `PORT_SCAN`（Linux 侧 `node net.createServer` 逐个 bind；`$1` = 起始端口、`$2` = 最多尝试数；输出第一个可绑定端口，用尽以 **98** 退出）；加入既有两条通用脚本测试的 `SCRIPTS` 表（现 6 条） |
| `service/workflow/wsl_launch.rs` | 新增 `resolve_port_wsl`（heal 判定 → 从当前值起扫描 → 变更即持久化并打与 Windows 分支相同的两条日志）、`scan_port`、`parse_port_scan_output`、`PORT_SCAN_TIMEOUT`（15 s）/ `PORT_SCAN_ATTEMPTS`（10）；新增 2 个单测（`parse_port_scan_output_accepts_single_port_line` / `..._rejects_empty_and_non_numeric`） |
| `service/workflow/launch.rs` | `resolve_heal_port` 提为 `pub(super)` 并补文档（Windows / WSL 共用）；WSL 分支的端口处理改调 `resolve_port_wsl`（不再用 Windows 侧 `is_port_in_use` / `wait_for_port_release`） |

### W4.1 三条边界错误路径（实测）

| 场景 | 触发方式 | 结果（错误码原文） |
|---|---|---|
| 发行版不存在 | `update_app_config({wslDistro:'NoSuchDistro'})`（格式合法，存在性只在运行时暴露）→ `launch_harness` | `WSL_PROBE_FAILED: PROBE exited with -1 in NoSuchDistro: 不存在具有所提供名称的分发。` |
| 发行版启动异常 | `wsl --import` 一个「有 rootfs 形状、无可用 bash」的临时发行版 `DSH-W4-BrokenTest` → `launch_harness` | `WSL_PROBE_FAILED: PROBE exited with 1 in DSH-W4-BrokenTest: wsl: Processing /etc/fstab with mount -a failed.` |
| `wsl.exe` 不可用 | 在应用目录（`src-tauri/target/debug`）放一个非 PE 的 `wsl.exe`，让 CreateProcess 失败 → `probe_wsl_core` / `launch_harness` | `WSL_EXEC_FAILED: wsl.exe -d Ubuntu -e bash -lc …: … (os error 216)`（真机 wsl.exe 缺失时同一路径，os error 2） |

三条都带 `WSL_` 前缀；前端错误页原样展示（`document.body.innerText` 实测：`启动失败 WSL_EXEC_FAILED: … › 重试 复制日志 安全模式`），**前端无需改动**（复用 `attachStartupDiagnostics`，方案 W4.1）。

### W4.2 Linux 侧端口探测（R-W3-2）

**（1）机制实证**（本机 2026-09-13，Windows 侧服务端 TIME_WAIT）：

| 状态 | Windows `bind(127.0.0.1:3085)` | Linux `PORT_SCAN 3085 2` |
|---|---|---|
| Python 服务端在 3085 主动关闭，`netstat` 显示 `127.0.0.1:3085 → 127.0.0.1:60158 TIME_WAIT` | **OK（报空闲）** | 输出 **3086**（3085 不可绑定） |

命令级对照（把 `script.rs` 的 `PORT_SCAN` 文本原样提取后在 WSL 内执行）：3081 空闲 → `3081`（退出 0）；3081 被占 → `3082`；3081+3082 被占且 `attempts=2` → 无输出、**退出 98**。冷启动 VM 的开销由 15 s 中继超时覆盖（同 `STOP_TIMEOUT`）。

**（2）端到端验收**（`pnpm tauri dev` + CDP；按前端真实时序：`set_active_core('wsl')` 后**立即** `launch_harness`）：

- Windows 核心运行在 3081（iframe 已连接）→ 制造服务端 TIME_WAIT（12 条）→ 切换并立即启动：
  `Harness port changed from 3081 to 3082 because the configured port is occupied` →
  `Harness process started successfully in WSL: pid=19824, port=3082`，**一次成功**（无 `EADDRINUSE`、无失败页）；store `port=3082` 已持久化；WSL 内 `dsh --profile web --port 3082`；前端刷新后 `54/54 client modules`。
- 约 2 分钟后（120 s TIME_WAIT 窗口过去）`restart_harness`：
  `Harness port healed from 3082 back to 3081 (no longer occupied)` → 新 `dsh --port 3081`，store 回落 **3081**。
- 附带证据：NAT → Mirrored 切换后的首次启动同样正确避让（3081 被 NAT 转发残留占用 → 落 3082，HTTP 200）；随后应用重建自启动时 heal 回 3081（`store port=3081`，健康 54/54）。

### W4.3 两种网络模式的 `127.0.0.1:<port>` 可达性（实测）

| 模式 | 操作 | 结果 |
|---|---|---|
| Mirrored（`.wslconfig` 原值） | WSL 核心启动后，Windows 侧 Python `urllib` 请求 `http://127.0.0.1:3081/` | **HTTP 200**；前端健康检查 54/54 |
| NAT | 备份 `.wslconfig` → `networkingMode=NAT` → `wsl --shutdown` → 重新启动 WSL 核心 | **HTTP 200**（3081）；前端健康检查 54/54 |
| 还原 | `cp` 回备份 + `wsl --shutdown`，再启动验证 | Mirrored 下 **HTTP 200**（3082，因残留避让，随后 heal 回 3081） |

`.wslconfig` 已还原为 `networkingMode=Mirrored`，备份文件已删除。

### W4.4 / W4.5（留 W5，本轮无代码）

- 数据目录独立（WSL `~/.dsh-desktop[.dev]` vs Windows `~/.dsh`）的「会话与插件不互通」文案：W5 设置页。
- 内置插件不注入 WSL 档案的只读提示：W5 面板（W3 已落三处 `CORE_WSL_PLUGIN_UNSUPPORTED` 闸门与 `internal plugins are not injected` 日志）。

### 门禁复核（「增量干净」口径，§0 v2）

| 项 | 结果 |
|---|---|
| 命令 1 `cargo test --locked` | **554 passed / 0 failed**（W3-R 的 552 + 2：`parse_port_scan_output_accepts_single_port_line`、`parse_port_scan_output_rejects_empty_and_non_numeric`） |
| 命令 2（v2）fmt | 首轮报 `FMT REGRESSION: src/service/workflow/wsl_launch.rs (upstream=1 now=2)`——**两处真实差异**：W3 遗留的 import 排序 1 处（正是 W3-R 记录里「新文件基线 u=1 会各漏 1 处」的那类）+ 本阶段新增行 1 处；已 rustfmt 整理，复跑 **无 REGRESSION**（全部改动文件 now = 0） |
| 命令 3a clippy（改动文件） | **20 条命中与 W3-R 完全一致，全部为上游既有行**（`setting.rs:393/394/400/401`、`builder.rs:422/497/679×2/724`、`version.rs:430`、`install/mod.rs:9/10/16/393/459`、`internal/mod.rs:112/221`、`process.rs:375/380/399`）；`wsl_launch.rs` / `wsl_core/*` **零命中** |
| 命令 3b clippy 全量 | `^warning` 计数 **81 = 基线 81** |
| 命令 4 | `pnpm run typecheck` 通过；`npx eslint src/hooks/use-dsh-cores.ts src/components/config-core.tsx` 0 errors（本阶段未改前端文件） |

### 环境事件记录（W4 期间，非代码问题）

- W4.1 的临时发行版 `DSH-W4-BrokenTest`（`wsl --import` 自建）实测后已 `wsl --unregister` 删除，临时目录 `E:\DSH-WSL\_tmp_w4` 已清理；`wsl -l -v` 仅剩 Ubuntu。
- W4.3 的 `.wslconfig` 改动经用户授权；测试期间 `wsl --shutdown` 两次（切 NAT、还原后），`memory` / `guiApplications` 行未动，还原后文件逐字一致。
- W4.1 的伪造 `wsl.exe` 已删除（清理后 `probe_wsl_core` 立即恢复正常：`node=/usr/bin/node`、`dsh=…/.npm-global/bin/dsh`）。
- 验收期间 dev store 的 `port` 在 3081/3082 间漂移，收尾时已还原为 3081。
- 应用自启动时 `wsl.exe` 冷启动 + NAT 残留会让首启端口短暂落到 3082，属预期避让（由 Linux 侧扫描自动处理），无需人工干预。

## W4-R 返修记录（按 `E:\DSH-WSL\WSL-CORE-REVIEW.md` §D 的 R-W4-1 ~ R-W4-7，2026-09-20）

按 §D.4 顺序执行：步骤 1–4（代码 + 门禁）→ 步骤 5（`pnpm tauri dev` 手动验收，store 先备份、验后还原）→ 本记录（步骤 6）。
范围：R-W4-1 / R-W4-3 / R-W4-2 为必返修三项，R-W4-4 ~ R-W4-7 顺带完成。

### 交付物

| 文件 | 内容 |
|---|---|
| `desktop/builder.rs` | R-W4-1：`.setup()` **回到上游同步形态**——sweep → migrate → heal → 首装档案四步同步执行 → `scheduler::start` → 只含 `start()` 的 `auto_start` 任务；只新增注释说明 WSL 核心下这里没有 `wsl.exe` 调用 |
| `service/workflow/sweep.rs` | R-W4-1：WSL 核心下**跳过** `terminate_stale_harness_processes`（`#[cfg(windows)] let wsl_active = is_wsl_active(app_handle)`，非 Windows 取 `false`）；Windows 侧 `.harness.pid` 的端口/PID 双重确认部分不变 |
| `service/workflow/process.rs` | R-W4-6：`terminate_stale_harness_processes` 的 WSL 分支**去掉 `return`**，STOP 后继续走上游按路径清扫；R-W4-5：`WslTarget` 与 `OwnedProcess.wsl` 加 `#[cfg_attr(not(windows), allow(dead_code))]` |
| `service/workflow/wsl_launch.rs` | R-W4-3：新增 `clear_stale`（STOP 提到端口判定之前；成功打 `Cleared stale WSL Harness processes in {distro}`，失败仅 warn 继续）；`spawn` 删除内部 STOP；R-W4-4：`resolve_port_wsl` 两处改 `update_store_dat_setting(\|s\| s.port = …)` 精确写入 |
| `service/workflow/launch.rs` | R-W4-3 / R-W4-4：WSL 分支顺序 = 锁 → `has_owned_process` 早退 → `clear_stale` → 锁内重读 `wsl_distro`（不一致 → `WSL_DISTRO_CHANGED: distro changed during preflight, retry`）→ `resolve_port_wsl` → `spawn` |
| `service/wsl_core/exec.rs` | R-W4-2：`wsl_exe_path()` 的**定义**移入本模块；`spawn_wsl` 拆 `spawn_wsl_at(exe, args)` + `spawn_wsl(args)`（后者传绝对路径）；错误文本带绝对路径；新增单测 `spawn_wsl_at_reports_missing_executable`，并接收从 `core::source` 搬来的 `wsl_exe_path_is_absolute_and_points_at_system32` |
| `service/core/source.rs` | R-W4-2：`wsl_exe_path` 改为 `pub use crate::service::wsl_core::exec::wsl_exe_path;`（全部调用点不变），对应单测移走 |
| `service/wsl_core/probe.rs` | R-W4-7：`store()` 的 `.insert(...)` 收成一行（v3 门禁报出的唯一差异） |

### 逐条记录

#### R-W4-1（中）启动准备回到上游同步顺序 + WSL 跳过发行版清扫

**实现**：撤销 W3-R 对 `builder.rs` 的改动（四步重新同步、事件循环启动前完成，前端任何 invoke 都不可能与迁移/档案引导并发）；代价——开机 WSL 侧 STOP 被移除，改由「首次 launch 的 `clear_stale`（持核心转换锁、位于端口扫描之前）」+「切核心 / 安装路径的 `terminate_stale_harness_processes`」回收。

**验收实测**（`active_core="wsl"` + `wsl --shutdown` 后冷启动）：

```
04:07:43.376  首条应用日志（.setup() 开始）
04:07:43.692  Starting dsh process monitor          ← 四步同步完成后立即启动监控
04:07:43.696  Starting WSL Harness service          ← auto_start 任务异步登台
04:07:50.513  Cleared stale WSL Harness processes in Ubuntu   ← 第一条 STOP 相关行，来自 launch()
04:07:50.717  Harness process started successfully in WSL: pid=16984, port=3081
04:07:55.627  health check passed: healthy - 54/54 client modules ready
```

- `Starting dsh process monitor` 之前**没有任何 `wsl.exe` 相关运行行**（全文仅 4 条含 "wsl" 的行，都是 cargo/vite 横幅里的路径 `E:\DSH-WSL\dsh-fork`）。
- 启动到监控 316 ms（W3-R 时因同步 STOP 冷启动是 ~5–6 s），窗口无冻结。
- Windows 核心回归：切回 `app` 后 `Harness process started successfully: pid=18992, port=3082` + 前端 `health check passed: healthy - 59/59 client modules ready`。

#### R-W4-3（中）残留 STOP 提到端口扫描之前

**实现**：`launch()` WSL 分支把 STOP 从 `spawn` 内部提到 `resolve_port_wsl` 之前（`clear_stale`）；`spawn` 不再跑 STOP，前置条件注释改为「残留已清、端口已定」。

**验收实测**（模拟崩溃残留：WSL 内手工按 START 形态起 dsh 占 3081，pid 文件写 pid `2308` + 当前 boot_id `3e552c55-…`）：

```
04:08:58.228  Cleared stale WSL Harness processes in Ubuntu
04:08:58.438  Starting Harness process in WSL distro Ubuntu (port 3081)
04:08:58.440  Harness process started successfully in WSL: pid=684, port=3081
```

- **没有** `Harness port changed from 3081 to 3082`（对比 R-W4-3 之前的顺序会白白漂移一次）；store `port=3081` 不变；残留 dsh（2308）被清掉，新 dsh 接手 3081。

#### R-W4-2（中）`wsl.exe` 一律经绝对路径解析

**实现**：`spawn_wsl_at(exe, args)` 收口 `Command::new(exe)`；`spawn_wsl(arg)` 传 `wsl_exe_path()`；`wsl_exe_path()` 定义移入 `wsl_core::exec`（`core::source` 用 `pub use` 转发，避免反向引用）。

**验收实测**：
- 单测：`spawn_wsl_at(临时目录 + 不存在的文件名)` → `WSL_EXEC_FAILED: …dsh-w4r-missing-wsl.exe --list: … (os error 2)`（用「目录存在、文件缺失」才稳定得到 2；目录不存在时报 3）。
- 运行中：在应用目录 `target/debug` 放一个非 PE 的 `wsl.exe`，`probe_wsl_core` **照常成功**（`node=/usr/bin/node`、`dsh=/home/pixel/.npm-global/bin/dsh`）——证明不再从可执行文件目录解析（R-W4-2 之前同一布置会 `WSL_EXEC_FAILED … os error 216`）。验后已删除伪造文件。

#### R-W4-4（低）精确写 port + 锁内重读 `wsl_distro`

**实现**：`resolve_port_wsl` 的两处持久化改 `update_store_dat_setting(|s| s.port = …)`；`launch()` 取锁后重读设置，`wsl_distro` 与 preflight 用的不同即 `Err(WSL_DISTRO_CHANGED)`。

**验收实测**（WSL 侧先占 3081，使端口走**变更**分支；preflight 期间并发改值）：

```
SET6_OK（preflight 期间 update_app_config({backupRetentionCount: 6})）
04:13:30.872  Cleared stale WSL Harness processes in Ubuntu
04:13:31.370  Harness port changed from 3081 to 3082 because the configured port is occupied
04:13:31.373  Harness process started successfully in WSL: pid=5900, port=3082
最终 store：port = 3082，manual_port = 3081（未被触碰），backup_retention_count = 6
```

端口写入确实发生，而期间改动的 `backup_retention_count` **存活**（旧的整对象回写会把它退回旧值）。

#### R-W4-5 / R-W4-6 / R-W4-7（低）

- **R-W4-5**：两处 `#[cfg_attr(not(windows), allow(dead_code))]` 已加（本机 Windows 构建看不出差异，留待 W6.4 三平台 CI 日志核对）。
- **R-W4-6**：去掉 `return`（按方案）。代价是 release 下每次切核心 / 安装多一次 ~1 s 的 PowerShell 枚举；debug 走既有 `cfg!(debug_assertions)` 捷径，零成本。
- **R-W4-7**：`probe.rs` 的 `store()` 已 rustfmt 整理；门禁命令 2 换 v3（新文件基线取 0）。

### 门禁复核（§0 v3）

| 项 | 结果 |
|---|---|
| 命令 1 `cargo test --locked` | **555 passed / 0 failed**（W4 的 554 + 1 新增：`spawn_wsl_at_reports_missing_executable`；`wsl_exe_path_is_absolute_and_points_at_system32` 从 `core::source` 迁到 `wsl_core::exec`） |
| 命令 2（**v3**）fmt | 首轮报 `exec.rs` 4 处、`probe.rs` 1 处（都是本阶段新写/未整理的行）；rustfmt 整理后复跑 **无 REGRESSION**。注意：整理时 rustfmt 一度把 4 个**上游本就不干净**的文件（`bridge/plugin.rs`、`patch/alpha_auth.rs`、`plugin/install/mod.rs`、`plugin/internal/mod.rs`）一并改写，已按「最小改动」原则逐处**回退**这些非我方改动（diff 量回到 W4 时的 5 / 8 / 9 / 8 行） |
| 命令 3a clippy（改动文件） | **21 条** = W4 的 20 条 + `sweep.rs:87`（`doc_lazy_continuation`，因 `sweep.rs` 首次进入改动文件集才被计入）；逐条以「命中行内容在 `upstream/main` 同文件中存在」核验，**非上游既有命中 0**；新增文件（`wsl_launch.rs` / `wsl_core/*`）0 命中 |
| 命令 3b clippy 全量 | `^warning` **81 = 基线 81** |
| 命令 4 | `pnpm run typecheck` 通过；eslint 0 问题（本阶段未改前端） |

### 环境事件记录（W4-R 期间，非代码问题）

- **外部直接改 `.store.dev.dat` 对运行中的应用无效**：设置经 tauri-plugin-store 读写，插件持有内存副本（`store.get`/`save`），磁盘上的外部改动既读不到、还会被下一次 `save()` 覆盖。本轮两次验收误判由此而来。结论：验收改设置要么走 `update_app_config`，要么先退出应用再改文件。
- `update_app_config({port: X})` 会**同时**写 `manual_port = X`（「用户手动端口」标记）→ heal 目标 = 当前值，走不到 heal 分支；因此验收端口变更要用「目标端口被占」的变更分支（已在 R-W4-4 采用）。
- 临时物均已清理：WSL 侧 `~/dsh-residue.sh`（模拟崩溃残留用）、3081 的临时监听（`pkill`）、`target/debug/wsl.exe` 伪造件。
- dev store 已还原基线：`active_core=null`、`wsl_distro=null`、`port=3081`、`manual_port=null`、`backup_retention_count=10`、`close_action=tray`；本次验收前的备份 `.store.dev.dat.bak-w4r-20260920`。
- 应用退出后 WSL 内无 dsh、Windows 侧无残留进程；`wsl -l -v` 仅剩 Ubuntu。

## W5 记录（设置页 UI 与 i18n，2026-09-20）

**范围**：方案 v5 §W5 第 1–7 条 + 验收（「全流程只在 UI 内完成 W3 验收第 1–6 条」、切换语言文案完整、`pnpm lint` / `typecheck` 通过）。本轮无审核单，W5 直接按方案执行；验收期另外暴露并修复了两处既有缺陷（W5-A / W5-B，见下）。

### 交付物

| 文件 | 内容 |
|---|---|
| `src/components/config-wsl-core.tsx`（新增） | `ConfigWslCore`：发行版下拉（`list_wsl_distros`，`isDefault` 回落首项）+ `probe_wsl_core` 状态行 + 安装 / 更新 + 「使用此核心」+ 只读提示 + 「从 Windows 导入 API Key」。`WslCoreInstallDialog`：`PanelProgress` 形态的进度对话框，`listen('install-progress')` **只处理 `type === 'wsl-core'`** |
| `src/components/config-dialog.tsx` | `harness` 面板内 `<ConfigCore />` 之后挂 `<ConfigWslCore />` |
| `src/components/config-core.tsx` | W5.6 三处适配：`displayVersion()` 无版本时回落 `version.source`；来源 Chip 增 `wsl`（`core.wsl`）；`!core.present` 时 WSL 行显示 `core.wsl_not_installed` |
| `src/hooks/use-app-config.ts` | `AppConfig.wsl_distro`（`string \| null`） |
| `src/i18n/locales/{zh-CN,en-US}.json` | 324 → **365** 键（+41：`core.wsl` / `core.wsl_not_installed` / `wsl_core.*` 39 个），中英一一对应 |
| `src-tauri/src/service/wsl_core/credentials.rs`（新增） | D-W5-1：`extract_api_key` / `merge_api_key` / `import_api_key`（读 Windows 侧 `.credentials.yaml` → 合并 `refs.DEEPSEEK_API_KEY` → 写 UNC 目标 → `CHMOD_CREDENTIALS`）+ 4 个单测；**不打印密钥内容** |
| `src-tauri/src/service/wsl_core/script.rs` | 新增 `CHMOD_CREDENTIALS`（`chmod 600`，失败 `\|\| true`）；`SCRIPTS` 表 6 → 7 条 |
| `src-tauri/src/service/wsl_core/{patch,mod}.rs` | `wsl_unc_path` 提为 `pub(super)`；挂 `pub mod credentials;` |
| `src-tauri/src/bridge/wsl_core.rs` | 新命令 `import_wsl_credentials`（`validate_distro` + `spawn_blocking`） |
| `src-tauri/src/desktop/builder.rs` | 命令注册表加 `crate::bridge::import_wsl_credentials` |

### 逐条记录

#### W5.1 发行版下拉 + 持久化 + 运行中禁用

下拉挂载即 `list_wsl_distros`；未选发行版时按 `isDefault`（回落首项）一次性写 `update_app_config({ wslDistro })`。运行中 `isDisabled`（含 `serviceRunning`）并显示 `wsl_core.distro_locked`「服务运行中，停止服务后可更换」。

**实测**：服务运行中打开面板 → 下拉 `data-disabled=true`、占位符「未选择发行版」、提示行在位，反复重开面板 `wsl_distro` 保持 `null`（自动选中被 `serviceRunning` 拦住，未产生写入）；停止服务后重开 → 自动选中 `Ubuntu` 且 store 落盘 `wsl_distro="Ubuntu"`。后端 `WSL_DISTRO_LOCKED` 仍是第二道闸（`wsl_distro` 为 `null` 时 `is_wsl_active()` 为 false，本就不拦——即「尚未选择」时写入是允许且期望的）。

#### W5.2 状态行（挂载即探测）

`probe_wsl_core` 在 `distro` 变化时现场探测（进程内缓存重启即空），渲染 Node 版本 / dsh 版本 / 「未安装」/「补丁未命中」Chip 与错误串。**实测**：`Node v22.22.1`、`dsh 0.1.5-rc.2`、按钮文案由「安装 dsh」自动变为「更新 dsh」、无补丁 Chip（`skip_auth_ready=true`）。

#### W5.3 安装 / 更新（确认 → 停服 → 安装 → 重启）

确认对话框按已装与否切换标题/描述；运行中先 `shutdown_harness` → `install_wsl_core` → 结束后（无论成败）恢复 `launch_harness`；进度对话框自行 `listen('install-progress')`，**不使用** `harness.installer` 状态。

**事件类型隔离证据**：`install-progress` 只有两个发射点——`service/wsl_core/install.rs`（`type: "wsl-core"`）与 `service/download/progress.rs`（`type` 由 `start_phase` 传入，取值 `"download"` / `"extract"`，见 `core/version.rs` 与 `workflow/install.rs`）。两侧取值不相交，因此按 `type` 过滤即可各走各的对话框。

**实测（真实安装，非幂等快路径）**：面板显示 `更新 dsh` → 确认 → 进度对话框依次 5% → 15% → 74/76/85%（带 `[stderr] npm ERR! …` 行）→ 关闭；日志随后 `--skip-auth patch for Ubuntu: startup=true, connection=true`、`Harness process started successfully in WSL: pid=28864`、`healthy - 54/54 client modules ready`。**npm ERR 是设计内的首次失败**：`INSTALL_DSH` 先 `npm i -g`（系统前缀无权限）→ 失败后 `echo 'npm global install failed, retrying with user prefix'` → `--prefix "$HOME/.npm-global"` 重试成功（W2 的 D-W2-1 回退）；WSL 侧 npm 调试日志三次运行均 `exit 0`/`info ok`，包版本由 `0.1.5-rc.1` 变为 `0.1.5-rc.2`。

#### W5.4 「使用此核心」

`set_active_core({ id: 'wsl' })` → 复用核心面板的 `store.harness.restart()`，复用 `core.switch_confirm_*` / `core.activate_toast` / `core.switch_restart_hint` 文案。**实测**：确认后 `active_core="wsl"`，日志 `internal plugins are not injected into the WSL core profile`（W3 第 6 条分支）→ `Harness port changed from 3081 to 3082`（Windows 侧 TIME_WAIT 占 3081）→ `Harness process started successfully in WSL: pid=15416, port=3082` → `healthy - 54/54`，按钮转为「使用中」（`isDisabled`）。

#### W5.5 只读提示 + D-W5-1 导入按钮

只读区展示数据目录（UNC，可 `reveal_in_folder`）、内置插件不注入、会话/插件/凭据与 Windows 侧不互通、首次会遇到「缺少凭据」。

**D-W5-1 裁决（采纳审核建议）**：**提供**「从 Windows 导入 API Key」按钮；**单向、显式点击、默认不自动同步**。实现细节：读 Windows 侧 `config::get_dsh_data_path(app)/.credentials.yaml` 的 `refs.DEEPSEEK_API_KEY` → `merge_api_key`（同名键替换；无则插入 `refs:` 块首行；保持其余行逐字不变）→ 写 `\\wsl.localhost\<distro>\<home>\<dsh home>/.credentials.yaml` → `chmod 600`（失败仅 warn，不阻断）；**同值不重写**（返回 `false` → 「已是最新」提示）；密钥内容不进日志。

**实测**：确认框文案中英完整；首次导入后端打 `[wsl-core] imported DEEPSEEK_API_KEY into Ubuntu credentials`，导入后 WSL 侧文件与 Windows 侧**仅** `secret:`（本地既有密钥，与 API Key 无关）不同，`refs.DEEPSEEK_API_KEY` 一致、权限 `-rw-------`（0600）、其余行未动；再次导入无新日志（走 `Ok(false)`），文件 mtime 不变。

#### W5.6 `config-core.tsx` 三处 WSL 适配

**实测**：核心列表出现 `0.1.5-rc.2` + 「WSL」Chip 的行（`core.wsl`），`displayVersion` 与未装文案分支生效。

#### W5.7 i18n

**实测**：语言切到 English 后，WSL 面板全部文案（含标题、提示、四个只读说明、按钮、禁用提示、确认框）均为英文且无残留 key；切回中文正常。语言写回 store（`language="zh"`）。

### 验收实测（W3 验收第 1–6 条，全程只在 UI 内完成）

| # | 结论 | 证据 |
|---|---|---|
| 1 | 通过 UI（下拉选发行版 + 「使用此核心」）切换到 WSL 核心并重启成功 | `active_core="wsl"` + `Starting Harness process in WSL distro Ubuntu (port 3082)` + `healthy - 54/54` |
| 2 | 发行版内进程与 pid 文件一致 | pid 文件 `1883` + `boot_id f196c47c-…`；`pgrep -fa "dsh --profile"` = `1883 node /home/pixel/.npm-global/bin/dsh --profile web --port 3082 --no-open --skip-auth` |
| 3 | dsh 内工作区与命令落在 Linux 侧 | 会话文件解压后 `"cwd":"/home/pixel"` 与 `Linux LAPTOP-QMU9QT6R 6.18.33.2-microsoft-standard-WSL2 #1 SMP PREEMPT_DYNAMIC … x86_64 GNU/Linux` |
| 4 | 应用内「重启服务」：旧 Linux 进程被 kill、新进程起来 | `Stopped WSL Harness processes in Ubuntu` → `Harness port healed from 3082 back to 3081 (no longer occupied)` → `pid=12256, port=3081`；pid 文件同步更新（1883 → 3790） |
| 5 | 退出应用后发行版内无残留 | `close_action` 经 UI 设为「退出应用」→ 点窗口关闭 → `Stopped WSL Harness processes in Ubuntu` + 应用进程退出 + `pgrep -fa "dsh --profile"` = `(none)` |
| 6 | 切回 `app` 核心后 Windows 核心正常、无 WSL 残留 | `active_core="app"` → `Port 3081 is occupied, trying the next port` → `Harness process started successfully: pid=31052, port=3082` + `healthy - 59/59 client modules ready` + `pgrep` = `(none)` |

### 验收期发现并修复的两处缺陷（W5-A / W5-B）

#### W5-A（低，前端）：核心列表 WSL 行的「数据目录」不随面板探测更新

**现象**：首次选定发行版后，面板「数据目录」长期停在占位符 `—`、「打开数据目录」按钮不出现，直到手动点「刷新」。
**根因**：核心列表 WSL 行的 `dir` / `version` / `present` 取自**后端进程内探测缓存**（`service/core/version.rs` 的 `wsl_data_dir_unc` 走 `probe::cached`）。面板自己的 `probe_wsl_core` 会写入该缓存，但写入后没有任何东西让 `['cores']` 查询失效；`setting_updated` 触发的那次重拉发生在探测完成**之前**。
**修**：`runProbe` 成功后调用 `use-dsh-cores` 的既有 `refreshCores()`（`.catch(() => {})`，不让列表刷新失败污染探测错误态）。附带代价：`refreshCores` 每次渲染重建，`useEffect(..., [distro])` 触发 `react/exhaustive-deps` 告警，按仓库既有风格（`download-core-dialog.tsx` / 本文件另两处）加一行带理由的 disable。
**复验**：清空 `wsl_distro` + 重启应用（清空探测缓存）→ 停止服务 → 重开面板 → 自动选中后**未点「刷新」**，数据目录直接显示 `\\wsl.localhost\Ubuntu\home\pixel\.dsh-desktop.dev`。

#### W5-B（中，后端）：`set_active_core('app-<tag>')` 在「激活目录已持有该 tag」时误报未下载

**现象**：WSL 核心激活时，点击核心列表里已安装的 `0.1.2-rc.1` 行 → `CORE_VERSION_NOT_DOWNLOADED: dsh-0.1.2-rc.1-33729514615`，切不回 Windows 核心。
**根因**：`service/core/version.rs::switch_app_version` 里「激活目录已是目标 tag → 只切来源标记」的快路径写在 `existing_slot_dir(tag)` 存在性检查**之后**；而激活目录固定名为 `dependencies/dsh`，同 tag 时磁盘上并不存在 `dependencies/<tag>`，于是先被槽位检查拦下。上游不可达：App 核心激活时该行 `active=true`，UI 的 `onActivate` 直接早退；WSL 激活后该行 `is_active` 为 false（`active` 要求来源为 App）而 `is_installed` 为 true（`present` 取激活目录），于是变成可点击，暴露该顺序错误。
**修**：把同 tag 快路径提前到 `existing_slot_dir` 之前（保留 `fs_guard::validate_id` 在前），并写明「必须早于槽位检查」的理由。行为影响面：仅把原本 `CORE_VERSION_NOT_DOWNLOADED` 的路径变成成功，其余分支不变。
**复验**：UI 点 `0.1.2-rc.1` 行 → 确认 → `active_core: "app"` → `Stopped WSL Harness processes in Ubuntu` → Windows 核心 `healthy - 59/59`；`pgrep` 无残留。**说明**：该函数需要 `AppHandle`，`version.rs` 现有测试都是纯函数 / 临时目录形态，无免句柄切面，故未加单测，改以上述端到端验收覆盖（已与 R-W4-2 的「以验收替代不可单测路径」同例）。

### 门禁复核（§0 v3）

| 项 | 结果 |
|---|---|
| 命令 1 `cargo test --locked` | **559 passed / 0 failed**（W4-R 的 555 + W5 新增 4 个 `credentials.rs` 单测） |
| 命令 2（**v3**）fmt | 首轮 `credentials.rs` 报 4 处（新文件基线 0，v3 口径生效）；`rustfmt` 整理后复跑 **无 REGRESSION** |
| 命令 3a clippy（改动文件） | 改动文件内**唯一命中 20 条**；逐条以「命中行内容在 `upstream/main` 同文件中存在」自动核验，**非上游既有 0**（脚本比对：`NOT upstream-existing: 0`）。新增 Rust 文件（`wsl_core/*` / `bridge/wsl_core.rs` / `wsl_launch.rs` / `credentials.rs`）**0 命中** |
| 命令 3b clippy 全量 | `^warning` **81 = 基线 81** |
| 命令 4 | `pnpm run typecheck` 通过；改动的前端文件 eslint **0 问题**；`pnpm run lint` **0 error / 23 warning**（23 条全部落在 `packages/**`，上游既有） |

### 环境事件记录（W5 期间，非代码问题）

- **主窗口 toast 不渲染（上游既有行为，本轮未改也未修）**：`src/components/toast-provider.tsx` 给 `Toast.Provider` 传的子节点是 `props.custom ? renderFn : null`，而 `@heroui/react` 的 `ToastProvider` 只在 `typeof children === "undefined"` 时使用默认渲染器，显式 `null` 会渲染为空（DOM 上表现为 `.toast-region` 里 3 个空 `<li>`）。证据：① 页面内直接 `import('/src/utils/toast.ts').toast('CDP-TEST-TOAST')` 同样无内容；② `toast-provider.tsx` 与 `upstream/main` 逐字相同（`git diff --stat` 为空）。影响：W5 的 toast 反馈（安装完成 / 导入成功 / 已是最新 / 导入失败）在 dev 下不可见；面板状态、后端日志与命令返回值仍可完整验证。**判定不属 W5 范围**（跨功能、影响桌宠窗口与所有既有 toast 调用方），留待 W6 或单独议题。
- 打开设置对话框时的 `A PressResponder was rendered without a pressable child` 警告：`toast-provider.tsx` 未改、W4-R 之前同样出现，判定上游既有。
- 验收对 WSL 侧环境的**有意改动**：`~/.dsh-desktop.dev` 内 dsh 由 `0.1.5-rc.1` 更新到 **`0.1.5-rc.2`**（W5.3 真实安装跑通的结果）；`.credentials.yaml` 被导入流程重写（`DEEPSEEK_API_KEY` 与 Windows 侧一致，权限仍 0600）。
- 端口双向迁移均按既有策略自动处理：Windows 3081 → 切 WSL（Windows TIME_WAIT 占 3081，Linux 侧扫描落 3082）→ 重启 heal 回 3081 → 切回 app（Linux 侧 TIME_WAIT 占 3081，Windows 自动 +1 → 3082）。即 F12 的两个方向，均无需人工干预。
- `getCurrentWindow().close()` 不可用（能力表未授权 `core:window:allow-close`），退出验收改用 PowerShell `Process.CloseMainWindow()` 发送 WM_CLOSE，等价于点标题栏 ×（`close_action="quit"` 时走 `app.exit(0)` → `stop_on_exit`）。
- dev store：验收前备份 `.store.dev.dat.bak-w5-20260920`；验后已还原基线（`active_core=null`、`wsl_distro=null`、`port=3081`、`manual_port=null`、`backup_retention_count=10`、`close_action=tray`；`language` 验后为 `zh`）。验收期间为验证退出路径曾把 `close_action` 设为 `quit`，已还原。
- 应用退出后：WSL 内无 dsh、Windows 侧无残留进程。

## W5-R 返修记录（2026-09-30，依据 `E:\DSH-WSL\WSL-CORE-REVIEW.md` §E）

按 §E.4 顺序逐项返修；每条给出代码落点与实际验收。**本记录不沿用审核端的 559 测试数**，
本轮复跑见「门禁」节。

### R-W5-1（高）凭据导入：合法 v1 文档 + 独占锁 + 0600 临时文件原子提交

**代码落点**
- `src-tauri/src/service/wsl_core/credentials.rs`（全量重写）：
  - `extract_api_key`：改由 `serde_yaml` 解析，**只认顶层 `refs`** 的同名键（`records` 等位置的同名键不再误命中）；值必须是字符串、去空白后非空且无控制字符。
  - `validate_v1`（新增）：顶层 mapping + `version: 1` + `refs`（若存在）为 mapping 或 null；解析错误**只报行/列位置**，不回显可能含密钥的原文。
  - `merge_api_key`：空目标生成 `version: 1\nrefs:\n  DEEPSEEK_API_KEY: …`；非空目标只改 `refs` 块内同名键（沿用块内缩进），`refs` 缺失时补一个；其余行（含注释与 `records`）逐行保留；值经 `serde_yaml` 标量编码。
  - `import_api_key` 新流程：Linux 侧 `ENSURE_CREDENTIALS_DIR`（`umask 077; mkdir -p`，新目录 0700、既有目录不动）→ 经 UNC `create_new` 取 `.credentials.yaml.lock`（`AlreadyExists` → `WSL_CREDENTIAL_BUSY`，不删他人锁、不重试）→ **锁内**重读目标（仅 `NotFound` 当空；其它错误终止）→ 非空先 `validate_v1`（同值分支不能被绕过）→ 同值则 `CHMOD_CREDENTIALS` 确认 0600 后返回 `Ok(false)` → `MAKE_CREDENTIALS_TMP`（`umask 077; mktemp` 同目录 0600 临时文件）→ 经 UNC 写入合并文本 → `COMMIT_CREDENTIALS`（`chmod 600 "$t" && mv -f -- "$t" <目标>`，任一步失败非零退出）→ 失败时清理本次临时文件并返回 `WSL_CREDENTIAL_WRITE_FAILED`；`LockGuard` 在所有出口删除自己的锁（先关句柄再删）。
  - 密钥不进 `wsl.exe` argv、不进脚本、不进日志与错误文本。
- `script.rs`：新增 `ENSURE_CREDENTIALS_DIR` / `MAKE_CREDENTIALS_TMP` / `COMMIT_CREDENTIALS`；`CHMOD_CREDENTIALS` 去掉 `|| true`（失败非零退出）；`SCRIPTS` 7 → 10 条。
- 单测：credentials 4 → 5（含「records 不误命中」「空目标产物可被 `validate_v1` 解析」「不支持目标报错且不回显内容」）；script 新增 1（凭据脚本 umask/原子提交/不吞错）。

**验收（隔离 + 虚构密钥，审计全程未使用真实密钥）**
- 首次导入（目标不存在）：产物 `version: 1` + `refs.DEEPSEEK_API_KEY: sk-rw51-fictional-key-0001`，`stat` **600**，无锁/临时残留；把该文件放进隔离 `DSH_HOME=/tmp/…` 启动 `dsh --profile web`，**exit=124（40s timeout，正常跑满）**、日志无 `flat layout`/`readable beyond` 错误 → 「目标不存在 → 导入 → 首次启动」成功。
- 既有文件（dsh 自己生成、含 `records.client-connection/browser-session`）：导入后 `refs` 值替换为新 key，**`records` 段 sha256 与导入前逐字相同**（`20e23fe6…`），0600。
- 同值再导入：toast「API Key 已是最新，无需导入」，文件 sha256 与 mtime 不变，权限仍 600（同值分支确实走到 chmod 确认）。
- 预置 `.credentials.yaml.lock`：导入报 `WSL_CREDENTIAL_BUSY: another writer holds …\.credentials.yaml.lock`，原文件 sha256 不变，**他人的锁未被删除**（内容 `held-by-test` 保持）。
- 目标换成目录（非 `NotFound` 读取错误）：报 `WSL_CREDENTIAL_WRITE_FAILED: cannot read …: 拒绝访问。 (os error 5)`，目录未被改动，无残留。
- 目录 `chmod 500`（提交前失败）：报 `cannot create lock …: 拒绝访问。 (os error 5)`，原文件 sha256 不变，无残留，目录权限未被改回。
- 提交脚本失败契约（脚本级）：tmp 不存在时 `chmod` 失败 → `&&` 短路、退出码 1，目标文件未被触碰。
- 未能自然构造的「commit 阶段失败」（同目录 rename 在正常权限下不失败）：以脚本级验证 + 代码路径审查替代（见上条）。

### R-W5-2（中）来源判定改为 `wslRunning = serviceRunning && active`

**代码落点**（`src/components/config-wsl-core.tsx`）
- 新增 `wslRunning`（`serviceRunning && active`，`active` 取 WSL 来源行）；下拉 `isDisabled`、`onSelectDistro` / `onProbeSelected` 守卫、运行中提示 `<If>`、安装前 `wasRunning` **全部**改用 `wslRunning`。
- 文案同步（`zh-CN` / `en-US`）：`wsl_core.distro_locked` 改为「WSL 核心运行中，停止服务后可更换发行版」；`wsl_core.update_confirm_desc` 改为「WSL 核心运行中会先停止服务，装完自动重启（约 1–2 分钟）。」

**验收**
- Windows 核心运行中（pid 18980 / 3081）：打开「核心」面板，发行版下拉**可用**、可探测、「更新 dsh」**实际完成安装**（WSL 内 `0.1.5-rc.2 → 0.2.0-rc.2`，`--skip-auth patch startup=true, connection=true`）。
- 全程 `http://127.0.0.1:3081/` 持续 200、日志无 `Stopping/Stopped Harness`、无新 `Harness process started`（Windows 会话未中断、未重启）。

### R-W5-3（中）候选显示与已确认配置分离

**代码落点**（同上文件）
- 删除「挂载时自动写 `wsl_distro`」的 effect；新增 `candidate`（仅显示用）与 `selected = distro || candidate`，`current = probe?.distro === selected ? probe : null`（状态行/就绪判定只认与当前显示目标一致的探测结果）。
- 只有显式操作持久化：下拉选择 `onSelectDistro`、「重新检测」`onProbeSelected`（候选未确认时先确认再探测）、安装 `onInstall` / 导入 `onImportKey`（`target !== distro` 时先 `persistDistro(target)`，后续全部使用当次 `target`）。
- 挂载即探测只针对**已持久化**的 `distro`（`useEffect([distro])`）。

**验收**
- 初始 `wsl_distro=null` 打开/关闭核心面板：`get_app_config().wsl_distro` 仍为 `null`，`get_cores()` **无 WSL 行**（后端探测缓存未被写入），日志无发行版探测痕迹。
- 点「重新检测」（显式操作）：`wsl_distro='Ubuntu'` 持久化 + 探测完成，WSL 行出现（`0.1.5-rc.2`、`dir=\\wsl.localhost\Ubuntu\home\pixel\.dsh-desktop.dev`），面板状态行完整（Node v22.22.1 / dsh 版本 / 更新按钮），数据目录无需手动「刷新」即显示（W5-A 修复保持）。
- 备注：下拉里选择与候选相同的项不会触发 HeroUI `Select.onSelectionChange`，同值路径由「重新检测」覆盖。

### R-W5-4（中）安装后接回前端完整 `store.harness.restart()`

**代码落点**（同上文件）
- `onInstall`：保留初始 `invoke('shutdown_harness')`（避免进度面板被前端 shutdown 关闭）；`finally` 中 `if (wasRunning) await store.harness.restart()`（**不再**调后端 `launch_harness`）；成功分支 `if (!wasRunning) await runProbe(target)`（完整 restart 会按既有行为关闭设置面板）；未修改 `src/store/modules/harness/store.ts`。

**验收：被本轮新发现的 D-W5R-2 / D-W5R-3 阻塞，未能完成端到端**。
- 阻塞链：UI「更新」→ `install_wsl_core` 以 `latest`(=0.2.0-rc.2) 为 target → 装出与桌面壳协议不兼容的核心（D-W5R-2）→ 即使走 `store.harness.restart()`，就绪检查也永远不过；换装其它版本又命中补丁锚点失配（D-W5R-3）→ `skip_auth_ready=false`、核心无法启动。
- 已可确认的部分：`store.harness.restart()` 的端口同步行为（切核心路径）在 W5/W4 多次实测（3081→3082 落点、heal 回 3081、`serviceUrl`/`iframeSrc` 同步）；本轮亦实际观察到 WSL 侧进程被停/起与 store.port 一致。**D-W5R-2/3/4 已于 2026-09-30 经用户确认；该项待相关实现完成后补做端到端验收。**

### R-W5-5（低）平台门控与空列表状态

**代码落点**
- `src/components/config-dialog.tsx`：新增 `detectWindows()` + `IS_WINDOWS`（复用 `navbar.tsx` 的 UA 判定风格），`<Case cond="harness">` 内以 `<If cond={IS_WINDOWS}>` 包裹 `<ConfigWslCore />`。
- `config-wsl-core.tsx`：`distrosError !== '' && distros.length === 0` → `distros_failed`（红）；`distrosError === '' && distros.length === 0` → `distro_empty`（灰）；安装/导入按钮在空列表时禁用。

**验收**
- Windows 正常列表不受影响：面板多轮实测（候选默认显示、探测、安装、导入均正常）。
- 非 Windows 不挂载 / 空列表与失败提示的**动态验收无法在本机（Windows）完成**，为纯条件渲染（`IS_WINDOWS` 常量 + `distros.length`/`distrosError` 分支），仅代码审查；Rust 侧非 Windows 门控未改动。

### R-W5-6（低）toast 根因修复

**代码落点**：`src/components/toast-provider.tsx` 非 custom 分支 `: null` → `: undefined`（唯一改动）；custom 桌宠分支与队列逻辑未动。

**验收**：主窗口三类反馈**均可见**（`.toast-region` 实测）——导入成功「API Key 已导入」、同值「API Key 已是最新，无需导入」、失败「导入 API Key 失败」（分别由 R-W5-1 的三种场景触发）；`pnpm typecheck` 与 `toast-provider.tsx` eslint 通过。桌宠 custom 渲染器本轮未开窗（store `pet_enabled=false`），为代码审查 + 与上游逐字相同的 custom 分支。

### R-W5-7（低）两处无关格式 diff 还原

**代码落点**
- `src-tauri/src/desktop/builder.rs`：`preset_pet_asset_response` 调用还原为上游单行长行（含原尾随空格）。
- `src-tauri/src/service/core/mod.rs`：`pub(crate) use runtime::prepare_active_runtime;` 移回 `pub use version::{…}` 之后（保留 WSL 导出项与必要换行）。

**验收**：`builder.rs` fmt 计数 upstream=1 / now=1，`core/mod.rs` upstream=2 / now=2（无 REGRESSION 无 CHURN，§0 v4）；两文件的 diff 不再含宠物协议回调换行或 runtime 重导出重排。

### 门禁（§0 v4 命令，全部实际复跑）

| 命令 | 结果 |
|---|---|
| 1. `cargo test --all-features --locked` | **561 passed / 0 failed**（W5 的 559 + credentials 净增 1 + script 新增 1） |
| 2. fmt v4（逐文件去 CR 与 upstream 比 `^Diff in`） | 无 REGRESSION、无 CHURN（中途 `credentials.rs` 6 处 / `script.rs` 1 处经 rustfmt 归零） |
| 3a. clippy 改动文件命中 | 21 条，逐条核验**全部为上游既有**（脚本比对命中行在 upstream 同文件存在）；新增文件 0 命中 |
| 3b. `grep -cE '^warning'` | **81 = 基线**（中途 82，定位为 `credentials.rs` 测试里 `useless format!` 1 条，修复后归零） |
| 4. typecheck + eslint（§0 列出的 8 个文件） | 通过、0 问题 |

### W5-R 验收中发现的额外缺陷（**超出 §E 范围**，其中 3 项已裁决、待实施/验收）

#### D-W5R-1（已修，低）`INSTALL_DSH` 的 `--prefer-offline` 让陈旧缓存下的精确版本安装假失败

- 现象：registry 发布 `0.2.0-rc.2` 后，WSL 内 `npm view` 能解析出版本，但安装报 `npm ERR! notarget No matching version found for @deepseek-ai/dsh@0.2.0-rc.2`（本地 packument 缓存陈旧，`--prefer-offline` 命中缓存后不联网刷新）。
- 对照实测（隔离前缀）：带 `--prefer-offline` 失败；**去掉后成功**。
- 修复：`INSTALL_DSH` 两处去掉 `--prefer-offline`（单测加 `!INSTALL_DSH.contains("--prefer-offline")` 断言）。R-W2-1 原本担心的 dist-tag 旧值由 `RESOLVE_DSH` 在线解析成精确版本解决，install 只接收精确版本。修复后 UI「更新」实测成功。

#### D-W5R-2（**已裁决，待实施/验收**，高）WSL 安装走 `latest`，与桌面端「推荐版本」策略不一致，可装出不可用核心

- `WSL_DSH_VERSION_SPEC = "latest"`；当前 registry `latest`/`next` 均为 `0.2.0-rc.2`，而桌面端自身日志明确 `Suppressing dsh update toast because latest is above recommended (preview/alpha): 0.2.0-rc.2`，且其核心下载**不使用 latest**（`bridge/lifecycle.rs` 注释：「安装目标遵循应用资源中的推荐版本」；`src-tauri/resources/version-recommend.json` = `{"dsh": "0.1.2-rc.1"}`）。
- 实测后果：WSL 核心装 `0.2.0-rc.2` 后，桌面壳健康检查对 `/plugins/@deepseek-ai/dsh-client-ui-layout/client.js` 等返回 **404（not a plugin bundle）**、`HARNESS_NOT_READY: 0/2 ready`，前端 `startup failed`（0.2.0 的客户端包结构已变：`dsh-client-ui-layout` 无 `client.js`）。
- 另据 D-W5R-3/4 的实测，「回落装 0.1.x」也不是无痛路径；版本组合（主包 vs `^` 解析出的子包）会决定补丁与 profile 是否兼容。
- **用户裁决（2026-09-30）**：默认目标共用 `config::recommended_dsh_version()`，缺失/无效时报错，不回退 latest；本轮只适配推荐版本，不扩展 0.2 或通用版本矩阵。主包同版本不等于依赖组合相同，须按方案 v7 在干净安装上验证。

#### D-W5R-3（**已裁决，待实施/验收**，高）`--skip-auth` 补丁与 `skip_auth_ready` 判定对现有 npm 全局布局/形态不健壮

实测三类失配（均为 npm 全局安装的原版核心）：
1. `dsh-client-connection` 位于 `…/@deepseek-ai/dsh/node_modules/@deepseek-ai/dsh-web-app/node_modules/@deepseek-ai/dsh-client-connection`（web-app 嵌套）时，`CONNECTION_INDEX_RELS` 两个候选与 `PROBE` 脚本的两个候选**都不命中**（0.1.5-rc.2 干净安装实测：`patch target not found`、`connection=false`、`skipAuthReady=false`）。
2. `dsh-web-app@0.1.5-rc.3` / `0.1.2-rc.1` 的 `startup.js` 为**单行链式**（`.option("--no-open", …)` 不在行首、无 `\t\t` 缩进）时，`STARTUP_OPTION_ANCHOR` **失配**（ACTION 锚点命中、OPTION 失配）。
   - **裁决时源码复核**：当前 OPTION 锚点为不含行首/缩进条件的普通子串，`patch_startup` 使用 `contains`；因此“单行链式导致失配”尚不能成立。执行端须提供实际片段与两锚点分别的命中结果，再修真实差异。
3. 对照：Windows 侧 app 核心（核心列表标「预打包」）的 `startup.js` 已内置换行顶格形态的 `--skip-auth`（构建期注入），不走运行时锚点；`web_startup_supports_skip_auth` 因此对它恒真。
- 后果：`skip_auth_ready=false` → `START` 固定传 `--skip-auth` 无法启用 → WSL 核心启动即退出（`HARNESS_NOT_READY`/进程 code 1）。
- 本轮为完成 R-W5-1 环境隔离与 R-W5-2 安装验收，曾在 WSL 环境内做**等价手工补丁**（与 `patch_startup`/`patch_connection` 相同的两处替换）与一次**等价 symlink**（把 web-app 嵌套的 connection 链接回候选 1 路径）；这些是环境操作、**不是代码修复**，且未解决 (2)。
- **用户裁决（2026-09-30）**：补齐实际加载模块的嵌套路径，patch/PROBE 同步；启动脚本按真实失配样本局部修复，不重建通用能力探测/补丁机制，保留 Host/Origin 校验。

#### D-W5R-4（**已裁决，待实施/验收**，中）同树 reify 的版本切换可能留下不一致依赖树

- 实测：`0.2.0-rc.2 → 0.1.5-rc.2` 的 `npm i -g` 后，dsh 启动报 `plugin tree failed to load: @deepseek-ai/dsh-sandbox-local`、退出码 1；`rm -rf` 安装目录后干净重装解决。
- **用户裁决（2026-09-30）**：跨版本采用限定 DSH 包目录的可回滚干净重装；先确认目录/前缀与共享 CLI 影响，停服、备份旧包及对应入口，安装/验证失败恢复。不得清空整个 npm 前缀、其它包或用户数据；具体范围与验收见方案 v7。
- 另：WSL profile 的 `package.json` 含 `"patchReload": "live"`（自 2026-09-12 起），`0.1.2-rc.1` 启动要求 Cordis HMR 服务（`user patch-layer watching requires the Cordis HMR service`），同为版本组合脆弱性的一部分。

### 环境状态与回归

- 代码工作树：`feat/wsl-core`，改动集中在 `src-tauri/src/service/wsl_core/**`、`src/components/{config-wsl-core,config-dialog,toast-provider}.tsx`、i18n 两个 json；未提交（按约定等 W6 统一处理）。
- dev store：`active_core='app'`、`wsl_distro='Ubuntu'`、`port=3081`、`manual_port=null`（WSL 核心当前不可用，已切回 app 核心避免误导）。
- WSL 内环境：dsh = `0.1.2-rc.1`（干净重装）+ 等价手工补丁 + 候选 symlink；（**已更新**：该手工补丁 / symlink 环境已在第二轮验收前删除，现为代码补丁的干净环境，见下节。）`~/.dsh-desktop.dev/.credentials.yaml` 已从备份还原（0600，内容与导入前一致）；`/tmp` 的隔离测试文件已清理。
- 未执行：`pnpm tauri build`（按约定安装前停下）、W6 全量验证。

### W5-R 用户裁决确认（2026-09-30）

用户确认：“就按你建议的来”。确认范围如下，**均为执行方向已确认，不表示代码或验收已完成**：

| 事项 | 已确认方向 | 下一步 |
|---|---|---|
| D-W5R-2 | 默认桌面推荐版本；缺配置停止，不回退 latest；不扩展 0.2 兼容范围 | 统一自动入口并在干净实际依赖组合上验证 |
| D-W5R-3 | 实际路径与真实失配样本的局部修复；保留 Host/Origin 校验 | 路径/PROBE 一致性、锚点根因确认及样本测试 |
| D-W5R-4 | 跨版本限定目录备份后干净重装，失败恢复；不清用户数据 | 实现恢复路径并完成版本切换与失败恢复验收 |

`patchReload: "live"` 的处理先做干净临时 profile 对照；需要修改既有 profile 时再提交精确字段和值供确认，不自动重置或迁移。原验收留下的手工补丁/symlink 不能充当最终通过证据。

执行依据：`E:/DSH-WSL/DSH-WSL-CORE-PLAN.md` **v7 → W5-R 补充裁决与验收**。补齐 R-W5-4 的更新后桌面端就绪验证并记录真实结果，再进入 W6。本次文档更新未运行安装、未修改 WSL 环境。

## W5-R 补充裁决实施记录（2026-09-30 第二轮，依据方案 v7）

### 实施与门禁（本轮真实数字）

D-W5R-2/3/4 已实施并完成可完成的验收；R-W5-4 被「阻塞 1」阻断，按用户 2026-09-30 裁定**阻塞上报**（不允许变更 profile 字段）。

- 测试：`cargo test --all-features --locked` = **568 passed / 0 failed**（较上轮 561 净增 7：script 3、install 3、patch 2、alpha_auth 1；删除废弃的 `default_spec_is_latest_dist_tag` 1）。
- fmt v4：全部改动 / 新增 rust 文件无 REGRESSION。
- clippy：3b `^warning` = **81 = 基线**；改动文件命中 20 条经脚本核验全部上游既有；**新增文件 0 命中**。
- 前端：`pnpm run lint` 0 errors / 23 warnings（全在 `packages/**` 上游既有）；`pnpm run typecheck` 通过。
- 删除 `WSL_DSH_VERSION_SPEC`（"latest"）：`install_wsl_core` 与 `bridge/lifecycle.rs` 开机自愈统一走 `install::default_version_spec()`（= `recommended_dsh_version()`；缺失报 `WSL_DSH_VERSION_UNCONFIGURED`；已装用户缺配置时以当前版本幂等保留）。

### D-W5R-2（已实施）：默认推荐版本

- 实测首装：默认目标 = `0.1.2-rc.1`（`resources/version-recommend.json`），825 s 完成，`skipAuthReady=true`。
- 实际包组合（npm 原版 0.1.2-rc.1）：主包 / web-app / connection / app-boot 均 0.1.2-rc.1；**cordis 4.0.4、cordis-plugin-loader 1.0.5、cordis-plugin-hmr 1.0.19、cordis-plugin-timer 1.1.6**（`^` 范围解析到的最新，见阻塞 1）。

### D-W5R-3（已实施）：按真实样本局部修复

隔离干净安装样本（无手工补丁 / 无 symlink）：

| 版本 | web-app 位置 | connection 位置 | OPTION | ACTION | REJECTION | AUTHORIZE |
|---|---|---|---|---|---|---|
| 0.1.2-rc.1 | dsh 直属嵌套 | dsh 直属嵌套 | 1 | 1 | 1 | 1 |
| 0.1.5-rc.2（web-app / connection 0.1.5-rc.3） | dsh 直属嵌套 | **web-app 嵌套** | 1 | 1 | 1 | 1 |

- **采纳审核端纠正**：四锚点在两个样本均逐字单次命中；「单行链式使锚点失配」不成立，**真正差异是 connection 的路径**。`alpha_auth.rs` 锚点零改动。
- `patch.rs` 候选 2→3（web-app 嵌套 > dsh 直属 > 扁平，Node 解析优先序）；`script.rs` 的 PROBE `SKIPAUTH` 改同序「第一个存在」判定；两处候选由交叉测试逐字锁定（`patch_targets_match_probe_candidates_exactly`）。
- `alpha_auth.rs` 增真实样本形态测试（单行链式 startup + 带 `authorizeIndex(req, res)` 变体的 connection；未改 / 已补丁 / 重复补丁三态）。
- 实测：补丁在两种布局均 `startup=true, connection=true`（候选 2 / 候选 1 各自命中）。

### D-W5R-4（已实施）：可回滚干净重装

- 新增脚本 `BACKUP_DSH` / `REMOVE_DSH_PACKAGE` / `RESTORE_DSH` / `CLEAN_DSH_BACKUP` / `VERIFY_CORE`（SCRIPTS 10→15）；`install.rs` 仅在**版本切换**启用窗口（备份→清包→安装→补丁→启动验证→成功清理 / 失败恢复）；同版本幂等不进窗口。
- **实测（受控失败恢复，全链路）**：`0.1.2-rc.1 → 0.1.5-rc.2` 切换：备份（323 MB，p0 槽位）→ 清包 → 安装 → 补丁命中（新候选 1）→ 启动验证失败（见阻塞 2）→ **自动恢复**（版本 / 补丁痕迹 / bin 入口全回来，错误含 `(previous dsh version restored)`）；备份与 `.wsl-core-verify.log` 现场保留。
- **未实测分支**：「验证通过 → 清理备份」的收尾（与 R-W5-4 同因阻塞，待 §F）。

### R-W5-4：**阻塞**（用户裁定：不允许变更字段，阻塞上报）

UI 流程已走通（面板状态行 `Node v22.22.1 / dsh 0.1.2-rc.1` → 使用此核心 → `active_core='wsl'`）；WSL 服务启动失败，readiness / iframe 同步无法达成。

#### 阻塞 1（高）：npm 原版 0.1.2-rc.1 在 `patchReload: "live"` 下无法启动

- 现象与复现：`dsh: user patch-layer watching requires the Cordis HMR service`（dsh-app-boot `watchUserPatches`，profile-boot 279 行调用点）→ 退出码 1；既有 profile 与全新临时 `DSH_HOME` 均复现。
- 排除项：`NODE_OPTIONS=--expose-internals` 被 Node 拒绝（exit 9）；`node --expose-internals lib/bin.js` 同样崩；`loader.internal` 本身正常（插桩：`fromInternal => ok/v1`，Node 22.22.1）。
- 变量分离与可用路径：仅将 `profiles/web/package.json` 的 `dsh.profile.patchReload` 由 `"live"` 改为 `"startup"` → dsh 启动正常且**全部 client bundle 就绪**（readiness 检查逐项通过）；实验后已还原 `live`。
- 根因（插桩定位，均已还原）：`ctx.loader.create(hmr)` 返回对象的 `state`/`error` 均 undefined、`ctx.fiber` 为 undefined —— npm 原版把 cordis 系解析到 **4.0.4 / loader 1.0.5 / hmr 1.0.19**（`^` 范围最新），与主包内 app-boot 0.1.2-rc.1 的 API 期待不兼容 → HMR 服务静默不就绪。Windows 预打包核心锁定 4.0.2 / 1.0.3 / 1.0.17 且 desktop profile 用 `startup`，故不受影响。
- **精确字段与候选值（供 §F 裁决）**：`<DSH_HOME>/profiles/web/package.json` → `dsh.profile.patchReload`；现值 `"live"`（模板默认 `DEFAULT_PROFILE_PATCH_RELOAD = "live"`），候选 `"startup"`（实测可用）；语义取舍：patch 层仅启动时加载、不热重载。替代路径：等上游修 app-boot / loader 兼容；或 WSL 改用预打包核心（架构变更）。**本轮未实施。**

#### 阻塞 2（中）：0.1.5-rc.2 启动验证失败（安装器按设计拦下）

- `dsh: plugin(s) failed to load: @deepseek-ai/dsh-sandbox-local`（干净安装 + 补丁命中后仍复现）；`VERIFY_CORE` 失败 → 自动恢复，行为正确；0.1.5 系不在 D-W5R-2 承诺范围。

### 环境与证据

- WSL 内：纯代码补丁环境（无手工补丁 / symlink），dsh = 0.1.2-rc.1；WSL profile 保持原状（`patchReload: "live"`，未擅自修改）。
- 失败窗口现场保留：`~/.dsh-desktop.dev/.wsl-core-backup/1790781969`（323 MB）与 `~/.dsh-desktop.dev/.wsl-core-verify.log`（供审核端复核）。
- 诊断日志（证据）：`~/rw54b.log`（干净 profile 崩溃）、`~/rw54c.log` / `~/rw54d.log`（startup 可用性）、`~/rw54e.log`（NODE_OPTIONS 被拒）、`~/rw54f.log`（node flag 仍崩）、`~/rw54g.log` / `~/rw54h.log`（插桩）。
- **WSL `/tmp` 是 tmpfs**：VM 空闲关停（F14）后清空（本轮隔离安装确实消失过）。WSL 侧脚本的临时目录一律放数据目录内（`VERIFY_CORE` 已如此）。
- dev store 已还原基线：`active_core='app'`、`wsl_distro='Ubuntu'`、`port=3081`、`manual_port=null`、`close_action='tray'`；dev 应用已退出；`pnpm tauri build` 未执行。
- Windows 侧观察（非本项目范围）：`~/.dsh.dev/profiles/desktop/.dsh-module-fallback` 被桌面端 internal legacy 清理与 dsh heal 的时序竞态删除后，触发 `healProfileModuleFallback` ENOENT 启动失败；重建空目录恢复。

### W5-R 第二轮结论

本轮 D-W5R-2/3/4 的实施与可完成验收记录如上。**当前后续方向见 §F：保留 live，采用通过隔离验证的固定兼容依赖与受控 runtime**；接入后补 R-W5-4 UI、成功清理备份及失败恢复验收，再进入 W6。不再等待把 profile 字段改为 startup 的裁决。

## F. 兼容依赖锁定裁决与隔离验证（2026-10-01）

### F.0 裁决与状态

用户授权：“先测试一下吧，如果没有问题的话就用你说的改法，然后更新一下方案和裁决结果”。隔离安装和全新 npm ci 的验证均通过，现确认采用 **D-W5R-5：受控 runtime + 固定兼容依赖，保留 `patchReload: "live"`**。

- **已完成**：兼容性诊断、两次独立安装与 live/资源就绪/重启实测、验证清单与锁文件保存、执行方案 v8 更新。
- **待执行**：将安装器/PROBE/START/VERIFY/回滚接入私有 runtime，Tauri UI 的 R-W5-4 与成功清理备份分支验收。未修改本项目产品代码，未替换用户现有全局安装。
- 不采用把 profile 改成 startup 的处理；用户原先“既有 profile 不改”的要求仍成立。D-W5R-4 的回滚目标由旧全局 DSH 包转为**应用自有 runtime 整体**，不扩大到用户 npm prefix。

### F.1 根因证据

旧自动解析组合并非仅仅缺少 Node 参数：
1. 当前 loader 1.0.5 的 `create()` 完成时，HMR fiber 仍可能处于初始化状态；最小运行复现中 HMR 尚不存在，`await loader.await()` 后正常激活。
2. 激活后 HMR 1.0.19 的 `registerConfig` 仍为 undefined，而 dsh-app-boot 0.1.2-rc.1 的 `watchUserPatches()` 必须调用该方法。仅补等待不能解决 API 缺失。
3. loader 1.0.3 会等待 fiber 激活，hmr 1.0.17 提供 `registerConfig()`；这两个 npm 发布包的源码与桌面 dev 预打包内对应文件逐字一致。无需从 Windows 复制源码到 Linux，也无需另补 HMR 实现。
4. 对先前“create 返回对象没有 state/error、ctx.fiber undefined”的解释作澄清：`create()` 返回的是 entry id 字符串，不能据此当成 fiber 检查；本轮直接检查根 Context 的 fiber 为 ACTIVE（2）。

### F.2 精确基线与可重复安装

- 主包：`@deepseek-ai/dsh@0.1.2-rc.1`。
- overrides：cordis **4.0.2**、loader **1.0.3**、hmr **1.0.17**、timer **1.1.4**、include **1.0.7**、group **1.0.2**。
- manifest/lock：`E:/DSH-WSL/validation/wsl-live-20261001/package.json`、`package-lock.json`。锁 SHA-256：`46d0671df390de891168284922d4ee3dd62a6e1ba4ca469dd957f37b045fbe54`。
- 初次 npm install：527 个安装包，561.4 秒；另一空目录 npm ci：527 个安装包，5.2 秒，锁文件逐字不变（后一次利用缓存；不作产品耗时承诺）。六项固定包在锁树中各只有一个版本。
- 两次仅应用当前 `alpha_auth.rs` 中的既有 startup/connection 补丁；没有修改 HMR/Loader/Cordis 源码，也没有向启动器另加 await。

### F.3 实测结果

| 检查 | 首次安装 | 全新 npm ci |
|---|---|---|
| 原生 live profile 启动 | 通过 | 通过 |
| profile patch 新增/改值/删除 | 同一 PID 14045 生效 | 同一 PID 14209 生效 |
| home patch 新增/删除 | 同一 PID 生效 | 同一 PID 生效 |
| Windows loopback 首页 + boot graph bundle | 首页 200，47/47 通过 | 首页 200，47/47 通过 |
| 同端口重启 | 44100 → 44100，通过 | 45596 → 45596，通过 |
| 换端口重启 | 44100 → 47246，通过 | 45596 → 45688，通过 |
| 三轮停止与清理 | SIGTERM 退出 0，无进程组残留 | SIGTERM 退出 0，无进程组残留 |
| 既有安装/profile 保护 | 10 个相关文件哈希一致 | 10 个相关文件哈希一致 |

检查采用桌面实际 boot graph 与非空/非 HTML bundle 判定，支持 `/plugins/??` combo URL。首版隔离检查器曾误剔除 combo URL，修正为与 `service/workflow/utils.rs` 一致后重新完成上述两套全流程；该检查器错误不归因于产品。

原始无秘密的结果、测试 apply/dispose 事件、脚本、manifest/lock 与说明均保存在 `E:/DSH-WSL/validation/wsl-live-20261001/`。服务日志可能包含临时启动 token，未复制进资料目录；真实凭据未读取或改动。

### F.4 执行端下一步

按 `E:/DSH-WSL/DSH-WSL-CORE-PLAN.md` **v8 → W5-R v8 兼容依赖裁决与集成验收** 执行：
1. 将验证过的 manifest/lock 纳入桌面资源，安装目标改为 `$HOME/<本应用数据目录>/runtime`；使用 npm ci，不修改全局 dsh 或用户 npm 配置。
2. 安装判定包含兼容锁基线，不只比较主包版本。缺推荐配置或对应锁资源时停止，不以当前全局版本兜底。
3. 统一所有 WSL 路径，runtime 整体备份/切换/恢复，profile 与凭据保持原状；针对新入口复验 STOP。
4. 本次未启动 Tauri 窗口，故 **R-W5-4 的 UI/iframe 同步及安装器回滚集成仍未完成**；接入后补齐并记录实际结果，再进入 W6。

---

## G. W5-R v8 受控 runtime 实施与集成验收（2026-10-01，依据方案 v8）

### G.0 结论

方案 v8「专用 runtime + 受控安装清单/锁文件，保留 live」已落地到产品代码并完成集成验收：受控运行时的安装/更新/回滚全链路、live 热重载与就绪检查、R-W5-4 的 UI 更新 + 端口 heal + `serviceUrl`/`iframeSrc` 同步、候选失败/切换失败/成功清理、STOP 与退出无残留、用户全局 dsh 与用户数据未改动、§0 门禁复跑通过。**执行端自验收结果记录如上；2026-10-02 审核发现三项未闭环，当前 W6 入口暂不满足，见 §H。**

### G.1 代码落点

| 位置 | 内容 |
|---|---|
| `src-tauri/resources/wsl-runtime/0.1.2-rc.1/{package.json,package-lock.json}` | 已隔离验证的兼容基线（六项 overrides；锁 SHA-256 `46d0671d…fbe54`，与 validation 目录逐字一致） |
| `src-tauri/src/service/wsl_core/runtime.rs`（新） | 资源定位（debug 源码优先 / release 资源优先）、manifest+lock 交叉校验、锁哈希、基线记录 `.dsh-runtime.json`；缺资源/不匹配显式停止，无回退 |
| `src-tauri/src/service/wsl_core/script.rs`（重写） | 16 条脚本：PROBE/START/VERIFY_CORE 改受控 `.bin/dsh`；新增 PREPARE_RUNTIME_CANDIDATE / RUNTIME_NPM_CI / SWITCH_RUNTIME / RESTORE_RUNTIME / CLEAN_RUNTIME_BACKUP / DROP_RUNTIME_CANDIDATE；删除全局 `npm i -g` 族与 PATH 全局前缀 |
| `src-tauri/src/service/wsl_core/install.rs`（重写） | `ensure()` 候选窗口：npm ci → 写基线 → 打补丁 → VERIFY_CORE → 整体切换 → 清理备份；`needs_runtime_update`（版本+锁基线）、`default_version_spec`（D-W5R-2） |
| `src-tauri/src/service/wsl_core/probe.rs` | `WslCoreProbe.runtime_dir`；`dsh`/`npm_root` 语义 = 受控运行时 |
| `src-tauri/src/service/wsl_core/patch.rs` | `apply(distro, npm_root)` 只对显式根操作（正式或候选运行时） |
| `src/components/config-wsl-core.tsx` + `src/i18n/locales/*.json` | 确认框展示受控运行时路径（368 键中英一致） |
| `src-tauri/resources/README.md` | 资源说明（受控运行时、无无锁后备） |

### G.2 受控运行时布局

`$HOME/.dsh-desktop.dev/runtime`（debug）内含 `package.json`、`package-lock.json`、`node_modules/`（`.bin/dsh` → `@deepseek-ai/dsh/lib/bin.js`）与 `.dsh-runtime.json`（`{version, lockSha256}`）。主包 `0.1.2-rc.1`；overrides cordis 4.0.2 / loader 1.0.3 / hmr 1.0.17 / timer 1.1.4 / include 1.0.7 / group 1.0.2 —— 实测为顶层扁平布局（D-W5R-3 候选 3 档命中第 2 档）。切换窗口 = `runtime-candidate` → `runtime`（备份 `runtime-backup-<stamp>`），失败即 `RESTORE_RUNTIME`。

### G.3 集成验收（真实结果）

| 检查 | 结果 |
|---|---|
| 首装 `npm ci` + 六项解析 | 0 mismatch；`.dsh-runtime.json` 的 lockSha256 与资源一致 |
| live 首次启动 / 热重载 / 关停（对部署运行时直接驱动） | 47/47 client bundles；profile patch 增/改/删 + home patch 增/删同 PID 生效；SIGTERM exit 0；同端口与换端口重启通过（3211→3212） |
| Windows loopback（首页 + boot graph 全部资源） | 48/48（首启、同端口重启、产品路径 3081/3082 共 4 次） |
| Tauri UI R-W5-4 | 锁基线失配触发更新 → 停服 → npm ci → 切换 → 清理备份；端口 heal 3081→3082；`serviceUrl`/`iframeSrc` 同步 `http://127.0.0.1:3082/`；healthy 47/47 |
| 候选失败不影响旧运行时 | 假资源（0.9.9-rc.9）→ `WSL_DSH_INSTALL_FAILED: npm ci ETARGET`；runtime 树哈希前后一致、无残留 |
| 缺资源停止 | 缺省路径请求无资源版本 → `WSL_RUNTIME_RESOURCE_MISSING`；树哈希不变 |
| 缺配置停止（D-W5R-2） | 隐藏 `version-recommend.json`（源 + `target/debug/resources` 副本）→ `WSL_DSH_VERSION_UNCONFIGURED`，无 latest 回退 |
| 切换失败恢复 | `RESTORE_RUNTIME` 三态实测 `RESTORE_SKIPPED` / `RESTORE_NOTHING` / `RESTORE_OK` 均 exit 0；备份移回、版本 0.1.2-rc.1 |
| 成功后清理备份 | `runtime switched to 0.1.2-rc.1 (stamp …); backup cleaned`；无 `runtime-backup-*`/`runtime-candidate` 残留 |
| STOP / 退出无残留 | 新入口 cmdline `node …/runtime/node_modules/.bin/dsh --profile web …` 命中 `dsh --profile`；UI 停止后 pgrep 空、pid 文件删、端口释放 |
| 用户全局 dsh / 用户数据 | 与 v8 前快照逐项一致（全局 `package.json`、补丁后 startup.js / connection index.js、`bin/dsh`、profiles/web 两文件、`.credentials.yaml`；`~/.dsh.dev/.credentials.yaml` 仍不存在） |
| runtime 树哈希 | `e217870c…fb7f4`（24982 文件）—— 验收全程结束与污染前基线逐字一致 |

### G.4 §0 门禁（冻结版复跑）

| 命令 | 结果 |
|---|---|
| 1 `cargo test --all-features --locked` | **573 passed / 0 failed** |
| 2 fmt v4 | 无 REGRESSION / CHURN |
| 3a clippy 改动文件 | 21 处命中（20 个位置），全部 blame 到 `upstream/main` 祖先提交；新增 10 个文件 0 命中 |
| 3b 全量告警 | **81 = 基线 81**；直方图与基线一致（先修掉新增文件 `runtime.rs` 的 1 条 `useless_format`） |
| 4 前端 | `dsh-tauri` / `dsh-tauri-ui` build ✅（publint No issues）、typecheck ✅、eslint ✅ |

### G.5 环境注意事项（本轮实测新增）

1. **Tauri dev watcher 监视 `src-tauri/resources/`**：增/删/改名资源文件会触发重建与应用重启；`resource_dir()` = `src-tauri/target/debug`，其 `resources/` 为**增量复制、不删旧文件**。做「缺配置/缺资源」类测试必须同时隐藏源文件与 target 下的旧副本。
2. **不要用 `cp -al`（硬链接）造候选树**：候选与正式 runtime 共享 inode，脚本编辑候选会**同时改到正式运行时**（本轮 startup.js 被截断；已用全局安装中同哈希文件逐字恢复，树哈希回到 `e217870c`）。脚本级验收必须实复制。
3. **`corepack pnpm` 会污染嵌套 pnpm 调用**：tsdown 的 publint 步骤嵌套执行 `pnpm pack` 时命中 `COREPACK_ROOT`，拒绝切换到项目声明的 10.28.2 并报版本检查错误。§0 命令 4 用应用自带 pnpm 直跑（`node <app>/dependencies/pnpm/bin/pnpm.cjs …`）。
4. **`wsl.exe -e` 退出会回收后台进程**：长任务必须以「前台持有 wsl.exe 任务」形式运行。
5. Windows 侧 `python -c` 输出带 CRLF；拼 JSON 前需 `tr -d '\r'`。

### G.6 环境还原

dev store 与 v8 前备份（`.store.dev.dat.bak-v8-20261001`）**逐键一致**（`active_core=app`、`port=3081`、`manual_port=null`、`wsl_distro=Ubuntu`）；WSL 侧无 dsh 进程、3081/3082 已释放、无 `runtime-backup-*`/`runtime-candidate`/`.wsl-core-verify` 残留。dev 应用保持运行供 W6 使用。

### G.7 下一步（W6）

先完成 §H / REVIEW §F 的三项返修并复核 W6 入口，再执行 W6 全量验证与文档：`docs/spec/WSL_CORE.zh.md`、README 两处功能行、`pnpm tauri build`（**安装前停下向用户确认**）、本地提交并推送到 fork、推送触发的三平台 CI 与 ubuntu/macos 日志 `warning:` 核对。

## H. v8 执行情况审核与 W6 入口（2026-10-02）

**审核结论：暂不进入 W6。** §G 的正常路径和门禁结果保留；本轮代码复核与隔离脚本测试发现以下三项实现缺口，不能将 §G 的目录恢复三态等同于正式启动失败后的回滚。

| 编号 | 问题 | 状态 |
|---|---|---|
| R-V8-1（高） | 真实 restart/readiness 前已清备份；新 runtime 存在时 RESTORE 直接 SKIPPED，不能恢复启动失败的新树 | 待返修及验收 |
| R-V8-2（中） | 缺推荐配置时自愈回退已装版本；已装同版本缺资源时 early return 成功，与 v8 明确报错要求不符 | 待返修及验收 |
| R-V8-3（中） | VERIFY 无统一中断清理/实际 Linux 截止时间；隔离 SIGTERM 测试中验证子进程残留，已由审核清理 | 待返修及验收 |

本轮复跑：Rust 573 passed / 0 failed；32 个 Rust 文件 fmt 无 REGRESSION/CHURN；clippy exit 0、81 条 warning；typecheck 与局部 eslint exit 0。资源 manifest/lock 与已验证基线逐字一致。820 个源文件/资源等在审核文档修改前哈希不变；审核没有执行产品返修，也未关闭开发应用或操作业务 runtime。

详细修复步骤与验收：`E:/DSH-WSL/WSL-CORE-REVIEW.md` **§F**。证据：`E:/DSH-WSL/validation/v8-review-20261002/`。这些是落实已有 v8 要求的技术返修，不是新的产品方案裁决；完成后追加实际验收结果，再确认 W6 入口。

---

## I. R-V8-1/2/3 返修实施与验收（2026-10-02，执行端）

### I.0 结论

§H 三项返修完成并逐项实测（含受控失败与回滚）；§0 门禁冻结版复跑全绿。**执行端自验收记录如上；审核复验结论见 §J：R-V8-2/3 通过，R-V8-1 仍有两项未闭环，暂不进入 W6。**

### I.1 代码落点

| 位置 | 内容 |
|---|---|
| `src-tauri/src/service/wsl_core/switch.rs`（新） | 「待确认切换」标记（应用数据目录 `wsl-runtime-switch.json`：distro/dirName/stamp/version/lockSha256/switchedAt/startedAt/rollbackFailed）；`arm`（切换成功后登记）、`note_launch_started`（正式启动登记 = 宽限起点）、`note_health`（健康成功→确认；`HARNESS_NOT_OWNED` 或超宽限→回滚）、`on_launch_failure`（启动路径失败→回滚）、`rollback`（停服→失败新树移出 `runtime-failed-<stamp>`→备份恢复→刷新探测缓存；失败保留文件、置位不再重试）；`rollback_reason` 纯函数 + 单测 |
| `src-tauri/src/service/wsl_core/install.rs` | 资源**先加载校验再判定**（缺资源/不匹配不再被 no-op 早退绕过）；`default_version_spec(app)` 统一签名（删除 `installed` 兜底，R-W2-1 之外不再有静默保留）；窗口前房务 `CLEAN_RUNTIME_LEFTOVERS`（仅无待确认切换时）；切换成功后只 `switch::arm`，**不在安装内清理备份**；验证错误路径统一 `CLEANUP_VERIFY` + 丢弃候选（不再 `?` 绕过）；`VERIFY_TIMEOUT` = 截止 90 s + 45 s 收尾余量 |
| `src-tauri/src/service/wsl_core/script.rs` | `VERIFY_CORE`：真实总截止（`date +%s` 差值，非迭代数）、每次探测 `timeout -k 2 25`、`cleanup()` + `trap EXIT/TERM/INT/HUP`、pidfile 写 pid+boot_id；新增 `CLEANUP_VERIFY`（三重确认回收验证进程组、日志保留）、`ROLLBACK_RUNTIME`（失败新树移出 + 备份恢复 + 三步兜底，不因 `runtime` 存在而跳过）、`CLEAN_RUNTIME_LEFTOVERS`（房务）；`CLEAN_RUNTIME_BACKUP` 改为只清 `runtime-backup-$2` 本次槽 |
| `src-tauri/src/service/workflow/wsl_launch.rs` | `spawn` 成功后 `switch::note_launch_started`（无标记时 no-op） |
| `src-tauri/src/service/workflow/launch.rs` | WSL 分支（preflight→端口→spawn）整体包住，任一失败 → `switch::on_launch_failure` 回滚 |
| `src-tauri/src/bridge/system_os.rs` | `proxy_health_check` 之后 `switch::note_health`——确认/回滚挂在**既有 readiness 完成路径**，前端零改动（§0 规则 5） |
| `src-tauri/src/service/wsl_core/mod.rs`、`src-tauri/src/service/workflow/mod.rs` | 共享 `run_script`；导出 `stop_in_distro` 供回滚停服 |

### I.2 验收（真实结果）

| 场景 | 结果 |
|---|---|
| R-V8-2 缺推荐配置（已装同版本 + 基线匹配） | `WSL_DSH_VERSION_UNCONFIGURED`；runtime 树哈希不变 |
| R-V8-2 缺锁文件 | `WSL_RUNTIME_RESOURCE_INVALID: …package-lock.json: 系统找不到指定的文件`；哈希不变 |
| R-V8-2 无效资源（manifest 钉 0.1.1-rc.2、请求 0.1.2-rc.1） | `WSL_RUNTIME_MANIFEST_MISMATCH: manifest pins 0.1.1-rc.2, requested 0.1.2-rc.1`；哈希不变 |
| R-V8-2 资源完好 + 基线匹配 | 幂等 no-op（`install_wsl_core` 只回探测，无切换、无标记） |
| R-V8-1 确认路径 | 注入锁基线失配 → 切换成功：标记 `{stamp:1790924128, startedAt:null}` + `runtime-backup-1790924128` **保留**，日志 `backup kept until the service confirms readiness`；`set_active_core('wsl')` → 停服 → `launch_harness`（`startedAt` 写入）→ 首次 `proxy_health_check` 即 `healthy - 47/47` → `runtime switch confirmed … cleaning the previous runtime in background` + `CLEANED 1790924128`；标记清除、只清本次槽、服务保持运行 |
| R-V8-1 启动失败回滚（preflight 失败） | 旧树预置 `OLD-TREE-MARKER` 后切换；`chmod -x` 破坏新树 → `launch_harness` 返回 `WSL_DSH_NOT_INSTALLED` → 日志 `rolling back Ubuntu/1790924240` → `runtime rolled back to the previous tree; failed tree kept at ~/.dsh-desktop.dev/runtime-failed-1790924240`；`runtime` 为带标记的旧树、`dsh --version` 可用、失败树保留、标记清除 |
| R-V8-1 进程退出回滚 + 护栏 | 未启动时健康轮询 `HARNESS_NOT_OWNED` → **不回滚**（护栏 = `started_at` 为空，标记与备份原样）；启动后杀 dsh → 轮询 → `runtime switch not confirmed (the Harness process exited after the runtime switch); rolling back` → 回滚成功 |
| 房务清理 | 窗口开始实测 `CLEANED_LEFTOVERS 1`（清掉注入的假遗留槽）；有待确认切换时跳过（代码把关 + 单测） |
| 组件级脚本（T1–T6） | `VERIFY_CORE` 对真实 runtime `VERIFY_OK` 且临时目录清理；桩永不就绪时 deadline=8 s → 实测 9 s 结束、`VERIFY_FAILED`、进程组回收；验证 shell SIGTERM → 退出 143 且进程组回收；`SIGKILL`（模拟中继超时）后 `CLEANUP_VERIFY` → `VERIFY_CLEANED`、进程与临时目录回收、日志保留；`ROLLBACK_RUNTIME` 身份正确（runtime=OLD / runtime-failed=NEW）、备份缺失 `ROLLBACK_NOTHING`；`CLEAN_RUNTIME_BACKUP` 只清指定 stamp（另一槽保留）；`CLEAN_RUNTIME_LEFTOVERS` 清净且保留正式 runtime |

### I.3 §0 门禁（冻结版复跑）

| 命令 | 结果 |
|---|---|
| 1 `cargo test --all-features --locked` | **577 passed / 0 failed** |
| 2 fmt v4 | 无 REGRESSION / CHURN |
| 3a clippy 改动文件 | 21 处命中（20 个位置，与 §G 逐行相同，全部上游行）；新增/改动文件 0 命中 |
| 3b 全量告警 | **81 = 基线**，直方图与基线逐项一致 |
| 4 前端 | `dsh-tauri` / `dsh-tauri-ui` build（publint No issues）、typecheck、eslint 全过 |

（3b 期间一度出现脚本注释触发的 `doc_lazy_continuation`，改写注释后回到 81。）

### I.4 环境与边界

- 环境还原：dev store `setting` 与 v8 基线逐键一致（`active_core=app`、`port=3081`、`manual_port=null`、`wsl_distro=Ubuntu`；`window_state` 因窗口位置变化而异，非本功能字段）；runtime 树哈希 `e217870c…fb7f4`；`profiles/web` 两文件与凭据哈希与基线一致；无 `runtime-backup-*` / `runtime-failed-*` / `runtime-candidate` / `.wsl-core-verify` 残留。
- 设计边界：切换后**从未尝试启动**（既未启动也未探测失败）时标记与备份保留，由下次启动或下一次安装窗口接管；回滚失败置 `rollbackFailed` 并保留全部文件、不再自动重试，日志明确报告。
- 应用级测试通过 CDP 调用与前端相同的后端命令（`install_wsl_core` / `set_active_core` / `shutdown_harness` / `launch_harness` / `proxy_health_check`）驱动；前端启动状态机未改动。
- 实测发现（供审核）：资源文件改动会触发 Tauri dev 的**秒级增量重建并重启应用**（重命名资源后立即调用会命中重启窗口），资源类故障注入测试应在重启后调用。

## J. R-V8 返修审核复验（2026-10-02）

**结论：R-V8-2、R-V8-3 通过；R-V8-1 暂未闭环，W6 入口仍未满足。**

本轮复跑 Rust **577 passed / 0 failed**，34 个 Rust 文件 fmt 无差异，clippy exit 0 / 81 条 warning，typecheck 与局部 eslint 均通过。隔离脚本确认新树已存在时回滚有效、只清指定备份槽，VERIFY 截止/TERM/强杀后清理均能回收验证进程。产品代码未改动。

| 剩余项 | 证据与要求 |
|---|---|
| R-V8-1A 超时触发 | 实际前端轮询在 inactivity 180s / absolute 300s 结束，后端 360s 宽限只在后续健康请求中检查；无独立触发，进程存活但始终不就绪时无法保证回滚。需后端截止处置及对应测试 |
| R-V8-1B 目标身份 | 健康成功未匹配当前发行版/本次已启动 pending，只比较主包版本；可能清另一目标未确认的备份。全局单标记也不得被后续安装覆盖。需目标绑定与单 pending 保护 |

详细修复和验收：`E:/DSH-WSL/WSL-CORE-REVIEW.md` **§G**。证据：`E:/DSH-WSL/validation/v8-rereview-20261002/`。只继续剩余两项，不重做已经通过的 R-V8-2/3；完成后再复核进入 W6。

---

## K. R-V8-1A/1B 返修实施与验收（2026-10-02，执行端）

### K.0 结论

REVIEW §G 两项返修完成并逐项实测：**R-V8-1A**（前端停止轮询后的后端截止处置）与 **R-V8-1B**（健康成功绑定实际运行目标；单标记不被覆盖）。R-V8-2/3 代码路径未改动、保留 §J 结论。§0 门禁冻结版复跑全绿，环境已还原。**审核复验结论见 §L：独立截止触发通过；R-V8-1B 尚有请求身份与处置互斥缺口，暂不进入 W6。**

### K.1 代码落点

| 位置 | 内容 |
|---|---|
| `switch.rs`（R-V8-1A） | `note_launch_started` 写 `started_at` 后 `spawn(deadline_task)`，日志 `backend deadline armed at <unix>`；任务睡到 `deadline_at(started_at) = started_at + 360 s`，唤醒后先跑 `marker_still_current`（stamp + `started_at` + 发行版/数据目录一致、未 `rollbackFailed`——被确认 / 已回滚 / 被替换都直接退出并记 `the marker was already resolved … nothing to do`），再核对核心来源与目标身份，最后**自测一次** `proxy_health_check(port)`：健康 `try_confirm`、失败 `rollback`。`PENDING_GRACE` 保持 360 s（未改小）。 |
| `switch.rs`（R-V8-1B） | `PendingSwitch` 增加 `home`（Linux `$HOME` 绝对路径；`parse` 要求 `/` 开头）；`target_matches` 纯函数（store 发行版必须等于 pending 发行版；持有进程存在时其 (distro, dirName) 也必须一致）；`health_target_matches`（`get_store_dat_setting().wsl_distro` + `workflow::owned_wsl_target()`）；`runtime_baseline_matches`（UNC 读 `$HOME/<dir>/runtime/.dsh-runtime.json` 比 version + lockSha256，读不到 → `None` 不采信）；`try_confirm`（未启动 / 异目标 / 基线不匹配都只记日志、不清标记不删备份）；`confirm`、`rollback` 首次动作前复核 `marker_still_current`（与截止任务共享同一身份规则）；`arm` 见已有未完成标记 → `WSL_RUNTIME_SWITCH_MARKER_OCCUPIED` 拒绝覆盖；install 的「标记写入失败」兜底改走 `rollback_unarmed`（该切换从未登记、无身份可核）。**删除**「只比缓存版本字符串」的确认分支。 |
| `process.rs` / `workflow/mod.rs` | `owned_wsl_target() -> Option<(String, String)>`：持有进程的 (发行版, 数据目录)。 |
| `install.rs` | 候选窗口开始处（0a）：`switch::read` 有未完成切换时，同发行版**先 `rollback`** 恢复旧运行时再继续；`rollbackFailed` 或属于**另一发行版**则 `WSL_RUNTIME_SWITCH_UNRESOLVED` 明确拒绝（给出备份/失败树路径或另一发行版提示）；解决后才跑房务（0b）。`arm(...)` / `PendingSwitch::new(...)` 补 `home`。 |
| 单测 +3 | `target_matches_binds_to_store_distro_and_owned_process`、`marker_still_current_requires_same_identity`、`deadline_sits_after_frontend_polling_limits`（`PENDING_GRACE > 300 > 180`、`deadline_at` 饱和加）；`parse` 拒绝相对路径 `home`。 |

### K.2 验收（真实结果）

| 场景 | 结果 |
|---|---|
| **D1** 第二次更新不覆盖未确认记录（真实安装） | 合成 pending `r9x1790929369`（Ubuntu，未启动）+ 真实全量备份副本；篡改 runtime 锁基线触发更新窗口 → `install_wsl_core` 实测：`08:23:16 WARN unresolved runtime switch (stamp r9x…) found before install; rolling it back first` → `08:23:17 runtime rolled back to the previous tree (stamp r9x…)` → `08:23:17 leftover runtime slots cleaned before install (CLEANED_LEFTOVERS 1)` → `08:23:30 runtime switched to 0.1.2-rc.1 (stamp 1790929409); backup kept until the service confirms readiness`。新标记 stamp 换代（≠ 旧 stamp），旧备份先恢复进 runtime、再成为新备份槽——无任何「无记录覆盖」 |
| **D2** 另一发行版的未完成切换 | 合成 pending（`distro=Debian`）+ 强制更新 → `install_wsl_core` **1 s 内**返回 `WSL_RUNTIME_SWITCH_UNRESOLVED: a runtime switch for Debian is still awaiting confirmation (stamp r9debianx); launch that core once or resolve it before installing Ubuntu`；标记逐字不变、无备份/失败树产生、runtime 未动（`arm` 的 `MARKER_OCCUPIED` 拒绝为同语义的二道闸） |
| **A1** 慢失败 → 后端截止回滚（真实安装 + 真实启动 + 真实前端轮询） | T=08:36:04.971 日志 `pending switch 1790930110: backend deadline armed at 1790930524`；runtime 入口替换为「应答 `--version` 的存活哑进程」（不监听端口）→ 前端轮询持续失败（常量 reason `HARNESS_BOOT_MANIFEST_REQUEST_FAILED`），**末次轮询 08:39:05.529 = T+180.6 s**（inactivity 停止，之后无任何健康调用）；后端 **08:42:04.985 = T+360.014 s** `reached its backend deadline; evaluating readiness` → `08:42:07.030 is not ready at its deadline (…); rolling back` → `08:42:08.867 runtime rolled back to the previous tree (stamp 1790930110); failed tree kept at ~/.dsh-desktop.dev/runtime-failed-1790930110`；标记清除、哑进程被 STOP 回收（`Owned Harness process … exited with code 15`）、runtime 恢复 |
| **A2** 同构复跑 + 树身份校验 | T=08:45:19.905 armed（1790931079）→ 前端末次 08:48:20.406（+180.5 s）→ 08:51:19.918（+360.013 s）截止 → 08:51:23.705 回滚。**安装前 H0 与回滚后 H1（24982 文件全量 sha256 汇总）逐位相等**（`287b4f60…`）——恢复的正是切换前那棵树（H0 含本轮注入用篡改基线文件；随后恢复基线文件，树哈希回到 E2E 前基线 `b2468971…`） |
| **B** 正常就绪：截止前确认、迟到任务不回滚 | T=08:27:28.797 armed（1790930008）；08:27:31.696 健康成功 → `runtime switch confirmed (0.1.2-rc.1 stamp 1790929624)`；08:27:32.829 `cleanup finished (CLEANED 1790929624)`；**08:33:28.802 = T+360.005 s** `switch deadline for stamp 1790929624 reached; the marker was already resolved (confirmed / rolled back / replaced); nothing to do`——不碰任何文件；无 `runtime-failed-*`、服务保持健康 |
| **C1** 未启动 pending 不被确认 | pending `c1ubuntu`（`startedAt` 缺省）+ 假备份槽；真实服务 `proxy_health_check → healthy - 47/47` 后标记与备份**原样保留** |
| **C2** 异目标同版本健康不被确认 | pending `c2debian`（`startedAt` 已写、version 相同）+ 假备份；active 为 Ubuntu 且健康 `47/47` → 标记/备份原样保留（对应审核「pending Ubuntu + active Debian 同版本健康」） |
| **C3** 基线不匹配不被确认 | pending `c3badver`（version 9.9.9）→ 健康 `47/47` 后 `WARN runtime baseline does not match pending switch c3badver; keeping the marker and the previous runtime`；标记/备份保留 |
| **C4** 正确目标+基线才清本次备份 | pending `c4confirm` + 假备份槽 → `runtime switch confirmed (… stamp c4confirm)` + `cleanup finished (CLEANED c4confirm)`；只清本次槽（C1/C2/C3 的假槽在各自场景中均未被触碰，最终由验收脚本手工清理） |
| **C4-neg**（意外获得的反向样本） | 同 C4 但 runtime 锁基线仍是被 A2 现场篡改的值 → `WARN runtime baseline does not match pending switch c4confirm; keeping …`，标记/备份保留；恢复基线文件后同一次调用即转为确认 |
| **180/300 两条路径** | 前端 inactivity 停止实测 T+180.6 s（A1）/ T+180.5 s（A2）；absolute 300 s 由审核端 §G.1 实测（298.125 s 停止）。后端截止为**绝对时刻**（实测 +360.014 / +360.013 / +360.005 s），与前端停止模式无关，两条路径均严格早于截止；单测断言 `PENDING_GRACE(360) > 300 > 180` |

### K.3 §0 门禁（v4 命令，全部实际复跑）

| 命令 | 结果 |
|---|---|
| 1 `cargo test --all-features --locked` | **580 passed / 0 failed**（§J 的 577 + 本轮 3 条新单测） |
| 2 fmt v4 | 34 个改动/新增 Rust 文件，**无 REGRESSION / 无 CHURN** |
| 3a clippy 改动文件 | 21 处命中（20 个位置），逐处核对为 upstream 同文件**原文行**；新文件（`wsl_core/**`、`bridge/wsl_core.rs`）**0 命中** |
| 3b 全量告警 | **81 = 基线**；直方图逐项与基线一致 |
| 4 前端 | `pnpm --filter dsh-tauri build && pnpm --filter dsh-tauri-ui build`（publint No issues）、`pnpm run typecheck` exit 0、§0 列出的 8 个文件 eslint exit 0（应用自带 pnpm 10.28.2 直跑） |

### K.4 环境还原

- dev store `setting` 与 E2E 前备份（`.store.dev.dat.bak-r9-20261002`）**JSON 逐键相等**（`active_core=app`、`port=3081`、`manual_port=null`、`wsl_distro=Ubuntu`）。
- runtime 树哈希 `b2468971…`（24982 文件 / 324 M，`find -type f` 全量 sha256 汇总）**= E2E 前基线**；`runtime/.dsh-runtime.json` 为真实值；`.bin/dsh --version` = 0.1.2-rc.1。
- 无 `wsl-runtime-switch.json`、无 `runtime-backup-*` / `runtime-failed-*` / `runtime-candidate`；WSL 无 dsh 进程、pid 文件已清；app 核心 59/59 健康，dev 应用保持运行。

### K.5 验收方法与边界（供审核）

- 应用级测试经 CDP 调用与前端相同的后端命令（`install_wsl_core` / `set_active_core` / `shutdown_harness` / `launch_harness` / `proxy_health_check`）；A 组的「前端轮询」由窗口重载触发**真实前端 boot**（`setup → boot → launchAndWait → pollReadiness`），停止后无任何健康调用。
- 慢失败注入：把 runtime 入口（`.bin/dsh` 或其符号链接目标 `bin.js`）换成「应答 `--version`、之后存活但不监听端口」的哑脚本——探测/预检通过、正式启动成功、健康永远失败；回滚时 STOP 按 pid 文件 + boot_id + cmdline `dsh --profile` 三重确认后按进程组回收（实测退出码 15）。
- 1B 的「另一目标 / 未启动」场景用**合成标记 + 真实健康服务**模拟（无需在验证机装第二个发行版）；「正确目标 + 基线」在 C4 与 B 的真实确认路径均覆盖。
- 强制进入真实安装窗口：把 `runtime/.dsh-runtime.json` 的锁哈希改为不匹配值（热缓存下每轮 `npm ci` + 校验 + 切换约 13–15 s）；每次窗口结束后恢复真实值。
- 环境注意（本轮实测新增）：Tauri Store（`.store.dev.dat`）在进程内**缓存整份设置**——直接改文件后需应用重启才会被读到，否则下一次应用写入会覆盖回去；WSL 内 `ls` 的时区显示与 Windows 不同（脚本一律用 Unix 时间戳比较）；dev watcher 对源码文件的 touch 也会触发重建并重启应用（本轮借此完成 store 重读）。

---

## L. R-V8-1A/1B 审核再复验（2026-10-02）

**当前结论：暂不进入 W6。R-V8-1A 独立截止触发通过；R-V8-1B 继续完成两项返修。**

| 项 | 本轮审核结果 |
|---|---|
| 独立截止触发 | 生产 `switch.rs` 的测试时钟复验：前端 180/300 秒停止请求后，截止任务仍自测并派发回滚；截止前确认后不再回滚。§K 的正常路径记录有实现依据 |
| R-V8-1B-1 请求身份 | bridge 只在探测前取端口，结果返回后才读取 pending/store/owned。隔离复现旧目标/旧启动成功确认新 pending；失败分支没有目标检查，Debian 失败可触发 Ubuntu 回滚 |
| R-V8-1B-2 处置互斥 | 回滚 guard 不约束 confirm。隔离复现 STOP 等待期间确认清备份，随后回滚得到 ROLLBACK_NOTHING；实际 WSL 脚本复测同样保持 new 树且旧备份已丢失 |
| 门禁 | Rust **580 passed / 0 failed**；34 个 Rust 文件 fmt 无 REGRESSION/CHURN；clippy exit 0、81 warning，改动文件 21 处命中均对应上游原文行；typecheck 与局部 eslint exit 0 |
| 范围 | 生产代码未修改；821 个源码/资源文件在审核文档更新前哈希不变。故障编排使用隔离桩，实际脚本只操作独立测试目录，未对业务实例注入故障 |

具体落点、复现顺序、最小修复与验收要求见 `E:/DSH-WSL/WSL-CORE-REVIEW.md` **§H**。证据位于 `E:/DSH-WSL/validation/r-v8-1ab-review-20261002/`。R-V8-2/3 保留通过结论；仅继续 R-V8-1B-1/2，完成后再复核 W6 入口。

---

## M. R-V8-1B-1/2 返修实施与验收（2026-10-02，执行端）

### M.0 结论

REVIEW §H 两项返修完成并逐项复跑：**R-V8-1B-1**（健康结果绑定探测**发起前**的上下文；成功、失败与截止自测共用同一身份规则）与 **R-V8-1B-2**（确认 / 回滚 / 安装前解决与登记共享处置互斥）。组件级 harness（与审核侧同构）15 用例 + 应用级真实接线 4 用例全部符合预期；§0 门禁全绿（**581 passed**）；环境已还原。**审核复验见 §N：原 15 场景通过，截止自测期间原进程退出的收尾仍有缺口，暂不进入 W6。**

### M.1 代码落点

| 位置 | 内容 |
|---|---|
| `switch.rs`（R-V8-1B-1） | 新增 `HealthClaim`（探测发起前固定：核心来源 / store 发行版 / 端口 / 持有进程 / pending 的 stamp + startedAt）与 `claim_health_target(app, port)`；`claim_matches_pending`（截止自测**发起前**确认「要测的就是本次切换」）与 `claim_resolves`（完成后 claim ↔ 当前世界 ↔ 当前标记三方核对）为可单测纯函数；`note_health(app, claim, result)` 改按 claim 处置——旧请求（启动身份换代）、另一目标的成功/失败、已解决标记一律忽略；失败路径与成功、截止自测共用这一份规则（「本次持有实例已退出」由**下一次探测在正确目标上下文中重新绑定**，不用「当前任意实例不在」代替）。 |
| `switch.rs`（R-V8-1B-2） | 处置互斥 `disposal()`（`OnceLock<tokio::sync::Mutex<()>>`）：`confirm` 用 `try_lock`（另一路处置进行中直接放弃——回滚正在恢复的备份绝不清掉）；`rollback` / `rollback_unarmed` / `arm` / `note_launch_started` 阻塞获取后**复核身份**再动作；`rollback_now` 收尾只在标记仍是自己持有的记录时才 `clear` / 落 `rollbackFailed`（换代标记一概不碰）。**删除** `ROLLBACK_IN_FLIGHT` / `RollbackGuard`（原子标志由互斥取代，安装前解决 / 登记一并纳入同一互斥）。 |
| `bridge/system_os.rs` | `proxy_health_check` 在 `await` 探测**前**取 claim，完成后 `note_health(app, &claim, &result)`——不再在完成时才读 pending/store。 |
| `install.rs` | 第 8 步 `switch::arm(...)` 直接 `await`（arm 内部持有处置互斥，拒绝覆盖任何未完成旧切换）。 |
| 单测 +1 | `claim_binds_probe_to_request_time_identity`：异目标、迟到请求、换代标记、端口 / 发行版 / 持有进程变化、未启动、非 WSL 来源全部不采信；「已退出（`owned=None`）的正确目标」可采信且 `HARNESS_NOT_OWNED` 立即回滚；`claim_matches_pending` 同覆盖。 |

### M.2 组件级 harness（与审核侧同构，15 用例全通过）

方法学与 `validation/r-v8-1ab-review-20261002/switch-harness` 一致：`#[path]` 直接加载生产 `switch.rs`；bridge 按生产原文提取（仅去 `#[tauri::command]`）；AppHandle/store、HTTP、进程停止与脚本执行由受控桩 + 异步闸门替代；假时钟推进到 361 s 跨过定时器边界（不改生产 360 s 常量）。

| 用例 | 规则 | 观察（`results.json`） |
|---|---|---|
| control_unstarted_preserved | §K 保留 | 标记/备份保留，无 CLEAN |
| control_other_target_success_preserved | §K 保留 + 单 pending | `MARKER_OCCUPIED` 拒绝覆盖 |
| bridge_confirm_happy_path | 正常路径不误伤 | `PROBE → CLEAN → CLEANED`，标记清除 |
| exit_of_pending_target_rolled_back | 正确实例退出仍回滚 | `PROBE → STOP → ROLLBACK_OK`，标记清除 |
| baseline_mismatch_kept | §K C3 保留 | 标记/备份保留 |
| **wrong_target_failure_kept_ubuntu** | **H.1 #1**：Debian 失败不得回滚 Ubuntu | 仅 `PROBE Debian:44001`，标记/备份保留（旧编排：STOP+ROLLBACK Ubuntu） |
| **stale_target_success_kept_ubuntu** | **H.1 #2**：返回前世界切到 Ubuntu:44002 | 仅 `PROBE Debian:44001`，无 CLEAN（旧编排：CLEAN Ubuntu） |
| **stale_attempt_success_kept_new_attempt** | **H.1 #3**：重新登记启动后的旧成功 | 标记保留、无 CLEAN（旧编排：确认新尝试） |
| **confirm_during_rollback_skipped** | **H.2**：STOP 中确认必须放弃 | `try_confirm=false`；备份在恢复前存在；`ROLLBACK_OK` 完整执行；无 CLEAN |
| **confirmed_switch_not_rolled_back** | **H.2**：已确认后回滚不得停服 | `CLEAN/CLEANED` 后 `rollback()` 直接返回，无 STOP/ROLLBACK |
| **late_finalization_kept_new_marker** | **H.2**：迟到收尾不改换代标记 | 新标记原样保留、未落 `rollbackFailed` |
| **arm_waited_for_rollback** | **H.2**：登记与进行中的回滚串行化 | arm 等待互斥、回滚释放后才写入 |
| `deadline_after_frontend_180` / `_300` | R-V8-1A 保留 | 假时钟跨过 360 s 后标记清除、runtime=old |
| confirmed_switch_not_rolled_back_at_deadline | R-V8-1A 保留 | 无 ROLLBACK 事件 |

### M.3 应用级 E2E（真实构建 + 真实 WSL 现场）

| 用例 | 结果 |
|---|---|
| **E1** 正常确认 | 真实 WSL 服务（47/47）+ 合成 pending `1790938018` + 假备份槽 → `10:47:00.335 runtime switch confirmed (0.1.2-rc.1 stamp 1790938018)`；`10:47:00.491 previous runtime cleanup finished (CLEANED 1790938018)`；标记与假槽均消失 |
| **E4** 未启动保持 | pending `1790938034`（无 `startedAt`）+ 健康 47/47 → `10:47:16.166 health result ignored: it does not match the pending switch captured at request start`；标记/假槽原样 |
| **E2** 实例退出回滚（真实脚本 + 真实备份恢复） | 停服 → H0=`b2468971…`（24982 文件全量 sha256 汇总）→ `cp -a runtime runtime-backup-1790938064` → 注入 `R10-E2-MUTATION.txt`（H1=`fc80e1d5…`）→ pending `1790938064` → `10:47:59.959 runtime switch not confirmed (the Harness process exited after the runtime switch); rolling back Ubuntu/1790938064`；`10:48:00.551 runtime rolled back to the previous tree (stamp 1790938064); failed tree kept at ~/.dsh-desktop.dev/runtime-failed-1790938064`；恢复树哈希重算 = `b2468971…`（=H0），变异文件在失败树、已不在 runtime；标记清除 |
| **E3** 异目标失败保持（H.1 #1 应用级同构） | store 发行版切 Debian（服务已停）→ pending Ubuntu `1790938146`（已启动）+ 假备份槽 → 健康 `HARNESS_NOT_OWNED` → `10:49:07.641 health result ignored: …`；标记/假槽原样、无 STOP/ROLLBACK（旧编排会回滚 Ubuntu） |

证据：`E:/DSH-WSL/validation/r-v8-1b12-fix-20261002/`（`switch-harness/` + `results.json` + `app-e2e/wsl-core-tail.log` + `gates/`）。E2 的 H0 文件落在 WSL `/tmp`（tmpfs），其后 VM 空闲重启导致文件丢失，证据以「打印的 H0 值 = 恢复后重算的 H2 值（均 = 基线 `b2468971…`）」为准。

### M.4 §0 门禁（v4 命令，全部实际复跑）

| 命令 | 结果 |
|---|---|
| 1 `cargo test --all-features --locked` | **581 passed / 0 failed** |
| 2 fmt v4 | 34 个改动/新增 Rust 文件，0 REGRESSION / 0 CHURN |
| 3a clippy 改动文件 | 21 处命中（20 位置），逐处为 upstream 原文行；新文件（`wsl_core/**`）0 命中 |
| 3b 全量告警 | **81 = 基线** |
| 4 前端 | 两个共享包 build（publint No issues）、`typecheck` exit 0、8 文件 eslint exit 0 |

### M.5 环境还原

- dev store 与 E2E 前备份（`.store.dev.dat.bak-r10-20261002`）**JSON 逐键相等**（`active_core=app`、`port=3081`、`manual_port=null`、`wsl_distro=Ubuntu`）。
- runtime 树哈希 = 基线 `b2468971…`；无 `wsl-runtime-switch.json`、无 `runtime-backup-*` / `runtime-failed-*`；WSL 无 dsh 进程；app 核心 59/59 健康，dev 应用保持运行（本轮代码已重建生效）。

### M.6 验收方法与边界（供审核）

- 组件 harness：三处受控闸门（HTTP 返回、STOP、假时钟）+ 「探测期间切换世界」的直接状态改写，逐一复现 §H 所列交错；断言覆盖「标记/备份是否保持」「事件顺序中是否出现 STOP / ROLLBACK / CLEAN」。
- 应用级 E1–E4 经 CDP 调用与前端相同的后端命令；E2 用真实 `ROLLBACK_RUNTIME` 脚本完成「失败树移出 + 备份移回」并按树哈希核对逐位恢复；E3 用 `update_app_config` 真实切换 store 发行版。
- 接口变更（供审核侧 harness 适配）：`note_health(app, claim, result)` + 新增 `claim_health_target`；`arm` 变为 `async`；`ROLLBACK_IN_FLIGHT` 删除（由 `disposal()` 取代）。
- 本轮未改动脚本（`script.rs`）、R-V8-1A 截止任务语义与 R-V8-2/3 代码路径；未在业务实例注入故障（应用级 E1–E4 使用合成标记 + 真实服务/真实脚本，均在验证后还原）。

---

## N. R-V8-1B-1/2 审核复验与 W6 入口（2026-10-02）

**当前结论：暂不进入 W6，仅继续 R-V8-1A-EXIT。** §H 两项返修的原列问题已通过独立复测；请求上下文与处置互斥保留，R-V8-2/3 保留通过结论。

- 生产 Rust **581 passed / 0 failed**；34 文件 fmt 无 REGRESSION/CHURN；clippy 81 条 warning、直方图与上轮一致且无新增源代码行命中；typecheck、局部 eslint 通过。
- 执行端的 **15 个组件场景全部独立复跑通过**，应用级 E1–E4 关键日志与记录相符。
- 新增复测发现：前端在 180/300 s 停止请求后，若原进程在截止自测期间退出，`claim_resolves` 因 owned 从 Some 变 None 拒绝结果，截止任务直接返回。测试时钟到 T+721 s 仍无回滚；只有人为追加一次健康请求才恢复。尚未健康时前端退出事件也不会补发请求。
- 审核未修改产品代码或业务运行时；文档更新前 821 个仓库文件哈希与本轮起点一致。新增交错使用直接加载生产代码的隔离 harness，不冒充业务实例端到端故障验收。

具体复现、最小返修和验收见 `E:/DSH-WSL/WSL-CORE-REVIEW.md` **§I**；证据：`E:/DSH-WSL/validation/r-v8-1b12-review-20261002/`。只补同一实例在截止自测中退出的自主收尾，不重做原两项方案或已通过用例；通过后再确认 W6 入口。

---

## O. R-V8-1A-EXIT 截止自测期间实例退出的收尾实施（2026-10-02）

对应 REVIEW **§I.1** 的返修要求：截止处置不得依赖前端 / 人工再发请求；对「原 pending / 目标 / 启动身份未变，且本次实例已退出而非被新实例替代」的情况，在截止路径中做到受身份约束的失败处置；不普遍放宽 owned 比较、不重新采信旧成功结果；沿用已有共享互斥。

### O.1 生产代码改动

| 位置 | 内容 |
|---|---|
| `switch.rs` 新增 `exit_rebind_matches`（纯函数） | 截止**失败**路径的重绑定判定：claim 必须是本次启动尝试（stamp / 启动时刻一致）、发起探测时持有进程就是 pending 的目标（排除异目标 / 未启动的结果）、**现在已无持有进程**（本次实例退出；被新实例替代——owned 仍为某个值——与异目标一概不算）、标记仍是这次切换（未确认 / 未回滚 / 未换代 / 未重新登记 / 未回滚失败）、核心来源仍是 WSL 且 store 发行版与端口未变。只放宽「owned 由本次目标 → `None`」这一种情形。 |
| `switch.rs` `deadline_task` 失败分支 | `resolve_claim` 不成立时先按上述规则重绑定（`rebind_exited_target` 读取当前世界与标记，只读、不改状态）再处置；重绑定成功记独立日志（`the owned instance exited while the deadline probe was in flight; disposing of the failure under the pending identity`），随后仍由 `rollback` 在共享处置互斥与其自身的标记复核下执行。成功分支不变（退出后的成功照旧不确认）。 |

### O.2 组件 harness（21 用例全部通过；原 15 场景保留）

| 用例 | §I 复现的对应点 | 本轮观察 |
|---|---|---|
| `frontend_180_exit_in_deadline_probe_auto_rollback` | 前端 180 s 停止轮询、截止自测**期间** owned 由本次目标 → None、探测返回普通失败 | **无需任何额外请求**：`PROBE → STOP Ubuntu → ROLLBACK_OK`，runtime=old、标记清除、备份消耗（复现基线：仅一次 PROBE、全部保留） |
| `frontend_300_exit_in_deadline_probe_auto_rollback` | 同上（300 s 路径） | 同上 |
| `exit_probe_replaced_instance_kept` | 防误伤：owned 被另一实例替代（仍为 Some） | 不处置；标记 / 备份原样（owned 比较不放宽） |
| `exit_probe_replaced_marker_kept` | 防误伤：探测期间标记被换代 | 新标记原样保留，无 STOP / ROLLBACK |
| `exit_probe_relaunched_attempt_kept` | 防误伤：探测期间同一切换重新登记启动（身份刷新） | 旧结果不处置，标记保留 |
| `exit_probe_success_not_confirmed` | 回归：探测期间退出后返回成功 | 不确认、不清洗（成功不重绑定） |

### O.3 §0 门禁（v4 命令，全部实际复跑）

| 命令 | 结果 |
|---|---|
| 1 `cargo test --all-features --locked` | **582 passed / 0 failed**（§M 的 581 + 新增 `exit_rebind_only_covers_own_instance_exit_under_pending_identity`） |
| 2 fmt v4 | 34 个改动/新增 Rust 文件，0 REGRESSION / 0 CHURN |
| 3a clippy 改动文件 | 21 处命中（20 位置），逐处为 upstream 原文行；新文件（`wsl_core/**`）0 命中 |
| 3b 全量告警 | **81 = 基线** |
| 4 前端 | `dsh-tauri` / `dsh-tauri-ui` build（publint No issues）、`typecheck`、8 文件 eslint 全 exit 0 |

### O.4 环境与边界

- 本轮仅组件级 harness（`#[path]` 直接加载生产 `switch.rs`，HTTP / 进程 / 脚本为隔离桩，假时钟推进 180 / 300 / 361 s）与只读门禁；未改动脚本、R-V8-1B 的确认 / 互斥路径与 R-V8-2/3 代码；未在业务实例注入或等待该故障（与审核侧同一验证口径）。
- 应用数据目录与业务 runtime 未触碰；dev 应用保持运行。

证据：`E:/DSH-WSL/validation/r-v8-1a-exit-fix-20261002/`（`switch-harness/` + `results.json` + `switch-harness.log` + `gates/`）。

---

## P. R-V8-1A-EXIT 审核复验与 W6 入口（2026-10-02）

**当前结论：暂不进入 W6；仅补重绑定入口的当前核心来源校验。**

- 独立复跑执行端 **21 个组件场景全部通过**，包括 180/300 秒停止后、截止自测期间实例退出的自动回滚。
- `rebind_exited_target` 未检查当前 `is_wsl_active(app)`；`claim.wsl_active` 只代表探测发起时。隔离复现自测在途切到非 WSL 后，仍 STOP/ROLLBACK 旧 Ubuntu 并清 pending，未落实 §O.1 声明的“当前核心仍是 WSL”。
- 最小返修：重绑定入口补当前来源校验；探测前/在途切走核心均应忽略，当前仍为 WSL 的退出场景仍应回滚。详见 `E:/DSH-WSL/WSL-CORE-REVIEW.md` **§J**。
- 本轮 Rust **582 passed / 0 failed**；34 文件 fmt 无 REGRESSION/CHURN；clippy 81 条 warning、直方图与上轮一致，无新增源代码行命中；typecheck 与局部 eslint 通过。
- 产品代码及业务现场未改；文档更新前 821 个仓库文件哈希不变。证据：`E:/DSH-WSL/validation/r-v8-1a-exit-review-20261002/`。

本项仍属于 R-V8-1A-EXIT 的身份保护，不新增产品裁决；补齐并复验后再确认 W6 入口。

---

## Q. R-V8-1A-EXIT 当前核心来源校验修复实施（2026-10-02，执行端）

对应 REVIEW **§J.1** 的最小返修要求：`rebind_exited_target` 入口补上与 `resolve_claim` 相同的**当前**核心来源校验；不得用 claim 里探测发起时的历史布尔值替代当前查询；不新增状态机、不放宽 owned / 标记匹配。

### Q.1 生产代码改动

| 位置 | 内容 |
|---|---|
| `switch.rs` `rebind_exited_target` 入口 | `!crate::service::core::is_wsl_active(app)` → `None`（与 `resolve_claim` 完全一致）。用户从 WSL 切到 app/local 会停掉原服务，但已选发行版与端口保持不变；此前该分支会把主动切换造成的停服当成「本次实例退出」而回滚旧树。纯函数 `exit_rebind_matches`、owned / 标记匹配、处置互斥与截止任务编排均未改动。 |

### Q.2 组件 harness（23 用例全部通过；原 21 场景保留）

| 用例 | §J 对应的复现点 | 本轮观察 |
|---|---|---|
| `source_changed_before_probe_ignored` | 对照：截止探测**前**切走核心 | 无任何事件；标记 / 备份保留 |
| `source_changed_during_probe_ignored` | **§J.1 复现（在途切走）**：探测发出后 `wsl_active=false`、owned→None，失败结果随后放行 | **不处置**：仅 `PROBE Ubuntu:44001`，无 STOP / ROLLBACK / CLEAN；标记 / 备份 / runtime=new 原样（复现基线：`PROBE → STOP → ROLLBACK_OK`、标记清除） |

回归：`frontend_{180,300}_exit_in_deadline_probe_auto_rollback`（当前仍为 WSL 的实例退出）继续自动回滚，修复未误伤；原 21 场景全部通过。

### Q.3 §0 门禁（v4 命令，全部实际复跑）

| 命令 | 结果 |
|---|---|
| 1 `cargo test --all-features --locked` | **582 passed / 0 failed** |
| 2 fmt v4 | 34 个改动/新增 Rust 文件，0 REGRESSION / 0 CHURN |
| 3a clippy 改动文件 | 21 处命中（20 位置），与上轮一致（均为 upstream 原文行；本轮改动文件不在命中集合）；新文件（`wsl_core/**`）0 命中 |
| 3b 全量告警 | **81 = 基线** |
| 4 前端 | `dsh-tauri` / `dsh-tauri-ui` build（publint No issues）、`typecheck`、8 文件 eslint 全 exit 0 |

### Q.4 环境与边界

- 本轮仅组件级 harness（`#[path]` 直接加载生产 `switch.rs`；`is_wsl_active` 桩改为可变化状态以复现来源切换；HTTP / 进程 / 脚本仍为隔离桩，假时钟推进 361 s）与只读门禁；未在业务实例注入或等待该故障（与审核侧同一验证口径）。
- 应用数据目录与业务 runtime 未触碰；dev 应用保持运行。

证据：`E:/DSH-WSL/validation/r-v8-1a-exit-source-fix-20261002/`（`switch-harness/` + `results.json` + `switch-harness.log` + `gates/`）。

---

## R. R-V8-1A-EXIT 审核通过与 W6 入口确认（2026-10-02）

**当前结论：W6 入口条件满足，可以进入 W6 全量验证与文档；入口返修项已闭环。**

- 当前核心来源检查已在 `rebind_exited_target` 入口生效。独立复测“探测前切走”和“在途切走”均保持原 pending/备份，不派发 STOP/ROLLBACK/CLEAN。
- 当前仍为 WSL 时，180/300 秒停止请求后的实例退出仍能自动回滚。审核侧 **23 个组件场景全部通过**，原请求身份、互斥、基线和单标记保护继续通过。
- 生产 Rust **582 passed / 0 failed**；34 文件 fmt 无 REGRESSION/CHURN；clippy 81 条 warning、直方图与上轮一致，无新增源代码行命中；typecheck、局部 eslint 通过。
- 产品代码和业务现场未改；文档更新前 821 个仓库文件哈希不变。证据：`E:/DSH-WSL/validation/r-v8-1a-exit-source-review-20261002/`；详细审核：`E:/DSH-WSL/WSL-CORE-REVIEW.md` **§K**。

下一步按方案 W6.1–W6.4 做全量检查、打包、文档，完成本地提交并推送到 fork `Sukvii/deepseek-harness-desktop` 的 `origin/feat/wsl-core`，核对推送触发的三平台 CI。本次不代表 W6 已完成；**运行安装包前仍需用户确认**。

---

## S. W6 全量验证与文档实施（2026-10-02，执行端）

按 `DSH-WSL-CORE-PLAN.md` W6.1–W6.4 执行；本记录随实现提交。

### S.1 全量门禁（W6.1）

| 项 | 结果 |
|---|---|
| `cargo test --all-features --locked` | **582 passed / 0 failed** |
| fmt v4 | 34 个改动/新增 Rust 文件，0 REGRESSION / 0 CHURN |
| clippy 3a（改动文件） | 21 处命中（20 位置），均为 upstream 原文行；`wsl_core/**` 等新文件 0 命中 |
| clippy 3b（全量） | **81 = 审核基线**；按 lint 名直方图与基线逐项一致 |
| `pnpm run lint` | **0 error** / 23 条既有 warning |
| `pnpm run typecheck` | exit 0 |
| `pnpm run test --run`（vitest） | **166 passed / 166**（23 个测试文件） |
| `pnpm run build` | exit 0 |

全量 lint 首次覆盖仓库后暴露并修复了 11 个 error（2 个文件，均为本分支新增内容的格式问题）：

- `src-tauri/docs/WSL-CORE-DECISIONS.md` 1 处 markdown：行内裸 `_300` 被解析为强调标记 → 用例名改用反引号包裹。
- `src-tauri/resources/wsl-runtime/0.1.2-rc.1/package.json` 10 处 jsonc：对象空格风格 / `overrides` 键序 / 文件末尾换行 → 按仓库 lint 规则重排格式。**语义不变**：与改动前及 `validation/wsl-live-20261001/package.json` 三方 JSON 解析逐键相等；`package-lock.json` 未改动，锁 SHA-256 仍为 `46d0671d…fbe54`；新的 manifest SHA-256 = `1a74891b…8398`。

### S.2 安装包（W6.2）

`pnpm tauri build`（release）产物：

| 产物 | 大小 |
|---|---|
| `bundle/msi/Deepseek Harness Desktop_0.11.2_x64_en-US.msi` | ~11.6 MB |
| `bundle/msi/Deepseek Harness Desktop_0.11.2_x64_zh-CN.msi` | ~11.6 MB |
| `bundle/nsis/Deepseek Harness Desktop_0.11.2_x64-setup.exe` | ~7.3 MB |

安装包内已包含 WSL 受控运行时资源（MSI `main.wxs` 含 `wsl-runtime\0.1.2-rc.1\package.json` 与 `package-lock.json` 条目）。**运行安装包前将停下向用户确认**；本节不代表已安装或已发布。

### S.3 文档（W6.3）

- 新增 `docs/spec/WSL_CORE.zh.md`：架构图（桌面壳 → wsl.exe 中继 → dsh）、数据目录、安装/更新/回滚流程、已知限制（内置插件缺席、Mirrored/NAT 网络模式实测说明、需自备 Node ≥ 20、`--skip-auth` 补丁随 dsh 升级需重打、首次安装需网络）。
- `README.md` / `README.en.md` 功能列表各加一行「WSL 核心 / WSL core」。
- 本文件（含本节）随实现提交。

### S.4 本地提交与推送（W6.4）

按阶段拆分提交（提交范围如下；具体 hash 见分支历史）：

| 提交 | 范围 |
|---|---|
| 配置模型与核心列表 | W1：`Setting.wsl_distro`、`CoreSource::Wsl`、核心列表行与文案 |
| WSL 核心服务层 | W2–W4：探测/安装/补丁/启动/停止/切换与回滚、bridge 命令、边界与降级 |
| 设置页 UI | W5：WSL 核心面板、hooks、toast 修复与 i18n |
| 受控运行时资源 | W5-R v8：`resources/wsl-runtime/0.1.2-rc.1/` 与打包声明 |
| 文档与 CI | W6：`docs/spec/WSL_CORE.zh.md`、README × 2、本决策记录、`.github/workflows/ci.yml` 分支触发 |

`git status` 变更范围与 PLAN W6.4 允许清单一致（`src-tauri/src/**`、`src-tauri/resources/wsl-runtime/**` 及资源声明、前端组件/hooks、i18n 两个 json、文档、CI 配置）；未触碰 `packages/dsh-tauri-wsl/`。推送到 fork `Sukvii/deepseek-harness-desktop` 的 `origin/feat/wsl-core`；三平台矩阵与 ubuntu / macos 新增 `warning:` 核对结果记录于 `E:/DSH-WSL/validation/w6-20261002/`。

### S.5 三平台 CI 核对与 R-W3-9 修复（2026-10-02）

**第一次推送**（`f5a32a3`，run `37026086203`）：三平台全部通过；但对照基线（`fe056f8` 的 upstream run `34480406119`）发现 **ubuntu / macos 新增 12 项 dead_code 告警**——WSL 核心的若干函数 / 常量 / 枚举变体只在 Windows 调用路径可达，非 Windows 构建下不可达属预期（R-W3-9 预先安排在本阶段核对）。

修复（`49f5ce8`）：按 **R-W4-5 先例**逐项加 `#[cfg_attr(not(windows), allow(dead_code))]` 并注明原因，共 4 个文件 12 处——`exec.rs`（`WslStream` 变体）、`probe.rs`（`cached_fresh`）、`script.rs`（`START` / `STOP`）、`switch.rs`（`PendingSwitch::matches`、`claim_matches_pending`、`exit_rebind_matches`、`rebind_exited_target`、`deadline_at`、`note_launch_started`、`deadline_task`、`on_launch_failure`）。Windows 行为不变（属性在 Windows 下不生效；本机 `cargo test` 582 passed、clippy 81 = 基线）。

**第二次推送**（`49f5ce8`，run `37027390404`）核对结果：

| 平台 | 结果 |
|---|---|
| ubuntu-22.04 | 27 行 warning = 基线 27 行，**无新增**（原 12 项具体告警全部消除） |
| macos-14 | 28 行 = 基线 28 行，**无新增具体告警**；仅 cargo 汇总行统计文本随工具链演进（`14 duplicates / 1 suggestion` → `15 duplicates / 2 suggestions`；警告总数 `16 / 23` 不变，且与当前 ubuntu / 基线 ubuntu 文本一致） |
| windows-latest | 3 行 = 基线，无新增 |

过程注记：首轮 ubuntu 曾失败一次——上游既有测试 `service::cli::shim::build::tests::pnpm_sh_shim_prefers_existing_bundle_over_selected_path` 偶发 `ETXTBSY`（"Text file busy"，exec 时内核级竞态，与本次改动无关；同一提交重跑即通过，且首轮 f5a32a3 的 ubuntu 相同测试通过）。已重跑该 job，最终 run 结论 success。

核对方法：`gh run view <id> --log` 日志含 ANSI 色码（`warning` 与 `:` 之间），先剥 `\x1b\[[0-9;]*m` 与时间戳前缀再按行提取比较；脚本与原始日志见证据目录。

