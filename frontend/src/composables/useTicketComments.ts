import { type Ref } from 'vue';
import ticketService from '@/services/ticketService';
import apiClient from '@/services/apiConfig';

/**
 * Composable for managing ticket comments
 */
export function useTicketComments(
  ticket: Ref<any>,
  refreshTicket: () => Promise<void>,
) {

  // Add comment
  async function addComment(data: { content: string; user_uuid: string; files: File[] }): Promise<void> {
    if (!ticket.value) return;

    // Validate input
    if (!data.content.trim() && (!data.files || data.files.length === 0)) {
      return;
    }

    // Keep content as-is (empty string if no text, allows attachment-only comments)

    // Generate a temporary negative ID for optimistic update
    const tempId = -Date.now();

    // Optimistically add to UI immediately with temp ID
    const optimisticComment = {
      id: tempId,
      content: data.content,
      user_uuid: data.user_uuid,
      createdAt: new Date().toISOString(),
      created_at: new Date().toISOString(),
      ticket_id: ticket.value.id,
      attachments: [],
      user: null,
    };

    if (ticket.value.commentsAndAttachments) {
      ticket.value.commentsAndAttachments.unshift(optimisticComment);
    } else {
      ticket.value.commentsAndAttachments = [optimisticComment];
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

      // Create comment on server
      const newComment = await ticketService.addCommentToTicket(
        ticket.value.id,
        data.content,
        attachments,
      );

      // Replace the optimistic comment with the real one from HTTP response
      // This ensures user data is always present, even if SSE is delayed
      if (ticket.value.commentsAndAttachments) {
        const tempIndex = ticket.value.commentsAndAttachments.findIndex(
          (c: any) => c.id === tempId
        );

        // Check if SSE already added the real comment
        const sseAlreadyAdded = ticket.value.commentsAndAttachments.some(
          (c: any) => c.id === newComment.id
        );

        if (tempIndex !== -1) {
          if (sseAlreadyAdded) {
            // SSE already added it, just remove the optimistic one
            ticket.value.commentsAndAttachments.splice(tempIndex, 1);
          } else {
            // Replace optimistic with real comment (includes user data)
            ticket.value.commentsAndAttachments.splice(tempIndex, 1, {
              id: newComment.id,
              content: newComment.content,
              user_uuid: newComment.user_uuid,
              createdAt: newComment.created_at || newComment.createdAt,
              created_at: newComment.created_at || newComment.createdAt,
              ticket_id: newComment.ticket_id,
              attachments: newComment.attachments || [],
              user: newComment.user,
            });
          }
        } else if (!sseAlreadyAdded) {
          // Optimistic comment was already removed (maybe by SSE race), add real one
          ticket.value.commentsAndAttachments.unshift({
            id: newComment.id,
            content: newComment.content,
            user_uuid: newComment.user_uuid,
            createdAt: newComment.created_at || newComment.createdAt,
            created_at: newComment.created_at || newComment.createdAt,
            ticket_id: newComment.ticket_id,
            attachments: newComment.attachments || [],
            user: newComment.user,
          });
        }
      }
    } catch (error) {
      console.error('Error adding comment:', error);
      // Remove optimistic comment on error
      if (ticket.value.commentsAndAttachments) {
        const index = ticket.value.commentsAndAttachments.findIndex(
          (c: any) => c.id === tempId
        );
        if (index !== -1) {
          ticket.value.commentsAndAttachments.splice(index, 1);
        }
      }
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

      // Check if this is the last attachment on an attachment-only comment
      // (no text content or just placeholder text like "Attachment added")
      const hasNoRealContent = !comment.content ||
        comment.content.trim() === '' ||
        comment.content.trim().toLowerCase() === 'attachment added';
      const isLastAttachment = comment.attachments.length === 1;
      const shouldDeleteComment = isLastAttachment && hasNoRealContent;

      if (shouldDeleteComment) {
        // Delete the entire comment (which will also delete attachments on backend)
        await deleteComment(comment.id);
      } else {
        // Just delete the attachment
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
