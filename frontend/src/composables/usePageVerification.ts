/**
 * Page verification mutations.
 *
 * Verification state lives directly on the page response (verified_by,
 * verified_at, verify_interval_days, is_stale), so no read query is
 * needed here — the page detail composable already surfaces it.
 * What this module provides is the *write* side: verify and
 * unverify mutations that invalidate the page detail cache so the
 * banner refreshes off the new state.
 */
import { useMutation, useQueryCache } from '@pinia/colada'
import documentationService from '@/services/documentationService'

interface VerifyPayload {
  pageId: string | number
  /** Days until the verification expires; null/undefined = never. */
  intervalDays?: number | null
}

export function useVerifyPageMutation() {
  const queryCache = useQueryCache()
  return useMutation({
    mutation: ({ pageId, intervalDays }: VerifyPayload) =>
      documentationService.verifyPage(pageId, intervalDays ?? null),
    onSettled: () => {
      queryCache.invalidateQueries({ key: ['documentation-page'] })
    },
  })
}

export function useUnverifyPageMutation() {
  const queryCache = useQueryCache()
  return useMutation({
    mutation: ({ pageId }: { pageId: string | number }) =>
      documentationService.unverifyPage(pageId),
    onSettled: () => {
      queryCache.invalidateQueries({ key: ['documentation-page'] })
    },
  })
}
