/**
 * WSL 核心真机用例 A（U8.4 真机矩阵的桌面侧分支）。
 *
 * 与 `boot.e2e.ts` 的分工：那边验「Windows 内核装配 → 服务 → iframe」，这里验**同一套壳层
 * 在 WSL 核心下**的行为。四个用例都走应用自己的协议，不靠改文件：
 *
 *   1. 切到 WSL 核心：`update_app_config({ wslDistro })` → `set_active_core({ id: 'wsl' })`
 *      → 冷启动应用让服务换到新核心 → 独立数据根（UNC）与帧内渲染在真机上闭环；
 *   2. 端口避让：发行版内占住服务端口后重启，Linux 侧扫描应换端口，iframe 的地址印记随之改变；
 *   3. 缺本机内核时的切换协议：`set_active_core('app-bundled')` 必须报前置错误且不把状态写坏；
 *   4. 核心列表契约：wsl 行的标识、UNC 目录与可移除性；
 *   5. 重复安装幂等：相同基线再调一次 `install_wsl_core` 必须走 `install.rs` 的早退——不报
 *      `WSL_INSTALL_BUSY`、不重放 `npm ci`、运行时现场一个字节都不动，且 WSL 目标版本来自
 *      独立推荐值（`get_wsl_recommended_version`）而不是桌面推荐值。
 *
 * store 在哪（实测，踩过）：E2E 应用的 store 是
 * `<scratch>/home/AppData/Roaming/dsh-tauri/.store.test.dat` —— `app_data_dir()` 跟随被
 * `startDesktopApp` 重定向的 `USERPROFILE`，**真实** `%APPDATA%\dsh-tauri\` 下的同名文件
 * 应用根本不读（`store-probe.e2e.ts` 逐步打印三个候选文件后确认）。
 * 因此本车道**不预先写 store**（写了也不生效），设置一律经 IPC 在运行期写入；
 * 而「重启」必须复用同一个 scratch 根，否则 store 连同旧根一起消失，
 * `active_core`/`wsl_distro` 全丢——这正是冷启动后报 `active_source: 'app'`、
 * `get_cores()` 里连 wsl 行都没有的原因。
 *
 * 为什么用「冷启动整个应用」驱动重启：`set_active_core` 只写设置，**不起服务**；服务与
 * iframe 挂载由壳层 `store.harness.boot()` → `launchAndWait()` 负责，而 `serviceHealthy`
 * （iframe 的挂载门控）全应用只有 `completeReadiness`（`store.ts:418`）一处置 true。
 * 后端 IPC 里没有「启动并进入就绪态」的原子命令，所以**只能让前端跑一次 boot**。
 * 实测教训：光调 `shutdown_harness` + `launch_harness` 会出现「后端已切到 wsl、
 * `get_runtime_info` 已报 `active_source: 'wsl'`，但 iframe 永远不挂」（前端没被要求启动）。
 *
 * ⚠️ **不能用 `browser.refresh()` 让前端重跑 boot**（踩过）：驱动发起的页面刷新之后，
 * msedgedriver/WebView2 就再也无法对内嵌帧注入脚本——`switchToFrame` 仍然成功，但任何
 * `executeScript` 都以
 * `WebDriverError: Frame ExecuteScript failed: Error { code: HRESULT(0x8007139F),
 *  message: "组或资源的状态不是执行请求操作的正确状态。" }`
 * 失败，等 5 s 也不恢复。对照组实测（`refresh-probe.e2e.ts`，同一个应用核心帧、不碰 WSL）：
 * 刷新**前** `execute(() => 1 + 1)` 返回 2，刷新**后**同一帧两次尝试都抛上面的错。
 * 因此改用**停应用 → 用同一份 store 冷启动**（新的 WebDriver 会话，帧注入恢复可用）。
 * 与用户点「使用此核心」（`store.harness.restart()`，`src/ui/config/wsl-core.tsx:259-266`）
 * 的差别只在「进程是否换掉」：核心切换的持久化与启动分流完全一致——`set_active_core` 已把
 * `active_core` 写进 store，冷启动时后端 `workflow::launch` 按它分流到 WSL 分支，
 * 壳层 `boot()` 照常拉起服务并挂帧。
 *
 * 帧「活着」怎么判定（不能读 store，也不能数桥消息）：
 *   - `src/store/modules/harness/store.ts` 没有把状态挂到 window 上，页面里读不到
 *     `iframeAliveKey`；
 *   - 桥消息是 `postMessage`（`src/layout/components/iframe.tsx:249` 直接调
 *     `store.harness.markIframeAlive()`），**不是** `CustomEvent`，所以「包一层
 *     `window.dispatchEvent` 数桥消息」的探针永远数到 0（踩过，已删）；
 *   - 于是用渲染层观测量：帧内 `#root` 有子节点（进帧看）+ 壳层没有「帧里没有 dsh 页面」
 *     覆盖层。覆盖层的出现条件是 `serviceHealthy && iframeError`，而 `iframeError` 只在
 *     `status === 'ready' && serviceHealthy && !iframeAlive` 持续
 *     `IFRAME_LOAD_TIMEOUT(20 s)`（已加载过则为 `IFRAME_FRAME_GRACE_TIMEOUT(5 s)`）后才置位
 *     （`src/store/modules/harness/constants.ts:9/19`、`store.ts:859-890`）——所以「等够 25 s
 *     仍没有覆盖层」等价于「壳层收到了帧的自报」。
 *
 * 前置（必须由人工确认，用例会硬拦）：
 *   - 发行版 `Ubuntu` 的**默认用户**已临时切到 `dshu8`，且该用户的
 *     `~/.dsh-desktop.dev/runtime/node_modules/.bin/dsh` 是 U8.4 受控安装产物。
 *   - 该默认用户在测试窗口内不会被并行任务改动（应用自己不带 `-u`，落到默认用户家目录）。
 *
 * 不做的事：不在测试里改 `/etc/wsl.conf`、不 `wsl --shutdown`、不碰 `/home/pixel`。
 */

