import type { SessionHost } from '../types'
import { defineHostRuntime } from 'dsh-tauri'

export const { setCurrentHostInstance, getCurrentHostInstance } = defineHostRuntime<SessionHost>()

export function clearHostRuntime(): void {
  setCurrentHostInstance(undefined)
}
