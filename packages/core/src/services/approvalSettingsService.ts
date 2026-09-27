// How approvals behave in a workspace. Which request types need approval, and
// from whom, is set per request type.
import apiClient from '../apiClient'

/** `badge`: waiting tickets stay in the queues, marked; `held`: kept out of them. */
export type ApprovalWaitingDisplay = 'badge' | 'held'
export type ApprovalSkipBy = 'nobody' | 'admins' | 'agents'

export interface ApprovalSettings {
  waiting_display: ApprovalWaitingDisplay
  skip_by: ApprovalSkipBy
  /** Approve automatically after this many days without an answer; null = never. */
  auto_approve_days: number | null
}

export const approvalSettingsService = {
  async get(): Promise<ApprovalSettings> {
    const { data } = await apiClient.get<ApprovalSettings>('/admin/approval-settings')
    return data
  },
  async save(settings: ApprovalSettings): Promise<ApprovalSettings> {
    const { data } = await apiClient.put<ApprovalSettings>('/admin/approval-settings', settings)
    return data
  },
}
