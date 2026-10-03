# WSL 核心（Windows）

> 状态：**已实现**。实现决策、验收记录与返修历史见 `src-tauri/docs/WSL-CORE-DECISIONS.md`。

「WSL 核心」让桌面版把 Harness 核心放到 **WSL 发行版内**运行：窗口与设置界面仍是 Windows 侧的桌面壳，但 dsh 进程、数据目录、工作区与命令全部在 Linux 侧。适用于希望在 Linux 环境里运行会话与工作区的场景。

## 架构

```
┌───────────────────────────── Windows ─────────────────────────────┐
│  DeepSeek Harness 桌面版（Tauri 2 外壳 + React UI）               │
│    │  设置「核心」→ 核心来源 = WSL 发行版（Setting.wsl_distro）   │
│    ▼                                                              │
│  Rust 后端：wsl.exe -d <发行版> -e bash -lc <脚本> <位置参数…>    │
│    │        （动态值只作位置参数，绝不拼进脚本文本）              │
│    ▼                                                              │
│  wsl.exe 中继（Windows ↔ WSL 的进程桥）                           │
└───────────────┬───────────────────────────────────────────────────┘
                ▼
┌──────────────────────── WSL 发行版（Linux）───────────────────────┐
│  bash -lc <受控脚本>：探测 / 安装 / 启动 / 停止 / 健康检查        │
│    │                                                              │
│    ▼                                                              │
│  受控 runtime：$HOME/.dsh-desktop/runtime                         │
│    └ node_modules/.bin/dsh   （由应用内置清单 + 锁文件 npm ci）   │
│        │  --profile … --skip-auth（本地回环、跳过登录）           │
│        ▼                                                          │
│  dsh agent 服务 → 工作区 / 会话 / 凭据均在 Linux 侧               │
└───────────────────────────────────────────────────────────────────┘
```

桌面壳不直接管理用户全局安装的 dsh：受控 runtime 是发行版内 `$HOME/<数据目录>/runtime` 下一个由应用维护的 npm 项目，与用户自己的 npm 全局目录、其他 Node 包互不影响。停止服务时先在 Linux 侧按 pid 文件结束进程组，再回收中继。

## 数据目录

| 位置 | 内容 |
| --- | --- |
| WSL 内 `$HOME/.dsh-desktop`（debug 构建为 `$HOME/.dsh-desktop.dev`） | 受控 `runtime/`（dsh 与固定依赖树）；dsh 自己的 profiles、会话、凭据 |
| Windows 应用数据目录 | 设置（核心来源、发行版、端口等）；进行中的运行时切换标记 `wsl-runtime-switch.json`（debug 位于 `dev/` 子目录） |

WSL 核心与 Windows 核心的数据目录**互不迁移**：切换核心来源不会搬动任何 profile、会话或凭据。

## 安装、更新与回滚

每个受支持的 dsh 版本随安装包分发一份**固定基线**（`src-tauri/resources/wsl-runtime/<版本>/package.json` + `package-lock.json`）：manifest 用 `overrides` 固定六个 Cordis 兼容组件（`cordis`、`plugin-loader`、`plugin-hmr`、`plugin-timer`、`plugin-include`、`plugin-group`），lockfile 固定全部传递依赖。

安装/更新流程：

1. 在数据目录内创建候选 runtime，`npm ci` 重建资源指定的精确依赖树；
2. 对候选打 `--skip-auth` 补丁；
3. 用独立临时 live profile 启动候选并核对客户端资源全部就绪（不是只看进程存活）；
4. 整体切换到 `runtime/`，旧运行时保留为备份；
5. 正式启动并由桌面就绪确认后，才清理本次备份。

任何一步失败都不会破坏正在使用的运行时：候选失败直接丢弃；切换后启动失败自动回滚到旧运行时（失败树保留便于排查）。请求的版本**缺少资源时停止安装**——不存在无锁的 `npm install` 后备路径（`^` 范围会解析出无法在 `patchReload: "live"` 下启动的依赖组合）。

## 独立版本与推荐基线

WSL 核心的版本由**独立字段**描述，不与 Windows 内核的推荐值/最低基线混用：

| 字段（`src-tauri/resources/manifest.jsonc` → `engines.dsh`） | 含义 | 当前值 |
| --- | --- | --- |
| `wslRecommend` | WSL 核心的推荐基线：安装/更新确认框的缺省目标版本，也是判断 WSL 行「高于推荐」的基准 | `0.1.2-rc.1` |
| `recommend` | Windows 内核的推荐版本 | `0.2.0-rc.2` |
| `minimum` | Windows 内核的最低支持基线（`MIN_SUPPORTED_CORE_VERSION`） | `0.1.5-rc.1` |

判据落在两处：Rust 侧 `src-tauri/src/config/manifest.rs` 的 `engines.dsh.wslRecommend`（缺字段时为空串，由 `validated_recommendation` 兜底），经 `get_wsl_recommended_version` 命令下发；前端 `src/config/query-keys.ts` 的 `wslRecommendedVersion` 查询与 `src/ui/config/wsl-core.tsx` 使用它。**Windows 内核的推荐值变化不会影响 WSL 行的推荐与「高于推荐」判定**，反之亦然。当前随安装包分发的受控 runtime 只有 `0.1.2-rc.1` 一份资源（见上节）。

## 已知限制

1. **内置插件缺席**。内置插件（DSH Tauri 系列）只为 Windows 侧档案注入；WSL 核心使用独立的 Linux 数据目录与插件解析根，注入既不生效也不参与启动。设置页对依赖内置插件的操作会给出「WSL 核心不支持」提示。
2. **网络模式**。WSL 的 Mirrored 与 NAT 两种网络模式均已实测：桌面壳从 Windows 侧通过 `127.0.0.1:<端口>` 访问 Linux 内服务，两种模式都可达（NAT 模式依赖 Windows 的 localhost 转发）。端口占用检测与避让始终在 Linux 侧完成。
3. **需要自备 Node ≥ 20**。应用不在发行版内做系统级安装：WSL 发行版内需有可用的 Node，应用只校验**主版本 ≥ 20**（`src-tauri/src/service/wsl_core/install.rs` 的 `MIN_NODE_MAJOR`），否则核心探测会明确报告缺失（`WSL_NODE_MISSING` / `WSL_NODE_TOO_OLD`）并停止。
   WSL 内的 Node 与桌面侧**内嵌 Node 是两套东西**：桌面下载并管理的 Node 版本由 `src-tauri/src/config/constants.rs` 的 `NODE_VERSION`（当前 `v22.22.0`）与清单 `engines.node.version`（`>=22.22.0`）约束，只用于 Windows 侧；WSL 核心不嵌入、不共享该运行时。
4. **`--skip-auth` 补丁随 dsh 升级需重打**。该补丁让 dsh 在本地回环下跳过登录检查，锚定 dsh 的源码结构；dsh 版本升级后补丁锚点可能变化。应用会在候选 runtime 上自动重打并验证，补丁缺失或失败时不会切换。
5. **首次安装需要网络**。候选 `npm ci` 需要访问 npm registry；离线时安装失败并保持现状（不影响已安装的运行时）。
