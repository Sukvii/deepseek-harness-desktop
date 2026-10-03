/**
 * WSL 核心运行时信息的「尚未探测」标记（R-U9-2 / R-U9-3）。
 *
 * 与后端 `src-tauri/src/bridge/system_os.rs` 的 `DATA_DIR_PROBE_PENDING` 逐字一致：
 * 该函数在 WSL 分支下**不用宿主路径兜底**，probe 缓存缺失时把 `data_dir` 置成这个
 * 标记、`node_version` 置空。前端据此把该字段渲染成「未探测」提示并禁用「打开数据
 * 目录」按钮，而不是显示一个会误导人的 Windows 路径（后端也会拒绝用这个标记当路径）。
 *
 * 两侧改动必须同步：`test/wsl-runtime-info.test.ts` 会直接读后端源码做字符串守卫。
 */
export const WSL_DATA_DIR_PROBE_PENDING = '(WSL: not probed yet)'

/** 该值是否为「WSL 未探测」标记 */
export function isWslDataDirProbePending(dataDir: string | null | undefined): boolean {
  return (dataDir ?? '').trim() === WSL_DATA_DIR_PROBE_PENDING
}
