// @vitest-environment jsdom
import type { ReactNode } from 'react'
import type { Plugin } from '@/store/modules/plugins'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ConfigPlugin } from './plugin'

const { manager, openDialog } = vi.hoisted(() => ({
  manager: { installed: [] as Plugin[], processes: [], loading: false, error: '', disable: vi.fn(), enable: vi.fn() },
  openDialog: vi.fn(),
}))

vi.mock('@/hooks/use-plugins-manager', () => ({ useDshPluginsManager: () => manager }))
vi.mock('@overlastic/react', () => ({ useOverlay: () => [null, openDialog] }))
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }))
vi.mock('@tanstack/react-query', () => ({ useQueryClient: () => ({}), useMutation: () => ({}) }))
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