import type { DesktopApp } from '../support/desktop'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import process from 'node:process'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { startDesktopApp } from '../support/desktop'
import { completePreinstall } from '../support/preinstall'
import { SHELL_IFRAME } from '../support/selectors'
import {
  defaultWslUser,
  E2E_DISTRO,
  E2E_WSL_HOME_DIR,
  E2E_WSL_USER,
  wslBash,
  wslDshPids,
  wslRuntimeEntryExists,
} from '../support/wsl'

// ============================================================================
// 常量与类型
// ============================================================================

// 首次启动要过装配引导 + 拉起本机核心，重载后切到 WSL 又要走一次 preflight（含发行版冷启动），
// 因此这里给足余量（配置里的 180 s 不够）
const READY_TIMEOUT_MS = 300_000
const RESTART_TIMEOUT_MS = 240_000
/** 用例体上限：要容下「切换核心重载 → 健康探测 → 进帧/出帧 → 帧自报」整条链，比单个等待更长 */
const CASE_TIMEOUT_MS = 600_000
const SETTLE_MS = 3_000
/** 等壳层把「帧是空的」这个结论摆出来所需的时间：`IFRAME_LOAD_TIMEOUT(20 s)` + 余量 */
const OVERLAY_SETTLE_MS = 25_000
const DSH_ROOT = '#root'

interface DshWindow {
  __TAURI_INTERNALS__?: { invoke: (cmd: string, args?: unknown) => Promise<unknown> }
  __dshPageErrors?: string[]
}

interface HarnessCore {
  id: string
  source: string
  dir: string
  path: string
  present: boolean
  active: boolean
  removable: boolean
  version: string
  error: string | null
}

interface RuntimeInfo {
  active_source: string
  data_dir: string
  node_version: string
  service_url: string
}

/** `update_app_config` 的回包：整份设置（`config::Setting` 全字段）。 */
interface AppConfig {
  port: number
  wsl_distro: string | null
  active_core: string | null
}

interface WslCoreProbe {
  distro: string
  home: string
  node: string | null
  nodeVersion: string | null
  dsh: string | null
  dshVersion: string | null
  runtimeDir: string
  skipAuthReady: boolean
}

/** iframe `src` 上的挂载印记。 */
interface FrameStamp {
  t: string
  token: string
}

// ============================================================================
// 隔离根
// ============================================================================

/**
 * 本跑独占的 scratch 根，**跨重启复用**。
 *
 * ⚠️ 关键事实（实测，踩过）：E2E 应用把 store 写到
 * `<scratch>/home/AppData/Roaming/dsh-tauri/.store.test.dat` —— `app_data_dir()` 在
 * Windows 上跟随被重定向的 `USERPROFILE`（`startDesktopApp` 把它指到 `<scratch>/home`），
 * 所以**真实** `%APPDATA%\dsh-tauri\` 下的同名文件应用根本不读。
 * 推论：重启必须复用同一个 scratch 根，否则 store 随旧根一起被删，
 * `active_core`/`wsl_distro` 全部丢失（冷启动报 `active_source: 'app'`、`get_cores()`
 * 里连 wsl 行都没有）——这正是 `store-probe.e2e.ts` 测出来的失效形态。
 */
function makeScratchHome(): string {
  return mkdtempSync(join(tmpdir(), 'dsh-e2e-desktop-'))
}

// ============================================================================
// 页面内探针（都跑在 WebView 的文档里）
// ============================================================================

/**
 * 帧内错误收集器（与 `boot.e2e.ts` 的做法一致）。换应用实例后需要重装。
 */
function collectPageErrors(): void {
  const w = window as unknown as DshWindow & { __dshCollectorInstalled?: boolean }
  w.__dshPageErrors = []
  if (w.__dshCollectorInstalled === true)
    return
  w.__dshCollectorInstalled = true
  window.addEventListener('error', event => w.__dshPageErrors!.push(`error: ${event.message}`))
  window.addEventListener('unhandledrejection', event => w.__dshPageErrors!.push(`rejection: ${String(event.reason)}`))
}

function readPageErrors(): string[] {
  return (window as unknown as DshWindow).__dshPageErrors ?? []
}

function dshRootRendered(selector: string): boolean {
  return (document.querySelector(selector)?.childElementCount ?? 0) > 0
}

/**
 * 壳层的「帧里没有 dsh 页面」错误覆盖层。
 *
 * `showIframeError = serviceHealthy && iframeError`、`iframeErrorHint` 仅在 `!iframeAlive`
 * 时给出文案：覆盖层不在（或在了但没有补充说明）说明**壳层没有理由认为帧是空的**。
 */
function readIframeErrorOverlay(): { overlay: boolean, hint: string } {
  const hint = Array.from(document.querySelectorAll('p'))
    .map(node => node.textContent?.trim() ?? '')
    .find(text => text.includes('.dsh-desktop.dev') || text.includes('.dsh.dev'))
  return { overlay: hint !== undefined, hint: hint ?? '' }
}

/**
 * iframe `src`（地址印记的原料）。
 *
 * **必须自包含**：`browser.execute(fn)` 只把函数体串发送到页面里 eval，页面里没有本模块的
 * 作用域。实测教训：此处若调用同模块的 `readIframeSrc()`，会得到
 * `WebDriverError: readIframeSrc is not defined`（TC-002 就是这么挂的）。
 */
function readIframeSrc(): string {
  return document.querySelector('[data-testid="dsh-shell-iframe"]')?.getAttribute('src') ?? ''
}

/**
 * iframe `src` 上的挂载印记（`t=<时间戳>&token=…`）：重挂必变。
 *
 * 同样必须自包含——`readIframeSrc()` 不能在这里调用（见上）。
 */
function iframeSrcStamp(): FrameStamp {
  const src = document.querySelector('[data-testid="dsh-shell-iframe"]')?.getAttribute('src') ?? ''
  if (src === '')
    return { t: '', token: '' }
  try {
    const params = new URLSearchParams(new URL(src).search)
    return { t: params.get('t') ?? '', token: params.get('token') ?? '' }
  }
  catch {
    return { t: '', token: '' }
  }
}

