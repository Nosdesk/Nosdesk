import apiClient from '../apiClient';
import type { AssetLifecycleEvent } from '../types/asset';

/** Pinia Colada cache keys for an asset's lifecycle timeline. */
export const assetLifecycleKeys = {
  root: ['asset-lifecycle'] as const,
  forAsset: (assetId: number) => ['asset-lifecycle', assetId] as const,
};

/** Disposal detail sent with a transition to `disposed`. Captured atomically
 *  with the transition for the compliance / export trail. */
export interface DisposalBody {
  /** NIST SP 800-88 category: clear | purge | destroy | none. */
  sanitization_method: string;
  data_bearing: boolean;
  itad_vendor?: string | null;
  notes?: string | null;
}

export interface LifecycleTransitionBody {
  to_status: string;
  reason?: string | null;
  ticket_id?: number | null;
  metadata?: Record<string, unknown>;
  disposal?: DisposalBody;
}

export const assetLifecycleService = {
  async list(assetId: number): Promise<AssetLifecycleEvent[]> {
    const { data } = await apiClient.get<AssetLifecycleEvent[]>(`/assets/${assetId}/lifecycle`);
    return data;
  },

  async transition(assetId: number, body: LifecycleTransitionBody): Promise<AssetLifecycleEvent> {
    const { data } = await apiClient.post<AssetLifecycleEvent>(
      `/assets/${assetId}/lifecycle`,
      body,
    );
    return data;
  },
};
