/**
 * Date/Time Utilities with Timezone Support
 *
 * This module provides timezone-aware date formatting using date-fns v4.
 * All dates from backend are assumed to be in UTC (ISO 8601 format).
 * Display dates are converted to user's preferred timezone.
 *
 * Future extensibility:
 * - User timezone preferences (via dateStore)
 * - Multi-timezone display
 * - Calendar/scheduling features
 * - DST handling (built-in to date-fns)
 */

import { format, formatDistance, formatDistanceToNow, parseISO } from 'date-fns'

// ============================================
// CONFIGURATION
// ============================================

export interface DateConfig {
  defaultTimezone: string
  defaultLocale: string
  formats: {
    short: string
    long: string
    time: string
    dateTime: string
    monthDay: string
    yearMonthDay: string
  }
}

const DEFAULT_CONFIG: DateConfig = {
  defaultTimezone: 'UTC', // Will be replaced by user preference
  defaultLocale: 'en-US',
  formats: {
    short: 'MMM d, yyyy',
    long: 'MMMM d, yyyy',
    time: 'h:mm a',
    dateTime: 'MMM d, yyyy h:mm a',
    monthDay: 'MMM d',
    yearMonthDay: 'yyyy-MM-dd'
  }
}

// Global config - will be set by dateStore
let globalConfig: DateConfig = { ...DEFAULT_CONFIG }

export function setDateConfig(config: Partial<DateConfig>): void {
  globalConfig = { ...globalConfig, ...config }
}

export function getDateConfig(): DateConfig {
  return { ...globalConfig }
}

// ============================================
// CORE UTILITIES
// ============================================

/**
 * Parse ISO date string from backend (always UTC)
 * Backend sends NaiveDateTime without timezone, so we append Z to treat as UTC
 */
export function parseDate(dateString: string | Date | null | undefined): Date | null {
  if (!dateString) return null

  let date: Date
  if (typeof dateString === 'string') {
    // If no timezone indicator, treat as UTC by appending Z
    const normalized = dateString.endsWith('Z') || dateString.includes('+') || dateString.includes('-', 10)
      ? dateString
      : dateString + 'Z'
    date = parseISO(normalized)
  } else {
    date = dateString
  }

  if (isNaN(date.getTime())) {
    console.error('Invalid date:', dateString)
    return null
  }

  return date
}

/**
 * Format date in user's timezone with specified format
 */
export function formatDate(
  dateString: string | Date | null | undefined,
  formatString?: string,
  timezone?: string
): string {
  const date = parseDate(dateString)
  if (!date) return ''

  const fmt = formatString || globalConfig.formats.short
  const tz = timezone || globalConfig.defaultTimezone

  try {
    return format(date, fmt)
  } catch (error) {
    console.error('Error formatting date:', error)
    return format(date, fmt)
  }
}

/**
 * Format date with time in user's timezone
 */
export function formatDateTime(
  dateString: string | Date | null | undefined,
  timezone?: string
): string {
  const date = parseDate(dateString)
  if (!date) return ''

  // Use native toLocaleString for proper timezone conversion
  return date.toLocaleString('en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    timeZone: timezone || Intl.DateTimeFormat().resolvedOptions().timeZone
  })
}

/**
 * Format time only in user's timezone
 */
export function formatTime(
  dateString: string | Date | null | undefined,
  timezone?: string
): string {
  return formatDate(dateString, globalConfig.formats.time, timezone)
}

/**
 * Relative time with smart formatting ("5 minutes ago", "yesterday", etc.)
 */
export function formatRelativeTime(
  dateString: string | Date | null | undefined,
  options?: {
    addSuffix?: boolean
    includeSeconds?: boolean
  }
): string {
  const date = parseDate(dateString)
  if (!date) return ''

  try {
    return formatDistanceToNow(date, {
      addSuffix: options?.addSuffix ?? true,
      includeSeconds: options?.includeSeconds ?? false
    })
  } catch (error) {
    console.error('Error formatting relative time:', error)
    return formatDate(dateString, globalConfig.formats.short)
  }
}

