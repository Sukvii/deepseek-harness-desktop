import { defineRoutes } from 'dsh-tauri'
import resume from './session/resume/post'
import ungrouped from './ungrouped/get'

export const routes = defineRoutes((disposer) => {
  disposer.post({ kind: 'exact', path: '/api/desktop/dsh-tauri-ui/session/resume' }, resume)
  disposer.get({ kind: 'exact', path: '/api/desktop/dsh-tauri-ui/ungrouped' }, ungrouped)
})
