// Customer-portal API calls. Thin wrappers over the portal axios client; the
// shapes mirror what the backend `/api/portal` handlers return.
import portalApi from './api'

export interface PortalTicket {
  id: number
  uuid: string
  title: string
  priority: string
  workflow_state_id: number
  created: string
  modified: string
  closed: string | null
}

export interface PortalComment {
  id: number
  content: string
  user_uuid: string
  created_at: string
}

export interface PortalTicketDetail {
  ticket: PortalTicket
  comments: PortalComment[]
}

/** Request a passwordless sign-in link. Always resolves (uniform response). */
export async function requestMagicLink(email: string): Promise<void> {
  await portalApi.post('/auth/magic-link', { email })
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

/** Open a new ticket; the description becomes the first comment. */
export async function createMyTicket(title: string, description: string): Promise<PortalTicket> {
  const { data } = await portalApi.post<PortalTicket>('/tickets', { title, description })
  return data
}

/** Reply on one of the customer's own tickets. */
export async function replyToMyTicket(id: number, content: string): Promise<PortalComment> {
  const { data } = await portalApi.post<PortalComment>(`/tickets/${id}/comments`, { content })
  return data
}
