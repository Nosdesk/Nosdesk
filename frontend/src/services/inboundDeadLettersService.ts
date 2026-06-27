/**
 * Platform-operator read of the inbound dead-letter log
 * (`GET /api/admin/inbound/dead-letters`).
 *
 * Cross-tenant operator data: mail forwarded to an unknown `<token>` that
 * passed spam/virus scans but matched no active forwarding address. The
 * endpoint is platform-admin gated server-side; this service is only reachable
 * from the platform-admin-only Unrouted Inbound view.
 */
import apiClient from '@nosdesk/core/apiClient';

export interface DeadLetterRow {
  id: number;
  envelope_recipient: string;
  from_address: string | null;
  subject: string | null;
  received_at: string;
}

export interface DeadLetterListResponse {
  rows: DeadLetterRow[];
  /** Count received in the last 7 days. */
  count_7d: number;
}

export const inboundDeadLettersService = {
  async list(): Promise<DeadLetterListResponse> {
    const { data } = await apiClient.get<DeadLetterListResponse>('/admin/inbound/dead-letters');
    return data;
  },
};
