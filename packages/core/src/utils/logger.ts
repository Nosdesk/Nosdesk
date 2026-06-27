/**
 * Structured console logger.
 *
 * Headless: it writes to `console` and carries no DOM/transport/env coupling,
 * so it travels to the mobile app unchanged. The two platform-specific bits,
 * the minimum level (prod vs dev) and how to resolve the current user id, are
 * injected by the host via `configureLogger` at bootstrap (web:
 * frontend/src/utils/loggerSetup.ts).
 *
 * Backend log forwarding is NOT done here. The web app mirrors console output
 * to the backend separately (frontend/src/utils/remoteLogger.ts); this module
 * stays a pure console sink.
 */
export enum LogLevel {
  DEBUG = 0,
  INFO = 1,
  WARN = 2,
  ERROR = 3,
  FATAL = 4
}

export interface LogEntry {
  level: LogLevel
  message: string
  timestamp: string
  context?: Record<string, unknown>
  correlationId?: string
  userId?: string
}

/** Platform bits the host injects at bootstrap. */
export interface LoggerHost {
  /** Minimum level to emit. Host sets it per environment (prod: INFO). */
  minLevel?: LogLevel
  /** Resolve the current user id, to tag entries. Defaults to none. */
  getUserId?: () => string | undefined
}

class Logger {
  private minLevel: LogLevel = LogLevel.DEBUG
  private getUserId: () => string | undefined = () => undefined
  private correlationId: string | null = null

  configure(host: LoggerHost) {
    if (host.minLevel !== undefined) this.minLevel = host.minLevel
    if (host.getUserId) this.getUserId = host.getUserId
  }

  setCorrelationId(id: string | null) {
    this.correlationId = id
  }

  debug(message: string, context?: unknown) {
    this.log(LogLevel.DEBUG, message, context as Record<string, unknown> | undefined)
  }

  info(message: string, context?: unknown) {
    this.log(LogLevel.INFO, message, context as Record<string, unknown> | undefined)
  }

  warn(message: string, context?: unknown) {
    this.log(LogLevel.WARN, message, context as Record<string, unknown> | undefined)
  }

  error(message: string, context?: unknown) {
    this.log(LogLevel.ERROR, message, context as Record<string, unknown> | undefined)
  }

  fatal(message: string, context?: unknown) {
    this.log(LogLevel.FATAL, message, context as Record<string, unknown> | undefined)
  }

  private log(level: LogLevel, message: string, context?: Record<string, unknown>) {
    if (level < this.minLevel) {
      return
    }

    const entry: LogEntry = {
      level,
      message,
      timestamp: new Date().toISOString(),
      context: this.sanitizeContext(context),
      correlationId: this.correlationId ?? undefined,
      userId: this.getUserId()
    }

    this.logToConsole(entry)
  }

  private logToConsole(entry: LogEntry) {
    const prefix = `[${entry.timestamp}]`
    const levelColors = {
      [LogLevel.DEBUG]: 'color: gray',
      [LogLevel.INFO]: 'color: blue',
      [LogLevel.WARN]: 'color: orange',
      [LogLevel.ERROR]: 'color: red',
      [LogLevel.FATAL]: 'color: red; font-weight: bold'
    }

    const logFn = {
      [LogLevel.DEBUG]: console.debug,
      [LogLevel.INFO]: console.info,
      [LogLevel.WARN]: console.warn,
      [LogLevel.ERROR]: console.error,
      [LogLevel.FATAL]: console.error
    }[entry.level]

    if (entry.context) {
      logFn(`%c${prefix} ${entry.message}`, levelColors[entry.level], entry.context)
    } else {
      logFn(`%c${prefix} ${entry.message}`, levelColors[entry.level])
    }
  }

  private sanitizeContext(context?: Record<string, unknown>): Record<string, unknown> | undefined {
    if (!context) return undefined

    const sanitized = { ...context }

    // Remove sensitive fields
    const sensitiveKeys = ['password', 'token', 'secret', 'authorization', 'csrf']
    for (const key of Object.keys(sanitized)) {
      if (sensitiveKeys.some(sk => key.toLowerCase().includes(sk))) {
        sanitized[key] = '[REDACTED]'
      }
    }

    return sanitized
  }
}

// Singleton instance. Runs with dev-friendly defaults until the host calls
// `configureLogger` at bootstrap.
export const logger = new Logger()

/** Inject host platform bits (min level, user-id resolver). Called once at startup. */
export function configureLogger(host: LoggerHost) {
  logger.configure(host)
}
