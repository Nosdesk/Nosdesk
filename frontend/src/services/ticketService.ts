import apiClient from './apiConfig';
import { logger } from '@/utils/logger';
import { RequestManager } from '@/utils/requestManager';
import type { Ticket, RecentTicket, Comment, Attachment, Device, Project } from '@/types/ticket';
import type { UserInfo } from '@/types/user';
import type { PaginatedResponse } from '@/types/pagination';
import type { CommentWithAttachments } from '@/types/comment';

// Request cancellation manager instance
const requestManager = new RequestManager();

// Re-export types for backwards compatibility
export type { Ticket, RecentTicket, Comment, Attachment, Device, Project, UserInfo, CommentWithAttachments };

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
export const getPaginatedTickets = async (params: TicketPaginationParams, requestKey: string = 'paginated-tickets'): Promise<PaginatedResponse<Ticket>> => {
  try {
    // Create cancellable request
    const controller = requestManager.createRequest(requestKey);

    const response = await apiClient.get('/tickets/paginated', {
      params,
      signal: controller.signal
    });

    return response.data;
  } catch (error) {
    // Don't throw if request was cancelled
    const errorWithName = error as { name?: string };
    if (errorWithName.name === 'AbortError' || errorWithName.name === 'CanceledError') {
      logger.debug('Request cancelled', { requestKey });
      throw new Error('REQUEST_CANCELLED');
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

export const updateTicket = async (id: number, ticket: Partial<Ticket>): Promise<Ticket> => {
  try {
    const response = await apiClient.patch(`/tickets/${id}`, ticket);
    return response.data;
  } catch (error) {
    logger.error('Failed to update ticket', { error, ticketId: id });
    throw error;
  }
};

export const deleteTicket = async (id: number): Promise<void> => {
  try {
    await apiClient.delete(`/tickets/${id}`);
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

// Add a comment to a ticket
export const addCommentToTicket = async (
  ticketId: number,
  content: string,
  attachments: { url: string; name: string }[] = [],
  isInternal: boolean = false
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
      is_internal: isInternal
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
    await apiClient.post(`/tickets/${ticketId}/devices/${deviceId}`);
  } catch (error) {
    logger.error('Failed to add device to ticket', { error, ticketId, deviceId });
    throw error;
  }
};

// Remove device from ticket
export const removeDeviceFromTicket = async (ticketId: number, deviceId: number): Promise<void> => {
  try {
    await apiClient.delete(`/tickets/${ticketId}/devices/${deviceId}`);
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

// Export default object with all functions
export default {
  getTickets,
  getPaginatedTickets,
  getTicketById,
  createTicket,
  updateTicket,
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
  cancelAllRequests
}; 