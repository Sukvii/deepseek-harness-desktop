import type { SchedulerTask } from '../types'
import { describe, expect, it } from 'vitest'
import { isTaskDue, selectWaitingTaskIds } from './waiting'

const NOW = Date.parse('2026-09-16T10:00:00.000Z')
const DUE = '2026-09-16T09:59:00.000Z'
const FUTURE = '2026-09-16T11:00:00.000Z'

function makeTask(id: string, nextRunAt?: string, enabled = true): SchedulerTask {
  return {
    id,
    name: id,
    prompt: 'prompt',
    schedule: { kind: 'daily', time: '09:00', timeZone: 'UTC' },
    enabled,
    createdAt: '2026-09-16T00:00:00.000Z',
    updatedAt: '2026-09-16T00:00:00.000Z',
    nextRunAt,
  }
}

describe('isTaskDue', () => {
  it('treats an enabled task whose nextRunAt has passed as due', () => {
    expect(isTaskDue(makeTask('a', DUE), NOW)).toBe(true)
    expect(isTaskDue(makeTask('a', DUE, false), NOW)).toBe(false)
    expect(isTaskDue(makeTask('a', FUTURE), NOW)).toBe(false)
    expect(isTaskDue(makeTask('a'), NOW)).toBe(false)
  })
})

describe('selectWaitingTaskIds', () => {
  it('marks every due task as waiting when no capacity is left', () => {
    const tasks = [makeTask('a', DUE), makeTask('b', DUE)]
    expect([...selectWaitingTaskIds(tasks, new Set(), 0, NOW)]).toEqual(['a', 'b'])
  })

  it('keeps the tasks that this tick can start out of the waiting set', () => {
    const tasks = [makeTask('a', DUE), makeTask('b', DUE), makeTask('c', DUE)]
    expect([...selectWaitingTaskIds(tasks, new Set(), 1, NOW)]).toEqual(['b', 'c'])
  })

  it('counts in-flight runs against the capacity', () => {
    const tasks = [makeTask('a', DUE), makeTask('b', DUE)]
    expect([...selectWaitingTaskIds(tasks, new Set(['z']), 1, NOW)]).toEqual(['b'])
  })

  it('never marks a running, paused or future task as waiting', () => {
    const tasks = [makeTask('a', DUE), makeTask('b', DUE, false), makeTask('c', FUTURE)]
    expect([...selectWaitingTaskIds(tasks, new Set(['a']), 0, NOW)]).toEqual([])
  })
})
