// @vitest-environment jsdom
import type { ReactNode } from 'react'
import type { Plugin } from '@/store/modules/plugins'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ConfigPlugin } from './plugin'

const { manager, openDialog, runtimeInfo } = vi.hoisted(() => ({
  manager: { installed: [] as Plugin[], processes: [], loading: false, error: '', disable: vi.fn(), enable: vi.fn() },
  openDialog: vi.fn(),
  /**
   * 面板不再自行比对 `active_core`，而是消费后端 `get_runtime_info` 的
   * `active_source`（R-U9-4）。这里把这个查询桩出来，让用例能按来源切换渲染。
   */
  runtimeInfo: { data: undefined as { active_source: string } | undefined },
}))

vi.mock('@/hooks/use-plugins-manager', () => ({ useDshPluginsManager: () => manager }))
vi.mock('@overlastic/react', () => ({ useOverlay: () => [null, openDialog] }))
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }))
vi.mock('@tanstack/react-query', () => ({
  useQueryClient: () => ({}),
  useMutation: () => ({}),
  useQuery: () => runtimeInfo,
}))
vi.mock('@/store', () => ({ store: { preinstall: { open: vi.fn(), installing: false }, setting: { active_core: null } } }))
vi.mock('valtio-define', () => ({ useStore: (value: unknown) => value }))
vi.mock('@/utils/toast', () => ({ toast: vi.fn() }))
vi.mock('@/components/modal', () => ({ Modal: () => null }))
vi.mock('@/components/item', () => ({ Item: ({ left, right }: { left: ReactNode, right: ReactNode }) => (
  <div>
    {left}
    {right}
  </div>
) }))
vi.mock('@/components/ellipsis', () => ({ Ellipsis: ({ children }: { children: ReactNode }) => <span>{children}</span> }))
vi.mock('@/components/panel', () => ({ Panel: { Header: () => null, Loadable: ({ children }: { children: ReactNode }) => <div>{children}</div> } }))
vi.mock('@heroui/react', () => ({
  Chip: ({ children, onClick }: { children: ReactNode, onClick?: () => void }) => <button type="button" onClick={onClick}>{children}</button>,
  Button: ({ children, onPress }: { children: ReactNode, onPress?: () => void }) => <button type="button" onClick={onPress}>{children}</button>,
  Input: () => null,
  Label: ({ children }: { children: ReactNode }) => <span>{children}</span>,
  Spinner: () => null,
  Switch: () => null,
  Tooltip: () => null,
}))

function plugin(overrides: Partial<Plugin> = {}): Plugin {
  return {
    id: 'dsh-tauri-pet',
    name: 'Desktop Pet',
    internal: true,
    disabled: false,
    patchDisabled: false,
    version: '1.0.0',
    description: '',
    repoUrl: '',
    bundled: true,
    recommended: false,
    fix: false,
    hasSnapshot: false,
    error: null,
    latest: null,
    updateAvailable: false,
    incompatible: false,
    latestIncompatible: false,
    ...overrides,
  }
}

function showBuiltIn() {
  render(<ConfigPlugin />)
  fireEvent.click(screen.getByRole('button', { name: 'plugins.builtin_title' }))
}

beforeEach(() => {
  manager.installed = [plugin()]
  manager.disable.mockReset().mockResolvedValue(undefined)
  manager.enable.mockReset().mockResolvedValue(undefined)
  openDialog.mockReset()
  runtimeInfo.data = { active_source: 'local' }
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('built-in plugin toggles', () => {
  it('waits for risk confirmation before disabling a built-in plugin', async () => {
    let confirm: () => void = () => {}
    openDialog.mockImplementation(() => new Promise<void>((resolve) => {
      confirm = resolve
    }))
    showBuiltIn()
    fireEvent.click(screen.getByRole('button', { name: 'plugins.disable' }))
    expect(openDialog).toHaveBeenCalledWith(expect.objectContaining({
      status: 'warning',
      title: 'plugins.disable_builtin_confirm_title',
      confirmText: 'plugins.disable',
    }))
    expect(manager.disable).not.toHaveBeenCalled()
    await act(async () => confirm())
    expect(manager.disable).toHaveBeenCalledExactlyOnceWith('dsh-tauri-pet')
  })

  it('does not disable when the risk dialog is cancelled', async () => {
    openDialog.mockRejectedValue(new Error('cancelled'))
    showBuiltIn()
    await act(async () => fireEvent.click(screen.getByRole('button', { name: 'plugins.disable' })))
    expect(openDialog).toHaveBeenCalledTimes(1)
    expect(manager.disable).not.toHaveBeenCalled()
  })

  it('enables a desktop-disabled built-in without showing a disable action', async () => {
    manager.installed = [plugin({ disabled: true })]
    showBuiltIn()
    expect(screen.queryByRole('button', { name: 'plugins.disable' })).toBeNull()
    await act(async () => fireEvent.click(screen.getByRole('button', { name: 'plugins.enable' })))
    expect(manager.enable).toHaveBeenCalledExactlyOnceWith('dsh-tauri-pet', { clearConfigOverride: false })
    expect(openDialog).not.toHaveBeenCalled()
  })

  it('keeps explicit confirmation for enabling a config-disabled built-in', async () => {
    manager.installed = [plugin({ patchDisabled: true })]
    openDialog.mockResolvedValue(undefined)
    showBuiltIn()
    await act(async () => fireEvent.click(screen.getByRole('button', { name: 'plugins.enable' })))
    expect(openDialog).toHaveBeenCalledWith(expect.objectContaining({ title: 'plugins.enable_override_confirm_title' }))
    expect(manager.enable).toHaveBeenCalledExactlyOnceWith('dsh-tauri-pet', { clearConfigOverride: true })
  })

  it('keeps non-built-in disabling direct', async () => {
    manager.installed = [plugin({ internal: false })]
    render(<ConfigPlugin />)
    await act(async () => fireEvent.click(screen.getByRole('button', { name: 'plugins.disable' })))
    expect(manager.disable).toHaveBeenCalledExactlyOnceWith('dsh-tauri-pet')
    expect(openDialog).not.toHaveBeenCalled()
  })
})

/**
 * R-U9-4：WSL 判定必须跟后端「有效 WSL 选择」一致（平台支持 + 发行版非空），
 * 不能只看 `active_core === 'wsl'`——清除发行版后后端会回落到本机来源。
 */
describe('来源判定与后端一致（R-U9-4）', () => {
  it('来源为 wsl 时显示说明而不挂载插件内容', () => {
    runtimeInfo.data = { active_source: 'wsl' }
    render(<ConfigPlugin />)
    expect(screen.getByText('plugins.wsl_active_notice')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'plugins.disable' })).toBeNull()
  })

  it('清除发行版后按本机来源渲染（即使设置里仍是 active_core=wsl）', () => {
    runtimeInfo.data = { active_source: 'local' }
    render(<ConfigPlugin />)
    expect(screen.queryByText('plugins.wsl_active_notice')).toBeNull()
    expect(screen.getByRole('button', { name: 'plugins.builtin_title' })).toBeTruthy()
  })

  it('来源尚未取到时按本机来源渲染，不留空白面板', () => {
    runtimeInfo.data = undefined
    render(<ConfigPlugin />)
    expect(screen.queryByText('plugins.wsl_active_notice')).toBeNull()
    expect(screen.getByRole('button', { name: 'plugins.builtin_title' })).toBeTruthy()
  })
})
