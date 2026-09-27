// Known-issue notices: staff post one during an outage; the portal and guest
// pages show the live one.
import apiClient from '../apiClient'

export type NoticeSeverity = 'info' | 'degraded' | 'outage'

export interface Notice {
  id: number
  title: string
  body: string | null
  severity: NoticeSeverity
  starts_at: string
  ends_at: string
  incident_ticket_id: number | null
  created_by: string | null
  created_at: string
  updated_at: string
}

export interface NoticeFields {
  title: string
  body: string | null
  severity: NoticeSeverity
  starts_at: string
  ends_at: string
  incident_ticket_id: number | null
}

/** A notice as requesters see it (public settings and the portal). */
export interface PublicNotice {
  id: number
  title: string
  body: string | null
  severity: NoticeSeverity
  updated_at: string
  /** Whether signed-in requesters can follow the issue. */
  followable: boolean
}

/** Whether a notice is showing right now. */
export function isLive(n: Notice, now = Date.now()): boolean {
  return Date.parse(n.starts_at) <= now && Date.parse(n.ends_at) > now
}

export const noticeService = {
  async list(): Promise<Notice[]> {
    const { data } = await apiClient.get<{ notices: Notice[] }>('/notices')
    return data.notices
  },
  async create(fields: NoticeFields): Promise<Notice> {
    const { data } = await apiClient.post<Notice>('/notices', fields)
    return data
  },
  async update(id: number, fields: NoticeFields): Promise<Notice> {
    const { data } = await apiClient.put<Notice>(`/notices/${id}`, fields)
    return data
  },
  async end(id: number): Promise<Notice> {
    const { data } = await apiClient.post<Notice>(`/notices/${id}/end`)
    return data
  },
}
