/**
 * Web wiring for @nosdesk/core's headless logger.
 *
 * Injects the two platform bits the logger can't resolve itself: the minimum
 * level (INFO in prod, DEBUG in dev) and the current user id (read from the
 * persisted `user`). Runs at module load so it's set before app logging; the
 * mobile app ships its own equivalent.
 */
import { configureLogger, LogLevel } from '@nosdesk/core/utils/logger'

configureLogger({
  minLevel: import.meta.env.PROD ? LogLevel.INFO : LogLevel.DEBUG,
  getUserId: () => {
    try {
      const userStr = localStorage.getItem('user')
      if (!userStr) return undefined
      return (JSON.parse(userStr) as { uuid?: string }).uuid
    } catch {
      return undefined
    }
  },
})
