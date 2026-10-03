export type OperationResult = {
  ok: boolean;
  error?: string;
};

export interface PostOpenPathBody {
  path?: string;
}
export interface PostOpenUrlBody {
  url?: string;
}