// ============================================================================
// IPC 辅助
// ============================================================================

/**
 * 调一次应用 IPC。
 *
 * 这里不用 `browser.executeAsync`：它的回调参数类型由 `ReturnValue` 推导
 * （`(result?: TransformElement<T>) => void`），与本函数「成功回值 / 失败回 `{ __error }`」
 * 的双形态对不上（实测 `TS2345`）。改用 `browser.execute` 直接用返回的 Promise 更干净。
 */
async function invoke<T>(browser: WebdriverIO.Browser, cmd: string, args?: unknown): Promise<T> {
  return browser.execute(async (command: string, payload: object) => {
    const internals = (window as unknown as DshWindow).__TAURI_INTERNALS__
    if (internals === undefined)
      return { __error: '__TAURI_INTERNALS__ missing' }
    try {
      return await internals.invoke(command, payload)
    }
    catch (error) {
      return { __error: String(error) }
    }
  }, cmd, (args ?? {}) as object) as Promise<T>
}

/** invoke 结果若带 `__error` 就取出错误串，否则返回 undefined。 */
function invokeError(value: unknown): string | undefined {
  if (value !== null && typeof value === 'object' && '__error' in value)
    return String((value as { __error: unknown }).__error)
  return undefined
}

async function updateWslDistro(browser: WebdriverIO.Browser, distro: string): Promise<AppConfig> {
  const result = await invoke<unknown>(browser, 'update_app_config', { wslDistro: distro })
  expect(invokeError(result), `写入 wslDistro 失败：${invokeError(result) ?? ''}`).toBeUndefined()
  return result as AppConfig
}

/**
 * 改服务端口（`bridge/config.rs:115-120`：`port` 会**同时**写进 `setting.manual_port`）。
 *
 * `manual_port` 是关键：`resolve_port_wsl`（`src-tauri/src/service/workflow/wsl_launch.rs:148-189`）
 * 会先把 `setting.port` heal 回 `manual_port.unwrap_or(default_port())`（仅当该端口空闲）。
 * 不动 `manual_port` 时，任何偏离默认端口的 `port` 都会被 heal 回默认值——端口用例必须钉住。
 */
async function updateAppPort(browser: WebdriverIO.Browser, port: number): Promise<AppConfig> {
  const result = await invoke<unknown>(browser, 'update_app_config', { port })
  expect(invokeError(result), `写入 port 失败：${invokeError(result) ?? ''}`).toBeUndefined()
  return result as AppConfig
}

async function setActiveCore(browser: WebdriverIO.Browser, id: string): Promise<{ core?: HarnessCore, error?: string }> {
  const result = await invoke<unknown>(browser, 'set_active_core', { id })
  const error = invokeError(result)
  if (error !== undefined)
    return { error }
  return { core: result as HarnessCore }
}

async function readRuntimeInfo(browser: WebdriverIO.Browser): Promise<RuntimeInfo> {
  const info = await invoke<RuntimeInfo>(browser, 'get_runtime_info')
  expect(invokeError(info), `get_runtime_info 失败：${invokeError(info) ?? ''}`).toBeUndefined()
  return info
}

async function readCores(browser: WebdriverIO.Browser): Promise<HarnessCore[]> {
  const cores = await invoke<HarnessCore[]>(browser, 'get_cores')
  expect(invokeError(cores), `get_cores 失败：${invokeError(cores) ?? ''}`).toBeUndefined()
  return cores
}

/**
 * 现场探测发行版里的受控运行时（`bridge/wsl_core.rs:probe_wsl_core`）。
 *
 * **这一步不能省**：`core::list` 只读进程内探测缓存（`service/core/version.rs:165`
 * 用 `wsl_core::probe::cached`），缓存是空的就一律报 `present: false, dir: ""`。
 * 真实用户路径由 WSL 面板挂载时发这次探测；E2E 必须显式复刻。
 */
async function probeWslCore(browser: WebdriverIO.Browser, distro: string): Promise<WslCoreProbe> {
  const probe = await invoke<WslCoreProbe>(browser, 'probe_wsl_core', { distro })
  expect(invokeError(probe), `probe_wsl_core 失败：${invokeError(probe) ?? ''}`).toBeUndefined()
  return probe
}

/**
 * WSL 核心的独立推荐版本（`bridge/wsl_core.rs:55-57`，**不带任何参数**）。
 *
 * 与桌面推荐值解耦是 U1 的约束：桌面升到 0.2 不该把 WSL 目标一起挪走
 * （0.2.0-rc.2 的 client bundle 会 404，见 `bridge/wsl_core.rs:31-35`）。
 */
async function getWslRecommendedVersion(browser: WebdriverIO.Browser): Promise<string> {
  const version = await invoke<unknown>(browser, 'get_wsl_recommended_version')
  expect(invokeError(version), `get_wsl_recommended_version 失败：${invokeError(version) ?? ''}`).toBeUndefined()
  return String(version)
}

/**
 * 受控运行时在磁盘上的可观测状态（「重复安装什么都没动」的判据）。
 *
 * 用 `sha256sum` 判内容、用 `stat` 记包锁时间：只要应用真重放了 `npm ci`，内容或时间必然变
 * 一个。候选 / 备份 / 失败三个槽位一并列出——正常路径下它们都不该存在
 * （只有 `script.rs` 的 PREPARE / SWITCH / ROLLBACK 才会造）。
 */
function readManagedRuntimeState(): string {
  const result = wslBash([
    `d="$HOME/${E2E_WSL_HOME_DIR}"`,
    'sha256sum "$d/runtime/.dsh-runtime.json" "$d/settings.yaml" "$d/.credentials.yaml" 2>&1 || true',
    'stat -c "lock_mtime=%Y lock_size=%s" "$d/runtime/package-lock.json" 2>&1 || true',
    'ls -d "$d"/runtime-candidate "$d"/runtime-backup-* "$d"/runtime-failed-* 2>/dev/null || echo slots=none',
    'printf "global_dsh=%s\\n" "$(command -v dsh || echo none)"',
  ].join('; '))
  expect(result.status, `读取受控运行时现场失败：${result.stderr || result.stdout}`).toBe(0)
  return result.stdout
}

