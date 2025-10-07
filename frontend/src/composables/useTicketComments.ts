import { type Ref } from 'vue';
import ticketService from '@/services/ticketService';
import apiClient from '@/services/apiConfig';

/**
 * Composable for managing ticket comments
 */
export function useTicketComments(ticket: Ref<any>, refreshTicket: () => Promise<void>) {

  // Add comment
  async function addComment(data: { content: string; user_uuid: string; files: File[] }): Promise<void> {
    if (!ticket.value) return;

    // Validate input
    if (!data.content.trim() && (!data.files || data.files.length === 0)) {
      return;
    }

    // Set placeholder content if only files
    if (!data.content.trim() && data.files?.length > 0) {
      data.content = 'Attachment added';
    }

    try {
      // Upload files if any
      let attachments: { id: number; url: string; name: string }[] = [];
      if (data.files?.length > 0) {
        const formData = new FormData();
        data.files.forEach((file) => {
          formData.append('files', file, file.name);
        });

        const response = await apiClient.post('/upload', formData, {
          headers: { 'Content-Type': 'multipart/form-data' },
        });

        attachments = response.data.map((file: any) => ({
          id: file.id,
          url: file.url,
          name: file.name,
        }));
      }

      // Create comment
      const newComment = await ticketService.addCommentToTicket(
        ticket.value.id,
        data.content,
        attachments,
      );

      // Optimistically add to local state - use unshift to preserve array reference
      if (ticket.value.commentsAndAttachments) {
        const optimisticComment = {
          id: newComment.id,
          content: newComment.content,
          user_uuid: newComment.user_uuid,
          createdAt: newComment.created_at,
          created_at: newComment.created_at,
          ticket_id: newComment.ticket_id,
          attachments: newComment.attachments || [],
          user: newComment.user,
        };

        ticket.value.commentsAndAttachments.unshift(optimisticComment);
      }
    } catch (error) {
      console.error('Error adding comment:', error);
      await refreshTicket();
    }
  }

  // Delete attachment
  async function deleteAttachment(data: { commentId: number; attachmentIndex: number }): Promise<void> {
    if (!ticket.value) return;

    try {
      const comment = ticket.value.commentsAndAttachments?.find((c: any) => c.id === data.commentId);
      if (!comment?.attachments?.[data.attachmentIndex]) {
        return;
      }

      const attachment = comment.attachments[data.attachmentIndex];
      if (!attachment.id) {
        return;
      }

      // Optimistic update - use splice to preserve array reference
      if (ticket.value.commentsAndAttachments) {
        const commentIndex = ticket.value.commentsAndAttachments.findIndex(
          (c: any) => c.id === data.commentId,
        );

        if (commentIndex !== -1) {
          // Directly mutate the attachments array to preserve references
          comment.attachments.splice(data.attachmentIndex, 1);
        }
      }

      // Delete from backend
      await ticketService.deleteAttachment(attachment.id);

      // Delete comment if it was the last attachment and has no content
      if (comment.attachments.length === 1 && (!comment.content || comment.content.trim() === '')) {
        await deleteComment(comment.id);
      }
    } catch (err) {
      console.error('Error deleting attachment:', err);
      await refreshTicket();
    }
  }

  // Delete comment
  async function deleteComment(commentId: number): Promise<void> {
    if (!ticket.value) return;

    try {
      // Optimistic update - use splice to preserve array reference
      if (ticket.value.commentsAndAttachments) {
        const index = ticket.value.commentsAndAttachments.findIndex(
          (c: any) => c.id === commentId,
        );
        if (index !== -1) {
          ticket.value.commentsAndAttachments.splice(index, 1);
        }
      }

      // Delete from backend
      await ticketService.deleteComment(commentId);
    } catch (err) {
      console.error('Error deleting comment:', err);
      await refreshTicket();
    }
  }

  return {
    addComment,
    deleteAttachment,
    deleteComment,
  };
}
