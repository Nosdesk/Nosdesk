/**
 * Email suppression-list admin client. The list is populated
 * automatically by hard-bounce detection (J Pass 2.2b) and
 * manually managed by admins from the suppression view.
 */
import apiClient from '@nosdesk/core/apiClient'

export interface EmailSuppression {
  email: string
  reason: string
  bounce_diagnostic?: string
  bounce_count: number
  created_at: string
  last_seen_at: string
}

export interface EmailSuppressionsPage {
  rows: EmailSuppression[]
  total: number
  next_cursor: string | null
}

export interface ListQuery {
  before?: string
  limit?: number
}

export const emailSuppressionsService = {
  async list(query: ListQuery = {}): Promise<EmailSuppressionsPage> {
    const params: Record<string, string | number> = {}
    if (query.before) params.before = query.before
    if (query.limit) params.limit = query.limit
    const { data } = await apiClient.get<EmailSuppressionsPage>(
      '/admin/email-suppressions',
      { params },
    )
    return data
  },

  async add(email: string, note?: string): Promise<EmailSuppression> {
    const { data } = await apiClient.post<EmailSuppression>(
      '/admin/email-suppressions',
      { email, note },
    )
    return data
  },

  async remove(email: string): Promise<void> {
    await apiClient.delete(`/admin/email-suppressions/${encodeURIComponent(email)}`)
  },
}
