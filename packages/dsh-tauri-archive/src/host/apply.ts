import type { HostContext } from 'dsh-tauri'
import type { SessionHost } from './types'
import { PLUGIN_ID } from '../shared/constants'
import { clearHostRuntime, setCurrentHostInstance } from './config/runtime'
import { routes } from './routes'

export function apply(ctx: HostContext): void {
  setCurrentHostInstance(ctx as unknown as SessionHost)

  ctx.effect(() => routes(ctx), `${PLUGIN_ID}: routes`)
  ctx.effect(() => () => clearHostRuntime(), `${PLUGIN_ID}: host runtime`)
}
