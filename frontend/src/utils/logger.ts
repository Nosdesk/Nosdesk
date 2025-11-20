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
  context?: Record<string, any>
  correlationId?: string
  userId?: string
}

export interface LoggerConfig {
  minLevel: LogLevel
  enableConsole: boolean
  enableRemote: boolean
  remoteEndpoint?: string
  bufferSize?: number
  flushInterval?: number
}

class Logger {
  private config: LoggerConfig
  private buffer: LogEntry[] = []
  private flushTimer?: ReturnType<typeof setTimeout>
  private correlationId: string | null = null

  constructor(config?: Partial<LoggerConfig>) {
    this.config = {
      minLevel: import.meta.env.PROD ? LogLevel.INFO : LogLevel.DEBUG,
      enableConsole: true,
      enableRemote: import.meta.env.PROD,
      remoteEndpoint: '/api/logs',
      bufferSize: 50,
      flushInterval: 5000,
      ...config
    }

    // Start flush timer if remote logging enabled
    if (this.config.enableRemote) {
      this.startFlushTimer()
    }

    // Flush on page unload
    if (typeof window !== 'undefined') {
      window.addEventListener('beforeunload', () => this.flush())
    }
  }

  setCorrelationId(id: string | null) {
    this.correlationId = id
  }

  debug(message: string, context?: any) {
    this.log(LogLevel.DEBUG, message, context)
  }

  info(message: string, context?: any) {
    this.log(LogLevel.INFO, message, context)
  }

  warn(message: string, context?: any) {
    this.log(LogLevel.WARN, message, context)
  }

  error(message: string, context?: any) {
    this.log(LogLevel.ERROR, message, context)
  }

  fatal(message: string, context?: any) {
    this.log(LogLevel.FATAL, message, context)
  }

  private log(level: LogLevel, message: string, context?: Record<string, any>) {
    if (level < this.config.minLevel) {
      return
    }

    const entry: LogEntry = {
      level,
      message,
      timestamp: new Date().toISOString(),
      context: this.sanitizeContext(context),
      correlationId: this.correlationId ?? undefined,
      userId: this.getCurrentUserId()
    }

    // Console output
    if (this.config.enableConsole) {
      this.logToConsole(entry)
    }

    // Remote logging
    if (this.config.enableRemote) {
      this.buffer.push(entry)

      if (this.buffer.length >= (this.config.bufferSize ?? 50)) {
        this.flush()
      }
    }
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

  private sanitizeContext(context?: Record<string, any>): Record<string, any> | undefined {
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

  private getCurrentUserId(): string | undefined {
    try {
      // Try to get from localStorage or store
      const userStr = localStorage.getItem('user')
      if (userStr) {
        const user = JSON.parse(userStr)
        return user.uuid
      }
    } catch {
      // Ignore errors
    }
    return undefined
  }

  private startFlushTimer() {
    this.flushTimer = setInterval(() => {
      this.flush()
    }, this.config.flushInterval)
  }

  private async flush() {
    if (this.buffer.length === 0) return

    const logs = [...this.buffer]
    this.buffer = []

    try {
      // Get CSRF token from cookie
      const csrfToken = document.cookie
        .split('; ')
        .find(row => row.startsWith('csrf_token='))
        ?.split('=')[1]

      const headers: Record<string, string> = {
        'Content-Type': 'application/json'
      }

      if (csrfToken) {
        headers['X-CSRF-Token'] = csrfToken
      }

      await fetch(this.config.remoteEndpoint!, {
        method: 'POST',
        headers,
        body: JSON.stringify({ logs }),
        credentials: 'include'
      })
    } catch (err) {
      // Failed to send logs - restore buffer
      console.error('Failed to send logs to server:', err)
      this.buffer.unshift(...logs)
    }
  }

  destroy() {
    if (this.flushTimer) {
      clearInterval(this.flushTimer)
    }
    this.flush()
  }
}

// Export singleton instance
export const logger = new Logger()

// Allow configuration
export function configureLogger(config: Partial<LoggerConfig>) {
  logger.destroy()
  return new Logger(config)
}
