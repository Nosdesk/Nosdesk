// Admin-only client for the GET /api/admin/audit-log endpoint.
// Mirrors backend/src/handlers/audit_log.rs response shape.

import apiClient from './apiConfig';

export type AuditLogOp = 'I' | 'U' | 'D';

export interface AuditLogDiffEntry {
  field: string;
  old: unknown;
  new: unknown;
}

export interface AuditLogRow {
  id: number;
  table_name: string;
  pk_text: string;
  op: AuditLogOp;
  actor_uuid: string | null;
  correlation_id: string | null;
  occurred_at: string;
  diff: AuditLogDiffEntry[];
}

export interface AuditLogPage {
  rows: AuditLogRow[];
  next_cursor: string | null;
}

export interface AuditLogQuery {
  table_name?: string;
  pk_text?: string;
  actor_uuid?: string;
  since?: string;
  until?: string;
  limit?: number;
  cursor?: string;
}

export const auditLogService = {
  async list(query: AuditLogQuery = {}): Promise<AuditLogPage> {
    const params: Record<string, string> = {};
    if (query.table_name) params.table_name = query.table_name;
    if (query.pk_text) params.pk_text = query.pk_text;
    if (query.actor_uuid) params.actor_uuid = query.actor_uuid;
    if (query.since) params.since = query.since;
    if (query.until) params.until = query.until;
    if (query.limit !== undefined) params.limit = String(query.limit);
    if (query.cursor) params.cursor = query.cursor;

    const { data } = await apiClient.get<AuditLogPage>('/admin/audit-log', { params });
    return data;
  },
};
