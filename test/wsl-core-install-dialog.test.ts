// @vitest-environment jsdom
import type { WslCoreProbe } from '@/types'
import { cleanup, render } from '@testing-library/react'
import { createElement } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'

/**
 * R-U9-1 的面板流程回归：**对话框本体**（不只是纯函数）必须做到
 * 「进度订阅完成 → 确认未卸载 → 才发起安装」。
 *
 * 为什么要有这一层：缺陷正是「订阅与安装是两条互不等待的独立链」，只测
 * `runWslCoreInstall` 纯函数不能证明组件真的接了它。这里挂载真实对话框，
 * 让 `listen` 返回**不 resolve 的 promise**，断言安装一次都没发生；手动
 * resolve 后才允许安装，且只发生一次。
 *
 * 文件名是 `.test.ts`（unit 项目的 include 只收 `test/` 下的 `.test.ts` 与
 * `src/` 下的 `.test.ts`/`.test.tsx`），因此这里用 `createElement` 而非 JSX。
 * 环境用文件级 `@vitest-environment jsdom`。
 */

/**
 * 可控的订阅桩：`vi.mock` 的工厂会被提升到文件顶部，共享状态必须用
 * `vi.hoisted` 一起提升，否则工厂执行时它还在暂时性死区里。
 */
const listenStub = vi.hoisted(() => ({
  resolvers: [] as ((unlisten: () => void) => void)[],
  rejecters: [] as ((err: unknown) => void)[],
  calls: 0,
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: () => {
    listenStub.calls += 1
    return new Promise<() => void>((resolve, reject) => {
      listenStub.resolvers.push(resolve)
      listenStub.rejecters.push(reject)
    })
  },
}))

// 对话框只需要 overlastic 的「已打开」状态；其余渲染细节与本用例无关
vi.mock('@overlastic/react', () => ({
  useDisclosure: () => ({
    visible: true,
    confirm: vi.fn(),
    cancel: vi.fn(),
  }),
}))

const { WslCoreInstallDialog } = await import('@/ui/dialog/wsl-core-install')

const PROBE = { distro: 'Ubuntu' } as unknown as WslCoreProbe

function renderDialog(runInstall: () => Promise<WslCoreProbe>) {
  return render(createElement(WslCoreInstallDialog, { distro: 'Ubuntu', runInstall }))
}

/** 让已排队的微任务跑干净（组件 effect 里的 promise 链） */
async function flushMicrotasks(times = 6) {
  for (let i = 0; i < times; i += 1)
    await Promise.resolve()
}

afterEach(() => {
  cleanup()
  listenStub.resolvers = []
  listenStub.rejecters = []
  listenStub.calls = 0
})

describe('安装对话框挂载流程（R-U9-1）', () => {
  it('订阅尚未完成时不调用 runInstall', async () => {
    const runInstall = vi.fn(async () => PROBE)
    renderDialog(runInstall)

    await flushMicrotasks()

    expect(listenStub.calls).toBe(1)
    expect(runInstall).not.toHaveBeenCalled()
  })

  it('订阅完成后只调用一次 runInstall', async () => {
    const runInstall = vi.fn(async () => PROBE)
    renderDialog(runInstall)
    await flushMicrotasks()

    listenStub.resolvers[0](() => {})
    await flushMicrotasks()

    expect(runInstall).toHaveBeenCalledTimes(1)
  })

  it('订阅失败时不调用 runInstall（错误留在对话框内）', async () => {
    const runInstall = vi.fn(async () => PROBE)
    renderDialog(runInstall)
    await flushMicrotasks()

    listenStub.rejecters[0](new Error('listen denied'))
    await flushMicrotasks()

    expect(runInstall).not.toHaveBeenCalled()
  })

  it('卸载早于订阅 resolve：注销监听且不调用 runInstall', async () => {
    const runInstall = vi.fn(async () => PROBE)
    const unlisten = vi.fn()
    const view = renderDialog(runInstall)
    await flushMicrotasks()

    view.unmount()
    listenStub.resolvers[0](unlisten)
    await flushMicrotasks()

    expect(unlisten).toHaveBeenCalledTimes(1)
    expect(runInstall).not.toHaveBeenCalled()
  })
})
