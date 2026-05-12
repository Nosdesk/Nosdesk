// Admin-only client for the outbound email queue (Item J Pass 1).
// Mirrors the response shapes in backend/src/handlers/email_queue.rs.

import apiClient from './apiConfig';

export type OutboundEmailStatus =
  | 'pending'
  | 'sending'
  | 'sent'
  | 'failed'
  | 'dead'
  | 'suppressed';

export interface OutboundEmailRow {
  id: number;
  channel_id: number;
  ticket_id: number | null;
  comment_id: number | null;
  recipient: string;
  subject: string;
  status: OutboundEmailStatus;
  attempts: number;
  last_smtp_code: number | null;
  last_error: string | null;
  next_attempt_at: string;
  created_at: string;
  sent_at: string | null;
  failed_at: string | null;
  /** J Pass 2.2 bounce-linkage fields. Set when an inbound DSN
   *  matched this row's deterministic Message-ID; drives the
   *  "Bounced" badge and the expanded-row diagnostic detail.
   *  Independent from `status` (most bounced rows sit in `sent`
   *  because SMTP relay succeeded; the remote MTA rejected later
   *  via DSN). */
  bounced_at?: string;
  bounce_recipient?: string;
  bounce_diagnostic?: string;
}

export interface OutboundEmailPage {
  rows: OutboundEmailRow[];
  next_cursor: string | null;
}

export interface OutboundEmailQuery {
  /** Comma-separated, e.g. "pending,failed,dead" */
  status?: string;
  ticket_id?: number;
  recipient_domain?: string;
  since?: string;
  until?: string;
  limit?: number;
  cursor?: string;
}

export interface OutboundEmailStats {
  by_status: Array<{ status: OutboundEmailStatus | string; count: number }>;
  pending_total: number;
  oldest_pending_age_seconds: number | null;
}

export const emailQueueService = {
  async list(query: OutboundEmailQuery = {}): Promise<OutboundEmailPage> {
    const params: Record<string, string> = {};
    if (query.status) params.status = query.status;
    if (query.ticket_id !== undefined) params.ticket_id = String(query.ticket_id);
    if (query.recipient_domain) params.recipient_domain = query.recipient_domain;
    if (query.since) params.since = query.since;
    if (query.until) params.until = query.until;
    if (query.limit !== undefined) params.limit = String(query.limit);
    if (query.cursor) params.cursor = query.cursor;
    const { data } = await apiClient.get<OutboundEmailPage>('/admin/email-queue', { params });
    return data;
  },

  async stats(): Promise<OutboundEmailStats> {
    const { data } = await apiClient.get<OutboundEmailStats>('/admin/email-queue/stats');
    return data;
  },

  async retryNow(id: number): Promise<void> {
    await apiClient.post(`/admin/email-queue/${id}/retry`);
  },

  async cancel(id: number): Promise<void> {
    await apiClient.post(`/admin/email-queue/${id}/cancel`);
  },
};
