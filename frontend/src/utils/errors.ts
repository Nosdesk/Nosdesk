import { translate } from '../i18n'
import { LogLevel } from '@nosdesk/core/utils/logger'

export abstract class AppError extends Error {
  public readonly timestamp: Date
  public readonly context?: Record<string, any>
  /**
   * The HTTP response this error was built from, in the shape an axios error
   * has it. Set by `createErrorFromResponse`, so call sites written against a
   * raw axios error (`err.response?.status`, `err.response?.data?.code`) keep
   * working after the API client's interceptor has typed the error.
   */
  public response?: { status: number; data?: any }

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
type AxiosResponseData = NonNullable<NonNullable<AxiosLikeError['response']>['data']>

interface AxiosLikeError {
  response?: {
    status: number;
    data?: {
      /** Canonical envelope: `{ error, code }`. */
      error?: string;
      code?: string;
      /** Older login-flow bodies: `{ status, message }`. */
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

/**
 * The response body of a failed request, from a raw axios error or a typed
 * `AppError` (both carry `response`), or undefined when there was none.
 */
export function errorBody(error: unknown): Record<string, any> | undefined {
  const data = (error as { response?: { data?: unknown } } | undefined)?.response?.data
  return data && typeof data === 'object' ? (data as Record<string, any>) : undefined
}

/** The server's machine-readable `code`, if the body carried one. */
export function errorCode(error: unknown): string | undefined {
  const code = errorBody(error)?.code
  return typeof code === 'string' ? code : undefined
}

/** The HTTP status, if the error came from a response. */
export function errorStatus(error: unknown): number | undefined {
  return (error as { response?: { status?: number } } | undefined)?.response?.status
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

  const { status, data } = axiosError.response
  const typed = typedErrorFromResponse(axiosError.response, text(data))
  typed.response = { status, data }
  return typed
}

function text(data: AxiosResponseData | undefined): string | undefined {
  // Two body shapes reach here: the canonical `{ error, code }` envelope and
  // the older `{ status, message }` login bodies. Read `message` first so the
  // older shape keeps its text; `error` carries it for everything else.
  return data?.message || data?.error
}

function typedErrorFromResponse(
  response: NonNullable<AxiosLikeError['response']>,
  text: string | undefined,
): AppError {
  const { status, data, config } = response
  if (status === 401) {
    return new AuthenticationError(
      text || 'Authentication required',
      { endpoint: config.url }
    )
  }

  if (status === 403) {
    return new PermissionError(
      text || 'Permission denied',
      data?.required_role
    )
  }

  if (status === 422) {
    return new ValidationError(
      text || 'Validation failed',
      data?.field,
      { errors: data?.errors }
    )
  }

  return new ApiError(
    text || 'An error occurred',
    status,
    config.url ?? '',
    { data }
  )
}

/**
 * True when an error (already mapped through `createErrorFromResponse` by the
 * apiClient interceptor) is an HTTP 404. Used by optimistic deletes to treat a
 * "not found" as already-deleted (keep the row removed) rather than rolling the
 * removal back, which would resurrect a row the server no longer has.
 */
export function isNotFoundError(err: unknown): boolean {
  return err instanceof ApiError && err.statusCode === 404
}
