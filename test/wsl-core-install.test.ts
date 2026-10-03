import type { InstallProgress } from '@/store/modules/harness/types'
import type { WslCoreProbe } from '@/types'
import { describe, expect, it, vi } from 'vitest'
import { runWslCoreInstall } from '@/ui/dialog/wsl-core-install'

/** R-U9-1：安装对话框「订阅成功 → 未卸载 → 才安装」的时序契约 */

/** 只关心版本字段的探测结果桩：其余字段与断言无关，故只补必要形状 */
const PROBE_STUB = { distro: 'Ubuntu' } as unknown as WslCoreProbe

/** 手工可控的 subscribe 实现：显式 resolve / reject，并把进度回调留给测试驱动 */
function deferredSubscribe() {
  let resolveFn: ((fn: () => void) => void) | undefined
  let rejectFn: ((err: unknown) => void) | undefined
  let handler: ((payload: InstallProgress) => void) | undefined
  const promise = new Promise<() => void>((resolve, reject) => {
    resolveFn = resolve
    rejectFn = reject
  })
  return {
    promise,
    /** 记录调用方传入的进度回调，供 `emit` 触发（模拟 tauri event 到达） */
    subscribe: (h: (payload: InstallProgress) => void) => {
      handler = h
      return promise
    },
    resolve: (unlisten: () => void) => resolveFn!(unlisten),
    reject: (err: unknown) => rejectFn!(err),
    emit: (payload: InstallProgress) => handler?.(payload),
    hasHandler: () => handler != null,
  }
}

const installStub = () => vi.fn(async () => PROBE_STUB)

const progress = (payload: Partial<InstallProgress>) => payload as InstallProgress

describe('runWslCoreInstall 时序（R-U9-1）', () => {
  it('订阅未完成时不发起安装', async () => {
    const sub = deferredSubscribe()
    const install = installStub()
    runWslCoreInstall({
      subscribe: sub.subscribe,
      onProgress: () => {},
      install,
      isCancelled: () => false,
      onConfirm: () => {},
      onError: () => {},
    })

    // 让微任务队列跑空：订阅仍 pending，安装必须尚未发生
    await Promise.resolve()
    await Promise.resolve()
    expect(install).not.toHaveBeenCalled()

    sub.resolve(() => {})
    await vi.waitFor(() => expect(install).toHaveBeenCalledTimes(1))
  })

  it('订阅 resolve 后只安装一次，并把探测结果交给 onConfirm', async () => {
    const sub = deferredSubscribe()
    const install = installStub()
    const onConfirm = vi.fn()
    runWslCoreInstall({
      subscribe: sub.subscribe,
      onProgress: () => {},
      install,
      isCancelled: () => false,
      onConfirm,
      onError: () => {},
    })

    sub.resolve(() => {})
    await vi.waitFor(() => expect(onConfirm).toHaveBeenCalledTimes(1))
    expect(install).toHaveBeenCalledTimes(1)
    expect(onConfirm.mock.calls[0][0]).toBe(PROBE_STUB)
  })

  it('订阅失败时不发起安装，错误进入对话框', async () => {
    const sub = deferredSubscribe()
    const install = installStub()
    const onError = vi.fn()
    const onConfirm = vi.fn()
    runWslCoreInstall({
      subscribe: sub.subscribe,
      onProgress: () => {},
      install,
      isCancelled: () => false,
      onConfirm,
      onError,
    })

    sub.reject(new Error('listen denied'))
    await vi.waitFor(() => expect(onError).toHaveBeenCalledTimes(1))
    expect(onError.mock.calls[0][0]).toContain('listen denied')
    expect(install).not.toHaveBeenCalled()
    expect(onConfirm).not.toHaveBeenCalled()
  })

  it('订阅 resolve 前已卸载：立即注销且不安装', async () => {
    const sub = deferredSubscribe()
    const install = installStub()
    const unlisten = vi.fn()
    let cancelled = false
    const cleanup = runWslCoreInstall({
      subscribe: sub.subscribe,
      onProgress: () => {},
      install,
      isCancelled: () => cancelled,
      onConfirm: () => {},
      onError: () => {},
    })

    cancelled = true
    cleanup()
    sub.resolve(unlisten)
    await vi.waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1))
    expect(install).not.toHaveBeenCalled()
  })

  it('安装完成后卸载：注销监听且不再回调', async () => {
    const sub = deferredSubscribe()
    const install = installStub()
    const unlisten = vi.fn()
    const onConfirm = vi.fn()
    let cancelled = false
    const cleanup = runWslCoreInstall({
      subscribe: sub.subscribe,
      onProgress: () => {},
      install,
      isCancelled: () => cancelled,
      onConfirm,
      onError: () => {},
    })

    sub.resolve(unlisten)
    await vi.waitFor(() => expect(onConfirm).toHaveBeenCalledTimes(1))

    cancelled = true
    cleanup()
    expect(unlisten).toHaveBeenCalledTimes(1)
    expect(onConfirm).toHaveBeenCalledTimes(1)
  })

  it('安装失败时把错误交给 onError，且不触发 onConfirm', async () => {
    const sub = deferredSubscribe()
    const onError = vi.fn()
    const onConfirm = vi.fn()
    runWslCoreInstall({
      subscribe: sub.subscribe,
      onProgress: () => {},
      install: async () => {
        throw new Error('npm ci failed')
      },
      isCancelled: () => false,
      onConfirm,
      onError,
    })

    sub.resolve(() => {})
    await vi.waitFor(() => expect(onError).toHaveBeenCalledTimes(1))
    expect(onError.mock.calls[0][0]).toContain('npm ci failed')
    expect(onConfirm).not.toHaveBeenCalled()
  })

  it('进度事件原样交给 onProgress（type 过滤由调用方负责）', async () => {
    const sub = deferredSubscribe()
    const seen: InstallProgress[] = []
    runWslCoreInstall({
      subscribe: sub.subscribe,
      onProgress: payload => seen.push(payload),
      install: installStub(),
      isCancelled: () => false,
      onConfirm: () => {},
      onError: () => {},
    })

    // 订阅函数同步注册回调（tauri `listen` 语义）：resolve 前就应已登记
    expect(sub.hasHandler()).toBe(true)

    sub.resolve(() => {})
    sub.emit(progress({ type: 'wsl-core', percentage: 42 }))
    expect(seen).toHaveLength(1)
    expect(seen[0]).toMatchObject({ type: 'wsl-core', percentage: 42 })
  })
})
