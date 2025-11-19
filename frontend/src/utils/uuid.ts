/**
 * Generate a UUID v4 with progressive fallback for maximum compatibility
 * Following RFC 4122 specification
 */
export function uuid(): string {
  // Try native crypto.randomUUID if available (most modern browsers, requires HTTPS)
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    try {
      return crypto.randomUUID();
    } catch (e) {
      // Fall through to next method if it fails (e.g., in non-secure context)
      console.warn('crypto.randomUUID failed, using fallback');
    }
  }

  // Use crypto.getRandomValues if available (works in more contexts)
  if (typeof crypto !== 'undefined' && typeof crypto.getRandomValues === 'function') {
    try {
      return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
        const r = (crypto.getRandomValues(new Uint8Array(1))[0] % 16) | 0;
        const v = c === 'x' ? r : (r & 0x3) | 0x8;
        return v.toString(16);
      });
    } catch (e) {
      // Fall through to Math.random if crypto fails
      console.warn('crypto.getRandomValues failed, using Math.random fallback');
    }
  }

  // Final fallback using Math.random (works everywhere, less secure)
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, function(c) {
    const r = Math.random() * 16 | 0;
    const v = c === 'x' ? r : (r & 0x3 | 0x8);
    return v.toString(16);
  });
}
