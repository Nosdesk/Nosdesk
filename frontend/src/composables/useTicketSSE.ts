import { ref, onMounted, onUnmounted, type Ref } from "vue";
import { useSSE } from "@/services/sseService";
import { useAuthStore } from "@/stores/auth";
import { useTitleManager } from "@/composables/useTitleManager";
import { useRecentTicketsStore } from "@/stores/recentTickets";
import * as deviceService from "@/services/deviceService";
import { projectService } from "@/services/projectService";

/**
 * Composable for handling SSE events for tickets
 * Uses direct mutation to preserve object references and prevent component remounts
 */
export function useTicketSSE(
  ticket: Ref<any>,
  ticketId: Ref<number | undefined>,
  selectedStatus: Ref<any>,
  selectedPriority: Ref<any>,
) {
  const {
    addEventListener,
    removeEventListener,
    isConnected,
    connect,
    disconnect,
  } = useSSE();

  const authStore = useAuthStore();
  const titleManager = useTitleManager();
  const recentTicketsStore = useRecentTicketsStore();

  const recentlyAddedCommentIds = ref<Set<number>>(new Set());

  // Highlight comment
  function highlightComment(commentId: number): void {
    recentlyAddedCommentIds.value.add(commentId);
    setTimeout(() => {
      recentlyAddedCommentIds.value.delete(commentId);
    }, 3000);
  }

  // Handle ticket updated
  function handleTicketUpdated(eventData: any): void {
    console.log(
      "%c[SSE Handler] ticket-updated received",
      "color: #f97316; font-weight: bold",
      { rawData: eventData, timestamp: new Date().toISOString() }
    );

    const data = eventData.data || eventData;
    if (!ticket.value || data.ticket_id !== ticket.value.id) {
      console.log("[SSE Handler] ticket-updated: Not for this ticket, ignoring", {
        currentTicketId: ticket.value?.id,
        eventTicketId: data.ticket_id,
      });
      return;
    }

    console.log("[SSE Handler] ticket-updated: Processing field update", {
      field: data.field,
      value: data.value,
      updatedBy: data.updated_by,
    });

    // Use direct mutation to preserve object reference - prevents component remounts
    if (data.field === "title") {
      ticket.value.title = data.value;
      titleManager.setTicket(ticket.value);
      console.log("%c[SSE Handler] ✅ ticket-updated: Title updated", "color: #22c55e; font-weight: bold");
    } else if (data.field === "status") {
      ticket.value.status = data.value;
      selectedStatus.value = data.value;
      console.log("%c[SSE Handler] ✅ ticket-updated: Status updated", "color: #22c55e; font-weight: bold");
    } else if (data.field === "priority") {
      ticket.value.priority = data.value;
      selectedPriority.value = data.value;
      console.log("%c[SSE Handler] ✅ ticket-updated: Priority updated", "color: #22c55e; font-weight: bold");
    } else if (data.field === "modified") {
      ticket.value.modified = data.value;
      console.log("%c[SSE Handler] ✅ ticket-updated: Modified timestamp updated", "color: #22c55e; font-weight: bold");
    } else if (data.field === "requester") {
      if (typeof data.value === "string") {
        ticket.value.requester = data.value || null;
        if (!data.value) {
          ticket.value.requester_user = null;
        }
      } else if (data.value?.uuid) {
        ticket.value.requester = data.value.uuid;
        ticket.value.requester_user = data.value.user_info || ticket.value.requester_user;
      }
      console.log("%c[SSE Handler] ✅ ticket-updated: Requester updated", "color: #22c55e; font-weight: bold");
    } else if (data.field === "assignee") {
      if (typeof data.value === "string") {
        ticket.value.assignee = data.value || null;
        if (!data.value) {
          ticket.value.assignee_user = null;
        }
      } else if (data.value?.uuid) {
        ticket.value.assignee = data.value.uuid;
        ticket.value.assignee_user = data.value.user_info || ticket.value.assignee_user;
      }
      console.log("%c[SSE Handler] ✅ ticket-updated: Assignee updated", "color: #22c55e; font-weight: bold");
    }

    recentTicketsStore.updateTicketData(ticket.value.id, {
      title: ticket.value.title,
      status: ticket.value.status,
      requester: ticket.value.requester,
      assignee: ticket.value.assignee,
    });
  }

  // Handle comment added
  function handleCommentAdded(data: any): void {
    const eventData = data.data || data;
    if (!ticket.value || eventData.ticket_id !== ticket.value.id) return;

    const commentData = eventData.comment;
    if (!commentData) return;

    // Check for duplicates (will catch optimistic updates)
    if (
      ticket.value.commentsAndAttachments?.find(
        (c: any) => c.id === commentData.id,
      )
    ) {
      console.log('[SSE] Skipping duplicate comment', commentData.id);
      return;
    }

    const newComment = {
      id: commentData.id,
      content: commentData.content,
      user_uuid: commentData.user_uuid || commentData.user_id,
      createdAt: commentData.createdAt || commentData.created_at,
      created_at: commentData.created_at || commentData.createdAt,
      ticket_id: commentData.ticket_id,
      attachments: commentData.attachments || [],
      user: commentData.user,
    };

    // Use direct mutation on the array to preserve object reference
    if (ticket.value.commentsAndAttachments) {
      ticket.value.commentsAndAttachments.unshift(newComment);
    } else {
      ticket.value.commentsAndAttachments = [newComment];
    }

    highlightComment(newComment.id);
  }

  // Handle comment deleted
  function handleCommentDeleted(data: any): void {
    const eventData = data.data || data;
    if (!ticket.value || eventData.ticket_id !== ticket.value.id) return;

    if (ticket.value.commentsAndAttachments) {
      const index = ticket.value.commentsAndAttachments.findIndex(
        (comment: any) => comment.id === eventData.comment_id
      );
      if (index !== -1) {
        ticket.value.commentsAndAttachments.splice(index, 1);
      }
    }
  }

  // Handle device linked
  async function handleDeviceLinked(data: any): Promise<void> {
    const eventData = data.data || data;
    if (!ticket.value || eventData.ticket_id !== ticket.value.id) return;

    try {
      const device = await deviceService.getDeviceById(eventData.device_id);

      // Check if device already exists
      const deviceExists = ticket.value.devices?.find(
        (d: any) => d.id === eventData.device_id,
      );

      if (!deviceExists) {
        if (ticket.value.devices) {
          ticket.value.devices.push(device);
        } else {
          ticket.value.devices = [device];
        }
      }
    } catch (error) {
      console.error("Error fetching linked device:", error);
    }
  }

  // Handle device unlinked
  function handleDeviceUnlinked(data: any): void {
    const eventData = data.data || data;
    if (!ticket.value || eventData.ticket_id !== ticket.value.id) return;

    if (ticket.value.devices) {
      const index = ticket.value.devices.findIndex(
        (d: any) => d.id === eventData.device_id
      );
      if (index !== -1) {
        ticket.value.devices.splice(index, 1);
      }
    }
  }

  // Handle device updated
  function handleDeviceUpdated(data: any): void {
    const eventData = data.data || data;
    if (!ticket.value?.devices) return;

    const deviceIndex = ticket.value.devices.findIndex(
      (d: any) => d.id === eventData.device_id,
    );

    if (
      deviceIndex !== -1 &&
      eventData.field &&
      eventData.value !== undefined
    ) {
      (ticket.value.devices[deviceIndex] as any)[eventData.field] = eventData.value;
    }
  }

  // Handle ticket linked
  function handleTicketLinked(data: any): void {
    const eventData = data.data || data;
    if (!ticket.value) return;

    const isSourceTicket = eventData.ticket_id === ticket.value.id;
    const isTargetTicket = eventData.linked_ticket_id === ticket.value.id;

    if (!isSourceTicket && !isTargetTicket) return;

    const linkedTicketId = isSourceTicket
      ? eventData.linked_ticket_id
      : eventData.ticket_id;

    // Check if already linked
    const alreadyLinked = ticket.value.linkedTickets?.includes(linkedTicketId);

    if (!alreadyLinked) {
      if (ticket.value.linkedTickets) {
        ticket.value.linkedTickets.push(linkedTicketId);
      } else {
        ticket.value.linkedTickets = [linkedTicketId];
      }
    }
  }

  // Handle ticket unlinked
  function handleTicketUnlinked(data: any): void {
    console.log(
      "%c[SSE Handler] ticket-unlinked received",
      "color: #f97316; font-weight: bold",
      { rawData: data, timestamp: new Date().toISOString() }
    );

    const eventData = data.data || data;
    if (!ticket.value) {
      console.log("[SSE Handler] ticket-unlinked: No ticket.value, ignoring");
      return;
    }

    const isSourceTicket = eventData.ticket_id === ticket.value.id;
    const isTargetTicket = eventData.linked_ticket_id === ticket.value.id;

    console.log("[SSE Handler] ticket-unlinked: Checking ticket match", {
      currentTicketId: ticket.value.id,
      eventTicketId: eventData.ticket_id,
      eventLinkedTicketId: eventData.linked_ticket_id,
      isSourceTicket,
      isTargetTicket,
      currentLinkedTickets: ticket.value.linkedTickets,
    });

    if (!isSourceTicket && !isTargetTicket) {
      console.log(
        "[SSE Handler] ticket-unlinked: Not for this ticket, ignoring"
      );
      return;
    }

    const linkedTicketIdToRemove = isSourceTicket
      ? eventData.linked_ticket_id
      : eventData.ticket_id;

    console.log("[SSE Handler] ticket-unlinked: Removing linked ticket", {
      linkedTicketIdToRemove,
      beforeLength: ticket.value.linkedTickets?.length || 0,
    });

    if (ticket.value.linkedTickets) {
      const index = ticket.value.linkedTickets.indexOf(linkedTicketIdToRemove);
      if (index !== -1) {
        ticket.value.linkedTickets.splice(index, 1);
        console.log(
          "%c[SSE Handler] ✅ ticket-unlinked: Successfully removed",
          "color: #22c55e; font-weight: bold",
          {
            removedTicketId: linkedTicketIdToRemove,
            afterLength: ticket.value.linkedTickets.length,
            remainingLinkedTickets: ticket.value.linkedTickets,
          }
        );
      } else {
        console.warn(
          "[SSE Handler] ticket-unlinked: Ticket not found in linkedTickets array",
          { searchedFor: linkedTicketIdToRemove }
        );
      }
    } else {
      console.warn("[SSE Handler] ticket-unlinked: No linkedTickets array");
    }
  }

  // Handle project assigned
  async function handleProjectAssigned(data: any): Promise<void> {
    console.log(
      "%c[SSE Handler] project-assigned received",
      "color: #f97316; font-weight: bold",
      { rawData: data, timestamp: new Date().toISOString() }
    );

    const eventData = data.data || data;
    if (!ticket.value || eventData.ticket_id !== ticket.value.id) {
      console.log(
        "[SSE Handler] project-assigned: Not for this ticket, ignoring",
        {
          currentTicketId: ticket.value?.id,
          eventTicketId: eventData.ticket_id,
        }
      );
      return;
    }

    const projectId = eventData.project_id;

    console.log("[SSE Handler] project-assigned: Adding to projects array", {
      projectId,
      currentProjects: ticket.value.projects,
    });

    // Check if already in the array
    const alreadyAssigned = ticket.value.projects?.includes(projectId);
    if (!alreadyAssigned) {
      if (ticket.value.projects) {
        ticket.value.projects.push(projectId);
      } else {
        ticket.value.projects = [projectId];
      }
      console.log(
        "%c[SSE Handler] ✅ project-assigned: Successfully assigned",
        "color: #22c55e; font-weight: bold",
        {
          projectId,
          updatedProjects: ticket.value.projects
        }
      );
    } else {
      console.log(
        "[SSE Handler] project-assigned: Already assigned, skipping",
        { projectId }
      );
    }
  }

  // Handle project unassigned
  function handleProjectUnassigned(data: any): void {
    console.log(
      "%c[SSE Handler] project-unassigned received",
      "color: #f97316; font-weight: bold",
      { rawData: data, timestamp: new Date().toISOString() }
    );

    const eventData = data.data || data;
    if (!ticket.value || eventData.ticket_id !== ticket.value.id) {
      console.log(
        "[SSE Handler] project-unassigned: Not for this ticket, ignoring",
        {
          currentTicketId: ticket.value?.id,
          eventTicketId: eventData.ticket_id,
        }
      );
      return;
    }

    const projectId = eventData.project_id;

    console.log("[SSE Handler] project-unassigned: Removing from projects array", {
      projectId,
      currentProjects: ticket.value.projects,
      beforeLength: ticket.value.projects?.length || 0,
    });

    if (ticket.value.projects) {
      const index = ticket.value.projects.indexOf(projectId);
      if (index !== -1) {
        ticket.value.projects.splice(index, 1);
        console.log(
          "%c[SSE Handler] ✅ project-unassigned: Successfully unassigned",
          "color: #22c55e; font-weight: bold",
          {
            unassignedProjectId: projectId,
            afterLength: ticket.value.projects.length,
            remainingProjects: ticket.value.projects,
          }
        );
      } else {
        console.warn(
          "[SSE Handler] project-unassigned: Project not found in array",
          { searchedFor: projectId, currentProjects: ticket.value.projects }
        );
      }
    } else {
      console.warn("[SSE Handler] project-unassigned: No projects array");
    }
  }

  // Event handler configuration - DRY principle
  const eventHandlers = {
    "ticket-updated": handleTicketUpdated,
    "comment-added": handleCommentAdded,
    "comment-deleted": handleCommentDeleted,
    "device-linked": handleDeviceLinked,
    "device-unlinked": handleDeviceUnlinked,
    "device-updated": handleDeviceUpdated,
    "ticket-linked": handleTicketLinked,
    "ticket-unlinked": handleTicketUnlinked,
    "project-assigned": handleProjectAssigned,
    "project-unassigned": handleProjectUnassigned,
  } as const;

  // Setup event listeners
  function setupEventListeners(): void {
    console.log(
      "%c[SSE Setup] Registering event listeners",
      "color: #06b6d4; font-weight: bold",
      { eventTypes: Object.keys(eventHandlers) }
    );
    Object.entries(eventHandlers).forEach(([event, handler]) => {
      addEventListener(event as any, handler);
      console.log(`[SSE Setup] ✓ Registered listener for: ${event}`);
    });
  }

  // Remove event listeners
  function cleanupEventListeners(): void {
    Object.entries(eventHandlers).forEach(([event, handler]) => {
      removeEventListener(event as any, handler);
    });
  }

  // Auto-setup on mount - connect immediately for real-time updates
  onMounted(async () => {
    console.log(
      "%c[SSE Setup] Mounting ticket SSE handlers",
      "color: #06b6d4; font-weight: bold; font-size: 14px",
      { ticketId: ticketId.value, isAuthenticated: authStore.isAuthenticated }
    );
    setupEventListeners();
    // Connect immediately - no delay needed
    if (authStore.isAuthenticated && ticketId.value) {
      console.log("[SSE Setup] Connecting to SSE...", { ticketId: ticketId.value });
      await connect(ticketId.value);
      console.log("[SSE Setup] SSE connection initiated");
    } else {
      console.warn("[SSE Setup] Cannot connect - missing auth or ticket ID", {
        isAuthenticated: authStore.isAuthenticated,
        ticketId: ticketId.value,
      });
    }
  });

  // Auto-cleanup on unmount
  onUnmounted(() => {
    cleanupEventListeners();
    disconnect();
  });

  return {
    isConnected,
    recentlyAddedCommentIds,
  };
}
