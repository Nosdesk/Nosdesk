import apiClient from '../apiClient';
import type { AxiosRequestConfig } from 'axios';
import { logger } from '../utils/logger';
import { RequestManager } from '../utils/requestManager';
import type {
  Ticket,
  RecentTicket,
  Comment,
  Attachment,
  Asset,
  Project,
  MergeResponse,
  MergeHistory,
} from '../types/ticket';
import type { UserInfo } from '../types/user';
import type { PaginatedResponse } from '../types/pagination';
import type { CommentWithAttachments } from '../types/comment';

// Request cancellation manager instance
const requestManager = new RequestManager();

// Re-export types for backwards compatibility
export type { Ticket, RecentTicket, Comment, Attachment, Asset, Project, UserInfo, CommentWithAttachments };

// Extended pagination params for tickets
export interface TicketPaginationParams {
  page: number;
  pageSize: number;
  sortField?: string;
  sortDirection?: 'asc' | 'desc';
  search?: string;
  status?: string;
  priority?: string;
  category?: string;
  assignee?: string;
  requester?: string;
  // Date filtering parameters
  createdAfter?: string;
  createdBefore?: string;
  createdOn?: string;
  modifiedAfter?: string;
  modifiedBefore?: string;
  modifiedOn?: string;
  closedAfter?: string;
  closedBefore?: string;
  closedOn?: string;
}

// API functions for tickets
export const getTickets = async (): Promise<Ticket[]> => {
  try {
    const response = await apiClient.get('/tickets');
    return response.data;
  } catch (error) {
    logger.error('Failed to fetch tickets', { error });
    throw error;
  }
};

// Get paginated tickets
/**
 * Cancellation options for {@link getPaginatedTickets}.
 *
 * Precedence: an explicit `signal` (Pinia Colada hands one to every
 * query and aborts it when the query key goes stale) means the query
 * layer owns the lifecycle, so we don't double-manage it. Otherwise a
 * caller-supplied `requestKey` routes through `requestManager` for the
 * legacy "a new call cancels the previous same-key call" behaviour
 * (ticket-list filter changes). With neither, the request fires with no
 * cross-call cancellation. There is deliberately NO shared default key:
 * one used to exist (`'paginated-tickets'`), and it made unrelated
 * concurrent consumers — the profile view's assigned + requested lists,
 * the dashboard's assigned + unassigned widgets — cancel each other.
 */
export interface PaginatedTicketsOptions {
  signal?: AbortSignal;
  requestKey?: string;
}

export const getPaginatedTickets = async (
  params: TicketPaginationParams,
  options: PaginatedTicketsOptions = {},
): Promise<PaginatedResponse<Ticket>> => {
  const usingManager = !options.signal && !!options.requestKey;
  try {
    const signal =
      options.signal ??
      (options.requestKey ? requestManager.createRequest(options.requestKey).signal : undefined);

    const response = await apiClient.get('/tickets/paginated', { params, signal });
    return response.data;
  } catch (error) {
    const errorWithName = error as { name?: string };
    if (errorWithName.name === 'AbortError' || errorWithName.name === 'CanceledError') {
      // Legacy requestManager callers expect the sentinel; Colada-signal
      // callers get the original abort error so Colada recognises its own
      // stale-query cancellation and doesn't surface it as an error.
      if (usingManager) {
        logger.debug('Request cancelled', { requestKey: options.requestKey });
        throw new Error('REQUEST_CANCELLED');
      }
      throw error;
    }
    logger.error('Failed to fetch paginated tickets', { error, params });
    throw error;
  }
};

export const getTicketById = async (id: number): Promise<Ticket> => {
  try {
    const response = await apiClient.get(`/tickets/${id}`);
    return response.data;
  } catch (error) {
    logger.error('Failed to fetch ticket', { error, ticketId: id });
    throw error;
  }
};

// Remove this function as we are using the createEmptyTicket function instead
export const createTicket = async (ticket: Omit<Ticket, 'id' | 'created' | 'modified'>): Promise<Ticket> => {
  try {
    const response = await apiClient.post(`/tickets`, ticket);
    return response.data;
  } catch (error) {
    logger.error('Failed to create ticket', { error });
    throw error;
  }
};

export const updateTicket = async (id: number, ticket: Partial<Ticket>, config?: AxiosRequestConfig): Promise<Ticket> => {
  try {
    const response = await apiClient.patch(`/tickets/${id}`, ticket, config);
    return response.data;
  } catch (error) {
    logger.error('Failed to update ticket', { error, ticketId: id });
    throw error;
  }
};

