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

/**
 * Whether a page should surface in a "needs verification" list.
 *
 * Mirrors the backend gate (`requires_verification`): a never-verified
 * page only needs attention when its collection has verification
 * required. A page that was verified with a cadence and has since
 * lapsed is `stale` regardless of the collection flag, so it always
 * surfaces. `requiresVerification` is the page's collection flag and
 * is required so callers can't accidentally re-introduce the bug
 * where every unverified page was flagged.
 */
export function pageNeedsVerificationAttention(
  row: Pick<DocPageRow, 'verified_at' | 'verify_interval_days'>,
  requiresVerification: boolean,
): boolean {
  const state = pageVerificationState(row)
  if (state === 'stale') return true
  if (state === 'never') return requiresVerification
  return false
}
