import type { EventHandlerRequest } from 'dsh-tauri'
import type { ActionResult } from '../../index.types'
import { defineEventHandler } from 'dsh-tauri'
import { recovery } from '../../../service/recovery'

export default defineEventHandler<EventHandlerRequest, Promise<ActionResult>>(async () => {
  await recovery.recover()
  return { ok: true }
})
