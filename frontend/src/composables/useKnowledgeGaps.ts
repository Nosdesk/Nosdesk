/**
 * Pinia Colada composables for the Knowledge Gaps queue.
 *
 * Cache key namespace: `knowledge-gaps`. Mutations invalidate
 * narrowly (the specific gap detail) plus broadly (the list)
 * so the queue view, dashboard widget, and ticket-side flag
 * indicator all refresh off a single write.
 *
 * The flag/unflag mutations also invalidate the *ticket* detail
 * cache so a ticket sidebar that surfaces "this is flagged"
 * reads refreshes from the same write that re-renders the queue.
 */
import { computed, type MaybeRefOrGetter, toValue } from 'vue'
import { useMutation, useQuery, useQueryCache } from '@pinia/colada'
import knowledgeGapsService, {
  type KnowledgeGap,
  type KnowledgeGapStatus,
} from '@/services/knowledgeGapsService'

export const knowledgeGapKeys = {
  root: ['knowledge-gaps'] as const,
  list: (statuses: KnowledgeGapStatus[]) =>
    ['knowledge-gaps', 'list', [...statuses].sort().join(',')] as const,
  detail: (gapId: number) => ['knowledge-gaps', 'detail', gapId] as const,
  forTicket: (ticketId: number) =>
    ['knowledge-gaps', 'ticket', ticketId] as const,
}

const DEFAULT_STATUSES: KnowledgeGapStatus[] = ['open', 'drafting']

/**
 * List of gaps for the queue view. Default status filter is the
 * "active" set (open + drafting). Caller can override for
 * resolved/dismissed history views.
 */
export function useKnowledgeGaps(
  statusesGetter?: MaybeRefOrGetter<KnowledgeGapStatus[]>,
) {
  const statuses = computed(() => toValue(statusesGetter) ?? DEFAULT_STATUSES)
  const query = useQuery({
    key: () => knowledgeGapKeys.list(statuses.value),
    query: () =>
      knowledgeGapsService.listKnowledgeGaps({ status: statuses.value }),
  })
  return {
    gaps: computed(() => query.data.value ?? []),
    isLoading: computed(
      () => query.status.value === 'pending' && query.data.value === undefined,
    ),
    isError: computed(() => query.status.value === 'error'),
    refetch: query.refetch,
  }
}

export function useKnowledgeGap(
  gapIdGetter: MaybeRefOrGetter<number | null | undefined>,
) {
  const query = useQuery({
    key: () => knowledgeGapKeys.detail(toValue(gapIdGetter) ?? 0),
    query: () => knowledgeGapsService.getKnowledgeGap(toValue(gapIdGetter)!),
    enabled: () => !!toValue(gapIdGetter),
  })
  return {
    gap: computed<KnowledgeGap | null>(() => query.data.value ?? null),
    isLoading: computed(
      () => query.status.value === 'pending' && query.data.value === undefined,
    ),
    isError: computed(() => query.status.value === 'error'),
    refetch: query.refetch,
  }
}

/**
 * Read-only helper: does a manual flag exist for this ticket?
 * Used by the ticket sidebar to decide whether to show "Flag for
 * documentation" or "Flagged for documentation". Returns null
 * when there's no flag, or the gap when there is. Cached per
 * ticket so multiple sidebar widgets share the lookup.
 */
export function useTicketFlagState(
  ticketIdGetter: MaybeRefOrGetter<number | null | undefined>,
) {
  const query = useQuery({
    key: () => knowledgeGapKeys.forTicket(toValue(ticketIdGetter) ?? 0),
    query: async (): Promise<KnowledgeGap | null> => {
      const id = toValue(ticketIdGetter)
      if (!id) return null
      // No dedicated endpoint yet — list active gaps and find one
      // whose source matches. The active set is small in practice
      // and the cache is per-ticket so this stays cheap.
      const gaps = await knowledgeGapsService.listKnowledgeGaps({
        status: ['open', 'drafting'],
        limit: 200,
      })
      // Need to fetch detail to inspect signals; do it for the
      // first matching candidate. For 2a, walk the list and pick
      // the gap whose title carries "Ticket #{id}:" (the seed
      // format flag_ticket uses). Cheaper than N detail calls.
      const candidate = gaps.find((g) =>
        g.title.startsWith(`Ticket #${id}:`),
      )
      if (!candidate) return null
      return await knowledgeGapsService.getKnowledgeGap(candidate.id)
    },
    enabled: () => !!toValue(ticketIdGetter),
  })
  return {
    gap: computed(() => query.data.value ?? null),
    isFlagged: computed(() => query.data.value !== null && query.data.value !== undefined),
    isLoading: computed(() => query.status.value === 'pending'),
    refetch: query.refetch,
  }
}

