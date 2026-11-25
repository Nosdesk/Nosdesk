import apiClient from '../apiClient';
import type { AssetDisposal, AssetLifecycleEvent } from '../types/asset';

/** Pinia Colada cache keys for an asset's lifecycle timeline. */
export const assetLifecycleKeys = {
  root: ['asset-lifecycle'] as const,
  forAsset: (assetId: number) => ['asset-lifecycle', assetId] as const,
  disposal: (assetId: number) => ['asset-disposal', assetId] as const,
  itadVendors: ['asset-itad-vendors'] as const,
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

/** The record a move into a status opens, of the type that status opens
 *  with. The work starts with the move and links its ticket. */
export interface TransitionRecordBody {
  summary?: string | null;
  vendor?: string | null;
  performed_by_uuid?: string | null;
  /** RMA, work order or invoice number. */
  reference?: string | null;
  notes?: string | null;
  /** Values for the type's own fields. */
  attributes?: Record<string, unknown>;
}

/** The completion a move out of a status writes on its open record. */
export interface CompleteRecordBody {
  /** Which open record; needed only when more than one is open. */
  record_id?: number | null;
  outcome?: string | null;
  /** Without a currency, the amount is in the workspace currency. */
  cost?: { amount: string; currency?: string | null } | null;
  /** The day the work finished; left out, the moment of the move. */
  completed_on?: string | null;
  completed_at?: string | null;
}

export interface LifecycleTransitionBody {
  /** The category; may be left out when `to_status_id` names the status. */
  to_status: string;
  /** A named status inside `to_status`; left out, the plain category. */
  to_status_id?: number | null;
  reason?: string | null;
  ticket_id?: number | null;
  metadata?: Record<string, unknown>;
  disposal?: DisposalBody;
  /** Open a record of the type `to_status` opens with; left out, none. */
  record?: TransitionRecordBody;
  /** Complete the open record of the type the current status opens with. */
  complete_record?: CompleteRecordBody;
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

  /** Distinct ITAD vendors previously entered, for the disposal-form datalist. */
  async listItadVendors(): Promise<string[]> {
    const { data } = await apiClient.get<string[]>('/assets/itad-vendors');
    return data;
  },

  /** The asset's disposal record, or null if it has none (404). */
  async getDisposal(assetId: number): Promise<AssetDisposal | null> {
    try {
      const { data } = await apiClient.get<AssetDisposal>(`/assets/${assetId}/disposal`);
      return data;
    } catch (error) {
      if ((error as { response?: { status?: number } }).response?.status === 404) {
        return null;
      }
      throw error;
    }
  },
};
