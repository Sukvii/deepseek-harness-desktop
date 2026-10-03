import type { EventHandlerRequest } from 'dsh-tauri'
import type { ActionResult, IdBody } from '../index.types'
import { defineEventHandler, readBody } from 'dsh-tauri'
import { runs } from '../../service/runs'

export default defineEventHandler<EventHandlerRequest, Promise<ActionResult>>(async (event) => {
  const body = await readBody<IdBody>(event)
  const id = typeof body?.id === 'string' ? body.id : ''
  if (id.length === 0) {
    event.res.status = 400
    return { error: '缺少执行记录 id' }
  }
  if (!await runs.remove(id)) {
    event.res.status = 400
    return { error: '执行记录不存在' }
  }
  return { ok: true }
})