/**
 * 帧就绪判据：`#root` 有子节点**且**文档加载完。
 *
 * 两个条件都要：`#root` 有子节点只说明 dsh 页面渲染了，文档仍在加载时 WebView2 驱动
 * 仍会拒绝 `execute`。
 */
function dshFrameReady(): boolean {
  const root = document.querySelector('#root')
  return document.readyState === 'complete' && root !== null && root.childElementCount > 0
}

/**
 * 进入帧并等 dsh 页面渲染出来，**失败可重试**。
 *
 * 两条实测教训（非常容易踩）：
 * 1. **不能从父文档观察帧**：壳层与 dsh 服务不同源（壳层是 `tauri://localhost` 这类
 *    自定义协议，帧是 `http://127.0.0.1:<port>`），`iframe.contentDocument` 恒为 `null`。
 *    第 6 次运行里父侧判据每 500 ms 返回一次 `false`，整整空转 300 s——只能从**帧内**
 *    观察（WebDriver 切帧不受同源限制）。
 * 2. **只在 `src` 上有新印记就切帧太早**：那一刻文档还在加载，切进去的 `execute` 会被
 *    WebView2 驱动拒绝，原文
 *    `WebDriverError: Frame ExecuteScript failed: Error { code: HRESULT(0x8007139F),
 *    message: "组或资源的状态不是执行请求操作的正确状态。" }`（第 4/5 次运行就是这条，
 *    WebDriver 每次重试约 10 s，把整条用例拖到超时）。
 *    故把「切帧 + 帧内就绪」当成一次可重试的尝试，抛错就退出来重来。
 */
async function enterFrame(browser: WebdriverIO.Browser, timeout = 120_000): Promise<void> {
  const deadline = Date.now() + timeout
  let lastError = ''
  let attempt = 0
  while (Date.now() < deadline) {
    attempt += 1
    try {
      // **每次尝试都必须从顶层开始**：上一轮可能停在帧上下文里，那样 `$(SHELL_IFRAME)`
      // 是在帧里找元素（找不到 / 拿到失效引用），随后 `switchFrame` 必然以
      // `0x8007139F` 收场（第 7 次运行 TC-001 的 10 次重试就是这么空转到 300 s 的）。
      try {
        await browser.switchFrame(null)
      }
      catch { /* 已经不在帧里 */ }
      const frame = await browser.$(SHELL_IFRAME)
      await frame.waitForDisplayed({ timeout: Math.min(10_000, timeout) })
      // 切帧本身也要看时机：`src` 刚被设上时文档还在导航，切进去的 `execute` 会被
      // WebView2 拒（`0x8007139F`）。故等一小会儿再切，切完只在帧内轮询。
      await browser.pause(1_000)
      await browser.switchFrame(frame)
      await browser.waitUntil(() => browser.execute(dshFrameReady), { timeout: 10_000, interval: 400 })
      await browser.pause(SETTLE_MS)
      return
    }
    catch (error) {
      lastError = String(error)
      try {
        await browser.switchFrame(null)
      }
      catch { /* 已经回到顶层 */ }
      await browser.pause(1_000)
    }
  }
  throw new Error(`进帧 ${Math.round(timeout / 1000)} s 内 ${attempt} 次尝试都未成功（最后一次：${lastError}）`)
}

/**
 * 出帧回到壳层文档。
 *
 * **必须出帧**：`browser.execute` 跑在「当前上下文」里，进帧后所有 IPC 调用都发给了
 * 内嵌的 dsh 文档——那里没有 `__TAURI_INTERNALS__`。实测教训（第 4 次运行）：出帧前调
 * `get_runtime_info` / `get_cores`，整条用例会以
 * `WebDriverError: Frame ExecuteScript failed: Error { code: HRESULT(0x8007139F),
 * message: "组或资源的状态不是执行请求操作的正确状态。" }` 挂掉（webdriver 重试 10 次后失败）。
 * `boot.e2e.ts` 收尾也是这么做的（`await browser.switchFrame(null)`）。
 */
async function leaveFrame(browser: WebdriverIO.Browser): Promise<void> {
  await browser.switchFrame(null)
}

/** 读当前挂载印记；如果此刻还没有挂载则返回空印记。 */
async function currentStamp(browser: WebdriverIO.Browser): Promise<FrameStamp> {
  return browser.execute(iframeSrcStamp)
}

/** 等 iframe 真正挂上（`src` 非空）——首次启动或重载之后用。 */
async function waitForMountStamp(browser: WebdriverIO.Browser, timeout: number, previous?: FrameStamp): Promise<FrameStamp> {
  await browser.waitUntil(
    async () => {
      const stamp = await browser.execute(iframeSrcStamp)
      if (stamp.t === '')
        return false
      // 重载场景必须等**新一代**印记：页面重载后旧 iframe 会短暂消失，但轮询的第一拍
      // 仍可能落在旧文档上（实测教训：只判 `t !== ''` 会拿到旧印记，断言随即失败）。
      return previous === undefined || stamp.t !== previous.t
    },
    { timeout, interval: 300, timeoutMsg: 'iframe 没有挂载（src 上一直没有新的挂载印记）' },
  )
  return browser.execute(iframeSrcStamp)
}

// 「让服务换一代」的实现是 `restartAppHost()`，定义在用例区（它需要替换 describe 作用域里的
// `app` / `browser`）。为什么是冷启动而不是 `browser.refresh()`，见文件头 ⚠️ 段。

// ============================================================================
// 真机前置
// ============================================================================

