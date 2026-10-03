import { defineRoutes } from 'dsh-tauri'
import bindings from './bindings/get'
import attach from './bindings/post'
import checkout from './checkouts/post'
import deleteWorktree from './delete'
import postWorktree from './post'
import status from './status/get'

export const routes = defineRoutes((disposer) => {
  disposer.post({ kind: 'exact', path: '/api/desktop/dsh-tauri-worktree' }, postWorktree)
  disposer.delete({ kind: 'exact', path: '/api/desktop/dsh-tauri-worktree' }, deleteWorktree)
  disposer.get({ kind: 'exact', path: '/api/desktop/dsh-tauri-worktree/bindings' }, bindings)
  disposer.get({ kind: 'exact', path: '/api/desktop/dsh-tauri-worktree/status' }, status)
  disposer.post({ kind: 'exact', path: '/api/desktop/dsh-tauri-worktree/bindings' }, attach)
  disposer.post({ kind: 'exact', path: '/api/desktop/dsh-tauri-worktree/checkouts' }, checkout)
})
