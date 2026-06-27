// Admin / audit-reviewer client for the unified audit feed.
// Mirrors backend/src/handlers/audit.rs + repository/audit.rs.

import apiClient from '../apiClient';

export type AuditSource = 'tier1' | 'tier2' | 'tier3';

export interface AuditDiffEntry {
  field: string;
  old: unknown;
  new: unknown;
}

export interface AuditTargetRef {
  kind: string;
  id: string;
}

export interface AuditEntry {
  /** Stable composite key "{tier}:{row_id}". */
  id: string;
  source: AuditSource;
  occurred_at: string;
  actor_kind: string;
  actor_uuid: string | null;
  /** Server-resolved display name for a user actor; null for
   *  system/anonymous/token actors or when the user can't be resolved. */
  actor_name: string | null;
  event_type: string;
  target: AuditTargetRef | null;
  payload: unknown | null;
  diff: AuditDiffEntry[];
  correlation_id: string | null;
  source_ip: string | null;
  severity: string;
  event_uuid: string | null;
}

export interface AuditPage {
  entries: AuditEntry[];
  next_cursor: string | null;
}

export interface AuditQuery {
  actor_uuid?: string;
  since?: string;
  until?: string;
  /** Event-type prefix, e.g. "auth." */
  event_prefix?: string;
  /** 1 (app), 2 (auth), 3 (record changes). */
  tier?: number;
  severity?: string;
  limit?: number;
  cursor?: string;
}

function toParams(query: AuditQuery): Record<string, string> {
  const params: Record<string, string> = {};
  if (query.actor_uuid) params.actor_uuid = query.actor_uuid;
  if (query.since) params.since = query.since;
  if (query.until) params.until = query.until;
  if (query.event_prefix) params.event_prefix = query.event_prefix;
  if (query.tier !== undefined) params.tier = String(query.tier);
  if (query.severity) params.severity = query.severity;
  if (query.limit !== undefined) params.limit = String(query.limit);
  if (query.cursor) params.cursor = query.cursor;
  return params;
}

export const auditService = {
  async list(query: AuditQuery = {}): Promise<AuditPage> {
    const { data } = await apiClient.get<AuditPage>('/admin/audit', {
      params: toParams(query),
    });
    return data;
  },

  /** Fetch the full filtered set (server caps at 5000) for download.
   * Hitting this endpoint emits a `data.audit.exported` audit event. */
  async export(query: AuditQuery = {}): Promise<unknown> {
    const { data } = await apiClient.get('/admin/audit/export', {
      params: toParams(query),
    });
    return data;
  },
};
