/**
 * WSL 核心真机用例（`test/e2e/desktop/wsl-core.e2e.ts`）的宿主侧辅助。
 *
 * 纯 `node:child_process`，不经 shell（[] 传参，无引号转义问题）。
 * `wsl.exe` 的 stdout 是 UTF-16LE 且带 CRLF，解码口径集中在这里。
 *
 * 注意：`-u <user>` 会**覆盖**发行版默认用户，因此「默认用户是谁」必须用不带 `-u` 的
 * 调用来问——否则守卫会永远通过，测的就不是应用真实走的那条路径了。
 */

import type { Buffer } from 'node:buffer'
import type { SpawnSyncReturns } from 'node:child_process'
import { spawnSync } from 'node:child_process'

/** 真机用例约定的发行版与隔离测试用户（见 `docs/spec/WSL_CORE.zh.md` 的验收环境）。 */
export const E2E_DISTRO = 'Ubuntu'
export const E2E_WSL_USER = 'dshu8'

/** 发行版内 debug 构建的数据目录名（`service::wsl_core::dsh_home_dir_name`）。 */
export const E2E_WSL_HOME_DIR = '.dsh-desktop.dev'

export interface WslResult {
  /** 退出码；被信号终止时为 null。 */
  status: number | null
  stdout: string
  stderr: string
}

function decode(buffer: Buffer | string | null): string {
  if (buffer === null || buffer === undefined)
    return ''
  // 经 `wsl.exe -e <命令>` 跑出来的 stdout/stderr 是 **UTF-8**（只有 `wsl.exe --list`、
  // `--status` 这类「WSL 自己输出」的文本才是 UTF-16LE）。这里按 UTF-8 解；UTF-16 专用
  // 路径见 `decodeUtf16`。
  const text = typeof buffer === 'string' ? buffer : buffer.toString('utf8')
  return text.replace(/^\uFEFF/, '').replace(/\r\n/g, '\n').trim()
}

/** 解码 WSL 自身输出的 UTF-16LE 文本（`--list` 等），并剥掉 BOM。 */
export function decodeUtf16(buffer: Buffer | string | null): string {
  if (buffer === null || buffer === undefined)
    return ''
  const text = typeof buffer === 'string' ? buffer : buffer.toString('utf16le')
  return text.replace(/^\uFEFF/, '').replace(/\r\n/g, '\n').trim()
}

function run(args: string[], timeout: number): WslResult {
  const done: SpawnSyncReturns<Buffer> = spawnSync('wsl.exe', args, {
    timeout,
    windowsHide: true,
    encoding: 'buffer',
  })
  if (done.error) {
    return { status: null, stdout: '', stderr: String(done.error.message) }
  }
  return {
    status: done.status,
    stdout: decode(done.stdout),
    stderr: decode(done.stderr),
  }
}

/** 在指定发行版里跑一条 bash 命令（`-e` 形态，不经登录 shell 的二次展开）。 */
export function wslBash(script: string, options: { user?: string, distro?: string, timeout?: number } = {}): WslResult {
  const distro = options.distro ?? E2E_DISTRO
  const args = ['-d', distro]
  if (options.user !== undefined)
    args.push('-u', options.user)
  args.push('-e', 'bash', '-lc', script)
  return run(args, options.timeout ?? 60_000)
}

/**
 * 发行版当前生效的默认用户。
 *
 * 不带 `-u`：问的就是「应用启动 `wsl.exe -d <distro> -e bash -lc …` 时会落到谁家」。
 */
export function defaultWslUser(distro: string = E2E_DISTRO): string {
  // 用**内联实换行**的 `printf`，不用 `printf "\\n"` 那种转义：转义序列要经
  // Node → CreateProcess → WSL → bash 四层传递，任何一层吃掉反斜杠都会让输出粘成一行
  // （实测踩过：输出变成 `dshu8\n\n/home/dshu8`，`split('\n')` 于是把 home 读成空串）。
  const done = wslBash('printf "id=%s\\nhome=%s\\n" "$(id -un)" "$HOME"', { distro })
  if (done.status !== 0) {
    throw new Error(`无法读取 ${distro} 的默认用户（wsl.exe exit ${done.status}）：${done.stderr || done.stdout}`)
  }
  const match = /(?:^|\n)id=(.*)\nhome=(.*)/.exec(done.stdout)
  if (match === null) {
    throw new Error(`无法解析 ${distro} 的默认用户输出：${JSON.stringify(done.stdout)}`)
  }
  return `${(match[1] ?? '').trim()}:${(match[2] ?? '').trim()}`
}

/**
 * 发行版内 dsh 服务的进程号；没有匹配进程时返回空数组。
 *
 * 模式写成 `[b]in/dsh`：`pgrep -f` 会看到**自己**的命令行，方括号让自身命令行里出现的是
 * 字面量 `[b]in/dsh`，从而不匹配自己；末尾再按「含受控数据根路径」过滤一次兜底。
 */
export function wslDshPids(distro: string = E2E_DISTRO): number[] {
  const done = run(['-d', distro, '-e', 'pgrep', '-af', '[b]in/dsh'], 30_000)
  if (done.status !== 0 || done.stdout === '')
    return []
  return done.stdout
    .split('\n')
    .filter(line => line.includes(`${E2E_WSL_HOME_DIR}/runtime/node_modules/.bin/dsh`))
    .map(line => Number.parseInt(line.trim().split(/\s+/)[0] ?? '', 10))
    .filter(pid => Number.isInteger(pid) && pid > 0)
}

/** 隔离用户的受控 runtime 入口是否存在且可执行。 */
export function wslRuntimeEntryExists(distro: string = E2E_DISTRO): boolean {
  const done = wslBash(
    `test -x "$HOME/${E2E_WSL_HOME_DIR}/runtime/node_modules/.bin/dsh" && echo yes || echo no`,
    { distro },
  )
  return done.stdout.trim() === 'yes'
}
