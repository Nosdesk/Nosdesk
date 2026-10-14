import { computed, type MaybeRefOrGetter, toValue } from 'vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import { assetLoanKeys, assetLoanService } from '@nosdesk/core/services/assetLoanService';
import { assetLifecycleKeys } from '@nosdesk/core/services/assetLifecycleService';
import type { AssetLoan } from '@nosdesk/core/types/asset';

/**
 * An asset's loan ledger: the open loan and the returned ones. Every caller
 * shares one cached query, so the asset page's header, Loans card and
 * status form read the same rows from one request.
 */
export function useAssetLoans(assetId: MaybeRefOrGetter<number>) {
  const queryCache = useQueryCache();
  const query = useQuery({
    key: () => assetLoanKeys.forAsset(toValue(assetId)),
    query: () => assetLoanService.list(toValue(assetId)),
  });
  const loans = computed<AssetLoan[]>(() =>
    Array.isArray(query.data.value) ? query.data.value : [],
  );
  const activeLoan = computed<AssetLoan | null>(
    () => loans.value.find((l) => !l.returned_at) ?? null,
  );
  const pastLoans = computed<AssetLoan[]>(() => loans.value.filter((l) => l.returned_at));
  const isFirstLoad = computed(
    () => query.status.value === 'pending' && query.data.value === undefined,
  );

  /** Refetch the ledger and the status timeline: issuing or returning a loan
   *  changes the asset's status and writes a lifecycle event. */
  function invalidate() {
    const id = toValue(assetId);
    return Promise.all([
      queryCache.invalidateQueries({ key: assetLoanKeys.forAsset(id) }),
      queryCache.invalidateQueries({ key: assetLifecycleKeys.forAsset(id) }),
    ]);
  }

  return { query, loans, activeLoan, pastLoans, isFirstLoad, invalidate };
}
