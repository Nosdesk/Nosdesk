// Platform-admin client for the bug-report ("Report a problem") log.
// Mirrors backend/src/handlers/bug_reports.rs (GET /admin/bug-reports).

import apiClient from '../apiClient';

export interface BugReportBreadcrumb {
  /** "route" or "api". */
  category: string;
  ts: number;
  summary: string;
}

export interface BugReportRow {
  id: number;
  workspace_id: number;
  user_uuid: string | null;
  session_id: string;
  description: string;
  url: string;
  breadcrumbs: BugReportBreadcrumb[];
  build_sha: string;
  user_agent: string | null;
  viewport: { w: number; h: number } | null;
  occurred_at: string;
  received_at: string;
}

export interface BugReportsQuery {
  limit?: number;
  offset?: number;
}

export const bugReportsService = {
  /** Newest-first bug reports across every workspace. Operator-only on the
   *  backend (`require_platform_admin`); the caller should gate the UI too. */
  async list(query: BugReportsQuery = {}): Promise<BugReportRow[]> {
    const params: Record<string, string> = {};
    if (query.limit != null) params.limit = String(query.limit);
    if (query.offset != null) params.offset = String(query.offset);
    const { data } = await apiClient.get<BugReportRow[]>('/admin/bug-reports', { params });
    return data;
  },
};