// -----------------------------------------------------------------
// Mutations
// -----------------------------------------------------------------

interface FlagTicketPayload {
  ticketId: number
  reason?: string
}

export function useFlagTicketMutation() {
  const queryCache = useQueryCache()
  return useMutation({
    mutation: ({ ticketId, reason }: FlagTicketPayload) =>
      knowledgeGapsService.flagTicketAsGap(ticketId, reason),
    onSettled: (_data, _err, vars) => {
      if (!vars) return
      queryCache.invalidateQueries({ key: knowledgeGapKeys.root })
      queryCache.invalidateQueries({
        key: knowledgeGapKeys.forTicket(vars.ticketId),
      })
    },
  })
}

export function useUnflagTicketMutation() {
  const queryCache = useQueryCache()
  return useMutation({
    mutation: ({ ticketId }: { ticketId: number }) =>
      knowledgeGapsService.unflagTicketAsGap(ticketId),
    onSettled: (_data, _err, vars) => {
      if (!vars) return
      queryCache.invalidateQueries({ key: knowledgeGapKeys.root })
      queryCache.invalidateQueries({
        key: knowledgeGapKeys.forTicket(vars.ticketId),
      })
    },
  })
}

export function useDismissGapMutation() {
  const queryCache = useQueryCache()
  return useMutation({
    mutation: ({ gapId }: { gapId: number }) =>
      knowledgeGapsService.dismissKnowledgeGap(gapId),
    onSettled: () => {
      queryCache.invalidateQueries({ key: knowledgeGapKeys.root })
    },
  })
}

interface ResolveGapPayload {
  gapId: number
  pageId: number
}

/** Mutation: run all auto-detection passes (cluster + failed
 *  search) in one click. Invalidates the whole knowledge-gaps
 *  cache since either pass can both create and update gaps.
 *  Returns the combined totals. */
export function useDetectClustersMutation() {
  const queryCache = useQueryCache()
  return useMutation({
    mutation: async (options?: { days?: number; minSize?: number }) => {
      const [clusters, searches, staleDocs] = await Promise.all([
        knowledgeGapsService.detectClusters(options ?? {}),
        knowledgeGapsService.detectFailedSearches({ days: options?.days }),
        knowledgeGapsService.detectStaleDocs({ recentTicketDays: options?.days }),
      ])
      const sum = (...vs: (number | undefined)[]) =>
        vs.reduce<number>((a, b) => a + (b ?? 0), 0)
      return {
        clusters_detected: sum(
          clusters?.clusters_detected,
          searches?.clusters_detected,
          staleDocs?.clusters_detected,
        ),
        gaps_created: sum(
          clusters?.gaps_created,
          searches?.gaps_created,
          staleDocs?.gaps_created,
        ),
        gaps_updated: sum(
          clusters?.gaps_updated,
          searches?.gaps_updated,
          staleDocs?.gaps_updated,
        ),
      }
    },
    onSettled: () => {
      queryCache.invalidateQueries({ key: knowledgeGapKeys.root })
    },
  })
}

export function useResolveGapMutation() {
  const queryCache = useQueryCache()
  return useMutation({
    mutation: ({ gapId, pageId }: ResolveGapPayload) =>
      knowledgeGapsService.resolveKnowledgeGap(gapId, pageId),
    onSettled: () => {
      queryCache.invalidateQueries({ key: knowledgeGapKeys.root })
      // Phase 1's docs<->tickets join carries the cascade so
      // those queries should refresh too.
      queryCache.invalidateQueries({ key: ['page-ticket-links'] })
    },
  })
}
