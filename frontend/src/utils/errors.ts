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
      return 'The requested resource was not found.'
    }
    if (this.statusCode >= 500) {
      return 'A server error occurred. Please try again later.'
    }
    if (this.statusCode === 422) {
      return this.message || 'The provided data is invalid.'
    }
    return this.message || 'An error occurred while processing your request.'
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
    return 'Unable to connect to the server. Please check your internet connection.'
  }

  getLogLevel(): LogLevel {
    return LogLevel.ERROR
  }
}

export class AuthenticationError extends AppError {
  constructor(message: string = 'Authentication failed', context?: Record<string, any>) {
    super(message, context)
  }

  getUserMessage(): string {
    return 'Your session has expired. Please log in again.'
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
    return 'You do not have permission to perform this action.'
  }

  getLogLevel(): LogLevel {
    return LogLevel.WARN
  }
}

// Error factory for creating errors from API responses
interface AxiosLikeError {
  response?: {
    status: number;
    data?: { message?: string; required_role?: string };
    config: { url?: string };
  };
  message?: string;
}

export function createErrorFromResponse(error: unknown): AppError {
  const axiosError = error as AxiosLikeError;
  if (!axiosError.response) {
    return new NetworkError('Network request failed', {
      originalError: axiosError.message
    })
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
    config.url,
    { data }
  )
}
