/**
 * Remote Logger - Sends frontend logs to backend for debugging
 * Useful for mobile development where DevTools isn't accessible
 */

import apiClient from '@/services/apiConfig';

type LogLevel = 'debug' | 'info' | 'warn' | 'error';

interface LogEntry {
  level: LogLevel;
  message: string;
  data?: any;
  timestamp: string;
  url: string;
  userAgent: string;
}

// Queue logs to batch send them
let logQueue: LogEntry[] = [];
let flushTimeout: ReturnType<typeof setTimeout> | null = null;
const FLUSH_INTERVAL = 1000; // Send logs every 1 second
const MAX_QUEUE_SIZE = 50; // Force flush if queue gets too large

// Check if remote logging is enabled
const isRemoteLoggingEnabled = () => {
  // TEMPORARY: Always enabled for debugging sync issues
  // In production, disable with: localStorage.setItem('remote-logging', 'false')
  const explicitDisable = localStorage.getItem('remote-logging') === 'false';
  return !explicitDisable;
};

// Send logs to backend
const flushLogs = async () => {
  if (logQueue.length === 0) return;

  const logsToSend = [...logQueue];
  logQueue = [];

  try {
    await apiClient.post('/debug/frontend-logs', { logs: logsToSend });
  } catch (e) {
    // Don't log errors about logging - that could cause infinite loops
    // Just silently fail
  }
};

// Schedule a flush
const scheduleFlush = () => {
  if (flushTimeout) return;

  flushTimeout = setTimeout(() => {
    flushTimeout = null;
    flushLogs();
  }, FLUSH_INTERVAL);
};

// Add log to queue
const queueLog = (level: LogLevel, message: string, data?: any) => {
  if (!isRemoteLoggingEnabled()) return;

  const entry: LogEntry = {
    level,
    message,
    data: data !== undefined ? safeStringify(data) : undefined,
    timestamp: new Date().toISOString(),
    url: window.location.href,
    userAgent: navigator.userAgent,
  };

  logQueue.push(entry);

  // Force flush if queue is getting large
  if (logQueue.length >= MAX_QUEUE_SIZE) {
    flushLogs();
  } else {
    scheduleFlush();
  }
};

// Safely stringify data, handling circular references
const safeStringify = (obj: any): string => {
  const seen = new WeakSet();
  return JSON.stringify(obj, (key, value) => {
    if (typeof value === 'object' && value !== null) {
      if (seen.has(value)) {
        return '[Circular]';
      }
      seen.add(value);
    }
    // Handle Error objects
    if (value instanceof Error) {
      return {
        name: value.name,
        message: value.message,
        stack: value.stack,
      };
    }
    return value;
  }, 2);
};

// Remote logger API
export const remoteLog = {
  debug: (message: string, data?: any) => queueLog('debug', message, data),
  info: (message: string, data?: any) => queueLog('info', message, data),
  warn: (message: string, data?: any) => queueLog('warn', message, data),
  error: (message: string, data?: any) => queueLog('error', message, data),

  // Force immediate flush (useful before page unload)
  flush: flushLogs,
};

// Store original console methods
const originalConsole = {
  log: console.log.bind(console),
  info: console.info.bind(console),
  warn: console.warn.bind(console),
  error: console.error.bind(console),
  debug: console.debug.bind(console),
};

// Intercept console methods to also send to backend
export const interceptConsole = () => {
  if (!isRemoteLoggingEnabled()) return;

  console.log = (...args: any[]) => {
    originalConsole.log(...args);
    queueLog('info', args.map(a => typeof a === 'string' ? a : safeStringify(a)).join(' '));
  };

  console.info = (...args: any[]) => {
    originalConsole.info(...args);
    queueLog('info', args.map(a => typeof a === 'string' ? a : safeStringify(a)).join(' '));
  };

  console.warn = (...args: any[]) => {
    originalConsole.warn(...args);
    queueLog('warn', args.map(a => typeof a === 'string' ? a : safeStringify(a)).join(' '));
  };

  console.error = (...args: any[]) => {
    originalConsole.error(...args);
    queueLog('error', args.map(a => typeof a === 'string' ? a : safeStringify(a)).join(' '));
  };

  console.debug = (...args: any[]) => {
    originalConsole.debug(...args);
    queueLog('debug', args.map(a => typeof a === 'string' ? a : safeStringify(a)).join(' '));
  };

  // Capture unhandled errors
  window.addEventListener('error', (event) => {
    queueLog('error', `Uncaught Error: ${event.message}`, {
      filename: event.filename,
      lineno: event.lineno,
      colno: event.colno,
      error: event.error,
    });
  });

  // Capture unhandled promise rejections
  window.addEventListener('unhandledrejection', (event) => {
    queueLog('error', `Unhandled Promise Rejection: ${event.reason}`, {
      reason: event.reason,
    });
  });

  // Flush logs before page unload
  window.addEventListener('beforeunload', () => {
    flushLogs();
  });

  console.info('[RemoteLogger] Console interception enabled - logs will be sent to backend');
};

// Export for manual control
export default remoteLog;
