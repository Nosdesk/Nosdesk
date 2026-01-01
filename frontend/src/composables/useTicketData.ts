import { ref, computed } from "vue";
import { useRouter } from "vue-router";
import { useRecentTicketsStore } from "@/stores/recentTickets";
import { useTitleManager } from "@/composables/useTitleManager";
import ticketService from "@/services/ticketService";
import { logger } from "@/utils/logger";
import { formatDateTime, getCurrentUTCDateTime } from "@/utils/dateUtils";
import type { TicketStatus, TicketPriority } from "@/constants/ticketOptions";
import type { UserInfo } from '@/types/user';
import type { Ticket, Device } from '@/types/ticket';
import type { CommentWithAttachments, Attachment } from '@/types/comment';

// Local type extending the canonical Ticket type with UI-specific fields
interface LocalTicket extends Ticket {
  commentsAndAttachments?: CommentWithAttachments[];
}

/**
 * Composable for managing ticket data and state
 */
export function useTicketData() {
  const router = useRouter();
  const recentTicketsStore = useRecentTicketsStore();
  const titleManager = useTitleManager();

  // State
  const ticket = ref<LocalTicket | null>(null);
  const loading = ref(true);
  const error = ref<string | null>(null);
  const selectedStatus = ref<TicketStatus>("open");
  const selectedPriority = ref<TicketPriority>("low");
  const selectedCategory = ref<number | null>(null);

  // Computed
  const formattedCreatedDate = computed(() =>
    formatDateTime(ticket.value?.created),
  );
  const formattedModifiedDate = computed(() =>
    formatDateTime(ticket.value?.modified),
  );
  const comments = computed(() => ticket.value?.commentsAndAttachments || []);
  const devices = computed(() => ticket.value?.devices || []);

  // Transform comments from API format
  // Uses spread to preserve all fields (including future additions like transcription)
  // Only explicitly maps fields that need transformation
  function transformComments(apiComments: CommentWithAttachments[]): CommentWithAttachments[] {
    return apiComments
      .map((comment) => ({
        ...comment,
        createdAt: comment.created_at, // Add camelCase alias for consistency
        attachments: comment.attachments || [],
      }))
      .sort(
        (a, b) =>
          new Date(b.created_at).getTime() - new Date(a.created_at).getTime(),
      );
  }

  // Transform devices from API format
  // Uses spread to preserve all fields automatically
  function transformDevices(apiDevices: Device[]): Device[] {
    return apiDevices.map((device) => ({ ...device }));
  }

  // Fetch ticket
  async function fetchTicket(
    ticketId: string | string[],
    fromRecent = false,
  ): Promise<void> {
    const id = Number(ticketId);
    loading.value = true;
    error.value = null;

    try {
      const fetchedTicket = await ticketService.getTicketById(id);

      if (!fetchedTicket) {
        router.push("/404");
        return;
      }

      // Transform data
      const commentsAndAttachments = transformComments(
        fetchedTicket.comments || [],
      );
      const transformedDevices = transformDevices(fetchedTicket.devices || []);

      // Extract project IDs from projects array
      const projectIds = fetchedTicket.projects?.map(p => String(p.id)) || [];

      // Update ticket
      ticket.value = {
        ...fetchedTicket,
        projects: projectIds,
        linkedTickets:
          fetchedTicket.linked_tickets || fetchedTicket.linkedTickets || [],
        devices: transformedDevices,
        commentsAndAttachments,
      } as LocalTicket;

      // Update UI state
      selectedStatus.value = ticket.value.status;
      selectedPriority.value = ticket.value.priority;
      selectedCategory.value = ticket.value.category_id || null;

      // Update title manager
      titleManager.setTicket({
        id: ticket.value.id,
        title: ticket.value.title,
      });

      // Record the ticket view on the server (updates recent tickets)
      await recentTicketsStore.recordTicketView(id);
    } catch (err) {
      logger.error(`Error fetching ticket ${id}`, { error: err });
      error.value = "Failed to load ticket. Please try again later.";
    } finally {
      loading.value = false;
    }
  }

  // Refresh ticket
  async function refreshTicket(): Promise<void> {
    if (ticket.value) {
      await fetchTicket(String(ticket.value.id));
    }
  }

  // Update ticket field
  async function updateTicketField<K extends keyof LocalTicket>(field: K, value: LocalTicket[K]): Promise<void> {
    if (!ticket.value) return;

    const oldValue = ticket.value[field];
    if (oldValue === value) return;

    try {
      const nowDateTime = getCurrentUTCDateTime();
      const updateData = { [field]: value, modified: nowDateTime };

      // Optimistic update - use direct mutation to preserve object reference
      // This prevents component remounts when the ticket object is updated
      ticket.value[field] = value;
      ticket.value.modified = nowDateTime;

      // Clear user objects when clearing requester/assignee
      if (field === "requester" && !value) {
        ticket.value.requester_user = undefined;
      }
      if (field === "assignee" && !value) {
        ticket.value.assignee_user = undefined;
      }

      // Update UI-specific refs
      if (field === "status") selectedStatus.value = value;
      if (field === "priority") selectedPriority.value = value;

      // Update stores for consistent state
      if (["title", "status", "requester", "assignee"].includes(field)) {
        recentTicketsStore.updateTicketData(ticket.value.id, {
          [field]: value,
          modified: nowDateTime,
        });
      }

      if (field === "title") {
        titleManager.setTicket({ id: ticket.value.id, title: value });
      }

      // Send update to backend - SSE will broadcast to other clients
      const response = await ticketService.updateTicket(ticket.value.id, updateData);

      // Update user objects from backend response to keep UI in sync
      if (response && ticket.value) {
        if (field === "requester" && response.requester_user) {
          ticket.value.requester_user = response.requester_user;
        }
        if (field === "assignee" && response.assignee_user) {
          ticket.value.assignee_user = response.assignee_user;
        }
      }
    } catch (err) {
      logger.error(`Error updating ticket field: ${field}`, { error: err, field });
      // Revert optimistic update on error - also use direct mutation
      ticket.value[field] = oldValue;
      if (field === "status") selectedStatus.value = oldValue;
      if (field === "priority") selectedPriority.value = oldValue;
      throw err;
    }
  }

  // Update status
  async function updateStatus(newStatus: string): Promise<void> {
    await updateTicketField("status", newStatus);
  }

  // Update priority
  async function updatePriority(newPriority: string): Promise<void> {
    await updateTicketField("priority", newPriority);
  }

  // Update requester
  async function updateRequester(newRequester: string): Promise<void> {
    await updateTicketField("requester", newRequester);
  }

  // Update assignee
  async function updateAssignee(newAssignee: string): Promise<void> {
    await updateTicketField("assignee", newAssignee);
  }

  // Update title
  async function updateTitle(newTitle: string): Promise<void> {
    await updateTicketField("title", newTitle);
  }

  // Update category
  async function updateCategory(newCategory: string): Promise<void> {
    const categoryId = newCategory ? parseInt(newCategory, 10) : null;
    selectedCategory.value = categoryId;
    await updateTicketField("category_id", categoryId);
  }

  // Delete ticket
  async function deleteTicket(): Promise<void> {
    if (!ticket.value) return;

    const ticketId = ticket.value.id;
    await ticketService.deleteTicket(ticketId);
    // Recent tickets will be automatically updated when the list is refreshed
    router.push("/tickets");
  }

  return {
    // State
    ticket,
    loading,
    error,
    selectedStatus,
    selectedPriority,
    selectedCategory,

    // Computed
    formattedCreatedDate,
    formattedModifiedDate,
    comments,
    devices,

    // Methods
    fetchTicket,
    refreshTicket,
    updateStatus,
    updatePriority,
    updateCategory,
    updateRequester,
    updateAssignee,
    updateTitle,
    deleteTicket,
  };
}
