import apiClient from './apiConfig';
import { logger } from '@/utils/logger';

// Type definitions for version history
export interface ArticleRevision {
  id: number;
  article_content_id: number;
  revision_number: number;
  contributed_by: (string | null)[];
  created_at: string;
  word_count: number | null;
}

export interface ArticleRevisionDetail extends ArticleRevision {
  yjs_document_content: string; // Base64 encoded Yjs document
}

export interface RestoreRevisionResponse {
  success: boolean;
  message: string;
}

// Version history service
export const versionHistoryService = {
  /**
   * Get all revisions for a ticket
   * @param ticketId - The ticket ID
   * @returns List of revisions ordered by revision_number desc
   */
  async getRevisions(ticketId: number): Promise<ArticleRevision[]> {
    const response = await apiClient.get(`/collaboration/tickets/${ticketId}/revisions`);
    return response.data;
  },

  /**
   * Get a specific revision with its full content
   * @param ticketId - The ticket ID
   * @param revisionNumber - The revision number
   * @returns Revision detail with base64-encoded Yjs document
   */
  async getRevision(ticketId: number, revisionNumber: number): Promise<ArticleRevisionDetail> {
    const response = await apiClient.get(`/collaboration/tickets/${ticketId}/revisions/${revisionNumber}`);
    return response.data;
  },

  /**
   * Restore a ticket to a specific revision
   * @param ticketId - The ticket ID
   * @param revisionNumber - The revision number to restore
   * @returns Success response
   */
  async restoreRevision(ticketId: number, revisionNumber: number): Promise<RestoreRevisionResponse> {
    const response = await apiClient.post(`/collaboration/tickets/${ticketId}/restore/${revisionNumber}`);
    return response.data;
  },
};

export default versionHistoryService;