function assertRealMachinePreconditions(): void {
  const who = defaultWslUser()
  const [user = '', home = ''] = who.split(':')
  expect(
    user,
    `发行版 ${E2E_DISTRO} 的默认用户是 ${user || '(未知)'}；真机用例只允许在隔离用户 `
    + `${E2E_WSL_USER} 下运行（应用不带 -u，默认用户就是它实际读写的家目录）。`
    + '临时切换默认用户后重跑（见 docs/spec/WSL_CORE.zh.md），跑完务必还原。',
  ).toBe(E2E_WSL_USER)
  expect(home, '隔离用户的 HOME 与预期不符').toBe(`/home/${E2E_WSL_USER}`)
  expect(
    wslRuntimeEntryExists(),
    `缺少 U8.4 受控安装产物：$HOME/${E2E_WSL_HOME_DIR}/runtime/node_modules/.bin/dsh。`
    + '先在隔离用户下跑一次受控安装。',
  ).toBe(true)
  killOrphanWslHarness()
}

/** 清掉上一轮遗留的发行版内 dsh（只按受控 runtime 路径匹配，不碰别的东西）。 */
function killOrphanWslHarness(): void {
  const orphans = wslDshPids()
  if (orphans.length === 0)
    return
  process.stdout.write(`[wsl-core-e2e] 清理上一轮遗留的 WSL dsh 进程：${orphans.join(', ')}\n`)
  wslBash(`kill ${orphans.join(' ')} 2>/dev/null || true`)
}

// ============================================================================
// 发行版侧端口占用器（验端口避让）
// ============================================================================

/**
 * 占住一个端口——**必须做在发行版内**。
 *
 * 实测教训：
 * 1. 镜像网络下 Linux 里的 dsh 绑住 3082 时，Windows 侧 `node:net` 绑同一端口会直接抛
 *    `listen EADDRINUSE: address already in use 127.0.0.1:3082` —— 即「Windows 侧能绑」与
 *    「Linux 侧能绑」并不是一回事，而 `resolve_port_wsl`
 *    （`src-tauri/src/service/wsl_launch.rs:148-189`）的判定走的是**发行版内**的 bind。
 * 2. **不要用 `$!` 当「占用成功」的判据**：`nc -l -k -p <port> &` 的 `$!` 在
 *    `bash -lc` 里可能指向一个已经退出的壳（第 8 次运行就出现过
 *    `OCCUPY_OK=no / OCCUPY_PID=1772 / OCCUPY_LISTEN=yes`——进程号取不到，端口却真的在听）。
 *    判据只认事实：`ss` 上有没有 LISTEN 行，并从 `ss -ltnp` 里取回真正占着它的 pid。
 * 3. **调用前必须先把服务停掉**（`stopAppHost()`），并且这里用 `PRE_LISTEN` 兜底：
 *    如果 `nc` 起来之前该端口**已经**有人监听，说明监听者是别人（例如还活着的 dsh），
 *    此时 `OCCUPY_OK=yes` 毫无意义——直接失败，避免第 10 次运行那种「占了 dsh 自己的端口」。
 */
function occupyPortInDistro(port: number): number {
  const result = wslBash([
    `P=/tmp/dsh-e2e-occupy-${port}.pid`,
    'rm -f "$P"',
    `if ss -ltn 2>/dev/null | grep -q ':${port} '; then echo "PRE_LISTEN=yes"; else echo "PRE_LISTEN=no"; fi`,
    `setsid nc -l -k -p ${port} >/dev/null 2>&1 < /dev/null &`,
    'sleep 1.5',
    `if ss -ltn 2>/dev/null | grep -q ':${port} '; then echo "OCCUPY_OK=yes"; else echo "OCCUPY_OK=no"; fi`,
    `ss -ltnp 2>/dev/null | grep ':${port} ' || true`,
    `ss -ltnp 2>/dev/null | grep ':${port} ' | grep -o 'pid=[0-9]*' | head -1 | cut -d= -f2 > "$P" || true`,
    'cat "$P" 2>/dev/null || true',
  ].join('\n'))
  process.stdout.write(`[wsl-core-e2e] occupy stdout=${JSON.stringify(result.stdout)} stderr=${JSON.stringify(result.stderr)}\n`)
  expect(
    /PRE_LISTEN=no/.test(result.stdout),
    `占端口前 ${port} 就已经有人在听（多半是没停干净的 dsh）：${JSON.stringify(result.stdout)}`,
  ).toBe(true)
  expect(
    /OCCUPY_OK=yes/.test(result.stdout),
    `发行版内占端口失败：${JSON.stringify(result.stdout)} / ${result.stderr}`,
  ).toBe(true)
  // 从 `ss -ltnp` 的回显里取 pid（`pid=<n>`）；取不到就交给调用方用端口反查
  const pid = Number(/pid=(\d+)/.exec(result.stdout)?.[1] ?? Number.NaN)
  return Number.isInteger(pid) && pid > 0 ? pid : -1
}

/** 确认端口真的被发行版内进程占住了（`ss` 看 LISTEN 行）。 */
function distroPortListening(port: number): boolean {
  const result = wslBash(`ss -ltn 2>/dev/null | grep -c ':${port} ' || true`)
  return Number(result.stdout.trim()) > 0
}

function releasePortInDistro(pid: number | undefined): void {
  if (pid === undefined || pid <= 0)
    return
  wslBash(`kill ${pid} 2>/dev/null || true`)
}

// ============================================================================
// 用例
// ============================================================================

