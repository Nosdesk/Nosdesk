// A ticket's approval, as staff see it in the app.
import apiClient from '../apiClient'

export type ApprovalState = 'pending' | 'approved' | 'declined' | 'skipped'

export interface ApproverStatus {
  uuid: string
  name: string
  decision: 'approved' | 'declined' | 'skipped' | null
  comment: string | null
  decided_at: string | null
}

export interface ApprovalSummary {
  ticket_id: number
  title: string
  request_type: string | null
  requester_name: string | null
  description: string | null
  created_at: string
  approval_state: ApprovalState | null
  approvers: ApproverStatus[]
}

export interface TicketApproval {
  approval: ApprovalSummary
  /** The request type needs approval but nobody could be found to give it. */
  no_approver: boolean
  /** The viewer is a waiting approver. */
  can_decide: boolean
  /** The viewer may skip it under the workspace's setting. */
  can_skip: boolean
}

export const approvalService = {
  async get(ticketId: number): Promise<TicketApproval> {
    const { data } = await apiClient.get<TicketApproval>(`/tickets/${ticketId}/approval`)
    return data
  },
  async decide(ticketId: number, approve: boolean, comment?: string): Promise<void> {
    await apiClient.post(`/tickets/${ticketId}/approval/decide`, {
      decision: approve ? 'approve' : 'decline',
      comment: comment || null,
    })
  },
  async skip(ticketId: number, comment?: string): Promise<void> {
    await apiClient.post(`/tickets/${ticketId}/approval/skip`, { comment: comment || null })
  },
}
