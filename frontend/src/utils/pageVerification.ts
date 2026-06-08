import type { DocPageRow } from '@/sync/stores/documentation'

export type PageVerificationState = 'never' | 'fresh' | 'stale'

/** Mirrors backend `is_page_stale` — interval elapsed since last verify. */
export function isPageStale(
  verifiedAt: string | null | undefined,
  verifyIntervalDays: number | null | undefined,
  nowMs: number = Date.now(),
): boolean {
  if (!verifiedAt || verifyIntervalDays == null) return false
  const verifiedMs = new Date(verifiedAt).getTime()
  if (Number.isNaN(verifiedMs)) return false
  const expiryMs = verifiedMs + verifyIntervalDays * 86_400_000
  return nowMs > expiryMs
}

export function pageVerificationState(
  row: Pick<DocPageRow, 'verified_at' | 'verify_interval_days'>,
): PageVerificationState {
  if (!row.verified_at) return 'never'
  return isPageStale(row.verified_at, row.verify_interval_days) ? 'stale' : 'fresh'
}

export function pageNeedsVerificationAttention(
  row: Pick<DocPageRow, 'verified_at' | 'verify_interval_days'>,
): boolean {
  const state = pageVerificationState(row)
  return state === 'never' || state === 'stale'
}
