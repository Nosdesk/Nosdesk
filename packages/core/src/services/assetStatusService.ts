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

/** A workspace's own name for a status, inside one category. An asset holds
 *  it as `status_id`; a plain asset (`status_id` null) reads as its
 *  category's default named status when there is one. */
export interface AssetNamedStatus {
  id: number;
  category: AssetStatus;
  /** Stable key for imports, the API and plugins. */
  slug: string;
  label: string;
  description: string | null;
  /** `#rrggbb`, or null for the category's color. */
  color: string | null;
  /** Order within the category. */
  position: number;
  /** The category's name in this workspace. */
  is_default: boolean;
  /** Kind slugs offered the status; null for every kind. */
  applies_to_kinds: string[] | null;
  /** Archived: kept on the assets that hold it, left out of pickers. */
  archived: boolean;
}

export interface AssetStatusCatalogue {
  categories: AssetStatusCategory[];
  /** The workspace's named statuses in picker order, archived ones too. */
  statuses: AssetNamedStatus[];
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
  const known = categories?.find((c) => c.key === current);
  // A stored status outside the nine moves as an in-service asset does, and
  // in service itself is the move that puts it right.
  const from = known ?? categories?.find((c) => c.default);
  if (!from?.targets) {
    return ASSET_STATUSES.filter((s) => {
      if (s === current) return false;
      if (s === 'on_loan') return hasActiveLoan;
      return true;
    });
  }
  const targets = known ? from.targets : [{ key: from.key, needs: [] }, ...from.targets];
  return targets
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
