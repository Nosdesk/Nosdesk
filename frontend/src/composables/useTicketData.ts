import { ref, computed } from "vue";
import { useRouter } from "vue-router";
import { useRecentTicketsStore } from "@/stores/recentTickets";
import { useTitleManager } from "@/composables/useTitleManager";
import ticketService from "@/services/ticketService";
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
  function transformComments(apiComments: any[]): CommentWithAttachments[] {
    return apiComments
      .map((comment) => ({
        id: comment.id,
        content: comment.content,
        user_uuid: comment.user_uuid,
        createdAt: comment.created_at,
        created_at: comment.created_at,
        ticket_id: comment.ticket_id,
        attachments:
          comment.attachments?.map((att: any) => ({
            id: att.id,
            url: att.url,
            name: att.name,
            comment_id: att.comment_id,
          })) || [],
        user: comment.user,
      }))
      .sort(
        (a, b) =>
          new Date(b.created_at).getTime() - new Date(a.created_at).getTime(),
      );
  }

  // Transform devices from API format
  function transformDevices(apiDevices: any[]): Device[] {
    return apiDevices.map((device) => ({
      id: device.id,
      name: device.name,
      hostname: device.hostname,
      serial_number: device.serial_number,
      model: device.model,
      manufacturer: device.manufacturer,
      warranty_status: device.warranty_status,
    }));
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

      // Update title manager
      titleManager.setTicket({
        id: ticket.value.id,
        title: ticket.value.title,
      });

      // Record the ticket view on the server (updates recent tickets)
      await recentTicketsStore.recordTicketView(id);
    } catch (err) {
      console.error(`Error fetching ticket ${id}:`, err);
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
  async function updateTicketField(field: string, value: any): Promise<void> {
    if (!ticket.value) return;

    const oldValue = (ticket.value as any)[field];
    if (oldValue === value) return;

    try {
      const nowDateTime = getCurrentUTCDateTime();
      const updateData = { [field]: value, modified: nowDateTime };

      // Optimistic update - use direct mutation to preserve object reference
      // This prevents component remounts when the ticket object is updated
      (ticket.value as any)[field] = value;
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
      console.error(`Error updating ${field}:`, err);
      // Revert optimistic update on error - also use direct mutation
      (ticket.value as any)[field] = oldValue;
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
    updateRequester,
    updateAssignee,
    updateTitle,
    deleteTicket,
  };
}