/**
 * Broadcast an in-flight value for a field to other ticket viewers
 * without writing to the database. The backend fans the value out
 * over the per-ticket SSE topic and skips both the activity log
 * and webhook fan-out. Pair with `updateTicket` on a commit
 * boundary (blur, idle timeout) to persist.
 *
 * Best-effort: a network failure here just drops one preview tick.
 * The next preview supersedes it, and the eventual commit carries
 * the final value.
 */
export const previewTicketField = async (
  id: number,
  field: 'title' | 'resolution_notes',
  value: string,
): Promise<void> => {
  await apiClient.post(`/tickets/${id}/field-preview`, { field, value });
};

export const deleteTicket = async (id: number, config?: AxiosRequestConfig): Promise<void> => {
  try {
    await apiClient.delete(`/tickets/${id}`, config);
  } catch (error) {
    logger.error('Failed to delete ticket', { error, ticketId: id });
    throw error;
  }
};

export const createEmptyTicket = async (): Promise<Ticket> => {
  try {
    logger.debug('Creating empty ticket');
    const response = await apiClient.post('/tickets/empty');
    logger.info('Empty ticket created', { ticketId: response.data.id });
    return response.data;
  } catch (error) {
    logger.error('Failed to create empty ticket', { error });
    throw error;
  }
};

// Link a ticket to another ticket
export const linkTicket = async (ticketId: number, linkedTicketId: number): Promise<void> => {
  try {
    await apiClient.post(`/tickets/${ticketId}/link/${linkedTicketId}`);
  } catch (error) {
    logger.error('Failed to link tickets', { error, ticketId, linkedTicketId });
    throw error;
  }
};

// Unlink a ticket from another ticket
export const unlinkTicket = async (ticketId: number, linkedTicketId: number): Promise<void> => {
  try {
    await apiClient.delete(`/tickets/${ticketId}/unlink/${linkedTicketId}`);
  } catch (error) {
    logger.error('Failed to unlink tickets', { error, ticketId, linkedTicketId });
    throw error;
  }
};

// Merge one or more source tickets into a destination ticket.
export interface MergeTicketsInput {
  destination_ticket_id: number
  source_ticket_ids: number[]
  reason: string | null
  notify_customer: boolean
  marker_body: string | null
  expected_state: { ticket_id: number; workflow_state_id: number }[]
}

export const mergeTickets = async (input: MergeTicketsInput): Promise<MergeResponse> => {
  try {
    const response = await apiClient.post('/tickets/merge', input);
    return response.data;
  } catch (error) {
    logger.error('Failed to merge tickets', { error, input });
    throw error;
  }
};

// Fetch the merge history for a ticket (both directions).
export const fetchMergeHistory = async (ticketId: number): Promise<MergeHistory> => {
  try {
    const response = await apiClient.get(`/tickets/${ticketId}/merge-history`);
    return response.data;
  } catch (error) {
    logger.error('Failed to fetch merge history', { error, ticketId });
    throw error;
  }
};

// Add a comment to a ticket
export const addCommentToTicket = async (
  ticketId: number,
  content: string,
  attachments: { url: string; name: string }[] = [],
  isInternal: boolean = false,
  clientId?: string
): Promise<Comment> => {
  try {
    const response = await apiClient.post(`/tickets/${ticketId}/comments`, {
      content,
      // user information is extracted from JWT token on backend for security
      attachments,
      // `is_internal = true` hides the note from requester-facing views
      // and suppresses the channel outbound relay (see backend
      // `repository::comments::get_public_comments_by_ticket_id` and
      // `services::channels::relay::decide_relay`).
      is_internal: isInternal,
      // Tells the backend the bytes in `content` are HTML (the
      // ProseMirror editor's native output) so the email outbound
      // relay can ship a multipart/alternative message instead of
      // dumping raw `<p>` tags into the plaintext body. The backend
      // defaults to `html` when the field is missing, so older
      // bundles keep working — explicit is just clearer.
      content_format: 'html',
      // Client-minted id echoed back as the sync action's correlation_id so the
      // optimistic create reconciles structurally (see sync/optimisticCreates).
      // Omitted (undefined) by callers that don't opt in; backend defaults None.
      client_id: clientId,
    });
    return response.data;
  } catch (error) {
    logger.error('Failed to add comment to ticket', { error, ticketId });
    throw error;
  }
};

// Add an attachment to a comment
export const addAttachmentToComment = async (commentId: number, url: string, name: string): Promise<Attachment> => {
  try {
    const response = await apiClient.post(`/comments/${commentId}/attachments`, {
      url,
      name,
    });
    return response.data;
  } catch (error) {
    logger.error('Failed to add attachment to comment', { error, commentId });
    throw error;
  }
};