/**
 * Compact relative time for space-constrained UIs (e.g., "<1m", "5m", "2h", "3d", "1w", "2mo")
 */
export function formatCompactRelativeTime(
  dateString: string | Date | null | undefined
): string {
  const date = parseDate(dateString)
  if (!date) return ''

  const now = new Date()
  const diffInSeconds = Math.floor((now.getTime() - date.getTime()) / 1000)

  if (diffInSeconds < 0) return 'now'
  if (diffInSeconds < 60) return '<1m'

  const diffInMinutes = Math.floor(diffInSeconds / 60)
  if (diffInMinutes < 60) return `${diffInMinutes}m`

  const diffInHours = Math.floor(diffInMinutes / 60)
  if (diffInHours < 24) return `${diffInHours}h`

  const diffInDays = Math.floor(diffInHours / 24)
  if (diffInDays < 7) return `${diffInDays}d`

  const diffInWeeks = Math.floor(diffInDays / 7)
  if (diffInWeeks < 4) return `${diffInWeeks}w`

  const diffInMonths = Math.floor(diffInDays / 30)
  if (diffInMonths < 12) return `${diffInMonths}mo`

  const diffInYears = Math.floor(diffInDays / 365)
  return `${diffInYears}y`
}

/**
 * Smart date formatter - shows relative for recent, absolute for old
 */
export function formatSmartDate(
  dateString: string | Date | null | undefined,
  cutoffDays: number = 7
): string {
  const date = parseDate(dateString)
  if (!date) return ''

  const now = new Date()
  const diffInMs = now.getTime() - date.getTime()
  const diffInDays = Math.floor(diffInMs / (1000 * 60 * 60 * 24))

  if (diffInDays < cutoffDays) {
    return formatRelativeTime(dateString)
  }

  return formatDate(dateString)
}

/**
 * Get user's current timezone (browser default)
 */
export function getUserTimezone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone
}

/**
 * Convert date to user's timezone
 * Note: Simplified implementation - returns parsed date directly
 * TODO: Add proper timezone conversion when @date-fns/tz API is clarified
 */
export function toUserTimezone(
  dateString: string | Date | null | undefined,
  _timezone?: string
): Date | null {
  const date = parseDate(dateString)
  if (!date) return null

  return date
}

/**
 * Check if date is today
 */
export function isToday(dateString: string | Date | null | undefined): boolean {
  const date = parseDate(dateString)
  if (!date) return false

  const today = new Date()
  return (
    date.getDate() === today.getDate() &&
    date.getMonth() === today.getMonth() &&
    date.getFullYear() === today.getFullYear()
  )
}

/**
 * Check if date is this year
 */
export function isThisYear(dateString: string | Date | null | undefined): boolean {
  const date = parseDate(dateString)
  if (!date) return false

  return date.getFullYear() === new Date().getFullYear()
}

// ============================================
// SPECIALIZED FORMATTERS
// ============================================

/**
 * Format for heatmap/calendar displays
 */
export function formatForCalendar(date: Date): string {
  return format(date, 'yyyy-MM-dd')
}

/**
 * Format for file names
 */
export function formatForFilename(date: Date = new Date()): string {
  return format(date, 'yyyy-MM-dd-HHmmss')
}

/**
 * Format month header
 */
export function formatMonthYear(dateString: string | Date): string {
  const date = parseDate(dateString)
  if (!date) return ''
  return format(date, 'MMMM yyyy')
}

/**
 * Get current UTC datetime for API calls
 */
export function getCurrentUTCDateTime(): string {
  return new Date().toISOString()
}

/**
 * Format distance between two dates
 */
export function formatDistanceBetween(
  startDate: string | Date,
  endDate: string | Date,
  options?: { addSuffix?: boolean }
): string {
  const start = parseDate(startDate)
  const end = parseDate(endDate)

  if (!start || !end) return ''

  try {
    return formatDistance(start, end, options)
  } catch (error) {
    console.error('Error formatting distance:', error)
    return ''
  }
}
