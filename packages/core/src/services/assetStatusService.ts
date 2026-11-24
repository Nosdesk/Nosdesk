// The asset status categories and the moves the server allows between them
// (`GET /assets/statuses`). A server older than the endpoint answers 404;
// the status dialog then offers what it always did.
import apiClient from '../apiClient';
import { ASSET_STATUSES, type AssetStatus } from '../types/asset';

/** Pinia Colada cache key for the status catalogue. */
export const assetStatusKeys = {
  root: ['asset-statuses'] as const,
};

/** What a move needs beyond the matrix: the disposal record, a loan on the
 *  asset not yet returned, or no such loan. */
export type AssetStatusNeed = 'disposal' | 'active_loan' | 'no_active_loan';

export interface AssetStatusTarget {
  key: AssetStatus;
  needs: AssetStatusNeed[];
}

export interface AssetStatusCategory {
  key: AssetStatus;
  /** The status a new asset gets when the create names none. */
  default: boolean;
  /** A new asset may be created in it. */
  start: boolean;
  loanable: boolean;
  assignable: boolean;
  old_asset_target: boolean;
  terminal: boolean;
  /** The moves by hand out of it; staff only. */
  targets?: AssetStatusTarget[];
}

export interface AssetStatusCatalogue {
  categories: AssetStatusCategory[];
  /** The workspace's named statuses; none yet. */
  statuses: unknown[];
}

export const assetStatusService = {
  /** The catalogue, or null from a server that doesn't have the endpoint. */
  async list(): Promise<AssetStatusCatalogue | null> {
    try {
      const { data } = await apiClient.get<AssetStatusCatalogue>('/assets/statuses');
      return data;
    } catch (error) {
      if ((error as { response?: { status?: number } }).response?.status === 404) {
        return null;
      }
      throw error;
    }
  },
};

/** The statuses a person may move an asset in `current` to. From the
 *  server's targets when it sent them; without them (a server older than
 *  the endpoint), every other status, on loan only to restore an asset whose
 *  loan is still open. */
export function manualTargets(
  catalogue: AssetStatusCatalogue | null,
  current: string,
  hasActiveLoan: boolean,
): AssetStatus[] {
  const categories = catalogue?.categories;
  const from =
    categories?.find((c) => c.key === current) ??
    // A stored status outside the nine moves as an in-service asset does.
    categories?.find((c) => c.default);
  if (!from?.targets) {
    return ASSET_STATUSES.filter((s) => {
      if (s === current) return false;
      if (s === 'on_loan') return hasActiveLoan;
      return true;
    });
  }
  return from.targets
    .filter((target) => target.key !== current)
    .filter((target) =>
      target.needs.every((need) => {
        if (need === 'active_loan') return hasActiveLoan;
        if (need === 'no_active_loan') return !hasActiveLoan;
        return true;
      }),
    )
    .map((target) => target.key);
}
