import apiClient from './apiConfig';
import type { AssetLoan } from '@/types/asset';

/** Pinia Colada cache keys for an asset's loan ledger. */
export const assetLoanKeys = {
  root: ['asset-loans'] as const,
  forAsset: (assetId: number) => ['asset-loans', assetId] as const,
  forTicket: (ticketId: number) => ['ticket-loans', ticketId] as const,
};

export interface IssueLoanBody {
  borrower_user_uuid: string;
  /** ISO date (YYYY-MM-DD). Omit for an open-ended loan. */
  due_back?: string | null;
  ticket_id?: number | null;
  notes?: string | null;
}

export interface ReturnLoanBody {
  /** ISO timestamp; defaults to now on the server. */
  returned_at?: string | null;
  notes?: string | null;
}

export interface EditLoanBody {
  /** New due date. Absent leaves it unchanged. */
  due_back?: string | null;
  notes?: string | null;
}

export const assetLoanService = {
  async list(assetId: number): Promise<AssetLoan[]> {
    const { data } = await apiClient.get<AssetLoan[]>(`/assets/${assetId}/loans`);
    return data;
  },

  async listByTicket(ticketId: number): Promise<AssetLoan[]> {
    const { data } = await apiClient.get<AssetLoan[]>(`/tickets/${ticketId}/loans`);
    return data;
  },

  async issue(assetId: number, body: IssueLoanBody): Promise<AssetLoan> {
    const { data } = await apiClient.post<AssetLoan>(`/assets/${assetId}/loans`, body);
    return data;
  },

  async returnLoan(assetId: number, loanId: number, body: ReturnLoanBody): Promise<AssetLoan> {
    const { data } = await apiClient.post<AssetLoan>(
      `/assets/${assetId}/loans/${loanId}/return`,
      body,
    );
    return data;
  },

  async edit(assetId: number, loanId: number, body: EditLoanBody): Promise<AssetLoan> {
    const { data } = await apiClient.patch<AssetLoan>(`/assets/${assetId}/loans/${loanId}`, body);
    return data;
  },
};
