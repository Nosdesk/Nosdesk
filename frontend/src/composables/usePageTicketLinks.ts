/**
 * Page <-> Ticket link queries and mutations.
 *
 * Phase 1 of the docs/KB redesign hooks every link/unlink and every
 * verification toggle through this single composable so that:
 *   - the doc detail view, the ticket detail view, and any future
 *     widget that surfaces the relationship share one cache entry,
 *   - mutations invalidate both ends in one place (linking a ticket
 *     to a doc has to refresh both the page-side list and the
 *     ticket-side list, not just one),
 *   - SSE-driven invalidations have an obvious target.
 *
 * Cache keys are namespaced under `page-ticket-links` so a single
 * `invalidateQueries({ key: pageTicketLinkKeys.root })` clears the
 * whole tree if the backend ever fans out a "links changed somewhere"
 * SSE event.
 */
import { computed, type ComputedRef, type MaybeRefOrGetter, toValue } from 'vue'
import { useQuery, useMutation, useQueryCache } from '@pinia/colada'
import documentationService, {
  type PageTicketLink,
  type TicketDocLink,
} from '@/services/documentationService'

export const pageTicketLinkKeys = {
  root: ['page-ticket-links'] as const,
  forPage: (pageId: string | number) =>
    ['page-ticket-links', 'page', String(pageId)] as const,
  forTicket: (ticketId: number) =>
    ['page-ticket-links', 'ticket', ticketId] as const,
}

/**
 * Read tickets currently linked to a page. Source for the doc
 * detail view's "Resolved / Referenced" panel.
 *
 * `pageId` is a getter so the caller can pass a route-bound ref
 * without having to re-mount the composable on navigation.
 */
export function usePageTickets(pageId: MaybeRefOrGetter<string | number | null | undefined>) {
  const query = useQuery({
    key: () => pageTicketLinkKeys.forPage(toValue(pageId) ?? ''),
    query: () => documentationService.listPageTickets(toValue(pageId)!),
    enabled: () => !!toValue(pageId),
  })
  return {
    links: computed(() => query.data.value ?? []),
    isLoading: computed(
      () => query.status.value === 'pending' && query.data.value === undefined,
    ),
    isError: computed(() => query.status.value === 'error'),
    refetch: query.refetch,
  }
}

/**
 * Read docs currently linked to a ticket. Source for the ticket
 * detail view's "See also" panel.
 */
export function useTicketDocs(ticketId: MaybeRefOrGetter<number | null | undefined>) {
  const query = useQuery({
    key: () => pageTicketLinkKeys.forTicket(toValue(ticketId) ?? 0),
    query: () => documentationService.listDocsForTicket(toValue(ticketId)!),
    enabled: () => !!toValue(ticketId),
  })
  return {
    links: computed(() => query.data.value ?? []),
    isLoading: computed(
      () => query.status.value === 'pending' && query.data.value === undefined,
    ),
    isError: computed(() => query.status.value === 'error'),
    refetch: query.refetch,
  }
}

interface LinkPayload {
  pageId: string | number
  ticketId: number
  linkType?: 'resolves' | 'references'
}

/**
 * Mutation: create or upsert a doc<->ticket link. Invalidates both
 * sides — and the page detail's `?embed=tickets` cache — so every
 * surface refreshes off the same write.
 */
export function useLinkTicketMutation() {
  const queryCache = useQueryCache()
  return useMutation({
    mutation: ({ pageId, ticketId, linkType }: LinkPayload) =>
      documentationService.linkTicketToPage(pageId, ticketId, linkType ?? 'references'),
    onSettled: (_data, _err, vars) => {
      if (!vars) return
      queryCache.invalidateQueries({ key: pageTicketLinkKeys.forPage(vars.pageId) })
      queryCache.invalidateQueries({ key: pageTicketLinkKeys.forTicket(vars.ticketId) })
      // Page detail caches its embed under documentation-page keys
      // (see usePageDetail). Invalidate broadly so the embedded
      // linked_tickets array refreshes.
      queryCache.invalidateQueries({ key: ['documentation-page'] })
    },
  })
}

interface UnlinkPayload {
  pageId: string | number
  ticketId: number
}

export function useUnlinkTicketMutation() {
  const queryCache = useQueryCache()
  return useMutation({
    mutation: ({ pageId, ticketId }: UnlinkPayload) =>
      documentationService.unlinkTicketFromPage(pageId, ticketId),
    onSettled: (_data, _err, vars) => {
      if (!vars) return
      queryCache.invalidateQueries({ key: pageTicketLinkKeys.forPage(vars.pageId) })
      queryCache.invalidateQueries({ key: pageTicketLinkKeys.forTicket(vars.ticketId) })
      queryCache.invalidateQueries({ key: ['documentation-page'] })
    },
  })
}

/** Convenience: most-common-case shape that the UI deals with. */
export type DisplayedPageLink = PageTicketLink & { ticket_status_label: string }
export type DisplayedTicketLink = TicketDocLink
