/**
 * Put a saved row into a cached list where one row is the default (SLA
 * policies, working calendars): replace it by id, or add it, and when it is
 * now the default, clear the flag on the others, as the server did.
 */
export function withSavedRow<T extends { id: number; is_default: boolean }>(
  rows: readonly T[],
  saved: T,
): T[] {
  const found = rows.some((r) => r.id === saved.id)
  const merged = found ? rows.map((r) => (r.id === saved.id ? saved : r)) : [...rows, saved]
  if (!saved.is_default) return merged
  return merged.map((r) => (r.id !== saved.id && r.is_default ? { ...r, is_default: false } : r))
}
