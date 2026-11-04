/**
 * The largest file this client may upload, checked from `File.size` before any
 * byte is read: the server's limit (`/api/config`) and, in the mobile app, the
 * cap of its native HTTP bridge, whichever is lower. With neither known,
 * nothing is refused here and the server's own check stands.
 */
import { getServerMaxUploadBytes } from './instanceConfig';

/** The axios error code a transport throws for a body over its cap. */
export const UPLOAD_TOO_LARGE = 'ERR_UPLOAD_TOO_LARGE';

let transportCapBytes: number | null = null;

/**
 * Set by a platform whose transport can't carry bodies past a size (the mobile
 * app copies each request body through its native bridge). `null` lifts it.
 */
export function setTransportUploadCap(bytes: number | null): void {
  transportCapBytes = bytes;
}

/** The transport's own cap in bytes, or null when it has none. */
export function getTransportUploadCap(): number | null {
  return transportCapBytes;
}

/** The largest file that may be uploaded now, in bytes, or null for no limit. */
export function uploadLimitBytes(): number | null {
  const limits = [getServerMaxUploadBytes(), transportCapBytes].filter(
    (n): n is number => n != null,
  );
  return limits.length ? Math.min(...limits) : null;
}

/** The files over {@link uploadLimitBytes}; reads only their sizes. */
export function oversizeFiles<T extends Blob>(files: readonly T[]): T[] {
  const limit = uploadLimitBytes();
  return limit == null ? [] : files.filter((f) => f.size > limit);
}

/** A byte count in whole megabytes, rounded down, for messages. */
export function formatUploadLimitMb(bytes: number): number {
  return Math.floor(bytes / (1024 * 1024));
}
