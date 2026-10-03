export type ArchivedListPayload = {
  archivedSessionIds: string[];
  meta: Record<string, { createdAt?: number; cwd?: string; title?: string }>;
};
export type OpenSessionDirResult = {
  ok: boolean;
  error?: string;
};

export interface SessionIdBody {
  sessionId: string;
}
export interface SessionIdsBody {
  sessionIds: string[];
}
export interface WorkspaceArchiveBody {
  sessionIds: string[];
  workspaceId?: string;
}
