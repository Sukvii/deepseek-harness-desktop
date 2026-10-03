import { PLUGIN_ID } from './shared/constants'

export const name = PLUGIN_ID

export const inject = [
  'tools',
  'webServer',
  'agents',
  'sessions',
  'workspaceRegistry',
  'agentDefaultModel',
  'agentPresets',
  'permissionPresets',
  'llm',
  'connection',
]

export { apply } from './host/apply'

export type { Config } from './host/apply'
