import { defineRoutes } from 'dsh-tauri'
import openPath from './open/path/post'
import openUrl from './open/url/post'

export const routes = defineRoutes((disposer) => {
  disposer.post({ kind: 'exact', path: '/api/desktop/dsh-tauri-rightclick/open/url' }, openUrl)
  disposer.post({ kind: 'exact', path: '/api/desktop/dsh-tauri-rightclick/open/path' }, openPath)
})
