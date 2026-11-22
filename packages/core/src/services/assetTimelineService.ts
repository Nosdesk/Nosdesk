// An asset's timeline: its status changes, records, replacements, loans,
// linked tickets and markers in one list, newest first. Anyone shown assets
// reads it; records, replacements and predecessors are staff only.
import apiClient from '../apiClient';
import type { AssetTimelinePage, AssetTimelineSource } from '../types/asset';

/** Pinia Colada cache keys for asset timelines. */
export const assetTimelineKeys = {
  root: ['asset-timeline'] as const,
  forAsset: (assetId: number) => ['asset-timeline', assetId] as const,
  page: (assetId: number, filter: AssetTimelineFilter) =>
    ['asset-timeline', assetId, filter] as const,
};

/** Which entries a page lists. */
export interface AssetTimelineFilter {
  /** All sources when absent. */
  sources?: AssetTimelineSource[];
  /** Records (and replacements) of this type only, by slug or id. */
  recordType?: string | number;
  /** Entries on or after this day (YYYY-MM-DD). */
  from?: string;
  /** Entries on or before this day (YYYY-MM-DD). */
  to?: string;
  /** Also the timelines of the assets this one replaced. Staff only. */
  includePredecessors?: boolean;
  /** The `next_cursor` of the page before. */
  before?: string;
  /** 1 to 200; 100 by default. */
  limit?: number;
}

export const assetTimelineService = {
  async page(assetId: number, filter: AssetTimelineFilter = {}): Promise<AssetTimelinePage> {
    const { data } = await apiClient.get<AssetTimelinePage>(`/assets/${assetId}/timeline`, {
      params: {
        sources: filter.sources?.length ? filter.sources.join(',') : undefined,
        record_type: filter.recordType,
        from: filter.from,
        to: filter.to,
        include_predecessors: filter.includePredecessors || undefined,
        before: filter.before,
        limit: filter.limit,
      },
    });
    return data;
  },
};
