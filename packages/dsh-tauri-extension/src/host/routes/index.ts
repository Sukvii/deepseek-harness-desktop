import type { ExtensionRouteDeps } from './index.types'
import { defineRoutes } from 'dsh-tauri'
import restart from './host/restart/post'
import importApply from './import/apply/post'
import importScan from './import/scan/get'
import mcpCheck from './mcp/check/post'
import mcpCopy from './mcp/copy/post'
import mcpRemove from './mcp/delete'
import mcp from './mcp/get'
import mcpSave from './mcp/post'
import mcpToggle from './mcp/toggle/post'
import openDir from './open/dir/post'
import rootsRemove from './roots/delete'
import roots from './roots/get'
import rootsAdd from './roots/post'
import skillDelete from './skill/delete'
import skill from './skill/get'
import skillPolicy from './skill/policy/post'
import skillSave from './skill/post'
import skills from './skills/get'
import skillsRefresh from './skills/refresh/post'

export const routes = defineRoutes<ExtensionRouteDeps>((disposer) => {
  disposer.get({ kind: 'exact', path: '/api/desktop/dsh-tauri-extension/skills' }, skills)
  disposer.post({ kind: 'exact', path: '/api/desktop/dsh-tauri-extension/skills/refresh' }, skillsRefresh)
  disposer.get({ kind: 'exact', path: '/api/desktop/dsh-tauri-extension/skill' }, skill)
  disposer.post({ kind: 'exact', path: '/api/desktop/dsh-tauri-extension/skill' }, skillSave)
  disposer.delete({ kind: 'exact', path: '/api/desktop/dsh-tauri-extension/skill' }, skillDelete)
  disposer.post({ kind: 'exact', path: '/api/desktop/dsh-tauri-extension/skill/policy' }, skillPolicy)
  disposer.post({ kind: 'exact', path: '/api/desktop/dsh-tauri-extension/open/dir' }, openDir)

  disposer.get({ kind: 'exact', path: '/api/desktop/dsh-tauri-extension/mcp' }, mcp)
  disposer.post({ kind: 'exact', path: '/api/desktop/dsh-tauri-extension/mcp' }, mcpSave)
  disposer.delete({ kind: 'exact', path: '/api/desktop/dsh-tauri-extension/mcp' }, mcpRemove)
  disposer.post({ kind: 'exact', path: '/api/desktop/dsh-tauri-extension/mcp/toggle' }, mcpToggle)
  disposer.post({ kind: 'exact', path: '/api/desktop/dsh-tauri-extension/mcp/check' }, mcpCheck)
  disposer.post({ kind: 'exact', path: '/api/desktop/dsh-tauri-extension/mcp/copy' }, mcpCopy)

  disposer.get({ kind: 'exact', path: '/api/desktop/dsh-tauri-extension/import/scan' }, importScan)
  disposer.post({ kind: 'exact', path: '/api/desktop/dsh-tauri-extension/import/apply' }, importApply)

  disposer.get({ kind: 'exact', path: '/api/desktop/dsh-tauri-extension/roots' }, roots)
  disposer.post({ kind: 'exact', path: '/api/desktop/dsh-tauri-extension/roots' }, rootsAdd)
  disposer.delete({ kind: 'exact', path: '/api/desktop/dsh-tauri-extension/roots' }, rootsRemove)

  disposer.post({ kind: 'exact', path: '/api/desktop/dsh-tauri-extension/host/restart' }, restart)
})
