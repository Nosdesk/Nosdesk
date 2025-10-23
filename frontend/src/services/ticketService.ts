import apiClient from './apiConfig';
import type { TicketStatus, TicketPriority } from '@/constants/ticketOptions';

// Request cancellation manager
class RequestManager {
  private activeRequests = new Map<string, AbortController>();

  createRequest(key: string): AbortController {
    // Cancel any existing request with the same key
    this.cancelRequest(key);
    
    // Create new abort controller
    const controller = new AbortController();
    this.activeRequests.set(key, controller);
    
    return controller;
  }

  cancelRequest(key: string): void {
    const controller = this.activeRequests.get(key);
    if (controller) {
      controller.abort();
      this.activeRequests.delete(key);
    }
  }

  cancelAllRequests(): void {
    this.activeRequests.forEach(controller => controller.abort());
    this.activeRequests.clear();
  }
}

const requestManager = new RequestManager();

// Define interfaces for our data models
export interface Device {
  id: number;
  name: string;
  hostname: string;
  serial_number: string;
  model: string;
  warranty_status: string;
  ticket_id?: number | null;
}

export interface Comment {
  id: number;
  content: string;
  user_uuid: string;
  created_at: string;
  ticket_id: number;
  attachments?: Attachment[];
  user?: UserInfo;
}

export interface Attachment {
  id: number;
  url: string;
  name: string;
  comment_id: number;
}

export interface Project {
  id: number;
  name: string;
  description?: string | null;
  status: 'active' | 'completed' | 'archived';
  created_at: string;
  updated_at: string;
  ticket_count?: number;
}

export interface Ticket {
  id: number;
  title: string;
  status: TicketStatus;
  priority: TicketPriority;
  created: string;
  modified: string;
  assignee: string;
  requester: string;
  requester_user?: UserInfo | null;  // Complete requester data
  assignee_user?: UserInfo | null;   // Complete assignee data
  closed_at?: string;
  devices?: Device[];
  comments?: Comment[];
  article_content?: string;
  linkedTickets?: number[];
  linked_tickets?: number[];
  projects?: Project[];
}

