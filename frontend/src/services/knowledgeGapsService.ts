/**
 * Knowledge Gaps API client.
 *
 * Phase 2a wraps the manual-flagging entry points and the queue
 * read paths. The shape is deliberately uniform across signal
 * types so 2b/2c/2d (cluster, failed-search, stale-doc) can be
 * served by the same list/detail endpoints — only the signal
 * type and source_kind change.
 */
import apiClient from './apiConfig'
import { logger } from '@/utils/logger'

export type KnowledgeGapStatus = 'open' | 'drafting' | 'resolved' | 'dismissed'
export type KnowledgeGapSignalType =
  | 'manual_flag'
  | 'ticket_cluster'
  | 'failed_search'
  | 'stale_doc'
  | 'ai_suggested'
export type KnowledgeGapSourceKind =
  | 'ticket'
  | 'search_query'
  | 'cluster_key'
  | 'page'

export interface KnowledgeGap {
  id: number
  title: string
  description: string | null
  status: KnowledgeGapStatus
  assignee_uuid: string | null
  resolved_page_id: number | null
  evidence_count: number
  last_evidence_at: string | null
  impact_score: number
  created_at: string
  updated_at: string
  created_by: string | null
  dismissed_at: string | null
  dismissed_by: string | null
  resolved_at: string | null
  /** Only present on the detail response. */
  signals?: KnowledgeGapSignal[]
}

export interface KnowledgeGapSignalUser {
  uuid: string
  name: string
  avatar_url?: string | null
  avatar_thumb?: string | null
}

export interface KnowledgeGapSignal {
  id: number
  gap_id: number
  signal_type: KnowledgeGapSignalType
  source_kind: KnowledgeGapSourceKind
  source_ref: string
  /** Loose JSON; consumer pulls signal-type-specific fields. */
  payload: Record<string, unknown>
  confidence: number
  detected_by: string | null
  detected_at: string
  dismissed_at: string | null
  dismissed_by: string | null
  /** Hydrated by the backend for ticket-typed signals. */
  ticket_title?: string | null
  ticket_status?: string | null
  /** Hydrated detector — present for manual_flag signals
   *  (who flagged it). Null for auto-detection signals. */
  detected_by_user?: KnowledgeGapSignalUser | null
}

export interface ListGapsOptions {
  status?: KnowledgeGapStatus[]
  limit?: number
  offset?: number
}

export const flagTicketAsGap = async (
  ticketId: number,
  reason?: string,
): Promise<KnowledgeGap | null> => {
  try {
    const response = await apiClient.post(`/tickets/${ticketId}/flag-as-gap`, {
      reason: reason ?? null,
    })
    return response.data as KnowledgeGap
  } catch (error) {
    logger.error(`Error flagging ticket ${ticketId}:`, error)
    return null
  }
}

export const unflagTicketAsGap = async (ticketId: number): Promise<boolean> => {
  try {
    await apiClient.delete(`/tickets/${ticketId}/flag-as-gap`)
    return true
  } catch (error) {
    logger.error(`Error unflagging ticket ${ticketId}:`, error)
    return false
  }
}

export const listKnowledgeGaps = async (
  options: ListGapsOptions = {},
): Promise<KnowledgeGap[]> => {
  try {
    const params: Record<string, string | number> = {}
    if (options.status?.length) params.status = options.status.join(',')
    if (options.limit !== undefined) params.limit = options.limit
    if (options.offset !== undefined) params.offset = options.offset
    const response = await apiClient.get('/knowledge-gaps', { params })
    return Array.isArray(response.data) ? (response.data as KnowledgeGap[]) : []
  } catch (error) {
    logger.error('Error listing knowledge gaps:', error)
    return []
  }
}

export const getKnowledgeGap = async (gapId: number): Promise<KnowledgeGap | null> => {
  try {
    const response = await apiClient.get(`/knowledge-gaps/${gapId}`)
    return response.data as KnowledgeGap
  } catch (error) {
    logger.error(`Error loading gap ${gapId}:`, error)
    return null
  }
}

export const dismissKnowledgeGap = async (gapId: number): Promise<KnowledgeGap | null> => {
  try {
    const response = await apiClient.post(`/knowledge-gaps/${gapId}/dismiss`)
    return response.data as KnowledgeGap
  } catch (error) {
    logger.error(`Error dismissing gap ${gapId}:`, error)
    return null
  }
}

export const resolveKnowledgeGap = async (
  gapId: number,
  pageId: number,
): Promise<KnowledgeGap | null> => {
  try {
    const response = await apiClient.post(`/knowledge-gaps/${gapId}/resolve`, {
      page_id: pageId,
    })
    return response.data as KnowledgeGap
  } catch (error) {
    logger.error(`Error resolving gap ${gapId}:`, error)
    return null
  }
}

export default {
  flagTicketAsGap,
  unflagTicketAsGap,
  listKnowledgeGaps,
  getKnowledgeGap,
  dismissKnowledgeGap,
  resolveKnowledgeGap,
}
