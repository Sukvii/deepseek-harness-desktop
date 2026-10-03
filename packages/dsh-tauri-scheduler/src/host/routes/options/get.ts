import type { EventHandlerRequest } from 'dsh-tauri'
import type { SchedulerOptions } from '../../types'
import { defineEventHandler } from 'dsh-tauri'
import { options } from '../../service/options'

export default defineEventHandler<EventHandlerRequest, Promise<SchedulerOptions>>(async () => options.resolve())
