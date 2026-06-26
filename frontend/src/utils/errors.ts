import { translate } from '../i18n'
import { LogLevel } from './logger'

export abstract class AppError extends Error {
  public readonly timestamp: Date
  public readonly context?: Record<string, any>

  constructor(message: string, context?: Record<string, any>) {
    super(message)
    this.name = this.constructor.name
    this.timestamp = new Date()
    this.context = context

    // Maintains proper stack trace for where error was thrown
    if (Error.captureStackTrace) {
      Error.captureStackTrace(this, this.constructor)
    }
  }

  abstract getUserMessage(): string
  abstract getLogLevel(): LogLevel
}

export class ValidationError extends AppError {
  constructor(message: string, public readonly field?: string, context?: Record<string, any>) {
    super(message, context)
  }

  getUserMessage(): string {
    return this.field
      ? `Invalid ${this.field}: ${this.message}`
      : `Validation error: ${this.message}`
  }

  getLogLevel(): LogLevel {
    return LogLevel.WARN
  }
}

export class ApiError extends AppError {
  constructor(
    message: string,
    public readonly statusCode: number,
    public readonly endpoint: string,
    context?: Record<string, any>
  ) {
    super(message, context)
  }

  getUserMessage(): string {
    if (this.statusCode === 404) {
      return translate('error-resource-not-found', undefined, 'The requested resource was not found.')
    }
    if (this.statusCode >= 500) {
      return translate('error-api-server', undefined, 'A server error occurred. Please try again later.')
    }
    if (this.statusCode === 422) {
      return this.message || translate('error-api-validation', undefined, 'The provided data is invalid.')
    }
    return this.message || translate('error-api-generic', undefined, 'An error occurred while processing your request.')
  }

  getLogLevel(): LogLevel {
    return this.statusCode >= 500 ? LogLevel.ERROR : LogLevel.WARN
  }
}

export class NetworkError extends AppError {
  constructor(message: string = 'Network request failed', context?: Record<string, any>) {
    super(message, context)
  }

  getUserMessage(): string {
    // Deliberately does NOT mention "internet": this fires whenever a request
    // got no response, which on an internal deployment (admin tools talking to
    // on-prem services) is usually the app server being down/unreachable, not
    // the operator's connectivity.
    return translate('error-network', undefined, "Couldn't reach the server. It may be offline or unreachable.")
  }

  getLogLevel(): LogLevel {
    return LogLevel.ERROR
  }
}

/** A request that got no response because it ran past the client timeout. The
 *  operation may still be completing on the server (e.g. a long directory sync),
 *  so this is distinct from an unreachable server. */
export class TimeoutError extends AppError {
  constructor(message: string = 'Request timed out', context?: Record<string, any>) {
    super(message, context)
  }

  getUserMessage(): string {
    return translate('error-timeout', undefined, 'The request timed out. The server may still be processing it.')
  }

  getLogLevel(): LogLevel {
    return LogLevel.WARN
  }
}

export class AuthenticationError extends AppError {
  constructor(message: string = 'Authentication failed', context?: Record<string, any>) {
    super(message, context)
  }

  getUserMessage(): string {
    return translate('error-session-expired', undefined, 'Your session has expired. Please log in again.')
  }

  getLogLevel(): LogLevel {
    return LogLevel.WARN
  }
}

export class PermissionError extends AppError {
  constructor(message: string = 'Permission denied', public readonly requiredRole?: string) {
    super(message, { requiredRole })
  }

  getUserMessage(): string {
    return translate('error-forbidden', undefined, 'You do not have permission to perform this action.')
  }

  getLogLevel(): LogLevel {
    return LogLevel.WARN
  }
}

// Error factory for creating errors from API responses
interface AxiosLikeError {
  response?: {
    status: number;
    data?: {
      message?: string;
      required_role?: string;
      field?: string;
      errors?: Record<string, string[]>;
    };
    config: { url?: string };
  };
  /** Axios sets ECONNABORTED on a client timeout, ERR_NETWORK on an unreachable
   *  server. We split these into TimeoutError vs NetworkError. */
  code?: string;
  message?: string;
}

/**
 * Extract a user-facing message from any error shape. Tries the
 * server-provided body first (`response.data.message` / `.error`),
 * falls back to the supplied default. Use this everywhere you'd
 * otherwise show a generic "Please try again" — the server almost
 * always knows more about what went wrong than the frontend does,
 * and surfacing its message lets operators self-debug instead of
 * retrying a doomed action.
 */
export function extractErrorMessage(error: unknown, fallback: string): string {
  if (!error) return fallback
  const e = error as AxiosLikeError
  const fromBody = e.response?.data?.message ?? (e.response?.data as { error?: string } | undefined)?.error
  if (typeof fromBody === 'string' && fromBody.trim().length > 0) return fromBody
  if (typeof e.message === 'string' && e.message.trim().length > 0 && e.message !== 'Request failed') {
    return e.message
  }
  return fallback
}

export function createErrorFromResponse(error: unknown): AppError {
  const axiosError = error as AxiosLikeError;
  if (!axiosError.response) {
    // No HTTP response. Split a client-side timeout (the op may still be running
    // server-side) from a genuinely unreachable server, so they read differently.
    const code = axiosError.code
    const timedOut =
      code === 'ECONNABORTED' || code === 'ETIMEDOUT' || /timeout/i.test(axiosError.message ?? '')
    return timedOut
      ? new TimeoutError('Request timed out', { originalError: axiosError.message })
      : new NetworkError('Network request failed', { originalError: axiosError.message })
  }

  const { status, data, config } = axiosError.response

  if (status === 401) {
    return new AuthenticationError(
      data?.message || 'Authentication required',
      { endpoint: config.url }
    )
  }

  if (status === 403) {
    return new PermissionError(
      data?.message || 'Permission denied',
      data?.required_role
    )
  }

  if (status === 422) {
    return new ValidationError(
      data?.message || 'Validation failed',
      data?.field,
      { errors: data?.errors }
    )
  }

  return new ApiError(
    data?.message || 'An error occurred',
    status,
    config.url ?? '',
    { data }
  )
}
