export type WorktreeBindings = {
  bindings: WorktreeBindingSummary[];
  jobs: WorktreeDiscardJobSummary[];
};
export type WorktreeBindingSummary = {
  sessionId: string;
  sourceSessionId: string;
  hash: string;
  dirname: string;
  worktreeKey: string;
  worktreePath: string;
  projectPath: string;
  log: string[];
};
export type WorktreeDiscardJobSummary = {
  sessionId: string;
  jobId: string;
  state: string;
  error?: string;
  worktreeKey: string;
  worktreePath?: string;
};
export type WorktreeAttach = {
  ok?: boolean;
  workspaceId?: string;
  error?: string;
};
export type WorktreeCheckout = {
  ok?: boolean;
  branch?: string;
  projectPath?: string;
  targetSessionId?: string;
  error?: string;
};
export type WorktreeDiscard = {
  ok?: boolean;
  jobId?: string;
  error?: string;
};
export type WorktreeCreate = {
  ok?: boolean;
  error?: string;
  hash?: string;
  dirname?: string;
  worktreeKey?: string;
  worktreePath?: string;
  projectPath?: string;
  sourceSessionId?: string;
  log?: string[];
  existed?: boolean;
  inherited?: boolean;
};
export type WorktreeStatus = {
  mode?: string;
  jobId?: string;
  error?: string;
  hash?: string;
  dirname?: string;
  worktreeKey?: string;
  worktreePath?: string;
  projectPath?: string;
  sourceSessionId?: string;
  log?: string[];
  isGit?: boolean | null;
};

export interface AttachBody {
  sessionId?: string;
}
export interface CheckoutBody {
  sessionId?: string;
  worktreeHashDirname?: string;
  branchName?: string;
  carryStaged?: boolean;
}
export interface DiscardBody {
  sessionId?: string;
  worktreeHashDirname?: string;
}
export interface CreateBody {
  sessionId?: string;
  sourceSessionId?: string;
  carryStaged?: boolean;
  inherit?: boolean;
}
export interface GetStatusQuery {
  sessionId?: string;
  jobId?: string;
}
