import type { EventHandlerRequest } from 'dsh-tauri'
import type { DiscardBody, WorktreeDiscard } from './index.types'
import { defineEventHandler, readBody } from 'dsh-tauri'
import { worktree } from '../service/worktree'

export default defineEventHandler<EventHandlerRequest, Promise<WorktreeDiscard>>(async (event) => {
  const body = (await readBody<DiscardBody>(event, { type: 'json' })) ?? {}
  const result = await worktree.discard(
    String(body.sessionId ?? ''),
    String(body.worktreeHashDirname ?? ''),
  )
  return result.ok ? { ok: true, jobId: result.jobId } : { ok: false, error: result.error }
})