describe.skipIf(process.platform !== 'win32')('WSL 核心真机链路', () => {
  let app: DesktopApp | undefined
  let browser: WebdriverIO.Browser
  /** 本跑独占且跨重启复用的隔离根（store 就在它里面，见 makeScratchHome 的注释） */
  let scratchHome: string

  beforeAll(async () => {
    assertRealMachinePreconditions()
    scratchHome = makeScratchHome()

    app = await startDesktopApp({ homeDir: scratchHome, keepHome: true, resetStore: false })
    browser = app.browser

    // 尽早装壳层收集器：装配失败、iframe 加载失败都会在壳层留下痕迹
    await browser.execute(collectPageErrors)

    // 首次装配引导会挡在服务之前（`.store.test.dat` 每次运行都被删，没有预设指纹基线），
    // 跳过它，壳层才会真正拉起本机核心并挂上 iframe —— 与 `boot.e2e.ts` 同一口径
    await completePreinstall(browser, READY_TIMEOUT_MS)
    await waitForMountStamp(browser, READY_TIMEOUT_MS)
    // 页面可能刚重载，收集器重装一次（自幂等）
    await browser.execute(collectPageErrors)
  }, READY_TIMEOUT_MS)

  afterAll(async () => {
    // 保留隔离根到最后一刻再删：`stop()` 默认会删掉它，而 store 就在里面
    await app?.stop({ keepHome: true })
    rmSync(scratchHome, { force: true, recursive: true })
    killOrphanWslHarness()
  })

  /**
   * 只停机：走应用自己的停机协议，再把应用进程收掉（保留隔离根）。
   *
   * 单独拆出来是给「占端口」类用例用的：**必须先把服务停掉再占端口**。
   * 否则会出现这种误判（第 10 次运行实测）：旧 dsh 还听着 3081 时 `nc -l -k -p 3081`
   * 根本绑不上、直接退出，而 `ss -ltn | grep ':3081 '` 看到的是 **dsh 自己的监听行** →
   * 占端口被判成成功、取到的 pid 其实是 dsh 的 pid；随后重启把 dsh 停掉，端口又空了，
   * 新服务照旧绑 3081。
   */
  async function stopAppHost(): Promise<void> {
    const shutdown = await invoke<unknown>(browser, 'shutdown_harness')
    if (invokeError(shutdown) !== undefined)
      process.stdout.write(`[wsl-core] shutdown_harness 返回错误（继续冷启动）：${invokeError(shutdown)}\n`)

    await app?.stop({ keepHome: true })

    // 等发行版里的 dsh 真正退出（最多 10 s）：占端口的用例必须先确认它没了，
    // 否则会占出「dsh 自己的端口」这种假成功（见 occupyPortInDistro）
    for (let i = 0; i < 20 && wslDshPids().length > 0; i++)
      await new Promise(resolve => setTimeout(resolve, 500))
  }

  /**
   * 冷启动一次应用实例，并等新一代帧挂上。
   *
   * `previous` 是重启前的挂载印记：`completePreinstall` 只要看到 `SHELL_IFRAME` 就返回，
   * 而新实例的帧可能还没挂——必须等**印记换代**才算真的换了服务。
   */
  async function startAppHost(previous: FrameStamp, timeout = RESTART_TIMEOUT_MS): Promise<FrameStamp> {
    // 必须复用同一个隔离根：store 就写在里面，换根等于换 store（见 makeScratchHome）
    app = await startDesktopApp({ homeDir: scratchHome, keepHome: true, resetStore: false })
    browser = app.browser
    await browser.execute(collectPageErrors)

    // 冷启动同样会撞上装配引导（`.store.test.dat` 的预设指纹与真实环境对不上）
    await completePreinstall(browser, timeout)
    const stamp = await waitForMountStamp(browser, timeout, previous)
    await browser.execute(collectPageErrors)
    return stamp
  }

  /**
   * 停掉当前应用实例，用**同一份 store** 冷启动一次。
   *
   * 这是本车道唯一的重启手段（为什么不能用 `browser.refresh()`，见文件头 ⚠️ 段）。
   */
  async function restartAppHost(previous: FrameStamp, timeout = RESTART_TIMEOUT_MS): Promise<FrameStamp> {
    await stopAppHost()
    return startAppHost(previous, timeout)
  }

  it('TC-WSL-L3-01-001 切到 WSL 核心：来源判定、独立数据根与帧身份握手在真机上闭环', async () => {
    // ① 起点：本机核心（本跑没有本机内核槽位，活动核心是随包核心）
    const before = await readRuntimeInfo(browser)
    expect(['app', 'local'], '起点竟然已经是 WSL 核心').toContain(before.active_source)

    // ② 经应用自己的协议写入发行版
    await updateWslDistro(browser, E2E_DISTRO)

    // ③ 现场探测受控运行时：这一步同时填充后端的进程内探测缓存，
    //    真实用户路径由 WSL 面板挂载时发起（`src/ui/config/wsl-core.tsx` 的 probe 查询）
    const probe = await probeWslCore(browser, E2E_DISTRO)
    expect(probe.distro).toBe(E2E_DISTRO)
    expect(probe.home, '发行版内的 $HOME 不是隔离用户家目录').toBe(`/home/${E2E_WSL_USER}`)
    expect(probe.dsh, '受控 runtime 里没有 dsh 入口').not.toBeNull()
    expect(probe.runtimeDir).toContain(E2E_WSL_HOME_DIR)
    expect(probe.skipAuthReady, '--skip-auth 补丁未就位，受控安装不完整').toBe(true)

    const coresBeforeSwitch = await readCores(browser)
    const wslRow = coresBeforeSwitch.find(core => core.id === 'wsl')
    expect(wslRow, 'get_cores() 里没有 wsl 行').toBeDefined()
    expect(wslRow!.present, `探测缓存已填充，wsl 行却仍报 absent：${wslRow!.error ?? ''}`).toBe(true)
    expect(wslRow!.dir, `wsl 行的数据目录不是 UNC：${wslRow!.dir}`).toMatch(/^\\\\wsl(\.localdomain|\.localhost)?\\/)
    expect(wslRow!.active, '尚未切换，wsl 行不该是活动核心').toBe(false)

    // ④ 切换核心，然后冷启动一次让服务换到新核心上
    //    （`set_active_core` 自己不起服务，见文件头）
    const beforeSwitch = await currentStamp(browser)
    expect(beforeSwitch.t, '起点没有挂载中的 iframe').not.toBe('')

    const switched = await setActiveCore(browser, 'wsl')
    expect(switched.error, `set_active_core('wsl') 失败：${switched.error ?? ''}`).toBeUndefined()
    expect(switched.core?.active, '切换回包里的活动核心不是 wsl').toBe(true)

    const stampAfter = await restartAppHost(beforeSwitch)
    expect(stampAfter.t, '冷启动后 iframe 的挂载印记没变').not.toBe(beforeSwitch.t)

    // ⑤ 帧内确实渲染了 dsh 页面（进帧看 `#root`，然后**必须出帧**，见 leaveFrame 注释）
    await enterFrame(browser)
    expect(await browser.execute(dshRootRendered, DSH_ROOT), '帧内没有渲染 dsh 页面').toBe(true)
    await leaveFrame(browser)

    // ⑥ 壳层没有「帧里没有 dsh 页面」的结论：`iframeError` 只在
    //    `status==='ready' && serviceHealthy && !iframeAlive` 持续 20 s（已加载过则 5 s）后置位，
    //    所以「挂载后再等够 25 s 仍然没有覆盖层」等价于「壳层收到了帧的自报」
    //    （桥消息是 postMessage，数不到；见文件头）
    await browser.pause(OVERLAY_SETTLE_MS)
    const overlay = await browser.execute(readIframeErrorOverlay)
    expect(overlay.hint, '壳层认为帧里没有 dsh 页面').toBe('')
    expect(await browser.execute(readPageErrors), '壳层有未捕获错误').toEqual([])

    // ⑧ 数据根：必须是发行版里的独立目录，而不是 Windows 侧
    const info = await readRuntimeInfo(browser)
    expect(info.active_source).toBe('wsl')
    expect(info.data_dir).toContain(`\\\\wsl`)
    expect(info.data_dir).toContain(`\\home\\${E2E_WSL_USER}\\${E2E_WSL_HOME_DIR}`)
    expect(info.data_dir, 'WSL 核心不该用 Windows 侧数据根').not.toContain('AppData')
    // WSL 分支下这个字段来自发行版内的探测结果（`system_os.rs:apply_wsl_runtime_info`
    // 用 probe 的 nodeVersion 覆盖），不是桌面内嵌 Node 的版本
    expect(info.node_version, 'WSL 核心没有报出发行版内的 Node 版本').toBe('v22.22.1')

    // ⑨ 帧的地址就是 WSL 服务的地址
    //    壳层用 `generateTimestampedUrl`（`src/store/modules/harness/utils.ts:42-46`）拼 src，
    //    只加两个参数：`t=<Date.now()>` 与 `dshDesktop=1`——**没有 token**（token 是 dsh 自己
    //    URL 上的东西，见 `.harness.log` 里那条 `?token=…`），所以这里断言的是
    //    「src 是壳层按服务地址造的」，而不是「带了鉴权参数」
    expect(stampAfter.t, '挂载印记为空').not.toBe('')
    const infoHost = new URL(info.service_url).host
    const frameSrc = await browser.execute(readIframeSrc)
    expect(frameSrc, `iframe src 指向的不是 WSL 服务：${frameSrc}`).toContain(infoHost)
    expect(frameSrc, `iframe src 没有壳层的挂载标记：${frameSrc}`).toContain('dshDesktop=1')

    // ⑩ 发行版里确实有受控 runtime 拉起来的 dsh 进程
    expect(wslDshPids().length, `发行版里没有 dsh 进程：${JSON.stringify(wslDshPids())}`).toBeGreaterThan(0)
  }, CASE_TIMEOUT_MS)

  it('TC-WSL-L3-01-002 端口被占时 Linux 侧换端口，iframe 地址印记随之改变', async () => {
    const before = await readRuntimeInfo(browser)
    const stampBefore = await currentStamp(browser)
    expect(before.active_source).toBe('wsl')
    expect(stampBefore.t, '起点没有挂载中的 iframe').not.toBe('')

    // ① 先把服务端口钉到远离固定端口的位置：`update_app_config({ port })` 会**同时**写
    //    `manual_port`（见 updateAppPort 的注释），不钉就会被 heal 回默认端口，占端口也就白占了。
    //    这么做还有个硬理由：`assertPreconditions` 要求固定端口 3081 空闲才肯启动应用
    //    （`test/e2e/support/desktop.ts:349`），而镜像网络下「发行版里占住的端口」在 Windows 侧
    //    同样算被监听（第 10/11 次运行的实测）——服务只要还在 3081，占端口就等于把后续启动
    //    直接拦在前置校验上。
    const basePort = Number(new URL(before.service_url).port)
    const pinTarget = basePort + 10
    await updateAppPort(browser, pinTarget)

    const stampPinned = await restartAppHost(stampBefore)
    const pinnedPort = Number(new URL((await readRuntimeInfo(browser)).service_url).port)
    expect(pinnedPort, `端口设置没生效：期望 >= ${pinTarget}，实际 ${pinnedPort}`).toBeGreaterThanOrEqual(pinTarget)
    const pidsPinned = wslDshPids()
    expect(pidsPinned.length, '钉端口后发行版里没有 dsh 进程').toBeGreaterThan(0)

    // ② 停机 → 在发行版内占住这个端口 → 冷启动：端口扫描必须避开它。
    //    顺序不能反：服务还听着这个端口时占端口只会误判成「占成功」（见 occupyPortInDistro）。
    //    占端口本身必须做在发行版内：Windows 侧绑不上 Linux 正在服务的端口，
    //    也证明不了 `resolve_port_wsl` 在发行版内的那次 bind 判定。
    await stopAppHost()
    expect(wslDshPids().length, '停机后发行版里还有 dsh 进程，占端口会误判').toBe(0)

    const blocker = occupyPortInDistro(pinnedPort)

    try {
      expect(distroPortListening(pinnedPort), `发行版内没有进程占住 ${pinnedPort}`).toBe(true)

      const stampAfter = await startAppHost(stampPinned)
      const after = await readRuntimeInfo(browser)

      expect(after.active_source).toBe('wsl')
      const portAfter = Number(new URL(after.service_url).port)
      expect(portAfter, `端口 ${pinnedPort} 被占着，服务却仍回到该端口`).not.toBe(pinnedPort)
      expect(portAfter).toBeGreaterThan(pinnedPort)
      expect(stampAfter.t, 'iframe 地址印记没变').not.toBe(stampPinned.t)

      const pidsAfter = wslDshPids()
      expect(pidsAfter.length).toBeGreaterThan(0)
      expect(
        pidsAfter.some(pid => !pidsPinned.includes(pid)),
        `发行版里的 dsh 进程没有换新：占端口前 ${JSON.stringify(pidsPinned)} → 避让后 ${JSON.stringify(pidsAfter)}`,
      ).toBe(true)
      // 端口写回设置：界面/后续启动都该沿用这个新端口
      const config = await updateWslDistro(browser, E2E_DISTRO)
      expect(config.port, '避让后的端口没有写回设置').toBe(portAfter)

      // 帧内确实渲染了新地址的页面；**帧内检查放最后**，而且必须出帧，
      // 否则 WebDriver 会话留在内嵌文档里，后面的用例全都调不到 IPC（见 leaveFrame 注释）
      await enterFrame(browser)
      try {
        expect(await browser.execute(dshRootRendered, DSH_ROOT), '换端口后帧内没有渲染 dsh 页面').toBe(true)
      }
      finally {
        await leaveFrame(browser)
      }
    }
    finally {
      releasePortInDistro(blocker)
    }
  }, CASE_TIMEOUT_MS)

  it('TC-WSL-L3-01-003 缺本机内核时 set_active_core 报前置错误且不把状态写坏', async () => {
    // 本跑的数据根在 scratch 里，既没有随包内核也没有已下载的内核槽位
    const before = await readRuntimeInfo(browser)
    expect(before.active_source).toBe('wsl')

    const attempt = await setActiveCore(browser, 'app-bundled')
    expect(attempt.error, '缺本机内核时 set_active_core 竟然成功了').toBeDefined()
    expect(attempt.error).toContain('CORE_BUNDLED_NOT_FOUND')

    // 状态没被写坏：活动核心仍是 wsl，服务还在，iframe 还挂着
    const after = await readRuntimeInfo(browser)
    expect(after.active_source).toBe('wsl')
    expect((await readCores(browser)).find(core => core.id === 'wsl')?.active).toBe(true)
    expect(wslDshPids().length).toBeGreaterThan(0)
    // 帧仍然挂载着（`src` 上有挂载印记）——比 `isDisplayed()` 更贴近「壳层仍认为服务健康」
    expect((await currentStamp(browser)).t, '切换失败后 iframe 被撤下了').not.toBe('')
  }, RESTART_TIMEOUT_MS)

  it('TC-WSL-L3-01-004 核心列表契约：wsl 行标识、UNC 目录与可移除性', async () => {
    const row = (await readCores(browser)).find(core => core.id === 'wsl')
    expect(row, 'get_cores() 里没有 wsl 行').toBeDefined()
    expect(row!.source).toBe('wsl')
    expect(row!.removable, 'wsl 行不该可移除').toBe(false)
    expect(row!.dir, `wsl 行的 dir 不是 UNC：${row!.dir}`).toMatch(/^\\\\wsl(\.localdomain|\.localhost)?\\/)
    if (row!.present)
      expect(row!.error, 'present 的行不该带错误').toBeNull()
    else
      expect(row!.error, 'absent 的行必须说明原因').not.toBeNull()
  }, RESTART_TIMEOUT_MS)

  it('TC-WSL-L3-01-005 相同基线重复安装不重装、也不把桌面推荐值带给 WSL', async () => {
    // 对应用户场景 #4「重复安装」与 #5「已有 runtime 时的桌面推荐值不污染 WSL 目标」：
    // 这两项此前只有脚本与静态证据，这里走应用自己的 IPC 取证。
    const before = await readRuntimeInfo(browser)
    expect(before.active_source, '本用例要求服务正跑在 WSL 核心上').toBe('wsl')

    const probeBefore = await probeWslCore(browser, E2E_DISTRO)
    expect(probeBefore.dshVersion, '受控运行时不是 U8.4 装的那个版本').toBe('0.1.2-rc.1')
    expect(probeBefore.skipAuthReady, '受控运行时缺少 --skip-auth 补丁').toBe(true)

    // WSL 目标版本来自独立推荐值，且必须等于已装基线（桌面推荐值 0.2.0-rc.2 不许出现在这里）
    expect(await getWslRecommendedVersion(browser), 'WSL 独立推荐值不等于已装基线').toBe('0.1.2-rc.1')

    const stateBefore = readManagedRuntimeState()
    expect(stateBefore, '正常路径下不该存在候选/备份/失败槽位').toContain('slots=none')
    expect(stateBefore, '现场快照缺少全局 CLI 一项').toContain('global_dsh=')

    // 服务在跑时再调一次安装：相同基线必须走 `install.rs:411-420` 的早退，而该早退在
    // running 检查**之前**（`:421-423` 才是 `WSL_INSTALL_BUSY`）——所以这里既不该报忙，
    // 也不该重放 `npm ci`（首装实测 76 s）。
    const started = Date.now()
    const again = await invoke<unknown>(browser, 'install_wsl_core', { distro: E2E_DISTRO })
    const elapsed = Date.now() - started
    const error = invokeError(again)
    expect(error, `相同基线重复安装报错（报 WSL_INSTALL_BUSY 即说明被判成「需要更新」）：${error ?? ''}`).toBeUndefined()
    const probeAgain = again as WslCoreProbe
    expect(probeAgain.dshVersion, '重复安装后受控版本变了').toBe('0.1.2-rc.1')
    expect(probeAgain.skipAuthReady, '重复安装后补丁状态变了').toBe(true)
    expect(elapsed, `重复安装耗时 ${elapsed} ms，像是在重放 npm ci（首装 76 s）`).toBeLessThan(30_000)

    // 「什么都没动」：哈希、包锁时间、槽位三者逐字一致
    expect(readManagedRuntimeState(), '重复安装改动了受控运行时现场').toBe(stateBefore)

    // 服务没被这次调用弄坏：进程还在、地址没变、iframe 还挂着
    expect(wslDshPids().length, '重复安装后发行版里的 dsh 进程没了').toBeGreaterThan(0)
    expect((await readRuntimeInfo(browser)).service_url).toBe(before.service_url)
    expect((await currentStamp(browser)).t, '重复安装后 iframe 被撤下了').not.toBe('')
  }, CASE_TIMEOUT_MS)
})
