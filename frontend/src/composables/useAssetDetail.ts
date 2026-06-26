/**
 * Read + write data layer for a single asset's detail record.
 *
 * The Pinia Colada query cache is the SINGLE source of truth: reads come from
 * `asset`, and `patchAsset` applies an optimistic patch, persists it, rolls
 * back on error, and reconciles via invalidation. This replaces the old
 * hand-rolled `device` ref + `attributeDraft` / `editValues` drafts + manual
 * `fetchDeviceData` refetch in `AssetView`, which could silently lose an edit:
 * a debounced PUT could be clobbered by a concurrent refetch (the draft was
 * reset to the server row before the save fired, so `attributesDirty` went
 * false and the save no-oped), and nothing surfaced the failure.
 *
 * With one source of truth there is no draft to clobber, mutations fire on
 * commit (no debounce window to lose), and rollback + invalidation are the
 * library's job, not ad-hoc imperative code.
 */
import { computed, type Ref } from 'vue';
import { useQuery, useMutation, useQueryCache } from '@pinia/colada';
import { getAssetById, updateAsset } from '@/services/assetService';
import { assetDetailKey } from '@nosdesk/core/queries/assets';
import type { Asset } from '@nosdesk/core/types/asset';

interface PatchContext {
  previous?: Asset;
}

export function useAssetDetail(id: Ref<number>) {
  const queryCache = useQueryCache();
  const keyOf = () => assetDetailKey(id.value);

  const query = useQuery({
    key: () => assetDetailKey(id.value),
    query: () => getAssetById(id.value),
  });

  const asset = computed<Asset | null>(() => query.data.value ?? null);
  // Skeleton only on the genuine cold start, never on background refresh.
  const isFirstLoad = computed(
    () => query.status.value === 'pending' && query.data.value === undefined,
  );

  const patch = useMutation<Asset, Partial<Asset>, Error, PatchContext>({
    mutation: (changes) => updateAsset(id.value, changes),
    // Optimistically reflect the change so the UI updates on commit, keeping a
    // snapshot to roll back to if the write is rejected.
    onMutate: (changes) => {
      const key = keyOf();
      const previous = queryCache.getQueryData<Asset>(key);
      if (previous) queryCache.setQueryData(key, { ...previous, ...changes });
      return { previous };
    },
    onError: (_err, _changes, ctx) => {
      // Pinia Colada types `ctx` as the union of possible contexts; onMutate
      // above always returns PatchContext, so the cast is safe.
      const c = ctx as PatchContext | undefined;
      if (c?.previous) queryCache.setQueryData(keyOf(), c.previous);
    },
    // Reconcile with the authoritative row on success, then revalidate so a
    // concurrent SSE-driven invalidation lands on a consistent cache.
    onSettled: (updated) => {
      if (updated) queryCache.setQueryData(keyOf(), updated);
      void queryCache.invalidateQueries({ key: keyOf() });
    },
  });

  return {
    asset,
    isFirstLoad,
    error: query.error,
    refetch: query.refetch,
    /** Invalidate the cached record (used by the SSE sync-action listener). */
    invalidate: () => queryCache.invalidateQueries({ key: keyOf() }),
    /** Replace the cached record with an authoritative one returned by a
     *  sibling endpoint (model stamp/clear, unmanage) that already persisted. */
    setAsset: (next: Asset) => queryCache.setQueryData(keyOf(), next),
    /** Optimistic, awaitable partial update. Throws on rejection (after rollback). */
    patchAsset: patch.mutateAsync,
    isSaving: patch.isLoading,
    saveError: patch.error,
  };
}
