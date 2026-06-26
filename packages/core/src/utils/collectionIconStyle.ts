/** Muted tile background for a collection emoji icon. */
export function collectionIconBackgroundStyle(color?: string | null): Record<string, string> {
  if (color) {
    return { backgroundColor: `color-mix(in srgb, ${color} 18%, transparent)` }
  }
  return { backgroundColor: 'color-mix(in srgb, var(--color-accent) 12%, transparent)' }
}
