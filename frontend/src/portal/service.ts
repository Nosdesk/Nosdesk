import type { RequestTypeOption } from '@/components/requester/requestTypes'
// Customer-portal API calls. Thin wrappers over the portal axios client; the
// shapes mirror what the backend `/api/portal` handlers return.
import portalApi from './api'
import type { CommentContentFormat, CommentRenderKind } from '@nosdesk/core/types/comment'

export type StateCategory = 'triage' | 'backlog' | 'active' | 'in_review' | 'done' | 'cancelled'

export interface PortalState {
  name: string
  category: StateCategory
}

export interface PortalTicket {
  id: number
  uuid: string
  title: string
  priority: string
  workflow_state_id: number
  created: string
  modified: string
  closed: string | null
  state: PortalState | null
}

export interface PortalAttachment {
  id: number
  name: string
  file_size: number | null
  mime_type: string | null
}

export interface PortalComment {
  id: number
  content: string
  content_format: CommentContentFormat
  render_kind: CommentRenderKind | null
  new_content: string | null
  quoted_content: string | null
  created_at: string
  author: { name: string; avatar_url: string | null; is_staff: boolean; is_you: boolean }
  attachments: PortalAttachment[]
}

/** The requester's answer to "is it fixed?". */
export interface PortalRating {
  rating: 'good' | 'bad'
  comment: string | null
}

export interface PortalTicketDetail {
  ticket: PortalTicket
  comments: PortalComment[]
  /** The viewer's own answer, if they gave one. */
  rating: PortalRating | null
  /** Whether the viewer requested it (only they answer "is it fixed?"). */
  is_requester: boolean
}

export interface PortalMe {
  uuid: string
  name: string
  email: string | null
  effective_locale: string
}

/** Closed means resolved or cancelled; everything else is still open. */
export function isClosed(ticket: PortalTicket): boolean {
  return ticket.state?.category === 'done' || ticket.state?.category === 'cancelled'
}

/** Request a passwordless sign-in link. Always resolves (uniform response). */
export async function requestMagicLink(email: string): Promise<void> {
  await portalApi.post('/auth/magic-link', { email })
}

/** Sign in with the 6-digit code from the sign-in email. */
export async function signInWithCode(email: string, code: string): Promise<void> {
  await portalApi.post('/auth/code', { email, code })
}

export async function getMe(): Promise<PortalMe> {
  const { data } = await portalApi.get<PortalMe>('/me')
  return data
}

export async function signOut(): Promise<void> {
  await portalApi.post('/logout')
}

/** The signed-in customer's own tickets. */
export async function listMyTickets(): Promise<PortalTicket[]> {
  const { data } = await portalApi.get<PortalTicket[]>('/tickets')
  return data
}

/** One of the customer's tickets with its customer-visible thread. */
export async function getMyTicket(id: number): Promise<PortalTicketDetail> {
  const { data } = await portalApi.get<PortalTicketDetail>(`/tickets/${id}`)
  return data
}

/** Open a new ticket; the description (and any files) become the first comment. */
export async function createMyTicket(
  title: string,
  description: string,
  attachmentIds: number[] = [],
  categoryId: number | null = null,
): Promise<PortalTicket> {
  const { data } = await portalApi.post<PortalTicket>('/tickets', {
    title,
    description,
    attachment_ids: attachmentIds,
    category_id: categoryId,
  })
  return data
}

/** What a requester can pick when opening a request (empty when none). */
export async function listRequestTypes(): Promise<RequestTypeOption[]> {
  const { data } = await portalApi.get<RequestTypeOption[]>('/request-types')
  return data
}

/** Reply on one of the customer's own tickets. */
export async function replyToMyTicket(
  id: number,
  content: string,
  attachmentIds: number[] = [],
  stillNeedsHelp = false,
): Promise<void> {
  await portalApi.post(`/tickets/${id}/comments`, {
    content,
    attachment_ids: attachmentIds,
    still_needs_help: stillNeedsHelp,
  })
}

/** The requester says it's fixed: closes an open request and records it. */
export async function resolveMyTicket(id: number, comment?: string): Promise<void> {
  await portalApi.post(`/tickets/${id}/resolve`, { comment: comment ?? null })
}

/** Stage a file for the next reply or request; returns its id. */
export async function uploadFile(file: File): Promise<PortalAttachment> {
  const form = new FormData()
  form.append('file', file)
  const { data } = await portalApi.post<PortalAttachment>('/files', form, {
    headers: { 'Content-Type': 'multipart/form-data' },
  })
  return data
}

/** Download URL for a file on a public comment (portal-authenticated). */
export function attachmentUrl(ticketId: number, attachmentId: number): string {
  return `/api/portal/tickets/${ticketId}/attachments/${attachmentId}`
}
