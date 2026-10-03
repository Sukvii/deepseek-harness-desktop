import type { Config } from './config/onboarding'
import type { HostContext } from './types'
import { PLUGIN_ID } from '../shared/constants'
import { ONBOARDING_CONFIG_GLOBAL } from '../shared/onboarding-config'
import { setCurrentHostInstance } from './config/runtime'
import { routes } from './routes'

const INDEX_EFFECT = `${PLUGIN_ID}: onboarding config global`

const ROUTES_EFFECT = `${PLUGIN_ID}: routes`

const RUNTIME_EFFECT = `${PLUGIN_ID}: host runtime`

export function apply(ctx: HostContext, config?: Config): void {
  setCurrentHostInstance(ctx)

  ctx.effect(() => ctx.on('webserver/index-inject', (table) => {
    table.push({
      kind: 'global',
      name: ONBOARDING_CONFIG_GLOBAL,
      value: { credentialOnboarding: config?.credentialOnboarding ?? true },
    })
  }), INDEX_EFFECT)

  ctx.effect(() => routes(ctx), ROUTES_EFFECT)
  ctx.effect(() => () => setCurrentHostInstance(undefined), RUNTIME_EFFECT)
}
