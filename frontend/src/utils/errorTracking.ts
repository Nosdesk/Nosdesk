/**
 * Error Tracking Integration
 *
 * This module provides a thin wrapper around error tracking services (e.g., Sentry).
 * Currently uses a stub implementation that can be easily replaced with actual Sentry integration.
 *
 * To enable Sentry:
 * 1. npm install @sentry/vue
 * 2. Uncomment the Sentry-specific code below
 * 3. Add VITE_SENTRY_DSN to .env.production
 * 4. Call initErrorTracking(app) in main.ts
 */

import type { App } from 'vue'
// import * as Sentry from '@sentry/vue'
// import router from '@/router'

export function initErrorTracking(app: App) {
  if (!import.meta.env.PROD) {
    console.log('Error tracking disabled in development mode')
    return
  }

  // Stub implementation - will log to console in production for now
  console.log('Error tracking initialized (stub)')

  /*
  // To enable Sentry, uncomment this block:

  const sentryDsn = import.meta.env.VITE_SENTRY_DSN

  if (!sentryDsn) {
    console.warn('VITE_SENTRY_DSN not configured - error tracking disabled')
    return
  }

  Sentry.init({
    app,
    dsn: sentryDsn,
    environment: import.meta.env.MODE,

    // Performance monitoring
    integrations: [
      Sentry.browserTracingIntegration({ router }),
      Sentry.replayIntegration({
        maskAllText: true,
        blockAllMedia: true
      })
    ],

    // Capture 100% of errors, 10% of performance traces
    tracesSampleRate: 0.1,
    replaysSessionSampleRate: 0.1,
    replaysOnErrorSampleRate: 1.0,

    // Filter out sensitive data
    beforeSend(event, hint) {
      // Remove sensitive headers
      if (event.request?.headers) {
        delete event.request.headers['X-CSRF-Token']
        delete event.request.headers['Authorization']
      }

      // Filter out specific errors
      if (hint.originalException instanceof Error) {
        if (hint.originalException.message.includes('Network Error')) {
          // Don't send network errors
          return null
        }
      }

      return event
    },

    // Ignore specific errors
    ignoreErrors: [
      'REQUEST_CANCELLED',
      'AbortError',
      'Network request failed',
      /^ResizeObserver/
    ]
  })

  console.log('Sentry error tracking initialized')
  */
}

export const ErrorTracker = {
  captureException: (error: unknown, context?: Record<string, any>) => {
    if (import.meta.env.PROD) {
      // Stub implementation - log to console
      console.error('[ErrorTracker] Exception:', error, context)

      /*
      // To enable Sentry, uncomment this:
      Sentry.captureException(error, {
        extra: context
      })
      */
    }
  },

  captureMessage: (message: string, level: 'info' | 'warning' | 'error' = 'info') => {
    if (import.meta.env.PROD) {
      // Stub implementation - log to console
      console.log(`[ErrorTracker] ${level.toUpperCase()}:`, message)

      /*
      // To enable Sentry, uncomment this:
      Sentry.captureMessage(message, level)
      */
    }
  },

  setUser: (user: { id: string; username?: string; email?: string }) => {
    if (import.meta.env.PROD) {
      // Stub implementation - log to console
      console.log('[ErrorTracker] User set:', user.id)

      /*
      // To enable Sentry, uncomment this:
      Sentry.setUser(user)
      */
    }
  },

  clearUser: () => {
    if (import.meta.env.PROD) {
      // Stub implementation - log to console
      console.log('[ErrorTracker] User cleared')

      /*
      // To enable Sentry, uncomment this:
      Sentry.setUser(null)
      */
    }
  }
}