// Delete a comment
export const deleteComment = async (commentId: number): Promise<void> => {
  try {
    await apiClient.delete(`/comments/${commentId}`);
  } catch (error) {
    logger.error('Failed to delete comment', { error, commentId });
    throw error;
  }
};

// Delete an attachment
export const deleteAttachment = async (attachmentId: number): Promise<void> => {
  try {
    await apiClient.delete(`/attachments/${attachmentId}`);
  } catch (error) {
    logger.error('Failed to delete attachment', { error, attachmentId });
    throw error;
  }
};

// Get comments for a ticket
export const getCommentsByTicketId = async (ticketId: number): Promise<CommentWithAttachments[]> => {
  try {
    const response = await apiClient.get(`/tickets/${ticketId}/comments`);
    return response.data;
  } catch (error) {
    logger.error('Failed to get comments for ticket', { error, ticketId });
    throw error;
  }
};

// Add device to ticket
export const addDeviceToTicket = async (ticketId: number, deviceId: number): Promise<void> => {
  try {
    await apiClient.post(`/tickets/${ticketId}/assets/${deviceId}`);
  } catch (error) {
    logger.error('Failed to add device to ticket', { error, ticketId, deviceId });
    throw error;
  }
};

// Remove device from ticket
export const removeDeviceFromTicket = async (ticketId: number, deviceId: number): Promise<void> => {
  try {
    await apiClient.delete(`/tickets/${ticketId}/assets/${deviceId}`);
  } catch (error) {
    logger.error('Failed to remove device from ticket', { error, ticketId, deviceId });
    throw error;
  }
};

// Cancel all active requests
export const cancelAllRequests = (): void => {
  requestManager.cancelAllRequests();
};

// Get recent tickets for the authenticated user
export const getRecentTickets = async (): Promise<RecentTicket[]> => {
  const response = await apiClient.get<RecentTicket[]>('/tickets/recent');
  return response.data;
};

// Record a ticket view
export const recordTicketView = async (ticketId: number) => {
  const response = await apiClient.post(`/tickets/${ticketId}/view`);
  return response.data;
};

// Remove a ticket from the recent views list
export const removeRecentTicket = async (ticketId: number) => {
  await apiClient.delete(`/tickets/${ticketId}/view`);
};

// Bulk operations
export interface BulkActionRequest {
  action: 'delete' | 'set-status' | 'set-priority' | 'assign';
  ids: number[];
  value?: string;
}

export const bulkAction = async (request: BulkActionRequest): Promise<{ affected: number }> => {
  const response = await apiClient.post('/tickets/bulk', request);
  return response.data;
};

// ---- Activity timeline -----------------------------------------
//
// Backed by `GET /api/tickets/:id/activity`, which scans the
// sync_actions event log filtered to the ticket's group. Cursor-
// paginated descending by sync_id; pass the previous response's
// `next_cursor` as `before` to fetch the next page.

/** One row from the ticket activity timeline. Mirrors the
 *  Rust-side `TicketActivityRow` shape — keep these in lockstep
 *  when fields are added on either side. */
export interface TicketActivityEvent {
  sync_id: number
  aggregate: string
  aggregate_id: string
  op: 'I' | 'U' | 'D' | 'A'
  event_type: string
  data: Record<string, unknown>
  actor_uuid: string | null
  actor_kind: string
  actor_ref: string | null
  occurred_at: string
}

export interface TicketActivityResponse {
  events: TicketActivityEvent[]
  next_cursor: number | null
}

export const getTicketActivity = async (
  ticketId: number,
  options: { before?: number; after?: number; limit?: number } = {},
): Promise<TicketActivityResponse> => {
  const response = await apiClient.get(`/tickets/${ticketId}/activity`, {
    params: {
      before: options.before,
      after: options.after,
      limit: options.limit,
    },
  })
  return response.data
}

// Export default object with all functions
export default {
  getTickets,
  getPaginatedTickets,
  getTicketById,
  createTicket,
  updateTicket,
  previewTicketField,
  deleteTicket,
  createEmptyTicket,
  linkTicket,
  unlinkTicket,
  addCommentToTicket,
  addAttachmentToComment,
  deleteComment,
  deleteAttachment,
  getCommentsByTicketId,
  addDeviceToTicket,
  removeDeviceFromTicket,
  getRecentTickets,
  recordTicketView,
  removeRecentTicket,
  bulkAction,
  getTicketActivity,
  cancelAllRequests
}; 