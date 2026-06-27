/**
 * Logger wiring, the mobile twin of the web's
 * `frontend/src/utils/loggerSetup.ts`. Injects the two platform bits the
 * headless logger can't resolve itself: the minimum level and the current
 * user id.
 */
import { configureLogger, LogLevel } from '@nosdesk/core/utils/logger'

export interface MobileLoggerOptions {
  /** INFO in production, DEBUG otherwise. */
  isProd: boolean
  /** Resolve the current user id, to tag log entries. */
  getUserId?: () => string | undefined
}

export function setupLogger(opts: MobileLoggerOptions): void {
  configureLogger({
    minLevel: opts.isProd ? LogLevel.INFO : LogLevel.DEBUG,
    getUserId: opts.getUserId,
  })
}
