/**
 * The queries behind an asset's Timeline card: the timeline a page at a
 * time, the open records pinned above it, what the records add up to, and
 * the workspace's record types. All cache-first and workspace-scoped; sync
 * actions on this asset's records, status changes, loans, ticket links and
 * files refetch them.
 */
import { computed, toValue, type MaybeRefOrGetter } from 'vue'
import {
  assetTimelineKeys,
  assetTimelineService,
  type AssetTimelineFilter,
} from '@nosdesk/core/services/assetTimelineService'
import { assetRecordKeys, assetRecordService } from '@nosdesk/core/services/assetRecordService'
import type {
  AssetRecord,
  AssetRecordSummary,
  AssetRecordType,
  AssetTimelineEntry,
  AssetTimelinePage,
} from '@nosdesk/core/types/asset'
import {
  useWorkspaceInfiniteQuery,
  useWorkspaceQuery,
  useWorkspaceQueryCache,
} from '@/queries/workspaceQuery'
import { useSyncActions } from '@/composables/useSyncActions'

/** A function that refetches everything the Timeline card shows for an
 *  asset. Call during setup. */
export function useAssetTimelineInvalidate() {
  const cache = useWorkspaceQueryCache()
  return (assetId: number) =>
    Promise.all([
      cache.invalidateQueries({ key: [...assetTimelineKeys.forAsset(assetId)] }),
      cache.invalidateQueries({ key: [...assetRecordKeys.forAsset(assetId)] }),
    ])
}

/** The asset's timeline under `filter`, newest first, a page at a time.
 *  `before` in the filter is ignored: the query pages itself. */
export function useAssetTimelinePages(
  assetId: MaybeRefOrGetter<number>,
  filter: MaybeRefOrGetter<AssetTimelineFilter>,
) {
  const query = useWorkspaceInfiniteQuery<AssetTimelinePage, Error, string | null>({
    key: () => [...assetTimelineKeys.page(toValue(assetId), toValue(filter))],
    initialPageParam: null,
    query: ({ pageParam }) =>
      assetTimelineService.page(toValue(assetId), { ...toValue(filter), before: pageParam ?? undefined }),
    getNextPageParam: (last) => last.next_cursor,
  })
  const pages = computed(() => query.data.value?.pages ?? [])
  const entries = computed<AssetTimelineEntry[]>(() => pages.value.flatMap((p) => p.entries))
  const hasMore = computed(() => query.hasNextPage.value)
  const loaded = computed(() => query.data.value !== undefined)
  const loadingMore = computed(() => query.asyncStatus.value === 'loading' && loaded.value)
  /** Load the next page, after any fetch already under way. */
  async function loadMore() {
    if (query.asyncStatus.value === 'loading') await query.refresh()
    if (!hasMore.value) return
    await query.loadNextPage()
  }
  return { query, entries, hasMore, loaded, loadingMore, loadMore }
}

/** One source of the timeline, up to `limit` entries: the tickets linked to
 *  the asset, or the assets it replaced. */
export function useAssetTimelineSource(
  assetId: MaybeRefOrGetter<number>,
  source: 'ticket' | 'replacement',
  limit: number,
  enabled: MaybeRefOrGetter<boolean> = true,
) {
  const filter = (): AssetTimelineFilter => ({ sources: [source], limit })
  const query = useWorkspaceQuery({
    key: () => [...assetTimelineKeys.page(toValue(assetId), filter())],
    query: () => assetTimelineService.page(toValue(assetId), filter()),
    enabled: () => toValue(enabled),
  })
  return computed<AssetTimelineEntry[]>(() => query.data.value?.entries ?? [])
}

/** The asset's open records, newest first. Staff only. */
export function useAssetOpenRecords(
  assetId: MaybeRefOrGetter<number>,
  enabled: MaybeRefOrGetter<boolean>,
) {
  const query = useWorkspaceQuery({
    key: () => [...assetRecordKeys.forAsset(toValue(assetId)), 'open'],
    query: () => assetRecordService.list(toValue(assetId), { open: true }),
    enabled: () => toValue(enabled),
  })
  return computed<AssetRecord[]>(() => query.data.value?.records ?? [])
}

/** What the asset's records add up to, of one type when `recordType` is
 *  set. Staff only. */
export function useAssetRecordSummary(
  assetId: MaybeRefOrGetter<number>,
  recordType: MaybeRefOrGetter<string>,
  enabled: MaybeRefOrGetter<boolean>,
) {
  const query = useWorkspaceQuery({
    key: () => [...assetRecordKeys.summary(toValue(assetId), toValue(recordType))],
    query: () => assetRecordService.summary(toValue(assetId), toValue(recordType) || undefined),
    enabled: () => toValue(enabled),
  })
  return computed<AssetRecordSummary | null>(() => query.data.value ?? null)
}

/** The workspace's record types, archived ones too (old records keep
 *  theirs), with the currency the record form's cost offers. Shares its
 *  cache entry with the status change dialog. */
export function useAssetRecordTypes(enabled: MaybeRefOrGetter<boolean>) {
  const query = useWorkspaceQuery({
    key: [...assetRecordKeys.types, 'with-archived'],
    query: () => assetRecordService.types(true),
    enabled: () => toValue(enabled),
  })
  const types = computed<AssetRecordType[]>(() => query.data.value?.types ?? [])
  const currency = computed(() => query.data.value?.currency ?? null)
  const suggestedCurrency = computed(() => query.data.value?.suggested_currency ?? null)
  /** The workspace timezone, which the server reads a record's day in. */
  const timezone = computed(() => query.data.value?.timezone ?? null)
  return { query, types, currency, suggestedCurrency, timezone }
}

/** Refetch the asset's timeline when a record, status change, loan, ticket
 *  link or file of it changes, coalesced over 250 ms. So does a change on
 *  one of `related` (predecessors in view), or a record that names this
 *  asset as its replacement. */
export function useAssetTimelineSync(
  assetId: MaybeRefOrGetter<number>,
  related: MaybeRefOrGetter<number[]> = [],
) {
  const invalidate = useAssetTimelineInvalidate()
  useSyncActions(
    (actions) => {
      const id = toValue(assetId)
      const others = toValue(related)
      const touches = actions.some((a) => {
        const data = a.data as { asset_id?: number; replacement_asset_id?: number | null } | null
        if (!data) return false
        return (
          data.asset_id === id ||
          data.replacement_asset_id === id ||
          (data.asset_id !== undefined && others.includes(data.asset_id))
        )
      })
      if (touches) void invalidate(id)
    },
    {
      aggregates: ['asset_record', 'asset_lifecycle_event', 'asset_loan', 'ticket_asset', 'asset_media'],
      debounceMs: 250,
    },
  )
  return () => invalidate(toValue(assetId))
}