// Pagination interface
export interface PaginationParams {
  page: number;
  pageSize: number;
  sortField?: string;
  sortDirection?: 'asc' | 'desc';
  search?: string;
  status?: string;
  priority?: string;
  assignee?: string;
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

// Paginated response interface
export interface PaginatedResponse<T> {
  data: T[];
  total: number;
  page: number;
  pageSize: number;
  totalPages: number;
}

// User info interface for comments
export interface UserInfo {
  uuid: string;
  name: string;
  avatar_thumb?: string | null; // Avatar thumbnail for comment users
}

// Add CommentWithAttachments interface
export interface CommentWithAttachments {
  comment: Comment;
  attachments: Attachment[];
  user?: UserInfo;
}

// API functions for tickets
export const getTickets = async (): Promise<Ticket[]> => {
  try {
    const response = await apiClient.get('/tickets');
    return response.data;
  } catch (error) {
    console.error('Error fetching tickets:', error);
    throw error;
  }
};

// Get paginated tickets
export const getPaginatedTickets = async (params: PaginationParams, requestKey: string = 'paginated-tickets'): Promise<PaginatedResponse<Ticket>> => {
  try {
    // Create cancellable request
    const controller = requestManager.createRequest(requestKey);
    
    const response = await apiClient.get('/tickets/paginated', { 
      params,
      signal: controller.signal 
    });
    
    // Remove from active requests on success
    requestManager.cancelRequest(requestKey);
    
    return response.data;
  } catch (error: any) {
    // Don't throw if request was cancelled
    if (error.name === 'AbortError' || error.name === 'CanceledError') {
      console.log('Request cancelled:', requestKey);
      throw new Error('REQUEST_CANCELLED');
    }
    console.error('Error fetching paginated tickets:', error);
    throw error;
  }
};

export const getTicketById = async (id: number): Promise<Ticket> => {
  try {
    const response = await apiClient.get(`/tickets/${id}`);
    return response.data;
  } catch (error) {
    console.error(`Error fetching ticket ${id}:`, error);
    throw error;
  }
};

// Remove this function as we are using the createEmptyTicket function instead
export const createTicket = async (ticket: Omit<Ticket, 'id' | 'created' | 'modified'>): Promise<Ticket> => {
  try {
    const response = await apiClient.post(`/tickets`, ticket);
    return response.data;
  } catch (error) {
    console.error('Error creating ticket:', error);
    throw error;
  }
};

export const updateTicket = async (id: number, ticket: Partial<Ticket>): Promise<Ticket> => {
  try {
    const response = await apiClient.patch(`/tickets/${id}`, ticket);
    return response.data;
  } catch (error) {
    console.error(`Error updating ticket ${id}:`, error);
    throw error;
  }
};

export const deleteTicket = async (id: number): Promise<void> => {
  try {
    await apiClient.delete(`/tickets/${id}`);
  } catch (error) {
    console.error(`Error deleting ticket ${id}:`, error);
    throw error;
  }
};

export const createEmptyTicket = async (): Promise<Ticket> => {
  console.log('createEmptyTicket called');
  try {
    console.log('Sending POST request to create empty ticket');
    const response = await apiClient.post('/tickets/empty');
    console.log('Empty ticket created successfully:', response.data);
    return response.data;
  } catch (error) {
    console.error('Error creating empty ticket:', error);
    throw error;
  }
};

// Link a ticket to another ticket
export const linkTicket = async (ticketId: number, linkedTicketId: number): Promise<void> => {
  try {
    await apiClient.post(`/tickets/${ticketId}/link/${linkedTicketId}`);
  } catch (error) {
    console.error(`Error linking ticket ${ticketId} to ${linkedTicketId}:`, error);
    throw error;
  }
};

// Unlink a ticket from another ticket
export const unlinkTicket = async (ticketId: number, linkedTicketId: number): Promise<void> => {
  try {
    await apiClient.delete(`/tickets/${ticketId}/unlink/${linkedTicketId}`);
  } catch (error) {
    console.error(`Error unlinking ticket ${ticketId} from ${linkedTicketId}:`, error);
    throw error;
  }
};

// Add a comment to a ticket
export const addCommentToTicket = async (
  ticketId: number, 
  content: string, 
  attachments: { url: string; name: string }[] = []
): Promise<Comment> => {
  try {
    const response = await apiClient.post(`/tickets/${ticketId}/comments`, {
      content,
      // user information is extracted from JWT token on backend for security
      attachments
    });
    return response.data;
  } catch (error) {
    console.error(`Error adding comment to ticket ${ticketId}:`, error);
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
    console.error(`Error adding attachment to comment ${commentId}:`, error);
    throw error;
  }
};

// Delete a comment
export const deleteComment = async (commentId: number): Promise<void> => {
  try {
    await apiClient.delete(`/comments/${commentId}`);
  } catch (error) {
    console.error(`Error deleting comment ${commentId}:`, error);
    throw error;
  }
};

// Delete an attachment
export const deleteAttachment = async (attachmentId: number): Promise<void> => {
  try {
    await apiClient.delete(`/attachments/${attachmentId}`);
  } catch (error) {
    console.error(`Error deleting attachment ${attachmentId}:`, error);
    throw error;
  }
};

// Get comments for a ticket
export const getCommentsByTicketId = async (ticketId: number): Promise<CommentWithAttachments[]> => {
  try {
    const response = await apiClient.get(`/tickets/${ticketId}/comments`);
    return response.data;
  } catch (error) {
    console.error(`Error getting comments for ticket ${ticketId}:`, error);
    throw error;
  }
};

// Add device to ticket
export const addDeviceToTicket = async (ticketId: number, deviceId: number): Promise<void> => {
  try {
    await apiClient.post(`/tickets/${ticketId}/devices/${deviceId}`);
  } catch (error) {
    console.error(`Error adding device ${deviceId} to ticket ${ticketId}:`, error);
    throw error;
  }
};

// Remove device from ticket
export const removeDeviceFromTicket = async (ticketId: number, deviceId: number): Promise<void> => {
  try {
    await apiClient.delete(`/tickets/${ticketId}/devices/${deviceId}`);
  } catch (error) {
    console.error(`Error removing device ${deviceId} from ticket ${ticketId}:`, error);
    throw error;
  }
};

// Cancel all active requests
export const cancelAllRequests = (): void => {
  requestManager.cancelAllRequests();
};

// Get recent tickets for the authenticated user
export const getRecentTickets = async () => {
  const response = await apiClient.get('/tickets/recent');
  return response.data;
};

// Record a ticket view
export const recordTicketView = async (ticketId: number) => {
  const response = await apiClient.post(`/tickets/${ticketId}/view`);
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
  cancelAllRequests
}; 